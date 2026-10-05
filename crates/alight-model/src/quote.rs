use crate::{ModelError, latency, methodology_hash, pooled, utc};
use alight_types::*;

/// Exact integer synthetic fee-plus-tip cost, in lamports, for ordering candidates.
pub fn nominal_cost(config: &CanaryConfig) -> u64 {
    let priority = (u128::from(config.cu_price_micro_lamports) * u128::from(config.cu_limit))
        .div_ceil(1_000_000);
    config
        .tip_lamports
        .saturating_add(5000)
        .saturating_add(u64::try_from(priority).unwrap_or(u64::MAX))
}
/// Probability-only quote, with optional latency-quantile target. Economics remain a Phase 3 layer.
pub fn quote(
    samples: &[TrainingCanary],
    request: &ModelQuoteRequest,
    models: &[ModelFit],
) -> Result<ModelQuote, ModelError> {
    if request.candidates.is_empty()
        || request.candidates.len() > 1000
        || request.context.region.is_empty()
    {
        return Err(ModelError::Invalid);
    }
    let horizon = match request.target {
        PredictionTarget::Probability {
            target_p,
            horizon_slots,
        } if target_p.is_finite() && target_p > 0.0 && target_p <= 1.0 && horizon_slots > 0 => {
            horizon_slots
        }
        PredictionTarget::LatencyQuantile {
            quantile,
            max_slots,
            max_ms,
        } if quantile.is_finite()
            && quantile > 0.0
            && quantile < 1.0
            && max_slots.is_some() != max_ms.is_some()
            && max_slots
                .or(max_ms)
                .is_some_and(|v| v.is_finite() && v > 0.0) =>
        {
            4
        }
        _ => return Err(ModelError::Invalid),
    };
    let model = models.iter().find(|m| {
        m.horizon_slots == horizon
            && m.context.source == request.context.source
            && m.context.regime_id == request.context.regime_id
    });
    let mut candidates = request.candidates.clone();
    candidates.sort_by_key(nominal_cost);
    let mut needed = u32::MAX;
    let mut result = ModelQuote {
        contract_version: CONTRACT_VERSION,
        methodology_hash: methodology_hash(),
        context: request.context.clone(),
        target: request.target.clone(),
        evidence: Evidence::Insufficient,
        recommendation: None,
        latency: None,
        samples_needed: Some(30),
        reason: Some("insufficient_evidence_or_target_not_supported".into()),
    };
    for config in candidates {
        let prediction = pooled::predict(
            samples,
            model,
            &config,
            &request.context,
            horizon,
            &request.leader_class_next,
            &request.covariates,
        )?;
        needed = needed.min(prediction.samples_needed.unwrap_or(1));
        if prediction.evidence == Evidence::Insufficient
            || prediction.evidence == Evidence::Extrapolated
        {
            continue;
        }
        let mut q = None;
        let qualifies = match request.target {
            PredictionTarget::Probability { target_p, .. } => {
                prediction.p_interval_95[0] >= target_p
            }
            PredictionTarget::LatencyQuantile {
                quantile,
                max_slots,
                max_ms,
            } => {
                let estimate = latency::estimate_quantile(
                    samples,
                    &config,
                    &request.context,
                    quantile,
                    latency::cluster_seed(&request.context.as_of_utc),
                )?;
                let okay = estimate.evidence == Evidence::Measured
                    && if let Some(max) = max_slots {
                        estimate.slots_interval_95[1].is_some_and(|v| v <= max)
                    } else {
                        estimate.ms_interval_95[1]
                            .zip(max_ms)
                            .is_some_and(|(v, max)| v <= max)
                    };
                q = Some(estimate);
                okay
            }
        };
        if qualifies {
            result.evidence = prediction.evidence;
            result.recommendation = Some(prediction);
            result.latency = q;
            result.samples_needed = None;
            result.reason = None;
            break;
        }
    }
    if result.recommendation.is_none() {
        result.samples_needed = Some(needed.clamp(1, 30));
    }
    Ok(result)
}
/// Fixed tip baselines stay on the chosen route/size/fee. B3 is absent without a fresh tape median.
pub fn baselines(
    samples: &[TrainingCanary],
    request: &ModelQuoteRequest,
    models: &[ModelFit],
    chosen: Option<&CanaryConfig>,
    tape: &[TipTapeObservation],
    frozen: Option<(&str, &[ModelFit])>,
) -> Result<Vec<BaselineForecast>, ModelError> {
    let anchor = chosen
        .or_else(|| request.candidates.first())
        .ok_or(ModelError::Invalid)?;
    let horizon = match request.target {
        PredictionTarget::Probability { horizon_slots, .. } => horizon_slots,
        _ => 4,
    };
    let tape_summary = tape_median(tape, &request.context)?;
    let mut results = Vec::new();
    for (id, tip) in [
        (
            "B1",
            Some(if anchor.route == Route::Rpc {
                0
            } else {
                100_000
            }),
        ),
        (
            "B2",
            Some(if anchor.route == Route::Rpc {
                0
            } else {
                500_000
            }),
        ),
        ("B3", tape_summary.as_ref().map(|t| t.median_lamports)),
    ] {
        let Some(tip) = tip else {
            results.push(BaselineForecast {
                id: id.into(),
                tape: None,
                config: None,
                p_hat: None,
                model_snapshot_hash: None,
                unavailable_reason: Some("no_fresh_five_minute_tape_median".into()),
            });
            continue;
        };
        let mut config = anchor.clone();
        config.tip_lamports = tip;
        config.tip_tier = match tip {
            0 => TipTier::None,
            100_000 => TipTier::X1,
            200_000 => TipTier::X2,
            500_000 => TipTier::X5,
            _ => TipTier::X10,
        };
        let model = models.iter().find(|m| m.horizon_slots == horizon);
        let p = pooled::predict(
            samples,
            model,
            &config,
            &request.context,
            horizon,
            &request.leader_class_next,
            &request.covariates,
        )?;
        results.push(BaselineForecast {
            id: id.into(),
            tape: (id == "B3").then(|| tape_summary.clone()).flatten(),
            config: Some(config),
            p_hat: (p.evidence != Evidence::Insufficient).then_some(p.p_hat),
            model_snapshot_hash: None,
            unavailable_reason: (p.evidence == Evidence::Insufficient)
                .then(|| "insufficient_baseline_evidence".into()),
        });
    }
    let b4 = if let Some((hash, models)) = frozen {
        if let Some(m) = models.iter().find(|m| {
            m.horizon_slots == horizon
                && m.context.source == request.context.source
                && utc(&m.context.as_of_utc)
                    .is_ok_and(|then| utc(&request.context.as_of_utc).is_ok_and(|now| then <= now))
        }) {
            BaselineForecast {
                id: "B4".into(),
                tape: None,
                config: Some(anchor.clone()),
                p_hat: Some(
                    pooled::probability(
                        m,
                        anchor,
                        &request.leader_class_next,
                        &request.covariates,
                    )?
                    .0,
                ),
                model_snapshot_hash: Some(hash.into()),
                unavailable_reason: None,
            }
        } else {
            BaselineForecast {
                id: "B4".into(),
                tape: None,
                config: None,
                p_hat: None,
                model_snapshot_hash: Some(hash.into()),
                unavailable_reason: Some("frozen_horizon_unavailable".into()),
            }
        }
    } else {
        BaselineForecast {
            id: "B4".into(),
            tape: None,
            config: None,
            p_hat: None,
            model_snapshot_hash: None,
            unavailable_reason: Some("no_frozen_model".into()),
        }
    };
    results.push(b4);
    Ok(results)
}

/// Median of distinct same-source tape transactions observed in the preceding five minutes.
pub fn tape_median(
    tape: &[TipTapeObservation],
    context: &CurveContext,
) -> Result<Option<TipTapeSummary>, ModelError> {
    if tape.len() > 100_000 {
        return Err(ModelError::Invalid);
    }
    let end = utc(&context.as_of_utc)?;
    let start = end - chrono::Duration::minutes(5);
    let mut ids = std::collections::BTreeSet::new();
    let mut tips = Vec::new();
    for row in tape.iter().filter(|r| r.source == context.source) {
        let time = utc(&row.observed_at_utc)?;
        if time < start || time > end {
            continue;
        }
        if row.id.is_empty() || !ids.insert(&row.id) {
            return Err(ModelError::Duplicate);
        }
        tips.push(row.tip_lamports);
    }
    if tips.is_empty() {
        return Ok(None);
    }
    tips.sort_unstable();
    let n = tips.len();
    let median = ((u128::from(tips[(n - 1) / 2]) + u128::from(tips[n / 2])) / 2) as u64;
    Ok(Some(TipTapeSummary {
        window: [start.to_rfc3339(), end.to_rfc3339()],
        samples: n as u32,
        median_lamports: median,
    }))
}
