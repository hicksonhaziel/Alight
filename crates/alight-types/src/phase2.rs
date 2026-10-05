use crate::{CanaryConfig, CurveContext, Evidence, ForecastStatus, SignalVerdict, Source};
use serde::{Deserialize, Serialize};

/// Congestion is a dimensionless measured covariate; unknown is not zero.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelCovariates {
    pub congestion: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFit {
    pub context: CurveContext,
    pub horizon_slots: u32,
    pub coefficients: Vec<f64>,
    pub covariance: Vec<Vec<f64>>,
    pub n_effective: f64,
    pub observations: u32,
    pub latest_send_utc: Option<String>,
    pub support: Vec<CanaryConfig>,
    pub holdout_m0_log_loss: Option<f64>,
    pub holdout_m1_log_loss: Option<f64>,
    pub m1_weight: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurvePrediction {
    pub config: CanaryConfig,
    pub p_hat: f64,
    pub p_interval_95: [f64; 2],
    pub evidence: Evidence,
    pub n_effective: f64,
    pub data_age_s: Option<f64>,
    pub observation_window: Option<[String; 2]>,
    pub unresolved_share: f64,
    pub samples_needed: Option<u32>,
    pub m1_weight: f64,
}
/// Infinity/unidentifiable quantiles are null, never silently excluded failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyEstimate {
    pub quantile: f64,
    pub slots: Option<f64>,
    pub slots_interval_95: [Option<f64>; 2],
    pub ms: Option<f64>,
    pub ms_interval_95: [Option<f64>; 2],
    pub n_effective: f64,
    pub evidence: Evidence,
    pub samples_needed: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PredictionTarget {
    Probability {
        target_p: f64,
        horizon_slots: u32,
    },
    LatencyQuantile {
        quantile: f64,
        max_slots: Option<f64>,
        max_ms: Option<f64>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelQuoteRequest {
    pub context: CurveContext,
    pub candidates: Vec<CanaryConfig>,
    pub covariates: ModelCovariates,
    #[serde(default)]
    pub leader_class_next: Vec<crate::LeaderClass>,
    pub target: PredictionTarget,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelQuote {
    pub contract_version: u32,
    pub methodology_hash: String,
    pub context: CurveContext,
    pub target: PredictionTarget,
    pub evidence: Evidence,
    pub recommendation: Option<CurvePrediction>,
    pub latency: Option<LatencyEstimate>,
    pub samples_needed: Option<u32>,
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineForecast {
    pub id: String,
    pub config: Option<CanaryConfig>,
    pub p_hat: Option<f64>,
    pub model_snapshot_hash: Option<String>,
    pub unavailable_reason: Option<String>,
    #[serde(default)]
    pub tape: Option<TipTapeSummary>,
}
/// Passive market observations; these never become model training denominators.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TipTapeObservation {
    pub id: String,
    pub source: Source,
    pub observed_at_utc: String,
    #[serde(with = "crate::decimal_u64")]
    pub tip_lamports: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TipTapeSummary {
    pub window: [String; 2],
    pub samples: u32,
    #[serde(with = "crate::decimal_u64")]
    pub median_lamports: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Forecast {
    pub id: String,
    pub source: Source,
    pub created_at_utc: String,
    pub expires_at_utc: String,
    pub regime_id: String,
    pub methodology_hash: String,
    pub model_snapshot_hash: String,
    pub request: ModelQuoteRequest,
    pub quote: ModelQuote,
    pub baselines: Vec<BaselineForecast>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastEntry {
    #[serde(with = "crate::decimal_u64")]
    pub sequence: u64,
    pub prev_hash: String,
    pub hash: String,
    pub forecast: Forecast,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliabilityBucket {
    pub lower: f64,
    pub upper: f64,
    pub n: u32,
    pub predicted: f64,
    pub observed: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbabilityScores {
    pub n: u32,
    pub observed_rate: f64,
    pub brier: f64,
    pub log_loss: f64,
    pub interval_coverage: Option<bool>,
    pub expected_calibration_error: f64,
    pub reliability: Vec<ReliabilityBucket>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastGrade {
    pub forecast_hash: String,
    pub graded_at_utc: String,
    pub status: ForecastStatus,
    pub unresolved: u32,
    pub scores: Option<ProbabilityScores>,
    pub through_change_scores: Option<ProbabilityScores>,
    pub latency_coverage: Option<f64>,
    pub baselines: Vec<(String, Option<ProbabilityScores>)>,
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalEffect {
    pub name: String,
    pub estimate: Option<f64>,
    pub interval_99: [Option<f64>; 2],
    pub practical_threshold: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalCell {
    pub route: crate::Route,
    pub size_class: crate::SizeClass,
    pub uniform_samples: u32,
    pub excluded: u32,
    pub verdict: SignalVerdict,
    pub effects: Vec<SignalEffect>,
    pub sensitivity_effects: Vec<SignalEffect>,
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalReport {
    pub source: Source,
    pub day: String,
    pub methodology_hash: String,
    pub as_of_utc: String,
    pub completed_utc_day: bool,
    pub bootstrap_replicates: u32,
    pub cells: Vec<SignalCell>,
}
