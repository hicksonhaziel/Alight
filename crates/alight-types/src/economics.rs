//! Market evidence and conditional USD economics. Raw prices remain decimal text.
use crate::{CanaryConfig, CurveContext, CurvePrediction, LatencyEstimate, ModelQuote, Source};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BlurPool {
    pub pool: String,
    pub mint: String,
    pub quote_mint: String,
    pub dex: String,
    pub price_usd: String,
    pub tvl_usd: String,
}

/// Indices retain Blur's provider scope; block_time_unix_s has second precision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MarketTrade {
    pub source: Source,
    pub pool: String,
    pub mint: String,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub slot: u64,
    pub block_time_unix_s: i64,
    pub signature: String,
    pub tx_index: u32,
    pub ix_index: u32,
    pub inner_ix_index: Option<i32>,
    pub price_usd: String,
    pub candle_ok: bool,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub base_amount: u64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub quote_amount: u64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub base_reserve: u64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub quote_reserve: u64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub fee_amount: u64,
}

/// Captured one-minute candles are retained separately from slot-based estimates.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MarketCandle {
    pub source: Source,
    pub pool: String,
    pub mint: String,
    pub time_unix_s: i64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
    pub trades: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BlurEvent {
    Connected {
        source: Source,
        region: Option<String>,
    },
    Trade {
        trade: MarketTrade,
    },
    Disconnected {
        source: Source,
        reason: String,
    },
}

/// Return magnitudes are basis points, delay_ms uses the supplied measured slot clock.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DelayCostPoint {
    pub delay_slots: u32,
    pub delay_ms: f64,
    pub median_bps: Option<f64>,
    pub upper_bps: Option<f64>,
    pub pairs: u32,
    pub split_half_relative_change: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DelayCostSnapshot {
    pub source: Source,
    pub regime_id: String,
    pub pool: String,
    pub mint: String,
    pub as_of_utc: String,
    pub window_s: u32,
    pub measured_slot_ms: f64,
    pub upper_quantile: f64,
    pub max_age_s: f64,
    pub minimum_pairs: u32,
    pub observation_window: Option<[String; 2]>,
    pub data_age_s: Option<f64>,
    pub trade_samples: u32,
    pub slot_samples: u32,
    pub disconnected: bool,
    pub stale: bool,
    pub sparse: bool,
    pub points: Vec<DelayCostPoint>,
}

/// Unconditional masses, including nonlanding. They are scoped to owned canary evidence.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LandingMass {
    pub delay_slots: u32,
    pub probability: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LandingDistribution {
    pub context: CurveContext,
    pub n_effective: f64,
    pub data_age_s: f64,
    pub masses: Vec<LandingMass>,
    pub nonlanding_probability: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EconomicCandidate {
    pub prediction: CurvePrediction,
    pub latency: Option<LatencyEstimate>,
    pub distribution: LandingDistribution,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EconomicBaseline {
    pub id: String,
    pub candidate: Option<EconomicCandidate>,
    pub unavailable_reason: Option<String>,
}

/// USD size/conversion are decimal text; edge is bps; lambda is dimensionless.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EconomicsInputs {
    pub pool: String,
    pub size_usd: String,
    pub sol_usd: String,
    pub edge_bps: f64,
    pub lambda: f64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub base_fee_lamports: u64,
    pub use_upper_quantile: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EconomicCost {
    pub prediction: CurvePrediction,
    pub latency: Option<LatencyEstimate>,
    pub qualifies: bool,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub nominal_lamports: u64,
    pub nominal_spend_usd: String,
    pub delay_cost_usd: String,
    pub upper_delay_cost_usd: String,
    pub missed_edge_usd: String,
    pub expected_cost_usd: String,
    pub upper_cost_usd: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BaselineCostComparison {
    pub id: String,
    pub cost: Option<EconomicCost>,
    /// Positive means the baseline costs more under the chosen objective.
    pub savings_usd: Option<String>,
    pub unavailable_reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EconomicsSummary {
    pub inputs: EconomicsInputs,
    pub market: DelayCostSnapshot,
    pub assumption: String,
    pub evaluated: Vec<EconomicCost>,
    pub frontier: Vec<EconomicCost>,
    pub knee: Option<CanaryConfig>,
    pub recommendation: EconomicCost,
    pub baselines: Vec<BaselineCostComparison>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EconomicsQuote {
    pub model_quote: ModelQuote,
    pub economics: Option<EconomicsSummary>,
    pub fallback_reason: Option<String>,
}
