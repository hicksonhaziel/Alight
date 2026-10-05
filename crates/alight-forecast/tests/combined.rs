use alight_forecast::{issue_combined, preview_combined};
use alight_store::{Store, canonical};
use alight_types::*;

const NOW: &str = "2026-10-05T00:10:00Z";
fn config(tip: u64) -> CanaryConfig {
    CanaryConfig {
        route: Route::BeamHttp,
        tip_lamports: tip,
        cu_price_micro_lamports: 0,
        cu_limit: 25000,
        fee_bucket: FeeBucket::Zero,
        tip_tier: if tip == 100000 {
            TipTier::X1
        } else {
            TipTier::X5
        },
        size_class: SizeClass::Small,
    }
}
fn inputs() -> EconomicsInputs {
    EconomicsInputs {
        pool: "synthetic-pool".into(),
        size_usd: "1000".into(),
        sol_usd: "100".into(),
        edge_bps: 10.0,
        lambda: 1.0,
        base_fee_lamports: 5000,
        use_upper_quantile: false,
    }
}
fn request() -> QuoteServiceRequest {
    QuoteServiceRequest {
        model: ModelQuoteRequest {
            context: CurveContext {
                source: Source::Sim,
                regime_id: "sim-r0".into(),
                region: "synthetic-local".into(),
                as_of_utc: NOW.into(),
            },
            candidates: vec![config(100000), config(500000)],
            covariates: ModelCovariates::default(),
            leader_class_next: vec![],
            target: PredictionTarget::Probability {
                target_p: 0.4,
                horizon_slots: 2,
            },
        },
        ttl_s: 60,
        economics: Some(inputs()),
        frozen_model_hash: None,
    }
}
fn market() -> DelayCostSnapshot {
    DelayCostSnapshot {
        source: Source::Sim,
        regime_id: "sim-r0".into(),
        pool: "synthetic-pool".into(),
        mint: "synthetic-mint".into(),
        as_of_utc: NOW.into(),
        window_s: 300,
        measured_slot_ms: 250.0,
        upper_quantile: 0.9,
        max_age_s: 30.0,
        minimum_pairs: 30,
        observation_window: Some(["2026-10-05T00:05:00Z".into(), NOW.into()]),
        data_age_s: Some(0.0),
        trade_samples: 500,
        slot_samples: 500,
        disconnected: false,
        stale: false,
        sparse: false,
        points: [1, 4, 16]
            .map(|d| DelayCostPoint {
                delay_slots: d,
                delay_ms: f64::from(d) * 250.0,
                median_bps: Some(f64::from(d) * 30.0),
                upper_bps: Some(f64::from(d) * 60.0),
                pairs: 100,
                split_half_relative_change: Some(0.0),
            })
            .to_vec(),
    }
}
async fn populate(store: &Store) -> Vec<TrainingCanary> {
    let data = alight_sim::generate(&alight_sim::Parameters {
        canaries: 1,
        ..Default::default()
    })
    .expect("synthetic base");
    let base = &data.canaries[0];
    let mut samples = Vec::new();
    for cell in 0..2 {
        for i in 0..100 {
            let mut c = base.clone();
            c.id = format!("controlled-{cell}-{i}");
            c.signature = Some(c.id.clone());
            c.config = config(if cell == 0 { 100000 } else { 500000 });
            c.source = Source::Sim;
            c.regime_id = "sim-r0".into();
            c.send_wall_utc = "2026-10-05T00:09:00Z".into();
            c.resolved_at_utc = Some("2026-10-05T00:09:10Z".into());
            c.sent_slot = 1000;
            let d = if cell == 0 {
                if i < 60 {
                    1
                } else if i < 80 {
                    4
                } else if i < 90 {
                    16
                } else {
                    0
                }
            } else if i < 85 {
                1
            } else if i < 90 {
                4
            } else if i < 95 {
                16
            } else {
                0
            };
            c.outcome = Some(if d == 0 {
                Outcome::Expired
            } else {
                Outcome::LandedOk
            });
            c.landed_slot = (d != 0).then_some(1000 + d);
            let sample = TrainingCanary {
                canary: c,
                finalized: true,
                covariates: ModelCovariates::default(),
            };
            store
                .import_training(&sample)
                .await
                .expect("owned simulation import");
            samples.push(sample);
        }
    }
    samples
}

#[tokio::test]
async fn public_preview_is_read_only_and_matches_the_frozen_operator_response() {
    let dir = tempfile::tempdir().expect("dir");
    let store = Store::open(&dir.path().join("preview.db"), 16 * 1024 * 1024)
        .await
        .expect("store");
    populate(&store).await;
    store.save_market_snapshot(&market()).await.expect("market");
    let before = canonical(&store.counts(Source::Sim).await.expect("counts")).expect("canonical");
    let preview = preview_combined(&store, request()).await.expect("preview");
    assert!(
        preview
            .economics
            .as_ref()
            .expect("economics")
            .economics
            .is_some()
    );
    assert_eq!(
        store
            .verify_ledger(Source::Sim)
            .await
            .expect("empty ledger"),
        0
    );
    assert!(
        store
            .load_models(&preview.model_snapshot_hash)
            .await
            .is_err()
    );
    assert_eq!(
        before,
        canonical(&store.counts(Source::Sim).await.expect("counts")).expect("canonical")
    );
    let entry = issue_combined(&store, request()).await.expect("freeze");
    assert_eq!(
        preview.model_snapshot_hash,
        entry.forecast.model_snapshot_hash
    );
    assert_eq!(
        canonical(&preview.quote).expect("preview"),
        canonical(&entry.forecast.quote).expect("frozen")
    );
    assert_eq!(
        canonical(&preview.economics).expect("preview"),
        canonical(&entry.forecast.economics).expect("frozen")
    );
    assert_eq!(
        canonical(&preview.baselines).expect("preview"),
        canonical(&entry.forecast.baselines).expect("frozen")
    );
    assert_eq!(
        canonical(&preview.requested_economics).expect("preview inputs"),
        canonical(&entry.forecast.requested_economics).expect("frozen inputs")
    );
    assert_eq!(
        store
            .forecast_page(Source::Sim, 0, 1)
            .await
            .expect("page")
            .len(),
        1
    );
    assert!(
        store
            .forecast_page(Source::Sim, entry.sequence, 1)
            .await
            .expect("next")
            .is_empty()
    );
    assert!(
        store
            .forecast_page(Source::Live, 0, 1)
            .await
            .expect("isolated")
            .is_empty()
    );
}

#[tokio::test]
async fn request_response_and_market_inputs_survive_immutable_ledger_and_restart() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("combined.db");
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("store");
    let samples = populate(&store).await;
    let mut r = request();
    let no_market = issue_combined(&store, r.clone())
        .await
        .expect("absent market fallback");
    assert_eq!(
        no_market
            .forecast
            .economics
            .as_ref()
            .expect("fallback")
            .fallback_reason
            .as_deref(),
        Some("market_snapshot_unavailable")
    );
    assert_eq!(
        no_market
            .forecast
            .quote
            .recommendation
            .as_ref()
            .expect("supported probability")
            .config
            .tip_lamports,
        100000
    );
    assert_eq!(
        canonical(&no_market.forecast.requested_economics).expect("frozen request"),
        canonical(&r.economics).expect("request")
    );
    r.economics.as_mut().expect("inputs").size_usd = "2000".into();
    let changed = issue_combined(&store, r)
        .await
        .expect("different fallback inputs");
    assert_ne!(changed.hash, no_market.hash);
    let market_hash = store
        .save_market_snapshot(&market())
        .await
        .expect("save market");
    assert_eq!(
        market_hash,
        store
            .save_market_snapshot(&market())
            .await
            .expect("idempotent snapshot")
    );
    assert!(
        store
            .market_snapshot(Source::Live, "sim-r0", "synthetic-pool", NOW)
            .await
            .expect("source lookup")
            .is_none()
    );
    assert!(
        store
            .market_snapshot(Source::Sim, "other-regime", "synthetic-pool", NOW)
            .await
            .expect("regime lookup")
            .is_none()
    );
    assert!(
        store
            .market_snapshot(
                Source::Sim,
                "sim-r0",
                "synthetic-pool",
                "2026-10-05T00:09:59Z"
            )
            .await
            .expect("future lookup")
            .is_none()
    );
    let r = request();
    let (left, right) = tokio::join!(
        issue_combined(&store, r.clone()),
        issue_combined(&store, r.clone())
    );
    let entry = left.expect("concurrent quote");
    assert_eq!(entry.hash, right.expect("concurrent retry").hash);
    let econ = entry.forecast.economics.as_ref().expect("econ response");
    let summary = econ.economics.as_ref().expect("valid economics");
    assert_eq!(
        entry
            .forecast
            .quote
            .recommendation
            .as_ref()
            .expect("economic recommendation")
            .config
            .tip_lamports,
        500000
    );
    assert_eq!(
        canonical(&econ.model_quote).expect("response"),
        canonical(&entry.forecast.quote).expect("ledger quote")
    );
    assert_eq!(
        canonical(&summary.market).expect("frozen market"),
        canonical(&market()).expect("source market")
    );
    let baseline = &summary.baselines[0].cost.as_ref().expect("B1 cost");
    assert_eq!(
        baseline.prediction.config,
        entry.forecast.baselines[0]
            .config
            .clone()
            .expect("B1 config")
    );
    // Projection retains exactly the model horizon probability and excludes later/unfinalized evidence.
    let p = &summary.recommendation.prediction;
    let d = alight_model::latency::landing_distribution(
        &samples,
        &p.config,
        &r.model.context,
        2,
        p.p_hat,
    )
    .expect("distribution")
    .expect("supported");
    assert!(
        (d.masses
            .iter()
            .filter(|m| m.delay_slots <= 2)
            .map(|m| m.probability)
            .sum::<f64>()
            - p.p_hat)
            .abs()
            < 1e-10
    );
    assert!(
        (d.masses.iter().map(|m| m.probability).sum::<f64>() + d.nonlanding_probability - 1.0)
            .abs()
            < 1e-10
    );
    let mut extra = samples.clone();
    let mut future = extra[100].clone();
    future.canary.id = "future".into();
    future.canary.send_wall_utc = "2026-10-05T00:10:01Z".into();
    future.canary.resolved_at_utc = Some("2026-10-05T00:10:02Z".into());
    extra.push(future);
    let mut pending = extra[100].clone();
    pending.canary.id = "pending".into();
    pending.finalized = false;
    extra.push(pending);
    // Pending evidence can change evidence gates; it never changes resolved conditional shape.
    let e = alight_model::latency::landing_distribution(
        &extra,
        &p.config,
        &r.model.context,
        2,
        p.p_hat,
    )
    .expect("exclusions")
    .expect("still supported");
    assert_eq!(
        canonical(&d).expect("before"),
        canonical(&e).expect("after")
    );
    assert_eq!(
        store
            .verify_ledger(Source::Sim)
            .await
            .expect("ledger chain"),
        3
    );
    let mut invalid = entry.forecast.clone();
    invalid.id = "mismatched".into();
    invalid
        .economics
        .as_mut()
        .expect("econ")
        .model_quote
        .context
        .region = "other".into();
    assert!(store.append_forecast(&invalid).await.is_err());
    store.close().await;
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("reopen");
    let loaded = store
        .forecast(Source::Sim, &entry.hash)
        .await
        .expect("read")
        .expect("exists");
    assert_eq!(
        canonical(&loaded).expect("stored"),
        canonical(&entry).expect("returned")
    );
    assert_eq!(
        store
            .verify_ledger(Source::Sim)
            .await
            .expect("restart chain"),
        3
    );
}

#[tokio::test]
async fn stale_market_preserves_probability_quote_and_invalid_inputs_are_rejected() {
    let dir = tempfile::tempdir().expect("dir");
    let store = Store::open(&dir.path().join("fallback.db"), 16 * 1024 * 1024)
        .await
        .expect("store");
    populate(&store).await;
    let mut stale = market();
    stale.as_of_utc = "2026-10-05T00:09:00Z".into();
    store
        .save_market_snapshot(&stale)
        .await
        .expect("historical market");
    let entry = issue_combined(&store, request())
        .await
        .expect("stale fallback");
    assert_eq!(
        entry
            .forecast
            .economics
            .as_ref()
            .expect("fallback")
            .fallback_reason
            .as_deref(),
        Some("blur_stale_or_disconnected")
    );
    assert_eq!(
        entry
            .forecast
            .quote
            .recommendation
            .as_ref()
            .expect("probability recommendation")
            .config
            .tip_lamports,
        100000
    );
    let mut bad = request();
    bad.economics.as_mut().expect("inputs").size_usd = "NaN".into();
    assert!(issue_combined(&store, bad).await.is_err());
    assert_eq!(
        store
            .verify_ledger(Source::Sim)
            .await
            .expect("no invalid forecast"),
        1
    );
}
