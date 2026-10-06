//! Timestamped diagnostics. Missing measurements remain null with an explicit reason.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    SlotMs,
    SlotP95Ms,
    SkipRate,
    ReferenceLandingRate,
    BlockFullness,
    NonVoteShare,
    ObserverLagMs,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SignalOrigin {
    Live,
    Simulation,
    Replay,
    Backfill,
}
impl SignalOrigin {
    pub fn matches(self, source: Source) -> bool {
        matches!(
            (self, source),
            (Self::Live, Source::Live)
                | (Self::Simulation, Source::Sim)
                | (Self::Replay | Self::Backfill, Source::Replay)
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SignalMeasure {
    pub kind: SignalKind,
    pub value: Option<f64>,
    pub unit: String,
    pub n: u32,
    pub unavailable_reason: Option<String>,
    pub provenance: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SignalWindow {
    pub id: String,
    pub source: Source,
    pub origin: SignalOrigin,
    pub from_utc: String,
    pub through_utc: String,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub start_slot: Option<u64>,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub end_slot: Option<u64>,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub epoch: Option<u64>,
    pub measures: Vec<SignalMeasure>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BlockDiagnostic {
    pub source: Source,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub slot: u64,
    pub block_id: String,
    pub at_utc: String,
    pub transactions: u32,
    pub non_vote_transactions: Option<u32>,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub compute_units: Option<u64>,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub compute_capacity: Option<u64>,
    pub capacity_provenance: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ChangeVote {
    pub signal: SignalKind,
    pub baseline: f64,
    pub recent: f64,
    pub short_run_probability: f64,
    pub cusum: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RegimeChange {
    pub id: String,
    pub source: Source,
    pub origin: SignalOrigin,
    pub previous_regime_id: String,
    pub regime_id: String,
    pub detected_at_utc: String,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub start_slot: Option<u64>,
    pub signal_window_id: String,
    pub votes: Vec<ChangeVote>,
    pub confidence: f64,
    pub annotation: Option<String>,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub epoch: Option<u64>,
    /// Full reset is an explicit zero cap on old-regime sample mass, not relabeling old outcomes.
    pub old_effective_n_cap: f64,
    pub exploration_fraction: f64,
    pub exploration_until_utc: String,
    pub policy: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WebhookReceipt {
    pub source: Source,
    pub status: WebhookStatus,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WebhookStatus {
    Accepted,
    IgnoredUnowned,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ObserverComparison {
    pub observer: ObserverKind,
    pub observations: u32,
    pub comparable_lags: u32,
    pub receive_lag_p50_ms: Option<f64>,
    pub receive_lag_p95_ms: Option<f64>,
    pub send_to_seen_p50_ms: Option<f64>,
    pub send_to_seen_p95_ms: Option<f64>,
    pub missing_owned: u32,
    pub conflicts: u32,
    pub incomparable_clocks: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ObserverPair {
    pub a: ObserverKind,
    pub b: ObserverKind,
    pub compared: u32,
    pub agreed: u32,
    pub disagreed: u32,
    pub incomplete: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DisagreementEvent {
    pub id: String,
    pub source: Source,
    pub canary_id: String,
    pub signature: String,
    pub detected_at_utc: String,
    pub kind: String,
    pub missing_observers: Vec<ObserverKind>,
    pub evidence_refs: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PositionComparison {
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub tip_lamports: u64,
    pub index_scope: IndexScope,
    pub canaries: u32,
    pub tape_transfers: u32,
    pub canary_p50_index: f64,
    pub tape_p50_index: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FidelityDiagnostic {
    pub comparisons: Vec<PositionComparison>,
    pub excluded_unmatched: u32,
    pub excluded_conflicting: u32,
    pub limits: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BackfillReport {
    pub id: String,
    pub source: Source,
    pub requested_days: u32,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub first_available_slot: u64,
    pub from_utc: Option<String>,
    pub through_utc: Option<String>,
    pub timestamps: u32,
    pub missing_timestamps: u32,
    pub requests: u32,
    pub status: String,
    pub limitation: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DiagnosticsPage {
    pub source: Source,
    pub as_of_utc: String,
    pub signals: Vec<SignalWindow>,
    pub regimes: Vec<RegimeChange>,
    pub observers: Vec<ObserverComparison>,
    pub pairs: Vec<ObserverPair>,
    pub disagreements: Vec<DisagreementEvent>,
    pub expected_observers: Vec<ObserverKind>,
    pub owned_window_n: u32,
    pub fidelity: FidelityDiagnostic,
    pub backfills: Vec<BackfillReport>,
    pub alerts: Vec<Alert>,
    pub limits: Vec<String>,
}
