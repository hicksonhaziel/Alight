use alight_forecast::issue_combined;
use alight_store::Store;
use alight_types::*;

const NOW: &str = "2026-10-05T00:10:00Z";
fn row(signature: &str, recipient: &str, amount: Option<u64>) -> PassiveTip {
    PassiveTip {
        source: Source::Sim,
        observer: ObserverKind::Grpc,
        signature: signature.into(),
        slot: 1000,
        block_id: None,
        index_in_block: Some(5),
        index_scope: IndexScope::ProviderReported,
        recipient: recipient.into(),
        tip_lamports: amount,
        requested_tip_lamports: amount.unwrap_or(999999),
        fee_lamports: 5000,
        cu_price_micro_lamports: None,
        cu_limit: None,
        success: true,
        received: ReceiveTime {
            clock_id: "sim".into(),
            mono_ns: 1,
            wall_utc: NOW.into(),
        },
    }
}
#[tokio::test]
async fn b3_aggregates_recipients_once_per_signature_and_excludes_unknown_fork_and_observer_conflicts()
 {
    let dir = tempfile::tempdir().expect("dir");
    let store = Store::open(&dir.path().join("passive.db"), 16 * 1024 * 1024)
        .await
        .expect("store");
    let mut rows = vec![
        row("tx-a", "r-a", Some(100000)),
        row("tx-a", "r-b", Some(200000)),
        row("tx-b", "r-a", Some(300000)),
        row("unknown", "r-a", None),
        row("fork", "r-a", Some(900000)),
        row("disagree", "r-a", Some(700000)),
    ];
    let mut duplicate = rows[0].clone();
    duplicate.observer = ObserverKind::Mirage;
    rows.push(duplicate);
    let mut fork = row("fork", "r-a", Some(900000));
    fork.block_id = Some("other-block".into());
    rows.push(fork);
    let mut conflict = row("disagree", "r-a", Some(800000));
    conflict.observer = ObserverKind::Mirage;
    rows.push(conflict);
    let mut failed = row("failed", "r-a", Some(0));
    failed.success = false;
    failed.requested_tip_lamports = 100000;
    rows.push(failed);
    store
        .save_passive_tips(&rows, &TapeLimits::default(), NOW)
        .await
        .expect("passive captures");
    assert!(
        store
            .training_canaries(Source::Sim)
            .await
            .expect("no owned labels")
            .is_empty()
    );
    let entry = issue_combined(
        &store,
        QuoteServiceRequest {
            model: ModelQuoteRequest {
                context: CurveContext {
                    source: Source::Sim,
                    regime_id: "empty".into(),
                    region: "local".into(),
                    as_of_utc: NOW.into(),
                },
                candidates: vec![CanaryConfig {
                    route: Route::BeamHttp,
                    tip_lamports: 100000,
                    cu_price_micro_lamports: 0,
                    cu_limit: 25000,
                    fee_bucket: FeeBucket::Zero,
                    tip_tier: TipTier::X1,
                    size_class: SizeClass::Small,
                }],
                covariates: ModelCovariates::default(),
                leader_class_next: vec![],
                target: PredictionTarget::Probability {
                    target_p: 0.9,
                    horizon_slots: 2,
                },
            },
            ttl_s: 60,
            economics: None,
            frozen_model_hash: None,
        },
    )
    .await
    .expect("empty owned quote");
    assert_eq!(entry.forecast.quote.evidence, Evidence::Insufficient);
    assert!(entry.forecast.quote.recommendation.is_none());
    let b3 = entry
        .forecast
        .baselines
        .iter()
        .find(|b| b.id == "B3")
        .expect("B3");
    let summary = b3.tape.as_ref().expect("passive summary");
    assert_eq!(summary.samples, 2);
    assert_eq!(summary.median_lamports, 300000);
    assert!(b3.p_hat.is_none());
    assert_eq!(store.verify_ledger(Source::Sim).await.expect("ledger"), 1);
}
