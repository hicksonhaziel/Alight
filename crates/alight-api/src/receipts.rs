use super::*;

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
    let _permit = s.work()?;
    let explicit = q.capture.is_some();
    let capture = match q.capture {
        Some(hash) => s.store.wallet_capture(s.source, &wallet, &hash).await,
        None => s.store.latest_wallet_capture(s.source, &wallet).await,
    }
    .map_err(|_| s.unavailable())?
    .ok_or_else(|| {
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
            "No saved history for this wallet and source; live provider history is unavailable",
        )
    })?;
    if utc(&capture.through_utc).map_err(|_| s.unavailable())?
        > utc(&s.now().await).map_err(|_| s.unavailable())?
    {
        return Err(s.invalid());
    }
    let request = WalletReceiptRequest {
        region: q.region.unwrap_or(s.region.clone()),
        target_p: q.target_p.unwrap_or(0.9),
        horizon_slots: q.horizon_slots.unwrap_or(2),
        max_curve_age_s: q.max_curve_age_s.unwrap_or(300),
    };
    Ok(Json(
        alight_tape::receipts::evaluate(&s.store, &capture, &request)
            .await
            .map_err(|e| match e {
                alight_tape::receipts::ReceiptError::Store(_) => s.unavailable(),
                _ => s.invalid(),
            })?,
    ))
}
