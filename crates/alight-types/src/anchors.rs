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
