//! Passive native SOL transfers to a configured recipient set, never owned training labels.
use alight_types::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

const SYSTEM: &str = "11111111111111111111111111111111";
const COMPUTE: &str = "ComputeBudget111111111111111111111111111111";

#[derive(Debug, Error)]
pub enum TapeError {
    #[error("invalid passive transaction shape or encoding")]
    Invalid,
    #[error("passive transaction exceeds parser bounds")]
    Limit,
    #[error("conflicting instruction identity or compute budget")]
    Conflict,
}

fn key(text: &str, size: usize) -> Result<(), TapeError> {
    if text.len() > 100
        || bs58::decode(text)
            .into_vec()
            .map_err(|_| TapeError::Invalid)?
            .len()
            != size
    {
        return Err(TapeError::Invalid);
    }
    Ok(())
}

fn bytes<const N: usize>(data: &[u8], start: usize) -> Result<[u8; N], TapeError> {
    data.get(start..start + N)
        .ok_or(TapeError::Invalid)?
        .try_into()
        .map_err(|_| TapeError::Invalid)
}

/// Aggregates transfer intent in lamports per recipient. Failed transactions paid zero.
/// A successful transaction with matching inner transfers has unknown payment because
/// an outer program can catch a failed CPI. This parser does not infer a transport route.
pub fn parse(tx: &PassiveTransaction, recipients: &[String]) -> Result<Vec<PassiveTip>, TapeError> {
    if tx.account_keys.len() > 256 || tx.instructions.len() > 4096 || recipients.len() > 256 {
        return Err(TapeError::Limit);
    }
    key(&tx.signature, 64)?;
    chrono::DateTime::parse_from_rfc3339(&tx.received.wall_utc).map_err(|_| TapeError::Invalid)?;
    for account in tx.account_keys.iter().chain(recipients) {
        key(account, 32)?;
    }
    let allowed = recipients.iter().collect::<BTreeSet<_>>();
    let mut identities = BTreeSet::new();
    let outers = tx
        .instructions
        .iter()
        .filter(|ix| ix.inner_index.is_none())
        .map(|ix| ix.outer_index)
        .collect::<BTreeSet<_>>();
    let mut amounts: BTreeMap<String, (u64, bool, u64)> = BTreeMap::new();
    let mut cu_limit = None;
    let mut cu_price = None;
    for ix in &tx.instructions {
        if ix.inner_index.is_some() && !outers.contains(&ix.outer_index) {
            return Err(TapeError::Invalid);
        }
        if !identities.insert((ix.outer_index, ix.inner_index)) {
            return Err(TapeError::Conflict);
        }
        let program = tx
            .account_keys
            .get(ix.program_id_index as usize)
            .ok_or(TapeError::Invalid)?;
        if ix
            .accounts
            .iter()
            .any(|i| *i as usize >= tx.account_keys.len())
        {
            return Err(TapeError::Invalid);
        }
        if program != SYSTEM && program != COMPUTE {
            continue;
        }
        if ix.data_base64.len() > 2048 {
            return Err(TapeError::Limit);
        }
        let data = STANDARD
            .decode(&ix.data_base64)
            .map_err(|_| TapeError::Invalid)?;
        if program == COMPUTE && ix.inner_index.is_none() {
            match data.first() {
                Some(2) if data.len() == 5 => {
                    let value = u32::from_le_bytes(bytes(&data, 1)?);
                    if cu_limit.replace(value).is_some() {
                        return Err(TapeError::Conflict);
                    }
                }
                Some(3) if data.len() == 9 => {
                    let value = u64::from_le_bytes(bytes(&data, 1)?);
                    if cu_price.replace(value).is_some() {
                        return Err(TapeError::Conflict);
                    }
                }
                Some(2 | 3) => return Err(TapeError::Invalid),
                _ => {}
            }
        }
        if program != SYSTEM {
            continue;
        }
        let tag = u32::from_le_bytes(bytes(&data, 0)?);
        let recipient_index = match tag {
            2 if data.len() == 12 && ix.accounts.len() == 2 => 1,
            11 if ix.accounts.len() == 3 => {
                let seed_len = usize::try_from(u64::from_le_bytes(bytes(&data, 12)?))
                    .map_err(|_| TapeError::Invalid)?;
                if seed_len > 32
                    || data.len() != 52 + seed_len
                    || std::str::from_utf8(data.get(20..20 + seed_len).ok_or(TapeError::Invalid)?)
                        .is_err()
                {
                    return Err(TapeError::Invalid);
                }
                2
            }
            2 | 11 => return Err(TapeError::Invalid),
            _ => continue,
        };
        let recipient = &tx.account_keys[ix.accounts[recipient_index] as usize];
        if !allowed.contains(recipient) {
            continue;
        }
        let amount = u64::from_le_bytes(bytes(&data, 4)?);
        let total = amounts.entry(recipient.clone()).or_default();
        total.0 = total.0.checked_add(amount).ok_or(TapeError::Invalid)?;
        total.1 |= ix.inner_index.is_some();
        if tx.account_keys[ix.accounts[0] as usize] != *recipient {
            total.2 = total.2.checked_add(amount).ok_or(TapeError::Invalid)?;
        }
    }
    Ok(amounts
        .into_iter()
        .map(|(recipient, (amount, has_inner, paid))| PassiveTip {
            source: tx.source,
            observer: tx.observer,
            signature: tx.signature.clone(),
            slot: tx.slot,
            block_id: tx.block_id.clone(),
            index_in_block: tx.index_in_block,
            index_scope: tx.index_scope,
            recipient,
            requested_tip_lamports: amount,
            tip_lamports: if !tx.success {
                Some(0)
            } else if has_inner {
                None
            } else {
                Some(paid)
            },
            fee_lamports: tx.fee_lamports,
            cu_price_micro_lamports: cu_price,
            cu_limit,
            success: tx.success,
            received: tx.received.clone(),
        })
        .collect())
}

fn decode_key(v: &Value, size: usize) -> Result<String, TapeError> {
    if v.as_str().is_none_or(|s| s.len() > 100) {
        return Err(TapeError::Invalid);
    }
    let raw = STANDARD
        .decode(v.as_str().ok_or(TapeError::Invalid)?)
        .map_err(|_| TapeError::Invalid)?;
    if raw.len() != size {
        return Err(TapeError::Invalid);
    }
    Ok(bs58::encode(raw).into_string())
}
fn number(v: &Value) -> Result<u64, TapeError> {
    v.as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| v.as_u64())
        .ok_or(TapeError::Invalid)
}
fn capture_ix(v: &Value, outer: u32, inner: Option<u32>) -> Result<PassiveInstruction, TapeError> {
    if v.get("accounts")
        .is_some_and(|a| a.as_str().is_none_or(|s| s.len() > 344))
        || v.get("data")
            .is_some_and(|a| a.as_str().is_none_or(|s| s.len() > 2048))
    {
        return Err(TapeError::Limit);
    }
    let accounts = match v.get("accounts") {
        None => vec![],
        Some(a) => STANDARD
            .decode(a.as_str().ok_or(TapeError::Invalid)?)
            .map_err(|_| TapeError::Invalid)?,
    };
    Ok(PassiveInstruction {
        program_id_index: u32::try_from(
            v.get("programIdIndex")
                .map(number)
                .transpose()?
                .unwrap_or(0),
        )
        .map_err(|_| TapeError::Invalid)?,
        accounts: accounts.into_iter().map(u32::from).collect(),
        data_base64: v
            .get("data")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        outer_index: outer,
        inner_index: inner,
    })
}

/// Reads the captured Yellowstone JSON transaction shape in a Mirage fixture.
/// Replay source is supplied explicitly; fixture capture labels never become live evidence.
pub fn captured_transaction(
    v: &Value,
    source: Source,
    received: ReceiveTime,
) -> Result<Option<PassiveTransaction>, TapeError> {
    let Some(t) = v.get("data").and_then(|d| d.get("transaction")) else {
        return Ok(None);
    };
    let info = t.get("transaction").ok_or(TapeError::Invalid)?;
    let message = &info["transaction"]["message"];
    let meta = info.get("meta").ok_or(TapeError::Invalid)?;
    let static_keys = message["accountKeys"]
        .as_array()
        .ok_or(TapeError::Invalid)?;
    if static_keys.len() > 256 {
        return Err(TapeError::Limit);
    }
    let mut account_keys = static_keys
        .iter()
        .map(|k| decode_key(k, 32))
        .collect::<Result<Vec<_>, _>>()?;
    for name in ["loadedWritableAddresses", "loadedReadonlyAddresses"] {
        if let Some(keys) = meta.get(name) {
            if keys
                .as_array()
                .is_none_or(|k| k.len() + account_keys.len() > 256)
            {
                return Err(TapeError::Limit);
            }
            account_keys.extend(
                keys.as_array()
                    .ok_or(TapeError::Invalid)?
                    .iter()
                    .map(|k| decode_key(k, 32))
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
    }
    if message["instructions"]
        .as_array()
        .is_none_or(|k| k.len() > 4096)
    {
        return Err(TapeError::Limit);
    }
    let mut instructions = message["instructions"]
        .as_array()
        .ok_or(TapeError::Invalid)?
        .iter()
        .enumerate()
        .map(|(i, ix)| capture_ix(ix, i as u32, None))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(inner) = meta.get("innerInstructions") {
        for group in inner.as_array().ok_or(TapeError::Invalid)? {
            let outer = u32::try_from(number(&group["index"])?).map_err(|_| TapeError::Invalid)?;
            if group["instructions"]
                .as_array()
                .is_none_or(|k| k.len() + instructions.len() > 4096)
            {
                return Err(TapeError::Limit);
            }
            for (i, ix) in group["instructions"]
                .as_array()
                .ok_or(TapeError::Invalid)?
                .iter()
                .enumerate()
            {
                instructions.push(capture_ix(ix, outer, Some(i as u32))?);
            }
        }
    }
    Ok(Some(PassiveTransaction {
        source,
        observer: ObserverKind::Mirage,
        signature: decode_key(&info["signature"], 64)?,
        slot: number(&t["slot"])?,
        block_id: None,
        index_in_block: Some(number(&info["index"])?),
        index_scope: IndexScope::ProviderReported,
        success: meta.get("err").is_none_or(Value::is_null),
        fee_lamports: number(&meta["fee"])?,
        account_keys,
        instructions,
        received,
    }))
}
