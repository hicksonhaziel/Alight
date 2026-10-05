//! Decayed empirical quantiles, including failure mass, with seeded percentile bootstrap.
use crate::{ModelError, estimate, label, utc};
use alight_types::*;
use std::collections::BTreeMap;

pub(crate) struct Random(pub u64);
impl Random {
    pub fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }
}
pub(crate) fn percentile(values: &mut [f64], q: f64) -> f64 {
    values.sort_by(f64::total_cmp);
    values[((values.len() - 1) as f64 * q).round() as usize]
}
pub(crate) fn weighted_quantile(values: &[(f64, f64)], q: f64) -> f64 {
    let total = values.iter().map(|(_, w)| w).sum::<f64>();
    let mut so_far = 0.0;
    for &(value, w) in values {
        so_far += w;
        if so_far >= q * total {
            return value;
        }
    }
    f64::INFINITY
}
pub(crate) fn latency(sample: &TrainingCanary) -> Result<(f64, Option<f64>), ModelError> {
    let c = &sample.canary;
    if !matches!(c.outcome, Some(Outcome::LandedOk | Outcome::LandedFailed)) {
        return Ok((f64::INFINITY, Some(f64::INFINITY)));
    }
    let slots = c
        .landed_slot
        .and_then(|v| v.checked_sub(c.sent_slot))
        .ok_or(ModelError::Invalid)? as f64;
    let ms = c
        .observer_first_seen
        .values()
        .filter(|t| t.clock_id == c.clock_id && t.mono_ns >= c.send_mono_ns)
        .map(|t| (t.mono_ns - c.send_mono_ns) as f64 / 1_000_000.0)
        .min_by(f64::total_cmp);
    Ok((slots, ms))
}
fn distribution(
    samples: &[TrainingCanary],
    config: &CanaryConfig,
    context: &CurveContext,
    milliseconds: bool,
) -> Result<Vec<(f64, f64)>, ModelError> {
    let mut values = Vec::new();
    for sample in samples.iter().filter(|s| s.canary.config == *config) {
        if let Some((_, w)) = label(sample, context, 1)? {
            let (slots, ms) = latency(sample)?;
            if milliseconds {
                let Some(ms) = ms else {
                    return Ok(Vec::new());
                };
                values.push((ms, w));
            } else {
                values.push((slots, w));
            }
        }
    }
    values.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut histogram: Vec<(f64, f64)> = Vec::new();
    for (v, w) in values {
        if let Some(last) = histogram.last_mut().filter(|last| last.0 == v) {
            last.1 += w;
        } else {
            histogram.push((v, w));
        }
    }
    Ok(histogram)
}
fn bootstrap(values: &[(f64, f64)], q: f64, n: usize, seed: u64) -> [Option<f64>; 2] {
    if values.is_empty() || n == 0 {
        return [None, None];
    }
    let total = values.iter().map(|(_, w)| w).sum::<f64>();
    let mut cumulative = Vec::new();
    let mut sum = 0.0;
    for (_, w) in values {
        sum += w / total;
        cumulative.push(sum);
    }
    let mut rng = Random(seed);
    let mut quantiles = Vec::with_capacity(2000);
    for _ in 0..2000 {
        let mut counts = vec![0u32; values.len()];
        for _ in 0..n {
            let random = rng.unit();
            let index = cumulative
                .partition_point(|c| *c < random)
                .min(values.len() - 1);
            counts[index] += 1;
        }
        let rank = (q * n as f64).ceil() as u32;
        let mut count = 0;
        let mut quantile = f64::INFINITY;
        for ((v, _), n) in values.iter().zip(counts) {
            count += n;
            if count >= rank {
                quantile = *v;
                break;
            }
        }
        quantiles.push(quantile);
    }
    let finite = |n: f64| n.is_finite().then_some(n);
    let bootstrap = [
        percentile(&mut quantiles, 0.025),
        percentile(&mut quantiles, 0.975),
    ];
    // Distribution-free rank interval: P(k <= Binomial(n,q) <= l-1) >= .95.
    // Decayed mass uses an approximate effective n; retain the larger bootstrap envelope.
    let mut cumulative = 0.0;
    let mut lower = 0usize;
    let mut upper = n;
    let mut found = false;
    let total_gamma = crate::beta::log_gamma(n as f64 + 1.0);
    for k in 0..=n {
        let mass = (total_gamma
            - crate::beta::log_gamma(k as f64 + 1.0)
            - crate::beta::log_gamma((n - k) as f64 + 1.0)
            + k as f64 * q.ln()
            + (n - k) as f64 * (-q).ln_1p())
        .exp();
        cumulative += mass;
        if !found && cumulative >= 0.025 {
            lower = k;
            found = true;
        }
        if cumulative >= 0.975 {
            upper = (k + 1).min(n);
            break;
        }
    }
    let rank_low = weighted_quantile(values, lower as f64 / n as f64);
    let rank_high = weighted_quantile(values, upper as f64 / n as f64);
    [
        finite(bootstrap[0].min(rank_low)),
        finite(bootstrap[1].max(rank_high)),
    ]
}
/// Quantile units are slots and same-clock first-observed milliseconds; no slot-time conversion.
pub fn estimate_quantile(
    samples: &[TrainingCanary],
    config: &CanaryConfig,
    context: &CurveContext,
    quantile: f64,
    seed: u64,
) -> Result<LatencyEstimate, ModelError> {
    if !quantile.is_finite() || quantile <= 0.0 || quantile >= 1.0 {
        return Err(ModelError::Invalid);
    }
    let cell = estimate(samples, config, context, 1)?;
    let slots = distribution(samples, config, context, false)?;
    let ms = distribution(samples, config, context, true)?;
    let value = weighted_quantile(&slots, quantile);
    let millis = weighted_quantile(&ms, quantile);
    let tail_needed = if quantile >= 0.99 {
        20.0 / (1.0 - quantile)
    } else {
        crate::MIN_EFFECTIVE_N
    };
    let enough =
        cell.evidence == Evidence::Measured && cell.n_effective >= tail_needed && value.is_finite();
    let n = cell.n_effective.floor() as usize;
    let slots_interval = if enough {
        bootstrap(&slots, quantile, n, seed)
    } else {
        [None, None]
    };
    let ms_interval = if enough && !ms.is_empty() {
        bootstrap(&ms, quantile, n, seed)
    } else {
        [None, None]
    };
    let measured = enough && slots_interval[1].is_some();
    Ok(LatencyEstimate {
        quantile,
        slots: value.is_finite().then_some(value),
        slots_interval_95: slots_interval,
        ms: millis.is_finite().then_some(millis),
        ms_interval_95: ms_interval,
        n_effective: cell.n_effective,
        evidence: if measured {
            Evidence::Measured
        } else {
            Evidence::Insufficient
        },
        samples_needed: if measured {
            None
        } else {
            Some(
                cell.samples_needed
                    .unwrap_or(0)
                    .max((tail_needed - cell.n_effective).ceil().max(1.0) as u32),
            )
        },
    })
}

pub(crate) fn cluster_seed(day: &str) -> u64 {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(format!("{day}:{}", crate::methodology_hash()));
    u64::from_le_bytes(h[..8].try_into().unwrap_or([0; 8]))
}
pub(crate) fn eligible_day<'a>(
    samples: &'a [TrainingCanary],
    source: Source,
    day: &str,
    as_of: &str,
) -> Result<Vec<&'a TrainingCanary>, ModelError> {
    let start = utc(&format!("{day}T00:00:00Z"))?;
    let end = start + chrono::Duration::days(1);
    let now = utc(as_of)?;
    let mut ids = BTreeMap::new();
    let mut result = Vec::new();
    for s in samples {
        if s.canary.source != source {
            continue;
        }
        let sent = utc(&s.canary.send_wall_utc)?;
        if s.canary.source != source || sent < start || sent >= end || sent > now {
            continue;
        }
        if ids.insert(&s.canary.id, ()).is_some() {
            return Err(ModelError::Duplicate);
        }
        let ctx = CurveContext {
            source,
            regime_id: s.canary.regime_id.clone(),
            region: "signal".into(),
            as_of_utc: as_of.into(),
        };
        if label(s, &ctx, 1)?.is_some() {
            result.push(s);
        }
    }
    Ok(result)
}
