//! Passive evidence and the combined quote service; no signing material is a contract.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PassiveInstruction {
    pub program_id_index: u32,
    pub accounts: Vec<u32>,
    pub data_base64: String,
    pub outer_index: u32,
    pub inner_index: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PassiveTransaction {
    pub source: Source,
    pub observer: ObserverKind,
    pub signature: String,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub slot: u64,
    pub block_id: Option<String>,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub index_in_block: Option<u64>,
    pub index_scope: IndexScope,
    pub success: bool,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub fee_lamports: u64,
    pub account_keys: Vec<String>,
    pub instructions: Vec<PassiveInstruction>,
    pub received: ReceiveTime,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PassiveTip {
    pub source: Source,
    pub observer: ObserverKind,
    pub signature: String,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub slot: u64,
    pub block_id: Option<String>,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub index_in_block: Option<u64>,
    pub index_scope: IndexScope,
    pub recipient: String,
    /// Successful outer transfers are paid; inner transfers require execution evidence.
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub tip_lamports: Option<u64>,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub requested_tip_lamports: u64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub fee_lamports: u64,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub cu_price_micro_lamports: Option<u64>,
    pub cu_limit: Option<u32>,
    pub success: bool,
    pub received: ReceiveTime,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TapeLimits {
    pub retention_s: u32,
    pub max_rows: u32,
    pub max_bytes: u32,
}
impl Default for TapeLimits {
    fn default() -> Self {
        Self {
            retention_s: 86400,
            max_rows: 10000,
            max_bytes: 16 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TapeUsage {
    pub rows: u32,
    pub payload_bytes: u32,
    pub limits: TapeLimits,
    pub population: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QuoteServiceRequest {
    pub model: ModelQuoteRequest,
    pub ttl_s: u32,
    pub economics: Option<EconomicsInputs>,
    pub frozen_model_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProveRequest {
    pub request_id: String,
    pub forecast_hash: String,
    pub n: u32,
    #[serde(default, with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub seed: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProveLock {
    pub id: String,
    pub source: Source,
    pub forecast_hash: String,
    pub model_snapshot_hash: String,
    pub methodology_hash: String,
    pub config: CanaryConfig,
    pub regime_id: String,
    pub region: String,
    pub locked_at_utc: String,
    pub expires_at_utc: String,
    pub target: PredictionTarget,
    pub claimed_probability: f64,
    pub n: u32,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub seed: Option<u64>,
    pub simulation: Option<SimProofEnvironment>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SimProofEnvironment {
    pub slot_ms: u32,
    pub congestion: f64,
    pub tip_slope: f64,
    pub fee_slope: f64,
    pub never_land_mass: Option<f64>,
    pub continuous_latency: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProveState {
    Locked,
    Running,
    WaitingFunds,
    WaitingObserver,
    BudgetCapped,
    Complete,
    Voided,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProveVerdict {
    Consistent,
    Inconsistent,
    Inconclusive,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProveReport {
    pub lock: ProveLock,
    pub lock_hash: String,
    pub as_of_utc: String,
    pub state: ProveState,
    pub attempts: u32,
    pub resolved: u32,
    pub successes: u32,
    pub unresolved: u32,
    pub observed_rate: Option<f64>,
    pub wilson_interval_95: Option<[f64; 2]>,
    pub verdict: ProveVerdict,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QuotePreview {
    pub quote: ModelQuote,
    pub economics: Option<EconomicsQuote>,
    pub baselines: Vec<BaselineForecast>,
    pub requested_economics: Option<EconomicsInputs>,
    pub model_snapshot_hash: String,
}
