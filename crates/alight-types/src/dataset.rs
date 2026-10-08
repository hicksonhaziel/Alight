//! Static reviewed dataset catalog and manifest. Each download declares its own source.
use crate::Source;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DatasetLink {
    pub id: String,
    pub day: String,
    pub source: Source,
    pub label: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DatasetCatalog {
    pub schema_version: u32,
    pub datasets: Vec<DatasetLink>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DatasetFile {
    pub bytes: u32,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DatasetTable {
    pub rows: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DatasetLedger {
    pub verified: bool,
    pub head: String,
    pub sequence: String,
    pub witness_scope: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DatasetManifest {
    pub schema_version: u32,
    pub kind: String,
    pub source: Source,
    pub day: String,
    pub license: String,
    pub formats: Vec<String>,
    pub tables: BTreeMap<String, DatasetTable>,
    pub regions: Vec<String>,
    pub ledger: DatasetLedger,
    pub coverage: String,
    pub vantage: String,
    pub limits: Vec<String>,
    pub files: BTreeMap<String, DatasetFile>,
}
