use alight_store::Store;
use alight_types::*;
use std::{
    collections::BTreeMap,
    io::{BufRead, Write},
    path::Path,
};

fn read<T: serde::de::DeserializeOwned>(path: &str, max: u64) -> Result<T, String> {
    let file = std::fs::File::open(path).map_err(|_| "Cannot read receipt input")?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(file, max + 1), &mut bytes)
        .map_err(|_| "Cannot read receipt input")?;
    if bytes.len() as u64 > max {
        return Err("Receipt input exceeds bounds".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid receipt JSON".into())
}
fn write(path: &str, value: &impl serde::Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| "Cannot encode receipt")?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "Output must be a new file in an existing directory")?;
    if file
        .write_all(&bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        let _ = std::fs::remove_file(path);
        return Err("Cannot write receipt".into());
    }
    Ok(())
}
pub async fn run(args: &[String]) -> Result<u8, String> {
    let command = args
        .get(1)
        .map(String::as_str)
        .ok_or("Usage: alight receipt capture|evaluate|import [options]")?;
    let allowed: &[&str] = match command {
        "capture" => &[
            "--input",
            "--wallet",
            "--recipients",
            "--from",
            "--through",
            "--output",
        ],
        "evaluate" => &["--database", "--input", "--request", "--output"],
        "import" => &["--database", "--input"],
        _ => return Err("Usage: alight receipt capture|evaluate|import [options]".into()),
    };
    let mut options = BTreeMap::new();
    for pair in args[2..].chunks(2) {
        if pair.len() != 2
            || !allowed.contains(&pair[0].as_str())
            || options.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return Err("Unknown/repeated receipt option or missing value".into());
        }
    }
    let required = |k| {
        options
            .get(k)
            .copied()
            .ok_or_else(|| format!("{k} is required"))
    };
    let capture: WalletHistoryCapture = if command == "capture" {
        let recipients: Vec<String> = read(required("--recipients")?, 65536)?;
        let wallet = required("--wallet")?;
        let path = required("--input")?;
        if std::fs::metadata(path)
            .map_err(|_| "Cannot read captured JSONL")?
            .len()
            > 4 * 1024 * 1024
        {
            return Err("Capture exceeds 4 MiB limit".into());
        }
        let file = std::fs::File::open(path).map_err(|_| "Cannot read captured JSONL")?;
        let mut rows = Vec::new();
        for line in std::io::BufReader::new(file).lines() {
            let line = line.map_err(|_| "Cannot read captured JSONL")?;
            let value: serde_json::Value =
                serde_json::from_str(&line).map_err(|_| "Invalid captured JSONL")?;
            let received = ReceiveTime {
                clock_id: "wallet-capture-replay".into(),
                mono_ns: 0,
                wall_utc: value["received_at"]
                    .as_str()
                    .ok_or("Capture timestamp missing")?
                    .into(),
            };
            if let Some(tx) = alight_tape::captured_transaction(&value, Source::Replay, received)
                .map_err(|_| "Invalid captured transaction")?
                && tx.account_keys.first().map(String::as_str) == Some(wallet)
            {
                rows.push(WalletHistoryRow {
                    transaction: tx,
                    chain_time_utc: None,
                    route: None,
                    size_class: None,
                    regime_id: None,
                });
                if rows.len() > 1000 {
                    return Err("Capture exceeds 1,000 transactions".into());
                }
            }
        }
        WalletHistoryCapture {
            schema_version: 1,
            source: Source::Replay,
            wallet: wallet.into(),
            from_utc: required("--from")?.into(),
            through_utc: required("--through")?.into(),
            tip_recipients: recipients,
            rows,
        }
    } else {
        read(required("--input")?, 4 * 1024 * 1024)?
    };
    alight_tape::receipts::validate(&capture).map_err(|e| e.to_string())?;
    if command == "capture" {
        write(required("--output")?, &capture)?;
        return Ok(0);
    }
    let db = Path::new(required("--database")?);
    if command == "import" {
        let default = std::env::current_dir()
            .map_err(|_| "Cannot locate workspace")?
            .join("data/alight.db");
        let absolute = if db.is_absolute() {
            db.to_owned()
        } else {
            std::env::current_dir()
                .map_err(|_| "Cannot locate workspace")?
                .join(db)
        };
        if absolute == default
            || db
                .canonicalize()
                .ok()
                .zip(default.canonicalize().ok())
                .is_some_and(|(a, b)| a == b)
        {
            return Err("Import requires an isolated Sim/replay database".into());
        }
        let store = Store::open(db, 256 * 1024 * 1024)
            .await
            .map_err(|_| "Cannot open isolated capture database")?;
        let hash = store
            .save_wallet_capture(&capture)
            .await
            .map_err(|e| e.to_string())?;
        store.close().await;
        println!(
            "{}",
            serde_json::json!({"source":capture.source,"capture_hash":hash,"transactions":capture.rows.len()})
        );
    } else {
        let request: WalletReceiptRequest = read(required("--request")?, 65536)?;
        let store = Store::open_read_only(db)
            .await
            .map_err(|_| "Cannot open historical evidence read-only")?;
        let report = alight_tape::receipts::evaluate(&store, &capture, &request)
            .await
            .map_err(|e| e.to_string())?;
        store.close().await;
        write(required("--output")?, &report)?;
        println!(
            "{}",
            serde_json::json!({"source":report.source,"transactions":report.transactions,"compared_transactions":report.compared_transactions,"output":required("--output")?})
        );
    }
    Ok(0)
}
