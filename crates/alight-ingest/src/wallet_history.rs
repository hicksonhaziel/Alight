//! Finite, read-only RPC wallet history. No identity, database or sender is loaded.
use crate::{Config, HttpProbe, MAINNET_GENESIS, ProbeError};
use alight_types::*;
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::json;
use std::collections::BTreeSet;

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("invalid wallet history window, recipients or limit")]
    Scope,
    #[error("wallet history response conflicts with its signature page")]
    Conflict,
    #[error(transparent)]
    Provider(#[from] ProbeError),
    #[error("unsupported or invalid wallet transaction response")]
    Transaction,
}

pub struct HistoryCapture {
    pub capture: WalletHistoryCapture,
    pub provider_requests: u32,
    pub address_signatures: u32,
    pub outside_window: u32,
    pub not_fee_payer: u32,
    pub unavailable: u32,
}

/// Retrieves one finalized signature page (1–32), then its in-window transactions.
/// Window endpoints are inclusive UTC, at most 24 hours; fees are lamports.
/// Uses at most `limit + 2` bounded read requests with no retries or pagination.
/// Coverage is always partial. Unavailable transactions are counted, never zero-filled.
pub async fn fetch(
    config: &Config,
    wallet: &str,
    from_utc: &str,
    through_utc: &str,
    tip_recipients: Vec<String>,
    limit: u32,
) -> Result<HistoryCapture, HistoryError> {
    let mut result = HistoryCapture {
        capture: WalletHistoryCapture {
            schema_version: 1,
            source: Source::Replay,
            wallet: wallet.into(),
            from_utc: from_utc.into(),
            through_utc: through_utc.into(),
            tip_recipients,
            rows: vec![],
        },
        provider_requests: 0,
        address_signatures: 0,
        outside_window: 0,
        not_fee_payer: 0,
        unavailable: 0,
    };
    alight_tape::receipts::validate(&result.capture).map_err(|_| HistoryError::Scope)?;
    let from = DateTime::parse_from_rfc3339(from_utc).map_err(|_| HistoryError::Scope)?;
    let through = DateTime::parse_from_rfc3339(through_utc).map_err(|_| HistoryError::Scope)?;
    if !(1..=32).contains(&limit) || through > Utc::now() {
        return Err(HistoryError::Scope);
    }
    let probe = HttpProbe::new()?;
    let genesis = probe.rpc(config, "getGenesisHash", json!([])).await?;
    result.provider_requests += 1;
    if genesis.as_str() != Some(MAINNET_GENESIS) {
        return Err(ProbeError::WrongCluster.into());
    }
    let page = probe
        .rpc(
            config,
            "getSignaturesForAddress",
            json!([wallet,{"commitment":"finalized","limit":limit}]),
        )
        .await?;
    result.provider_requests += 1;
    let entries = page.as_array().ok_or(ProbeError::Format)?;
    if entries.len() > limit as usize {
        return Err(ProbeError::Size.into());
    }
    let mut signatures = BTreeSet::new();
    for entry in entries {
        let signature = entry["signature"].as_str().ok_or(ProbeError::Format)?;
        if signature.len() > 88
            || !bs58::decode(signature)
                .into_vec()
                .is_ok_and(|v| v.len() == 64)
            || !signatures.insert(signature)
            || entry["confirmationStatus"] != "finalized"
        {
            return Err(ProbeError::Format.into());
        }
        let slot = entry["slot"].as_u64().ok_or(ProbeError::Format)?;
        let success = entry.get("err").ok_or(ProbeError::Format)?.is_null();
        result.address_signatures += 1;
        let Some(block_time) = entry["blockTime"].as_i64() else {
            if !entry["blockTime"].is_null() {
                return Err(ProbeError::Format.into());
            }
            result.unavailable += 1;
            continue;
        };
        let at = DateTime::from_timestamp(block_time, 0).ok_or(ProbeError::Format)?;
        if at < from || at > through {
            result.outside_window += 1;
            continue;
        }
        let value = probe
            .rpc(
                config,
                "getTransaction",
                json!([signature,{"commitment":"finalized","encoding":"json","maxSupportedTransactionVersion":0}]),
            )
            .await?;
        result.provider_requests += 1;
        let received = ReceiveTime {
            clock_id: "wallet-rpc-history".into(),
            mono_ns: 0,
            wall_utc: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        };
        let Some(row) = alight_tape::rpc_history::transaction(&value, received)
            .map_err(|_| HistoryError::Transaction)?
        else {
            result.unavailable += 1;
            continue;
        };
        if row.transaction.signature != signature
            || row.transaction.slot != slot
            || row.transaction.success != success
            || row.chain_time_utc.as_deref()
                != Some(at.to_rfc3339_opts(SecondsFormat::Secs, true).as_str())
        {
            return Err(HistoryError::Conflict);
        }
        if row.transaction.account_keys.first().map(String::as_str) != Some(wallet) {
            result.not_fee_payer += 1;
            continue;
        }
        result.capture.rows.push(row);
    }
    alight_tape::receipts::validate(&result.capture).map_err(|_| HistoryError::Transaction)?;
    Ok(result)
}
