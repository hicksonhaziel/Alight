//! Owned-canary M0 cells. No network, signing, passive observations, or prior-only recommendations.
mod beta;
pub mod latency;
pub mod pooled;
pub mod quote;
pub mod regime;
pub mod regression;
pub mod scoring;
pub mod signal;
use alight_types::{
    CONTRACT_VERSION, CanaryConfig, CurveContext, CurveSnapshot, Evidence, Outcome, TrainingCanary,
};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use thiserror::Error;

pub const HALF_LIFE_S: f64 = 3600.0;
pub const MIN_EFFECTIVE_N: f64 = 30.0;
pub const MAX_AGE_S: f64 = 300.0;
pub const MAX_UNRESOLVED_SHARE: f64 = 0.25;

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("invalid model context, sample, or horizon")]
    Invalid,
    #[error("duplicate owned canary id in model input")]
    Duplicate,
}

/// SHA-256 of the registered methodology bytes embedded in this build.
pub fn methodology_hash() -> String {
    format!(
        "sha256:{:x}",
        Sha256::digest(include_bytes!("../../../docs/methodology.md"))
    )
}

pub(crate) fn utc(text: &str) -> Result<DateTime<Utc>, ModelError> {
    DateTime::parse_from_rfc3339(text)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| ModelError::Invalid)
}

pub(crate) fn label(
    sample: &TrainingCanary,
    context: &CurveContext,
    horizon: u32,
) -> Result<Option<(f64, f64)>, ModelError> {
    let c = &sample.canary;
    if c.source != context.source || c.regime_id != context.regime_id {
        return Ok(None);
    }
    let as_of = utc(&context.as_of_utc)?;
    let sent = utc(&c.send_wall_utc)?;
    if sent > as_of {
        return Ok(None);
    }
    if !c.assignment_prob.is_finite() || c.assignment_prob <= 0.0 || c.assignment_prob > 1.0 {
        return Err(ModelError::Invalid);
    }
    let resolved = c.resolved_at_utc.as_deref().map(utc).transpose()?;
    if resolved.is_some_and(|t| t < sent) {
        return Err(ModelError::Invalid);
    }
    if !sample.finalized || resolved.is_none_or(|t| t > as_of) {
        return Ok(None);
    }
    let y = match c.outcome {
        Some(Outcome::LandedOk | Outcome::LandedFailed) => f64::from(
            c.landed_slot
                .and_then(|s| s.checked_sub(c.sent_slot))
                .ok_or(ModelError::Invalid)?
                <= u64::from(horizon),
        ),
        Some(Outcome::Expired | Outcome::Rejected | Outcome::LandedThenDropped) => 0.0,
        _ => return Ok(None),
    };
    Ok(Some((
        y,
        (-((as_of - sent).num_milliseconds() as f64 / 1000.0) / HALF_LIFE_S).exp2(),
    )))
}

/// Estimates P(finalized landing within H slots) for one exact configuration/regime.
/// Weight age is elapsed UTC seconds since send; finalization and resolution must be known as-of.
pub fn estimate(
    samples: &[TrainingCanary],
    config: &CanaryConfig,
    context: &CurveContext,
    horizon_slots: u32,
) -> Result<CurveSnapshot, ModelError> {
    if horizon_slots == 0 || context.region.is_empty() || context.regime_id.is_empty() {
        return Err(ModelError::Invalid);
    }
    let as_of = utc(&context.as_of_utc)?;
    let mut ids = BTreeSet::new();
    let mut alpha = 1.0;
    let mut beta = 1.0;
    let mut resolved = 0u32;
    let mut unresolved = 0u32;
    let mut first = None;
    let mut latest = None;
    for sample in samples {
        let c = &sample.canary;
        if c.source != context.source || c.regime_id != context.regime_id || c.config != *config {
            continue;
        }
        let sent = utc(&c.send_wall_utc)?;
        if sent > as_of {
            continue;
        }
        if !ids.insert(&c.id) {
            return Err(ModelError::Duplicate);
        }
        if !c.assignment_prob.is_finite()
            || !(0.0..=1.0).contains(&c.assignment_prob)
            || c.assignment_prob == 0.0
        {
            return Err(ModelError::Invalid);
        }
        let known = c.resolved_at_utc.as_deref().map(utc).transpose()?;
        if known.is_some_and(|t| t < sent) {
            return Err(ModelError::Invalid);
        }
        if !sample.finalized
            || known.is_none_or(|t| t > as_of)
            || c.outcome.is_none_or(|o| o == Outcome::Unresolved)
        {
            unresolved = unresolved.checked_add(1).ok_or(ModelError::Invalid)?;
            continue;
        }
        let landed = match c.outcome {
            Some(Outcome::LandedOk | Outcome::LandedFailed) => {
                let distance = c
                    .landed_slot
                    .and_then(|s| s.checked_sub(c.sent_slot))
                    .ok_or(ModelError::Invalid)?;
                distance <= u64::from(horizon_slots)
            }
            Some(Outcome::Expired | Outcome::Rejected | Outcome::LandedThenDropped) => false,
            _ => return Err(ModelError::Invalid),
        };
        let age_s = (as_of - sent).num_milliseconds() as f64 / 1000.0;
        let weight = (-age_s / HALF_LIFE_S).exp2();
        if landed {
            alpha += weight;
        } else {
            beta += weight;
        }
        resolved = resolved.checked_add(1).ok_or(ModelError::Invalid)?;
        first = Some(first.map_or(sent, |t: DateTime<Utc>| t.min(sent)));
        latest = Some(latest.map_or(sent, |t: DateTime<Utc>| t.max(sent)));
    }
    let n_effective = alpha + beta - 2.0;
    let assignments = resolved
        .checked_add(unresolved)
        .ok_or(ModelError::Invalid)?;
    let unresolved_share = if assignments == 0 {
        0.0
    } else {
        f64::from(unresolved) / f64::from(assignments)
    };
    let data_age_s = latest.map(|t| (as_of - t).num_milliseconds() as f64 / 1000.0);
    let mut insufficient_reasons = Vec::new();
    if n_effective < MIN_EFFECTIVE_N {
        insufficient_reasons.push("discounted_sample_mass_below_30".into());
    }
    if data_age_s.is_none_or(|n| n > MAX_AGE_S) {
        insufficient_reasons.push("fresh_resolved_probes_required".into());
    }
    if unresolved_share > MAX_UNRESOLVED_SHARE {
        insufficient_reasons.push("unresolved_share_above_25_percent".into());
    }
    let insufficient = !insufficient_reasons.is_empty();
    let deficit = (MIN_EFFECTIVE_N - n_effective).ceil().max(1.0) as u32;
    // Fresh resolved samples also dilute the unresolved fraction below its permitted ceiling.
    let pending_deficit = (f64::from(unresolved) / MAX_UNRESOLVED_SHARE - f64::from(assignments))
        .ceil()
        .max(0.0) as u32;
    let result = CurveSnapshot {
        contract_version: CONTRACT_VERSION,
        estimator: "m0-decayed-beta-v1".into(),
        methodology_hash: methodology_hash(),
        context: context.clone(),
        config: config.clone(),
        horizon_slots,
        half_life_s: HALF_LIFE_S,
        alpha,
        beta,
        p_hat: alpha / (alpha + beta),
        p_interval_95: [
            beta::quantile(alpha, beta, 0.025),
            beta::quantile(alpha, beta, 0.975),
        ],
        n_effective,
        assignments,
        resolved,
        unresolved,
        unresolved_share,
        data_age_s,
        observation_window: first
            .zip(latest)
            .map(|(a, b)| [a.to_rfc3339(), b.to_rfc3339()]),
        evidence: if insufficient {
            Evidence::Insufficient
        } else {
            Evidence::Measured
        },
        samples_needed: insufficient.then_some(deficit.max(pending_deficit)),
        insufficient_reasons,
    };
    result.validate().map_err(|_| ModelError::Invalid)?;
    Ok(result)
}
