use alight_canary::{
    governor::{BudgetError, Governor, sol_to_lamports},
    resolver::{resolve, resolve_pending},
};
use alight_store::{Store, raw_ref};
use alight_types::*;
use serde_json::json;
use std::collections::BTreeMap;

const UTC: &str = "2026-10-04T10:00:00Z";
const MS: u64 = 1_791_108_000_000;
fn canary() -> Canary {
    Canary {
        id: "simulated-send".into(),
        source: Source::Sim,
        config: CanaryConfig {
            route: Route::Rpc,
            tip_lamports: 0,
            cu_price_micro_lamports: 0,
            cu_limit: 20_000,
            fee_bucket: FeeBucket::Zero,
            tip_tier: TipTier::None,
            size_class: SizeClass::Small,
        },
        policy_id: "simulation".into(),
        assignment_prob: 1.0,
        uniform_arm: true,
        regime_id: "simulation".into(),
        sent_slot: 90,
        clock_id: "send-clock".into(),
        send_mono_ns: 100,
        send_wall_utc: "2026-10-04T09:00:00Z".into(),
        signature: Some("simulation-signature".into()),
        blockhash: "simulation-blockhash".into(),
        last_valid_block_height: 200,
        leader_class_next: vec![],
        outcome: None,
        landed_slot: None,
        landed_block_id: None,
        landed_index: None,
        landed_index_scope: None,
        observer_first_seen: BTreeMap::new(),
        resolved_at_utc: None,
    }
}
fn observation(observer: ObserverKind) -> ObserverEvent {
    ObserverEvent {
        observer,
        signature: "simulation-signature".into(),
        slot: Some(100),
        block_id: Some("simulation-block-a".into()),
        index_in_block: Some(7),
        index_scope: IndexScope::ProviderReported,
        success: Some(true),
        received: ReceiveTime {
            clock_id: "receive-clock".into(),
            mono_ns: 500,
            wall_utc: UTC.into(),
        },
        source: Source::Sim,
        raw_ref: "simulation".into(),
    }
}
fn rpc() -> RpcCheck {
    RpcCheck {
        signature: "simulation-signature".into(),
        source: Source::Sim,
        required_commitment: Commitment::Confirmed,
        checked_commitment: Commitment::Confirmed,
        checked_block_height: 201,
        searched_history: true,
        checked_at_utc: UTC.into(),
        landing: None,
        raw_ref: "simulation".into(),
    }
}

#[test]
fn exact_amounts_and_safe_configuration() {
    assert_eq!(sol_to_lamports("0.20").expect("decimal"), 200_000_000);
    assert_eq!(sol_to_lamports("0.000000001").expect("decimal"), 1);
    for value in [
        "NaN",
        "-1",
        "1e-9",
        ".2",
        "0.0000000001",
        "18446744073709551615",
        "1.2.3",
    ] {
        assert!(sol_to_lamports(value).is_err(), "{value}");
    }
}

#[tokio::test]
async fn adversarial_concurrent_routes_cannot_exceed_global_caps() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("budget.db");
    let first = Store::open(&path, 16 * 1024 * 1024).await.expect("store");
    let second = Store::open(&path, 16 * 1024 * 1024)
        .await
        .expect("second connection");
    let mut jobs = tokio::task::JoinSet::new();
    for i in 0..100 {
        let store = if i % 2 == 0 {
            first.clone()
        } else {
            second.clone()
        };
        jobs.spawn(async move {
            let governor = Governor::new(
                store,
                RunMode::Sim,
                Some("0.000000100"),
                Some("0.000000020"),
                60_000,
            )
            .expect("limits");
            governor
                .reserve(
                    &format!("attack-{i}"),
                    [Route::Rpc, Route::BeamHttp, Route::BeamQuic][i % 3],
                    1,
                    MS,
                )
                .await
                .is_ok()
        });
    }
    let mut accepted = 0;
    while let Some(r) = jobs.join_next().await {
        accepted += usize::from(r.expect("job"));
    }
    assert_eq!(accepted, 20);
    assert_eq!(
        first
            .counts(Source::Sim)
            .await
            .expect("count")
            .budget_reserved_lamports,
        20
    );
    let governor = Governor::new(
        first.clone(),
        RunMode::Sim,
        Some("0.000000100"),
        Some("0.000000020"),
        60_000,
    )
    .expect("limits");
    for batch in 1..=4 {
        assert!(
            governor
                .reserve(
                    &format!("batch-{batch}"),
                    Route::Rpc,
                    20,
                    MS + batch * 60_001
                )
                .await
                .is_ok()
        );
    }
    assert!(matches!(
        governor
            .reserve("daily-exceeded", Route::BeamQuic, 1, MS + 400_000)
            .await,
        Err(BudgetError::Denied)
    ));
    assert_eq!(
        first
            .counts(Source::Sim)
            .await
            .expect("count")
            .budget_reserved_lamports,
        100
    );
    assert!(
        first
            .budget_by_route(Source::Sim, "2026-10-04")
            .await
            .expect("routes")
            .as_object()
            .is_some_and(|v| v.len() == 3)
    );
    first.close().await;
    second.close().await;
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("reopen");
    let governor = Governor::new(
        store.clone(),
        RunMode::Sim,
        Some("0.000000100"),
        Some("0.000000020"),
        60_000,
    )
    .expect("limits");
    assert!(matches!(
        governor
            .reserve("after-restart", Route::Rpc, 1, MS + 500_000)
            .await,
        Err(BudgetError::Denied)
    ));
    assert_eq!(
        store
            .counts(Source::Sim)
            .await
            .expect("count")
            .budget_reserved_lamports,
        100
    );
}

#[tokio::test]
async fn duplicates_backward_clock_bad_day_and_read_only_modes_refuse_spend() {
    let dir = tempfile::tempdir().expect("dir");
    let store = Store::open(&dir.path().join("budget.db"), 16 * 1024 * 1024)
        .await
        .expect("store");
    let governor = Governor::new(store.clone(), RunMode::Sim, None, None, 60_000).expect("limits");
    let permit = governor
        .reserve("once", Route::Rpc, 1, MS)
        .await
        .expect("permit");
    assert_eq!(permit.consume().lamports, 1);
    assert!(governor.reserve("once", Route::Rpc, 1, MS).await.is_err());
    assert!(
        governor
            .reserve("clock-backward", Route::Rpc, 1, MS - 1)
            .await
            .is_err()
    );
    for mode in [RunMode::Observe, RunMode::Replay] {
        let g = Governor::new(store.clone(), mode, None, None, 60_000).expect("limits");
        assert!(matches!(
            g.reserve("no-send", Route::Rpc, 1, MS).await,
            Err(BudgetError::ReadOnly)
        ));
    }
    let limits = BudgetLimits {
        daily_lamports: 10,
        burst_lamports: 10,
        window_ms: 60_000,
    };
    let fabricated_day = BudgetReservation {
        id: "bad-day".into(),
        source: Source::Sim,
        route: Route::Rpc,
        day: "2099-01-01".into(),
        created_ms: MS,
        lamports: 1,
    };
    assert!(
        !store
            .reserve_budget(&fabricated_day, &limits)
            .await
            .expect("refuse")
    );
    assert_eq!(
        store
            .counts(Source::Live)
            .await
            .expect("counts")
            .budget_reserved_lamports,
        0
    );
}

#[test]
fn late_landing_and_incomplete_sighting_never_expire() {
    let c = canary();
    let mut evidence = ResolutionEvidence {
        observations: vec![observation(ObserverKind::Grpc)],
        rpc: Some(rpc()),
        ..Default::default()
    };
    assert_eq!(
        resolve(&c, &evidence, UTC).canary.outcome,
        Some(Outcome::LandedOk)
    );
    evidence.observations[0].block_id = None;
    assert_eq!(
        resolve(&c, &evidence, UTC).canary.outcome,
        Some(Outcome::Unresolved)
    );
}

#[test]
fn disagreement_is_unresolved_and_index_scopes_are_separate() {
    let c = canary();
    let a = observation(ObserverKind::Grpc);
    let mut b = observation(ObserverKind::Mirage);
    b.block_id = Some("simulation-fork".into());
    let mut e = ResolutionEvidence {
        observations: vec![a.clone(), b],
        ..Default::default()
    };
    assert_eq!(
        resolve(&c, &e, UTC).canary.outcome,
        Some(Outcome::Unresolved)
    );
    let mut b = observation(ObserverKind::Rpc);
    b.index_scope = IndexScope::RpcBlockList;
    b.index_in_block = Some(2);
    e.observations = vec![a, b];
    assert_eq!(resolve(&c, &e, UTC).canary.outcome, Some(Outcome::LandedOk));
    e.observations[1].success = Some(false);
    assert_eq!(
        resolve(&c, &e, UTC).canary.outcome,
        Some(Outcome::Unresolved)
    );
}

#[test]
fn expiry_needs_height_commitment_history_and_exact_signature_source() {
    let c = canary();
    let mut e = ResolutionEvidence::default();
    assert_eq!(
        resolve(&c, &e, UTC).canary.outcome,
        Some(Outcome::Unresolved)
    );
    e.rpc = Some(rpc());
    assert_eq!(resolve(&c, &e, UTC).canary.outcome, Some(Outcome::Expired));
    for bad in 0..6 {
        let mut r = rpc();
        match bad {
            0 => r.checked_block_height = 200,
            1 => r.checked_commitment = Commitment::Processed,
            2 => r.searched_history = false,
            3 => r.signature = "other".into(),
            4 => r.source = Source::Replay,
            _ => r.checked_at_utc = "2026-10-04T08:59:59Z".into(),
        };
        e.rpc = Some(r);
        assert_eq!(
            resolve(&c, &e, UTC).canary.outcome,
            Some(Outcome::Unresolved)
        );
    }
}

#[test]
fn dropping_requires_canonical_exclusion_and_absence_and_failure_is_preserved() {
    let c = canary();
    let mut o = observation(ObserverKind::Grpc);
    o.success = Some(false);
    let mut e = ResolutionEvidence {
        observations: vec![o],
        ..Default::default()
    };
    assert_eq!(
        resolve(&c, &e, UTC).canary.outcome,
        Some(Outcome::LandedFailed)
    );
    e.canonical_blocks.push(CanonicalBlock {
        source: Source::Sim,
        slot: 100,
        block_id: "simulation-fork".into(),
        commitment: Commitment::Finalized,
        signature: "simulation-signature".into(),
        signature_present: false,
        raw_ref: "simulation".into(),
    });
    assert_eq!(
        resolve(&c, &e, UTC).canary.outcome,
        Some(Outcome::LandedFailed)
    );
    e.rpc = Some(rpc());
    assert_eq!(
        resolve(&c, &e, UTC).canary.outcome,
        Some(Outcome::LandedThenDropped)
    );
    e.canonical_blocks[0].signature_present = true;
    assert_eq!(
        resolve(&c, &e, UTC).canary.outcome,
        Some(Outcome::LandedFailed)
    );
}

#[tokio::test]
async fn restart_mid_flight_reloads_canary_and_resolves_late_saved_landing() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("restart.db");
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("store");
    store.insert_canary(&canary()).await.expect("canary");
    resolve_pending(&store, ResolutionEvidence::default(), UTC)
        .await
        .expect("initial resolution");
    assert_eq!(
        store.pending_canaries().await.expect("pending")[0].outcome,
        Some(Outcome::Unresolved)
    );
    store.close().await;
    drop(store);
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("restart");
    let raw = json!({"source":"sim","signature":"simulation-signature","slot":"100","block_id":"simulation-block-a"});
    let mut o = observation(ObserverKind::Grpc);
    o.raw_ref = raw_ref(&raw).expect("hash");
    store
        .record(ObserverKind::Grpc, &IngestEvent::Observation(o), &raw)
        .await
        .expect("late landing");
    resolve_pending(
        &store,
        ResolutionEvidence {
            rpc: Some(rpc()),
            ..Default::default()
        },
        UTC,
    )
    .await
    .expect("resolve");
    let c = store.pending_canaries().await.expect("pending")[0].clone();
    assert_eq!(c.outcome, Some(Outcome::LandedOk));
    // Provisional outcomes remain pending until explicit finalized RPC evidence arrives.
    let mut r = rpc();
    r.checked_commitment = Commitment::Finalized;
    r.landing = Some(RpcLanding {
        slot: 100,
        block_id: "simulation-block-a".into(),
        success: true,
        commitment: Commitment::Finalized,
    });
    resolve_pending(
        &store,
        ResolutionEvidence {
            rpc: Some(r),
            ..Default::default()
        },
        UTC,
    )
    .await
    .expect("finalize");
    assert!(store.pending_canaries().await.expect("pending").is_empty());
}
