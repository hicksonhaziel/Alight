//! Monotone route GLM with shrinkage of leader/congestion deviations; temporal M2 blend.
use crate::{
    ModelError, estimate, label,
    regression::{self, Row},
    utc,
};
use alight_types::*;
use std::collections::{BTreeMap, BTreeSet};

const DIM: usize = 26;
/// Exploration beliefs group logged bucket/tier cells; forecast cells still use exact numeric fees.
pub fn policy_posteriors(
    samples: &[TrainingCanary],
    cells: &[CanaryConfig],
    context: &CurveContext,
    horizon: u32,
) -> Result<Vec<(f64, f64)>, ModelError> {
    let mut result = vec![(1.0, 1.0); cells.len()];
    for sample in samples {
        if let Some((y, w)) = label(sample, context, horizon)?
            && let Some(i) = cells.iter().position(|c| {
                c.route == sample.canary.config.route
                    && c.fee_bucket == sample.canary.config.fee_bucket
                    && c.tip_tier == sample.canary.config.tip_tier
                    && c.size_class == sample.canary.config.size_class
                    && c.cu_limit == sample.canary.config.cu_limit
            })
        {
            result[i].0 += w * y;
            result[i].1 += w * (1.0 - y);
        }
    }
    Ok(result)
}
pub fn features(
    config: &CanaryConfig,
    leaders: &[LeaderClass],
    covariates: &ModelCovariates,
) -> Result<Vec<f64>, ModelError> {
    if covariates
        .congestion
        .is_some_and(|v| !v.is_finite() || !(0.0..=5.0).contains(&v))
    {
        return Err(ModelError::Invalid);
    }
    let route = match config.route {
        Route::BeamQuic => 0,
        Route::BeamHttp => 1,
        Route::Rpc => 2,
    };
    let mut x = vec![0.0; DIM];
    x[route] = 1.0;
    x[3 + route] = (config.tip_lamports as f64 / 100_000.0).max(1.0).ln();
    x[6] = (config.cu_price_micro_lamports as f64 / 1000.0).ln_1p();
    x[7] = f64::from(config.size_class == SizeClass::Medium);
    x[8] = f64::from(config.size_class == SizeClass::Large);
    let stake = leaders
        .first()
        .map_or(Tercile::Unknown, |l| l.stake_tercile);
    let skip = leaders
        .first()
        .map_or(Tercile::Unknown, |l| l.skip_rate_tercile);
    for (tercile, start) in [(stake, 9), (skip, 12)] {
        match tercile {
            Tercile::Low => x[start] = 1.0,
            Tercile::High => x[start + 1] = 1.0,
            Tercile::Unknown => x[start + 2] = 1.0,
            Tercile::Middle => {}
        }
    }
    if let Some(c) = covariates.congestion {
        x[15 + route] = c;
        x[18] = c;
    } else {
        x[19] = 1.0;
    }
    x[20 + route * 2] = f64::from(stake == Tercile::Low);
    x[21 + route * 2] = f64::from(stake == Tercile::High);
    Ok(x)
}
fn key(config: &CanaryConfig) -> Result<String, ModelError> {
    serde_json::to_string(config).map_err(|_| ModelError::Invalid)
}

pub fn fit(
    samples: &[TrainingCanary],
    context: &CurveContext,
    horizon_slots: u32,
) -> Result<ModelFit, ModelError> {
    if horizon_slots == 0 {
        return Err(ModelError::Invalid);
    }
    let mut rows = Vec::new();
    let mut support = BTreeMap::new();
    let mut ids = BTreeSet::new();
    let mut latest = None;
    for sample in samples {
        if let Some((y, weight)) = label(sample, context, horizon_slots)? {
            if !ids.insert(&sample.canary.id) {
                return Err(ModelError::Duplicate);
            }
            let c = &sample.canary;
            rows.push(Row {
                x: features(&c.config, &c.leader_class_next, &sample.covariates)?,
                successes: y * weight,
                trials: weight,
            });
            support.insert(key(&c.config)?, c.config.clone());
            let sent = utc(&c.send_wall_utc)?;
            latest = Some(latest.map_or(sent, |t: chrono::DateTime<chrono::Utc>| t.max(sent)));
        }
    }
    let mut ridge = vec![2.0; DIM];
    ridge[..3].fill(0.1);
    ridge[3..9].fill(0.5);
    ridge[15..18].fill(8.0);
    ridge[20..].fill(12.0);
    let fitted = if rows.is_empty() {
        regression::Fit {
            coefficients: vec![0.0; DIM],
            covariance: (0..DIM)
                .map(|i| {
                    (0..DIM)
                        .map(|j| if i == j { 1.0 / ridge[i] } else { 0.0 })
                        .collect()
                })
                .collect(),
        }
    } else {
        regression::fit(&rows, &ridge, &[3, 4, 5, 6])?
    };
    Ok(ModelFit {
        context: context.clone(),
        horizon_slots,
        coefficients: fitted.coefficients,
        covariance: fitted.covariance,
        n_effective: rows.iter().map(|r| r.trials).sum(),
        observations: rows.len() as u32,
        latest_send_utc: latest.map(|t| t.to_rfc3339()),
        support: support.into_values().collect(),
        holdout_m0_log_loss: None,
        holdout_m1_log_loss: None,
        m1_weight: 0.0,
    })
}
pub fn probability(
    model: &ModelFit,
    config: &CanaryConfig,
    leaders: &[LeaderClass],
    covariates: &ModelCovariates,
) -> Result<(f64, [f64; 2]), ModelError> {
    if model.coefficients.len() != DIM
        || model.covariance.len() != DIM
        || model.covariance.iter().any(|r| r.len() != DIM)
    {
        return Err(ModelError::Invalid);
    }
    let x = features(config, leaders, covariates)?;
    let z = regression::dot(&x, &model.coefficients);
    let variance = x
        .iter()
        .enumerate()
        .map(|(i, a)| a * regression::dot(&model.covariance[i], &x))
        .sum::<f64>()
        .max(0.0);
    let margin = 1.96 * variance.sqrt();
    Ok((
        regression::sigmoid(z),
        [
            regression::sigmoid(z - margin),
            regression::sigmoid(z + margin),
        ],
    ))
}
/// Selects blend weight using an earlier model and strictly later held-out sends.
/// The final fitted coefficients may use those outcomes only after this prequential evaluation.
pub fn fit_blend(
    samples: &[TrainingCanary],
    context: &CurveContext,
    horizon: u32,
) -> Result<ModelFit, ModelError> {
    let mut eligible: Vec<_> = samples
        .iter()
        .filter_map(|s| match label(s, context, horizon) {
            Ok(Some(_)) => Some(Ok(s.clone())),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        })
        .collect::<Result<_, _>>()?;
    eligible.sort_by_key(|s| s.canary.send_wall_utc.clone());
    let split = eligible.len() * 4 / 5;
    if split < 200 || eligible.len() - split < 100 {
        return fit(samples, context, horizon);
    }
    let mut earlier = context.clone();
    earlier.as_of_utc = eligible[split].canary.send_wall_utc.clone();
    let early = fit(&eligible[..split], &earlier, horizon)?;
    let mut cells = BTreeMap::new();
    let (mut m0, mut m1, mut count) = (0.0, 0.0, 0u32);
    for s in &eligible[split..] {
        if utc(&s.canary.send_wall_utc)? <= utc(&earlier.as_of_utc)? {
            continue;
        }
        let id = key(&s.canary.config)?;
        if !cells.contains_key(&id) {
            cells.insert(
                id.clone(),
                estimate(&eligible[..split], &s.canary.config, &earlier, horizon)?,
            );
        }
        let p0 = cells[&id].p_hat;
        let p1 = probability(
            &early,
            &s.canary.config,
            &s.canary.leader_class_next,
            &s.covariates,
        )?
        .0;
        let y = label(s, context, horizon)?.ok_or(ModelError::Invalid)?.0;
        m0 += crate::scoring::log_loss(p0, y);
        m1 += crate::scoring::log_loss(p1, y);
        count += 1;
    }
    let mut final_fit = fit(samples, context, horizon)?;
    if count > 0 {
        m0 /= f64::from(count);
        m1 /= f64::from(count);
        final_fit.holdout_m0_log_loss = Some(m0);
        final_fit.holdout_m1_log_loss = Some(m1);
        final_fit.m1_weight = regression::sigmoid(50.0 * (m0 - m1));
    }
    Ok(final_fit)
}

/// M2 falls back to exact M0 unless the pooled fit has enough fresh comparable support.
pub fn predict(
    samples: &[TrainingCanary],
    model: Option<&ModelFit>,
    config: &CanaryConfig,
    context: &CurveContext,
    horizon: u32,
    leaders: &[LeaderClass],
    covariates: &ModelCovariates,
) -> Result<CurvePrediction, ModelError> {
    let cell = estimate(samples, config, context, horizon)?;
    let mut result = CurvePrediction {
        config: config.clone(),
        p_hat: cell.p_hat,
        p_interval_95: cell.p_interval_95,
        evidence: cell.evidence,
        n_effective: cell.n_effective,
        data_age_s: cell.data_age_s,
        observation_window: cell.observation_window,
        unresolved_share: cell.unresolved_share,
        samples_needed: cell.samples_needed,
        m1_weight: 0.0,
    };
    let Some(model) = model else {
        return Ok(result);
    };
    if model.context.source != context.source
        || model.context.regime_id != context.regime_id
        || model.horizon_slots != horizon
        || utc(&model.context.as_of_utc)? > utc(&context.as_of_utc)?
    {
        return Err(ModelError::Invalid);
    }
    let relevant: Vec<_> = samples
        .iter()
        .filter(|s| {
            s.canary.config.route == config.route
                && s.canary.config.size_class == config.size_class
                && s.canary.config.cu_limit == config.cu_limit
        })
        .cloned()
        .collect();
    let mut mass = 0.0;
    let mut latest = None;
    let mut first = None;
    let mut resolved = 0u32;
    let mut assigned = 0u32;
    for sample in &relevant {
        if sample.canary.source == context.source
            && sample.canary.regime_id == context.regime_id
            && utc(&sample.canary.send_wall_utc)? <= utc(&context.as_of_utc)?
        {
            assigned += 1;
        }
        if let Some((_, w)) = label(sample, context, horizon)? {
            mass += w;
            resolved += 1;
            let t = utc(&sample.canary.send_wall_utc)?;
            latest = Some(latest.map_or(t, |v: chrono::DateTime<chrono::Utc>| v.max(t)));
            first = Some(first.map_or(t, |v: chrono::DateTime<chrono::Utc>| v.min(t)));
        }
    }
    let age = latest
        .map(|t| utc(&context.as_of_utc).map(|a| (a - t).num_milliseconds() as f64 / 1000.0))
        .transpose()?;
    let share = if assigned > 0 {
        f64::from(assigned - resolved) / f64::from(assigned)
    } else {
        0.0
    };
    let fit_age = (utc(&context.as_of_utc)? - utc(&model.context.as_of_utc)?).num_milliseconds()
        as f64
        / 1000.0;
    if mass < 200.0
        || age.is_none_or(|n| n > crate::MAX_AGE_S)
        || fit_age > crate::MAX_AGE_S
        || share > crate::MAX_UNRESOLVED_SHARE
        || model.holdout_m1_log_loss.is_none()
    {
        return Ok(result);
    }
    let support: Vec<_> = model
        .support
        .iter()
        .filter(|c| {
            c.route == config.route
                && c.size_class == config.size_class
                && c.cu_limit == config.cu_limit
        })
        .collect();
    let within = |get: fn(&CanaryConfig) -> u64, value: u64| {
        support
            .iter()
            .map(|c| get(c))
            .min()
            .zip(support.iter().map(|c| get(c)).max())
            .is_some_and(|(lo, hi)| lo <= value && value <= hi)
    };
    let (p, interval) = probability(model, config, leaders, covariates)?;
    let weight = if cell.evidence == Evidence::Measured {
        model.m1_weight
    } else {
        1.0
    };
    result.p_hat = (1.0 - weight) * cell.p_hat + weight * p;
    // Envelope preserves uncertainty from either component; no falsely narrow independence assumption.
    result.p_interval_95 = [
        cell.p_interval_95[0].min(interval[0]),
        cell.p_interval_95[1].max(interval[1]),
    ];
    if cell.evidence != Evidence::Measured {
        result.evidence = if within(|c| c.tip_lamports, config.tip_lamports)
            && within(
                |c| c.cu_price_micro_lamports,
                config.cu_price_micro_lamports,
            ) {
            Evidence::Interpolated
        } else {
            Evidence::Extrapolated
        };
        result.n_effective = mass;
        result.data_age_s = age;
        result.unresolved_share = share;
        result.observation_window = first
            .zip(latest)
            .map(|(a, b)| [a.to_rfc3339(), b.to_rfc3339()]);
        result.samples_needed = None;
        result.p_interval_95 = interval;
        if result.evidence == Evidence::Extrapolated {
            result.p_interval_95 = [0.0, 1.0];
        }
    }
    result.m1_weight = weight;
    Ok(result)
}
