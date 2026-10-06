//! REST/WS response contracts. Telemetry values are sanitized local snapshots, not credentials.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ApiErrorResponse {
    pub source: Source,
    pub code: String,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HealthCounts {
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub slot_events: u64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub blocks: u64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub observations: u64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub gaps: u64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub open_gaps: u64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub canaries: u64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub budget_reserved_lamports: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ObserverHealthView {
    pub source: Source,
    pub observer: ObserverKind,
    pub status: String,
    pub last_receive_utc: Option<String>,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub age_ms: Option<u64>,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub cursor_slot: Option<u64>,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub open_gaps: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ObserverHealthPage {
    pub source: Source,
    pub observers: Vec<ObserverHealthView>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ApiHealth {
    pub schema_version: u32,
    pub source: Source,
    pub mode: RunMode,
    pub run_id: String,
    pub as_of_utc: String,
    pub region: String,
    pub status: String,
    pub collector_status: String,
    pub signing_enabled: bool,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub uptime_s: u64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub canaries_sent: u64,
    pub counts: HealthCounts,
    pub observers: Vec<ObserverHealthView>,
    pub send_attempts: BTreeMap<String, u32>,
    pub budget_reserved_today_by_route: BTreeMap<String, String>,
    pub canary_engine: serde_json::Value,
    pub leaders: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ClockWindow {
    pub sampled_slots: u32,
    pub ambiguous_candidate_slots: u32,
    pub dead_slots: u32,
    pub candidate_parent_skipped_slots: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ApiClock {
    pub source: Source,
    pub method: String,
    pub mean_slot_ms: Option<f64>,
    pub window: ClockWindow,
    pub minimum_slot_distance: u32,
    pub minimum_chain_seconds: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ApiTelemetry {
    pub source: Source,
    pub value: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CurvePage {
    pub source: Source,
    pub regime_id: String,
    pub curves: Vec<CurveSnapshot>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LedgerPage {
    pub source: Source,
    pub entries: Vec<ForecastEntry>,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub next_after: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LedgerVerification {
    pub source: Source,
    pub verified: bool,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub entries: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TapePage {
    pub source: Source,
    pub from: String,
    pub through: String,
    pub rows: Vec<PassiveTip>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProvePage {
    pub source: Source,
    pub reports: Vec<ProveReport>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ApiStreamSnapshot {
    pub kind: String,
    pub source: Source,
    pub as_of_utc: String,
    pub health: ApiHealth,
    pub clock: ApiClock,
    pub proves: Vec<ProveReport>,
    pub ledger: Vec<ForecastEntry>,
}

/// Phase 3 bounded forecast-only export; whole-chain verification is performed before pagination.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ForecastLedgerExport {
    pub schema_version: u32,
    pub source: Source,
    pub day: String,
    pub kind: String,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub verified_through_sequence: u64,
    pub entries: Vec<ForecastEntry>,
}

/// Bounded read-only workbench evidence. A regime label is not a detected network change.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkbenchEvidence {
    pub source: Source,
    pub as_of_utc: String,
    pub canaries: Vec<WorkbenchCanary>,
    pub grades: Vec<ForecastGrade>,
    pub regimes: Vec<WorkbenchRegime>,
    pub gaps: Vec<ObserverGap>,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub daily_cap_lamports: u64,
    pub limit: u32,
    pub operator_enabled: bool,
    pub runway: Vec<LeaderSlot>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkbenchCanary {
    pub canary: Canary,
    pub finalized: bool,
    pub prove_id: Option<String>,
    pub resolution_reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkbenchRegime {
    pub id: String,
    pub first_observed_utc: String,
    pub last_observed_utc: String,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub canaries: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CanaryEvidencePage {
    pub source: Source,
    pub canary_id: String,
    pub observations: Vec<ObserverEvent>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProveCanaryPage {
    pub source: Source,
    pub id: String,
    /// Ordered prospective members; no coincidentally matching records.
    pub canaries: Vec<TrainingCanary>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BrowserLedgerRow {
    pub entry: ForecastEntry,
    /// Exact Rust canonical bytes preserve float spelling for independent browser hashing.
    pub canonical_json: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BrowserLedgerPage {
    pub source: Source,
    pub rows: Vec<BrowserLedgerRow>,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub next_after: Option<u64>,
}
