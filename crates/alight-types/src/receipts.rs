//! Bounded wallet replay and conditional historical tip comparisons; amounts are lamports.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WalletHistoryRow {
    pub transaction: PassiveTransaction,
    /// Chain timestamp, not observer receive time. Missing time prevents comparison.
    pub chain_time_utc: Option<String>,
    /// Transport cannot be established merely from a recipient payment.
    pub route: Option<Route>,
    pub size_class: Option<SizeClass>,
    pub regime_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WalletHistoryCapture {
    pub schema_version: u32,
    pub source: Source,
    pub wallet: String,
    pub from_utc: String,
    pub through_utc: String,
    pub tip_recipients: Vec<String>,
    pub rows: Vec<WalletHistoryRow>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WalletReceiptRequest {
    pub region: String,
    pub target_p: f64,
    pub horizon_slots: u32,
    pub max_curve_age_s: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReceiptComparison {
    pub snapshot_id: String,
    pub snapshot: CurveSnapshot,
    pub age_at_transaction_s: f64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub spend_above_threshold_lamports: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WalletReceiptRow {
    pub signature: String,
    pub chain_time_utc: Option<String>,
    pub success: bool,
    pub route: Option<Route>,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub fee_lamports: u64,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub paid_tip_lamports: Option<u64>,
    pub comparison: Option<ReceiptComparison>,
    pub unavailable_reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WalletReceipt {
    pub schema_version: u32,
    pub source: Source,
    pub capture_hash: String,
    pub wallet: String,
    pub from_utc: String,
    pub through_utc: String,
    pub request: WalletReceiptRequest,
    pub population: String,
    pub transactions: u32,
    pub duplicates_removed: u32,
    pub failed_transactions: u32,
    pub visible_failed_share: Option<f64>,
    pub unknown_tip_payments: u32,
    pub compared_transactions: u32,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub fees_lamports: u64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub known_paid_tips_lamports: u64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub spend_above_threshold_lamports: u64,
    pub threshold_definition: String,
    pub rows: Vec<WalletReceiptRow>,
    pub limits: Vec<String>,
}
