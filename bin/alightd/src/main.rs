//! Persistent observe/live collector. Signing is confined to the governed canary engine.
mod alerts;
mod diagnostics;
mod market;
use alight_ingest::{
    Config, HttpProbe, MAINNET_GENESIS, adapter,
    clock::SlotClock,
    leaders::LeaderSchedule,
    stream::{self, Frame},
};
use alight_store::{Store, StoreError};
use alight_types::*;
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
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
    operator_key_file: Option<PathBuf>,
    seed: Option<u64>,
    sim_canaries: Option<u32>,
    sim_regimes: bool,
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
                "--mode" if ["observe", "live", "replay", "sim"].contains(&value.as_str()) => {
                    result.mode = Some(value)
                }
                "--replay" => result.replay = Some(value.into()),
                "--operator-key-file" => result.operator_key_file = Some(value.into()),
                "--seed" => result.seed = Some(value.parse().map_err(|_| Error::Configuration)?),
                "--sim-canaries" => {
                    result.sim_canaries = Some(value.parse().map_err(|_| Error::Configuration)?)
                }
                "--sim-regimes" => {
                    result.sim_regimes = value.parse().map_err(|_| Error::Configuration)?
                }
                "--run-for" => {
                    result.run_for = Some(value.parse().map_err(|_| Error::Configuration)?)
                }
                "--force-disconnect-after" => {
                    result.disconnect = Some(value.parse().map_err(|_| Error::Configuration)?)
                }
                _ => return Err(Error::Configuration),
            }
        }
        if result.run_for == Some(0)
            || result.disconnect == Some(0)
            || result.sim_regimes && result.mode.as_deref() != Some("sim")
        {
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
                *state.write().await = json!({"status":if schedule.covers(slot){"PASS"}else{"STALE"},"source":"live","epoch":schedule.epoch.to_string(),"at_slot":slot.to_string(),"next_leaders":schedule.next_leaders(slot,3),"next_slots":schedule.next_slots(slot,8),"classification":"epoch snapshot; fewer than 16 assigned slots means unknown skip rate"});
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
    let mut engine = alight_canary::engine::Engine::new(&config, store.clone())
        .await
        .map_err(|error| {
            eprintln!("live engine startup failed: {error}");
            StoreError::Invalid
        })?;
    loop {
        let now = stream::utc_now();
        let context = alight_forecast::current_context(&store, Source::Live, "local", &now).await?;
        let reports = store.proves(Source::Live, 100, true).await?;
        let mut queue = Vec::new();
        for report in reports {
            let report = alight_prove::refresh(&store, &report, &now, &context.regime_id)
                .await
                .map_err(|_| StoreError::Invalid)?;
            if !matches!(report.state, ProveState::Complete | ProveState::Voided) {
                queue.push(report);
            }
        }
        let proving = !queue.is_empty();
        let scheduled = queue.iter().find(|r| r.attempts < r.lock.n);
        let result = if let Some(report) = scheduled {
            tokio::select! {_=stop.changed()=>return Ok(()),r=engine.step_prove(&config,&report.lock)=>r}
        } else if proving {
            Ok(json!({"status":"WAITING_PROVE_OUTCOMES"}))
        } else {
            tokio::select! {_=stop.changed()=>return Ok(()),r=engine.step(&config)=>r}
        };
        let value = match result {
            Ok(status) => status,
            // Database errors terminate the daemon; stale/no-response preflight never signs.
            Err(alight_canary::engine::EngineError::Store(e)) => return Err(e),
            Err(alight_canary::engine::EngineError::Budget(
                alight_canary::governor::BudgetError::Store(e),
            )) => return Err(e),
            Err(e) => json!({"status":"PREFLIGHT_UNAVAILABLE","error_category":e.to_string()}),
        };
        if let Some(report) = scheduled {
            let mut next =
                alight_prove::refresh(&store, report, &stream::utc_now(), &context.regime_id)
                    .await
                    .map_err(|_| StoreError::Invalid)?;
            if !matches!(next.state, ProveState::Complete | ProveState::Voided) {
                next.state = match value["status"].as_str() {
                    Some("WAITING_FUNDS") => ProveState::WaitingFunds,
                    Some("BUDGET_CAPPED") => ProveState::BudgetCapped,
                    Some(
                        "WAITING_OBSERVER"
                        | "WAITING_EPOCH"
                        | "PENDING_LIMIT"
                        | "PREFLIGHT_UNAVAILABLE",
                    ) => ProveState::WaitingObserver,
                    _ => ProveState::Running,
                };
                next.reason = value["status"].as_str().map(str::to_owned);
                store.save_prove_report(&next).await?;
            }
        }
        let delay = if proving {
            1
        } else if value["canary_id"].is_string() {
            interval
        } else {
            interval.min(15)
        };
        *state.write().await = value;
        tokio::select! {_=stop.changed()=>return Ok(()),_=tokio::time::sleep(Duration::from_secs(delay))=>{}}
    }
}

async fn model_loop(store: Store, mut stop: watch::Receiver<bool>) -> Result<(), StoreError> {
    loop {
        let now = stream::utc_now();
        tokio::select! { _=stop.changed()=>return Ok(()),result=alight_forecast::tick(&store,Source::Live,"local",&now)=>{ result?; } }
        tokio::select! { _=stop.changed()=>return Ok(()),result=alight_forecast::daily(&store,Source::Live,&now)=>{ result?; } }
        tokio::select! { _=stop.changed()=>return Ok(()),_=tokio::time::sleep(Duration::from_secs(300))=>{} }
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
        let tape_limits = TapeLimits::default();
        let now = stream::utc_now();
        tokio::select! { _=stop.changed()=>return Ok(()),r=store.prune_passive_tips(Source::Live,&tape_limits,&now)=>{r?;} }
        tokio::select! {_=stop.changed()=>return Ok(()),_=tokio::time::sleep(Duration::from_secs(300))=>{}}
    }
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
        match frame.passive_tips {
            Ok(tips) if !tips.is_empty() => {
                store
                    .save_passive_tips(&tips, &TapeLimits::default(), &stream::utc_now())
                    .await?;
            }
            Err(reason) => eprintln!("passive tape record skipped: {reason}"),
            _ => {}
        }
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
        let delay = if store
            .proves(Source::Live, 8, true)
            .await?
            .iter()
            .any(|p| !matches!(p.state, ProveState::Complete | ProveState::Voided))
        {
            1
        } else {
            5
        };
        tokio::select! {_=stop.changed()=>return Ok(()),_=tokio::time::sleep(Duration::from_secs(delay))=>{}}
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

fn operator_key(path: Option<&Path>, configured: Option<&str>) -> Result<Option<String>, Error> {
    if let Some(path) = path {
        if std::fs::metadata(path)?.len() > 258 {
            return Err(Error::Configuration);
        }
        let text = std::fs::read_to_string(path)?;
        Ok(Some(text.trim().to_owned()))
    } else {
        Ok(configured.map(str::to_owned))
    }
}

async fn sim_server(args: Args) -> Result<(), Error> {
    if args.replay.is_some() || args.disconnect.is_some() {
        return Err(Error::Configuration);
    }
    let bind = args
        .bind
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 8080)));
    if !bind.ip().is_loopback() {
        return Err(Error::Configuration);
    }
    let db = args
        .db
        .unwrap_or_else(|| PathBuf::from(".alight/sim-api.db"));
    let _lock = lock(&db)?;
    let store = Store::open(&db, 512 * 1024 * 1024).await?;
    if store.counts(Source::Live).await?.canaries != 0
        || store.counts(Source::Live).await?.slot_events != 0
    {
        return Err(Error::Configuration);
    }
    let params = alight_sim::Parameters {
        seed: args.seed.unwrap_or(42),
        canaries: args.sim_canaries.unwrap_or(1000),
        ..Default::default()
    };
    params.validate().map_err(|_| Error::Configuration)?;
    let generation = params.clone();
    let mut data = tokio::task::spawn_blocking(move || {
        alight_sim::generate_focused(&generation, Route::BeamHttp, SizeClass::Small)
    })
    .await
    .map_err(|_| Error::Task)?
    .map_err(|_| Error::Configuration)?;
    if args.sim_regimes {
        let first = data.canaries.first().ok_or(Error::Configuration)?;
        let start = (alight_diagnostics::utc(&first.send_wall_utc)? - chrono::Duration::hours(2))
            .to_rfc3339();
        alight_diagnostics::simulation::scenario(&store, params.seed, &start).await?;
        let regime = store
            .active_regime(Source::Sim, &data.as_of_utc)
            .await?
            .ok_or(Error::Configuration)?;
        // These are freshly generated samples after the synthetic change. Stored old labels are never edited.
        for c in &mut data.canaries {
            c.regime_id = regime.regime_id.clone();
            c.id = format!("phase5-{}", c.id);
            c.signature = Some(c.id.clone());
        }
    }
    for sample in data.training().map_err(|_| Error::Configuration)? {
        store.import_training(&sample).await?;
    }
    let mut as_of = data.as_of_utc.clone();
    if let Some(floor) = store.experiment_clock_floor(Source::Sim).await? {
        let time =
            chrono::DateTime::parse_from_rfc3339(&floor).map_err(|_| Error::Configuration)?;
        if time > chrono::DateTime::parse_from_rfc3339(&as_of).map_err(|_| Error::Configuration)? {
            as_of = floor;
        }
    }
    alight_forecast::tick(&store, Source::Sim, "local", &as_of).await?;
    alight_diagnostics::refresh(&store, Source::Sim, &as_of, &[]).await?;
    let api = alight_api::ApiState::new(
        store.clone(),
        alight_api::Options {
            mode: RunMode::Sim,
            region: "local".into(),
            run_id: format!("sim-{}", params.seed),
            operator_key: operator_key(args.operator_key_file.as_deref(), None)?,
            simulated_as_of_utc: Some(as_of),
            simulation: Some(SimProofEnvironment {
                slot_ms: params.slot_ms,
                congestion: params.congestion,
                tip_slope: params.tip_slope,
                fee_slope: params.fee_slope,
                never_land_mass: params.never_land_mass,
                continuous_latency: params.continuous_latency,
            }),
            ..Default::default()
        },
    )
    .map_err(|_| Error::Configuration)?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    println!(
        "{}",
        json!({"mode":"sim","source":"sim","status":"READY","bind":listener.local_addr()?.to_string(),"signing_enabled":false,"network_provider_requests":0})
    );
    axum::serve(listener, alight_api::router(api))
        .with_graceful_shutdown(async move {
            shutdown(args.run_for).await;
        })
        .await?;
    store.close().await;
    Ok(())
}

async fn run() -> Result<(), Error> {
    let args = Args::parse()?;
    // Sim returns before reading .env, constructing a provider client, or loading keys.
    if args.mode.as_deref() == Some("sim") {
        return sim_server(args).await;
    }
    if args.seed.is_some() || args.sim_canaries.is_some() {
        return Err(Error::Configuration);
    }
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
    let mut api = alight_api::ApiState::new(
        store.clone(),
        alight_api::Options {
            mode,
            region: "local".into(),
            run_id: id.clone(),
            mirage_enabled: mirage.is_some(),
            operator_key: operator_key(
                args.operator_key_file.as_deref(),
                config.get("ALIGHT_OPERATOR_KEY"),
            )?,
            webhook_secret: config.get("SOLAMI_WEBHOOK_SECRET").map(str::to_owned),
            daily_budget_sol: config.get("ALIGHT_DAILY_BUDGET_SOL").map(str::to_owned),
            burst_budget_sol: config.get("ALIGHT_BURST_BUDGET_SOL").map(str::to_owned),
            ..Default::default()
        },
    )
    .map_err(|_| Error::Configuration)?;
    api.clock = clock.clone();
    api.engine = engine_state.clone();
    api.leaders = leader_state.clone();
    let app = alight_api::router(api);
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
    workers.spawn(model_loop(store.clone(), rx.clone()));
    workers.spawn(diagnostics::run(config.clone(), store.clone(), rx.clone()));
    workers.spawn(alerts::run(config.clone(), store.clone(), rx.clone()));
    workers.spawn(market::run(
        config.clone(),
        store.clone(),
        clock.clone(),
        rx.clone(),
    ));
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
