use alight_store::Store;
use alight_types::{AnchorDraft, Source};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::Path,
};
pub async fn run(args: &[String]) -> Result<u8, String> {
    if matches!(
        args.get(1).map(String::as_str),
        Some("preflight" | "send" | "run" | "reconcile" | "verify-network")
    ) {
        return network(args).await;
    }
    let command=args.get(1).map(String::as_str).ok_or("Usage: alight anchor prepare|verify --database DB [--source MODE --output FILE | --input FILE]")?;
    let allowed: &[&str] = match command {
        "prepare" => &["--database", "--source", "--output"],
        "verify" => &["--database", "--input"],
        _ => return Err("Only unsigned anchor prepare and offline verify are supported".into()),
    };
    let mut options = BTreeMap::new();
    for pair in args[2..].chunks(2) {
        if pair.len() != 2
            || !allowed.contains(&pair[0].as_str())
            || options.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return Err("Unknown/repeated anchor option or missing value".into());
        }
    }
    let required = |key| {
        options
            .get(key)
            .copied()
            .ok_or_else(|| format!("{key} is required"))
    };
    let store = Store::open_read_only(Path::new(required("--database")?))
        .await
        .map_err(|_| "Cannot open ledger read-only")?;
    if command == "prepare" {
        let source = match required("--source")? {
            "live" => Source::Live,
            "sim" => Source::Sim,
            "replay" => Source::Replay,
            _ => return Err("Invalid anchor source".into()),
        };
        let draft = alight_canary::anchor::prepare(&store, source)
            .await
            .map_err(|_| "Anchor preparation requires a verified nonempty ledger")?;
        let bytes =
            serde_json::to_vec_pretty(&draft).map_err(|_| "Cannot encode unsigned draft")?;
        let path = required("--output")?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| "Anchor output must be a new file")?;
        if file
            .write_all(&bytes)
            .and_then(|()| file.sync_all())
            .is_err()
        {
            let _ = std::fs::remove_file(path);
            return Err("Cannot save anchor draft".into());
        }
        println!(
            "{}",
            serde_json::json!({"source":source,"status":"PREPARED_UNSIGNED","sequence":draft.sequence.to_string(),"transactions_sent":0,"output":path})
        );
    } else {
        let file =
            std::fs::File::open(required("--input")?).map_err(|_| "Cannot read anchor draft")?;
        let mut bytes = Vec::new();
        file.take(4097)
            .read_to_end(&mut bytes)
            .map_err(|_| "Cannot read anchor draft")?;
        if bytes.len() > 4096 {
            return Err("Anchor draft exceeds bounds".into());
        }
        let draft: AnchorDraft =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid unsigned anchor JSON")?;
        alight_canary::anchor::verify(&store, &draft)
            .await
            .map_err(|_| "Unsigned anchor verification failed")?;
        println!(
            "{}",
            serde_json::json!({"source":draft.source,"status":"OFFLINE_VERIFIED_UNSIGNED","sequence":draft.sequence.to_string(),"transactions_sent":0})
        );
    }
    store.close().await;
    Ok(0)
}

async fn network(args: &[String]) -> Result<u8, String> {
    let command = args[1].as_str();
    let mut options = BTreeMap::new();
    for pair in args[2..].chunks(2) {
        if pair.len() != 2
            || ![
                "--database",
                "--source",
                "--max-fee-lamports",
                "--authorize-mainnet",
                "--interval-s",
                "--input",
            ]
            .contains(&pair[0].as_str())
            || options.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return Err("Invalid anchor option".into());
        }
    }
    let sending = matches!(command, "send" | "run");
    if sending && options.get("--authorize-mainnet") != Some(&"true") {
        return Err(
            "Anchor sending requires --authorize-mainnet true; preflight performs read-only checks"
                .into(),
        );
    }
    if !sending
        && (options.contains_key("--authorize-mainnet") || options.contains_key("--interval-s"))
    {
        return Err("Read-only anchor commands do not accept sending controls".into());
    }
    if command != "run" && options.contains_key("--interval-s") {
        return Err("--interval-s requires anchor run".into());
    }
    if command != "verify-network" && options.contains_key("--input") {
        return Err("--input requires verify-network".into());
    }
    let config = if sending {
        alight_ingest::Config::load()
    } else {
        alight_ingest::Config::load_observer()
    }
    .map_err(|_| "Invalid local configuration")?;
    let source = match options.get("--source").copied() {
        Some("live") => Source::Live,
        Some("sim") => Source::Sim,
        Some("replay") => Source::Replay,
        _ => return Err("--source live|sim|replay is required".into()),
    };
    let path = options
        .get("--database")
        .copied()
        .ok_or("--database ledger path is required")?;
    let ledger = Store::open_read_only(Path::new(path))
        .await
        .map_err(|_| "Cannot open ledger read-only")?;
    if command == "verify-network" {
        let input = options
            .get("--input")
            .copied()
            .ok_or("--input public anchor record is required")?;
        let mut bytes = Vec::new();
        std::fs::File::open(input)
            .map_err(|_| "Cannot read anchor record")?
            .take(16385)
            .read_to_end(&mut bytes)
            .map_err(|_| "Cannot read anchor record")?;
        if bytes.len() > 16384 {
            return Err("Anchor record exceeds bounds".into());
        }
        let record: alight_types::AnchorRecord =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid public anchor record")?;
        if record.source != source || record.attempt.commitment.source != source {
            return Err("Anchor source mismatch".into());
        }
        let event = alight_canary::anchor::verify_network(&ledger, &record.attempt, &config)
            .await
            .map_err(|e| e.to_string())?;
        let passed = event.status == "FINALIZED";
        println!(
            "{}",
            serde_json::json!({"source":source,"verification":event,"transactions_sent":0})
        );
        return Ok(if passed { 0 } else { 1 });
    }
    let cap = options
        .get("--max-fee-lamports")
        .copied()
        .unwrap_or("10000")
        .parse::<u64>()
        .map_err(|_| "Invalid fee cap")?;
    if command == "preflight" {
        let p = alight_canary::anchor::preflight(&ledger, source, &config, cap)
            .await
            .map_err(|e| e.to_string())?;
        println!("{}", p.summary());
        return Ok(if p.ready { 0 } else { 3 });
    }
    // Always share the configured collector database with canary routes; no independent anchor budget.
    let budget_path = Path::new(config.get("ALIGHT_DB_PATH").unwrap_or("data/alight.db"));
    if !budget_path.is_file() {
        return Err("The configured collector budget database must already exist".into());
    }
    let max = config
        .get("ALIGHT_DB_MAX_BYTES")
        .unwrap_or("536870912")
        .parse()
        .map_err(|_| "Invalid DB cap")?;
    let budgets = Store::open(budget_path, max)
        .await
        .map_err(|_| "Cannot open shared budget database")?;
    let interval = options
        .get("--interval-s")
        .copied()
        .unwrap_or("3600")
        .parse::<u64>()
        .map_err(|_| "Invalid anchor interval")?;
    if !(300..=86400).contains(&interval) {
        return Err("Anchor interval must be 300–86400 seconds".into());
    }
    loop {
        // Reconcile bounded old signatures before considering any new ledger head. No rebroadcast.
        for record in budgets
            .anchors(source)
            .await
            .map_err(|_| "Cannot read anchor records")?
            .into_iter()
            .filter(|r| matches!(r.event.status.as_str(), "PREPARED" | "ACCEPTED" | "UNKNOWN"))
            .take(8)
        {
            if let Ok(event) =
                alight_canary::anchor::verify_network(&ledger, &record.attempt, &config).await
            {
                budgets
                    .anchor_event(&event)
                    .await
                    .map_err(|_| "Cannot persist anchor verification")?;
                println!(
                    "{}",
                    serde_json::json!({"source":source,"verification":event,"transactions_sent":0})
                );
            }
        }
        if command == "reconcile" {
            let records = budgets
                .anchors(source)
                .await
                .map_err(|_| "Cannot read anchor records")?;
            let verified = records
                .iter()
                .filter(|r| r.event.status == "FINALIZED")
                .count();
            println!(
                "{}",
                serde_json::json!({"source":source,"verified_finalized_anchors":verified,"retained_attempts":records.len(),"transactions_sent":0})
            );
            return Ok(if verified > 0 { 0 } else { 3 });
        }
        match alight_canary::anchor::send(&ledger, &budgets, source, &config, cap, true).await {
            Ok(record) => println!(
                "{}",
                serde_json::to_string(&record).map_err(|_| "Cannot encode anchor record")?
            ),
            Err(error) if command == "run" => eprintln!("Anchor cycle: {error}"),
            Err(error) => return Err(error.to_string()),
        }
        if command != "run" {
            break;
        }
        tokio::select! { _=tokio::signal::ctrl_c()=>break, _=tokio::time::sleep(std::time::Duration::from_secs(interval))=>{} }
    }
    budgets.close().await;
    ledger.close().await;
    Ok(0)
}
