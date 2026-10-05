use crate::{ModelError, label, utc};
use alight_types::*;

pub fn log_loss(p: f64, y: f64) -> f64 {
    let p = p.clamp(1e-6, 1.0 - 1e-6);
    -y * p.ln() - (1.0 - y) * (-p).ln_1p()
}
/// Brier and natural-log loss on binary labels, with ten equal-width reliability buckets.
pub fn scores(
    values: &[(f64, f64)],
    interval: Option<[f64; 2]>,
) -> Result<Option<ProbabilityScores>, ModelError> {
    if values.is_empty() {
        return Ok(None);
    }
    if values
        .iter()
        .any(|(p, y)| !p.is_finite() || !(0.0..=1.0).contains(p) || ![0.0, 1.0].contains(y))
    {
        return Err(ModelError::Invalid);
    }
    let n = values.len() as u32;
    let observed = values.iter().map(|(_, y)| y).sum::<f64>() / f64::from(n);
    let mut reliability = Vec::new();
    let mut ece = 0.0;
    for i in 0..10 {
        let bucket: Vec<_> = values
            .iter()
            .filter(|(p, _)| ((p * 10.0) as usize).min(9) == i)
            .collect();
        if bucket.is_empty() {
            continue;
        }
        let count = bucket.len() as u32;
        let predicted = bucket.iter().map(|(p, _)| p).sum::<f64>() / f64::from(count);
        let actual = bucket.iter().map(|(_, y)| y).sum::<f64>() / f64::from(count);
        ece += f64::from(count) / f64::from(n) * (predicted - actual).abs();
        reliability.push(ReliabilityBucket {
            lower: i as f64 / 10.0,
            upper: (i + 1) as f64 / 10.0,
            n: count,
            predicted,
            observed: actual,
        });
    }
    Ok(Some(ProbabilityScores {
        n,
        observed_rate: observed,
        brier: values.iter().map(|(p, y)| (p - y).powi(2)).sum::<f64>() / f64::from(n),
        log_loss: values.iter().map(|(p, y)| log_loss(*p, *y)).sum::<f64>() / f64::from(n),
        interval_coverage: interval.map(|[lo, hi]| lo <= observed && observed <= hi),
        expected_calibration_error: ece,
        reliability,
    }))
}
/// Immutable forecast scoring uses only later same-source/config outcomes before expiry.
pub fn grade(
    entry: &ForecastEntry,
    samples: &[TrainingCanary],
    as_of_utc: &str,
) -> Result<ForecastGrade, ModelError> {
    let f = &entry.forecast;
    let created = utc(&f.created_at_utc)?;
    let expires = utc(&f.expires_at_utc)?;
    let as_of = utc(as_of_utc)?;
    if as_of < created || expires <= created {
        return Err(ModelError::Invalid);
    }
    let mut window = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    for s in samples.iter().filter(|s| s.canary.source == f.source) {
        let sent = utc(&s.canary.send_wall_utc)?;
        if sent > created && sent < expires && sent <= as_of {
            if !ids.insert(&s.canary.id) {
                return Err(ModelError::Duplicate);
            }
            window.push(s);
        }
    }
    let voided = window.iter().any(|s| s.canary.regime_id != f.regime_id);
    let horizon = match f.request.target {
        PredictionTarget::Probability { horizon_slots, .. } => horizon_slots,
        PredictionTarget::LatencyQuantile { .. } => 4,
    };
    let config = f.quote.recommendation.as_ref().map(|p| &p.config);
    let mut values = Vec::new();
    let mut unresolved = 0;
    let mut latency = (0u32, 0u32);
    for s in window.iter().filter(|s| config == Some(&s.canary.config)) {
        let context = CurveContext {
            source: f.source,
            regime_id: s.canary.regime_id.clone(),
            region: f.request.context.region.clone(),
            as_of_utc: as_of_utc.into(),
        };
        if let Some((y, _)) = label(s, &context, horizon)? {
            if let Some(p) = &f.quote.recommendation {
                values.push((p.p_hat, y));
            }
            if let Some(q) = &f.quote.latency {
                latency.1 += 1;
                let (slots, ms) = crate::latency::latency(s)?;
                let (bound, d) = match f.request.target {
                    PredictionTarget::LatencyQuantile {
                        max_ms: Some(_), ..
                    } => (q.ms, ms),
                    _ => (q.slots, Some(slots)),
                };
                if let Some((bound, d)) = bound.zip(d) {
                    latency.0 += u32::from(d <= bound);
                } else {
                    latency.1 -= 1;
                }
            }
        } else {
            unresolved += 1;
        }
    }
    let scored = scores(
        &values,
        f.quote.recommendation.as_ref().map(|p| p.p_interval_95),
    )?;
    let mut baselines = Vec::new();
    for b in &f.baselines {
        let mut outcomes = Vec::new();
        for s in window
            .iter()
            .filter(|s| Some(&s.canary.config) == b.config.as_ref())
        {
            let context = CurveContext {
                source: f.source,
                regime_id: s.canary.regime_id.clone(),
                region: f.request.context.region.clone(),
                as_of_utc: as_of_utc.into(),
            };
            if let (Some(p), Some((y, _))) = (b.p_hat, label(s, &context, horizon)?) {
                outcomes.push((p, y));
            }
        }
        baselines.push((b.id.clone(), scores(&outcomes, None)?));
    }
    Ok(ForecastGrade {
        forecast_hash: entry.hash.clone(),
        graded_at_utc: as_of_utc.into(),
        status: if voided
            || config.is_none()
            || (as_of >= expires && unresolved == 0 && scored.is_none())
        {
            ForecastStatus::Voided
        } else if as_of < expires || unresolved > 0 || scored.is_none() {
            ForecastStatus::Pending
        } else {
            ForecastStatus::Scored
        },
        unresolved,
        scores: if voided { None } else { scored.clone() },
        through_change_scores: if voided { scored } else { None },
        latency_coverage: if latency.1 > 0 {
            Some(f64::from(latency.0) / f64::from(latency.1))
        } else {
            None
        },
        baselines,
        reason: if voided {
            Some("forecast_straddles_regime_change".into())
        } else if config.is_none() {
            Some("no_recommendation".into())
        } else if as_of >= expires && unresolved == 0 && values.is_empty() {
            Some("no_later_matched_outcomes".into())
        } else {
            None
        },
    })
}
