use alight_api::{ApiState, Options, router};
use alight_store::Store;
use alight_types::*;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{path::Path, process::Command, time::Duration};
use tower::ServiceExt;

const NOW: &str = "2026-10-05T01:00:00Z";
const KEY: &str = "local-test-operator-key-42-only";
fn config() -> CanaryConfig {
    CanaryConfig {
        route: Route::BeamHttp,
        tip_lamports: 200000,
        cu_price_micro_lamports: 1001,
        cu_limit: 25000,
        fee_bucket: FeeBucket::LocalMedian,
        tip_tier: TipTier::X2,
        size_class: SizeClass::Small,
    }
}
fn environment() -> SimProofEnvironment {
    SimProofEnvironment {
        slot_ms: 250,
        congestion: 0.0,
        tip_slope: 0.45,
        fee_slope: 0.18,
        never_land_mass: None,
        continuous_latency: false,
    }
}
fn request() -> QuoteServiceRequest {
    QuoteServiceRequest {
        model: ModelQuoteRequest {
            context: CurveContext {
                source: Source::Sim,
                regime_id: "sim-r0".into(),
                region: "local".into(),
                as_of_utc: NOW.into(),
            },
            candidates: vec![config()],
            covariates: ModelCovariates::default(),
            leader_class_next: vec![],
            target: PredictionTarget::Probability {
                target_p: 0.7,
                horizon_slots: 2,
            },
        },
        ttl_s: 60,
        economics: None,
        frozen_model_hash: None,
    }
}
async fn setup(path: &Path) -> Store {
    let store = Store::open(path, 16 * 1024 * 1024).await.expect("store");
    let base = alight_sim::generate(&alight_sim::Parameters {
        canaries: 1,
        ..Default::default()
    })
    .expect("sim")
    .canaries
    .remove(0);
    for i in 0..100 {
        let mut c = base.clone();
        c.id = format!("training-{i}");
        c.config = config();
        c.send_wall_utc = "2026-10-05T00:59:59Z".into();
        c.resolved_at_utc = Some("2026-10-05T00:59:59.500Z".into());
        c.outcome = Some(if i < 90 {
            Outcome::LandedOk
        } else {
            Outcome::Expired
        });
        c.landed_slot = (i < 90).then_some(c.sent_slot + 1);
        store
            .import_training(&TrainingCanary {
                canary: c,
                finalized: true,
                covariates: ModelCovariates::default(),
            })
            .await
            .expect("import");
    }
    alight_forecast::tick(&store, Source::Sim, "local", NOW)
        .await
        .expect("tick");
    store
}
fn state(store: Store) -> ApiState {
    ApiState::new(
        store,
        Options {
            mode: RunMode::Sim,
            operator_key: Some(KEY.into()),
            simulated_as_of_utc: Some(NOW.into()),
            simulation: Some(environment()),
            requests_per_second: 200,
            ..Default::default()
        },
    )
    .expect("state")
}
async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    key: Option<&str>,
) -> (u16, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(key) = key {
        request = request.header("authorization", format!("Bearer {key}"));
    }
    let response = app
        .clone()
        .oneshot(
            request
                .body(if method == "GET" {
                    Body::empty()
                } else {
                    Body::from(serde_json::to_vec(&body).expect("json"))
                })
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .expect("bounded response");
    let json = serde_json::from_slice(&bytes).expect("JSON response");
    (status, json)
}
fn capture(
    cases: &mut Vec<Value>,
    spec: &Value,
    path: &str,
    method: &str,
    status: u16,
    payload: Value,
) {
    let schema_ref =
        spec["paths"][path][method]["responses"][status.to_string()]["content"]["application/json"]
            ["schema"]["$ref"]
            .as_str()
            .expect("declared response");
    cases.push(json!({"label":format!("{method} {path} {status}"),"schema_ref":schema_ref,"payload":payload}));
}
fn validate(dir: &Path, spec: Value, cases: Vec<Value>) {
    let bundle = dir.join("contracts.json");
    std::fs::write(
        &bundle,
        serde_json::to_vec(&json!({"openapi":spec,"cases":cases})).expect("bundle"),
    )
    .expect("write");
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/verify_api_contracts.py");
    let result = Command::new("python3")
        .arg(script)
        .arg(bundle)
        .output()
        .expect("Python schema validator");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[tokio::test]
async fn public_preview_operator_freeze_forty_prove_restart_and_served_schemas_work_end_to_end() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("api.db");
    let store = setup(&path).await;
    let app = router(state(store.clone()));
    let (status, spec) = call(&app, "GET", "/v1/openapi.json", Value::Null, None).await;
    assert_eq!(status, 200);
    assert!(!spec.to_string().contains(KEY));
    let mut cases = Vec::new();
    for path in [
        "/v1/health",
        "/v1/clock",
        "/v1/leaders",
        "/v1/curve",
        "/v1/observers",
        "/v1/ledger",
        "/v1/ledger/verify",
        "/v1/tape",
        "/v1/proves",
        "/v1/workbench",
        "/v1/ledger/payloads",
    ] {
        let (status, value) = call(&app, "GET", path, Value::Null, None).await;
        assert_eq!(status, 200, "{path}");
        assert_eq!(value["source"], "sim");
        capture(&mut cases, &spec, path, "get", status, value);
    }
    let query = serde_urlencoded::to_string([(
        "request",
        serde_json::to_string(&request()).expect("request"),
    )])
    .expect("query");
    let (status, preview) = call(
        &app,
        "GET",
        &format!("/v1/quote?{query}"),
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, 200);
    capture(
        &mut cases,
        &spec,
        "/v1/quote",
        "get",
        status,
        preview.clone(),
    );
    assert_eq!(
        store
            .verify_ledger(Source::Sim)
            .await
            .expect("read-only ledger"),
        0
    );
    let (status, unauthorized) = call(&app, "POST", "/v1/quote", json!(request()), None).await;
    assert_eq!(status, 401);
    capture(&mut cases, &spec, "/v1/quote", "post", status, unauthorized);
    let (status, frozen) = call(&app, "POST", "/v1/quote", json!(request()), Some(KEY)).await;
    assert_eq!(status, 201);
    capture(
        &mut cases,
        &spec,
        "/v1/quote",
        "post",
        status,
        frozen.clone(),
    );
    assert_eq!(preview["quote"], frozen["forecast"]["quote"]);
    let prove =
        json!({"request_id":"http-forty","forecast_hash":frozen["hash"],"n":40,"seed":"42"});
    cases.push(json!({"label":"ProveRequest","schema_ref":"#/components/schemas/ProveRequest","payload":prove}));
    let (status, report) = call(&app, "POST", "/v1/prove", prove.clone(), Some(KEY)).await;
    assert_eq!(status, 200);
    assert_eq!(report["attempts"], 40);
    assert_eq!(report["resolved"], 40);
    assert_eq!(report["state"], "COMPLETE");
    capture(
        &mut cases,
        &spec,
        "/v1/prove",
        "post",
        status,
        report.clone(),
    );
    let id = report["lock"]["id"].as_str().expect("id");
    let (status, read) = call(&app, "GET", &format!("/v1/prove/{id}"), Value::Null, None).await;
    assert_eq!(status, 200);
    assert_eq!(read, report);
    capture(&mut cases, &spec, "/v1/prove/{id}", "get", status, read);
    let (status, members) = call(
        &app,
        "GET",
        &format!("/v1/prove/{id}/canaries"),
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(members["canaries"].as_array().expect("members").len(), 40);
    capture(
        &mut cases,
        &spec,
        "/v1/prove/{id}/canaries",
        "get",
        status,
        members,
    );
    let (status, payloads) = call(&app, "GET", "/v1/ledger/payloads", Value::Null, None).await;
    assert_eq!(status, 200);
    let payload: Value = serde_json::from_str(
        payloads["rows"][0]["canonical_json"]
            .as_str()
            .expect("canonical bytes"),
    )
    .expect("JSON");
    assert_eq!(payload, frozen["forecast"]);
    capture(
        &mut cases,
        &spec,
        "/v1/ledger/payloads",
        "get",
        status,
        payloads,
    );
    assert_eq!(
        store
            .training_canaries(Source::Sim)
            .await
            .expect("training")
            .len(),
        100
    );
    assert_eq!(
        store
            .counts(Source::Sim)
            .await
            .expect("budget")
            .budget_reserved_lamports,
        8201040
    );
    let (status, verified) = call(&app, "GET", "/v1/ledger/verify", Value::Null, None).await;
    assert_eq!(status, 200);
    assert_eq!(verified["entries"], "1");
    capture(
        &mut cases,
        &spec,
        "/v1/ledger/verify",
        "get",
        status,
        verified,
    );
    assert!(
        store
            .forecast_tail(Source::Live, 1)
            .await
            .expect("source scope")
            .is_empty()
    );
    assert_eq!(
        store
            .forecast_tail(Source::Sim, 1)
            .await
            .expect("tail")
            .len(),
        1
    );
    let mut bad = json!(request());
    bad["model"]["candidates"][0]["tip_lamports"] = json!(200000);
    cases.push(json!({"label":"lossy number rejected","schema_ref":"#/components/schemas/QuoteServiceRequest","payload":bad,"valid":false}));
    let mut overflow = json!(request());
    overflow["model"]["candidates"][0]["tip_lamports"] = json!("18446744073709551616");
    cases.push(json!({"label":"u64 overflow rejected","schema_ref":"#/components/schemas/QuoteServiceRequest","payload":overflow,"valid":false}));
    let mut upper = json!(request());
    upper["model"]["candidates"][0]["tip_lamports"] = json!("18446744073709551615");
    cases.push(json!({"label":"full-width u64 schema","schema_ref":"#/components/schemas/QuoteServiceRequest","payload":upper}));
    validate(dir.path(), spec, cases);
    drop(app);
    store.close().await;
    let reopened = Store::open(&path, 16 * 1024 * 1024).await.expect("restart");
    let floor = reopened
        .experiment_clock_floor(Source::Sim)
        .await
        .expect("floor")
        .expect("persisted");
    assert_eq!(floor, report["as_of_utc"]);
    let mut api = state(reopened.clone());
    api.clock = std::sync::Arc::new(tokio::sync::RwLock::new(
        alight_ingest::clock::SlotClock::default(),
    ));
    let (status, retry) = call(&router(api), "POST", "/v1/prove", prove, Some(KEY)).await;
    assert_eq!(status, 200);
    assert_eq!(retry, report);
    assert_eq!(
        reopened
            .counts(Source::Sim)
            .await
            .expect("no duplicate spend")
            .budget_reserved_lamports,
        8201040
    );
}

#[tokio::test]
async fn workbench_reads_are_bounded_source_scoped_and_do_not_expose_future_records() {
    let dir = tempfile::tempdir().expect("temporary database");
    let store = setup(&dir.path().join("workbench.db")).await;
    let base = store
        .grading_canaries(Source::Sim)
        .await
        .expect("rows")
        .remove(0);
    let mut future = base.clone();
    future.canary.id = "future-row".into();
    future.canary.send_wall_utc = "2026-10-06T01:00:00Z".into();
    store
        .import_training(&future)
        .await
        .expect("future fixture");
    let mut replay = base;
    replay.canary.id = "other-source-row".into();
    replay.canary.source = Source::Replay;
    store
        .import_training(&replay)
        .await
        .expect("other source fixture");
    let app = router(state(store.clone()));
    let (status, evidence) = call(&app, "GET", "/v1/workbench", Value::Null, None).await;
    assert_eq!(status, 200);
    let evidence: WorkbenchEvidence = serde_json::from_value(evidence).expect("contract");
    assert_eq!(evidence.canaries.len(), 100);
    assert!(
        evidence
            .canaries
            .iter()
            .all(|r| r.canary.source == Source::Sim && r.canary.id != "future-row")
    );
    assert_eq!(evidence.regimes[0].canaries, 100);
    assert_eq!(evidence.daily_cap_lamports, 200_000_000);
    for path in [
        "/v1/canaries/other-source-row/observations",
        "/v1/prove/other-source-run/canaries",
    ] {
        assert_eq!(call(&app, "GET", path, Value::Null, None).await.0, 404);
    }
    assert_eq!(
        call(
            &app,
            "GET",
            "/v1/canaries/future-row/observations",
            Value::Null,
            None
        )
        .await
        .0,
        400
    );
    assert_eq!(
        store.verify_ledger(Source::Sim).await.expect("read only"),
        0
    );
}

#[tokio::test]
async fn authentication_source_bounds_read_only_modes_and_global_rate_caps_reject_without_mutation()
{
    let dir = tempfile::tempdir().expect("dir");
    let store = setup(&dir.path().join("guards.db")).await;
    let app = router(state(store.clone()));
    let (status, value) = call(
        &app,
        "POST",
        "/v1/quote",
        json!(request()),
        Some("wrong-key-secret-marker"),
    )
    .await;
    assert_eq!(status, 401);
    assert!(!value.to_string().contains("secret-marker"));
    let mut wrong = request();
    wrong.model.context.source = Source::Live;
    let q =
        serde_urlencoded::to_string([("request", serde_json::to_string(&wrong).expect("JSON"))])
            .expect("query");
    assert_eq!(
        call(&app, "GET", &format!("/v1/quote?{q}"), Value::Null, None)
            .await
            .0,
        400
    );
    assert_eq!(
        call(&app, "GET", "/v1/ledger?source=live", Value::Null, None)
            .await
            .0,
        400
    );
    assert_eq!(
        call(&app, "GET", "/v1/tape?limit=1001", Value::Null, None)
            .await
            .0,
        400
    );
    assert_eq!(
        call(&app, "GET", "/v1/curve?limit=1001", Value::Null, None)
            .await
            .0,
        400
    );
    assert_eq!(
        call(&app, "GET", "/v1/quote?request=invalid", Value::Null, None)
            .await
            .0,
        400
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/v1/quote?request={}", "x".repeat(8193)),
            Value::Null,
            None
        )
        .await
        .0,
        414
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/quote",
            json!({"padding":"x".repeat(65536)}),
            Some(KEY)
        )
        .await
        .0,
        413
    );
    let mut decimals = json!(request());
    decimals["model"]["candidates"][0]["tip_lamports"] = json!("0200000");
    assert_eq!(
        call(&app, "POST", "/v1/quote", decimals, Some(KEY)).await.0,
        400
    );
    assert_eq!(
        call(&app, "POST", "/v1/quote", json!(request()), Some(KEY))
            .await
            .0,
        429
    );
    let readonly = ApiState::new(
        store.clone(),
        Options {
            operator_key: Some(KEY.into()),
            ..Default::default()
        },
    )
    .expect("observe");
    assert_eq!(
        call(
            &router(readonly),
            "POST",
            "/v1/prove",
            json!({"request_id":"observe","forecast_hash":"unknown","n":40}),
            Some(KEY)
        )
        .await
        .0,
        403
    );
    let disabled = ApiState::new(store.clone(), Options::default()).expect("disabled");
    assert_eq!(
        call(
            &router(disabled),
            "POST",
            "/v1/quote",
            json!(request()),
            Some(KEY)
        )
        .await
        .0,
        403
    );
    let limited = ApiState::new(
        store.clone(),
        Options {
            requests_per_second: 1,
            ..Default::default()
        },
    )
    .expect("limited");
    let limited = router(limited);
    assert_eq!(
        call(&limited, "GET", "/v1/health", Value::Null, None)
            .await
            .0,
        200
    );
    assert_eq!(
        call(&limited, "GET", "/v1/clock", Value::Null, None)
            .await
            .0,
        429
    );
    assert_eq!(
        store.verify_ledger(Source::Sim).await.expect("no writes"),
        0
    );
    assert_eq!(
        store
            .counts(Source::Sim)
            .await
            .expect("no spend")
            .budget_reserved_lamports,
        0
    );
}

#[tokio::test]
async fn websocket_stream_is_schema_valid_bounded_and_cannot_accept_operator_controls() {
    let dir = tempfile::tempdir().expect("dir");
    let store = setup(&dir.path().join("ws.db")).await;
    let api = ApiState::new(
        store.clone(),
        Options {
            mode: RunMode::Sim,
            operator_key: Some(KEY.into()),
            simulated_as_of_utc: Some(NOW.into()),
            simulation: Some(environment()),
            max_streams: 1,
            ..Default::default()
        },
    )
    .expect("state");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        axum::serve(listener, router(api)).await.expect("server");
    });
    let spec: Value = reqwest::Client::new()
        .get(format!("http://{address}/v1/openapi.json"))
        .send()
        .await
        .expect("loopback HTTP")
        .json()
        .await
        .expect("spec");
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/v1/stream"))
        .await
        .expect("loopback WS");
    let message = tokio::time::timeout(Duration::from_secs(3), socket.next())
        .await
        .expect("frame deadline")
        .expect("frame")
        .expect("message");
    let text = message.into_text().expect("text");
    assert!(text.len() <= 1048576);
    let snapshot: Value = serde_json::from_str(&text).expect("snapshot");
    assert_eq!(snapshot["source"], "sim");
    assert!(!text.contains(KEY));
    let denied = tokio_tungstenite::connect_async(format!("ws://{address}/v1/stream"))
        .await
        .expect_err("stream cap");
    assert!(
        matches!(denied,tokio_tungstenite::tungstenite::Error::Http(r) if r.status().as_u16()==429)
    );
    let schema_ref = spec["paths"]["/v1/stream"]["get"]["x-websocket-message"]["$ref"].clone();
    validate(
        dir.path(),
        spec,
        vec![json!({"label":"actual WebSocket frame","schema_ref":schema_ref,"payload":snapshot})],
    );
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            "{\"operator\":\"prove\",\"n\":40}".into(),
        ))
        .await
        .expect("control attempt");
    let frame = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match socket.next().await.expect("close").expect("message") {
                tokio_tungstenite::tungstenite::Message::Close(Some(frame)) => break frame,
                tokio_tungstenite::tungstenite::Message::Text(text) => {
                    // A periodic frame queued while the schema validator ran may precede close.
                    let value: Value = serde_json::from_str(&text).expect("snapshot");
                    assert_eq!(value["kind"], "SNAPSHOT");
                }
                _ => panic!("unexpected server frame"),
            }
        }
    })
    .await
    .expect("close deadline");
    assert_eq!(u16::from(frame.code), 1008);
    assert_eq!(
        store.verify_ledger(Source::Sim).await.expect("no writes"),
        0
    );
    assert_eq!(
        store
            .counts(Source::Sim)
            .await
            .expect("no spend")
            .budget_reserved_lamports,
        0
    );
    server.abort();
}
