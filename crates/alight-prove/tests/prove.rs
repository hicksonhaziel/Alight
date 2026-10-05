use alight_canary::governor::Governor;
use alight_prove::{grade, lock, methodology_hash, refresh, run_sim, wilson};
use alight_store::{Store, canonical, content_hash};
use alight_types::*;

const NOW: &str = "2026-10-05T01:00:00Z";
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
fn claim() -> ProveLock {
    ProveLock {
        id: "prove-sim-unit".into(),
        source: Source::Sim,
        forecast_hash: "sha256:unit".into(),
        model_snapshot_hash: "sha256:unit-model".into(),
        methodology_hash: methodology_hash(),
        config: config(),
        regime_id: "sim-r0".into(),
        region: "local".into(),
        locked_at_utc: NOW.into(),
        expires_at_utc: "2026-10-05T01:01:00Z".into(),
        target: PredictionTarget::Probability {
            target_p: 0.8,
            horizon_slots: 2,
        },
        claimed_probability: 0.9,
        n: 40,
        seed: Some(42),
        simulation: Some(environment()),
    }
}
fn outcomes(lock: &ProveLock, k: u32) -> Vec<TrainingCanary> {
    (0..lock.n)
        .map(|i| {
            let mut sample = alight_sim::simulate_locked(lock, i).expect("prospective shape");
            sample.canary.outcome = Some(if i < k {
                Outcome::LandedOk
            } else {
                Outcome::Expired
            });
            sample.canary.landed_slot = (i < k).then_some(sample.canary.sent_slot + 1);
            sample.canary.resolved_at_utc = Some("2026-10-05T01:00:20Z".into());
            sample
        })
        .collect()
}
async fn setup(store: &Store) -> ForecastEntry {
    let base = alight_sim::generate(&alight_sim::Parameters {
        canaries: 1,
        ..Default::default()
    })
    .expect("base")
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
            .expect("training");
    }
    alight_forecast::issue(
        store,
        ModelQuoteRequest {
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
        60,
        None,
        &[],
    )
    .await
    .expect("supported forecast")
}
fn request(entry: &ForecastEntry, id: &str) -> ProveRequest {
    ProveRequest {
        request_id: id.into(),
        forecast_hash: entry.hash.clone(),
        n: 40,
        seed: Some(42),
    }
}

#[test]
fn wilson_and_all_three_verdicts_match_known_counts() {
    let [lo, hi] = wilson(36, 40).expect("Wilson");
    assert!((lo - 0.7694822477247763).abs() < 1e-12);
    assert!((hi - 0.9604204713173937).abs() < 1e-12);
    assert!(wilson(0, 0).is_none());
    assert!(wilson(41, 40).is_none());
    let lock = claim();
    let report = grade(
        &lock,
        &outcomes(&lock, 36),
        "2026-10-05T01:00:30Z",
        "sim-r0",
    )
    .expect("consistent");
    assert_eq!(report.verdict, ProveVerdict::Consistent);
    assert_eq!(report.state, ProveState::Complete);
    assert_eq!(report.observed_rate, Some(0.9));
    assert_eq!(
        grade(&lock, &outcomes(&lock, 0), "2026-10-05T01:00:30Z", "sim-r0")
            .expect("unfavorable")
            .verdict,
        ProveVerdict::Inconsistent
    );
    let mut unknown = outcomes(&lock, 36);
    unknown[0].finalized = false;
    unknown[0].canary.outcome = Some(Outcome::Unresolved);
    let partial = grade(&lock, &unknown, "2026-10-05T01:02:00Z", "sim-r0").expect("partial expiry");
    assert_eq!(partial.verdict, ProveVerdict::Inconclusive);
    assert_eq!(partial.unresolved, 1);
    assert_eq!(partial.resolved, 39);
    assert_eq!(unknown[0].canary.outcome, Some(Outcome::Unresolved));
    let shifted = grade(
        &lock,
        &outcomes(&lock, 36),
        "2026-10-05T01:00:30Z",
        "sim-r1",
    )
    .expect("regime void");
    assert_eq!(shifted.state, ProveState::Voided);
    assert_eq!(shifted.verdict, ProveVerdict::Inconclusive);
    let mut small = lock.clone();
    small.n = 10;
    assert_eq!(
        grade(
            &small,
            &outcomes(&small, 9),
            "2026-10-05T01:00:30Z",
            "sim-r0"
        )
        .expect("small N")
        .verdict,
        ProveVerdict::Inconclusive
    );
}

#[test]
fn clock_and_exact_scope_failures_cannot_be_counted_as_nonlanding() {
    let mut lock = claim();
    lock.target = PredictionTarget::LatencyQuantile {
        quantile: 0.9,
        max_slots: None,
        max_ms: Some(750.0),
    };
    let mut samples = outcomes(&lock, 40);
    for s in &mut samples {
        s.canary.observer_first_seen.clear();
    }
    let report =
        grade(&lock, &samples, "2026-10-05T01:00:30Z", "sim-r0").expect("missing comparable clock");
    assert_eq!(report.resolved, 0);
    assert_eq!(report.unresolved, 40);
    assert!(report.wilson_interval_95.is_none());
    let c = &mut samples[0].canary;
    c.observer_first_seen.insert(
        ObserverKind::Grpc,
        ReceiveTime {
            clock_id: "other-process".into(),
            mono_ns: c.send_mono_ns + 500_000_000,
            wall_utc: "2026-10-05T01:00:02Z".into(),
        },
    );
    assert_eq!(
        grade(&lock, &samples, "2026-10-05T01:00:30Z", "sim-r0")
            .expect("incompatible clock")
            .resolved,
        0
    );
    samples[0].canary.config.tip_lamports += 1;
    assert!(grade(&lock, &samples, "2026-10-05T01:00:30Z", "sim-r0").is_err());
}

#[tokio::test]
async fn quote_lock_forty_held_out_draws_governor_ledger_and_restart_work_end_to_end() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("prove.db");
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("store");
    let entry = setup(&store).await;
    let req = request(&entry, "forty");
    let mut invalid_env = environment();
    invalid_env.slot_ms = 0;
    assert!(
        lock(&store, RunMode::Sim, &req, NOW, Some(invalid_env))
            .await
            .is_err()
    );
    assert!(
        store
            .prove(Source::Sim, "prove-sim-forty")
            .await
            .expect("invalid environment writes nothing")
            .is_none()
    );
    let locked = lock(&store, RunMode::Sim, &req, NOW, Some(environment()))
        .await
        .expect("lock");
    assert_eq!(
        locked.lock.model_snapshot_hash,
        entry.forecast.model_snapshot_hash
    );
    assert_eq!(locked.lock.config, config());
    let retry = lock(
        &store,
        RunMode::Sim,
        &req,
        "2026-10-05T01:00:01Z",
        Some(environment()),
    )
    .await
    .expect("idempotent lock");
    assert_eq!(locked.lock_hash, retry.lock_hash);
    let conflicting = request(&entry, "same-cell");
    assert!(
        lock(&store, RunMode::Sim, &conflicting, NOW, Some(environment()))
            .await
            .is_err()
    );
    let report = run_sim(&store, &locked, None, None)
        .await
        .expect("prospective sim");
    assert_eq!(report.attempts, 40);
    assert_eq!(report.resolved, 40);
    assert_eq!(report.unresolved, 0);
    assert_eq!(report.state, ProveState::Complete);
    let members = store
        .prove_canaries(Source::Sim, &locked.lock.id)
        .await
        .expect("members");
    assert_eq!(members.len(), 40);
    assert!(
        members
            .iter()
            .all(|s| s.canary.config == config() && s.canary.source == Source::Sim)
    );
    assert_eq!(
        store
            .training_canaries(Source::Sim)
            .await
            .expect("heldout excluded")
            .len(),
        100
    );
    assert_eq!(
        store
            .grading_canaries(Source::Sim)
            .await
            .expect("grading includes heldout")
            .len(),
        140
    );
    assert_eq!(
        store
            .counts(Source::Sim)
            .await
            .expect("caps")
            .budget_reserved_lamports,
        40 * 205026
    );
    let scores = alight_model::scoring::grade(
        &entry,
        &store
            .grading_canaries(Source::Sim)
            .await
            .expect("owned grading"),
        "2026-10-05T01:02:00Z",
    )
    .expect("forecast grade");
    assert_eq!(scores.scores.as_ref().expect("heldout scores").n, 40);
    store.save_grade(&scores).await.expect("save score");
    assert_eq!(store.verify_ledger(Source::Sim).await.expect("ledger"), 1);
    let frozen = store
        .load_models(&locked.lock.model_snapshot_hash)
        .await
        .expect("frozen");
    assert_eq!(
        content_hash(&frozen).expect("hash"),
        locked.lock.model_snapshot_hash
    );
    store.close().await;
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("restart");
    let loaded = store
        .prove(Source::Sim, &locked.lock.id)
        .await
        .expect("read")
        .expect("run");
    assert_eq!(
        canonical(&loaded).expect("stored"),
        canonical(&report).expect("returned")
    );
    let repeat = run_sim(&store, &loaded, None, None)
        .await
        .expect("completed retry");
    assert_eq!(
        canonical(&repeat).expect("repeat"),
        canonical(&loaded).expect("before")
    );
    assert_eq!(
        store
            .counts(Source::Sim)
            .await
            .expect("no duplicate spend")
            .budget_reserved_lamports,
        40 * 205026
    );
    assert!(
        lock(&store, RunMode::Observe, &req, NOW, Some(environment()))
            .await
            .is_err()
    );
    assert!(store.save_prove_report(&locked).await.is_err());
    let voided = refresh(&store, &repeat, "2026-10-05T01:02:00Z", "changed-regime")
        .await
        .expect("regime void");
    assert_eq!(voided.state, ProveState::Voided);
    // A worker holding a report from before the regime change cannot revive the claim.
    assert!(
        refresh(&store, &repeat, "2026-10-05T01:02:01Z", "sim-r0")
            .await
            .is_err()
    );
    let still_voided = store
        .prove(Source::Sim, &locked.lock.id)
        .await
        .expect("read")
        .expect("exists");
    assert_eq!(still_voided.state, ProveState::Voided);
    assert_eq!(still_voided.verdict, ProveVerdict::Inconclusive);
}

#[tokio::test]
async fn interrupted_membership_and_report_update_resume_without_double_charging() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("interrupted.db");
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("store");
    let entry = setup(&store).await;
    let locked = lock(
        &store,
        RunMode::Sim,
        &request(&entry, "interrupt"),
        NOW,
        Some(environment()),
    )
    .await
    .expect("lock");
    let sample = alight_sim::simulate_locked(&locked.lock, 0).expect("first draw");
    let governor = Governor::new(store.clone(), RunMode::Sim, None, None, 60000).expect("governor");
    governor
        .reserve(
            &sample.canary.id,
            config().route,
            205026,
            chrono::DateTime::parse_from_rfc3339(&sample.canary.send_wall_utc)
                .expect("UTC")
                .timestamp_millis() as u64,
        )
        .await
        .expect("reserve before crash")
        .consume();
    store.close().await;
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("restart");
    let result = run_sim(&store, &locked, None, None)
        .await
        .expect("resume reservation");
    assert_eq!(result.attempts, 40);
    assert_eq!(
        store
            .counts(Source::Sim)
            .await
            .expect("spend")
            .budget_reserved_lamports,
        40 * 205026
    );
    // Replaying the original pre-result snapshot recovers the durable completed membership.
    let recovered = run_sim(&store, &locked, None, None)
        .await
        .expect("interrupted report recovery");
    assert_eq!(recovered.resolved, 40);
    assert_eq!(recovered.verdict, result.verdict);
}

#[tokio::test]
async fn capped_or_expired_runs_do_not_invent_the_missing_canaries() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("caps.db");
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("store");
    let entry = setup(&store).await;
    let locked = lock(
        &store,
        RunMode::Sim,
        &request(&entry, "capped"),
        NOW,
        Some(environment()),
    )
    .await
    .expect("lock");
    let report = run_sim(&store, &locked, Some("0.001"), Some("0.000411"))
        .await
        .expect("cap gate");
    assert_eq!(report.state, ProveState::BudgetCapped);
    assert_eq!(report.attempts, 2);
    assert_eq!(report.verdict, ProveVerdict::Inconclusive);
    assert_eq!(
        store
            .counts(Source::Sim)
            .await
            .expect("cap preserved")
            .budget_reserved_lamports,
        410052
    );
    let late = refresh(&store, &report, "2026-10-05T01:02:00Z", "sim-r0")
        .await
        .expect("expiry");
    assert_eq!(late.attempts, 2);
    assert_eq!(late.verdict, ProveVerdict::Inconclusive);
    assert_eq!(late.state, ProveState::Complete);
    let retry = run_sim(&store, &late, None, None)
        .await
        .expect("closed run");
    assert_eq!(retry.attempts, 2);
}
