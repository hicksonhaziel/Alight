use alight_model::{ModelError, estimate};
use alight_types::{
    Canary, CanaryConfig, CurveContext, Evidence, FeeBucket, Outcome, Route, SizeClass, Source,
    TipTier, TrainingCanary,
};
use std::collections::BTreeMap;

fn config() -> CanaryConfig {
    CanaryConfig {
        route: Route::Rpc,
        tip_lamports: 0,
        cu_price_micro_lamports: 0,
        cu_limit: 25_000,
        fee_bucket: FeeBucket::Zero,
        tip_tier: TipTier::None,
        size_class: SizeClass::Small,
    }
}
fn context() -> CurveContext {
    CurveContext {
        source: Source::Sim,
        regime_id: "r0".into(),
        region: "synthetic-local".into(),
        as_of_utc: "2026-10-05T00:00:00Z".into(),
    }
}
fn sample(id: usize, outcome: Outcome) -> TrainingCanary {
    TrainingCanary {
        covariates: Default::default(),
        finalized: true,
        canary: Canary {
            id: format!("sample-{id}"),
            source: Source::Sim,
            config: config(),
            policy_id: "fixture-uniform".into(),
            assignment_prob: 1.0 / 81.0,
            uniform_arm: true,
            regime_id: "r0".into(),
            sent_slot: 100,
            clock_id: "fixture".into(),
            send_mono_ns: 0,
            send_wall_utc: "2026-10-05T00:00:00Z".into(),
            signature: None,
            blockhash: "synthetic".into(),
            last_valid_block_height: 116,
            leader_class_next: vec![],
            outcome: Some(outcome),
            landed_slot: Some(101),
            landed_block_id: Some("synthetic-block".into()),
            landed_index: None,
            landed_index_scope: None,
            observer_first_seen: BTreeMap::new(),
            resolved_at_utc: Some("2026-10-05T00:00:00Z".into()),
        },
    }
}

#[test]
fn half_life_halves_mass_and_stale_input_refuses_recommendation() {
    let samples: Vec<_> = (0..100).map(|i| sample(i, Outcome::LandedOk)).collect();
    let before = estimate(&samples, &config(), &context(), 1).expect("estimate");
    assert_eq!(before.evidence, Evidence::Measured);
    assert!((before.n_effective - 100.0).abs() < 1e-10);
    let mut later = context();
    later.as_of_utc = "2026-10-05T01:00:00Z".into();
    let after = estimate(&samples, &config(), &later, 1).expect("decay");
    assert!((after.n_effective - 50.0).abs() < 1e-10);
    assert!(after.p_hat < before.p_hat);
    assert!(
        after.p_interval_95[1] - after.p_interval_95[0]
            > before.p_interval_95[1] - before.p_interval_95[0]
    );
    assert_eq!(after.evidence, Evidence::Insufficient);
    assert!(after.samples_needed.is_some_and(|n| n > 0));
}

#[test]
fn owned_final_known_samples_only_and_unresolved_share_is_visible() {
    let mut samples: Vec<_> = (0..40).map(|i| sample(i, Outcome::LandedFailed)).collect();
    let mut unresolved = sample(40, Outcome::Unresolved);
    unresolved.finalized = false;
    samples.push(unresolved);
    let mut provisional = sample(41, Outcome::LandedOk);
    provisional.finalized = false;
    samples.push(provisional);
    let mut late = sample(42, Outcome::LandedOk);
    late.canary.resolved_at_utc = Some("2026-10-05T00:00:01Z".into());
    samples.push(late);
    let mut future = sample(43, Outcome::LandedOk);
    future.canary.send_wall_utc = "2026-10-05T00:01:00Z".into();
    samples.push(future);
    let mut foreign = sample(44, Outcome::LandedOk);
    foreign.canary.source = Source::Live;
    samples.push(foreign);
    let mut old_regime = sample(45, Outcome::LandedOk);
    old_regime.canary.regime_id = "r1".into();
    samples.push(old_regime);
    let row = estimate(&samples, &config(), &context(), 1).expect("fit");
    assert_eq!(row.resolved, 40);
    assert_eq!(row.unresolved, 3);
    assert_eq!(row.assignments, 43);
    assert_eq!(row.alpha, 41.0); // LANDED_FAILED still landed within the horizon.
    assert_eq!(row.beta, 1.0);
    samples.extend((46..66).map(|i| sample(i, Outcome::Unresolved)));
    let row = estimate(&samples, &config(), &context(), 1).expect("pending gate");
    assert_eq!(row.evidence, Evidence::Insufficient);
    assert!(
        row.insufficient_reasons
            .iter()
            .any(|r| r.contains("unresolved"))
    );
}

#[test]
fn no_prior_recommendation_duplicates_rejected_and_exact_horizon() {
    let request = alight_types::ModelQuoteRequest {
        context: context(),
        candidates: vec![config()],
        covariates: Default::default(),
        leader_class_next: vec![],
        target: alight_types::PredictionTarget::LatencyQuantile {
            quantile: 0.99,
            max_slots: Some(16.0),
            max_ms: None,
        },
    };
    let quote = alight_model::quote::quote(&[], &request, &[]).expect("empty p99 quote");
    assert_eq!(quote.evidence, Evidence::Insufficient);
    assert_eq!(quote.samples_needed, Some(2000));
    let empty = estimate(&[], &config(), &context(), 1).expect("empty");
    assert_eq!(empty.evidence, Evidence::Insufficient);
    assert_eq!(empty.samples_needed, Some(30));
    let s = sample(1, Outcome::LandedOk);
    assert!(matches!(
        estimate(&[s.clone(), s.clone()], &config(), &context(), 1),
        Err(ModelError::Duplicate)
    ));
    let mut s = s;
    s.canary.landed_slot = Some(102);
    assert_eq!(
        estimate(&[s.clone()], &config(), &context(), 1)
            .expect("outside")
            .alpha,
        1.0
    );
    assert_eq!(
        estimate(&[s], &config(), &context(), 2)
            .expect("within")
            .alpha,
        2.0
    );
    let failures: Vec<_> = [
        Outcome::Expired,
        Outcome::Rejected,
        Outcome::LandedThenDropped,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, o)| sample(i, o))
    .collect();
    assert_eq!(
        estimate(&failures, &config(), &context(), 4)
            .expect("failures")
            .beta,
        4.0
    );
}
