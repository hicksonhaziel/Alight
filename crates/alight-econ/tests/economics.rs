use alight_econ::{
    EconError, blur, optimizer,
    series::{PriceSeries, SeriesSettings},
};
use alight_types::*;

fn context() -> CurveContext {
    CurveContext {
        source: Source::Sim,
        regime_id: "sim-regime".into(),
        region: "sim-region".into(),
        as_of_utc: "2026-10-05T00:01:00Z".into(),
    }
}
fn pool() -> BlurPool {
    BlurPool {
        pool: "synthetic-pool".into(),
        mint: "synthetic-mint".into(),
        quote_mint: "synthetic-quote".into(),
        dex: "synthetic".into(),
        price_usd: "100".into(),
        tvl_usd: "100000".into(),
    }
}
fn trade(i: u32, price: f64) -> MarketTrade {
    MarketTrade {
        source: Source::Sim,
        pool: pool().pool,
        mint: pool().mint,
        slot: 1000 + u64::from(i),
        block_time_unix_s: 1791158400 + i64::from(i / 4),
        signature: format!("sim-{i}"),
        tx_index: 0,
        ix_index: 0,
        inner_ix_index: None,
        price_usd: price.to_string(),
        candle_ok: true,
        base_amount: 1,
        quote_amount: 2,
        base_reserve: 3,
        quote_reserve: 4,
        fee_amount: 0,
    }
}
fn series(settings: SeriesSettings) -> PriceSeries {
    PriceSeries::new(&pool(), &context(), 1000, settings).expect("series")
}
fn numeric(s: &str) -> f64 {
    s.parse().expect("number")
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8, "{a} != {b}");
}

#[test]
fn captured_rest_and_ws_replay_preserve_exact_values_and_never_claim_live() {
    let (pools, trades, candles) =
        blur::replay_rest(include_bytes!("../../../data/fixtures/blur_sample.json"))
            .expect("REST fixture");
    assert_eq!(pools.len(), 3);
    assert_eq!(trades.len(), 9);
    assert_eq!(candles.len(), 10);
    assert!(trades.iter().all(|t| t.source == Source::Replay));
    assert!(candles.iter().all(|t| t.source == Source::Replay));
    assert_eq!(trades[0].price_usd, "0.9999761951158425");
    let events: Vec<_> = include_str!("../../../data/fixtures/blur_ws_sample.jsonl")
        .lines()
        .map(|s| blur::replay_frame(s.as_bytes()).expect("WS fixture"))
        .collect();
    assert!(matches!(events[0], BlurEvent::Connected { .. }));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, BlurEvent::Trade { .. }))
            .count(),
        11
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, BlurEvent::Trade { trade } if trade.inner_ix_index == Some(-1)))
            .count(),
        2
    );
    let exact = events
        .iter()
        .find_map(|event| match event {
            BlurEvent::Trade { trade }
                if trade.pool == "EgUV2hrWfsgfFyftxCx4ut82811ygWFbWYNPdv3cRWQA" =>
            {
                Some(trade)
            }
            _ => None,
        })
        .expect("large reserve fixture");
    assert_eq!(exact.base_reserve, 210_847_950_099_700_846);
    assert_eq!(exact.source, Source::Replay);
    assert_eq!(
        serde_json::to_value(exact).expect("JSON")["base_reserve"],
        "210847950099700846"
    );
    assert!(blur::replay_frame(br#"{"data":{"type":"connected"}}"#).is_err());
    assert!(blur::frame(br#"{"type":"unknown"}"#, Source::Replay).is_err());
    assert!(matches!(
        blur::replay_rest(&vec![b' '; 512 * 1024 + 1]),
        Err(EconError::TooLarge)
    ));
}

#[test]
fn historical_three_pool_fixture_is_sparse_and_becomes_stale() {
    let (pools, trades, _) =
        blur::replay_rest(include_bytes!("../../../data/fixtures/blur_sample.json"))
            .expect("fixture");
    let mut ctx = context();
    ctx.source = Source::Replay;
    ctx.as_of_utc = "2026-10-03T22:14:40Z".into();
    for pool in pools {
        let mut s = PriceSeries::new(&pool, &ctx, 0, SeriesSettings::default()).expect("series");
        for trade in trades.iter().filter(|t| t.pool == pool.pool) {
            s.ingest(trade.clone(), &ctx.as_of_utc).expect("ingest");
        }
        let snapshot = s
            .snapshot(&[1, 2, 4], 250.0, &ctx.as_of_utc)
            .expect("snapshot");
        assert!(snapshot.sparse);
        assert!(!snapshot.stale);
        assert!(snapshot.points.iter().all(|p| p.median_bps.is_none()));
        assert!(
            s.snapshot(&[1], 250.0, "2026-10-03T22:20:00Z")
                .expect("old snapshot")
                .stale
        );
    }
}

#[test]
fn known_geometric_prices_recover_delay_returns_and_stability_in_slot_units() {
    let mut s = series(SeriesSettings {
        max_age_s: 120.0,
        ..SeriesSettings::default()
    });
    for i in (0..100).rev() {
        s.ingest(
            trade(i, 100.0 * 1.01f64.powi(i as i32)),
            &context().as_of_utc,
        )
        .expect("ingest");
    }
    let d = s
        .snapshot(&[0, 1, 2, 4], 250.0, &context().as_of_utc)
        .expect("snapshot");
    assert!(!d.stale && !d.sparse);
    assert_eq!(d.slot_samples, 100);
    for point in &d.points {
        near(point.delay_ms, f64::from(point.delay_slots) * 250.0);
        near(
            point.median_bps.expect("median"),
            (1.01f64.powi(point.delay_slots as i32) - 1.0) * 10000.0,
        );
        near(
            point.upper_bps.expect("upper"),
            point.median_bps.expect("median"),
        );
        if point.delay_slots != 0 {
            assert!(point.split_half_relative_change.expect("stability") < 1e-8);
        }
    }
    let faster = s
        .snapshot(&[4], 200.0, &context().as_of_utc)
        .expect("clock conversion");
    near(faster.points[0].delay_ms, 800.0);
    near(
        faster.points[0].median_bps.expect("median"),
        d.points[3].median_bps.expect("median"),
    );
}

#[test]
fn bounds_dedup_conflicts_missing_slots_and_disconnects_are_visible() {
    let mut s = series(SeriesSettings {
        max_trades: 33,
        max_age_s: 120.0,
        ..SeriesSettings::default()
    });
    for i in 0..40 {
        s.ingest(trade(i, 100.0), &context().as_of_utc)
            .expect("ingest");
    }
    assert_eq!(s.retained_trades(), 33);
    assert!(
        !s.ingest(trade(39, 100.0), &context().as_of_utc)
            .expect("repeat")
    );
    assert!(matches!(
        s.ingest(trade(39, 101.0), &context().as_of_utc),
        Err(EconError::Conflict)
    ));
    assert!(
        !s.snapshot(&[1], 250.0, &context().as_of_utc)
            .expect("snapshot")
            .sparse
    );
    assert!(
        s.snapshot(&[10], 250.0, &context().as_of_utc)
            .expect("too few pairs")
            .sparse
    );
    s.disconnect();
    assert!(
        s.snapshot(&[1], 250.0, &context().as_of_utc)
            .expect("disconnected")
            .stale
    );
    assert!(
        !s.ingest(trade(39, 100.0), &context().as_of_utc)
            .expect("repeat cannot reconnect")
    );
    s.ingest(trade(40, 200.0), &context().as_of_utc)
        .expect("new segment");
    let d = s
        .snapshot(&[1], 250.0, &context().as_of_utc)
        .expect("new coverage");
    assert!(!d.stale && d.sparse);
    assert_eq!(d.points[0].pairs, 0);
    let mut wrong = trade(41, 200.0);
    wrong.source = Source::Live;
    assert!(s.ingest(wrong, &context().as_of_utc).is_err());
    assert!(s.snapshot(&[1], f64::NAN, &context().as_of_utc).is_err());
}

#[test]
fn empirical_median_upper_tail_and_split_half_shift_are_known_inputs() {
    let mut s = series(SeriesSettings {
        minimum_pairs: 2,
        max_age_s: 120.0,
        ..SeriesSettings::default()
    });
    let mut price = 100.0;
    s.ingest(trade(0, price), &context().as_of_utc)
        .expect("start");
    for i in 1..=8 {
        price *= if i <= 4 { 1.01 } else { 1.05 };
        s.ingest(trade(i, price), &context().as_of_utc)
            .expect("trade");
    }
    let d = s
        .snapshot(&[1], 250.0, &context().as_of_utc)
        .expect("snapshot");
    near(d.points[0].median_bps.expect("median"), 100.0);
    near(d.points[0].upper_bps.expect("upper"), 500.0);
    near(d.points[0].split_half_relative_change.expect("shift"), 0.8);
}

fn config(tip: u64) -> CanaryConfig {
    CanaryConfig {
        route: Route::BeamHttp,
        tip_lamports: tip,
        cu_price_micro_lamports: 0,
        cu_limit: 20000,
        fee_bucket: FeeBucket::Zero,
        tip_tier: match tip {
            100000 => TipTier::X1,
            200000 => TipTier::X2,
            _ => TipTier::X5,
        },
        size_class: SizeClass::Small,
    }
}
fn candidate(tip: u64, p: f64, masses: &[(u32, f64)]) -> EconomicCandidate {
    EconomicCandidate {
        prediction: CurvePrediction {
            config: config(tip),
            p_hat: p,
            p_interval_95: [p - 0.04, (p + 0.05).min(1.0)],
            evidence: Evidence::Measured,
            n_effective: 100.0,
            data_age_s: Some(1.0),
            observation_window: Some(["2026-10-05T00:00:00Z".into(), context().as_of_utc]),
            unresolved_share: 0.0,
            samples_needed: None,
            m1_weight: 0.0,
        },
        latency: None,
        distribution: LandingDistribution {
            context: context(),
            n_effective: 100.0,
            data_age_s: 1.0,
            masses: masses
                .iter()
                .map(|(delay_slots, probability)| LandingMass {
                    delay_slots: *delay_slots,
                    probability: *probability,
                })
                .collect(),
            nonlanding_probability: 1.0 - masses.iter().map(|(_, p)| p).sum::<f64>(),
        },
    }
}
fn quote(c: &EconomicCandidate, target_p: f64) -> ModelQuote {
    ModelQuote {
        contract_version: CONTRACT_VERSION,
        methodology_hash: "sim-methodology".into(),
        context: context(),
        target: PredictionTarget::Probability {
            target_p,
            horizon_slots: 4,
        },
        evidence: c.prediction.evidence,
        recommendation: Some(c.prediction.clone()),
        latency: None,
        samples_needed: None,
        reason: None,
    }
}
fn market() -> DelayCostSnapshot {
    DelayCostSnapshot {
        source: Source::Sim,
        regime_id: context().regime_id,
        pool: pool().pool,
        mint: pool().mint,
        as_of_utc: context().as_of_utc,
        window_s: 300,
        measured_slot_ms: 250.0,
        upper_quantile: 0.9,
        max_age_s: 30.0,
        minimum_pairs: 30,
        observation_window: Some(["2026-10-05T00:00:00Z".into(), context().as_of_utc]),
        data_age_s: Some(1.0),
        trade_samples: 100,
        slot_samples: 100,
        disconnected: false,
        stale: false,
        sparse: false,
        points: [1, 4]
            .iter()
            .map(|d| DelayCostPoint {
                delay_slots: *d,
                delay_ms: f64::from(*d) * 250.0,
                median_bps: Some(f64::from(*d) * 100.0),
                upper_bps: Some(f64::from(*d) * 200.0),
                pairs: 90,
                split_half_relative_change: Some(0.0),
            })
            .collect(),
    }
}
fn inputs() -> EconomicsInputs {
    EconomicsInputs {
        pool: pool().pool,
        size_usd: "1000".into(),
        sol_usd: "100".into(),
        edge_bps: 10.0,
        lambda: 1.0,
        base_fee_lamports: 5000,
        use_upper_quantile: false,
    }
}

#[test]
fn optimizer_golden_costs_include_unconditional_delay_and_named_baseline_savings() {
    let c = vec![
        candidate(100000, 0.9, &[(1, 0.5), (4, 0.4)]),
        candidate(200000, 0.95, &[(1, 0.9), (4, 0.05)]),
        candidate(500000, 0.98, &[(1, 0.98)]),
    ];
    let baselines = vec![
        EconomicBaseline {
            id: "B1".into(),
            candidate: Some(c[0].clone()),
            unavailable_reason: None,
        },
        EconomicBaseline {
            id: "B3".into(),
            candidate: None,
            unavailable_reason: Some("no_tape".into()),
        },
    ];
    let result = optimizer::optimize(&quote(&c[1], 0.90), &inputs(), &market(), &c, &baselines)
        .expect("optimize");
    let e = result.economics.expect("economics");
    assert_eq!(e.recommendation.prediction.config.tip_lamports, 500000);
    near(numeric(&e.evaluated[0].expected_cost_usd), 21.1105);
    near(numeric(&e.evaluated[0].upper_cost_usd), 42.1105);
    near(numeric(&e.evaluated[1].expected_cost_usd), 11.0705);
    near(numeric(&e.recommendation.expected_cost_usd), 9.8705);
    near(
        numeric(e.baselines[0].savings_usd.as_deref().expect("savings")),
        11.24,
    );
    assert_eq!(
        e.baselines[1].unavailable_reason.as_deref(),
        Some("no_tape")
    );
    assert_eq!(e.frontier.len(), 1);
    assert!(!e.evaluated[0].qualifies);
    assert!(e.assumption.contains("canary size class"));
}

#[test]
fn frontier_knee_and_priority_fee_ceiling_have_known_inputs() {
    let mut c = vec![
        candidate(100000, 0.7, &[(1, 0.7)]),
        candidate(200000, 0.9, &[(1, 0.9)]),
        candidate(500000, 0.95, &[(1, 0.95)]),
    ];
    c[0].prediction.config.cu_price_micro_lamports = 1;
    let mut input = inputs();
    input.edge_bps = 0.0;
    input.lambda = 0.0;
    let result = optimizer::optimize(&quote(&c[0], 0.6), &input, &market(), &c, &[])
        .expect("optimize")
        .economics
        .expect("economics");
    assert_eq!(result.recommendation.nominal_lamports, 105001);
    assert_eq!(result.frontier.len(), 3);
    assert_eq!(result.knee.expect("knee").tip_lamports, 200000);
}

#[test]
fn stale_sparse_gap_and_wrong_scope_preserve_probability_only_quotes() {
    let c = vec![candidate(100000, 0.9, &[(1, 0.9)])];
    let q = quote(&c[0], 0.8);
    let mut stale = market();
    stale.stale = true;
    {
        let m = stale;
        let result = optimizer::optimize(&q, &inputs(), &m, &c, &[]).expect("fallback");
        assert!(result.economics.is_none());
        assert_eq!(
            serde_json::to_value(result.model_quote).expect("JSON"),
            serde_json::to_value(&q).expect("JSON")
        );
    }
    for (reason, mut m) in [
        ("blur_sparse", market()),
        ("blur_stale_or_disconnected", market()),
        ("market_scope_mismatch", market()),
    ] {
        match reason {
            "blur_sparse" => m.sparse = true,
            "market_scope_mismatch" => m.source = Source::Replay,
            _ => m.disconnected = true,
        }
        let result = optimizer::optimize(&q, &inputs(), &m, &c, &[]).expect("fallback");
        assert_eq!(result.fallback_reason.as_deref(), Some(reason));
    }
    let mut aged = market();
    aged.as_of_utc = "2026-10-05T00:00:00Z".into();
    assert_eq!(
        optimizer::optimize(&q, &inputs(), &aged, &c, &[])
            .expect("age")
            .fallback_reason
            .as_deref(),
        Some("blur_stale_or_disconnected")
    );
}

#[test]
fn unsupported_mass_missing_tail_evidence_and_nonqualifying_targets_cannot_be_optimized() {
    let c = vec![candidate(100000, 0.9, &[(1, 0.9)])];
    let q = quote(&c[0], 0.8);
    let mut invalid = c.clone();
    invalid[0].distribution.nonlanding_probability = 0.5;
    assert!(optimizer::optimize(&q, &inputs(), &market(), &invalid, &[]).is_err());
    let mut tail = c.clone();
    tail[0].distribution.masses.push(LandingMass {
        delay_slots: 8,
        probability: 0.05,
    });
    tail[0].distribution.nonlanding_probability -= 0.05;
    assert_eq!(
        optimizer::optimize(&q, &inputs(), &market(), &tail, &[])
            .expect("tail")
            .fallback_reason
            .as_deref(),
        Some("candidate_latency_or_delay_evidence_insufficient")
    );
    let mut unsupported = c.clone();
    unsupported[0].prediction.evidence = Evidence::Insufficient;
    assert!(
        optimizer::optimize(&q, &inputs(), &market(), &unsupported, &[])
            .expect("no evidence")
            .economics
            .is_none()
    );
    assert!(
        optimizer::optimize(&quote(&c[0], 0.95), &inputs(), &market(), &c, &[])
            .expect("target")
            .economics
            .is_none()
    );
    let mut wrong = c.clone();
    wrong[0].distribution.context.region = "elsewhere".into();
    assert!(optimizer::optimize(&q, &inputs(), &market(), &wrong, &[]).is_err());
}

#[test]
fn upper_quantile_objective_changes_the_choice_and_latency_targets_keep_their_bound() {
    let mut candidates = vec![
        candidate(100000, 0.9, &[(1, 0.9)]),
        candidate(200000, 0.9, &[(4, 0.9)]),
    ];
    let mut m = market();
    m.points[0].median_bps = Some(10.0);
    m.points[0].upper_bps = Some(1000.0);
    m.points[1].median_bps = Some(20.0);
    m.points[1].upper_bps = Some(100.0);
    let mut input = inputs();
    let q = quote(&candidates[0], 0.8);
    let result = optimizer::optimize(&q, &input, &m, &candidates, &[]).expect("central");
    assert_eq!(
        result
            .economics
            .expect("economics")
            .recommendation
            .prediction
            .config
            .tip_lamports,
        100000
    );
    input.use_upper_quantile = true;
    let result = optimizer::optimize(&q, &input, &m, &candidates, &[]).expect("upper");
    assert_eq!(
        result
            .economics
            .expect("economics")
            .recommendation
            .prediction
            .config
            .tip_lamports,
        200000
    );
    input.use_upper_quantile = false;
    for (i, c) in candidates.iter_mut().enumerate() {
        c.latency = Some(LatencyEstimate {
            quantile: 0.5,
            slots: Some(1.0),
            slots_interval_95: [Some(0.0), Some(4.0)],
            ms: Some(400.0),
            ms_interval_95: [Some(300.0), Some(if i == 0 { 700.0 } else { 500.0 })],
            n_effective: 100.0,
            evidence: Evidence::Measured,
            samples_needed: None,
        });
    }
    let mut latency_quote = q;
    latency_quote.target = PredictionTarget::LatencyQuantile {
        quantile: 0.5,
        max_slots: None,
        max_ms: Some(600.0),
    };
    let result =
        optimizer::optimize(&latency_quote, &input, &m, &candidates, &[]).expect("latency");
    assert_eq!(
        result
            .economics
            .expect("economics")
            .recommendation
            .prediction
            .config
            .tip_lamports,
        200000
    );
}
