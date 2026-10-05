use super::*;
use axum::{Json, Router, routing::post};
use std::sync::{Arc, Mutex};
fn curve(regime: &str, now: &str, p: f64, interval: [f64; 2]) -> CurveSnapshot {
    CurveSnapshot {
        contract_version: 1,
        estimator: "M0".into(),
        methodology_hash: format!("sha256:{}", "0".repeat(64)),
        context: CurveContext {
            source: Source::Sim,
            regime_id: regime.into(),
            region: "local".into(),
            as_of_utc: now.into(),
        },
        config: CanaryConfig {
            route: Route::BeamHttp,
            tip_lamports: 200000,
            cu_price_micro_lamports: 1001,
            cu_limit: 25000,
            fee_bucket: FeeBucket::LocalMedian,
            tip_tier: TipTier::X2,
            size_class: SizeClass::Small,
        },
        horizon_slots: 2,
        half_life_s: 1800.0,
        alpha: 90.0,
        beta: 10.0,
        p_hat: p,
        p_interval_95: interval,
        n_effective: 100.0,
        assignments: 100,
        resolved: 100,
        unresolved: 0,
        unresolved_share: 0.0,
        data_age_s: Some(1.0),
        observation_window: Some(["2026-10-05T00:00:00Z".into(), now.into()]),
        evidence: Evidence::Measured,
        samples_needed: None,
        insufficient_reasons: vec![],
    }
}
fn fixture() -> (AlertSnapshot, AlertSnapshot) {
    let good = curve("r0", "2026-10-05T00:01:00Z", 0.9, [0.85, 0.95]);
    let initial = AlertSnapshot {
        source: Source::Sim,
        as_of_utc: good.context.as_of_utc.clone(),
        regime_id: "r0".into(),
        curves: vec![good.clone()],
        forecasts: vec![],
        observer_disagreements: 0,
        reserved_today_lamports: 0,
        daily_cap_lamports: 100,
    };
    let model = ModelQuoteRequest {
        context: good.context.clone(),
        candidates: vec![good.config.clone()],
        covariates: ModelCovariates::default(),
        leader_class_next: vec![],
        target: PredictionTarget::Probability {
            target_p: 0.8,
            horizon_slots: 2,
        },
    };
    let pred = CurvePrediction {
        config: good.config.clone(),
        p_hat: 0.9,
        p_interval_95: good.p_interval_95,
        evidence: Evidence::Measured,
        n_effective: 100.0,
        data_age_s: Some(1.0),
        observation_window: good.observation_window.clone(),
        unresolved_share: 0.0,
        samples_needed: None,
        m1_weight: 0.0,
    };
    let quote = ModelQuote {
        contract_version: 1,
        methodology_hash: good.methodology_hash.clone(),
        context: good.context.clone(),
        target: model.target.clone(),
        evidence: Evidence::Measured,
        recommendation: Some(pred),
        latency: None,
        samples_needed: None,
        reason: None,
    };
    let forecast = ForecastEntry {
        sequence: 1,
        prev_hash: "genesis".into(),
        hash: format!("sha256:{}", "1".repeat(64)),
        forecast: Forecast {
            id: "test".into(),
            source: Source::Sim,
            created_at_utc: good.context.as_of_utc.clone(),
            expires_at_utc: "2026-10-05T00:10:00Z".into(),
            regime_id: "r0".into(),
            methodology_hash: good.methodology_hash.clone(),
            model_snapshot_hash: "test".into(),
            request: model,
            quote,
            baselines: vec![],
            economics: None,
            requested_economics: None,
        },
    };
    let bad = AlertSnapshot {
        as_of_utc: "2026-10-05T00:02:00Z".into(),
        curves: vec![curve("r0", "2026-10-05T00:02:00Z", 0.2, [0.1, 0.3])],
        forecasts: vec![forecast],
        observer_disagreements: 3,
        reserved_today_lamports: 90,
        ..initial.clone()
    };
    (initial, bad)
}
#[tokio::test]
async fn five_induced_rules_emit_once_deliver_local_webhook_and_survive_restart() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("alerts.db");
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("store");
    let (initial, bad) = fixture();
    assert!(emit(&store, &initial).await.expect("prime").is_empty());
    let mut alerts = emit(&store, &bad).await.expect("alerts");
    assert_eq!(alerts.len(), 4);
    assert!(emit(&store, &bad).await.expect("dedup").is_empty());
    store.close().await;
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("restart");
    assert!(emit(&store, &bad).await.expect("restart dedup").is_empty());
    let changed = AlertSnapshot {
        as_of_utc: "2026-10-05T00:03:00Z".into(),
        regime_id: "r1".into(),
        curves: vec![],
        forecasts: vec![],
        observer_disagreements: 0,
        reserved_today_lamports: 0,
        ..bad.clone()
    };
    let regime = emit(&store, &changed).await.expect("regime");
    assert_eq!(regime.len(), 1);
    assert_eq!(regime[0].rule, AlertRule::RegimeChange);
    alerts.extend(regime);
    let rules = alerts
        .iter()
        .map(|a| format!("{:?}", a.rule))
        .collect::<BTreeSet<_>>();
    assert_eq!(rules.len(), 5);
    let received = Arc::new(Mutex::new(Vec::<Value>::new()));
    let state = received.clone();
    let app = Router::new().route(
        "/",
        post(move |Json(value): Json<Value>| {
            let state = state.clone();
            async move {
                state.lock().expect("receiver").push(value);
                axum::http::StatusCode::NO_CONTENT
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let endpoint = format!("http://{}/", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server");
    });
    let sender = Webhook::new(&endpoint, Format::Webhook).expect("sender");
    for alert in &alerts {
        sender.send(alert).await.expect("local delivery");
        store
            .alert_delivery(&alert.id, "DELIVERED")
            .await
            .expect("record delivery");
        assert_eq!(discord(alert)["allowed_mentions"]["parse"], json!([]));
        assert_eq!(slack(alert)["blocks"][0]["text"]["type"], "plain_text");
    }
    {
        let received = received.lock().expect("received");
        assert_eq!(received.len(), 5);
        assert!(
            received
                .iter()
                .all(|v| v["type"] == "alight.alert.v1" && v["data"]["source"] == "sim")
        );
    }
    server.abort();
    let _ = server.await;
    assert!(emit(&store, &changed).await.expect("dedup").is_empty());
    // Recovery rearms a later route/disagreement/quote/budget episode.
    let (_, events) = evaluate(&AlertState::default(), &bad).expect("new state");
    assert_eq!(events.len(), 3);
    store.close().await;
}
#[test]
fn stale_sparse_cross_source_and_regime_inputs_do_not_become_degradation_claims() {
    let (initial, mut bad) = fixture();
    let (previous, _) = evaluate(&AlertState::default(), &initial).expect("initial");
    bad.curves[0].n_effective = 2.0;
    bad.observer_disagreements = 0;
    bad.reserved_today_lamports = 0;
    assert!(evaluate(&previous, &bad).expect("sparse").1.is_empty());
    bad.curves[0].n_effective = 100.0;
    bad.curves[0].data_age_s = Some(1000.0);
    assert!(evaluate(&previous, &bad).expect("stale").1.is_empty());
    bad.curves[0].context.source = Source::Live;
    assert!(evaluate(&previous, &bad).is_err());
    assert!(Webhook::new("http://external.example/", Format::Webhook).is_err());
    assert!(Webhook::new("https://user:secret@example.com/", Format::Webhook).is_err());
}
