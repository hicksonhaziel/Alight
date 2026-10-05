use alight_canary::governor::Governor;
use alight_store::Store;
use alight_types::*;
use serde_json::json;

#[tokio::test]
async fn prepared_membership_is_atomic_frozen_held_out_and_never_rebroadcast_on_restart() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("prepared.db");
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("store");
    let dataset = alight_sim::generate_focused(
        &alight_sim::Parameters {
            canaries: 1000,
            ..Default::default()
        },
        Route::BeamHttp,
        SizeClass::Small,
    )
    .expect("sim");
    for s in dataset.training().expect("labels") {
        store.import_training(&s).await.expect("import");
    }
    let mut candidates = dataset
        .ground_truth
        .iter()
        .map(|g| g.config.clone())
        .collect::<Vec<_>>();
    candidates.sort_by_key(alight_model::quote::nominal_cost);
    let forecast = alight_forecast::issue(
        &store,
        ModelQuoteRequest {
            context: CurveContext {
                source: Source::Sim,
                regime_id: "sim-r0".into(),
                region: "local".into(),
                as_of_utc: dataset.as_of_utc.clone(),
            },
            candidates,
            covariates: ModelCovariates::default(),
            leader_class_next: vec![],
            target: PredictionTarget::Probability {
                target_p: 0.4,
                horizon_slots: 2,
            },
        },
        300,
        None,
        &[],
    )
    .await
    .expect("quote");
    let locked = alight_prove::lock(
        &store,
        RunMode::Sim,
        &ProveRequest {
            request_id: "prepared".into(),
            forecast_hash: forecast.hash,
            n: 40,
            seed: Some(42),
        },
        &dataset.as_of_utc,
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
    let mut sample = alight_sim::simulate_locked(&locked.lock, 0).expect("prospective identity");
    sample.finalized = false;
    sample.canary.outcome = None;
    sample.canary.resolved_at_utc = None;
    sample.canary.signature = Some("synthetic-prepared-signature".into());
    let g = Governor::new(store.clone(), RunMode::Sim, None, None, 60000).expect("governor");
    let cost = alight_model::quote::nominal_cost(&sample.canary.config);
    let millis = chrono::DateTime::parse_from_rfc3339(&sample.canary.send_wall_utc)
        .expect("time")
        .timestamp_millis() as u64;
    g.reserve(&sample.canary.id, sample.canary.config.route, cost, millis)
        .await
        .expect("reserve")
        .consume();
    let key = format!("prove/{}", locked.lock.id);
    let mut invalid = sample.canary.clone();
    invalid.config.cu_price_micro_lamports += 1;
    assert!(
        store
            .prepare_prove_send(
                &invalid,
                &key,
                0,
                &json!({"kind":"synthetic-fixed"}),
                "sha256:synthetic",
                &locked.lock.id
            )
            .await
            .is_err()
    );
    assert_eq!(
        store.next_policy_draw(&key).await.expect("rollback draw"),
        0
    );
    assert!(
        store
            .prove_canaries(Source::Sim, &locked.lock.id)
            .await
            .expect("rollback membership")
            .is_empty()
    );
    store
        .prepare_prove_send(
            &sample.canary,
            &key,
            0,
            &json!({"kind":"synthetic-fixed"}),
            "sha256:synthetic",
            &locked.lock.id,
        )
        .await
        .expect("durable preparation");
    assert_eq!(store.next_policy_draw(&key).await.expect("next ordinal"), 1);
    assert_eq!(
        store
            .training_canaries(Source::Sim)
            .await
            .expect("heldout")
            .len(),
        1000
    );
    store.close().await;
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("restart");
    assert_eq!(
        store
            .prove_canaries(Source::Sim, &locked.lock.id)
            .await
            .expect("membership")
            .len(),
        1
    );
    assert_eq!(
        store
            .send_summary(Source::Sim)
            .await
            .expect("uncertain send")["PREPARED"],
        1
    );
    assert_eq!(
        store
            .pending_canaries()
            .await
            .expect("resolver work")
            .iter()
            .filter(|c| c.id == sample.canary.id)
            .count(),
        1
    );
    assert!(
        store
            .prepare_prove_send(
                &sample.canary,
                &key,
                0,
                &json!({}),
                "sha256:synthetic",
                &locked.lock.id
            )
            .await
            .is_err()
    );
    assert_eq!(
        store
            .counts(Source::Sim)
            .await
            .expect("charged once")
            .budget_reserved_lamports,
        cost as i64
    );
}
