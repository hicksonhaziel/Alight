//! Explicit remote commands. API keys come from a file and are never printed or placed in URLs.
use alight_client::{Client, ClientError};
use alight_types::*;
use serde::de::DeserializeOwned;
use std::{collections::BTreeMap, io::Write, path::Path};

fn read<T: DeserializeOwned>(path: &str) -> Result<T, String> {
    let bytes = read_bytes(path, 65536)?;
    serde_json::from_slice(&bytes).map_err(|_| "Invalid request JSON".into())
}
fn read_bytes(path: &str, limit: u64) -> Result<Vec<u8>, String> {
    if std::fs::metadata(path)
        .map_err(|_| "Cannot read input file")?
        .len()
        > limit
    {
        return Err("Input file exceeds size limit".into());
    }
    let bytes = std::fs::read(path).map_err(|_| "Cannot read input file")?;
    if bytes.len() as u64 > limit {
        return Err("Input file exceeds size limit".into());
    }
    Ok(bytes)
}
fn error(e: ClientError) -> String {
    e.to_string()
}
fn print(value: impl serde::Serialize) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|_| "Cannot encode result")?
    );
    Ok(())
}
fn options(
    args: &[String],
    start: usize,
    allowed: &[&str],
) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    let mut i = start;
    while i < args.len() {
        let key = &args[i];
        if !allowed.contains(&key.as_str()) || result.contains_key(key) {
            return Err("Unknown or repeated API option".into());
        }
        if key == "--freeze" {
            result.insert(key.clone(), "true".into());
            i += 1;
            continue;
        }
        let value = args.get(i + 1).ok_or("Option requires a value")?;
        result.insert(key.clone(), value.clone());
        i += 2;
    }
    Ok(result)
}
pub async fn run(args: &[String]) -> Result<u8, String> {
    let command = args.first().ok_or("Missing command")?.as_str();
    let start = if command == "ledger" {
        if args.get(1).map(String::as_str) != Some("verify") {
            return Err("Usage: alight ledger verify --api URL --source MODE".into());
        }
        2
    } else {
        1
    };
    let allowed: &[&str] = match command {
        "quote" => &[
            "--api",
            "--source",
            "--request",
            "--freeze",
            "--operator-key-file",
        ],
        "prove" => &[
            "--api",
            "--source",
            "--request",
            "--id",
            "--operator-key-file",
        ],
        "export" => &["--api", "--source", "--day", "--output"],
        "doctor" | "ledger" | "diagnostics" => &["--api", "--source"],
        _ => return Err("Unknown API command".into()),
    };
    let o = options(args, start, allowed)?;
    let required = |k: &str| {
        o.get(k)
            .map(String::as_str)
            .ok_or_else(|| format!("{k} is required"))
    };
    let source = match required("--source")? {
        "live" => Source::Live,
        "sim" => Source::Sim,
        "replay" => Source::Replay,
        _ => return Err("--source must be live, sim or replay".into()),
    };
    let key = o
        .get("--operator-key-file")
        .map(|p| {
            read_bytes(p, 258)
                .and_then(|b| String::from_utf8(b).map_err(|_| "Invalid operator key file".into()))
        })
        .transpose()?;
    let client =
        Client::new(required("--api")?, source, key.as_deref().map(str::trim)).map_err(error)?;
    match command {
        "diagnostics" => {
            print(client.diagnostics().await.map_err(error)?)?;
            Ok(0)
        }
        "doctor" => {
            let h = client.health().await.map_err(error)?;
            let status = h.status.clone();
            print(h)?;
            Ok(match status.as_str() {
                "PASS" | "SIMULATED" | "REPLAY" => 0,
                "FAIL" => 1,
                _ => 3,
            })
        }
        "quote" => {
            let r: QuoteServiceRequest = read(required("--request")?)?;
            if o.contains_key("--freeze") {
                let f = client.freeze_quote(&r).await.map_err(error)?;
                let supported = f.forecast.quote.recommendation.is_some();
                print(f)?;
                Ok(if supported { 0 } else { 3 })
            } else {
                if key.is_some() {
                    return Err("--operator-key-file requires --freeze for quote".into());
                }
                let q = client.quote(&r).await.map_err(error)?;
                let supported = q.quote.recommendation.is_some();
                print(q)?;
                Ok(if supported { 0 } else { 3 })
            }
        }
        "prove" => {
            let r = match (o.get("--request"), o.get("--id")) {
                (Some(path), None) => client
                    .prove(&read::<ProveRequest>(path)?)
                    .await
                    .map_err(error)?,
                (None, Some(id)) => {
                    if key.is_some() {
                        return Err("Reading --id needs no operator key".into());
                    }
                    client.prove_report(id).await.map_err(error)?
                }
                _ => return Err("prove requires exactly one of --request or --id".into()),
            };
            let exit = match r.verdict {
                ProveVerdict::Consistent => 0,
                ProveVerdict::Inconsistent => 1,
                ProveVerdict::Inconclusive => 3,
            };
            print(r)?;
            Ok(exit)
        }
        "ledger" => match client.verify_ledger().await {
            Ok(v) => {
                print(v)?;
                Ok(0)
            }
            Err(ClientError::Api { status: 409, .. }) => {
                eprintln!("Ledger verification failed");
                Ok(1)
            }
            Err(e) => Err(error(e)),
        },
        "export" => {
            export(&client, required("--day")?, required("--output")?).await?;
            Ok(0)
        }
        _ => Err("Unknown API command".into()),
    }
}
async fn export(client: &Client, day: &str, output: &str) -> Result<(), String> {
    let date = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map_err(|_| "--day must be YYYY-MM-DD")?;
    if date.format("%Y-%m-%d").to_string() != day {
        return Err("--day must be YYYY-MM-DD".into());
    }
    let verified = client.verify_ledger().await.map_err(error)?;
    if verified.entries > 10000 {
        return Err("Export exceeds 10,000-row CLI limit".into());
    }
    let mut after = 0;
    let mut entries = Vec::new();
    let mut through = 0;
    while after < verified.entries {
        let page = client.ledger(after, 100).await.map_err(error)?;
        if page.entries.is_empty() {
            return Err("Ledger pagination ended before the verified head".into());
        }
        for row in page.entries {
            if row.sequence > verified.entries {
                break;
            }
            if row.sequence != after + 1 {
                return Err("Non-contiguous source ledger".into());
            }
            after = row.sequence;
            through = after;
            let created = chrono::DateTime::parse_from_rfc3339(&row.forecast.created_at_utc)
                .map_err(|_| "Invalid forecast timestamp")?;
            if created.with_timezone(&chrono::Utc).date_naive() == date {
                entries.push(row);
            }
        }
    }
    let count = entries.len();
    let bundle = ForecastLedgerExport {
        schema_version: 1,
        source: client.source(),
        day: day.into(),
        kind: "forecast_ledger_export".into(),
        verified_through_sequence: through,
        entries,
    };
    let data = serde_json::to_vec_pretty(&bundle).map_err(|_| "Cannot encode export")?;
    if data.len() > 64 * 1024 * 1024 {
        return Err("Export exceeds 64 MiB CLI limit".into());
    }
    let path = Path::new(output);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|_| "Cannot create export directory")?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "Cannot create export; output must not already exist")?;
    if file
        .write_all(&data)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        let _ = std::fs::remove_file(path);
        return Err("Cannot save export".into());
    }
    print(
        serde_json::json!({"source":client.source(),"day":day,"entries":count,"verified_through_sequence":through.to_string(),"output":output}),
    )
}
/// Forward the daemon's explicit mode/options, inheriting its signals, output and configuration.
pub fn daemon(args: &[String]) -> Result<u8, String> {
    if args.len() < 3 || !args[1..].chunks(2).all(|c| c.len() == 2) {
        return Err("Usage: alight run --mode sim|observe|live|replay [alightd options]".into());
    }
    let allowed = [
        "--mode",
        "--db",
        "--bind",
        "--replay",
        "--run-for",
        "--operator-key-file",
        "--seed",
        "--sim-canaries",
        "--sim-regimes",
        "--force-disconnect-after",
    ];
    let mut seen = std::collections::BTreeSet::new();
    for pair in args[1..].chunks(2) {
        if !allowed.contains(&pair[0].as_str()) || !seen.insert(pair[0].as_str()) {
            return Err("Unknown or repeated daemon option".into());
        }
    }
    if !seen.contains("--mode") {
        return Err("run requires explicit --mode".into());
    }
    let current = std::env::current_exe().map_err(|_| "Cannot locate CLI executable")?;
    let sibling = current.with_file_name("alightd");
    let executable = if sibling.is_file() {
        sibling
    } else {
        Path::new("alightd").to_owned()
    };
    let status = std::process::Command::new(executable)
        .args(&args[1..])
        .status()
        .map_err(|_| "Cannot start alightd; build or install both binaries")?;
    Ok(if status.success() { 0 } else { 2 })
}
