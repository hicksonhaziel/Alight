//! Bounded RPC corroboration. All requests pass through HttpProbe's read-only allowlist.
use crate::{
    Config, HttpProbe, ProbeError,
    stream::{ReceiveClock, utc_now},
};
use alight_store::{Store, raw_ref};
use alight_types::*;
use serde_json::{Value, json};

pub struct RpcEvidence {
    pub proof: Option<RpcCheck>,
    pub canonical_blocks: Vec<CanonicalBlock>,
    pub frames: Vec<(IngestEvent, Value)>,
    pub raw: Value,
}
fn commitment(v: &Value) -> Result<Commitment, ProbeError> {
    match v.as_str() {
        Some("processed") => Ok(Commitment::Processed),
        Some("confirmed") => Ok(Commitment::Confirmed),
        Some("finalized") => Ok(Commitment::Finalized),
        _ => Err(ProbeError::Format),
    }
}
fn hash(value: &Value) -> Result<String, ProbeError> {
    raw_ref(value).map_err(|_| ProbeError::Format)
}
fn block_id(value: &Value) -> Result<String, ProbeError> {
    let text = value.as_str().ok_or(ProbeError::Format)?;
    if bs58::decode(text)
        .into_vec()
        .map_err(|_| ProbeError::Format)?
        .len()
        != 32
    {
        return Err(ProbeError::Format);
    }
    Ok(text.to_owned())
}

/// Height is sampled before history status; absence also requires retained sent-slot history.
pub async fn corroborate(
    config: &Config,
    source: Source,
    signature: &str,
    sent_slot: u64,
    observations: &[ObserverEvent],
    clock: &ReceiveClock,
) -> Result<RpcEvidence, ProbeError> {
    if source != Source::Live {
        return Err(ProbeError::Configuration);
    }
    if bs58::decode(signature)
        .into_vec()
        .map_err(|_| ProbeError::Configuration)?
        .len()
        != 64
    {
        return Err(ProbeError::Configuration);
    }
    let client = HttpProbe::new()?;
    let finalized_slot = client
        .rpc(config, "getSlot", json!([{"commitment":"finalized"}]))
        .await?
        .as_u64()
        .ok_or(ProbeError::Format)?;
    let minimum = finalized_slot.max(sent_slot);
    let height = client
        .rpc(
            config,
            "getBlockHeight",
            json!([{"commitment":"finalized","minContextSlot":minimum}]),
        )
        .await?
        .as_u64()
        .ok_or(ProbeError::Format)?;
    let statuses = client
        .rpc(
            config,
            "getSignatureStatuses",
            json!([[signature],{"searchTransactionHistory":true}]),
        )
        .await?;
    let context = statuses["context"]["slot"]
        .as_u64()
        .filter(|s| *s >= minimum)
        .ok_or(ProbeError::Format)?;
    let values = statuses["value"]
        .as_array()
        .filter(|v| v.len() == 1)
        .ok_or(ProbeError::Format)?;
    let status = &values[0];
    let mut result = RpcEvidence {
        proof: None,
        canonical_blocks: vec![],
        frames: vec![],
        raw: Value::Null,
    };
    let mut landing = None;
    let mut first_available = None;
    let mut candidate_slots = std::collections::BTreeSet::new();
    if !status.is_null() {
        let slot = status["slot"].as_u64().ok_or(ProbeError::Format)?;
        let comm = commitment(&status["confirmationStatus"])?;
        let success = status.get("err").ok_or(ProbeError::Format)?.is_null();
        if comm < Commitment::Confirmed {
            return Ok(result);
        } // A processed status blocks negative conclusions.
        candidate_slots.insert(slot);
        landing = Some((slot, comm, success));
    } else {
        let first = client
            .rpc(config, "getFirstAvailableBlock", json!([]))
            .await?
            .as_u64()
            .ok_or(ProbeError::Format)?;
        if first > sent_slot {
            return Ok(result);
        } // Pruned history is missing evidence.
        first_available = Some(first);
        for o in observations
            .iter()
            .filter(|o| o.source == source && o.signature == signature)
        {
            if let Some(slot) = o.slot {
                candidate_slots.insert(slot);
            }
        }
    }
    // Bound per-canary requests. Additional fork candidates remain unresolved.
    if candidate_slots.len() > 4 {
        return Ok(result);
    }
    let mut rpc_landing = None;
    let mut block_evidence = vec![];
    for slot in candidate_slots {
        let comm = landing
            .as_ref()
            .map_or(Commitment::Finalized, |(_, c, _)| *c);
        let block=client.rpc(config,"getBlock",json!([slot,{"commitment":alight_store::label(comm).map_err(|_|ProbeError::Format)?,"transactionDetails":"signatures","rewards":false,"maxSupportedTransactionVersion":1}])).await?;
        if block.is_null() {
            return Ok(RpcEvidence {
                proof: None,
                canonical_blocks: vec![],
                frames: vec![],
                raw: Value::Null,
            });
        }
        let id = block_id(&block["blockhash"])?;
        let signatures = block["signatures"].as_array().ok_or(ProbeError::Format)?;
        if signatures.iter().any(|s| !s.is_string()) {
            return Err(ProbeError::Format);
        }
        let index = signatures
            .iter()
            .position(|s| s.as_str() == Some(signature));
        let mut raw = json!({"slot":slot.to_string(),"commitment":comm,"block":block,"signature_status":status});
        config.scrub(&mut raw);
        let reference = hash(&raw)?;
        result.canonical_blocks.push(CanonicalBlock {
            source: Source::Live,
            slot,
            block_id: id.clone(),
            commitment: comm,
            signature: signature.into(),
            signature_present: index.is_some(),
            raw_ref: reference.clone(),
        });
        if let Some((landed_slot, comm, success)) = landing
            && landed_slot == slot
        {
            let Some(index) = index else {
                return Ok(RpcEvidence {
                    proof: None,
                    canonical_blocks: vec![],
                    frames: vec![],
                    raw: Value::Null,
                });
            };
            rpc_landing = Some(RpcLanding {
                slot,
                block_id: id.clone(),
                success,
                commitment: comm,
            });
            result.frames.push((
                IngestEvent::Observation(ObserverEvent {
                    observer: ObserverKind::Rpc,
                    signature: signature.into(),
                    slot: Some(slot),
                    block_id: Some(id),
                    index_in_block: u32::try_from(index).ok(),
                    index_scope: IndexScope::RpcBlockList,
                    success: Some(success),
                    received: clock.receive(),
                    raw_ref: reference,
                    source: Source::Live,
                }),
                raw.clone(),
            ));
        }
        block_evidence.push(raw);
    }
    let mut raw = json!({"height":height.to_string(),"status":statuses,"blocks":block_evidence,"finalized_slot":finalized_slot.to_string(),"history_first_available_slot":first_available.map(|s|s.to_string())});
    config.scrub(&mut raw);
    result.proof = Some(RpcCheck {
        signature: signature.into(),
        source: Source::Live,
        required_commitment: Commitment::Confirmed,
        checked_commitment: landing.map_or(Commitment::Finalized, |(_, c, _)| c),
        checked_block_height: height,
        searched_history: true,
        history_covers_sent_slot: true,
        context_slot: context,
        checked_at_utc: utc_now(),
        landing: rpc_landing,
        raw_ref: hash(&raw)?,
    });
    result.raw = raw;
    Ok(result)
}

/// Fetches two independent RPC timestamps for an older saved gRPC clock window.
pub async fn check_clock(config: &Config, store: &Store) -> Result<Value, ProbeError> {
    let mut blocks = store
        .block_samples(ObserverKind::Grpc, Source::Live)
        .await
        .map_err(|_| ProbeError::Format)?;
    blocks.sort_by_key(|b| b.slot);
    let last = blocks.last().ok_or(ProbeError::Format)?.slot;
    let end = blocks
        .iter()
        .rev()
        .find(|b| b.slot <= last.saturating_sub(128) && b.block_time_unix_s.is_some())
        .ok_or(ProbeError::Format)?;
    let start = blocks
        .iter()
        .find(|b| b.slot + 64 <= end.slot && b.block_time_unix_s.is_some())
        .ok_or(ProbeError::Format)?;
    let client = HttpProbe::new()?;
    let first = client
        .rpc(config, "getBlockTime", json!([start.slot]))
        .await?
        .as_i64()
        .ok_or(ProbeError::Format)?;
    let last = client
        .rpc(config, "getBlockTime", json!([end.slot]))
        .await?
        .as_i64()
        .ok_or(ProbeError::Format)?;
    let rpc = SlotWindow {
        start_slot: start.slot,
        end_slot: end.slot,
        start_unix_s: first,
        end_unix_s: last,
    }
    .mean_slot_ms()
    .ok_or(ProbeError::Format)?;
    let grpc = SlotWindow {
        start_slot: start.slot,
        end_slot: end.slot,
        start_unix_s: start.block_time_unix_s.ok_or(ProbeError::Format)?,
        end_unix_s: end.block_time_unix_s.ok_or(ProbeError::Format)?,
    }
    .mean_slot_ms()
    .ok_or(ProbeError::Format)?;
    Ok(
        json!({"source":"live","canaries_sent":0,"start":start,"end":end,"rpc_start_unix_s":first,"rpc_end_unix_s":last,"grpc_mean_slot_ms":grpc,"rpc_mean_slot_ms":rpc,"tolerance_ms":1.0,"verdict":if (grpc-rpc).abs()<=1.0{"PASS"}else{"FAIL"}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_rpc_fixtures_retain_addressable_positive_and_negative_proofs() {
        for (text, absent) in [
            (
                include_str!("../../../data/fixtures/rpc_live_corroboration.json"),
                false,
            ),
            (
                include_str!("../../../data/fixtures/rpc_absence_probe.json"),
                true,
            ),
        ] {
            let fixture: Value = serde_json::from_str(text).expect("fixture");
            let proof: RpcCheck = serde_json::from_value(fixture["rpc"].clone()).expect("proof");
            assert_eq!(fixture["verdict"], "PASS");
            assert_eq!(fixture["canary_created"], false);
            assert_eq!(fixture["canaries_sent"], 0);
            assert_eq!(proof.source, Source::Live);
            assert_eq!(proof.landing.is_none(), absent);
            assert!(proof.searched_history && proof.history_covers_sent_slot);
            assert!(proof.checked_commitment >= Commitment::Confirmed);
            assert_eq!(
                proof.raw_ref,
                raw_ref(&fixture["captured_fields"]).expect("hash")
            );
            let finalized_slot: u64 = fixture["captured_fields"]["finalized_slot"]
                .as_str()
                .expect("slot")
                .parse()
                .expect("number");
            assert!(proof.context_slot >= finalized_slot);
            if let Some(landing) = proof.landing {
                assert!(landing.commitment >= Commitment::Confirmed);
                assert!(!landing.block_id.is_empty());
            } else {
                assert!(!fixture["captured_fields"]["history_first_available_slot"].is_null());
            }
        }
    }
}
