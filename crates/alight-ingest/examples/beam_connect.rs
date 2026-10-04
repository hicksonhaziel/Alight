//! Bounded Beam connection check. No transaction is built, signed, or submitted.
use alight_ingest::Config;
use serde_json::json;
use std::{process::ExitCode, time::Instant};

#[tokio::main]
async fn main() -> ExitCode {
    let start = Instant::now();
    let result = check().await;
    let verdict = if result.is_ok() { "PASS" } else { "FAIL" };
    let receipt = json!({
        "schema_version":1,"source":"live","purpose":"beam-quic-connect-only",
        "verdict":verdict,"elapsed_ms":start.elapsed().as_millis(),
        "error_category":result.err(),"canaries_signed":0,"canaries_sent":0,
        "landing_proven":false
    });
    let path = ".alight/probes/beam-connect.json";
    if std::fs::create_dir_all(".alight/probes").is_err()
        || std::fs::write(path, format!("{receipt}\n")).is_err()
    {
        eprintln!("Could not save Beam connection evidence");
        return ExitCode::from(2);
    }
    println!("{receipt}");
    ExitCode::from(if verdict == "PASS" { 0 } else { 1 })
}

async fn check() -> Result<(), &'static str> {
    let config = Config::load().map_err(|_| "configuration")?;
    let key = config.get("SOLAMI_SWQOS_KEY").ok_or("missing_key")?;
    let bytes = bs58::decode(key).into_vec().map_err(|_| "key_format")?;
    if bytes.len() != 64 {
        return Err("key_length");
    }
    solami::Keypair::try_from(bytes.as_slice()).map_err(|_| "keypair_mismatch")?;
    let endpoint = config.get("SOLAMI_BEAM_QUIC").ok_or("missing_endpoint")?;
    // Pin the public provider hostname; never put credentials into endpoint strings.
    if !matches!(
        endpoint,
        "beam.solami.dev:11000"
            | "ams.beam.solami.dev:11000"
            | "fra.beam.solami.dev:11000"
            | "nyc.beam.solami.dev:11000"
    ) {
        return Err("unsupported_endpoint");
    }
    let connection = solami::builder()
        .with_beam(key)
        .beam_endpoint(endpoint)
        .build();
    let client = tokio::time::timeout(std::time::Duration::from_secs(15), connection)
        .await
        .map_err(|_| "connection_timeout")?
        .map_err(|_| "connection_or_authentication")?;
    drop(client);
    Ok(())
}
