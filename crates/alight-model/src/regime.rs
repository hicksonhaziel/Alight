//! Bounded Adams–MacKay BOCPD with Normal–Inverse-Gamma predictive distributions.
//! CUSUM is an independently reported baseline. Calendar annotations never trigger detection.
use crate::{ModelError, beta::log_gamma};
use alight_types::{ChangeVote, SignalKind, SignalWindow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const HAZARD: f64 = 1.0 / 500.0;
const MAX_RUN: usize = 256;
const WARMUP: u32 = 32;
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Posterior {
    mu: f64,
    kappa: f64,
    alpha: f64,
    beta: f64,
}
impl Posterior {
    fn updated(&self, x: f64) -> Self {
        let k = self.kappa + 1.0;
        Self {
            mu: (self.kappa * self.mu + x) / k,
            kappa: k,
            alpha: self.alpha + 0.5,
            beta: self.beta + self.kappa * (x - self.mu).powi(2) / (2.0 * k),
        }
    }
    fn log_predictive(&self, x: f64) -> f64 {
        let df = 2.0 * self.alpha;
        let scale2 = self.beta * (self.kappa + 1.0) / (self.alpha * self.kappa);
        log_gamma((df + 1.0) / 2.0)
            - log_gamma(df / 2.0)
            - 0.5 * (df * std::f64::consts::PI * scale2).ln()
            - (df + 1.0) / 2.0 * ((x - self.mu).powi(2) / (df * scale2)).ln_1p()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Channel {
    n: u32,
    mean: f64,
    m2: f64,
    prior: Option<Posterior>,
    probabilities: Vec<f64>,
    parameters: Vec<Posterior>,
    cusum_up: f64,
    cusum_down: f64,
    persistent: u32,
}
fn floor(kind: SignalKind, mean: f64) -> f64 {
    match kind {
        SignalKind::SlotMs | SignalKind::SlotP95Ms => (mean.abs() * 0.015).max(2.0),
        SignalKind::ObserverLagMs => mean.abs().mul_add(0.1, 2.0),
        _ => 0.02,
    }
}
impl Channel {
    fn push(&mut self, kind: SignalKind, x: f64) -> Option<ChangeVote> {
        if self.n < WARMUP {
            self.n += 1;
            let d = x - self.mean;
            self.mean += d / f64::from(self.n);
            self.m2 += d * (x - self.mean);
            if self.n == WARMUP {
                let variance =
                    (self.m2 / f64::from(self.n - 1)).max(floor(kind, self.mean).powi(2));
                let prior = Posterior {
                    mu: self.mean,
                    kappa: 0.01,
                    alpha: 2.0,
                    beta: variance * 2.0,
                };
                let mut fitted = prior.clone();
                fitted.kappa += f64::from(self.n);
                fitted.alpha += f64::from(self.n) / 2.0;
                fitted.beta += self.m2 / 2.0;
                self.prior = Some(prior);
                self.parameters = vec![fitted];
                self.probabilities = vec![1.0];
            }
            return None;
        }
        let prior = self.prior.as_ref()?;
        let log_weights: Vec<_> = self
            .probabilities
            .iter()
            .zip(&self.parameters)
            .map(|(p, s)| p.max(1e-300).ln() + s.log_predictive(x))
            .collect();
        let maximum = log_weights
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let weights: Vec<_> = log_weights.iter().map(|p| (p - maximum).exp()).collect();
        let sum = weights.iter().sum::<f64>();
        let mut probabilities = vec![HAZARD * sum];
        probabilities.extend(weights.iter().map(|p| p * (1.0 - HAZARD)));
        let mut parameters = vec![prior.clone()];
        parameters.extend(self.parameters.iter().map(|s| s.updated(x)));
        // Bounded run-length approximation: discarded tail mass is renormalized.
        if probabilities.len() > MAX_RUN {
            probabilities.truncate(MAX_RUN);
            parameters.truncate(MAX_RUN);
        }
        let norm = probabilities.iter().sum::<f64>();
        for p in &mut probabilities {
            *p /= norm;
        }
        self.probabilities = probabilities;
        self.parameters = parameters;
        self.n = self.n.saturating_add(1);
        let sigma = (self.m2 / f64::from(WARMUP - 1))
            .sqrt()
            .max(floor(kind, self.mean));
        let z = (x - self.mean) / sigma;
        self.cusum_up = (self.cusum_up + z - 0.5).max(0.0);
        self.cusum_down = (self.cusum_down - z - 0.5).max(0.0);
        let short = self.probabilities.iter().skip(1).take(8).sum::<f64>();
        let effect = (x - self.mean).abs();
        let minimum = match kind {
            SignalKind::SlotMs | SignalKind::SlotP95Ms => self.mean.abs() * 0.05,
            SignalKind::ObserverLagMs => 10.0,
            _ => 0.05,
        };
        if short >= 0.80
            && effect >= minimum
            && z.abs() >= 3.0
            && self.cusum_up.max(self.cusum_down) >= 12.0
        {
            self.persistent += 1;
        } else {
            self.persistent = 0;
        }
        (self.persistent >= 2).then_some(ChangeVote {
            signal: kind,
            baseline: self.mean,
            recent: x,
            short_run_probability: short,
            cusum: self.cusum_up.max(self.cusum_down),
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Detector {
    channels: BTreeMap<SignalKind, Channel>,
    last_utc: Option<String>,
    last_id: Option<String>,
    source: Option<alight_types::Source>,
    origin: Option<alight_types::SignalOrigin>,
}
impl Detector {
    pub fn last_time(&self) -> Option<&str> {
        self.last_utc.as_deref()
    }
    /// One chronological, nonduplicated source window. Returned votes are evidence, not upgrade names.
    pub fn push(&mut self, window: &SignalWindow) -> Result<Vec<ChangeVote>, ModelError> {
        if !window.origin.matches(window.source) {
            return Err(ModelError::Invalid);
        }
        if self.source.is_some_and(|s| s != window.source)
            || self.origin.is_some_and(|o| o != window.origin)
        {
            return Err(ModelError::Invalid);
        }
        let now = crate::utc(&window.through_utc)?;
        if crate::utc(&window.from_utc)? > now {
            return Err(ModelError::Invalid);
        }
        if self.last_id.as_deref() == Some(&window.id) {
            return Ok(vec![]);
        }
        if self
            .last_utc
            .as_deref()
            .is_some_and(|t| crate::utc(t).is_ok_and(|t| t >= now))
        {
            return Err(ModelError::Invalid);
        }
        let mut names = std::collections::BTreeSet::new();
        for m in &window.measures {
            if !names.insert(m.kind) || m.value.is_some_and(|v| !v.is_finite() || m.n == 0) {
                return Err(ModelError::Invalid);
            }
        }
        self.source = Some(window.source);
        self.origin = Some(window.origin);
        let mut votes = Vec::new();
        for m in &window.measures {
            if let Some(x) = m.value {
                if !x.is_finite() || m.n == 0 {
                    return Err(ModelError::Invalid);
                }
                if let Some(v) = self.channels.entry(m.kind).or_default().push(m.kind, x) {
                    votes.push(v);
                }
            }
        }
        self.last_utc = Some(window.through_utc.clone());
        self.last_id = Some(window.id.clone());
        // Slot median/p95 are the same signal family. Observer-only changes cannot rename network regimes.
        let families: std::collections::BTreeSet<_> = votes
            .iter()
            .filter_map(|v| match v.signal {
                SignalKind::SlotMs | SignalKind::SlotP95Ms => Some(0),
                SignalKind::SkipRate => Some(1),
                SignalKind::ReferenceLandingRate => Some(2),
                SignalKind::BlockFullness => Some(3),
                SignalKind::NonVoteShare => Some(4),
                SignalKind::ObserverLagMs => None,
            })
            .collect();
        let strong_clock = votes.iter().any(|v| {
            v.signal == SignalKind::SlotMs
                && v.short_run_probability >= 0.95
                && self
                    .channels
                    .get(&v.signal)
                    .is_some_and(|c| c.persistent >= 3)
        });
        if families.len() < 2 && !strong_clock {
            return Ok(vec![]);
        }
        self.channels.clear();
        Ok(votes)
    }
}
