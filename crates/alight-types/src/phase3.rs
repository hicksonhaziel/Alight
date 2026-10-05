//! Passive evidence and the combined quote service; no signing material is a contract.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassiveInstruction {
    pub program_id_index: u32,
    pub accounts: Vec<u32>,
    pub data_base64: String,
    pub outer_index: u32,
    pub inner_index: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassiveTransaction {
    pub source: Source,
    pub observer: ObserverKind,
    pub signature: String,
    #[serde(with = "crate::decimal_u64")]
    pub slot: u64,
    pub block_id: Option<String>,
    #[serde(with = "crate::optional_decimal_u64")]
    pub index_in_block: Option<u64>,
    pub index_scope: IndexScope,
    pub success: bool,
    #[serde(with = "crate::decimal_u64")]
    pub fee_lamports: u64,
    pub account_keys: Vec<String>,
    pub instructions: Vec<PassiveInstruction>,
    pub received: ReceiveTime,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassiveTip {
    pub source: Source,
    pub observer: ObserverKind,
    pub signature: String,
    #[serde(with = "crate::decimal_u64")]
    pub slot: u64,
    pub block_id: Option<String>,
    #[serde(with = "crate::optional_decimal_u64")]
    pub index_in_block: Option<u64>,
    pub index_scope: IndexScope,
    pub recipient: String,
    /// Successful outer transfers are paid; inner transfers require execution evidence.
    #[serde(with = "crate::optional_decimal_u64")]
    pub tip_lamports: Option<u64>,
    #[serde(with = "crate::decimal_u64")]
    pub requested_tip_lamports: u64,
    #[serde(with = "crate::decimal_u64")]
    pub fee_lamports: u64,
    #[serde(with = "crate::optional_decimal_u64")]
    pub cu_price_micro_lamports: Option<u64>,
    pub cu_limit: Option<u32>,
    pub success: bool,
    pub received: ReceiveTime,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TapeUsage {
    pub rows: u32,
    pub payload_bytes: u32,
    pub limits: TapeLimits,
    pub population: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteServiceRequest {
    pub model: ModelQuoteRequest,
    pub ttl_s: u32,
    pub economics: Option<EconomicsInputs>,
    pub frozen_model_hash: Option<String>,
}
