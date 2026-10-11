use super::*;
use alight_ingest::data_api_history::{DataApiCapture, DataApiHistoryProvider};

pub(super) struct LiveHistory {
    provider: Arc<dyn DataApiHistoryProvider>,
    cache: AsyncMutex<HistoryCache>,
}
struct HistoryCache {
    entries: BTreeMap<String, (Instant, DataApiCapture)>,
    next_fetch: Instant,
}
impl LiveHistory {
    pub(super) fn new(provider: Arc<dyn DataApiHistoryProvider>) -> Self {
        Self {
            provider,
            cache: AsyncMutex::new(HistoryCache {
                entries: BTreeMap::new(),
                next_fetch: Instant::now(),
            }),
        }
    }
    async fn get(
        &self,
        wallet: &str,
        pin: Option<&str>,
        s: &ApiState,
    ) -> Result<Option<DataApiCapture>, ApiError> {
        let mut cache = self.cache.try_lock().map_err(|_| {
            s.error(
                StatusCode::TOO_MANY_REQUESTS,
                "HISTORY_BUSY",
                "A bounded wallet history fetch is in progress",
            )
        })?;
        cache
            .entries
            .retain(|_, (at, _)| at.elapsed() < Duration::from_secs(300));
        if let Some((_, entry)) = cache.entries.get(wallet)
            && (pin.is_none()
                || pin
                    == Some(
                        alight_store::content_hash(&live_capture(entry))
                            .map_err(|_| s.unavailable())?
                            .as_str(),
                    ))
        {
            return Ok(Some(entry.clone()));
        }
        if pin.is_some() {
            return Ok(None);
        }
        if Instant::now() < cache.next_fetch {
            return Err(s.error(
                StatusCode::TOO_MANY_REQUESTS,
                "HISTORY_RATE_LIMITED",
                "Wallet history fetches are limited to one every 30 seconds",
            ));
        }
        cache.next_fetch = Instant::now() + Duration::from_secs(30);
        let entry = tokio::time::timeout(Duration::from_secs(25), self.provider.fetch(wallet))
            .await
            .map_err(|_| {
                s.error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "HISTORY_UNAVAILABLE",
                    "Wallet history fetch timed out",
                )
            })?
            .map_err(|_| {
                s.error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "HISTORY_UNAVAILABLE",
                    "Wallet history requires working Data API and RPC read access",
                )
            })?;
        if entry.history.capture.wallet != wallet {
            return Err(s.unavailable());
        }
        alight_tape::receipts::validate(&entry.history.capture).map_err(|_| s.unavailable())?;
        if cache.entries.len() >= 16
            && let Some(oldest) = cache
                .entries
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(wallet, _)| wallet.clone())
        {
            cache.entries.remove(&oldest);
        }
        cache
            .entries
            .insert(wallet.into(), (Instant::now(), entry.clone()));
        Ok(Some(entry))
    }
}
fn live_capture(entry: &DataApiCapture) -> WalletHistoryCapture {
    let mut capture = entry.history.capture.clone();
    capture.source = Source::Live;
    for row in &mut capture.rows {
        row.transaction.source = Source::Live;
    }
    capture
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReceiptQuery {
    capture: Option<String>,
    region: Option<String>,
    target_p: Option<f64>,
    horizon_slots: Option<u32>,
    max_curve_age_s: Option<u32>,
}
pub(super) async fn receipt(
    State(s): State<ApiState>,
    Path(wallet): Path<String>,
    query: Result<Query<ReceiptQuery>, QueryRejection>,
) -> Result<Json<WalletReceipt>, ApiError> {
    let Query(q) = query.map_err(|_| s.invalid())?;
    if !(32..=44).contains(&wallet.len())
        || !wallet
            .bytes()
            .all(|b| b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz".contains(&b))
    {
        return Err(s.invalid());
    }
    if q.capture.as_ref().is_some_and(|hash| {
        hash.len() != 71
            || !hash.starts_with("sha256:")
            || !hash[7..].bytes().all(|b| b.is_ascii_hexdigit())
    }) {
        return Err(s.invalid());
    }
    let request = WalletReceiptRequest {
        region: q.region.unwrap_or(s.region.clone()),
        target_p: q.target_p.unwrap_or(0.9),
        horizon_slots: q.horizon_slots.unwrap_or(2),
        max_curve_age_s: q.max_curve_age_s.unwrap_or(300),
    };
    alight_tape::receipts::validate_request(&request).map_err(|_| s.invalid())?;
    // Reject invalid decoded keys before spending provider bandwidth.
    let now = s.now().await;
    alight_tape::receipts::validate(&WalletHistoryCapture {
        schema_version: 1,
        source: Source::Replay,
        wallet: wallet.clone(),
        from_utc: now.clone(),
        through_utc: now,
        tip_recipients: vec![],
        rows: vec![],
    })
    .map_err(|_| s.invalid())?;
    let _permit = s.work()?;
    let explicit = q.capture.is_some();
    let saved = match &q.capture {
        Some(hash) => s.store.wallet_capture(s.source, &wallet, hash).await,
        None => s.store.latest_wallet_capture(s.source, &wallet).await,
    }
    .map_err(|_| s.unavailable())?;
    let fetched = if saved.is_none() {
        match &s.wallet_history {
            Some(provider) => provider.get(&wallet, q.capture.as_deref(), &s).await?,
            None => None,
        }
    } else {
        None
    };
    let capture = saved.or_else(|| fetched.as_ref().map(live_capture)).ok_or_else(|| {
        s.error(
            if explicit {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            },
            if explicit {
                "HISTORY_NOT_FOUND"
            } else {
                "HISTORY_UNAVAILABLE"
            },
            "No saved or cached history for this wallet and source; live reads require Data API and RPC access",
        )
    })?;
    if utc(&capture.through_utc).map_err(|_| s.unavailable())?
        > utc(&s.now().await).map_err(|_| s.unavailable())?
    {
        return Err(s.invalid());
    }
    let report = if fetched.is_some() {
        alight_tape::receipts::evaluate_live_history(&s.store, &capture, &request).await
    } else {
        alight_tape::receipts::evaluate(&s.store, &capture, &request).await
    };
    let mut report = report.map_err(|e| match e {
        alight_tape::receipts::ReceiptError::Store(_) => s.unavailable(),
        _ => s.invalid(),
    })?;
    if let Some(entry) = fetched {
        report.population = "partial_data_api_indexed_finalized_fee_payer_transactions".into();
        // The index selects executed swaps; it cannot estimate the wallet's failed share.
        report.visible_failed_share = None;
        report.limits.insert(0, format!("Solami Data API trade index plus finalized RPC execution evidence, fetched {}. At most 8 trade rows in the preceding 24 hours; no pagination. LIVE describes this provider read, not evidence of a recent transaction.", entry.fetched_at_utc));
        report.limits.push(format!("Coverage: {} trade rows, {} unique signatures; {} outside the window, {} unavailable finalized transactions, {} paid by another account. No missing record becomes zero fees.", entry.trade_rows, entry.unique_signatures, entry.history.outside_window, entry.history.unavailable, entry.history.not_fee_payer));
        report.limits.push(format!("Provider venue-tip registry total: {} SOL over its rolling 24-hour window. This aggregate has separate registry/coverage, excludes base and priority fees, and is not added to transaction subtotals or allocated across trades.", entry.venue_tips_sol));
        report.limits.push("Transaction tips cover only Solami's public tip-address list; payments to other recipients are outside that coverage. The trade index does not establish failed executions, dropped submissions, transport route, workload or historical frontier context.".into());
    }
    Ok(Json(report))
}
