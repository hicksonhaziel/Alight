//! Source-scoped alert evidence and durable rule state. No credentials/provider payloads.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AlertRule {
    RouteDegradation,
    ObserverDisagreement,
    RegimeChange,
    QuoteDrift,
    Budget,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Alert {
    pub schema_version: u32,
    pub id: String,
    pub source: Source,
    pub rule: AlertRule,
    pub subject: String,
    pub at_utc: String,
    pub summary: String,
    pub details: serde_json::Value,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AlertState {
    pub as_of_utc: Option<String>,
    pub regime_id: Option<String>,
    pub references: BTreeMap<String, CurveSnapshot>,
    pub active: BTreeSet<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AlertSnapshot {
    pub source: Source,
    pub as_of_utc: String,
    pub regime_id: String,
    pub curves: Vec<CurveSnapshot>,
    pub forecasts: Vec<ForecastEntry>,
    pub observer_disagreements: u32,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub reserved_today_lamports: u64,
    #[serde(with = "crate::decimal_u64")]
    #[schemars(with = "crate::DecimalU64")]
    pub daily_cap_lamports: u64,
}
