//! Mode-specific judge prerequisites. Sim/replay never load ENV; observe never loads signing keys.
use alight_ingest::{Config, HttpProbe, MAINNET_GENESIS};
use alight_types::{RunMode, Source};
use serde_json::{Value, json};
pub async fn doctor(mode: RunMode, network: bool, output: Option<&str>) -> Result<u8, String> {
    if network && matches!(mode, RunMode::Sim | RunMode::Replay) {
        return Err("Sim/replay doctor is offline; remove --network".into());
    }
    let source = match mode {
        RunMode::Sim => Source::Sim,
        RunMode::Replay => Source::Replay,
        _ => Source::Live,
    };
    let mut exit = 0;
    let mut report = json!({"schema_version":1,"mode":mode,"source":source,"read_only":true,"signing_enabled":false,"canaries_sent":0,"provider_requests":0,"status":"READY_OFFLINE","requirements":[],"limitations":[]});
    if matches!(mode, RunMode::Sim | RunMode::Replay) {
        report["requirements"] = json!([
            "No provider key or funded wallet required",
            "Use an isolated database",
            "Replay requires recorded JSONL; Sim uses a recorded seed"
        ]);
    } else {
        let config =
            Config::load_observer().map_err(|_| "Cannot load read-only observer configuration")?;
        let configured = config.get("SOLAMI_RPC_URL").is_some()
            && (config.get("SOLAMI_GRPC_URL").is_some()
                || config.get("SOLAMI_STREAM_URL").is_some())
            && (config.get("SOLAMI_GRPC_TOKEN").is_some()
                || config.get("SOLAMI_STREAM_TOKEN").is_some()
                || config.get("SOLAMI_API_KEY").is_some());
        report["credentials_present"] =
            serde_json::to_value(config.presence()).map_err(|_| "Cannot encode presence")?;
        report["requirements"] = json!([
            "Own read key and dashboard RPC/gRPC endpoints",
            "No canary or SWQoS signing identity for observe",
            "Live sending separately requires a dedicated funded canary payer, authenticated route and existing budget/reserve gates"
        ]);
        report["limitations"] = json!([
            "Configuration presence does not establish provider entitlements or a working stream",
            "This doctor never signs, sends or resumes collection"
        ]);
        report["status"] = json!(if configured {
            "CONFIGURED_READ_ONLY"
        } else {
            "MISSING_READ_CONFIGURATION"
        });
        exit = if configured { 0 } else { 3 };
        if network {
            let probe = HttpProbe::new().map_err(|_| "Cannot construct bounded read-only probe")?;
            let genesis = probe.rpc(&config, "getGenesisHash", json!([])).await;
            report["provider_requests"] = json!(1);
            match genesis {
                Ok(Value::String(value)) if value == MAINNET_GENESIS => {
                    report["status"] = json!("RPC_MAINNET_VERIFIED_STREAM_UNCHECKED");
                    exit = if configured { 0 } else { 3 };
                }
                _ => {
                    report["status"] = json!("RPC_CHECK_FAILED");
                    exit = 1;
                }
            }
        }
    }
    let bytes = serde_json::to_vec_pretty(&report).map_err(|_| "Cannot encode judge doctor")?;
    if let Some(path) = output {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .map_err(|_| "Doctor output must be a new file in an existing directory")?;
        file.write_all(&bytes)
            .map_err(|_| "Cannot write judge doctor")?;
    }
    println!(
        "{}",
        String::from_utf8(bytes).map_err(|_| "Cannot encode judge doctor")?
    );
    Ok(exit)
}
