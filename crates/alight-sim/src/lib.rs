//! Seeded artificial landing environment. No keys, provider calls, signing or SOL spend.
use alight_canary::policy::{Assignment, Policy};
use alight_types::{
    Canary, CanaryConfig, IndexScope, LeaderClass, ObserverKind, Outcome, ReceiveTime, Route,
    SizeClass, Source, Tercile,
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("invalid simulator parameters")]
pub struct SimError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shift {
    pub at_draw: u32,
    pub slot_ms: u32,
    pub congestion: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameters {
    pub version: u32,
    #[serde(with = "alight_types::decimal_u64")]
    pub seed: u64,
    pub canaries: u32,
    pub slot_ms: u32,
    pub congestion: f64,
    pub tip_slope: f64,
    pub shift: Option<Shift>,
    #[serde(default = "default_fee_slope")]
    pub fee_slope: f64,
    #[serde(default)]
    pub never_land_mass: Option<f64>,
    #[serde(default)]
    pub continuous_latency: bool,
}
fn default_fee_slope() -> f64 {
    0.18
}
impl Default for Parameters {
    fn default() -> Self {
        Self {
            version: 1,
            seed: 42,
            canaries: 8100,
            slot_ms: 250,
            congestion: 0.0,
            tip_slope: 0.45,
            shift: None,
            fee_slope: 0.18,
            never_land_mass: None,
            continuous_latency: false,
        }
    }
}
impl Parameters {
    pub fn validate(&self) -> Result<(), SimError> {
        let environment = |slot_ms, congestion: f64| {
            (50..=2000).contains(&slot_ms)
                && congestion.is_finite()
                && (0.0..=5.0).contains(&congestion)
        };
        if !self.fee_slope.is_finite()
            || !(0.0..=2.0).contains(&self.fee_slope)
            || self
                .never_land_mass
                .is_some_and(|v| !v.is_finite() || !(0.0..=0.5).contains(&v))
            || self.version != 1
            || !(1..=100_000).contains(&self.canaries)
            || !environment(self.slot_ms, self.congestion)
            || !self.tip_slope.is_finite()
            || !(0.0..=2.0).contains(&self.tip_slope)
            || self.shift.as_ref().is_some_and(|s| {
                s.at_draw == 0
                    || s.at_draw >= self.canaries
                    || !environment(s.slot_ms, s.congestion)
            })
        {
            return Err(SimError);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundTruth {
    pub regime_id: String,
    pub slot_ms: u32,
    pub congestion: f64,
    pub config: CanaryConfig,
    pub p_within_1_slot: f64,
    pub p_within_2_slots: f64,
    pub p_within_4_slots: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dataset {
    pub source: Source,
    pub parameters: Parameters,
    pub as_of_utc: String,
    pub ground_truth: Vec<GroundTruth>,
    pub canaries: Vec<Canary>,
}

struct Random(u64);
impl Random {
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^= z >> 31;
        ((z >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
}
fn utc(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn hazard(
    config: &CanaryConfig,
    congestion: f64,
    leader: i32,
    tip_slope: f64,
    fee_slope: f64,
) -> f64 {
    let route = match config.route {
        Route::BeamQuic => 0.2,
        Route::BeamHttp => -0.05,
        Route::Rpc => -0.4,
    };
    let size = match config.size_class {
        SizeClass::Small => 0.0,
        SizeClass::Medium => 0.2,
        SizeClass::Large => 0.5,
    };
    let log_tip = (config.tip_lamports as f64 / 100_000.0).max(1.0).ln();
    (route
        + tip_slope * log_tip
        + fee_slope * (config.cu_price_micro_lamports as f64 / 1000.0).ln_1p()
        - size
        + f64::from(leader) * 0.2
        - congestion * 0.7)
        .exp()
}
fn never_land(congestion: f64) -> f64 {
    0.02 + 0.03 * congestion
}
/// Exact artificial landing probability within H slots, averaging three equally likely leader classes.
pub fn probability(
    config: &CanaryConfig,
    congestion: f64,
    tip_slope: f64,
    horizon_slots: u32,
) -> f64 {
    [-1, 0, 1]
        .into_iter()
        .map(|leader| {
            (1.0 - never_land(congestion))
                * (1.0
                    - (-f64::from(horizon_slots.min(16))
                        * hazard(config, congestion, leader, tip_slope, 0.18))
                    .exp())
        })
        .sum::<f64>()
        / 3.0
}

/// Produces finalized synthetic canaries in the live Canary schema, explicitly source=sim.
pub fn generate(parameters: &Parameters) -> Result<Dataset, SimError> {
    generate_inner(parameters, None)
}
/// Focused uniform experiment for discrimination/null validation, still normal synthetic canaries.
pub fn generate_focused(
    parameters: &Parameters,
    route: Route,
    size: SizeClass,
) -> Result<Dataset, SimError> {
    generate_inner(parameters, Some((route, size)))
}

/// One prospective synthetic attempt with exact locked settings; no keys or network.
/// Send spacing and latency use the frozen synthetic slot_ms, never a live clock claim.
pub fn simulate_locked(
    lock: &alight_types::ProveLock,
    ordinal: u32,
) -> Result<alight_types::TrainingCanary, SimError> {
    if lock.source != Source::Sim || ordinal >= lock.n {
        return Err(SimError);
    }
    let env = lock.simulation.as_ref().ok_or(SimError)?;
    let params = Parameters {
        seed: lock.seed.ok_or(SimError)?,
        canaries: 1,
        slot_ms: env.slot_ms,
        congestion: env.congestion,
        tip_slope: env.tip_slope,
        fee_slope: env.fee_slope,
        never_land_mass: env.never_land_mass,
        continuous_latency: env.continuous_latency,
        shift: None,
        version: 1,
    };
    params.validate()?;
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!(
        "alight.prove.sim.v1:{}:{}:{}",
        lock.id, params.seed, ordinal
    ));
    let mut rng = Random(u64::from_le_bytes(
        digest[..8].try_into().map_err(|_| SimError)?,
    ));
    let leader = (rng.unit() * 3.0).floor() as i32 - 1;
    let never = rng.unit()
        < params
            .never_land_mass
            .unwrap_or(never_land(params.congestion));
    let continuous = -rng.unit().ln()
        / hazard(
            &lock.config,
            params.congestion,
            leader,
            params.tip_slope,
            params.fee_slope,
        );
    let distance = (continuous.ceil() as u64).max(1);
    let landed = !never && distance <= 16;
    let elapsed = u64::from(ordinal + 1) * u64::from(env.slot_ms);
    let start = DateTime::parse_from_rfc3339(&lock.locked_at_utc)
        .map_err(|_| SimError)?
        .with_timezone(&Utc);
    let sent = start + Duration::milliseconds(elapsed as i64);
    let delay_ns = if landed && env.continuous_latency {
        (continuous * f64::from(env.slot_ms) * 1e6).round() as u64
    } else {
        (if landed { distance } else { 17 }) * u64::from(env.slot_ms) * 1_000_000
    };
    let clock = format!("sim-prove-{}", lock.id);
    let received = ReceiveTime {
        clock_id: clock.clone(),
        mono_ns: elapsed * 1_000_000 + delay_ns,
        wall_utc: utc(sent + Duration::nanoseconds(delay_ns as i64)),
    };
    let c = Canary {
        id: format!("{}-{ordinal}", lock.id),
        source: Source::Sim,
        config: lock.config.clone(),
        policy_id: alight_canary::policy::GridProfile::Full
            .held_out_policy_id()
            .into(),
        assignment_prob: 1.0,
        uniform_arm: false,
        regime_id: lock.regime_id.clone(),
        sent_slot: 1_000_000 + u64::from(ordinal),
        clock_id: clock,
        send_mono_ns: elapsed * 1_000_000,
        send_wall_utc: utc(sent),
        signature: None,
        blockhash: format!("synthetic-prove-blockhash-{ordinal}"),
        last_valid_block_height: 1_000_016 + u64::from(ordinal),
        leader_class_next: vec![],
        outcome: Some(if landed {
            Outcome::LandedOk
        } else {
            Outcome::Expired
        }),
        landed_slot: landed.then_some(1_000_000 + u64::from(ordinal) + distance),
        landed_block_id: landed.then(|| {
            format!(
                "synthetic-prove-block-{}",
                1_000_000 + u64::from(ordinal) + distance
            )
        }),
        landed_index: landed.then_some(0),
        landed_index_scope: landed.then_some(IndexScope::Unknown),
        observer_first_seen: if landed {
            BTreeMap::from([(ObserverKind::Grpc, received.clone())])
        } else {
            BTreeMap::new()
        },
        resolved_at_utc: Some(received.wall_utc),
    };
    Ok(alight_types::TrainingCanary {
        canary: c,
        finalized: true,
        covariates: alight_types::ModelCovariates {
            congestion: Some(env.congestion),
        },
    })
}
fn generate_inner(
    parameters: &Parameters,
    focus: Option<(Route, SizeClass)>,
) -> Result<Dataset, SimError> {
    parameters.validate()?;
    // Isolate runs that share a seed but differ in environment or focused cohort.
    use sha2::{Digest, Sha256};
    let namespace = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(parameters, focus)).map_err(|_| SimError)?)
    );
    let mut policy = Policy::new(parameters.seed, 0.30).map_err(|_| SimError)?;
    let mut focused_rng = Random(parameters.seed ^ 0xa0761d6478bd642f);
    let focused_cells: Vec<_> = policy
        .cells()
        .iter()
        .filter(|c| focus.is_none_or(|(r, s)| c.route == r && c.size_class == s))
        .cloned()
        .collect();
    let mut outcomes = Random(parameters.seed ^ 0xd1b54a32d192ed03);
    let start = DateTime::parse_from_rfc3339("2026-10-05T00:00:00Z")
        .map_err(|_| SimError)?
        .with_timezone(&Utc);
    let mut elapsed_ms = 0u64;
    let mut canaries = Vec::with_capacity(parameters.canaries as usize);
    for draw in 0..parameters.canaries {
        let shifted = parameters.shift.as_ref().filter(|s| draw >= s.at_draw);
        let (regime_id, slot_ms, congestion) = shifted
            .map_or(("sim-r0", parameters.slot_ms, parameters.congestion), |s| {
                ("sim-r1", s.slot_ms, s.congestion)
            });
        let assignment = if focus.is_some() {
            let mut config = focused_cells[((focused_rng.unit() * focused_cells.len() as f64)
                as usize)
                .min(focused_cells.len() - 1)]
            .clone();
            config.cu_price_micro_lamports = match config.fee_bucket {
                alight_types::FeeBucket::Zero => 0,
                alight_types::FeeBucket::LocalMedian => 1000,
                alight_types::FeeBucket::LocalP90 => 5000,
            };
            Assignment {
                config,
                policy_id: "focused-uniform-sim:full",
                seed: parameters.seed,
                draw: u64::from(draw),
                assignment_prob: 1.0 / focused_cells.len() as f64,
                uniform_arm: true,
            }
        } else {
            policy.assign(1000, 5000)
        };
        let sent = start + Duration::milliseconds(elapsed_ms as i64);
        let sent_slot = 1_000_000 + u64::from(draw);
        let leader = (outcomes.unit() * 3.0).floor() as i32 - 1;
        let never = outcomes.unit() < parameters.never_land_mass.unwrap_or(never_land(congestion));
        let continuous_distance = -outcomes.unit().ln()
            / hazard(
                &assignment.config,
                congestion,
                leader,
                parameters.tip_slope,
                parameters.fee_slope,
            );
        let distance = (continuous_distance.ceil() as u64).max(1);
        let landed = !never && distance <= 16;
        let delay_ms = if landed { distance } else { 17 } * u64::from(slot_ms);
        let delay_ns = if landed && parameters.continuous_latency {
            (continuous_distance * f64::from(slot_ms) * 1_000_000.0).round() as u64
        } else {
            delay_ms * 1_000_000
        };
        let received = ReceiveTime {
            clock_id: "sim-clock-v1".into(),
            mono_ns: elapsed_ms * 1_000_000 + delay_ns,
            wall_utc: utc(sent + Duration::nanoseconds(delay_ns as i64)),
        };
        let observer_first_seen = if landed {
            BTreeMap::from([(ObserverKind::Grpc, received.clone())])
        } else {
            BTreeMap::new()
        };
        let leader_tercile = match leader {
            -1 => Tercile::Low,
            0 => Tercile::Middle,
            _ => Tercile::High,
        };
        canaries.push(Canary {
            id: format!("sim-{namespace}-{draw}"),
            source: Source::Sim,
            config: assignment.config,
            policy_id: assignment.policy_id.into(),
            assignment_prob: assignment.assignment_prob,
            uniform_arm: assignment.uniform_arm,
            regime_id: regime_id.into(),
            sent_slot,
            clock_id: "sim-clock-v1".into(),
            send_mono_ns: elapsed_ms * 1_000_000,
            send_wall_utc: utc(sent),
            signature: None,
            blockhash: format!("synthetic-blockhash-{draw}"),
            last_valid_block_height: sent_slot + 16,
            leader_class_next: vec![LeaderClass {
                leader: format!("synthetic-leader-{leader}"),
                skip_rate_tercile: Tercile::Unknown,
                stake_tercile: leader_tercile,
            }],
            outcome: Some(if landed {
                Outcome::LandedOk
            } else {
                Outcome::Expired
            }),
            landed_slot: landed.then_some(sent_slot + distance),
            landed_block_id: landed.then(|| format!("synthetic-block-{}", sent_slot + distance)),
            landed_index: landed.then_some(0),
            landed_index_scope: landed.then_some(IndexScope::Unknown),
            observer_first_seen,
            resolved_at_utc: Some(received.wall_utc),
        });
        elapsed_ms += u64::from(slot_ms);
    }
    let mut ground_truth = Vec::new();
    let environments = std::iter::once(("sim-r0", parameters.slot_ms, parameters.congestion))
        .chain(
            parameters
                .shift
                .iter()
                .map(|s| ("sim-r1", s.slot_ms, s.congestion)),
        );
    for (regime_id, slot_ms, congestion) in environments {
        for cell in &focused_cells {
            let mut config = cell.clone();
            config.cu_price_micro_lamports = match config.fee_bucket {
                alight_types::FeeBucket::Zero => 0,
                alight_types::FeeBucket::LocalMedian => 1000,
                alight_types::FeeBucket::LocalP90 => 5000,
            };
            ground_truth.push(GroundTruth {
                regime_id: regime_id.into(),
                slot_ms,
                congestion,
                p_within_1_slot: probability_with(parameters, &config, congestion, 1),
                p_within_2_slots: probability_with(parameters, &config, congestion, 2),
                p_within_4_slots: probability_with(parameters, &config, congestion, 4),
                config,
            });
        }
    }
    // All synthetic outcomes, including failures, are known by this as-of time.
    let as_of_utc = utc(start + Duration::milliseconds((elapsed_ms + 34_000) as i64));
    Ok(Dataset {
        source: Source::Sim,
        parameters: parameters.clone(),
        as_of_utc,
        ground_truth,
        canaries,
    })
}

impl Dataset {
    pub fn training(&self) -> Result<Vec<alight_types::TrainingCanary>, SimError> {
        if self.source != Source::Sim || self.canaries.iter().any(|c| c.source != Source::Sim) {
            return Err(SimError);
        }
        Ok(self
            .canaries
            .iter()
            .cloned()
            .map(|canary| {
                let congestion = if canary.regime_id == "sim-r1" {
                    self.parameters
                        .shift
                        .as_ref()
                        .map_or(self.parameters.congestion, |s| s.congestion)
                } else {
                    self.parameters.congestion
                };
                alight_types::TrainingCanary {
                    canary,
                    finalized: true,
                    covariates: alight_types::ModelCovariates {
                        congestion: Some(congestion),
                    },
                }
            })
            .collect())
    }
}
pub fn probability_with(p: &Parameters, config: &CanaryConfig, congestion: f64, h: u32) -> f64 {
    [-1, 0, 1]
        .into_iter()
        .map(|l| {
            (1.0 - p.never_land_mass.unwrap_or(never_land(congestion)))
                * (1.0
                    - (-f64::from(h.min(16))
                        * hazard(config, congestion, l, p.tip_slope, p.fee_slope))
                    .exp())
        })
        .sum::<f64>()
        / 3.0
}
