use alight_diagnostics::{extraction::measure, process_window};
use alight_store::{Store, canonical};
use alight_types::*;
use chrono::{Duration, TimeZone, Utc};
#[tokio::test]
async fn change_voids_frozen_forecast_and_prove_and_refuses_old_evidence_without_changing_claim() {
    let dir = tempfile::tempdir().expect("temp");
    let store = Store::open(&dir.path().join("actions.db"), 64 * 1024 * 1024)
        .await
        .expect("store");
    let start = Utc
        .with_ymd_and_hms(2026, 10, 5, 0, 0, 0)
        .single()
        .expect("date");
    let base = alight_sim::generate_focused(
        &alight_sim::Parameters {
            canaries: 1,
            ..Default::default()
        },
        Route::BeamHttp,
        SizeClass::Small,
    )
    .expect("sim")
    .canaries
    .remove(0);
    let mut samples = Vec::new();
    for i in 0..100 {
        let mut c = base.clone();
        c.id = format!("old-training-{i}");
        c.signature = Some(c.id.clone());
        c.regime_id = "sim-r0".into();
        c.config.tip_lamports = 100000;
        c.config.tip_tier = TipTier::X1;
        c.config.fee_bucket = FeeBucket::Zero;
        c.config.cu_price_micro_lamports = 0;
        c.send_wall_utc = (start + Duration::seconds(2200)).to_rfc3339();
        c.resolved_at_utc = Some((start + Duration::seconds(2201)).to_rfc3339());
        c.outcome = Some(Outcome::LandedOk);
        c.landed_slot = Some(c.sent_slot + 1);
        let sample = TrainingCanary {
            canary: c,
            finalized: true,
            covariates: ModelCovariates::default(),
        };
        store.import_training(&sample).await.expect("import");
        samples.push(sample);
    }
    let context = CurveContext {
        source: Source::Sim,
        region: "local".into(),
        regime_id: "sim-r0".into(),
        as_of_utc: (start + Duration::seconds(2300)).to_rfc3339(),
    };
    let request = ModelQuoteRequest {
        context: context.clone(),
        candidates: vec![samples[0].canary.config.clone()],
        covariates: ModelCovariates::default(),
        leader_class_next: vec![],
        target: PredictionTarget::Probability {
            target_p: 0.8,
            horizon_slots: 2,
        },
    };
    let frozen = alight_forecast::issue(&store, request, 600, None, &[])
        .await
        .expect("forecast");
    assert!(frozen.forecast.quote.recommendation.is_some());
    let body = canonical(&frozen).expect("bytes");
    let locked = alight_prove::lock(
        &store,
        RunMode::Sim,
        &ProveRequest {
            forecast_hash: frozen.hash.clone(),
            request_id: "phase5-actions".into(),
            n: 40,
            seed: Some(42),
        },
        &context.as_of_utc,
        Some(SimProofEnvironment {
            slot_ms: 250,
            congestion: 0.0,
            tip_slope: 0.45,
            fee_slope: 0.18,
            never_land_mass: None,
            continuous_latency: false,
        }),
    )
    .await
    .expect("lock");
    let mut change = None;
    for i in 0..90 {
        let at = start + Duration::seconds(i * 30);
        let w = SignalWindow {
            id: format!("action-window-{i}"),
            source: Source::Sim,
            origin: SignalOrigin::Simulation,
            from_utc: (at - Duration::seconds(30)).to_rfc3339(),
            through_utc: at.to_rfc3339(),
            start_slot: None,
            end_slot: None,
            epoch: None,
            measures: vec![
                measure(
                    SignalKind::SlotMs,
                    Some(if i < 80 { 400.0 } else { 300.0 }),
                    "ms/slot",
                    128,
                    "",
                    "synthetic",
                ),
                measure(
                    SignalKind::ReferenceLandingRate,
                    Some(if i < 80 { 0.95 } else { 0.70 }),
                    "fraction",
                    40,
                    "",
                    "synthetic",
                ),
            ],
        };
        if let Some(c) = process_window(&store, &w).await.expect("process") {
            change = Some(c);
        }
    }
    let change = change.expect("detected");
    let saved = store
        .forecast(Source::Sim, &frozen.hash)
        .await
        .expect("read")
        .expect("exists");
    assert_eq!(canonical(&saved).expect("bytes"), body);
    let grade = store
        .workbench_grades(Source::Sim, &change.detected_at_utc)
        .await
        .expect("grade");
    assert!(grade.iter().any(|g| g.forecast_hash == frozen.hash
        && g.status == ForecastStatus::Voided
        && g.reason.as_deref() == Some("regime_changed")));
    assert!(
        store
            .forecasts_to_grade(Source::Sim, 100)
            .await
            .expect("grader")
            .is_empty()
    );
    assert_eq!(store.verify_ledger(Source::Sim).await.expect("ledger"), 1);
    let current =
        alight_forecast::current_context(&store, Source::Sim, "local", &change.detected_at_utc)
            .await
            .expect("context");
    assert_eq!(current.regime_id, change.regime_id);
    let curve =
        alight_model::estimate(&samples, &samples[0].canary.config, &current, 2).expect("estimate");
    assert_eq!(curve.n_effective, 0.0);
    assert_eq!(curve.evidence, Evidence::Insufficient);
    let proof = alight_prove::refresh(&store, &locked, &change.detected_at_utc, &current.regime_id)
        .await
        .expect("refresh");
    assert_eq!(proof.state, ProveState::Voided);
    let mut policy = alight_canary::policy::AdaptivePolicy::new(42, change.exploration_fraction)
        .expect("bounded policy");
    let posterior = vec![(1.0, 1.0); policy.cells().len()];
    let assigned = policy.assign(0, 0, &posterior).expect("uniform assignment");
    assert!(assigned.uniform_arm);
    assert!(
        store
            .pending_alerts(Source::Sim)
            .await
            .expect("alert")
            .iter()
            .any(|a| a.rule == AlertRule::RegimeChange)
    );
    assert_eq!(
        store
            .counts(Source::Sim)
            .await
            .expect("counts")
            .budget_reserved_lamports,
        0
    );
}
