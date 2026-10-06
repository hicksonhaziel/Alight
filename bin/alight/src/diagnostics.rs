use alight_diagnostics::{
    backfill::{History, plan, reconstruct},
    simulation::scenario,
};
use alight_ingest::{Config, HttpProbe, MAINNET_GENESIS};
use alight_store::Store;
use alight_types::*;
use serde_json::json;
use std::{collections::BTreeMap, path::Path};
/// Offline by default. --network is an explicit, finite read-only history job, not a collector.
pub async fn run(args: &[String]) -> Result<u8, String> {
    let command = args.first().ok_or("Missing diagnostic command")?.as_str();
    let mut options = BTreeMap::new();
    let mut i = 1;
    while i < args.len() {
        let k = &args[i];
        if ![
            "--database",
            "--seed",
            "--start",
            "--input",
            "--output",
            "--days",
            "--requests",
            "--network",
        ]
        .contains(&k.as_str())
            || options.contains_key(k)
        {
            return Err("Unknown or repeated diagnostic option".into());
        }
        if k == "--network" {
            options.insert(k.clone(), "true".into());
            i += 1;
        } else {
            options.insert(
                k.clone(),
                args.get(i + 1).ok_or("Option needs a value")?.clone(),
            );
            i += 2;
        }
    }
    let db = options.get("--database").ok_or("--database is required")?;
    let store = Store::open(Path::new(db), 512 * 1024 * 1024)
        .await
        .map_err(|_| "Cannot open diagnostic database")?;
    let value = if command == "regime-sim" {
        if options
            .keys()
            .any(|k| ["--network", "--input", "--days", "--requests"].contains(&k.as_str()))
        {
            return Err("regime-sim is offline only".into());
        }
        let seed = options
            .get("--seed")
            .map_or(Ok(42), |v| v.parse::<u64>())
            .map_err(|_| "Invalid seed")?;
        let start = options
            .get("--start")
            .map_or("2026-10-05T00:00:00Z", String::as_str);
        let report = scenario(&store, seed, start)
            .await
            .map_err(|_| "Regime simulation failed")?;
        alight_diagnostics::refresh(
            &store,
            Source::Sim,
            &(alight_diagnostics::utc(start).map_err(|_| "Invalid start timestamp")?
                + chrono::Duration::hours(2))
            .to_rfc3339(),
            &[],
        )
        .await
        .map_err(|_| "Snapshot failed")?;
        serde_json::to_value(report).map_err(|_| "Cannot encode simulation")?
    } else if command == "backfill" {
        let history = if let Some(input) = options.get("--input") {
            if options.contains_key("--network") {
                return Err("Choose recorded input or explicit network backfill".into());
            }
            if std::fs::metadata(input)
                .map_err(|_| "Cannot read history")?
                .len()
                > 4 * 1024 * 1024
            {
                return Err("History exceeds 4 MiB".into());
            }
            serde_json::from_slice::<History>(
                &std::fs::read(input).map_err(|_| "Cannot read history")?,
            )
            .map_err(|_| "Invalid recorded history")?
        } else {
            if !options.contains_key("--network") {
                return Err("backfill needs --input FILE or --network".into());
            }
            let days = options
                .get("--days")
                .map_or(Ok(56), |s| s.parse::<u32>())
                .map_err(|_| "Invalid days")?;
            let budget = options
                .get("--requests")
                .map_or(Ok(512), |s| s.parse::<u32>())
                .map_err(|_| "Invalid request limit")?;
            if !(64..=1024).contains(&budget) {
                return Err("Request limit is 64–1024, including metadata".into());
            }
            let config = Config::load_observer().map_err(|_| "Invalid observer configuration")?;
            let probe = HttpProbe::new().map_err(|_| "Cannot create read-only RPC client")?;
            if probe
                .rpc(&config, "getGenesisHash", json!([]))
                .await
                .map_err(|_| "Mainnet check unavailable")?
                .as_str()
                != Some(MAINNET_GENESIS)
            {
                return Err("Mainnet check failed".into());
            }
            let first = probe
                .rpc(&config, "getFirstAvailableBlock", json!([]))
                .await
                .map_err(|_| "Retention check unavailable")?
                .as_u64()
                .ok_or("Invalid retention response")?;
            let tip = probe
                .rpc(&config, "getSlot", json!([{"commitment":"finalized"}]))
                .await
                .map_err(|_| "Finalized slot unavailable")?
                .as_u64()
                .ok_or("Invalid slot response")?;
            let slots =
                plan(first, tip, days, budget - 3).map_err(|_| "Invalid backfill bounds")?;
            let mut history = History {
                requested_days: days,
                as_of_utc: chrono::Utc::now().to_rfc3339(),
                first_available_slot: first,
                requests: 3,
                blocks: vec![],
            };
            for slot in slots {
                history.requests += 1;
                let body=probe.rpc(&config,"getBlock",json!([slot,{"commitment":"finalized","transactionDetails":"none","rewards":false,"maxSupportedTransactionVersion":1}])).await;
                if let Ok(body) = body
                    && let Some(id) = body["blockhash"]
                        .as_str()
                        .filter(|s| !s.is_empty() && s.len() <= 128)
                {
                    let raw = json!({"slot":slot.to_string(),"blockhash":id,"blockTime":body["blockTime"],"parentSlot":body["parentSlot"]});
                    history.blocks.push(BlockMetaEvent {
                        source: Source::Replay,
                        slot,
                        block_id: id.into(),
                        parent_slot: body["parentSlot"].as_u64(),
                        parent_block_id: body["previousBlockhash"].as_str().map(str::to_owned),
                        block_time_unix_s: body["blockTime"].as_i64(),
                        block_height: body["blockHeight"].as_u64(),
                        executed_transactions: None,
                        received: ReceiveTime {
                            clock_id: "historical-chain-rpc-no-monotonic-comparison".into(),
                            mono_ns: 0,
                            wall_utc: history.as_of_utc.clone(),
                        },
                        raw_ref: alight_store::raw_ref(&raw).map_err(|_| "Cannot hash history")?,
                    });
                }
            }
            history
        };
        let report = reconstruct(&store, &history)
            .await
            .map_err(|_| "History reconstruction failed")?;
        alight_diagnostics::refresh(&store, Source::Replay, &history.as_of_utc, &[])
            .await
            .map_err(|_| "Snapshot failed")?;
        serde_json::to_value(report).map_err(|_| "Cannot encode backfill")?
    } else {
        return Err("Unknown diagnostic command".into());
    };
    let bytes = serde_json::to_vec_pretty(&value).map_err(|_| "Cannot encode report")?;
    if let Some(path) = options.get("--output") {
        std::fs::write(path, &bytes).map_err(|_| "Cannot write report")?;
    }
    println!(
        "{}",
        String::from_utf8(bytes).map_err(|_| "Cannot encode report")?
    );
    store.close().await;
    Ok(0)
}
