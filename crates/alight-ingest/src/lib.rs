//! Bounded, read-only Phase 0 HTTP probes. No signing or send methods are exposed.
use alight_types::{SlotWindow, Source, Verdict};
use reqwest::{Client, Url, redirect::Policy};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use thiserror::Error;

pub mod adapter;
pub mod clock;
pub mod leaders;
#[cfg(feature = "stream")]
pub mod rpc;
#[cfg(feature = "stream")]
pub mod stream;
pub mod webhook;

pub const MAINNET_GENESIS: &str = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d";
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

/// Sanitized transport failures deliberately contain no URL, token, or upstream text.
#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("missing or invalid configuration")]
    Configuration,
    #[error("network request failed or timed out")]
    Network,
    #[error("HTTP {0}; upstream body withheld")]
    Http(u16),
    #[error("response exceeded byte cap")]
    Size,
    #[error("unexpected response format")]
    Format,
    #[error("RPC rejected request (code {0}); upstream text withheld")]
    Rpc(i64),
    #[error("configured endpoint is not Solana mainnet-beta")]
    WrongCluster,
}

/// Local configuration has no Debug/Serialize implementation to prevent secret logging.
pub struct Config {
    values: BTreeMap<String, String>,
}

impl Config {
    /// Reads the ignored .env; process variables take precedence without mutating the process.
    pub fn load() -> Result<Self, ProbeError> {
        Self::load_filtered(false)
    }

    /// Observer configuration deliberately excludes both signing identities.
    pub fn load_observer() -> Result<Self, ProbeError> {
        Self::load_filtered(true)
    }

    fn load_filtered(observer_only: bool) -> Result<Self, ProbeError> {
        let allowed = |key: &str| {
            !observer_only || !["ALIGHT_CANARY_KEYPAIR", "SOLAMI_SWQOS_KEY"].contains(&key)
        };
        let mut values = BTreeMap::new();
        if std::path::Path::new(".env").exists() {
            for entry in dotenvy::from_path_iter(".env").map_err(|_| ProbeError::Configuration)? {
                let (key, value) = entry.map_err(|_| ProbeError::Configuration)?;
                if allowed(&key) {
                    values.insert(key, value);
                }
            }
        }
        for (key, value) in std::env::vars() {
            if (key.starts_with("SOLAMI_") || key.starts_with("ALIGHT_")) && allowed(&key) {
                values.insert(key, value);
            }
        }
        Ok(Self { values })
    }

    /// Returns a trimmed value; callers must not log returned strings.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values
            .get(key)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
    }

    /// Returns only presence booleans, never environment values.
    pub fn presence(&self) -> BTreeMap<String, bool> {
        [
            "SOLAMI_RPC_URL",
            "SOLAMI_API_KEY",
            "SOLAMI_GRPC_URL",
            "SOLAMI_GRPC_TOKEN",
            "SOLAMI_SWQOS_KEY",
            "SOLAMI_MIRAGE_SUBSCRIPTION_ID",
            "SOLAMI_BLUR_TOKEN",
            "ALIGHT_CANARY_KEYPAIR",
            "ALIGHT_WEBHOOK_CALLBACK_URL",
            "SOLAMI_WEBHOOK_ID",
        ]
        .into_iter()
        .map(|key| (key.to_owned(), self.get(key).is_some()))
        .collect()
    }

    /// Replaces known secret values in provider JSON before a fixture can be written.
    pub fn scrub(&self, value: &mut Value) {
        match value {
            Value::String(text) => {
                for (key, secret) in &self.values {
                    if secret.len() >= 8
                        && (key.contains("TOKEN")
                            || key.contains("KEY")
                            || key.contains("SECRET")
                            || key.contains("URL"))
                    {
                        *text = text.replace(secret, "[REDACTED]");
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| self.scrub(v)),
            Value::Object(map) => {
                for (key, value) in map {
                    if ["secret", "token", "api_key", "authorization"]
                        .contains(&key.to_lowercase().as_str())
                    {
                        *value = Value::String("[REDACTED]".into());
                    } else {
                        self.scrub(value);
                    }
                }
            }
            _ => {}
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: String,
    pub verdict: Verdict,
    pub elapsed_ms: u128,
    pub data: Value,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub source: Source,
    pub read_only: bool,
    pub credentials_present: BTreeMap<String, bool>,
    pub checks: Vec<Check>,
    pub canaries_sent: u32,
}

/// Reusable client restricts redirects and caps request time and received bytes.
pub struct HttpProbe {
    client: Client,
}

impl HttpProbe {
    pub fn new() -> Result<Self, ProbeError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(12))
            .redirect(Policy::none())
            .build()
            .map_err(|_| ProbeError::Configuration)?;
        Ok(Self { client })
    }

    async fn read_json(&self, request: reqwest::RequestBuilder) -> Result<Value, ProbeError> {
        self.read_json_limit(request, MAX_RESPONSE_BYTES).await
    }

    async fn read_json_limit(
        &self,
        request: reqwest::RequestBuilder,
        limit: usize,
    ) -> Result<Value, ProbeError> {
        let mut response = request.send().await.map_err(|_| ProbeError::Network)?;
        if !response.status().is_success() {
            return Err(ProbeError::Http(response.status().as_u16()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| ProbeError::Network)? {
            if bytes.len().saturating_add(chunk.len()) > limit {
                return Err(ProbeError::Size);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| ProbeError::Format)
    }

    /// Read-only RPC allowlist ensures a probe cannot accidentally send a transaction.
    pub async fn rpc(
        &self,
        config: &Config,
        method: &str,
        params: Value,
    ) -> Result<Value, ProbeError> {
        if ![
            "getGenesisHash",
            "getSlot",
            "getVersion",
            "getLatestBlockhash",
            "getBlockHeight",
            "getEpochInfo",
            "getLeaderSchedule",
            "getBlockTime",
            "getFirstAvailableBlock",
            "getBlocks",
            "getBlock",
            "getBalance",
            "getSignatureStatuses",
            "getTransaction",
            "getVoteAccounts",
            "getBlockProduction",
            "getRecentPrioritizationFees",
            "getFeeForMessage",
            "simulateTransaction",
        ]
        .contains(&method)
        {
            return Err(ProbeError::Configuration);
        }
        let endpoint = https_url(
            config
                .get("SOLAMI_RPC_URL")
                .ok_or(ProbeError::Configuration)?,
        )?;
        let mut request = self
            .client
            .post(endpoint)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}));
        if let Some(token) = config.get("SOLAMI_RPC_TOKEN") {
            request = request.header("x-api-key", token);
        }
        // Complete epoch schedules exceed the normal proof-response cap.
        let limit = if method == "getLeaderSchedule" || method == "getBlock" {
            // A complete epoch is roughly 3 MiB; retain a bounded large-response deadline.
            request = request.timeout(Duration::from_secs(45));
            8 * 1024 * 1024
        } else {
            MAX_RESPONSE_BYTES
        };
        let body = self.read_json_limit(request, limit).await?;
        rpc_result(body)
    }

    /// Public metadata or API-key-authenticated GET; all URLs remain private.
    pub async fn get(&self, url: &str, token: Option<&str>) -> Result<Value, ProbeError> {
        let mut request = self.client.get(https_url(url)?);
        if let Some(token) = token {
            request = request.header("x-api-key", token);
        }
        self.read_json(request).await
    }

    async fn rpc_check(
        &self,
        config: &Config,
        name: &str,
        method: &str,
        params: Value,
        report: &mut Report,
    ) -> Option<Value> {
        let start = Instant::now();
        let result = self.rpc(config, method, params).await;
        record(name, start, result, config, report)
    }

    /// Executes at most 30 sequential, read-only requests; each response is byte/time capped.
    pub async fn doctor(&self, config: &Config) -> Report {
        let mut report = Report {
            schema_version: 1,
            source: Source::Live,
            read_only: true,
            credentials_present: config.presence(),
            checks: vec![],
            canaries_sent: 0,
        };
        let genesis = self
            .rpc_check(
                config,
                "rpc_genesis",
                "getGenesisHash",
                json!([]),
                &mut report,
            )
            .await;
        if genesis.as_ref().and_then(Value::as_str) != Some(MAINNET_GENESIS) {
            report.checks.push(Check {
                name: "mainnet_guard".into(),
                verdict: Verdict::Fail,
                elapsed_ms: 0,
                data: json!({"error": ProbeError::WrongCluster.to_string()}),
            });
            return report;
        }
        self.rpc_check(config, "rpc_version", "getVersion", json!([]), &mut report)
            .await;
        let slot = self
            .rpc_check(
                config,
                "finalized_slot",
                "getSlot",
                json!([{"commitment":"finalized"}]),
                &mut report,
            )
            .await
            .and_then(|v| v.as_u64());
        self.rpc_check(
            config,
            "latest_blockhash",
            "getLatestBlockhash",
            json!([{"commitment":"confirmed"}]),
            &mut report,
        )
        .await;
        self.rpc_check(
            config,
            "block_height",
            "getBlockHeight",
            json!([{"commitment":"confirmed"}]),
            &mut report,
        )
        .await;
        self.rpc_check(
            config,
            "epoch_info",
            "getEpochInfo",
            json!([{"commitment":"finalized"}]),
            &mut report,
        )
        .await;
        self.rpc_check(
            config,
            "retention_floor",
            "getFirstAvailableBlock",
            json!([]),
            &mut report,
        )
        .await;
        self.rpc_check(
            config,
            "signature_status_shape",
            "getSignatureStatuses",
            json!([[],{"searchTransactionHistory":true}]),
            &mut report,
        )
        .await;
        if let Some(end_slot) = slot {
            let end_time = self
                .rpc_check(
                    config,
                    "clock_end",
                    "getBlockTime",
                    json!([end_slot]),
                    &mut report,
                )
                .await
                .and_then(|v| v.as_i64());
            for distance in [64_u64, 256, 1024] {
                if let (Some(start_slot), Some(end_unix_s)) =
                    (end_slot.checked_sub(distance), end_time)
                {
                    let start = Instant::now();
                    let result = self.rpc(config, "getBlockTime", json!([start_slot])).await.and_then(|v| {
                        let start_unix_s = v.as_i64().ok_or(ProbeError::Format)?;
                        let mean = SlotWindow { start_slot, end_slot, start_unix_s, end_unix_s }
                            .mean_slot_ms().ok_or(ProbeError::Format)?;
                        Ok(json!({"start_slot":start_slot.to_string(),"end_slot":end_slot.to_string(),
                            "start_unix_s":start_unix_s,"end_unix_s":end_unix_s,"mean_slot_ms":mean,
                            "method":"block_timestamp_delta_over_slot_delta","precision":"coarse_window_mean"}))
                    });
                    record(
                        &format!("slot_time_{distance}"),
                        start,
                        result,
                        config,
                        &mut report,
                    );
                }
            }
            let leaders = self
                .rpc_check(
                    config,
                    "leader_schedule",
                    "getLeaderSchedule",
                    json!([end_slot,{"commitment":"finalized"}]),
                    &mut report,
                )
                .await;
            if let Some(map) = leaders.and_then(|v| v.as_object().cloned())
                && let Some(check) = report.checks.last_mut()
            {
                check.data = json!({"leader_count":map.len(),"result":"retrieved"});
            }
            self.rpc_check(
                config,
                "block_identity",
                "getBlock",
                json!([end_slot,{"commitment":"finalized",
                "transactionDetails":"none","rewards":false,"maxSupportedTransactionVersion":1}]),
                &mut report,
            )
            .await;
            // Historical positions are probes, not assumed dates or protocol activation claims.
            for distance in [1_000_000_u64, 4_000_000, 16_000_000] {
                if let Some(old_slot) = end_slot.checked_sub(distance) {
                    self.rpc_check(
                        config,
                        &format!("historical_block_time_{distance}"),
                        "getBlockTime",
                        json!([old_slot]),
                        &mut report,
                    )
                    .await;
                }
            }
        }
        if let Some(key) = config.get("ALIGHT_CANARY_KEYPAIR") {
            match bs58::decode(key)
                .into_vec()
                .ok()
                .filter(|bytes| bytes.len() == 64)
            {
                Some(bytes) => {
                    let address = bs58::encode(&bytes[32..]).into_string();
                    self.rpc_check(
                        config,
                        "canary_balance",
                        "getBalance",
                        json!([address,{"commitment":"confirmed"}]),
                        &mut report,
                    )
                    .await;
                }
                None => report.checks.push(Check {
                    name: "canary_balance".into(),
                    verdict: Verdict::Fail,
                    elapsed_ms: 0,
                    data: json!({"error":"invalid keypair shape; key withheld"}),
                }),
            }
        }
        let base = config
            .get("SOLAMI_DATA_API_URL")
            .unwrap_or("https://api.solami.dev")
            .trim_end_matches('/');
        for (name, path, token) in [
            ("tip_addresses", "/onchain/tip-addresses", None),
            ("public_pricing", "/pricing", None),
            (
                "blur_pools",
                "/data/pools?chain=solana&limit=3&sort=liquidity_usd&order=desc",
                config.get("SOLAMI_BLUR_TOKEN"),
            ),
            (
                "blur_trades",
                "/data/token/trades?chain=solana&address=So11111111111111111111111111111111111111112&limit=3",
                config.get("SOLAMI_BLUR_TOKEN"),
            ),
            (
                "blur_candles",
                "/data/token/ohlcv?chain=solana&address=So11111111111111111111111111111111111111112&interval=1m&count=3",
                config.get("SOLAMI_BLUR_TOKEN"),
            ),
        ] {
            let start = Instant::now();
            let result = if name.starts_with("blur_") && token.is_none() {
                Err(ProbeError::Configuration)
            } else {
                let selected_base = if name.starts_with("blur_") {
                    config
                        .get("SOLAMI_BLUR_URL")
                        .unwrap_or(base)
                        .trim_end_matches('/')
                } else {
                    base
                };
                self.get(&format!("{selected_base}{path}"), token).await
            };
            record(name, start, result, config, &mut report);
        }
        report
    }
}

fn record(
    name: &str,
    start: Instant,
    result: Result<Value, ProbeError>,
    config: &Config,
    report: &mut Report,
) -> Option<Value> {
    let (verdict, mut data, value) = match result {
        Ok(value) if value.is_null() => (Verdict::Inconclusive, Value::Null, None),
        Ok(value) => (Verdict::Pass, value.clone(), Some(value)),
        Err(error) => (Verdict::Fail, json!({"error":error.to_string()}), None),
    };
    config.scrub(&mut data);
    report.checks.push(Check {
        name: name.into(),
        verdict,
        elapsed_ms: start.elapsed().as_millis(),
        data,
    });
    value
}

fn https_url(value: &str) -> Result<Url, ProbeError> {
    let url = Url::parse(value).map_err(|_| ProbeError::Configuration)?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(ProbeError::Configuration);
    }
    Ok(url)
}

fn rpc_result(body: Value) -> Result<Value, ProbeError> {
    if let Some(error) = body.get("error") {
        return Err(ProbeError::Rpc(
            error.get("code").and_then(Value::as_i64).unwrap_or(0),
        ));
    }
    body.get("result").cloned().ok_or(ProbeError::Format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_failures_do_not_include_echoed_secrets() {
        let error = rpc_result(json!({"error":{"code":-32000,"message":"secret-token-123"}}));
        assert!(matches!(error, Err(ProbeError::Rpc(-32000))));
        assert!(!format!("{error:?}").contains("secret-token-123"));
    }

    #[test]
    fn recursively_scrubs_known_secrets_and_sensitive_fields() {
        let config = Config {
            values: BTreeMap::from([("SOLAMI_API_KEY".into(), "secret-token-123".into())]),
        };
        let mut value = json!({"nested":[{"url":"https://example.invalid?api_key=secret-token-123","secret":"unknown secret"}]});
        config.scrub(&mut value);
        let text = value.to_string();
        assert!(!text.contains("secret-token-123"));
        assert!(!text.contains("unknown secret"));
        assert!(text.contains("[REDACTED]"));
    }

    #[test]
    fn null_rpc_result_is_preserved_as_missing_evidence() {
        assert!(matches!(
            rpc_result(json!({"result":null})),
            Ok(Value::Null)
        ));
        assert!(matches!(
            rpc_result(json!({"wrong":[]})),
            Err(ProbeError::Format)
        ));
        assert!(https_url("http://example.invalid").is_err());
    }
}
