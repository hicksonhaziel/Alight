//! Seeded stratified policy v0. Every cell retains positive probability.
use alight_types::{CanaryConfig, FeeBucket, Route, SizeClass, TipTier};
use serde::Serialize;
use thiserror::Error;

pub const POLICY_ID: &str = "stratified-v0-splitmix64";
pub const MIN_TIP_LAMPORTS: u64 = 100_000;
pub const ADAPTIVE_POLICY_ID: &str = "thompson-cost-v1-splitmix64";
#[derive(Debug, Error)]
#[error("invalid assignment policy configuration")]
pub struct PolicyError;
pub fn cu_limit(size: SizeClass) -> u32 {
    match size {
        SizeClass::Small => 25_000,
        SizeClass::Medium => 100_000,
        SizeClass::Large => 300_000,
    }
}
#[derive(Clone, Serialize)]
pub struct Assignment {
    pub config: CanaryConfig,
    pub policy_id: &'static str,
    #[serde(with = "alight_types::decimal_u64")]
    pub seed: u64,
    #[serde(with = "alight_types::decimal_u64")]
    pub draw: u64,
    pub assignment_prob: f64,
    pub uniform_arm: bool,
}
#[derive(Clone)]
pub struct Policy {
    cells: Vec<CanaryConfig>,
    counts: Vec<u64>,
    uniform_fraction: f64,
    seed: u64,
    draw: u64,
    state: u64,
}
impl Policy {
    /// 81 cells: two tipped routes × four tips × three fees × three sizes, plus nine RPC cells.
    pub fn new(seed: u64, uniform_fraction: f64) -> Result<Self, PolicyError> {
        if !uniform_fraction.is_finite() || !(0.01..=1.0).contains(&uniform_fraction) {
            return Err(PolicyError);
        }
        let mut cells = Vec::new();
        for route in [Route::BeamQuic, Route::BeamHttp, Route::Rpc] {
            let tips: &[(TipTier, u64)] = if route == Route::Rpc {
                &[(TipTier::None, 0)]
            } else {
                &[
                    (TipTier::X1, 1),
                    (TipTier::X2, 2),
                    (TipTier::X5, 5),
                    (TipTier::X10, 10),
                ]
            };
            for &(tip_tier, multiplier) in tips {
                for fee_bucket in [FeeBucket::Zero, FeeBucket::LocalMedian, FeeBucket::LocalP90] {
                    for size_class in [SizeClass::Small, SizeClass::Medium, SizeClass::Large] {
                        cells.push(CanaryConfig {
                            route,
                            tip_tier,
                            tip_lamports: multiplier * MIN_TIP_LAMPORTS,
                            fee_bucket,
                            cu_price_micro_lamports: 0,
                            size_class,
                            cu_limit: cu_limit(size_class),
                        });
                    }
                }
            }
        }
        Ok(Self {
            counts: vec![0; cells.len()],
            cells,
            uniform_fraction,
            seed,
            draw: 0,
            state: seed,
        })
    }
    pub fn cells(&self) -> &[CanaryConfig] {
        &self.cells
    }
    /// Marginal propensity includes both arms, computed before the assignment count changes.
    pub fn probabilities(&self) -> Vec<f64> {
        let weights: Vec<f64> = self
            .counts
            .iter()
            .map(|c| 1.0 / (1.0 + *c as f64))
            .collect();
        let sum: f64 = weights.iter().sum();
        weights
            .iter()
            .map(|w| {
                self.uniform_fraction / self.cells.len() as f64
                    + (1.0 - self.uniform_fraction) * w / sum
            })
            .collect()
    }
    fn random(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Fees are observed micro-lamports per CU; SOL floats are never involved.
    pub fn assign(&mut self, median: u64, p90: u64) -> Assignment {
        let probabilities = self.probabilities();
        let uniform_arm = self.random() < self.uniform_fraction;
        let random = self.random();
        let index = if uniform_arm {
            ((random * self.cells.len() as f64) as usize).min(self.cells.len() - 1)
        } else {
            let weights: Vec<f64> = self
                .counts
                .iter()
                .map(|c| 1.0 / (1.0 + *c as f64))
                .collect();
            let target = random * weights.iter().sum::<f64>();
            let mut total = 0.0;
            weights
                .iter()
                .position(|w| {
                    total += w;
                    total > target
                })
                .unwrap_or(self.cells.len() - 1)
        };
        let mut config = self.cells[index].clone();
        config.cu_price_micro_lamports = match config.fee_bucket {
            FeeBucket::Zero => 0,
            FeeBucket::LocalMedian => median,
            FeeBucket::LocalP90 => p90,
        };
        let result = Assignment {
            config,
            policy_id: POLICY_ID,
            seed: self.seed,
            draw: self.draw,
            assignment_prob: probabilities[index],
            uniform_arm,
        };
        self.counts[index] = self.counts[index].saturating_add(1);
        self.draw = self.draw.saturating_add(1);
        result
    }
    /// Recreates the deterministic stream up to the next durable draw.
    pub fn resume(&mut self, draws: u64) -> Result<(), PolicyError> {
        if draws > 10_000_000 {
            return Err(PolicyError);
        }
        for _ in 0..draws {
            self.assign(0, 0);
        }
        Ok(())
    }
}

/// Approximate Beta Thompson draws using posterior mean/variance, with a fixed draw count.
/// The Monte Carlo proposal is frozen before choosing the arm; its conditional propensity is exact.
#[derive(Clone)]
pub struct AdaptivePolicy {
    cells: Vec<CanaryConfig>,
    seed: u64,
    draw: u64,
    state: u64,
    uniform_fraction: f64,
}
impl AdaptivePolicy {
    const PROPOSALS: usize = 32;
    pub fn new(seed: u64, uniform_fraction: f64) -> Result<Self, PolicyError> {
        if !uniform_fraction.is_finite() || !(0.30..=1.0).contains(&uniform_fraction) {
            return Err(PolicyError);
        }
        Ok(Self {
            cells: Policy::new(seed, uniform_fraction)?.cells,
            seed,
            draw: 0,
            state: seed,
            uniform_fraction,
        })
    }
    pub fn cells(&self) -> &[CanaryConfig] {
        &self.cells
    }
    fn random(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Fixed random draw count makes resume O(1), independent of changing resolved outcomes.
    pub fn resume(&mut self, draw: u64) -> Result<(), PolicyError> {
        if draw > 10_000_000 {
            return Err(PolicyError);
        }
        self.draw = draw;
        let per_draw = (Self::PROPOSALS * self.cells.len() * 2 + 2) as u64;
        self.state = self
            .seed
            .wrapping_add(draw.wrapping_mul(per_draw).wrapping_mul(0x9e3779b97f4a7c15));
        Ok(())
    }
    pub fn assign(
        &mut self,
        median: u64,
        p90: u64,
        posterior: &[(f64, f64)],
    ) -> Result<Assignment, PolicyError> {
        if posterior.len() != self.cells.len()
            || posterior
                .iter()
                .any(|(a, b)| !a.is_finite() || !b.is_finite() || *a < 1.0 || *b < 1.0)
        {
            return Err(PolicyError);
        }
        let mut cells = self.cells.clone();
        for c in &mut cells {
            c.cu_price_micro_lamports = match c.fee_bucket {
                FeeBucket::Zero => 0,
                FeeBucket::LocalMedian => median,
                FeeBucket::LocalP90 => p90,
            };
        }
        let mut wins = vec![0u32; cells.len()];
        for _ in 0..Self::PROPOSALS {
            let mut best = 0;
            let mut best_utility = f64::NEG_INFINITY;
            for (i, (a, b)) in posterior.iter().enumerate() {
                let normal = (-2.0 * self.random().max(f64::MIN_POSITIVE).ln()).sqrt()
                    * (std::f64::consts::TAU * self.random()).cos();
                let mean = a / (a + b);
                let sd = (a * b / ((a + b).powi(2) * (a + b + 1.0))).sqrt();
                let p = (mean + sd * normal).clamp(0.0, 1.0);
                let utility = p - alight_model::quote::nominal_cost(&cells[i]) as f64 / 500_000.0;
                if utility > best_utility {
                    best = i;
                    best_utility = utility;
                }
            }
            wins[best] += 1;
        }
        let probabilities: Vec<_> = wins
            .iter()
            .map(|w| {
                self.uniform_fraction / cells.len() as f64
                    + (1.0 - self.uniform_fraction) * f64::from(*w) / Self::PROPOSALS as f64
            })
            .collect();
        let uniform_arm = self.random() < self.uniform_fraction;
        let random = self.random();
        let index = if uniform_arm {
            ((random * cells.len() as f64) as usize).min(cells.len() - 1)
        } else {
            let mut sum = 0.0;
            wins.iter()
                .position(|w| {
                    sum += f64::from(*w) / Self::PROPOSALS as f64;
                    sum > random
                })
                .unwrap_or(cells.len() - 1)
        };
        let assignment = Assignment {
            config: cells[index].clone(),
            policy_id: ADAPTIVE_POLICY_ID,
            seed: self.seed,
            draw: self.draw,
            assignment_prob: probabilities[index],
            uniform_arm,
        };
        self.draw += 1;
        Ok(assignment)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seeded_propensities_stay_normalized_uniform_and_reproducible() {
        for seed in [0, 1, 42, u64::MAX] {
            let mut policy = Policy::new(seed, 0.30).expect("policy");
            let mut uniform = 0;
            for _ in 0..10_000 {
                let probabilities = policy.probabilities();
                assert!((probabilities.iter().sum::<f64>() - 1.0).abs() < 1e-12);
                assert!(probabilities.iter().all(|p| *p >= 0.30 / 81.0));
                let a = policy.assign(100, 900);
                uniform += u64::from(a.uniform_arm);
            }
            assert!((uniform as f64 / 10_000.0 - 0.30).abs() < 0.025);
            let mut resumed = Policy::new(seed, 0.30).expect("resume");
            resumed.resume(10_000).expect("draws");
            assert_eq!(
                serde_json::to_value(policy.assign(100, 900)).expect("encode"),
                serde_json::to_value(resumed.assign(100, 900)).expect("encode")
            );
        }
    }
}
