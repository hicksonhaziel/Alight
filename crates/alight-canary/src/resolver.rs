use alight_store::{Store, StoreError};
use alight_types::*;
use serde_json::json;

pub struct Resolution {
    pub canary: Canary,
    pub finalized: bool,
    pub reason: &'static str,
}

/// Resolves only evidence for the exact canary source/signature. Timeouts have no outcome.
pub fn resolve(canary: &Canary, evidence: &ResolutionEvidence, utc: &str) -> Resolution {
    let mut result = canary.clone();
    result.outcome = Some(Outcome::Unresolved);
    result.resolved_at_utc = Some(utc.into());
    result.landed_slot = None;
    result.landed_block_id = None;
    result.landed_index = None;
    result.landed_index_scope = None;
    let Some(signature) = canary.signature.as_deref() else {
        let rejected = evidence.route_rejection.is_some();
        if rejected {
            result.outcome = Some(Outcome::Rejected)
        }
        return Resolution {
            canary: result,
            finalized: rejected,
            reason: if rejected {
                "explicit_route_rejection"
            } else {
                "missing_signature"
            },
        };
    };
    let observations: Vec<_> = evidence
        .observations
        .iter()
        .filter(|o| o.source == canary.source && o.signature == signature)
        .collect();
    for o in &observations {
        result
            .observer_first_seen
            .entry(o.observer)
            .or_insert_with(|| o.received.clone());
    }
    let rpc = evidence.rpc.as_ref().filter(|r| {
        r.source == canary.source
            && r.signature == signature
            && r.checked_commitment >= r.required_commitment
            && chrono::DateTime::parse_from_rfc3339(&r.checked_at_utc)
                .ok()
                .zip(chrono::DateTime::parse_from_rfc3339(&canary.send_wall_utc).ok())
                .is_some_and(|(checked, sent)| checked >= sent)
    });
    let complete: Vec<_> = observations
        .iter()
        .filter(|o| {
            o.slot.is_some()
                && o.block_id.as_deref().is_some_and(|s| !s.is_empty())
                && o.success.is_some()
        })
        .copied()
        .collect();
    let missing_identity = complete.len() != observations.len();
    let disagree = complete.iter().any(|a| {
        complete.iter().any(|b| {
            a.slot != b.slot
                || a.block_id != b.block_id
                || a.success != b.success
                || (a.observer == b.observer
                    && a.index_scope == b.index_scope
                    && a.index_in_block.is_some()
                    && b.index_in_block.is_some()
                    && a.index_in_block != b.index_in_block)
        })
    });
    if disagree {
        return Resolution {
            canary: result,
            finalized: false,
            reason: "observer_disagreement",
        };
    }
    if let Some(landing) = rpc.and_then(|r| r.landing.as_ref()) {
        if landing.block_id.is_empty()
            || landing.commitment < Commitment::Confirmed
            || rpc.is_some_and(|r| landing.commitment > r.checked_commitment)
            || observations.iter().any(|o| {
                o.slot.is_some_and(|s| s != landing.slot)
                    || o.block_id.as_deref().is_some_and(|s| s != landing.block_id)
                    || o.success.is_some_and(|s| s != landing.success)
            })
        {
            return Resolution {
                canary: result,
                finalized: false,
                reason: "rpc_disagreement_or_incomplete",
            };
        }
        result.outcome = Some(if landing.success {
            Outcome::LandedOk
        } else {
            Outcome::LandedFailed
        });
        result.landed_slot = Some(landing.slot);
        result.landed_block_id = Some(landing.block_id.clone());
        if let Some(o) = complete.first() {
            result.landed_index = o.index_in_block;
            result.landed_index_scope = Some(o.index_scope);
        }
        return Resolution {
            canary: result,
            finalized: landing.commitment == Commitment::Finalized,
            reason: "rpc_confirmed_landing",
        };
    }
    if let Some(first) = complete.first() {
        let absent = rpc.is_some_and(|r| {
            r.searched_history
                && r.landing.is_none()
                && r.checked_commitment >= Commitment::Confirmed
        });
        let canonical = evidence.canonical_blocks.iter().find(|b| {
            b.source == canary.source
                && b.signature == signature
                && Some(b.slot) == first.slot
                && b.commitment >= Commitment::Confirmed
                && !b.signature_present
                && !b.block_id.is_empty()
        });
        if let Some(canonical) = canonical
            && absent
            && !missing_identity
        {
            result.outcome = Some(Outcome::LandedThenDropped);
            result.landed_slot = first.slot;
            result.landed_block_id = first.block_id.clone();
            result.landed_index = first.index_in_block;
            result.landed_index_scope = Some(first.index_scope);
            return Resolution {
                canary: result,
                finalized: canonical.commitment == Commitment::Finalized
                    && rpc.is_some_and(|r| r.checked_commitment == Commitment::Finalized),
                reason: "canonical_block_excludes_signature",
            };
        }
        // gRPC is primary; agreement of two transports is supplemental, not validator independence.
        let primary = complete.iter().any(|o| o.observer == ObserverKind::Grpc);
        let mut kinds = std::collections::BTreeSet::new();
        for o in &complete {
            kinds.insert(o.observer);
        }
        if !missing_identity && (primary || kinds.len() >= 2) {
            result.outcome = Some(if first.success == Some(true) {
                Outcome::LandedOk
            } else {
                Outcome::LandedFailed
            });
            result.landed_slot = first.slot;
            result.landed_block_id = first.block_id.clone();
            result.landed_index = first.index_in_block;
            result.landed_index_scope = Some(first.index_scope);
            return Resolution {
                canary: result,
                finalized: false,
                reason: "provisional_observed_landing",
            };
        }
    }
    // Any sighting blocks expiry, even if its candidate identity is not yet known.
    if observations.is_empty()
        && let Some(rpc) = rpc
        && rpc.searched_history
        && rpc.landing.is_none()
        && rpc.checked_commitment >= Commitment::Confirmed
        && rpc.checked_block_height > canary.last_valid_block_height
    {
        result.outcome = Some(Outcome::Expired);
        return Resolution {
            canary: result,
            finalized: rpc.checked_commitment == Commitment::Finalized,
            reason: "height_passed_and_rpc_history_absent",
        };
    }
    if observations.is_empty() && evidence.route_rejection.is_some() {
        result.outcome = Some(Outcome::Rejected);
        return Resolution {
            canary: result,
            finalized: true,
            reason: "explicit_route_rejection",
        };
    }
    Resolution {
        canary: result,
        finalized: false,
        reason: "insufficient_evidence",
    }
}

/// Reads observations from SQLite on every pass, including after a process restart.
pub async fn resolve_pending(
    store: &Store,
    mut evidence: ResolutionEvidence,
    utc: &str,
) -> Result<usize, StoreError> {
    let mut count = 0;
    for canary in store.pending_canaries().await? {
        let Some(signature) = canary.signature.as_deref() else {
            continue;
        };
        evidence.observations = store.observations(canary.source, signature).await?;
        let resolution = resolve(&canary, &evidence, utc);
        if resolution.canary.outcome != canary.outcome
            || resolution.canary.landed_block_id != canary.landed_block_id
            || resolution.canary.observer_first_seen.len() != canary.observer_first_seen.len()
            || resolution.finalized
        {
            store
                .save_resolution(
                    &resolution.canary,
                    resolution.finalized,
                    utc,
                    &json!({"reason":resolution.reason,"evidence":evidence}),
                )
                .await?;
        }
        count += 1;
    }
    Ok(count)
}
