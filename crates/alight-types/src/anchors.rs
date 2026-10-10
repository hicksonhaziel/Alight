//! Unsigned ledger-head commitments. Preparation does not establish an on-chain timestamp.
use crate::Source;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnchorDraft {
    pub schema_version: u32,
    pub source: Source,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub sequence: u64,
    pub head_hash: String,
    pub memo: String,
    pub status: String,
    pub signing_enabled: bool,
    pub signature: Option<String>,
    pub explorer_url: Option<String>,
}

/// Immutable attempt persisted before broadcast; all money is lamports.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnchorAttempt {
    pub schema_version: u32,
    pub reservation_id: String,
    pub commitment: AnchorDraft,
    pub payer: String,
    pub signature: String,
    pub prepared_at_utc: String,
    pub wire_sha256: String,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub quoted_fee_lamports: u64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub last_valid_block_height: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnchorEvent {
    pub reservation_id: String,
    pub at_utc: String,
    pub status: String,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub confirmed_slot: Option<u64>,
    #[serde(with = "crate::optional_decimal_u64")]
    #[schemars(with = "Option<crate::DecimalU64>")]
    pub actual_fee_lamports: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AnchorRecord {
    pub source: Source,
    pub attempt: AnchorAttempt,
    pub event: AnchorEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AnchorPage {
    pub source: Source,
    pub records: Vec<AnchorRecord>,
}
