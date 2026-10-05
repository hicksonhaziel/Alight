//! Cost = nominal fees/tip + unconditional delay loss + missed-horizon edge.
use crate::{EconError, positive_decimal, utc};
use alight_types::*;
use std::collections::BTreeSet;

const MASS_TOLERANCE: f64 = 1e-8;

fn finite_nonnegative(n: f64) -> bool {
    n.is_finite() && n >= 0.0
}
fn probability(n: f64) -> bool {
    n.is_finite() && (0.0..=1.0).contains(&n)
}
fn usd(value: f64) -> Result<String, EconError> {
    if !value.is_finite() {
        return Err(EconError::Invalid);
    }
    // Round-trip text avoids rounding tiny fee differences away before candidate ordering.
    Ok(value.to_string())
}
fn objective(cost: &EconomicCost, upper: bool) -> Result<f64, EconError> {
    (if upper {
        &cost.upper_cost_usd
    } else {
        &cost.expected_cost_usd
    })
    .parse()
    .map_err(|_| EconError::Invalid)
}
fn context_matches(a: &CurveContext, b: &CurveContext) -> bool {
    a.source == b.source
        && a.regime_id == b.regime_id
        && a.region == b.region
        && a.as_of_utc == b.as_of_utc
}
fn validate_quote(quote: &ModelQuote) -> Result<(), EconError> {
    utc(&quote.context.as_of_utc)?;
    if quote.context.region.is_empty() || quote.context.regime_id.is_empty() {
        return Err(EconError::Invalid);
    }
    match quote.target {
        PredictionTarget::Probability {
            target_p,
            horizon_slots,
        } if probability(target_p) && target_p > 0.0 && horizon_slots > 0 => Ok(()),
        PredictionTarget::LatencyQuantile {
            quantile,
            max_slots,
            max_ms,
        } if probability(quantile)
            && quantile > 0.0
            && quantile < 1.0
            && max_slots.is_some() != max_ms.is_some()
            && max_slots
                .or(max_ms)
                .is_some_and(|n| n.is_finite() && n > 0.0) =>
        {
            Ok(())
        }
        _ => Err(EconError::Invalid),
    }
}
fn evaluate(
    candidate: &EconomicCandidate,
    inputs: &EconomicsInputs,
    market: &DelayCostSnapshot,
    quote: &ModelQuote,
    size: SizeClass,
) -> Result<Option<EconomicCost>, EconError> {
    let p = &candidate.prediction;
    let d = &candidate.distribution;
    if !context_matches(&d.context, &quote.context)
        || p.config.size_class != size
        || p.config.cu_limit == 0
        || (p.config.route == Route::Rpc && p.config.tip_lamports != 0)
        || !probability(p.p_hat)
        || !p.p_interval_95.iter().all(|n| probability(*n))
        || p.p_interval_95[0] > p.p_hat
        || p.p_interval_95[1] < p.p_hat
        || !probability(p.unresolved_share)
        || !probability(d.nonlanding_probability)
        || !finite_nonnegative(p.n_effective)
        || !finite_nonnegative(d.n_effective)
        || p.data_age_s.is_some_and(|n| !finite_nonnegative(n))
        || !finite_nonnegative(d.data_age_s)
        || d.masses.len() > 1000
    {
        return Err(EconError::Invalid);
    }
    let mut delays = BTreeSet::new();
    if d.masses
        .iter()
        .any(|m| !probability(m.probability) || !delays.insert(m.delay_slots))
    {
        return Err(EconError::Invalid);
    }
    let total = d.masses.iter().map(|m| m.probability).sum::<f64>() + d.nonlanding_probability;
    if (total - 1.0).abs() > MASS_TOLERANCE {
        return Err(EconError::Invalid);
    }
    let horizon = match quote.target {
        PredictionTarget::Probability { horizon_slots, .. } => horizon_slots,
        _ => 4,
    };
    let within = d
        .masses
        .iter()
        .filter(|m| m.delay_slots <= horizon)
        .map(|m| m.probability)
        .sum::<f64>();
    if (within - p.p_hat).abs() > MASS_TOLERANCE {
        return Err(EconError::Invalid);
    }
    if !matches!(p.evidence, Evidence::Measured | Evidence::Interpolated)
        || p.n_effective < alight_model::MIN_EFFECTIVE_N
        || d.n_effective < alight_model::MIN_EFFECTIVE_N
        || p.data_age_s.is_none_or(|n| n > alight_model::MAX_AGE_S)
        || d.data_age_s > alight_model::MAX_AGE_S
        || p.unresolved_share > alight_model::MAX_UNRESOLVED_SHARE
    {
        return Ok(None);
    }
    let qualifies = match quote.target {
        PredictionTarget::Probability { target_p, .. } => p.p_interval_95[0] >= target_p,
        PredictionTarget::LatencyQuantile {
            quantile,
            max_slots,
            max_ms,
        } => candidate.latency.as_ref().is_some_and(|q| {
            q.evidence == Evidence::Measured
                && q.quantile == quantile
                && q.n_effective >= alight_model::MIN_EFFECTIVE_N
                && if let Some(max) = max_slots {
                    q.slots_interval_95[1].is_some_and(|n| n.is_finite() && n >= 0.0 && n <= max)
                } else {
                    q.ms_interval_95[1]
                        .zip(max_ms)
                        .is_some_and(|(n, max)| n.is_finite() && n >= 0.0 && n <= max)
                }
        }),
    };
    let mut central = 0.0;
    let mut upper = 0.0;
    for mass in d
        .masses
        .iter()
        .filter(|m| m.probability > 0.0 && m.delay_slots > 0)
    {
        let Some(point) = market
            .points
            .iter()
            .find(|p| p.delay_slots == mass.delay_slots)
        else {
            return Ok(None);
        };
        let (Some(median), Some(high)) = (point.median_bps, point.upper_bps) else {
            return Ok(None);
        };
        if point.pairs < market.minimum_pairs {
            return Ok(None);
        }
        central += mass.probability * median;
        upper += mass.probability * high;
    }
    let size_usd = positive_decimal(&inputs.size_usd)?;
    let sol_usd = positive_decimal(&inputs.sol_usd)?;
    let priority = (u128::from(p.config.cu_price_micro_lamports) * u128::from(p.config.cu_limit))
        .div_ceil(1_000_000);
    let nominal =
        u128::from(p.config.tip_lamports) + u128::from(inputs.base_fee_lamports) + priority;
    let nominal_lamports = u64::try_from(nominal).map_err(|_| EconError::Invalid)?;
    let spend = nominal_lamports as f64 / 1_000_000_000.0 * sol_usd;
    let delay = size_usd * inputs.lambda * central / 10_000.0;
    let high_delay = size_usd * inputs.lambda * upper / 10_000.0;
    let missed = (1.0 - p.p_hat) * size_usd * inputs.edge_bps / 10_000.0;
    Ok(Some(EconomicCost {
        prediction: p.clone(),
        latency: candidate.latency.clone(),
        qualifies,
        nominal_lamports,
        nominal_spend_usd: usd(spend)?,
        delay_cost_usd: usd(delay)?,
        upper_delay_cost_usd: usd(high_delay)?,
        missed_edge_usd: usd(missed)?,
        expected_cost_usd: usd(spend + delay + missed)?,
        upper_cost_usd: usd(spend + high_delay + missed)?,
    }))
}
fn validate_market(market: &DelayCostSnapshot) -> Result<(), EconError> {
    utc(&market.as_of_utc)?;
    if market.pool.is_empty()
        || market.mint.is_empty()
        || market.regime_id.is_empty()
        || market.window_s == 0
        || market.minimum_pairs < 2
        || market.points.len() > 1000
        || !market.measured_slot_ms.is_finite()
        || market.measured_slot_ms <= 0.0
        || !market.max_age_s.is_finite()
        || market.max_age_s <= 0.0
        || !market.upper_quantile.is_finite()
        || !(0.5..1.0).contains(&market.upper_quantile)
        || market.data_age_s.is_some_and(|n| !finite_nonnegative(n))
    {
        return Err(EconError::Invalid);
    }
    let mut delays = BTreeSet::new();
    for point in &market.points {
        if !delays.insert(point.delay_slots)
            || !finite_nonnegative(point.delay_ms)
            || (point.delay_ms - f64::from(point.delay_slots) * market.measured_slot_ms).abs()
                > 1e-6
            || point.median_bps.is_some() != point.upper_bps.is_some()
            || point.median_bps.is_some_and(|n| !finite_nonnegative(n))
            || point.upper_bps.is_some_and(|n| !finite_nonnegative(n))
            || point
                .median_bps
                .zip(point.upper_bps)
                .is_some_and(|(a, b)| a > b)
            || point
                .split_half_relative_change
                .is_some_and(|n| !probability(n))
        {
            return Err(EconError::Invalid);
        }
    }
    Ok(())
}

/// Optimize only supported configurations; the model's probability/latency evidence gates persist.
/// Costs are USD estimates, nominal fees use ceiling(CU × micro-lamports/CU / 1e6).
pub fn optimize(
    quote: &ModelQuote,
    inputs: &EconomicsInputs,
    market: &DelayCostSnapshot,
    candidates: &[EconomicCandidate],
    baselines: &[EconomicBaseline],
) -> Result<EconomicsQuote, EconError> {
    validate_quote(quote)?;
    positive_decimal(&inputs.size_usd)?;
    positive_decimal(&inputs.sol_usd)?;
    if inputs.pool.is_empty()
        || !finite_nonnegative(inputs.edge_bps)
        || !finite_nonnegative(inputs.lambda)
        || candidates.len() > 1000
        || baselines.len() > 100
    {
        return Err(EconError::Invalid);
    }
    validate_market(market)?;
    let fallback = |reason: &str| EconomicsQuote {
        model_quote: quote.clone(),
        economics: None,
        fallback_reason: Some(reason.into()),
    };
    if market.source != quote.context.source
        || market.regime_id != quote.context.regime_id
        || market.pool != inputs.pool
    {
        return Ok(fallback("market_scope_mismatch"));
    }
    let elapsed = (utc(&quote.context.as_of_utc)? - utc(&market.as_of_utc)?).num_milliseconds()
        as f64
        / 1000.0;
    if elapsed < 0.0 {
        return Ok(fallback("market_snapshot_from_future"));
    }
    if market.stale
        || market.disconnected
        || market
            .data_age_s
            .is_none_or(|age| age + elapsed > market.max_age_s)
    {
        return Ok(fallback("blur_stale_or_disconnected"));
    }
    if market.sparse || market.slot_samples < 2 {
        return Ok(fallback("blur_sparse"));
    }
    let Some(size) = quote.recommendation.as_ref().map(|p| p.config.size_class) else {
        return Ok(fallback("model_evidence_insufficient"));
    };
    let mut configurations = BTreeSet::new();
    let mut evaluated = Vec::new();
    let mut incomplete = false;
    for candidate in candidates {
        let key =
            serde_json::to_string(&candidate.prediction.config).map_err(|_| EconError::Invalid)?;
        if !configurations.insert(key) {
            return Err(EconError::Invalid);
        }
        if let Some(cost) = evaluate(candidate, inputs, market, quote, size)? {
            evaluated.push(cost);
        } else {
            incomplete = true;
        }
    }
    // Skipping a configuration with missing evidence could fabricate the optimum.
    if incomplete {
        return Ok(fallback("candidate_latency_or_delay_evidence_insufficient"));
    }
    let mut objectives = Vec::with_capacity(evaluated.len());
    for cost in &evaluated {
        objectives.push(objective(cost, inputs.use_upper_quantile)?);
    }
    let chosen = evaluated
        .iter()
        .enumerate()
        .filter(|(_, c)| c.qualifies)
        .min_by(|(a, _), (b, _)| objectives[*a].total_cmp(&objectives[*b]).then(a.cmp(b)))
        .map(|(i, _)| i);
    let Some(chosen) = chosen else {
        return Ok(fallback("no_supported_economic_candidate_meets_target"));
    };
    let recommendation = evaluated[chosen].clone();
    let mut frontier = Vec::new();
    for (i, cost) in evaluated.iter().enumerate() {
        let dominated = evaluated.iter().enumerate().any(|(j, other)| {
            i != j
                && objectives[j] <= objectives[i]
                && other.prediction.p_hat >= cost.prediction.p_hat
                && (objectives[j] < objectives[i]
                    || other.prediction.p_hat > cost.prediction.p_hat
                    || j < i)
        });
        if !dominated {
            frontier.push(cost.clone());
        }
    }
    frontier.sort_by(|a, b| a.prediction.p_hat.total_cmp(&b.prediction.p_hat));
    let knee = knee(&frontier, inputs.use_upper_quantile)?;
    let mut comparisons = Vec::new();
    let mut baseline_ids = BTreeSet::new();
    for baseline in baselines {
        if baseline.id.is_empty() || !baseline_ids.insert(&baseline.id) {
            return Err(EconError::Invalid);
        }
        let cost = baseline
            .candidate
            .as_ref()
            .map(|c| evaluate(c, inputs, market, quote, size))
            .transpose()?
            .flatten();
        let savings = cost
            .as_ref()
            .map(|cost| usd(objective(cost, inputs.use_upper_quantile)? - objectives[chosen]))
            .transpose()?;
        let unavailable_reason = if cost.is_none() {
            Some(
                baseline
                    .unavailable_reason
                    .clone()
                    .unwrap_or_else(|| "baseline_evidence_unavailable".into()),
            )
        } else {
            None
        };
        comparisons.push(BaselineCostComparison {
            id: baseline.id.clone(),
            cost,
            savings_usd: savings,
            unavailable_reason,
        });
    }
    let mut model_quote = quote.clone();
    model_quote.recommendation = Some(recommendation.prediction.clone());
    model_quote.latency = recommendation.latency.clone();
    model_quote.evidence = recommendation.prediction.evidence;
    Ok(EconomicsQuote {
        model_quote, fallback_reason: None,
        economics: Some(EconomicsSummary {
            inputs: inputs.clone(), market: market.clone(),
            assumption: "Conditional estimate: your transaction behaves like this canary size class on this route, region and window. Nominal tip/base/priority fees are charged upfront in the objective; SOL/USD and pool price movement are supplied estimates. Slot delays use the measured regime clock; overlapping price pairs are not independent.".into(),
            evaluated, frontier, knee, recommendation, baselines: comparisons,
        }),
    })
}
/// Maximum perpendicular separation from the normalized cost/probability chord; no knee for <3 points.
fn knee(frontier: &[EconomicCost], upper: bool) -> Result<Option<CanaryConfig>, EconError> {
    if frontier.len() < 3 {
        return Ok(None);
    }
    let first = frontier.first().ok_or(EconError::Invalid)?;
    let last = frontier.last().ok_or(EconError::Invalid)?;
    let p_span = last.prediction.p_hat - first.prediction.p_hat;
    let low = objective(first, upper)?;
    let cost_span = objective(last, upper)? - low;
    if p_span <= 0.0 || cost_span <= 0.0 {
        return Ok(None);
    }
    let mut best = None;
    let mut distance = 1e-9;
    for point in frontier.iter().skip(1).take(frontier.len() - 2) {
        let x = (point.prediction.p_hat - first.prediction.p_hat) / p_span;
        let y = (objective(point, upper)? - low) / cost_span;
        let d = (x - y).abs();
        if d > distance {
            distance = d;
            best = Some(point.prediction.config.clone());
        }
    }
    Ok(best)
}
