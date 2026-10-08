//! Source-scoped REST/WS boundary. This crate has no signing key or transaction sender.
mod openapi;
mod receipts;
use alight_ingest::clock::SlotClock;
use alight_store::{Store, StoreError};
use alight_types::*;
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{
        Path, Query, Request, State,
        rejection::{JsonRejection, QueryRejection},
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::{Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
pub use openapi::document as openapi_document;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use tokio::sync::{Mutex as AsyncMutex, OwnedSemaphorePermit, RwLock, Semaphore};

const BODY_BYTES: usize = 65536;
const QUERY_BYTES: usize = 8192;
const STREAM_BYTES: usize = 1048576;

#[derive(Debug, thiserror::Error)]
#[error("invalid API configuration")]
pub struct ConfigurationError;
pub struct Options {
    pub mode: RunMode,
    pub region: String,
    pub run_id: String,
    pub operator_key: Option<String>,
    pub webhook_secret: Option<String>,
    pub mirage_enabled: bool,
    pub simulated_as_of_utc: Option<String>,
    pub simulation: Option<SimProofEnvironment>,
    pub requests_per_second: u32,
    pub max_streams: u32,
    pub daily_budget_sol: Option<String>,
    pub burst_budget_sol: Option<String>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            mode: RunMode::Observe,
            region: "local".into(),
            run_id: "local".into(),
            operator_key: None,
            webhook_secret: None,
            mirage_enabled: false,
            simulated_as_of_utc: None,
            simulation: None,
            requests_per_second: 20,
            max_streams: 32,
            daily_budget_sol: None,
            burst_budget_sol: None,
        }
    }
}
#[derive(Clone)]
pub struct ApiState {
    pub store: Store,
    pub source: Source,
    pub mode: RunMode,
    pub clock: Arc<RwLock<SlotClock>>,
    pub engine: Arc<RwLock<Value>>,
    pub leaders: Arc<RwLock<Value>>,
    region: String,
    run_id: String,
    mirage: bool,
    started: Instant,
    as_of: Arc<RwLock<Option<String>>>,
    simulation: Option<SimProofEnvironment>,
    operator_digest: Option<[u8; 32]>,
    webhook_secret: Option<Arc<str>>,
    rate: Arc<Mutex<(Instant, u32)>>,
    rate_limit: u32,
    write_rate: Arc<Mutex<(Instant, u32)>>,
    work: Arc<Semaphore>,
    streams: Arc<Semaphore>,
    controls: Arc<AsyncMutex<()>>,
    daily: Option<String>,
    burst: Option<String>,
}
impl ApiState {
    pub fn new(store: Store, options: Options) -> Result<Self, ConfigurationError> {
        let source = match options.mode {
            RunMode::Live | RunMode::Observe => Source::Live,
            RunMode::Sim => Source::Sim,
            RunMode::Replay => Source::Replay,
        };
        if options.region.is_empty()
            || options.region.len() > 64
            || options.run_id.len() > 128
            || !(1..=200).contains(&options.requests_per_second)
            || !(1..=32).contains(&options.max_streams)
            || options.operator_key.as_ref().is_some_and(|k| {
                !(16..=256).contains(&k.len()) || !k.bytes().all(|b| b.is_ascii_graphic())
            })
            || source == Source::Sim
                && (options.simulated_as_of_utc.is_none() || options.simulation.is_none())
            || source == Source::Live
                && (options.simulated_as_of_utc.is_some() || options.simulation.is_some())
            || source == Source::Replay && options.simulation.is_some()
            || options
                .webhook_secret
                .as_ref()
                .is_some_and(|s| source != Source::Live || !(16..=256).contains(&s.len()))
        {
            return Err(ConfigurationError);
        }
        if let Some(time) = &options.simulated_as_of_utc {
            utc(time).map_err(|_| ConfigurationError)?;
        }
        if let Some(env) = &options.simulation {
            alight_canary::governor::Governor::new(
                store.clone(),
                RunMode::Sim,
                options.daily_budget_sol.as_deref(),
                options.burst_budget_sol.as_deref(),
                60000,
            )
            .map_err(|_| ConfigurationError)?;
            alight_sim::Parameters {
                canaries: 1,
                slot_ms: env.slot_ms,
                congestion: env.congestion,
                tip_slope: env.tip_slope,
                fee_slope: env.fee_slope,
                never_land_mass: env.never_land_mass,
                continuous_latency: env.continuous_latency,
                ..Default::default()
            }
            .validate()
            .map_err(|_| ConfigurationError)?;
        }
        let operator_digest = options
            .operator_key
            .map(|key| Sha256::digest(key.as_bytes()).into());
        Ok(Self {
            store,
            source,
            mode: options.mode,
            clock: Arc::new(RwLock::new(SlotClock::default())),
            engine: Arc::new(RwLock::new(json!({"status":"DISABLED"}))),
            leaders: Arc::new(RwLock::new(json!({"status":"UNAVAILABLE","source":source}))),
            region: options.region,
            run_id: options.run_id,
            mirage: options.mirage_enabled,
            started: Instant::now(),
            as_of: Arc::new(RwLock::new(options.simulated_as_of_utc)),
            simulation: options.simulation,
            operator_digest,
            webhook_secret: options.webhook_secret.map(Arc::from),
            rate: Arc::new(Mutex::new((Instant::now(), options.requests_per_second))),
            rate_limit: options.requests_per_second,
            write_rate: Arc::new(Mutex::new((Instant::now(), 2))),
            work: Arc::new(Semaphore::new(2)),
            streams: Arc::new(Semaphore::new(options.max_streams as usize)),
            controls: Arc::new(AsyncMutex::new(())),
            daily: options.daily_budget_sol,
            burst: options.burst_budget_sol,
        })
    }
    async fn now(&self) -> String {
        self.as_of
            .read()
            .await
            .clone()
            .unwrap_or_else(|| Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
    }
    async fn advance(&self, time: &str) -> Result<(), ApiError> {
        let new = utc(time).map_err(|_| self.invalid())?;
        let mut clock = self.as_of.write().await;
        if let Some(old) = clock.as_ref()
            && new > utc(old).map_err(|_| self.unavailable())?
        {
            *clock = Some(time.into());
        }
        Ok(())
    }
    fn error(&self, status: StatusCode, code: &'static str, message: &'static str) -> ApiError {
        ApiError {
            source: self.source,
            status,
            code,
            message,
        }
    }
    fn invalid(&self) -> ApiError {
        self.error(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "Request fields or scope are invalid",
        )
    }
    fn unavailable(&self) -> ApiError {
        self.error(
            StatusCode::SERVICE_UNAVAILABLE,
            "DATA_UNAVAILABLE",
            "Stored evidence is unavailable",
        )
    }
    fn busy(&self) -> ApiError {
        self.error(
            StatusCode::TOO_MANY_REQUESTS,
            "RATE_LIMITED",
            "Request or concurrency limit reached",
        )
    }
    fn work(&self) -> Result<OwnedSemaphorePermit, ApiError> {
        self.work
            .clone()
            .try_acquire_owned()
            .map_err(|_| self.busy())
    }
    fn rate(&self, write: bool) -> Result<(), ApiError> {
        let (bucket, limit) = if write {
            (&self.write_rate, 2)
        } else {
            (&self.rate, self.rate_limit)
        };
        let mut bucket = bucket.lock().map_err(|_| self.unavailable())?;
        if bucket.0.elapsed() >= Duration::from_secs(1) {
            *bucket = (Instant::now(), limit);
        }
        if bucket.1 == 0 {
            return Err(self.busy());
        }
        bucket.1 -= 1;
        Ok(())
    }
    fn authenticate(&self, request: &Request) -> Result<(), ApiError> {
        let expected = self.operator_digest.ok_or_else(|| {
            self.error(
                StatusCode::FORBIDDEN,
                "OPERATOR_DISABLED",
                "Operator actions are disabled",
            )
        })?;
        let supplied = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        let valid = supplied.filter(|s| s.len() <= 256).is_some_and(|s| {
            let digest: [u8; 32] = Sha256::digest(s.as_bytes()).into();
            bool::from(expected.ct_eq(&digest))
        });
        if !valid {
            return Err(self.error(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "A valid operator bearer key is required",
            ));
        }
        Ok(())
    }
}
pub struct ApiError {
    source: Source,
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(ApiErrorResponse {
                source: self.source,
                code: self.code.into(),
                message: self.message.into(),
            }),
        )
            .into_response();
        if self.status == StatusCode::TOO_MANY_REQUESTS {
            response.headers_mut().insert(
                header::RETRY_AFTER,
                axum::http::HeaderValue::from_static("1"),
            );
        }
        if self.status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                axum::http::HeaderValue::from_static("Bearer"),
            );
        }
        response
    }
}
fn utc(s: &str) -> Result<DateTime<Utc>, ()> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| ())
}
fn short(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}

pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/clock", get(clock))
        .route("/v1/leaders", get(leaders))
        .route("/v1/curve", get(curve))
        .route("/v1/quote", get(preview).post(freeze))
        .route("/v1/prove", post(prove))
        .route("/v1/prove/{id}", get(prove_report))
        .route("/v1/proves", get(proves))
        .route("/v1/ledger", get(ledger))
        .route("/v1/ledger/verify", get(verify))
        .route("/v1/ledger/anchor", get(anchor_draft))
        .route("/v1/ledger/payloads", get(browser_ledger))
        .route("/v1/workbench", get(workbench))
        .route("/v1/diagnostics", get(diagnostics))
        .route("/v1/webhook", post(webhook))
        .route("/v1/canaries/{id}/observations", get(canary_evidence))
        .route("/v1/prove/{id}/canaries", get(prove_canaries))
        .route("/v1/tape", get(tape))
        .route("/v1/receipt/{wallet}", get(receipts::receipt))
        .route("/v1/observers", get(observers))
        .route("/v1/openapi.json", get(schema))
        .route("/v1/stream", get(stream))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(middleware::from_fn_with_state(state.clone(), boundary))
        .with_state(state)
}
async fn boundary(State(state): State<ApiState>, mut request: Request, next: Next) -> Response {
    let result = async {
        state.rate(false)?;
        if request.uri().query().is_some_and(|q| q.len() > QUERY_BYTES) {
            return Err(state.error(
                StatusCode::URI_TOO_LONG,
                "REQUEST_TOO_LARGE",
                "Query exceeds 8192 bytes",
            ));
        }
        // Authentication precedes body extraction and every possible mutation.
        if request.method() != Method::GET && request.method() != Method::HEAD {
            let webhook = request.uri().path() == "/v1/webhook" && request.method() == Method::POST;
            if !webhook {
                state.authenticate(&request)?;
            } else if state.webhook_secret.is_none() {
                return Err(state.error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "WEBHOOK_DISABLED",
                    "Webhook observation is disabled",
                ));
            }
            state.rate(true)?;
            let (parts, body) = request.into_parts();
            let bytes = tokio::time::timeout(Duration::from_secs(5), to_bytes(body, BODY_BYTES))
                .await
                .map_err(|_| {
                    state.error(
                        StatusCode::REQUEST_TIMEOUT,
                        "BODY_TIMEOUT",
                        "Request body did not arrive in time",
                    )
                })?
                .map_err(|_| {
                    state.error(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "REQUEST_TOO_LARGE",
                        "Request body exceeds 65536 bytes",
                    )
                })?;
            if webhook {
                let signature = parts
                    .headers
                    .get("x-webhook-signature")
                    .and_then(|h| h.to_str().ok())
                    .unwrap_or("");
                if !state.webhook_secret.as_ref().is_some_and(|s| {
                    alight_ingest::webhook::verify(s.as_bytes(), signature, &bytes)
                }) {
                    return Err(state.error(
                        StatusCode::UNAUTHORIZED,
                        "WEBHOOK_SIGNATURE_INVALID",
                        "Webhook signature was rejected",
                    ));
                }
            }
            request = Request::from_parts(parts, Body::from(bytes));
        }
        Ok::<_, ApiError>(next.run(request).await)
    }
    .await;
    let mut response = match result {
        Ok(r) => r,
        Err(e) => e.into_response(),
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        axum::http::HeaderValue::from_static("nosniff"),
    );
    response
}
async fn not_found(State(s): State<ApiState>) -> ApiError {
    s.error(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        "Resource does not exist",
    )
}
async fn method_not_allowed(State(s): State<ApiState>) -> ApiError {
    s.error(
        StatusCode::METHOD_NOT_ALLOWED,
        "METHOD_NOT_ALLOWED",
        "Method is not supported",
    )
}
async fn schema() -> Json<Value> {
    Json(openapi_document())
}
async fn diagnostics(State(s): State<ApiState>) -> Result<Json<DiagnosticsPage>, ApiError> {
    let now = s.now().await;
    Ok(Json(
        s.store
            .diagnostics(s.source, &now)
            .await
            .map_err(|_| s.unavailable())?
            .unwrap_or_else(|| DiagnosticsPage {
                source: s.source,
                as_of_utc: now,
                signals: vec![],
                regimes: vec![],
                observers: vec![],
                pairs: vec![],
                disagreements: vec![],
                expected_observers: vec![],
                owned_window_n: 0,
                fidelity: FidelityDiagnostic {
                    comparisons: vec![],
                    excluded_unmatched: 0,
                    excluded_conflicting: 0,
                    limits: vec!["No diagnostic snapshot is available".into()],
                },
                backfills: vec![],
                alerts: vec![],
                limits: vec![
                    "Diagnostic collection has not produced a snapshot for this source".into(),
                ],
            }),
    ))
}
async fn webhook(
    State(s): State<ApiState>,
    body: axum::body::Bytes,
) -> Result<Json<WebhookReceipt>, ApiError> {
    let (clock_id, started) = alight_types::process_clock_origin();
    let received = ReceiveTime {
        clock_id,
        mono_ns: u64::try_from(started.elapsed().as_nanos()).map_err(|_| s.unavailable())?,
        wall_utc: s.now().await,
    };
    let (event, raw) =
        alight_ingest::webhook::normalize(&body, s.source, received).map_err(|_| s.invalid())?;
    let Some(canary) = s
        .store
        .diagnostic_canary(s.source, &event.signature)
        .await
        .map_err(|_| s.unavailable())?
    else {
        return Ok(Json(WebhookReceipt {
            source: s.source,
            status: WebhookStatus::IgnoredUnowned,
        }));
    };
    if utc(&event.received.wall_utc).map_err(|_| s.invalid())?
        < utc(&canary.send_wall_utc).map_err(|_| s.invalid())?
        || event.slot.is_some_and(|slot| slot < canary.sent_slot)
    {
        return Err(s.invalid());
    }
    s.store
        .record(
            ObserverKind::Webhook,
            &IngestEvent::Observation(event),
            &raw,
        )
        .await
        .map_err(|_| s.unavailable())?;
    Ok(Json(WebhookReceipt {
        source: s.source,
        status: WebhookStatus::Accepted,
    }))
}
async fn observers(State(s): State<ApiState>) -> Result<Json<ObserverHealthPage>, ApiError> {
    Ok(Json(ObserverHealthPage {
        source: s.source,
        observers: observer_views(&s).await?,
    }))
}
async fn workbench(State(s): State<ApiState>) -> Result<Json<WorkbenchEvidence>, ApiError> {
    let _permit = s.work()?;
    let now = s.now().await;
    let canaries = s
        .store
        .workbench_canaries(s.source, &now, 100)
        .await
        .map_err(|_| s.unavailable())?;
    let grades = s
        .store
        .workbench_grades(s.source, &now)
        .await
        .map_err(|_| s.unavailable())?;
    let regimes = s
        .store
        .workbench_regimes(s.source, &now)
        .await
        .map_err(|_| s.unavailable())?;
    let gaps = s
        .store
        .workbench_gaps(s.source, &now)
        .await
        .map_err(|_| s.unavailable())?;
    let cap = alight_canary::governor::sol_to_lamports(s.daily.as_deref().unwrap_or("0.20"))
        .map_err(|_| s.unavailable())?;
    Ok(Json(WorkbenchEvidence {
        source: s.source,
        as_of_utc: now,
        canaries,
        grades,
        regimes,
        gaps,
        daily_cap_lamports: cap,
        limit: 100,
        operator_enabled: s.operator_digest.is_some()
            && matches!(s.mode, RunMode::Live | RunMode::Sim),
        runway: match s.leaders.read().await.get("next_slots") {
            Some(value) => serde_json::from_value(value.clone()).map_err(|_| s.unavailable())?,
            None => vec![],
        },
    }))
}
async fn canary_evidence(
    State(s): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<CanaryEvidencePage>, ApiError> {
    if !short(&id, 128) {
        return Err(s.invalid());
    }
    let _permit = s.work()?;
    let canary = s
        .store
        .workbench_canary(s.source, &id)
        .await
        .map_err(|_| s.unavailable())?
        .ok_or_else(|| s.error(StatusCode::NOT_FOUND, "NOT_FOUND", "Canary does not exist"))?;
    let now = s.now().await;
    if utc(&canary.send_wall_utc).map_err(|_| s.unavailable())?
        > utc(&now).map_err(|_| s.unavailable())?
    {
        return Err(s.invalid());
    }
    let observations = match canary.signature {
        Some(sig) => s
            .store
            .workbench_observations(s.source, &sig, &now)
            .await
            .map_err(|_| s.unavailable())?,
        None => vec![],
    };
    Ok(Json(CanaryEvidencePage {
        source: s.source,
        canary_id: id,
        observations,
    }))
}
async fn prove_canaries(
    State(s): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<ProveCanaryPage>, ApiError> {
    if !short(&id, 100) {
        return Err(s.invalid());
    }
    s.store
        .prove(s.source, &id)
        .await
        .map_err(|_| s.unavailable())?
        .ok_or_else(|| {
            s.error(
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
                "Prove run does not exist",
            )
        })?;
    let canaries = s
        .store
        .prove_canaries(s.source, &id)
        .await
        .map_err(|_| s.unavailable())?;
    if canaries.len() > 400 {
        return Err(s.unavailable());
    }
    Ok(Json(ProveCanaryPage {
        source: s.source,
        id,
        canaries,
    }))
}
async fn browser_ledger(
    State(s): State<ApiState>,
    query: Result<Query<LedgerQuery>, QueryRejection>,
) -> Result<Json<BrowserLedgerPage>, ApiError> {
    let Json(page) = ledger(State(s), query).await?;
    let rows = page
        .entries
        .into_iter()
        .map(|entry| {
            Ok(BrowserLedgerRow {
                canonical_json: alight_store::canonical(&entry.forecast).map_err(|_| ApiError {
                    source: page.source,
                    status: StatusCode::SERVICE_UNAVAILABLE,
                    code: "DATA_UNAVAILABLE",
                    message: "Stored evidence is unavailable",
                })?,
                entry,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    Ok(Json(BrowserLedgerPage {
        source: page.source,
        rows,
        next_after: page.next_after,
    }))
}
async fn observer_views(s: &ApiState) -> Result<Vec<ObserverHealthView>, ApiError> {
    let now = utc(&s.now().await).map_err(|_| s.unavailable())?;
    let mut views = Vec::new();
    for observer in [
        ObserverKind::Grpc,
        ObserverKind::Mirage,
        ObserverKind::Rpc,
        ObserverKind::Webhook,
    ] {
        if observer == ObserverKind::Mirage && !s.mirage {
            continue;
        }
        if observer == ObserverKind::Webhook && s.webhook_secret.is_none() {
            continue;
        }
        let value = s
            .store
            .observer_health(observer, s.source)
            .await
            .map_err(|_| s.unavailable())?;
        let last = value["last_receive_utc"].as_str().map(str::to_owned);
        let age = last
            .as_deref()
            .and_then(|t| utc(t).ok())
            .and_then(|t| u64::try_from((now - t).num_milliseconds()).ok());
        let gaps = value["open_gaps"].as_u64().ok_or_else(|| s.unavailable())?;
        let cursor = value["cursor_slot"]
            .as_str()
            .map(str::parse)
            .transpose()
            .map_err(|_| s.unavailable())?;
        views.push(ObserverHealthView {
            source: s.source,
            observer,
            status: if age.is_some_and(|a| a < 30000) && gaps == 0 {
                "PASS"
            } else {
                "STALE"
            }
            .into(),
            last_receive_utc: last,
            age_ms: age,
            cursor_slot: cursor,
            open_gaps: gaps,
        });
    }
    Ok(views)
}
async fn health_view(s: &ApiState) -> Result<ApiHealth, ApiError> {
    let now = s.now().await;
    let counts = s
        .store
        .counts(s.source)
        .await
        .map_err(|_| s.unavailable())?;
    let n = |v| u64::try_from(v).map_err(|_| s.unavailable());
    let counts = HealthCounts {
        slot_events: n(counts.slot_events)?,
        blocks: n(counts.blocks)?,
        observations: n(counts.observations)?,
        gaps: n(counts.gaps)?,
        open_gaps: n(counts.open_gaps)?,
        canaries: n(counts.canaries)?,
        budget_reserved_lamports: n(counts.budget_reserved_lamports)?,
    };
    let observers = observer_views(s).await?;
    let ready = observers
        .iter()
        .filter(|o| matches!(o.observer, ObserverKind::Grpc | ObserverKind::Mirage))
        .all(|o| o.status == "PASS");
    let status = match s.source {
        Source::Sim => "SIMULATED",
        Source::Replay => "REPLAY",
        Source::Live if ready => "PASS",
        _ => "DEGRADED",
    }
    .to_owned();
    let sends = s
        .store
        .send_summary(s.source)
        .await
        .map_err(|_| s.unavailable())?;
    let sends: BTreeMap<String, u32> =
        serde_json::from_value(sends).map_err(|_| s.unavailable())?;
    let budgets = s
        .store
        .budget_by_route(
            s.source,
            &utc(&now)
                .map_err(|_| s.unavailable())?
                .format("%Y-%m-%d")
                .to_string(),
        )
        .await
        .map_err(|_| s.unavailable())?;
    Ok(ApiHealth {
        schema_version: 1,
        source: s.source,
        mode: s.mode,
        run_id: s.run_id.clone(),
        as_of_utc: now,
        region: s.region.clone(),
        status: status.clone(),
        collector_status: status,
        signing_enabled: s.mode == RunMode::Live,
        uptime_s: s.started.elapsed().as_secs(),
        canaries_sent: u64::from(sends.get("ACCEPTED").copied().unwrap_or(0)),
        counts,
        observers,
        send_attempts: sends,
        budget_reserved_today_by_route: serde_json::from_value(budgets)
            .map_err(|_| s.unavailable())?,
        canary_engine: s.engine.read().await.clone(),
        leaders: s.leaders.read().await.clone(),
    })
}
async fn health(State(s): State<ApiState>) -> Result<Json<ApiHealth>, ApiError> {
    Ok(Json(health_view(&s).await?))
}
async fn clock_view(s: &ApiState) -> Result<ApiClock, ApiError> {
    let clock = s.clock.read().await;
    let window = serde_json::from_value(clock.summary()).map_err(|_| s.unavailable())?;
    Ok(ApiClock {
        source: s.source,
        method: if s.source == Source::Sim {
            "seeded simulator slot duration"
        } else {
            "gRPC candidate-block Unix seconds over rolling slot distance"
        }
        .into(),
        mean_slot_ms: if s.source == Source::Sim {
            s.simulation.as_ref().map(|v| f64::from(v.slot_ms))
        } else {
            clock.mean_slot_ms()
        },
        window,
        minimum_slot_distance: 64,
        minimum_chain_seconds: 10,
    })
}
async fn clock(State(s): State<ApiState>) -> Result<Json<ApiClock>, ApiError> {
    Ok(Json(clock_view(&s).await?))
}
async fn leaders(State(s): State<ApiState>) -> Json<ApiTelemetry> {
    Json(ApiTelemetry {
        source: s.source,
        value: s.leaders.read().await.clone(),
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuoteQuery {
    request: String,
}
async fn validate_quote(
    s: &ApiState,
    r: &QuoteServiceRequest,
    write: bool,
) -> Result<(), ApiError> {
    let m = &r.model;
    if m.context.source != s.source
        || !short(&m.context.regime_id, 128)
        || m.context.region != s.region
        || !(1..=86400).contains(&r.ttl_s)
        || m.candidates.is_empty()
        || m.candidates.len() > 81
        || m.leader_class_next.len() > 16
        || m.leader_class_next.iter().any(|l| !short(&l.leader, 128))
        || r.frozen_model_hash.as_ref().is_some_and(|h| !short(h, 100))
        || m.candidates.iter().any(|c| {
            c.size_class != m.candidates[0].size_class
                || c.cu_limit != alight_canary::policy::cu_limit(c.size_class)
                || c.route == Route::Rpc && c.tip_lamports != 0
                || c.route != Route::Rpc && c.tip_lamports < 100000
        })
    {
        return Err(s.invalid());
    }
    let now = s.now().await;
    let now_time = utc(&now).map_err(|_| s.unavailable())?;
    let request_time = utc(&m.context.as_of_utc).map_err(|_| s.invalid())?;
    if request_time > now_time {
        return Err(s.invalid());
    }
    let age = (now_time - request_time).num_milliseconds();
    if let Some(e) = &r.economics
        && (!short(&e.pool, 128)
            || e.size_usd.len() > 64
            || e.sol_usd.len() > 64
            || e.size_usd
                .parse::<f64>()
                .ok()
                .is_none_or(|n| !n.is_finite() || n <= 0.0)
            || e.sol_usd
                .parse::<f64>()
                .ok()
                .is_none_or(|n| !n.is_finite() || n <= 0.0)
            || !e.edge_bps.is_finite()
            || e.edge_bps < 0.0
            || !e.lambda.is_finite()
            || e.lambda < 0.0)
    {
        return Err(s.invalid());
    }
    if write || s.source != Source::Replay {
        let context = alight_forecast::current_context(&s.store, s.source, &s.region, &now)
            .await
            .map_err(|_| s.unavailable())?;
        if age > 30000 || context.regime_id != m.context.regime_id {
            return Err(s.error(
                StatusCode::CONFLICT,
                "QUOTE_CONTEXT_CHANGED",
                "Refresh the quote timestamp and current regime",
            ));
        }
    }
    Ok(())
}
async fn preview(
    State(s): State<ApiState>,
    query: Result<Query<QuoteQuery>, QueryRejection>,
) -> Result<Json<QuotePreview>, ApiError> {
    let Query(query) = query.map_err(|_| s.invalid())?;
    let request = serde_json::from_str(&query.request).map_err(|_| s.invalid())?;
    validate_quote(&s, &request, false).await?;
    let _permit = s.work()?;
    Ok(Json(
        alight_forecast::preview_combined(&s.store, request)
            .await
            .map_err(|e| match e {
                StoreError::Invalid => s.invalid(),
                _ => s.unavailable(),
            })?,
    ))
}
async fn freeze(
    State(s): State<ApiState>,
    body: Result<Json<QuoteServiceRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ForecastEntry>), ApiError> {
    let Json(request) = body.map_err(|_| s.invalid())?;
    let _control = s.controls.try_lock().map_err(|_| s.busy())?;
    validate_quote(&s, &request, true).await?;
    let _permit = s.work()?;
    let entry = alight_forecast::issue_combined(&s.store, request)
        .await
        .map_err(|e| match e {
            StoreError::Invalid => s.invalid(),
            _ => s.unavailable(),
        })?;
    Ok((StatusCode::CREATED, Json(entry)))
}
async fn prove(
    State(s): State<ApiState>,
    body: Result<Json<ProveRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ProveReport>), ApiError> {
    let Json(request) = body.map_err(|_| s.invalid())?;
    if !matches!(s.mode, RunMode::Live | RunMode::Sim) {
        return Err(s.error(
            StatusCode::FORBIDDEN,
            "MODE_READ_ONLY",
            "This mode cannot start Prove",
        ));
    }
    let _control = s.controls.try_lock().map_err(|_| s.busy())?;
    let _permit = s.work()?;
    let locked = alight_prove::lock(
        &s.store,
        s.mode,
        &request,
        &s.now().await,
        s.simulation.clone(),
    )
    .await
    .map_err(|e| match e {
        alight_prove::ProveError::Conflict => s.error(
            StatusCode::CONFLICT,
            "PROVE_CONFLICT",
            "Forecast expired or changed, or cell is locked",
        ),
        alight_prove::ProveError::Invalid => s.invalid(),
        _ => s.unavailable(),
    })?;
    let report = if s.mode == RunMode::Sim {
        let report =
            alight_prove::run_sim(&s.store, &locked, s.daily.as_deref(), s.burst.as_deref())
                .await
                .map_err(|_| s.unavailable())?;
        s.advance(&report.as_of_utc).await?;
        // Sim has no periodic live model worker. Grade the later held-out outcomes here;
        // model fitting still excludes every Prove member through the store contract.
        alight_forecast::tick(&s.store, s.source, &s.region, &report.as_of_utc)
            .await
            .map_err(|_| s.unavailable())?;
        report
    } else {
        locked
    };
    let status = if s.mode == RunMode::Live
        && !matches!(report.state, ProveState::Complete | ProveState::Voided)
    {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(report)))
}
async fn prove_report(
    State(s): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<ProveReport>, ApiError> {
    if !short(&id, 100) {
        return Err(s.invalid());
    }
    Ok(Json(
        s.store
            .prove(s.source, &id)
            .await
            .map_err(|_| s.unavailable())?
            .ok_or_else(|| {
                s.error(
                    StatusCode::NOT_FOUND,
                    "NOT_FOUND",
                    "Prove run does not exist",
                )
            })?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitQuery {
    limit: Option<u32>,
}
async fn proves(
    State(s): State<ApiState>,
    query: Result<Query<LimitQuery>, QueryRejection>,
) -> Result<Json<ProvePage>, ApiError> {
    let Query(q) = query.map_err(|_| s.invalid())?;
    let limit = q.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err(s.invalid());
    }
    Ok(Json(ProvePage {
        source: s.source,
        reports: s
            .store
            .proves(s.source, limit, false)
            .await
            .map_err(|_| s.unavailable())?,
    }))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CurveQuery {
    regime: Option<String>,
    limit: Option<u32>,
}
async fn curve(
    State(s): State<ApiState>,
    query: Result<Query<CurveQuery>, QueryRejection>,
) -> Result<Json<CurvePage>, ApiError> {
    let Query(q) = query.map_err(|_| s.invalid())?;
    let limit = q.limit.unwrap_or(243);
    if !(1..=1000).contains(&limit) || q.regime.as_ref().is_some_and(|r| !short(r, 128)) {
        return Err(s.invalid());
    }
    let now = s.now().await;
    let now_time = utc(&now).map_err(|_| s.unavailable())?;
    let regime = match q.regime {
        Some(r) => r,
        None => {
            alight_forecast::current_context(&s.store, s.source, &s.region, &now)
                .await
                .map_err(|_| s.unavailable())?
                .regime_id
        }
    };
    let curves = s
        .store
        .curve_snapshots(s.source, &regime, limit)
        .await
        .map_err(|_| s.unavailable())?
        .into_iter()
        .filter(|c| utc(&c.context.as_of_utc).is_ok_and(|t| t <= now_time))
        .collect();
    Ok(Json(CurvePage {
        source: s.source,
        regime_id: regime,
        curves,
    }))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LedgerQuery {
    after: Option<String>,
    limit: Option<u32>,
}
async fn ledger(
    State(s): State<ApiState>,
    query: Result<Query<LedgerQuery>, QueryRejection>,
) -> Result<Json<LedgerPage>, ApiError> {
    let Query(q) = query.map_err(|_| s.invalid())?;
    let limit = q.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(s.invalid());
    }
    let after = q
        .after
        .as_deref()
        .unwrap_or("0")
        .parse()
        .map_err(|_| s.invalid())?;
    let entries = s
        .store
        .forecast_page(s.source, after, limit)
        .await
        .map_err(|_| s.unavailable())?;
    let next_after = (entries.len() == limit as usize)
        .then(|| entries.last().map(|e| e.sequence))
        .flatten();
    Ok(Json(LedgerPage {
        source: s.source,
        entries,
        next_after,
    }))
}
async fn verify(State(s): State<ApiState>) -> Result<Json<LedgerVerification>, ApiError> {
    let _permit = s.work()?;
    let entries = s.store.verify_ledger(s.source).await.map_err(|e| match e {
        StoreError::Invalid => s.error(
            StatusCode::CONFLICT,
            "LEDGER_VERIFICATION_FAILED",
            "Ledger or referenced evidence failed verification",
        ),
        _ => s.unavailable(),
    })?;
    Ok(Json(LedgerVerification {
        source: s.source,
        verified: true,
        entries,
    }))
}
async fn anchor_draft(State(s): State<ApiState>) -> Result<Json<AnchorDraft>, ApiError> {
    let _permit = s.work()?;
    Ok(Json(
        alight_canary::anchor::prepare(&s.store, s.source)
            .await
            .map_err(|e| match e {
                StoreError::Invalid => s.error(
                    StatusCode::CONFLICT,
                    "ANCHOR_UNAVAILABLE",
                    "A verified nonempty ledger is required for an unsigned anchor draft",
                ),
                _ => s.unavailable(),
            })?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TapeQuery {
    from: Option<String>,
    through: Option<String>,
    limit: Option<u32>,
}
async fn tape(
    State(s): State<ApiState>,
    query: Result<Query<TapeQuery>, QueryRejection>,
) -> Result<Json<TapePage>, ApiError> {
    let Query(q) = query.map_err(|_| s.invalid())?;
    let limit = q.limit.unwrap_or(100);
    if !(1..=1000).contains(&limit) {
        return Err(s.invalid());
    }
    let now = s.now().await;
    let now_time = utc(&now).map_err(|_| s.unavailable())?;
    let through = q.through.unwrap_or(now);
    let end = utc(&through).map_err(|_| s.invalid())?;
    let from = q
        .from
        .unwrap_or_else(|| (end - chrono::Duration::minutes(5)).to_rfc3339());
    let start = utc(&from).map_err(|_| s.invalid())?;
    if start > end || end > now_time || (end - start) > chrono::Duration::days(1) {
        return Err(s.invalid());
    }
    let rows = s
        .store
        .passive_tips(s.source, &from, &through, limit)
        .await
        .map_err(|_| s.unavailable())?;
    Ok(Json(TapePage {
        source: s.source,
        from,
        through,
        rows,
    }))
}
async fn stream(State(s): State<ApiState>, ws: WebSocketUpgrade) -> Result<Response, ApiError> {
    let permit = s
        .streams
        .clone()
        .try_acquire_owned()
        .map_err(|_| s.busy())?;
    Ok(ws
        .max_message_size(4096)
        .max_frame_size(4096)
        .read_buffer_size(4096)
        .write_buffer_size(0)
        .max_write_buffer_size(STREAM_BYTES + 4096)
        .on_upgrade(move |socket| stream_socket(socket, s, permit)))
}
async fn stream_snapshot(s: &ApiState) -> Result<ApiStreamSnapshot, ApiError> {
    Ok(ApiStreamSnapshot {
        kind: "SNAPSHOT".into(),
        source: s.source,
        as_of_utc: s.now().await,
        health: health_view(s).await?,
        clock: clock_view(s).await?,
        proves: s
            .store
            .proves(s.source, 8, false)
            .await
            .map_err(|_| s.unavailable())?,
        ledger: s
            .store
            .forecast_tail(s.source, 1)
            .await
            .map_err(|_| s.unavailable())?,
    })
}
async fn stream_socket(mut socket: WebSocket, state: ApiState, _permit: OwnedSemaphorePermit) {
    let mut timer = tokio::time::interval(Duration::from_secs(1));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut input_window = (Instant::now(), 0u32);
    loop {
        tokio::select! {
            message=socket.recv()=>{
                if input_window.0.elapsed()>=Duration::from_secs(1) {input_window=(Instant::now(),0);}
                input_window.1+=1;
                if input_window.1>10 || matches!(message,Some(Ok(Message::Text(_)|Message::Binary(_)))) {
                    let _=tokio::time::timeout(Duration::from_secs(2),socket.send(Message::Close(Some(CloseFrame{code:1008,reason:"Read-only snapshot stream".into()})))).await;return;
                }
                if !matches!(message,Some(Ok(Message::Ping(_)|Message::Pong(_)))) {return;}
            },
            _=timer.tick()=>{
                let snapshot=match stream_snapshot(&state).await {Ok(v)=>v,Err(_)=>return};
                let bytes=match serde_json::to_string(&snapshot) {Ok(b) if b.len()<=STREAM_BYTES=>b,_=>return};
                if !matches!(tokio::time::timeout(Duration::from_secs(2),socket.send(Message::Text(bytes.into()))).await,Ok(Ok(()))) {return;}
            }
        }
    }
}
