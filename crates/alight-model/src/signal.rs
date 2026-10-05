//! Registered daily discrimination analysis; forecasting monotonicity is not imposed here.
use crate::{
    ModelError, label,
    latency::{self, Random},
    regression::{self, Row},
    utc,
};
use alight_types::*;
use std::collections::BTreeMap;

const DIM: usize = 17;
fn features(s: &TrainingCanary) -> Result<Vec<f64>, ModelError> {
    let c = &s.canary;
    let mut x = vec![0.0; DIM];
    x[0] = 1.0;
    x[1] = (c.config.tip_lamports as f64 / 100_000.0).max(1.0).ln();
    x[2] = f64::from(c.config.fee_bucket == FeeBucket::LocalMedian);
    x[3] = f64::from(c.config.fee_bucket == FeeBucket::LocalP90);
    for (kind, start) in [
        (
            c.leader_class_next
                .first()
                .map_or(Tercile::Unknown, |l| l.stake_tercile),
            4,
        ),
        (
            c.leader_class_next
                .first()
                .map_or(Tercile::Unknown, |l| l.skip_rate_tercile),
            7,
        ),
    ] {
        match kind {
            Tercile::Low => x[start] = 1.0,
            Tercile::High => x[start + 1] = 1.0,
            Tercile::Unknown => x[start + 2] = 1.0,
            Tercile::Middle => {}
        }
    }
    if let Some(congestion) = s.covariates.congestion {
        if !congestion.is_finite() || !(0.0..=5.0).contains(&congestion) {
            return Err(ModelError::Invalid);
        }
        x[10] = congestion;
    } else {
        x[11] = 1.0;
    }
    use chrono::Timelike;
    let hour = f64::from(utc(&c.send_wall_utc)?.hour());
    x[12] = (hour * std::f64::consts::TAU / 24.0).sin();
    x[13] = (hour * std::f64::consts::TAU / 24.0).cos();
    x[14] = x[10] * x[12];
    x[15] = x[1] * x[10];
    x[16] = x[1] * x[12];
    Ok(x)
}
struct Cluster {
    score: [Vec<f64>; 2],
    hist: Vec<Vec<f64>>,
}
#[allow(clippy::needless_range_loop)]
fn analyze(
    samples: &[&TrainingCanary],
    as_of: &str,
    seed: u64,
    sensitivity: bool,
) -> Result<Vec<SignalEffect>, ModelError> {
    let mut original = Vec::new();
    let mut ys = Vec::new();
    let mut weights = Vec::new();
    let mut tips = Vec::new();
    let mut latencies = Vec::new();
    let mut levels = Vec::new();
    for s in samples {
        original.push(features(s)?);
        let ctx = CurveContext {
            source: s.canary.source,
            regime_id: s.canary.regime_id.clone(),
            region: "signal".into(),
            as_of_utc: as_of.into(),
        };
        ys.push([
            label(s, &ctx, 1)?.ok_or(ModelError::Invalid)?.0,
            label(s, &ctx, 2)?.ok_or(ModelError::Invalid)?.0,
        ]);
        weights.push(if sensitivity {
            1.0 / (81.0 * s.canary.assignment_prob)
        } else {
            1.0
        });
        tips.push(s.canary.config.tip_lamports);
        let l = latency::latency(s)?.0;
        latencies.push(l);
        levels.push(l);
    }
    levels.sort_by(f64::total_cmp);
    levels.dedup();
    let mut tiers = tips.clone();
    tiers.sort_unstable();
    tiers.dedup();
    // Drop exact linear dependencies before fitting unrestricted scientific slopes.
    let unique: BTreeMap<Vec<u64>, Vec<f64>> = original
        .iter()
        .map(|x| (x.iter().map(|v| v.to_bits()).collect(), x.clone()))
        .collect();
    let mut active = Vec::new();
    let mut basis: Vec<Vec<f64>> = Vec::new();
    for j in 0..DIM {
        let mut column: Vec<_> = unique.values().map(|x| x[j]).collect();
        let norm = regression::dot(&column, &column);
        if norm <= 1e-12 {
            continue;
        }
        for b in &basis {
            let projection = regression::dot(&column, b);
            for (v, b) in column.iter_mut().zip(b) {
                *v -= projection * b;
            }
        }
        let residual = regression::dot(&column, &column);
        if residual <= norm * 1e-10 {
            continue;
        }
        for v in &mut column {
            *v /= residual.sqrt();
        }
        basis.push(column);
        active.push(j);
    }
    let p = active.len();
    let mut bins: BTreeMap<Vec<u64>, (Vec<f64>, [f64; 2], f64)> = BTreeMap::new();
    let mut xs = Vec::new();
    for (i, x) in original.iter().enumerate() {
        let x: Vec<_> = active.iter().map(|&j| x[j]).collect();
        let bin = bins
            .entry(x.iter().map(|v| v.to_bits()).collect())
            .or_insert((x.clone(), [0.0; 2], 0.0));
        for h in 0..2 {
            bin.1[h] += ys[i][h] * weights[i];
        }
        bin.2 += weights[i];
        xs.push(x);
    }
    let fits: Vec<_> = (0..2)
        .map(|h| {
            regression::fit(
                &bins
                    .values()
                    .map(|(x, y, n)| Row {
                        x: x.clone(),
                        successes: y[h],
                        trials: *n,
                    })
                    .collect::<Vec<_>>(),
                &vec![1e-4; p],
                &[],
            )
        })
        .collect::<Result<_, _>>()?;
    let mut clusters: BTreeMap<(String, String, u64), Cluster> = BTreeMap::new();
    for (i, s) in samples.iter().enumerate() {
        let key = (
            s.canary.regime_id.clone(),
            s.canary.clock_id.clone(),
            s.canary.sent_slot / 16,
        );
        let cluster = clusters.entry(key).or_insert_with(|| Cluster {
            score: [vec![0.0; p], vec![0.0; p]],
            hist: vec![vec![0.0; levels.len()]; tiers.len()],
        });
        for h in 0..2 {
            let residual = weights[i]
                * (ys[i][h] - regression::sigmoid(regression::dot(&xs[i], &fits[h].coefficients)));
            for (j, x) in xs[i].iter().enumerate() {
                cluster.score[h][j] += residual * x;
            }
        }
        let tier = tiers
            .binary_search(&tips[i])
            .map_err(|_| ModelError::Invalid)?;
        let level = levels
            .binary_search_by(|v| v.total_cmp(&latencies[i]))
            .map_err(|_| ModelError::Invalid)?;
        let age = (utc(as_of)? - utc(&s.canary.send_wall_utc)?).num_milliseconds() as f64 / 1000.0;
        cluster.hist[tier][level] += (-age / crate::HALF_LIFE_S).exp2() * weights[i];
    }
    let clusters: Vec<_> = clusters.into_values().collect();
    let mut histogram = vec![vec![0.0; levels.len()]; tiers.len()];
    for c in &clusters {
        for (tier, row) in histogram.iter_mut().enumerate() {
            for (level, v) in row.iter_mut().enumerate() {
                *v += c.hist[tier][level];
            }
        }
    }
    let q = |hist: &[f64], quantile| {
        latency::weighted_quantile(
            &levels
                .iter()
                .copied()
                .zip(hist.iter().copied())
                .collect::<Vec<_>>(),
            quantile,
        )
    };
    let primary: Vec<_> = [1, 2, 3, 14, 15, 16]
        .into_iter()
        .filter_map(|j| active.iter().position(|v| *v == j).map(|i| (j, i)))
        .collect();
    let range = |column: usize| {
        original
            .iter()
            .map(|x| x[column])
            .fold(f64::NEG_INFINITY, f64::max)
            - original
                .iter()
                .map(|x| x[column])
                .fold(f64::INFINITY, f64::min)
    };
    let scale = |column| match column {
        14 => range(14),
        15 => range(10),
        16 => range(12),
        _ => 1.0,
    };
    let mut families = Vec::new();
    let mut effects = Vec::new();
    let mut values = Vec::new();
    for h in 0..2 {
        for &(column, index) in &primary {
            effects.push(SignalEffect {
                name: format!(
                    "within_{}_slots_{}",
                    h + 1,
                    match column {
                        1 => "log_tip",
                        2 => "fee_median_vs_zero",
                        3 => "fee_p90_vs_zero",
                        14 => "congestion_hour_effect_over_window",
                        15 => "log_tip_congestion_effect_over_window",
                        _ => "log_tip_hour_effect_over_window",
                    }
                ),
                estimate: Some(fits[h].coefficients[index] * scale(column)),
                interval_99: [None, None],
                practical_threshold: 0.10,
            });
            values.push(Vec::with_capacity(2000));
            families.push(h);
        }
    }
    for quantile in [0.5, 0.9, 0.99] {
        for tier in 1..tiers.len() {
            let value = q(&histogram[tier], quantile) - q(&histogram[0], quantile);
            let sufficient = quantile < 0.99
                || histogram[tier].iter().sum::<f64>() * (1.0 - quantile) >= 20.0
                    && histogram[0].iter().sum::<f64>() * (1.0 - quantile) >= 20.0;
            effects.push(SignalEffect {
                name: format!(
                    "p{}_tip_{}_vs_{}_slots",
                    (quantile * 100.0) as u32,
                    tiers[tier],
                    tiers[0]
                ),
                estimate: (value.is_finite() && sufficient).then_some(value),
                interval_99: [None, None],
                practical_threshold: 0.25,
            });
            values.push(Vec::with_capacity(2000));
            families.push(if quantile == 0.5 {
                2
            } else if quantile == 0.9 {
                3
            } else {
                4
            });
        }
    }
    let mut rng = Random(seed);
    for _ in 0..2000 {
        let mut counts = vec![0u32; clusters.len()];
        for _ in 0..clusters.len() {
            let i = ((rng.unit() * clusters.len() as f64) as usize).min(clusters.len() - 1);
            counts[i] += 1;
        }
        let mut scores = [vec![0.0; p], vec![0.0; p]];
        let mut hist = vec![vec![0.0; levels.len()]; tiers.len()];
        for (cluster, count) in clusters.iter().zip(counts) {
            for h in 0..2 {
                for j in 0..p {
                    scores[h][j] += (f64::from(count) - 1.0) * cluster.score[h][j];
                }
            }
            if count > 0 {
                for (tier, row) in hist.iter_mut().enumerate() {
                    for (level, v) in row.iter_mut().enumerate() {
                        *v += f64::from(count) * cluster.hist[tier][level];
                    }
                }
            }
        }
        let mut index = 0;
        for h in 0..2 {
            for &(column, j) in &primary {
                values[index].push(
                    (fits[h].coefficients[j] + regression::dot(&fits[h].covariance[j], &scores[h]))
                        * scale(column),
                );
                index += 1;
            }
        }
        for quantile in [0.5, 0.9, 0.99] {
            for tier in 1..tiers.len() {
                values[index].push(q(&hist[tier], quantile) - q(&hist[0], quantile));
                index += 1;
            }
        }
    }
    let sd: Vec<_> = values
        .iter()
        .map(|v| {
            let mean = v.iter().sum::<f64>() / v.len() as f64;
            (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (v.len() - 1) as f64).sqrt()
        })
        .collect();
    for family in 0..5 {
        let indices: Vec<_> = families
            .iter()
            .enumerate()
            .filter(|(_, f)| **f == family)
            .map(|(i, _)| i)
            .filter(|&i| effects[i].estimate.is_some() && values[i].iter().all(|v| v.is_finite()))
            .collect();
        let mut maxima = Vec::with_capacity(2000);
        for replicate in 0..2000 {
            maxima.push(
                indices
                    .iter()
                    .map(|&i| {
                        if sd[i] > 0.0 {
                            (values[i][replicate] - effects[i].estimate.unwrap_or(0.0)).abs()
                                / sd[i]
                        } else {
                            0.0
                        }
                    })
                    .fold(0.0, f64::max),
            );
        }
        let critical = latency::percentile(&mut maxima, 0.99);
        for i in indices {
            let point = effects[i].estimate.ok_or(ModelError::Invalid)?;
            effects[i].interval_99 = [
                Some(point - critical * sd[i]),
                Some(point + critical * sd[i]),
            ];
        }
    }
    Ok(effects)
}
/// One immutable daily report; uniform-arm primary analysis and IPW sensitivity are separate.
pub fn report(
    samples: &[TrainingCanary],
    source: Source,
    day: &str,
    as_of_utc: &str,
) -> Result<SignalReport, ModelError> {
    let eligible = latency::eligible_day(samples, source, day, as_of_utc)?;
    let mut cells = Vec::new();
    for route in [Route::BeamQuic, Route::BeamHttp, Route::Rpc] {
        for size in [SizeClass::Small, SizeClass::Medium, SizeClass::Large] {
            let all: Vec<_> = eligible
                .iter()
                .copied()
                .filter(|s| s.canary.config.route == route && s.canary.config.size_class == size)
                .collect();
            let uniform: Vec<_> = all
                .iter()
                .copied()
                .filter(|s| s.canary.uniform_arm)
                .collect();
            let total = samples
                .iter()
                .filter(|s| {
                    s.canary.source == source
                        && s.canary.config.route == route
                        && s.canary.config.size_class == size
                        && utc(&s.canary.send_wall_utc).is_ok_and(|sent| {
                            sent.format("%Y-%m-%d").to_string() == day
                                && utc(as_of_utc).is_ok_and(|now| sent <= now)
                        })
                })
                .count();
            let mut tiers = BTreeMap::new();
            for s in &uniform {
                *tiers.entry(s.canary.config.tip_lamports).or_insert(0u32) += 1;
            }
            let enough = uniform.len() >= 200
                && tiers.values().all(|v| *v >= 30)
                && (route == Route::Rpc || tiers.len() >= 4);
            if !enough {
                cells.push(SignalCell {
                    route,
                    size_class: size,
                    uniform_samples: uniform.len() as u32,
                    excluded: total.saturating_sub(all.len()) as u32,
                    verdict: SignalVerdict::Inconclusive,
                    effects: vec![],
                    sensitivity_effects: vec![],
                    reason: Some("uniform_sample_or_tier_minimum_not_met".into()),
                });
                continue;
            }
            let seed = latency::cluster_seed(&format!("{day}:{route:?}:{size:?}"));
            let effects = analyze(&uniform, as_of_utc, seed, false)?;
            let sensitivity_effects = if uniform.len() == all.len() {
                effects.clone()
            } else {
                analyze(&all, as_of_utc, seed, true)?
            };
            let discriminating = effects.iter().any(|e| {
                e.interval_99[0]
                    .zip(e.interval_99[1])
                    .is_some_and(|(lo, hi)| {
                        lo > e.practical_threshold || hi < -e.practical_threshold
                    })
            });
            let flat = route != Route::Rpc
                && !effects.is_empty()
                && effects.iter().all(|e| {
                    e.interval_99[0]
                        .zip(e.interval_99[1])
                        .is_some_and(|(lo, hi)| {
                            lo > -e.practical_threshold && hi < e.practical_threshold
                        })
                });
            cells.push(SignalCell {
                route,
                size_class: size,
                uniform_samples: uniform.len() as u32,
                excluded: total.saturating_sub(all.len()) as u32,
                verdict: if discriminating {
                    SignalVerdict::Discriminating
                } else if flat {
                    SignalVerdict::Flat
                } else {
                    SignalVerdict::Inconclusive
                },
                effects,
                sensitivity_effects,
                reason: if discriminating || flat {
                    None
                } else {
                    Some("wide_or_unidentifiable_effect_intervals".into())
                },
            });
        }
    }
    Ok(SignalReport {
        source,
        day: day.into(),
        methodology_hash: crate::methodology_hash(),
        as_of_utc: as_of_utc.into(),
        completed_utc_day: utc(as_of_utc)?
            >= utc(&format!("{day}T00:00:00Z"))? + chrono::Duration::days(1),
        bootstrap_replicates: 2000,
        cells,
    })
}
