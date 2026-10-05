use crate::{compute, issue_inner, utc};
use alight_model::{latency, pooled, quote};
use alight_store::{Store, StoreError};
use alight_types::*;
use std::collections::BTreeMap;

/// Freezes a same-source model/economics response in the immutable ledger; sends nothing.
pub async fn issue_combined(
    store: &Store,
    request: QuoteServiceRequest,
) -> Result<ForecastEntry, StoreError> {
    let tape = read_tape(store, &request.model.context).await?;
    issue_inner(
        store,
        request.model,
        request.ttl_s,
        request.frozen_model_hash.as_deref(),
        &tape,
        request.economics.as_ref(),
    )
    .await
}

/// Computes a complete conditional quote without saving models, forecasts or market data.
pub async fn preview_combined(
    store: &Store,
    request: QuoteServiceRequest,
) -> Result<QuotePreview, StoreError> {
    if !(1..=86400).contains(&request.ttl_s) {
        return Err(StoreError::Invalid);
    }
    let tape = read_tape(store, &request.model.context).await?;
    let (samples, models, response) = compute(store, &request.model).await?;
    let frozen = if let Some(hash) = &request.frozen_model_hash {
        Some(store.load_models(hash).await?)
    } else {
        None
    };
    let (economics, baselines) = if let Some(inputs) = &request.economics {
        let (e, b) = combine(
            store,
            &samples,
            &request.model,
            &models,
            &response,
            inputs,
            &tape,
            request.frozen_model_hash.as_deref().zip(frozen.as_deref()),
        )
        .await?;
        (Some(e), b)
    } else {
        (
            None,
            quote::baselines(
                &samples,
                &request.model,
                &models,
                response.recommendation.as_ref().map(|r| &r.config),
                &tape,
                request.frozen_model_hash.as_deref().zip(frozen.as_deref()),
            )
            .map_err(|_| StoreError::Invalid)?,
        )
    };
    Ok(QuotePreview {
        quote: economics
            .as_ref()
            .map_or(response.clone(), |e| e.model_quote.clone()),
        economics,
        baselines,
        requested_economics: request.economics,
        model_snapshot_hash: alight_store::content_hash(&models)?,
    })
}

async fn read_tape(
    store: &Store,
    context: &CurveContext,
) -> Result<Vec<TipTapeObservation>, StoreError> {
    let end = &context.as_of_utc;
    let start = (utc(end)? - chrono::Duration::minutes(5)).to_rfc3339();
    let rows = store
        .passive_tips(context.source, &start, end, 10_000)
        .await?;
    // One total per signature. Ambiguous forks, unknown payments and observer conflicts
    // exclude the transaction from B3 while remaining in the passive evidence table.
    type Group = (u64, Option<String>, String, BTreeMap<String, u64>, bool);
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for row in rows {
        let group = groups.entry(row.signature).or_insert_with(|| {
            (
                row.slot,
                row.block_id.clone(),
                row.received.wall_utc.clone(),
                BTreeMap::new(),
                false,
            )
        });
        if group.0 != row.slot
            || group.1 != row.block_id
            || !row.success
            || row.tip_lamports.is_none()
        {
            group.4 = true;
        }
        if utc(&row.received.wall_utc)? < utc(&group.2)? {
            group.2 = row.received.wall_utc;
        }
        if let Some(amount) = row.tip_lamports
            && group
                .3
                .insert(row.recipient, amount)
                .is_some_and(|old| old != amount)
        {
            group.4 = true;
        }
    }
    let mut tape = Vec::new();
    for (signature, (_, _, observed, recipients, excluded)) in groups {
        if excluded {
            continue;
        }
        let tip = recipients.values().try_fold(0u64, |sum, amount| {
            sum.checked_add(*amount).ok_or(StoreError::Invalid)
        })?;
        if tip == 0 {
            continue;
        }
        tape.push(TipTapeObservation {
            id: signature,
            source: context.source,
            observed_at_utc: observed,
            tip_lamports: tip,
        });
    }
    Ok(tape)
}

fn candidate(
    samples: &[TrainingCanary],
    request: &ModelQuoteRequest,
    models: &[ModelFit],
    config: &CanaryConfig,
) -> Result<Option<EconomicCandidate>, StoreError> {
    let horizon = match request.target {
        PredictionTarget::Probability { horizon_slots, .. } => horizon_slots,
        _ => 4,
    };
    let model = models.iter().find(|m| m.horizon_slots == horizon);
    let prediction = pooled::predict(
        samples,
        model,
        config,
        &request.context,
        horizon,
        &request.leader_class_next,
        &request.covariates,
    )
    .map_err(|_| StoreError::Invalid)?;
    let Some(distribution) =
        latency::landing_distribution(samples, config, &request.context, horizon, prediction.p_hat)
            .map_err(|_| StoreError::Invalid)?
    else {
        return Ok(None);
    };
    let latency = if let PredictionTarget::LatencyQuantile { quantile, .. } = request.target {
        Some(
            latency::estimate_quantile(samples, config, &request.context, quantile, 42)
                .map_err(|_| StoreError::Invalid)?,
        )
    } else {
        None
    };
    Ok(Some(EconomicCandidate {
        prediction,
        distribution,
        latency,
    }))
}

fn economic_baselines(
    samples: &[TrainingCanary],
    request: &ModelQuoteRequest,
    models: &[ModelFit],
    baselines: &[BaselineForecast],
    frozen: Option<(&str, &[ModelFit])>,
) -> Result<Vec<EconomicBaseline>, StoreError> {
    let mut result = Vec::new();
    for baseline in baselines {
        let mut c = baseline
            .config
            .as_ref()
            .map(|config| candidate(samples, request, models, config))
            .transpose()?
            .flatten();
        if baseline.id == "B4" {
            // Frozen probability with the current exact-cell conditional delay shape.
            c = if let (Some(mut c), Some((_, frozen)), Some(p)) = (c, frozen, baseline.p_hat) {
                let horizon = match request.target {
                    PredictionTarget::Probability { horizon_slots, .. } => horizon_slots,
                    _ => 4,
                };
                let fit = frozen
                    .iter()
                    .find(|m| m.horizon_slots == horizon)
                    .ok_or(StoreError::Invalid)?;
                let (_, interval) = pooled::probability(
                    fit,
                    &c.prediction.config,
                    &request.leader_class_next,
                    &request.covariates,
                )
                .map_err(|_| StoreError::Invalid)?;
                c.prediction.p_hat = p;
                c.prediction.p_interval_95 = interval;
                c.distribution = latency::landing_distribution(
                    samples,
                    &c.prediction.config,
                    &request.context,
                    horizon,
                    p,
                )
                .map_err(|_| StoreError::Invalid)?
                .ok_or(StoreError::Invalid)?;
                Some(c)
            } else {
                None
            };
        }
        result.push(EconomicBaseline {
            id: baseline.id.clone(),
            candidate: c,
            unavailable_reason: baseline.unavailable_reason.clone(),
        });
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn combine(
    store: &Store,
    samples: &[TrainingCanary],
    request: &ModelQuoteRequest,
    models: &[ModelFit],
    model_quote: &ModelQuote,
    inputs: &EconomicsInputs,
    tape: &[TipTapeObservation],
    frozen: Option<(&str, &[ModelFit])>,
) -> Result<(EconomicsQuote, Vec<BaselineForecast>), StoreError> {
    // Invalid user inputs are distinct from stale or absent market evidence.
    if inputs.pool.is_empty()
        || inputs
            .size_usd
            .parse::<f64>()
            .ok()
            .is_none_or(|n| !n.is_finite() || n <= 0.0)
        || inputs
            .sol_usd
            .parse::<f64>()
            .ok()
            .is_none_or(|n| !n.is_finite() || n <= 0.0)
        || !inputs.edge_bps.is_finite()
        || inputs.edge_bps < 0.0
        || !inputs.lambda.is_finite()
        || inputs.lambda < 0.0
    {
        return Err(StoreError::Invalid);
    }
    let baselines_for = |response: &ModelQuote| {
        quote::baselines(
            samples,
            request,
            models,
            response.recommendation.as_ref().map(|r| &r.config),
            tape,
            frozen,
        )
        .map_err(|_| StoreError::Invalid)
    };
    let fallback = |reason: &str| -> Result<_, StoreError> {
        Ok((
            EconomicsQuote {
                model_quote: model_quote.clone(),
                economics: None,
                fallback_reason: Some(reason.into()),
            },
            baselines_for(model_quote)?,
        ))
    };
    let Some(market) = store
        .market_snapshot(
            request.context.source,
            &request.context.regime_id,
            &inputs.pool,
            &request.context.as_of_utc,
        )
        .await?
    else {
        return fallback("market_snapshot_unavailable");
    };
    let early = alight_econ::optimizer::optimize(model_quote, inputs, &market, &[], &[])
        .map_err(|_| StoreError::Invalid)?;
    if early
        .fallback_reason
        .as_deref()
        .is_some_and(|r| r.starts_with("blur_") || r == "model_evidence_insufficient")
    {
        return Ok((early, baselines_for(model_quote)?));
    }
    let mut candidates = Vec::new();
    for config in &request.candidates {
        let Some(c) = candidate(samples, request, models, config)? else {
            return fallback("candidate_latency_or_delay_evidence_insufficient");
        };
        candidates.push(c);
    }
    let preliminary =
        alight_econ::optimizer::optimize(model_quote, inputs, &market, &candidates, &[])
            .map_err(|_| StoreError::Invalid)?;
    let baselines = baselines_for(&preliminary.model_quote)?;
    let economic_baselines = economic_baselines(samples, request, models, &baselines, frozen)?;
    let mut result = alight_econ::optimizer::optimize(
        model_quote,
        inputs,
        &market,
        &candidates,
        &economic_baselines,
    )
    .map_err(|_| StoreError::Invalid)?;
    if let Some(summary) = &mut result.economics {
        summary.assumption.push_str(" Delay-bin shape is decayed exact-cell empirical evidence projected onto the quoted horizon probability; unobserved residual tail mass is treated as nonlanding. Passive B3 is a descriptive median of positive known paid transfers, excluding observer conflicts and ambiguous forks, with no inferred transport route.");
    }
    Ok((result, baselines))
}
