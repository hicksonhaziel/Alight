use alight_diagnostics::{extraction, fidelity, observers, refresh};
use alight_store::Store;
use alight_types::*;
const NOW: &str = "2026-10-05T00:02:00Z";
fn canaries() -> Vec<WorkbenchCanary> {
    alight_sim::generate(&alight_sim::Parameters {
        canaries: 3,
        ..Default::default()
    })
    .expect("sim")
    .canaries
    .into_iter()
    .enumerate()
    .map(|(i, mut c)| {
        c.id = format!("owned-{i}");
        c.signature = Some(format!("synthetic-sig-{i}"));
        c.send_wall_utc = "2026-10-05T00:01:00Z".into();
        c.resolved_at_utc = Some("2026-10-05T00:01:01Z".into());
        c.clock_id = "shared".into();
        c.send_mono_ns = 1_000_000;
        c.outcome = Some(Outcome::LandedOk);
        c.landed_slot = Some(101);
        c.landed_block_id = Some("candidate-a".into());
        c.landed_index = Some(9);
        c.landed_index_scope = Some(IndexScope::ProviderReported);
        c.config.tip_lamports = 100000;
        WorkbenchCanary {
            canary: c,
            finalized: true,
            prove_id: None,
            resolution_reason: None,
        }
    })
    .collect()
}
fn evidence(rows: &[WorkbenchCanary]) -> Vec<ObserverEvent> {
    rows.iter()
        .flat_map(|c| {
            [
                ObserverKind::Grpc,
                ObserverKind::Mirage,
                ObserverKind::Webhook,
                ObserverKind::Rpc,
            ]
            .into_iter()
            .enumerate()
            .map(|(i, k)| ObserverEvent {
                observer: k,
                signature: c.canary.signature.clone().expect("signature"),
                slot: Some(101),
                block_id: Some("candidate-a".into()),
                index_in_block: Some(if k == ObserverKind::Rpc { 40 } else { 9 }),
                index_scope: if k == ObserverKind::Rpc {
                    IndexScope::RpcBlockList
                } else {
                    IndexScope::ProviderReported
                },
                success: Some(true),
                received: ReceiveTime {
                    clock_id: "shared".into(),
                    mono_ns: 2_000_000 + i as u64 * 1_000_000,
                    wall_utc: "2026-10-05T00:01:01Z".into(),
                },
                source: Source::Sim,
                raw_ref: alight_store::raw_ref(&serde_json::json!({"fixture":format!("synthetic-evidence-{}-{i}", c.canary.id)})).expect("hash"),
            })
        })
        .collect()
}
#[tokio::test]
async fn drop_one_observer_is_visible_durable_alerted_and_clock_scopes_are_respected() {
    let expected = [
        ObserverKind::Grpc,
        ObserverKind::Mirage,
        ObserverKind::Webhook,
        ObserverKind::Rpc,
    ];
    let rows = canaries();
    let mut all = evidence(&rows);
    for e in &mut all {
        e.raw_ref = alight_store::raw_ref(
            &serde_json::json!({"fixture":e.signature,"observer":e.observer}),
        )
        .expect("hash");
    }
    let (m, p, e) = observers::compare(&rows, &all, Source::Sim, NOW, &expected).expect("compare");
    assert!(e.is_empty());
    assert!(p.iter().all(|p| p.agreed == 3 && p.incomplete == 0));
    assert_eq!(
        m.iter()
            .find(|m| m.observer == ObserverKind::Rpc)
            .expect("rpc")
            .receive_lag_p95_ms,
        Some(3.0)
    );
    let mut dropped = all.clone();
    dropped.retain(|e| e.observer != ObserverKind::Webhook);
    let (m, _, e) = observers::compare(&rows, &dropped, Source::Sim, NOW, &expected).expect("drop");
    assert_eq!(e.len(), 3);
    assert_eq!(
        m.iter()
            .find(|m| m.observer == ObserverKind::Webhook)
            .expect("webhook")
            .missing_owned,
        3
    );
    let dir = tempfile::tempdir().expect("temp");
    let store = Store::open(&dir.path().join("chaos.db"), 64 * 1024 * 1024)
        .await
        .expect("store");
    for r in &rows {
        store
            .import_training(&TrainingCanary {
                canary: r.canary.clone(),
                finalized: true,
                covariates: ModelCovariates::default(),
            })
            .await
            .expect("import");
    }
    for ev in &dropped {
        store
            .record(
                ev.observer,
                &IngestEvent::Observation(ev.clone()),
                &serde_json::json!({"fixture":ev.signature,"observer":ev.observer}),
            )
            .await
            .expect("original evidence");
    }
    let page = refresh(&store, Source::Sim, NOW, &expected)
        .await
        .expect("refresh");
    assert_eq!(page.disagreements.len(), 3);
    let alerts = alight_notify::poll(&store, Source::Sim, "local", NOW, 200_000_000)
        .await
        .expect("alerts");
    assert!(
        alerts
            .iter()
            .any(|a| a.rule == AlertRule::ObserverDisagreement)
    );
    assert!(
        alight_notify::poll(&store, Source::Sim, "local", NOW, 200_000_000)
            .await
            .expect("duplicate poll")
            .is_empty()
    );
    for ev in &all {
        store
            .record(
                ev.observer,
                &IngestEvent::Observation(ev.clone()),
                &serde_json::json!({"fixture":ev.signature,"observer":ev.observer}),
            )
            .await
            .expect("recovery");
    }
    let recovered = refresh(&store, Source::Sim, "2026-10-05T00:10:00Z", &expected)
        .await
        .expect("recovery");
    assert!(recovered.observers.iter().all(|o| o.missing_owned == 0));
    assert_eq!(
        recovered.disagreements.len(),
        3,
        "history survives recovery"
    );
    let mut mixed = all.clone();
    for e in &mut mixed {
        if e.observer == ObserverKind::Rpc {
            e.received.clock_id = "other-process".into();
            e.block_id = None;
        }
    }
    let (m, p, _) = observers::compare(&rows, &mixed, Source::Sim, NOW, &expected).expect("mixed");
    let rpc = m
        .iter()
        .find(|m| m.observer == ObserverKind::Rpc)
        .expect("rpc");
    assert_eq!(rpc.incomparable_clocks, 3);
    assert!(rpc.receive_lag_p50_ms.is_none());
    assert!(
        p.iter()
            .filter(|p| p.a == ObserverKind::Rpc || p.b == ObserverKind::Rpc)
            .all(|p| p.incomplete == 3)
    );
}
#[test]
fn fidelity_compares_equal_paid_tips_and_index_scope_and_excludes_forks_and_owned_signatures() {
    let rows = canaries();
    let base = PassiveTip {
        source: Source::Sim,
        observer: ObserverKind::Grpc,
        signature: "market-a".into(),
        slot: 101,
        block_id: Some("candidate-a".into()),
        index_in_block: Some(20),
        index_scope: IndexScope::ProviderReported,
        recipient: "synthetic".into(),
        tip_lamports: Some(100000),
        requested_tip_lamports: 100000,
        fee_lamports: 5000,
        cu_price_micro_lamports: None,
        cu_limit: None,
        success: true,
        received: ReceiveTime {
            clock_id: "shared".into(),
            mono_ns: 0,
            wall_utc: NOW.into(),
        },
    };
    let mut other_scope = base.clone();
    other_scope.signature = "market-rpc".into();
    other_scope.index_scope = IndexScope::RpcBlockList;
    let mut owned = base.clone();
    owned.signature = rows[0].canary.signature.clone().expect("sig");
    let report = fidelity::compare(
        &rows,
        &[base.clone(), base.clone(), other_scope, owned],
        Source::Sim,
    );
    assert_eq!(report.comparisons.len(), 1);
    assert_eq!(report.comparisons[0].canary_p50_index, 9.0);
    assert_eq!(report.comparisons[0].tape_p50_index, 20.0);
    assert_eq!(report.comparisons[0].tape_transfers, 1);
    assert_eq!(report.excluded_unmatched, 1);
    let mut second_payment = base.clone();
    second_payment.recipient = "another-known-recipient".into();
    second_payment.tip_lamports = Some(100000);
    let multi = fidelity::compare(&rows, &[base.clone(), second_payment], Source::Sim);
    assert!(
        multi.comparisons.is_empty(),
        "two 100k payments total 200k, and cannot match a 100k canary"
    );
    let mut fork = base.clone();
    fork.block_id = Some("candidate-b".into());
    assert!(
        fidelity::compare(&rows, &[base, fork], Source::Sim)
            .comparisons
            .is_empty()
    );
}
#[test]
fn incomplete_block_meta_and_unverified_compute_capacity_do_not_create_ratios() {
    let body = serde_json::json!({"blockhash":"recorded-rpc-shape-test","transactions":[{"meta":{"computeUnitsConsumed":100},"transaction":{"message":{"accountKeys":["Vote111111111111111111111111111111111111111"],"instructions":[{"programIdIndex":0}]}}},{"meta":{"computeUnitsConsumed":200},"transaction":{"message":{"accountKeys":["11111111111111111111111111111111"],"instructions":[{"programIdIndex":0}]}}}]});
    let sample =
        extraction::sample_block(&body, Source::Replay, 101, NOW, None, None).expect("sample");
    let signals = extraction::block_measures(&[sample]);
    assert!(signals[0].value.is_none());
    assert_eq!(signals[1].value, Some(0.5));
    let sample = extraction::sample_block(
        &body,
        Source::Replay,
        101,
        NOW,
        Some(1000),
        Some("explicit test capacity, not mainnet fact"),
    )
    .expect("capacity");
    assert_eq!(extraction::block_measures(&[sample])[0].value, Some(0.3));
    let mut missing = body;
    missing["transactions"][0]["meta"] = serde_json::Value::Null;
    assert!(
        extraction::sample_block(
            &missing,
            Source::Replay,
            101,
            NOW,
            Some(1000),
            Some("fixture")
        )
        .expect("missing")
        .compute_units
        .is_none()
    );
}

#[tokio::test]
async fn fidelity_excludes_owned_signatures_beyond_the_displayed_canary_page() {
    let dir = tempfile::tempdir().expect("temp");
    let store = Store::open(&dir.path().join("owned.db"), 64 * 1024 * 1024)
        .await
        .expect("store");
    let base = canaries().remove(0).canary;
    for i in 0..101 {
        let mut c = base.clone();
        c.id = format!("owned-page-{i:03}");
        c.signature = Some(format!("synthetic-owned-page-{i}"));
        if i == 0 {
            c.send_wall_utc = "2026-10-05T00:00:30Z".into();
        }
        store
            .import_training(&TrainingCanary {
                canary: c,
                finalized: true,
                covariates: ModelCovariates::default(),
            })
            .await
            .expect("owned");
    }
    let page = store
        .workbench_canaries(Source::Sim, NOW, 100)
        .await
        .expect("page");
    assert_eq!(page.len(), 100);
    assert!(page.iter().all(|r| r.canary.id != "owned-page-000"));
    let owned = PassiveTip {
        source: Source::Sim,
        observer: ObserverKind::Grpc,
        signature: "synthetic-owned-page-0".into(),
        slot: 101,
        block_id: Some("synthetic-candidate".into()),
        index_in_block: Some(0),
        index_scope: IndexScope::ProviderReported,
        recipient: "synthetic-recipient".into(),
        tip_lamports: Some(100000),
        requested_tip_lamports: 100000,
        fee_lamports: 5000,
        cu_price_micro_lamports: None,
        cu_limit: None,
        success: true,
        received: ReceiveTime {
            clock_id: "shared".into(),
            mono_ns: 0,
            wall_utc: NOW.into(),
        },
    };
    let mut market = owned.clone();
    market.signature = "synthetic-market-only".into();
    market.index_in_block = Some(20);
    store
        .save_passive_tips(&[owned, market], &TapeLimits::default(), NOW)
        .await
        .expect("tape");
    assert_eq!(
        store
            .passive_tips(Source::Sim, "2026-10-05T00:00:00Z", NOW, 1000)
            .await
            .expect("original tape")
            .len(),
        2
    );
    let diagnostics = refresh(&store, Source::Sim, NOW, &[])
        .await
        .expect("diagnostics");
    assert_eq!(diagnostics.fidelity.comparisons.len(), 1);
    let comparison = &diagnostics.fidelity.comparisons[0];
    assert_eq!(comparison.canaries, 100);
    assert_eq!(comparison.tape_transfers, 1);
    assert_eq!(comparison.tape_p50_index, 20.0);
}
