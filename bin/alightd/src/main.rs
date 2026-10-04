//! Local observe daemon. Signing and sending are not exposed by this binary.
use alight_ingest::{
    Config, HttpProbe, MAINNET_GENESIS, adapter,
    clock::SlotClock,
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
    #[error("observer startup failed; upstream values withheld")]
    Observer,
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
                "--mode" if ["observe", "replay"].contains(&value.as_str()) => {
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
    Ok(Json(
        json!({"schema_version":1,"mode":"observe","source":"live","run_id":state.run_id,
        "status":if ready{"PASS"}else{"DEGRADED"},"uptime_s":state.started.elapsed().as_secs().to_string(),
        "signing_enabled":false,"canaries_sent":0,"counts":counts,"observers":observers}),
    ))
}
async fn clock(State(state): State<Health>) -> Result<Json<Value>, StatusCode> {
    state.allow()?;
    Ok(Json(
        json!({"source":"live","method":"gRPC candidate-block Unix seconds over rolling slot distance",
        "mean_slot_ms":state.clock.read().await.mean_slot_ms(),"minimum_slot_distance":"64","minimum_chain_seconds":"10"}),
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
        if frame.observer == ObserverKind::Grpc
            && let IngestEvent::BlockMeta(e) = frame.event
        {
            clock.write().await.push(&e);
        }
    }
    Ok(())
}

async fn shutdown(seconds: Option<u64>) {
    let duration = Duration::from_secs(seconds.unwrap_or(365 * 24 * 3600));
    #[cfg(unix)]
    {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {_=tokio::signal::ctrl_c()=>{},_=signal.recv()=>{},_=tokio::time::sleep(duration)=>{}}
            return;
        }
    }
    tokio::select! {_=tokio::signal::ctrl_c()=>{},_=tokio::time::sleep(duration)=>{}}
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
    if args.mode.is_none() && config.get("ALIGHT_MODE").is_some_and(|s| s != "observe") {
        return Err(Error::Configuration);
    }
    let bind = args.bind.unwrap_or(
        config
            .get("ALIGHT_BIND")
            .unwrap_or("127.0.0.1:8080")
            .parse()
            .map_err(|_| Error::Configuration)?,
    );
    if !bind.ip().is_loopback() {
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
        .map_err(|_| Error::Observer)?
        .rpc(&config, "getGenesisHash", json!([]))
        .await
        .map_err(|_| Error::Observer)?;
    if genesis.as_str() != Some(MAINNET_GENESIS) {
        return Err(Error::Observer);
    }
    let (mut grpc, mirage) = stream::options(&config)
        .await
        .map_err(|_| Error::Observer)?;
    grpc.force_disconnect_after = args.disconnect.map(Duration::from_secs);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let store = Store::open(&db, max_bytes).await?;
    let id = uuid::Uuid::new_v4().to_string();
    store.start_run(&id, "observe", &stream::utc_now()).await?;
    let clock = Arc::new(RwLock::new(SlotClock::default()));
    let state = Health {
        store: store.clone(),
        clock: clock.clone(),
        started: Instant::now(),
        run_id: id.clone(),
        mirage: mirage.is_some(),
        limiter: Arc::new(Mutex::new((Instant::now(), 10))),
    };
    let app = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/clock", get(self::clock))
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
    if let Some(mirage) = mirage {
        workers.spawn(stream::mirage_worker(mirage, store.clone(), tx.clone(), rx));
    }
    drop(tx);
    let mut writer = tokio::spawn(write_frames(store.clone(), clock, frames));
    println!(
        "{}",
        json!({"mode":"observe","source":"live","status":"STARTING","signing_enabled":false,"run_id":id})
    );
    let mut worker_failed = false;
    let writer_result = tokio::select! {
        _=shutdown(args.run_for)=>None,
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
        json!({"mode":"observe","status":"STOPPED","canaries_sent":0,"counts":store.counts(Source::Live).await?})
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
