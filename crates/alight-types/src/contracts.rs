use crate::{Outcome, Route, Source};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONTRACT_VERSION: u32 = 1;

macro_rules! contract_enum {
    ($name:ident, $case:literal, $($variant:ident),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(rename_all = $case)]
        pub enum $name { $($variant),+ }
    };
}
contract_enum!(
    Evidence,
    "SCREAMING_SNAKE_CASE",
    Measured,
    Interpolated,
    Extrapolated,
    Insufficient
);
contract_enum!(
    ForecastStatus,
    "SCREAMING_SNAKE_CASE",
    Pending,
    Scored,
    Voided
);
contract_enum!(ObserverKind, "snake_case", Grpc, Mirage, Rpc, Webhook);
contract_enum!(SizeClass, "snake_case", Small, Medium, Large);
contract_enum!(
    SignalVerdict,
    "SCREAMING_SNAKE_CASE",
    Discriminating,
    Flat,
    Inconclusive
);
contract_enum!(FeeBucket, "snake_case", Zero, LocalMedian, LocalP90);
contract_enum!(TipTier, "snake_case", None, X1, X2, X5, X10);
contract_enum!(
    SlotStatus,
    "SCREAMING_SNAKE_CASE",
    FirstShred,
    Completed,
    CreatedBank,
    Processed,
    Confirmed,
    Finalized,
    Dead
);
contract_enum!(Tercile, "snake_case", Low, Middle, High, Unknown);
// The captured gRPC index and finalized RPC list position disagree. Keep scope.
contract_enum!(
    IndexScope,
    "snake_case",
    ProviderReported,
    RpcBlockList,
    Unknown
);

contract_enum!(RunMode, "snake_case", Observe, Live, Sim, Replay);
contract_enum!(Commitment, "snake_case", Processed, Confirmed, Finalized);
contract_enum!(
    RouteRejection,
    "SCREAMING_SNAKE_CASE",
    InvalidTransaction,
    Authentication,
    LocalPolicy,
    InsufficientFunds,
    RateLimited,
    TipTooLow
);

/// RPC evidence is checked against this exact signature, source, and commitment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcCheck {
    pub signature: String,
    pub source: Source,
    pub required_commitment: Commitment,
    pub checked_commitment: Commitment,
    #[serde(with = "decimal_u64")]
    pub checked_block_height: u64,
    pub searched_history: bool,
    #[serde(default)]
    pub history_covers_sent_slot: bool,
    #[serde(default, with = "decimal_u64")]
    pub context_slot: u64,
    pub checked_at_utc: String,
    pub landing: Option<RpcLanding>,
    pub raw_ref: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcLanding {
    #[serde(with = "decimal_u64")]
    pub slot: u64,
    pub block_id: String,
    pub success: bool,
    pub commitment: Commitment,
}
/// Canonical RPC block membership, required before classifying a provisional landing as dropped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalBlock {
    pub source: Source,
    #[serde(with = "decimal_u64")]
    pub slot: u64,
    pub block_id: String,
    pub commitment: Commitment,
    pub signature: String,
    pub signature_present: bool,
    pub raw_ref: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResolutionEvidence {
    pub observations: Vec<ObserverEvent>,
    pub rpc: Option<RpcCheck>,
    pub canonical_blocks: Vec<CanonicalBlock>,
    pub route_rejection: Option<RouteRejection>,
}

/// Worst-case lamports reserved before signing; window_ms is a rolling burst window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetLimits {
    #[serde(with = "decimal_u64")]
    pub daily_lamports: u64,
    #[serde(with = "decimal_u64")]
    pub burst_lamports: u64,
    #[serde(with = "decimal_u64")]
    pub window_ms: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetReservation {
    pub id: String,
    pub source: Source,
    pub route: Route,
    pub day: String,
    #[serde(with = "decimal_u64")]
    pub created_ms: u64,
    #[serde(with = "decimal_u64")]
    pub lamports: u64,
}

/// A provider's candidate-block metadata; chain times are Unix seconds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockMetaEvent {
    #[serde(with = "decimal_u64")]
    pub slot: u64,
    pub block_id: String,
    #[serde(with = "optional_decimal_u64")]
    pub parent_slot: Option<u64>,
    pub parent_block_id: Option<String>,
    pub block_time_unix_s: Option<i64>,
    #[serde(with = "optional_decimal_u64")]
    pub block_height: Option<u64>,
    #[serde(with = "optional_decimal_u64")]
    pub executed_transactions: Option<u64>,
    pub received: ReceiveTime,
    pub source: Source,
    pub raw_ref: String,
}

/// Normalized ingest shapes; provider evidence stays separately addressable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum IngestEvent {
    Slot(SlotEvent),
    Observation(ObserverEvent),
    BlockMeta(BlockMetaEvent),
}

/// JSON uses decimal strings for unsigned 64-bit values so JavaScript cannot round them.
pub mod decimal_u64 {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(D::Error::custom("expected an unsigned decimal string"));
        }
        text.parse().map_err(D::Error::custom)
    }
}

/// Optional unsigned 64-bit values follow the same lossless string convention.
pub mod optional_decimal_u64 {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    pub fn serialize<S: Serializer>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(n) => serializer.serialize_some(&n.to_string()),
            None => serializer.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<u64>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|text| {
                if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(D::Error::custom("expected an unsigned decimal string"));
                }
                text.parse().map_err(D::Error::custom)
            })
            .transpose()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanaryConfig {
    pub route: Route,
    #[serde(with = "decimal_u64")]
    pub tip_lamports: u64,
    #[serde(with = "decimal_u64")]
    pub cu_price_micro_lamports: u64,
    pub cu_limit: u32,
    pub fee_bucket: FeeBucket,
    pub tip_tier: TipTier,
    pub size_class: SizeClass,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderClass {
    pub leader: String,
    pub skip_rate_tercile: Tercile,
    pub stake_tercile: Tercile,
}

/// A durable send attempt. Monotonic times are comparable only within clock_id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Canary {
    pub id: String,
    pub source: Source,
    pub config: CanaryConfig,
    pub policy_id: String,
    pub assignment_prob: f64,
    pub uniform_arm: bool,
    pub regime_id: String,
    #[serde(with = "decimal_u64")]
    pub sent_slot: u64,
    pub clock_id: String,
    #[serde(with = "decimal_u64")]
    pub send_mono_ns: u64,
    pub send_wall_utc: String,
    pub signature: Option<String>,
    pub blockhash: String,
    #[serde(with = "decimal_u64")]
    pub last_valid_block_height: u64,
    pub leader_class_next: Vec<LeaderClass>,
    pub outcome: Option<Outcome>,
    #[serde(with = "optional_decimal_u64")]
    pub landed_slot: Option<u64>,
    pub landed_block_id: Option<String>,
    pub landed_index: Option<u32>,
    pub landed_index_scope: Option<IndexScope>,
    /// Each observer has its own clock identity and UTC receive time.
    pub observer_first_seen: BTreeMap<ObserverKind, ReceiveTime>,
    pub resolved_at_utc: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiveTime {
    pub clock_id: String,
    #[serde(with = "decimal_u64")]
    pub mono_ns: u64,
    pub wall_utc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotEvent {
    #[serde(with = "decimal_u64")]
    pub slot: u64,
    pub block_id: Option<String>,
    pub status: SlotStatus,
    pub received: ReceiveTime,
    pub leader: Option<String>,
    pub source: Source,
    pub raw_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObserverEvent {
    pub observer: ObserverKind,
    pub signature: String,
    #[serde(with = "optional_decimal_u64")]
    pub slot: Option<u64>,
    pub block_id: Option<String>,
    pub index_in_block: Option<u32>,
    pub index_scope: IndexScope,
    pub success: Option<bool>,
    pub received: ReceiveTime,
    pub raw_ref: String,
    pub source: Source,
}

/// Gaps are distinct records; they never fabricate a signature or transaction outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObserverGap {
    pub observer: ObserverKind,
    pub source: Source,
    pub start_utc: String,
    pub end_utc: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteRequest {
    pub route_set: Vec<Route>,
    pub target_p: f64,
    pub within_ms: Option<u32>,
    pub within_slots: Option<u32>,
    pub size_class: SizeClass,
    pub pool: Option<String>,
    pub size_usd: Option<String>,
    pub edge_bps: Option<f64>,
    pub lambda: Option<f64>,
}
impl QuoteRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.route_set.is_empty()
            || !self.target_p.is_finite()
            || !(0.0..=1.0).contains(&self.target_p)
            || self.target_p == 0.0
        {
            return Err("routes and a probability in (0,1] are required");
        }
        if self.within_ms.is_some() == self.within_slots.is_some()
            || self.within_ms == Some(0)
            || self.within_slots == Some(0)
        {
            return Err("exactly one positive horizon is required");
        }
        if self.edge_bps.is_some_and(|v| !v.is_finite())
            || self.lambda.is_some_and(|v| !v.is_finite() || v < 0.0)
        {
            return Err("economics inputs must be finite and lambda nonnegative");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeRef {
    pub id: String,
    pub slot_ms: f64,
    pub since: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyQuantiles {
    pub p50: f64,
    pub p90: f64,
    pub p99: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    pub config: CanaryConfig,
    pub region: String,
    pub observation_window: String,
    pub p_hat: f64,
    pub p_interval_95: [f64; 2],
    pub latency_ms: Option<LatencyQuantiles>,
    pub n_effective: f64,
    pub evidence: Evidence,
    pub data_age_s: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerRef {
    pub hash: String,
    pub prev_hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Economics {
    /// Decimal USD estimate, conditional on explicitly published assumptions.
    pub expected_cost_usd: String,
    pub assumptions: Vec<String>,
    pub data_age_s: f64,
    pub stale: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteResponse {
    pub contract_version: u32,
    pub source: Source,
    pub quote_id: String,
    pub request: QuoteRequest,
    pub regime: RegimeRef,
    pub evidence: Evidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<Recommendation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub samples_needed: Option<u32>,
    pub economics: Option<Economics>,
    pub ledger: LedgerRef,
}
impl QuoteResponse {
    pub fn validate(&self) -> Result<(), &'static str> {
        self.request.validate()?;
        if self.contract_version != CONTRACT_VERSION {
            return Err("unsupported contract version");
        }
        if !self.regime.slot_ms.is_finite() || self.regime.slot_ms <= 0.0 {
            return Err("regime slot duration must be finite and positive in ms");
        }
        if self.evidence == Evidence::Insufficient {
            if self.recommendation.is_some()
                || self.samples_needed.is_none_or(|n| n == 0)
                || self.economics.is_some()
            {
                return Err(
                    "insufficient evidence must omit recommendations/economics and state samples needed",
                );
            }
        } else if self.recommendation.as_ref().map(|r| r.evidence) != Some(self.evidence) {
            return Err("recommendation and evidence must agree");
        }
        if let Some(r) = &self.recommendation {
            let [lo, hi] = r.p_interval_95;
            if ![r.p_hat, lo, hi]
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                || lo > r.p_hat
                || r.p_hat > hi
                || !r.n_effective.is_finite()
                || r.n_effective <= 0.0
                || !r.data_age_s.is_finite()
                || r.data_age_s < 0.0
                || !self.request.route_set.contains(&r.config.route)
                || self.request.size_class != r.config.size_class
            {
                return Err("invalid recommendation probabilities, evidence context, or age");
            }
            if let Some(q) = &r.latency_ms
                && (![q.p50, q.p90, q.p99]
                    .iter()
                    .all(|v| v.is_finite() && *v >= 0.0)
                    || q.p50 > q.p90
                    || q.p90 > q.p99)
            {
                return Err("latency quantiles must be finite, nonnegative, and ordered in ms");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn amounts_above_js_safe_integer_roundtrip_without_rounding() {
        let config = CanaryConfig {
            route: Route::Rpc,
            tip_lamports: u64::MAX,
            cu_price_micro_lamports: 9_007_199_254_740_993,
            cu_limit: 200_000,
            fee_bucket: FeeBucket::Zero,
            tip_tier: TipTier::None,
            size_class: SizeClass::Small,
        };
        let json = serde_json::to_value(&config).expect("serialize");
        assert_eq!(json["tip_lamports"], "18446744073709551615");
        assert_eq!(
            serde_json::from_value::<CanaryConfig>(json)
                .expect("decode")
                .cu_price_micro_lamports,
            config.cu_price_micro_lamports
        );
    }
    #[test]
    fn quote_rejects_ambiguous_horizon_and_nan_probability() {
        let mut request = QuoteRequest {
            route_set: vec![Route::Rpc],
            target_p: 0.9,
            within_ms: Some(750),
            within_slots: None,
            size_class: SizeClass::Small,
            pool: None,
            size_usd: None,
            edge_bps: None,
            lambda: None,
        };
        assert!(request.validate().is_ok());
        request.within_slots = Some(3);
        assert!(request.validate().is_err());
        request.within_slots = None;
        request.target_p = f64::NAN;
        assert!(request.validate().is_err());
    }
    #[test]
    fn insufficient_evidence_cannot_emit_a_recommendation() {
        let mut response: QuoteResponse = serde_json::from_value(serde_json::json!({
            "contract_version":1,"source":"sim","quote_id":"shape-test",
            "request":{"route_set":["rpc"],"target_p":0.9,"within_ms":750,
                "within_slots":null,"size_class":"small","pool":null,
                "size_usd":null,"edge_bps":null,"lambda":null},
            "regime":{"id":"test","slot_ms":266.0,"since":"2026-10-04T00:00:00Z"},
            "evidence":"INSUFFICIENT","samples_needed":30,"economics":null,
            "ledger":{"hash":"test-hash","prev_hash":"test-prev"}
        }))
        .expect("decode test response");
        assert!(response.validate().is_ok());
        assert!(
            serde_json::to_value(&response)
                .expect("serialize")
                .get("recommendation")
                .is_none()
        );
        response.recommendation = Some(Recommendation {
            config: CanaryConfig {
                route: Route::Rpc,
                tip_lamports: 0,
                cu_price_micro_lamports: 0,
                cu_limit: 200_000,
                fee_bucket: FeeBucket::Zero,
                tip_tier: TipTier::None,
                size_class: SizeClass::Small,
            },
            region: "test".into(),
            observation_window: "test".into(),
            p_hat: 0.9,
            p_interval_95: [0.8, 0.95],
            latency_ms: None,
            n_effective: 20.0,
            evidence: Evidence::Measured,
            data_age_s: 1.0,
        });
        assert!(response.validate().is_err());
        response.evidence = Evidence::Measured;
        response.samples_needed = None;
        assert!(response.validate().is_ok());
        response
            .recommendation
            .as_mut()
            .expect("present")
            .p_interval_95 = [0.95, 0.8];
        assert!(response.validate().is_err());
    }
}
