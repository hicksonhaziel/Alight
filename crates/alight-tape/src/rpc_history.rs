//! Normalize captured legacy/v0 `getTransaction` JSON; no transport or signing.
use crate::{TapeError, key, parse};
use alight_types::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, SecondsFormat};
use serde_json::Value;

fn instruction(
    value: &Value,
    outer_index: u32,
    inner_index: Option<u32>,
) -> Result<PassiveInstruction, TapeError> {
    let accounts = value["accounts"].as_array().ok_or(TapeError::Invalid)?;
    let encoded = value["data"].as_str().ok_or(TapeError::Invalid)?;
    if accounts.len() > 256 || encoded.len() > 2048 {
        return Err(TapeError::Limit);
    }
    let data = bs58::decode(encoded)
        .into_vec()
        .map_err(|_| TapeError::Invalid)?;
    Ok(PassiveInstruction {
        program_id_index: value["programIdIndex"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or(TapeError::Invalid)?,
        accounts: accounts
            .iter()
            .map(|v| {
                v.as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or(TapeError::Invalid)
            })
            .collect::<Result<_, _>>()?,
        data_base64: STANDARD.encode(data),
        outer_index,
        inner_index,
    })
}

/// Reads the result of finalized `getTransaction` with `encoding=json` and
/// `maxSupportedTransactionVersion=0`. Fees are lamports; block time is UTC.
/// Null means unavailable, never a failed transaction or a zero-fee execution.
/// Captured history is Replay; transport, workload, regime and block index stay unknown.
pub fn transaction(
    value: &Value,
    received: ReceiveTime,
) -> Result<Option<WalletHistoryRow>, TapeError> {
    if value.is_null() {
        return Ok(None);
    }
    if value["version"] != "legacy" && value["version"] != 0 {
        return Err(TapeError::Invalid);
    }
    let meta = value["meta"].as_object().ok_or(TapeError::Invalid)?;
    let message = &value["transaction"]["message"];
    let signature = value["transaction"]["signatures"][0]
        .as_str()
        .ok_or(TapeError::Invalid)?;
    key(signature, 64)?;
    let mut account_keys: Vec<String> = Vec::new();
    let mut append_keys = |values: &Value| -> Result<(), TapeError> {
        let values = values.as_array().ok_or(TapeError::Invalid)?;
        if values.len() + account_keys.len() > 256 {
            return Err(TapeError::Limit);
        }
        for value in values {
            let text = value.as_str().ok_or(TapeError::Invalid)?;
            key(text, 32)?;
            account_keys.push(text.to_owned());
        }
        Ok(())
    };
    append_keys(&message["accountKeys"])?;
    if let Some(loaded) = meta.get("loadedAddresses") {
        append_keys(&loaded["writable"])?;
        append_keys(&loaded["readonly"])?;
    } else if message
        .get("addressTableLookups")
        .is_some_and(|v| v.as_array().is_none_or(|a| !a.is_empty()))
    {
        return Err(TapeError::Invalid);
    }
    if account_keys.is_empty() {
        return Err(TapeError::Invalid);
    }
    let outer = message["instructions"]
        .as_array()
        .ok_or(TapeError::Invalid)?;
    if outer.len() > 4096 {
        return Err(TapeError::Limit);
    }
    let mut instructions = outer
        .iter()
        .enumerate()
        .map(|(i, v)| instruction(v, i as u32, None))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(inner) = meta.get("innerInstructions").filter(|v| !v.is_null()) {
        let groups = inner.as_array().ok_or(TapeError::Invalid)?;
        if groups.len() > 4096 {
            return Err(TapeError::Limit);
        }
        for group in groups {
            let outer_index = group["index"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or(TapeError::Invalid)?;
            let inner = group["instructions"].as_array().ok_or(TapeError::Invalid)?;
            if inner.len() + instructions.len() > 4096 {
                return Err(TapeError::Limit);
            }
            for (i, value) in inner.iter().enumerate() {
                instructions.push(instruction(value, outer_index, Some(i as u32))?);
            }
        }
    }
    let chain_time_utc = match value.get("blockTime").ok_or(TapeError::Invalid)? {
        Value::Null => None,
        time => Some(
            DateTime::from_timestamp(time.as_i64().ok_or(TapeError::Invalid)?, 0)
                .ok_or(TapeError::Invalid)?
                .to_rfc3339_opts(SecondsFormat::Secs, true),
        ),
    };
    let transaction = PassiveTransaction {
        source: Source::Replay,
        observer: ObserverKind::Rpc,
        signature: signature.into(),
        slot: value["slot"].as_u64().ok_or(TapeError::Invalid)?,
        block_id: None,
        index_in_block: None,
        index_scope: IndexScope::Unknown,
        success: meta.get("err").ok_or(TapeError::Invalid)?.is_null(),
        fee_lamports: meta
            .get("fee")
            .and_then(Value::as_u64)
            .ok_or(TapeError::Invalid)?,
        account_keys,
        instructions,
        received,
    };
    parse(&transaction, &[])?;
    Ok(Some(WalletHistoryRow {
        transaction,
        chain_time_utc,
        route: None,
        size_class: None,
        regime_id: None,
    }))
}
