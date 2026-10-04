//! Adapter input matches recorded Phase 0 gRPC fields and Mirage protobuf JSON.
use alight_store::raw_ref;
use alight_types::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use thiserror::Error;

#[derive(Debug, Error)]
#[error("invalid observer payload; upstream values withheld")]
pub struct AdapterError;

fn u64_value(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str()?.parse().ok())
}
fn i64_value(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str()?.parse().ok())
}
fn status(code: i64) -> Result<SlotStatus, AdapterError> {
    Ok(match code {
        0 => SlotStatus::Processed,
        1 => SlotStatus::Confirmed,
        2 => SlotStatus::Finalized,
        3 => SlotStatus::FirstShred,
        4 => SlotStatus::Completed,
        5 => SlotStatus::CreatedBank,
        6 => SlotStatus::Dead,
        _ => return Err(AdapterError),
    })
}

/// Selects actual provider fields and returns an addressable JSON evidence subset.
pub fn normalize(
    input: &Value,
    observer: ObserverKind,
    source: Source,
    received: ReceiveTime,
) -> Result<Option<(IngestEvent, Value)>, AdapterError> {
    let raw = if observer == ObserverKind::Mirage && input.get("data").is_some() {
        let body = &input["data"];
        if let Some(s) = body.get("slot") {
            let code = match s.get("status").and_then(Value::as_str) {
                None | Some("SLOT_PROCESSED") => 0,
                Some("SLOT_CONFIRMED") => 1,
                Some("SLOT_FINALIZED") => 2,
                Some("SLOT_FIRST_SHRED_RECEIVED") => 3,
                Some("SLOT_COMPLETED") => 4,
                Some("SLOT_CREATED_BANK") => 5,
                Some("SLOT_DEAD") => 6,
                _ => return Err(AdapterError),
            };
            json!({"kind":"slot","slot":s["slot"],"parent":s["parent"],"status_code":code})
        } else if let Some(b) = body.get("blockMeta") {
            let mut b = b.clone();
            b["kind"] = json!("block_meta");
            b
        } else if let Some(t) = body.get("transaction") {
            let info = &t["transaction"];
            let bytes = STANDARD
                .decode(info["signature"].as_str().ok_or(AdapterError)?)
                .map_err(|_| AdapterError)?;
            if bytes.len() != 64 {
                return Err(AdapterError);
            }
            let success = info
                .get("meta")
                .filter(|v| v.is_object())
                .map(|m| m.get("err").is_none_or(Value::is_null));
            json!({"kind":"transaction","slot":t["slot"],"signature":bs58::encode(bytes).into_string(),
                "index":info.get("index").cloned().unwrap_or(json!("0")),"failed":success.map(|b|!b)})
        } else {
            return Ok(None);
        }
    } else {
        match input["kind"].as_str() {
            Some("slot") => {
                json!({"kind":"slot","slot":input["slot"],"parent":input["parent"],"status_code":input["status_code"]})
            }
            Some("transaction") => {
                json!({"kind":"transaction","slot":input["slot"],"signature":input["signature"],"index":input["index"],"failed":input["failed"]})
            }
            Some("block_meta") => {
                json!({"kind":"block_meta","slot":input["slot"],"blockhash":input["blockhash"],"parentSlot":input["parentSlot"],
                "parentBlockhash":input["parentBlockhash"],"blockTime":input["blockTime"],"blockHeight":input["blockHeight"],"executedTransactionCount":input["executedTransactionCount"]})
            }
            _ => return Ok(None),
        }
    };
    let reference = raw_ref(&raw).map_err(|_| AdapterError)?;
    let slot = u64_value(&raw["slot"]).ok_or(AdapterError)?;
    let event = match raw["kind"].as_str() {
        Some("slot") => IngestEvent::Slot(SlotEvent {
            slot,
            block_id: None,
            status: status(raw["status_code"].as_i64().ok_or(AdapterError)?)?,
            received,
            leader: None,
            source,
            raw_ref: reference,
        }),
        Some("block_meta") => {
            let id = raw["blockhash"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or(AdapterError)?;
            IngestEvent::BlockMeta(BlockMetaEvent {
                slot,
                block_id: id.into(),
                parent_slot: u64_value(&raw["parentSlot"]),
                parent_block_id: raw["parentBlockhash"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
                block_time_unix_s: i64_value(&raw["blockTime"]["timestamp"]),
                block_height: u64_value(&raw["blockHeight"]["blockHeight"]),
                executed_transactions: u64_value(&raw["executedTransactionCount"]),
                received,
                source,
                raw_ref: reference,
            })
        }
        Some("transaction") => {
            let signature = raw["signature"].as_str().ok_or(AdapterError)?;
            if bs58::decode(signature)
                .into_vec()
                .map_err(|_| AdapterError)?
                .len()
                != 64
            {
                return Err(AdapterError);
            }
            IngestEvent::Observation(ObserverEvent {
                observer,
                signature: signature.into(),
                slot: Some(slot),
                block_id: None,
                index_in_block: u64_value(&raw["index"]).and_then(|i| u32::try_from(i).ok()),
                index_scope: IndexScope::ProviderReported,
                success: raw["failed"].as_bool().map(|b| !b),
                received,
                raw_ref: reference,
                source,
            })
        }
        _ => return Err(AdapterError),
    };
    Ok(Some((event, raw)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recorded_grpc_and_mirage_preserve_failures_and_default_status() {
        let clock = ReceiveTime {
            clock_id: "fixture".into(),
            mono_ns: 0,
            wall_utc: "2026-10-04T00:00:00Z".into(),
        };
        let mut count = 0;
        let mut failed = 0;
        for line in include_str!("../../../data/fixtures/grpc_transactions_sample.jsonl").lines() {
            let v = serde_json::from_str(line).expect("fixture");
            if let Some((IngestEvent::Observation(e), _)) =
                normalize(&v, ObserverKind::Grpc, Source::Replay, clock.clone()).expect("adapter")
            {
                count += 1;
                failed += usize::from(e.success == Some(false));
                assert!(e.block_id.is_none());
            }
        }
        assert_eq!(count, 13);
        assert!(failed > 0);
        let mut count = 0;
        let mut processed = 0;
        for line in include_str!("../../../data/fixtures/mirage_transactions_sample.jsonl").lines()
        {
            let v = serde_json::from_str(line).expect("fixture");
            if let Some((e, _)) =
                normalize(&v, ObserverKind::Mirage, Source::Replay, clock.clone()).expect("adapter")
            {
                match e {
                    IngestEvent::Observation(_) => count += 1,
                    IngestEvent::Slot(e) if e.status == SlotStatus::Processed => processed += 1,
                    _ => {}
                }
            }
        }
        assert_eq!(count, 3);
        assert!(processed > 0);
    }
}
