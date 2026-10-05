//! Offline model/ledger commands. No environment loading or provider access.
use alight_forecast::{issue, signal_report, tick};
use alight_store::Store;
use alight_types::{ModelQuoteRequest, Source, TipTapeObservation};
use serde::de::DeserializeOwned;
use std::{collections::BTreeMap, path::Path};

fn read<T: DeserializeOwned>(path: &str) -> Result<T, String> {
    if std::fs::metadata(path)
        .map_err(|_| "Cannot read input")?
        .len()
        > 4 * 1024 * 1024
    {
        return Err("Input exceeds 4 MiB".into());
    }
    serde_json::from_slice(&std::fs::read(path).map_err(|_| "Cannot read input")?)
        .map_err(|_| "Invalid JSON input".into())
}
pub async fn run(args: &[String]) -> Result<u8, String> {
    let ledger = args[0] == "ledger";
    if ledger && args.get(1).map(String::as_str) != Some("verify") {
        return Err("Usage: alight ledger verify --database PATH --source live|sim|replay".into());
    }
    let mut options = BTreeMap::new();
    let allowed = [
        "--database",
        "--source",
        "--region",
        "--as-of",
        "--day",
        "--request",
        "--ttl",
        "--frozen-model",
        "--tape",
    ];
    let mut i = if ledger { 2 } else { 1 };
    while i < args.len() {
        let key = args[i].as_str();
        if !allowed.contains(&key) || options.contains_key(key) {
            return Err("Unknown or repeated model option".into());
        }
        options.insert(
            key,
            args.get(i + 1).ok_or("Option requires a value")?.as_str(),
        );
        i += 2;
    }
    let required = |key| {
        options
            .get(key)
            .copied()
            .ok_or_else(|| format!("{key} is required"))
    };
    let database = required("--database")?;
    let request: Option<ModelQuoteRequest> = if args[0] == "quote" {
        Some(read(required("--request")?)?)
    } else {
        None
    };
    let source = match options.get("--source").copied() {
        Some("live") => Source::Live,
        Some("sim") => Source::Sim,
        Some("replay") => Source::Replay,
        None if request.is_some() => request.as_ref().ok_or("Missing request")?.context.source,
        _ => return Err("--source must be live, sim or replay".into()),
    };
    if request.as_ref().is_some_and(|r| r.context.source != source) {
        return Err("Source differs from quote request".into());
    }
    let store = Store::open(Path::new(database), 512 * 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    let result = match args[0].as_str() {
        "ledger" => {
            serde_json::json!({"source":source,"verified_entries":store.verify_ledger(source).await.map_err(|e| e.to_string())?})
        }
        "quote" => {
            let ttl = options
                .get("--ttl")
                .copied()
                .unwrap_or("600")
                .parse()
                .map_err(|_| "Invalid TTL")?;
            let tape: Vec<TipTapeObservation> = options
                .get("--tape")
                .map(|p| read(p))
                .transpose()?
                .unwrap_or_default();
            serde_json::to_value(
                issue(
                    &store,
                    request.ok_or("Missing request")?,
                    ttl,
                    options.get("--frozen-model").copied(),
                    &tape,
                )
                .await
                .map_err(|e| e.to_string())?,
            )
            .map_err(|_| "Cannot encode forecast")?
        }
        "model-tick" | "grade" => {
            let result = tick(
                &store,
                source,
                options.get("--region").copied().unwrap_or("local"),
                required("--as-of")?,
            )
            .await
            .map_err(|e| e.to_string())?;
            serde_json::json!({"tick":result,"grades":store.grades(source).await.map_err(|e| e.to_string())?})
        }
        "signal" => {
            let report = signal_report(&store, source, required("--day")?, required("--as-of")?)
                .await
                .map_err(|e| e.to_string())?;
            let hash = store
                .save_signal(&report)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::json!({"hash":hash,"report":report})
        }
        _ => return Err("Unknown model command".into()),
    };
    store.close().await;
    println!(
        "{}",
        serde_json::to_string_pretty(&result).map_err(|_| "Cannot encode model result")?
    );
    Ok(0)
}
