//! Bounded Solami trade index plus finalized RPC execution evidence. Never signs or sends.
use crate::{
    Config, HttpProbe, MAINNET_GENESIS, ProbeError,
    wallet_history::{HistoryCapture, HistoryError},
};
use alight_types::*;
use chrono::{DateTime, SecondsFormat, Utc};
use futures_util::{StreamExt, TryStreamExt};
use serde_json::{Value, json};
use std::{collections::BTreeMap, future::Future, pin::Pin, time::Duration};

#[derive(Clone)]
pub struct DataApiCapture {
    pub history: HistoryCapture,
    pub trade_rows: u32,
    pub unique_signatures: u32,
    /// Rolling venue registry total in SOL, not transaction/network fees or a receipt subtotal.
    pub venue_tips_sol: String,
    pub fetched_at_utc: String,
}

pub type HistoryFuture<'a> =
    Pin<Box<dyn Future<Output = Result<DataApiCapture, HistoryError>> + Send + 'a>>;
/// Read-only provider boundary; callers must retain partial coverage and source provenance.
pub trait DataApiHistoryProvider: Send + Sync {
    fn fetch<'a>(&'a self, wallet: &'a str) -> HistoryFuture<'a>;
}

/// Contains only read credentials. Intentionally has no Debug or Serialize implementation.
pub struct DataApiHistory {
    config: Config,
    probe: HttpProbe,
    base: String,
    token: String,
}
impl DataApiHistory {
    pub fn new(config: &Config) -> Result<Self, HistoryError> {
        let base = config
            .get("SOLAMI_DATA_API_URL")
            .unwrap_or("https://api.solami.dev");
        let url = reqwest::Url::parse(base).map_err(|_| ProbeError::Configuration)?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || !matches!(url.path(), "" | "/")
        {
            return Err(ProbeError::Configuration.into());
        }
        let token = config
            .get("SOLAMI_DATA_API_TOKEN")
            .or(config.get("SOLAMI_API_KEY"))
            .ok_or(ProbeError::Configuration)?
            .to_owned();
        if !(16..=256).contains(&token.len()) || !token.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(ProbeError::Configuration.into());
        }
        let rpc = config
            .get("SOLAMI_RPC_URL")
            .ok_or(ProbeError::Configuration)?;
        crate::https_url(rpc)?;
        let values = ["SOLAMI_RPC_URL", "SOLAMI_RPC_TOKEN"]
            .into_iter()
            .filter_map(|k| config.get(k).map(|v| (k.into(), v.into())))
            .collect();
        Ok(Self {
            config: Config { values },
            probe: HttpProbe::new()?,
            base: base.trim_end_matches('/').into(),
            token,
        })
    }

    /// One trade page, one rolling 24-hour venue summary, genesis, and at most `limit`
    /// finalized transaction reads. Optional public tip discovery adds one request.
    /// UTC window <=24 hours; limit 1–16; overall deadline 25 seconds; no pagination/retries.
    pub async fn capture(
        &self,
        wallet: &str,
        from: &str,
        through: &str,
        recipients: Option<Vec<String>>,
        limit: u32,
    ) -> Result<DataApiCapture, HistoryError> {
        let envelope = WalletHistoryCapture {
            schema_version: 1,
            source: Source::Replay,
            wallet: wallet.into(),
            from_utc: from.into(),
            through_utc: through.into(),
            tip_recipients: recipients.clone().unwrap_or_default(),
            rows: vec![],
        };
        alight_tape::receipts::validate(&envelope).map_err(|_| HistoryError::Scope)?;
        let through_at = DateTime::parse_from_rfc3339(through).map_err(|_| HistoryError::Scope)?;
        if !(1..=16).contains(&limit) || through_at > Utc::now() {
            return Err(HistoryError::Scope);
        }
        tokio::time::timeout(
            Duration::from_secs(25),
            self.capture_inner(envelope, recipients, limit),
        )
        .await
        .map_err(|_| ProbeError::Network)?
    }
    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value, HistoryError> {
        let mut url = reqwest::Url::parse(&format!("{}{path}", self.base))
            .map_err(|_| ProbeError::Configuration)?;
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        Ok(self.probe.get(url.as_str(), Some(&self.token)).await?)
    }
    async fn capture_inner(
        &self,
        mut envelope: WalletHistoryCapture,
        recipients: Option<Vec<String>>,
        limit: u32,
    ) -> Result<DataApiCapture, HistoryError> {
        if self
            .probe
            .rpc(&self.config, "getGenesisHash", json!([]))
            .await?
            .as_str()
            != Some(MAINNET_GENESIS)
        {
            return Err(ProbeError::WrongCluster.into());
        }
        let through =
            DateTime::parse_from_rfc3339(&envelope.through_utc).map_err(|_| HistoryError::Scope)?;
        let page = self
            .get(
                "/data/wallet/trades",
                &[
                    ("chain", "solana".into()),
                    ("address", envelope.wallet.clone()),
                    ("before_time", (through.timestamp() + 1).to_string()),
                    ("limit", limit.to_string()),
                ],
            )
            .await?;
        let trades = trade_index(&page, &envelope.wallet, limit)?;
        let fees = self
            .get(
                "/data/wallet/fees",
                &[
                    ("chain", "solana".into()),
                    ("address", envelope.wallet.clone()),
                    ("window", "86400".into()),
                ],
            )
            .await?;
        let venue_tips_sol = venue_tips(&fees, &envelope.wallet)?;
        let mut requests = 3;
        if recipients.is_none() {
            let value = self
                .probe
                .get(&format!("{}/onchain/tip-addresses", self.base), None)
                .await?;
            let keys: Vec<String> =
                serde_json::from_value(value).map_err(|_| ProbeError::Format)?;
            envelope.tip_recipients = keys;
            alight_tape::receipts::validate(&envelope).map_err(|_| ProbeError::Format)?;
            requests += 1;
        }
        let mut history = HistoryCapture {
            capture: envelope,
            provider_requests: requests,
            address_signatures: trades.len() as u32,
            outside_window: 0,
            not_fee_payer: 0,
            unavailable: 0,
        };
        let from = DateTime::parse_from_rfc3339(&history.capture.from_utc)
            .map_err(|_| HistoryError::Scope)?
            .timestamp();
        let selected: Vec<_> = trades
            .into_iter()
            .filter(|(_, (.., time))| {
                if *time < from || *time > through.timestamp() {
                    history.outside_window += 1;
                    false
                } else {
                    true
                }
            })
            .collect();
        let responses: Vec<_> = futures_util::stream::iter(selected).map(|(signature, (slot,time))| async move {
            let value = self.probe.rpc(&self.config, "getTransaction", json!([signature,{"commitment":"finalized","encoding":"json","maxSupportedTransactionVersion":0}])).await?;
            Ok::<_, HistoryError>((signature,slot,time,value))
        }).buffered(4).try_collect().await?;
        history.provider_requests += responses.len() as u32;
        let fetched_at_utc = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        for (signature, slot, time, value) in responses {
            let Some(row) = normalize(&value, &signature, slot, time, &fetched_at_utc)? else {
                history.unavailable += 1;
                continue;
            };
            if row.transaction.account_keys.first() != Some(&history.capture.wallet) {
                history.not_fee_payer += 1;
                continue;
            }
            history.capture.rows.push(row);
        }
        alight_tape::receipts::validate(&history.capture).map_err(|_| HistoryError::Transaction)?;
        Ok(DataApiCapture {
            trade_rows: page.as_array().ok_or(ProbeError::Format)?.len() as u32,
            unique_signatures: history.address_signatures,
            history,
            venue_tips_sol,
            fetched_at_utc,
        })
    }
}
impl DataApiHistoryProvider for DataApiHistory {
    fn fetch<'a>(&'a self, wallet: &'a str) -> HistoryFuture<'a> {
        Box::pin(async move {
            let through = Utc::now();
            self.capture(
                wallet,
                &(through - chrono::Duration::hours(24)).to_rfc3339_opts(SecondsFormat::Secs, true),
                &through.to_rfc3339_opts(SecondsFormat::Secs, true),
                None,
                8,
            )
            .await
        })
    }
}

fn key(value: &str, size: usize) -> bool {
    value.len() <= 88
        && bs58::decode(value)
            .into_vec()
            .is_ok_and(|v| v.len() == size)
}
fn trade_index(
    page: &Value,
    wallet: &str,
    limit: u32,
) -> Result<BTreeMap<String, (u64, i64)>, HistoryError> {
    let rows = page.as_array().ok_or(ProbeError::Format)?;
    if rows.len() > limit as usize {
        return Err(ProbeError::Size.into());
    }
    let mut unique = BTreeMap::new();
    for row in rows {
        let sig = row["signature"]
            .as_str()
            .filter(|s| key(s, 64))
            .ok_or(ProbeError::Format)?;
        let slot = row["slot"].as_u64().ok_or(ProbeError::Format)?;
        let at = row["block_time"]
            .as_i64()
            .filter(|t| DateTime::from_timestamp(*t, 0).is_some())
            .ok_or(ProbeError::Format)?;
        if row["trader"].as_str() != Some(wallet) {
            return Err(HistoryError::Conflict);
        }
        if unique
            .insert(sig.to_owned(), (slot, at))
            .is_some_and(|old| old != (slot, at))
        {
            return Err(HistoryError::Conflict);
        }
    }
    Ok(unique)
}
fn venue_tips(value: &Value, wallet: &str) -> Result<String, HistoryError> {
    if value["wallet"].as_str() != Some(wallet) || !value["venues"].is_object() {
        return Err(HistoryError::Conflict);
    }
    let sol = value["total_sol"].as_str().ok_or(ProbeError::Format)?;
    let mut pieces = sol.split('.');
    let whole = pieces.next().ok_or(ProbeError::Format)?;
    let fraction = pieces.next();
    if sol.len() > 64
        || whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || pieces.next().is_some()
        || fraction.is_some_and(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(ProbeError::Format.into());
    }
    Ok(sol.into())
}
fn normalize(
    value: &Value,
    signature: &str,
    slot: u64,
    time: i64,
    fetched: &str,
) -> Result<Option<WalletHistoryRow>, HistoryError> {
    let received = ReceiveTime {
        clock_id: "wallet-data-api-rpc".into(),
        mono_ns: 0,
        wall_utc: fetched.into(),
    };
    let row = alight_tape::rpc_history::transaction(value, received)
        .map_err(|_| HistoryError::Transaction)?;
    if let Some(row) = &row {
        let at = DateTime::from_timestamp(time, 0)
            .ok_or(ProbeError::Format)?
            .to_rfc3339_opts(SecondsFormat::Secs, true);
        if row.transaction.signature != signature
            || row.transaction.slot != slot
            || row.chain_time_utc.as_deref() != Some(at.as_str())
            || !row.transaction.success
        {
            return Err(HistoryError::Conflict);
        }
    }
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../../data/fixtures/data_api_wallet_history.json"
        ))
        .expect("recording")
    }
    #[test]
    fn recorded_trade_index_deduplicates_swaps_and_rpc_is_authoritative_for_fees() {
        let f = fixture();
        let wallet = f["wallet"].as_str().expect("wallet");
        let index = trade_index(&f["trades"], wallet, 16).expect("index");
        assert_eq!(index.len(), 2);
        let mut duplicate = f["trades"].clone();
        duplicate
            .as_array_mut()
            .expect("rows")
            .push(f["trades"][0].clone());
        assert_eq!(
            trade_index(&duplicate, wallet, 16)
                .expect("deduplicate")
                .len(),
            2
        );
        duplicate[2]["slot"] = json!(1);
        assert!(matches!(
            trade_index(&duplicate, wallet, 16),
            Err(HistoryError::Conflict)
        ));
        assert_eq!(
            venue_tips(&f["venue_fees"], wallet).expect("venue total"),
            "0"
        );
        for tx in f["finalized_transactions"]
            .as_array()
            .expect("transactions")
        {
            let signature = tx["signature"].as_str().expect("sig");
            let (slot, time) = index[signature];
            let row = normalize(
                &tx["result"],
                signature,
                slot,
                time,
                f["captured_at_utc"].as_str().expect("time"),
            )
            .expect("normalize")
            .expect("available");
            assert_eq!(
                row.transaction.fee_lamports,
                tx["result"]["meta"]["fee"].as_u64().expect("RPC fee")
            );
            assert_eq!(row.transaction.source, Source::Replay);
            assert!(row.route.is_none() && row.size_class.is_none() && row.regime_id.is_none());
            assert!(
                normalize(
                    &tx["result"],
                    signature,
                    slot + 1,
                    time,
                    f["captured_at_utc"].as_str().expect("time")
                )
                .is_err()
            );
            assert!(
                normalize(
                    &tx["result"],
                    signature,
                    slot,
                    time + 1,
                    f["captured_at_utc"].as_str().expect("time")
                )
                .is_err()
            );
            let mut bad = tx["result"].clone();
            bad["meta"]["err"] = json!({"InstructionError":[0,"Custom"]});
            assert!(matches!(
                normalize(
                    &bad,
                    signature,
                    slot,
                    time,
                    f["captured_at_utc"].as_str().expect("time")
                ),
                Err(HistoryError::Conflict)
            ));
            assert!(
                normalize(
                    &Value::Null,
                    signature,
                    slot,
                    time,
                    f["captured_at_utc"].as_str().expect("time")
                )
                .expect("unavailable")
                .is_none()
            );
        }
    }
    #[test]
    fn rejects_wrong_wallet_malformed_amounts_and_excessive_pages_without_echoing_payloads() {
        let f = fixture();
        let wallet = f["wallet"].as_str().expect("wallet");
        assert!(trade_index(&f["trades"], "11111111111111111111111111111111", 16).is_err());
        assert!(trade_index(&f["trades"], wallet, 1).is_err());
        for bad in [json!(-1), json!(1.5), json!("private-upstream-text")] {
            let mut rows = f["trades"].clone();
            rows[0]["slot"] = bad;
            assert!(
                !trade_index(&rows, wallet, 16)
                    .expect_err("invalid")
                    .to_string()
                    .contains("private")
            );
        }
        for bad in ["NaN", "-1", "1e9", "1..2", ".1", ""] {
            let mut fees = f["venue_fees"].clone();
            fees["total_sol"] = json!(bad);
            assert!(venue_tips(&fees, wallet).is_err());
        }
        assert!(venue_tips(&f["venue_fees"], "11111111111111111111111111111111").is_err());
    }
    #[tokio::test]
    async fn reads_only_rpc_credentials_and_rejects_scope_before_network_access() {
        let config = Config {
            values: [
                ("SOLAMI_DATA_API_TOKEN", "local-test-read-key-42-only"),
                ("SOLAMI_RPC_URL", "https://invalid.example"),
                ("ALIGHT_CANARY_KEYPAIR", "must-not-load"),
                ("SOLAMI_SWQOS_KEY", "must-not-load"),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
        };
        let provider = DataApiHistory::new(&config).expect("provider");
        assert!(provider.config.get("ALIGHT_CANARY_KEYPAIR").is_none());
        assert!(provider.config.get("SOLAMI_SWQOS_KEY").is_none());
        let result = provider
            .capture(
                "invalid-wallet",
                "2026-10-09T00:00:00Z",
                "2026-10-09T23:59:59Z",
                Some(vec![]),
                2,
            )
            .await;
        assert!(matches!(result, Err(HistoryError::Scope)));
    }
}
