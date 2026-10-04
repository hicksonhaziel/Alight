//! Persistent observe/live collector. Signing is confined to the governed canary engine.
use alight_ingest::{
    Config, HttpProbe, MAINNET_GENESIS, adapter,
    clock::SlotClock,
    leaders::LeaderSchedule,
    stream::{self, Frame},
};
use alight_store::{Store, StoreError};
use alight_types::*;
use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::sync::{RwLock, mpsc, watch};

#[derive(Debug, Error)]
enum Error {
    #[error("invalid daemon arguments or configuration")]
    Configuration,
    #[error("another collector holds this database lock")]
    Locked,
    #[error("local filesystem or listener operation failed")]
    Io(#[from] std::io::Error),
    #[error("observer startup failed: {0}")]
    Observer(alight_ingest::ProbeError),
    #[error("observer configuration failed: {0}")]
    ObserverSetup(&'static str),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("background task failed")]
    Task,
    #[error("invalid replay fixture")]
    Replay,
}

#[derive(Default)]
struct Args {
    db: Option<PathBuf>,
    bind: Option<SocketAddr>,
    replay: Option<PathBuf>,
    mode: Option<String>,
    run_for: Option<u64>,
    disconnect: Option<u64>,
}
impl Args {
    fn parse() -> Result<Self, Error> {
        let mut result = Self::default();
        let mut args = std::env::args().skip(1);
        while let Some(key) = args.next() {
            let value = args.next().ok_or(Error::Configuration)?;
            match key.as_str() {
                "--db" => result.db = Some(value.into()),
                "--bind" => result.bind = Some(value.parse().map_err(|_| Error::Configuration)?),
                "--mode" if ["observe", "live", "replay"].contains(&value.as_str()) => {
                    result.mode = Some(value)
                }
                "--replay" => result.replay = Some(value.into()),
                "--run-for" => {
                    result.run_for = Some(value.parse().map_err(|_| Error::Configuration)?)
                }
                "--force-disconnect-after" => {
                    result.disconnect = Some(value.parse().map_err(|_| Error::Configuration)?)
                }
                _ => return Err(Error::Configuration),
            }
        }
        if result.run_for == Some(0) || result.disconnect == Some(0) {
            return Err(Error::Configuration);
        }
        Ok(result)
    }
}

fn lock(path: &Path) -> Result<File, Error> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path.with_extension("db.lock"))?;
    file.try_lock().map_err(|_| Error::Locked)?;
    Ok(file)
}

#[derive(Clone)]
struct Health {
    store: Store,
    clock: Arc<RwLock<SlotClock>>,
    started: Instant,
    run_id: String,
    mirage: bool,
    mode: RunMode,
    engine: Arc<RwLock<Value>>,
    leaders: Arc<RwLock<Value>>,
    limiter: Arc<Mutex<(Instant, u32)>>,
}
impl Health {
    // Shared fixed-window limit: 10 requests/s across both read-only endpoints.
    fn allow(&self) -> Result<(), StatusCode> {
        let mut bucket = self
            .limiter
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if bucket.0.elapsed() >= Duration::from_secs(1) {
            *bucket = (Instant::now(), 10);
        }
        if bucket.1 == 0 {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        bucket.1 -= 1;
        Ok(())
    }
}

async fn health(State(state): State<Health>) -> Result<Json<Value>, StatusCode> {
    state.allow()?;
    let counts = state
        .store
        .counts(Source::Live)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let mut observers = serde_json::Map::new();
    let mut ready = true;
    for observer in [ObserverKind::Grpc, ObserverKind::Mirage] {
        if observer == ObserverKind::Mirage && !state.mirage {
            continue;
        }
        let mut value = state
            .store
            .observer_health(observer, Source::Live)
            .await
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let age = value["last_receive_utc"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|t| {
                (chrono::Utc::now() - t.with_timezone(&chrono::Utc))
                    .num_milliseconds()
                    .max(0)
            });
        let fresh = age.is_some_and(|ms| ms < 30_000) && value["open_gaps"].as_i64() == Some(0);
        value["status"] = json!(if fresh { "PASS" } else { "STALE" });
        value["age_ms"] = json!(age.map(|n| n.to_string()));
        ready &= fresh;
        observers.insert(
            alight_store::label(observer).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?,
            value,
        );
    }
    let sends = state
        .store
        .send_summary(Source::Live)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let sent = sends["ACCEPTED"].as_u64().unwrap_or(0);
    let budget = state
        .store
        .budget_by_route(
            Source::Live,
            &chrono::Utc::now().format("%Y-%m-%d").to_string(),
        )
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(Json(
        json!({"schema_version":1,"mode":state.mode,"source":"live","run_id":state.run_id,
        "status":if ready{"PASS"}else{"DEGRADED"},"collector_status":if ready{"PASS"}else{"DEGRADED"},"uptime_s":state.started.elapsed().as_secs().to_string(),
        "signing_enabled":state.mode==RunMode::Live,"canaries_sent":sent,"send_attempts":sends,"budget_reserved_today_by_route":budget,
        "canary_engine":state.engine.read().await.clone(),"leaders":state.leaders.read().await.clone(),"counts":counts,"observers":observers}),
    ))
}
async fn leaders(State(state): State<Health>) -> Result<Json<Value>, StatusCode> {
    state.allow()?;
    Ok(Json(state.leaders.read().await.clone()))
}

async fn leader_loop(
    config: Arc<Config>,
    store: Store,
    state: Arc<RwLock<Value>>,
    mut stop: watch::Receiver<bool>,
) -> Result<(), StoreError> {
    let mut schedule: Option<LeaderSchedule> = None;
    loop {
        let slot = store.cursor(ObserverKind::Grpc, Source::Live).await?;
        if let Some(slot) = slot {
            if schedule.as_ref().is_none_or(|s| !s.covers(slot)) {
                match tokio::select! { _=stop.changed()=>return Ok(()),value=LeaderSchedule::fetch(&config)=>value }
                {
                    Ok(next) => {
                        if let Ok(evidence) = next.evidence() {
                            store.save_evidence(&evidence).await?;
                        }
                        schedule = Some(next);
                    }
                    Err(_) => {
                        *state.write().await = json!({"status":"UNAVAILABLE","source":"live"});
                    }
                }
            }
            if let Some(schedule) = &schedule {
                *state.write().await = json!({"status":if schedule.covers(slot){"PASS"}else{"STALE"},"source":"live","epoch":schedule.epoch.to_string(),"at_slot":slot.to_string(),"next_leaders":schedule.next_leaders(slot,3),"classification":"epoch snapshot; fewer than 16 assigned slots means unknown skip rate"});
            }
        }
        tokio::select! { _=stop.changed()=>return Ok(()),_=tokio::time::sleep(Duration::from_secs(15))=>{} }
    }
}

async fn canary_loop(
    config: Arc<Config>,
    store: Store,
    state: Arc<RwLock<Value>>,
    mut stop: watch::Receiver<bool>,
) -> Result<(), StoreError> {
    let interval = config
        .get("ALIGHT_CANARY_INTERVAL_S")
        .unwrap_or("120")
        .parse::<u64>()
        .ok()
        .filter(|s| (30..=3600).contains(s))
        .ok_or(StoreError::Invalid)?;
    let mut engine = alight_canary::engine::Engine::new(&config, store)
        .await
        .map_err(|error| {
            eprintln!("live engine startup failed: {error}");
            StoreError::Invalid
        })?;
    loop {
        let result = tokio::select! {_=stop.changed()=>return Ok(()),r=engine.step(&config)=>r};
        let value = match result {
            Ok(status) => status,
            // Database errors terminate the daemon; stale/no-response preflight never signs.
            Err(alight_canary::engine::EngineError::Store(e)) => return Err(e),
            Err(alight_canary::engine::EngineError::Budget(
                alight_canary::governor::BudgetError::Store(e),
            )) => return Err(e),
            Err(e) => json!({"status":"PREFLIGHT_UNAVAILABLE","error_category":e.to_string()}),
        };
        let delay = if value["canary_id"].is_string() {
            interval
        } else {
            interval.min(15)
        };
        *state.write().await = value;
        tokio::select! {_=stop.changed()=>return Ok(()),_=tokio::time::sleep(Duration::from_secs(delay))=>{}}
    }
}

async fn retention_loop(
    config: Arc<Config>,
    store: Store,
    mut stop: watch::Receiver<bool>,
) -> Result<(), StoreError> {
    let hours = config
        .get("ALIGHT_METADATA_RETENTION_HOURS")
        .unwrap_or("24")
        .parse::<i64>()
        .ok()
        .filter(|h| (1..=168).contains(h))
        .ok_or(StoreError::Invalid)?;
    loop {
        let cutoff = (chrono::Utc::now() - chrono::Duration::hours(hours))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        for _ in 0..20 {
            let pruned = tokio::select! {_=stop.changed()=>return Ok(()),r=store.prune_metadata(&cutoff)=>r}?;
            if pruned == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
        tokio::select! {_=stop.changed()=>return Ok(()),_=tokio::time::sleep(Duration::from_secs(300))=>{}}
    }
}
async fn clock(State(state): State<Health>) -> Result<Json<Value>, StatusCode> {
    state.allow()?;
    let clock = state.clock.read().await;
    Ok(Json(
        json!({"source":"live","method":"gRPC candidate-block Unix seconds over rolling slot distance",
            "mean_slot_ms":clock.mean_slot_ms(),"window":clock.summary(),"minimum_slot_distance":"64","minimum_chain_seconds":"10"}),
    ))
}

async fn write_frames(
    store: Store,
    clock: Arc<RwLock<SlotClock>>,
    mut frames: mpsc::Receiver<Frame>,
) -> Result<(), StoreError> {
    while let Some(frame) = frames.recv().await {
        store
            .record(frame.observer, &frame.event, &frame.raw)
            .await?;
        if frame.observer == ObserverKind::Grpc {
            match frame.event {
                IngestEvent::BlockMeta(e) => clock.write().await.push(&e),
                IngestEvent::Slot(e) => clock.write().await.push_status(&e),
                _ => {}
            }
        }
    }
    Ok(())
}

async fn resolve_loop(
    config: Arc<Config>,
    store: Store,
    mut stop: watch::Receiver<bool>,
) -> Result<(), StoreError> {
    let clock = stream::ReceiveClock::new();
    let mut last_id = String::new();
    loop {
        tokio::select! {_=stop.changed()=>return Ok(()),_=tokio::time::sleep(Duration::from_secs(5))=>{}}
        let mut pending = store.pending_canaries().await?;
        pending.retain(|c| c.source == Source::Live);
        pending.sort_by_key(|c| c.id.clone());
        let next = pending
            .iter()
            .find(|c| c.id > last_id)
            .or_else(|| pending.first());
        let Some(canary) = next else { continue };
        last_id = canary.id.clone();
        let Some(signature) = canary.signature.as_deref() else {
            continue;
        };
        let observations = store.observations(Source::Live, signature).await?;
        let proof = tokio::select! {_=stop.changed()=>return Ok(()),r=alight_ingest::rpc::corroborate(&config,canary.source,signature,canary.sent_slot,&observations,&clock)=>r};
        let mut evidence = ResolutionEvidence {
            observations,
            ..Default::default()
        };
        if let Ok(proof) = proof {
            for (event, raw) in &proof.frames {
                store.record(ObserverKind::Rpc, event, raw).await?;
            }
            if proof.proof.is_some() {
                store.save_evidence(&proof.raw).await?;
            }
            for block in &proof.canonical_blocks {
                // Block evidence is included in the proof bundle; persist its exact subset too.
                if let Some(raw) = proof.raw["blocks"].as_array().and_then(|blocks| {
                    blocks
                        .iter()
                        .find(|r| r["slot"].as_str() == Some(&block.slot.to_string()))
                }) {
                    store.save_evidence(raw).await?;
                }
            }
            evidence.rpc = proof.proof;
            evidence.canonical_blocks = proof.canonical_blocks;
            evidence.observations = store.observations(Source::Live, signature).await?;
        }
        let resolved = alight_canary::resolver::resolve(canary, &evidence, &stream::utc_now());
        if resolved.canary.outcome != canary.outcome
            || resolved.canary.landed_block_id != canary.landed_block_id
            || resolved.canary.observer_first_seen.len() != canary.observer_first_seen.len()
            || resolved.finalized
        {
            store
                .save_resolution(
                    &resolved.canary,
                    resolved.finalized,
                    &stream::utc_now(),
                    &json!({"reason":resolved.reason,"evidence":evidence}),
                )
                .await?;
        }
    }
}

async fn shutdown(seconds: Option<u64>) -> &'static str {
    let timer = async {
        if let Some(seconds) = seconds {
            tokio::time::sleep(Duration::from_secs(seconds)).await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            return tokio::select! {r=tokio::signal::ctrl_c()=>if r.is_ok(){"SIGINT"}else{"signal_registration_failed"},r=signal.recv()=>if r.is_some(){"SIGTERM"}else{"signal_stream_closed"},_=timer=>"runtime_limit"};
        }
    }
    tokio::select! {r=tokio::signal::ctrl_c()=>if r.is_ok(){"SIGINT"}else{"signal_registration_failed"},_=timer=>"runtime_limit"}
}

async fn replay(args: Args) -> Result<(), Error> {
    if args.mode.as_deref() != Some("replay") || args.bind.is_some() || args.disconnect.is_some() {
        return Err(Error::Configuration);
    }
    let path = args.replay.ok_or(Error::Configuration)?;
    if std::fs::metadata(&path)?.len() > 10 * 1024 * 1024 {
        return Err(Error::Replay);
    }
    let data = std::fs::read_to_string(path)?;
    let db = args.db.unwrap_or_else(|| PathBuf::from("data/replay.db"));
    let _lock = lock(&db)?;
    let store = Store::open(&db, 512 * 1024 * 1024).await?;
    let id = uuid::Uuid::new_v4().to_string();
    store.start_run(&id, "replay", &stream::utc_now()).await?;
    let fixture_id = alight_store::raw_ref(&json!(data))?;
    for line in data.lines().filter(|s| !s.is_empty()) {
        if line.len() > 1024 * 1024 {
            return Err(Error::Replay);
        }
        let input: Value = serde_json::from_str(line).map_err(|_| Error::Replay)?;
        let received = ReceiveTime {
            clock_id: fixture_id.clone(),
            mono_ns: input["recv_mono_ns"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .ok_or(Error::Replay)?,
            wall_utc: input["received_at"]
                .as_str()
                .ok_or(Error::Replay)?
                .to_owned(),
        };
        let observer = if input.get("data").is_some() {
            ObserverKind::Mirage
        } else {
            ObserverKind::Grpc
        };
        if let Some((event, raw)) = adapter::normalize(&input, observer, Source::Replay, received)
            .map_err(|_| Error::Replay)?
        {
            store.record(observer, &event, &raw).await?;
        }
    }
    store.end_run(&id, &stream::utc_now()).await?;
    println!(
        "{}",
        json!({"mode":"replay","source":"replay","network_requests":0,"canaries_sent":0,"counts":store.counts(Source::Replay).await?})
    );
    store.close().await;
    Ok(())
}

async fn run() -> Result<(), Error> {
    let args = Args::parse()?;
    if args.mode.as_deref() == Some("replay") {
        return replay(args).await;
    }
    if args.replay.is_some() {
        return Err(Error::Configuration);
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = Config::load_observer().map_err(|_| Error::Configuration)?;
    let mode = match args
        .mode
        .as_deref()
        .or(config.get("ALIGHT_MODE"))
        .unwrap_or("observe")
    {
        "observe" => RunMode::Observe,
        "live" => RunMode::Live,
        _ => return Err(Error::Configuration),
    };
    let bind = args.bind.unwrap_or(
        config
            .get("ALIGHT_BIND")
            .unwrap_or("127.0.0.1:8080")
            .parse()
            .map_err(|_| Error::Configuration)?,
    );
    if !bind.ip().is_loopback() && config.get("ALIGHT_ALLOW_REMOTE_BIND") != Some("true") {
        return Err(Error::Configuration);
    }
    let db = args
        .db
        .unwrap_or_else(|| PathBuf::from(config.get("ALIGHT_DB_PATH").unwrap_or("data/alight.db")));
    let max_bytes = config
        .get("ALIGHT_DB_MAX_BYTES")
        .unwrap_or("536870912")
        .parse()
        .map_err(|_| Error::Configuration)?;
    let _lock = lock(&db)?;
    let genesis = HttpProbe::new()
        .map_err(Error::Observer)?
        .rpc(&config, "getGenesisHash", json!([]))
        .await
        .map_err(Error::Observer)?;
    if genesis.as_str() != Some(MAINNET_GENESIS) {
        return Err(Error::Observer(alight_ingest::ProbeError::WrongCluster));
    }
    let (mut grpc, mirage) = stream::options(&config)
        .await
        .map_err(Error::ObserverSetup)?;
    grpc.force_disconnect_after = args.disconnect.map(Duration::from_secs);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let store = Store::open(&db, max_bytes).await?;
    let id = uuid::Uuid::new_v4().to_string();
    store
        .start_run(
            &id,
            if mode == RunMode::Live {
                "live"
            } else {
                "observe"
            },
            &stream::utc_now(),
        )
        .await?;
    let clock = Arc::new(RwLock::new(SlotClock::default()));
    for block in store
        .block_samples(ObserverKind::Grpc, Source::Live)
        .await?
    {
        clock.write().await.push(&block);
    }
    let engine_state = Arc::new(RwLock::new(
        json!({"status":if mode==RunMode::Live{"STARTING"}else{"DISABLED"}}),
    ));
    let leader_state = Arc::new(RwLock::new(json!({"status":"STARTING","source":"live"})));
    let state = Health {
        store: store.clone(),
        clock: clock.clone(),
        started: Instant::now(),
        run_id: id.clone(),
        mirage: mirage.is_some(),
        mode,
        engine: engine_state.clone(),
        leaders: leader_state.clone(),
        limiter: Arc::new(Mutex::new((Instant::now(), 10))),
    };
    let app = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/clock", get(self::clock))
        .route("/v1/leaders", get(leaders))
        .with_state(state);
    let (stop, rx) = watch::channel(false);
    let mut server_rx = rx.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = server_rx.changed().await;
            })
            .await
    });
    let (tx, frames) = mpsc::channel(128);
    let mut workers = tokio::task::JoinSet::new();
    workers.spawn(stream::grpc_worker(
        grpc,
        store.clone(),
        tx.clone(),
        rx.clone(),
    ));
    let config = Arc::new(config);
    workers.spawn(resolve_loop(config.clone(), store.clone(), rx.clone()));
    workers.spawn(leader_loop(
        config.clone(),
        store.clone(),
        leader_state,
        rx.clone(),
    ));
    workers.spawn(retention_loop(config.clone(), store.clone(), rx.clone()));
    if mode == RunMode::Live {
        // Load keys only in this worker's private configuration, never in observer state.
        let signing = Arc::new(Config::load().map_err(|_| Error::Configuration)?);
        workers.spawn(canary_loop(
            signing,
            store.clone(),
            engine_state,
            rx.clone(),
        ));
    }
    if let Some(mirage) = mirage {
        workers.spawn(stream::mirage_worker(mirage, store.clone(), tx.clone(), rx));
    }
    drop(tx);
    let mut writer = tokio::spawn(write_frames(store.clone(), clock, frames));
    println!(
        "{}",
        json!({"mode":mode,"source":"live","status":"STARTING","signing_enabled":mode==RunMode::Live,"run_id":id})
    );
    let mut worker_failed = false;
    let mut stop_reason = "writer_closed";
    let writer_result = tokio::select! {
        reason=shutdown(args.run_for)=>{stop_reason=reason;None},
        result=&mut writer=>Some(result),
        _=workers.join_next()=>{worker_failed=true;None},
    };
    let _ = stop.send(true);
    while let Some(result) = workers.join_next().await {
        result.map_err(|_| Error::Task)??;
    }
    match writer_result {
        Some(result) => result.map_err(|_| Error::Task)??,
        None => writer.await.map_err(|_| Error::Task)??,
    }
    server.await.map_err(|_| Error::Task)??;
    if worker_failed {
        return Err(Error::Task);
    }
    store.end_run(&id, &stream::utc_now()).await?;
    println!(
        "{}",
        json!({"mode":mode,"status":"STOPPED","stop_reason":stop_reason,"send_attempts":store.send_summary(Source::Live).await?,"counts":store.counts(Source::Live).await?})
    );
    store.close().await;
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
