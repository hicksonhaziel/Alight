use alight_store::Store;
use alight_tape::receipts::{evaluate, validate};
use alight_types::*;
use base64::{Engine, engine::general_purpose::STANDARD};

fn capture() -> WalletHistoryCapture {
    let wallet = bs58::encode([9u8; 32]).into_string();
    let recipient = bs58::encode([8u8; 32]).into_string();
    let ix = |program_id_index, accounts, data: Vec<u8>, outer_index| PassiveInstruction {
        program_id_index,
        accounts,
        data_base64: STANDARD.encode(data),
        outer_index,
        inner_index: None,
    };
    let tx = PassiveTransaction {
        source: Source::Sim,
        observer: ObserverKind::Grpc,
        signature: bs58::encode([6u8; 64]).into_string(),
        slot: 50,
        block_id: Some("candidate-A".into()),
        index_in_block: Some(1),
        index_scope: IndexScope::ProviderReported,
        success: true,
        fee_lamports: 5000,
        account_keys: vec![
            wallet.clone(),
            recipient.clone(),
            "11111111111111111111111111111111".into(),
            "ComputeBudget111111111111111111111111111111".into(),
        ],
        instructions: vec![
            ix(
                3,
                vec![],
                [vec![2], 25000u32.to_le_bytes().to_vec()].concat(),
                0,
            ),
            ix(
                3,
                vec![],
                [vec![3], 0u64.to_le_bytes().to_vec()].concat(),
                1,
            ),
            ix(
                2,
                vec![0, 1],
                [
                    2u32.to_le_bytes().to_vec(),
                    500000u64.to_le_bytes().to_vec(),
                ]
                .concat(),
                2,
            ),
        ],
        received: ReceiveTime {
            clock_id: "test".into(),
            mono_ns: 1,
            wall_utc: "2026-10-05T00:00:02Z".into(),
        },
    };
    WalletHistoryCapture {
        schema_version: 1,
        source: Source::Sim,
        wallet,
        from_utc: "2026-10-05T00:00:00Z".into(),
        through_utc: "2026-10-05T00:00:03Z".into(),
        tip_recipients: vec![recipient],
        rows: vec![WalletHistoryRow {
            transaction: tx,
            chain_time_utc: Some("2026-10-05T00:00:01Z".into()),
            route: Some(Route::BeamQuic),
            size_class: Some(SizeClass::Small),
            regime_id: Some("sim-r0".into()),
        }],
    }
}
fn request() -> WalletReceiptRequest {
    WalletReceiptRequest {
        region: "synthetic-local".into(),
        target_p: 0.9,
        horizon_slots: 1,
        max_curve_age_s: 300,
    }
}
fn curve(tip: u64, at: &str) -> CurveSnapshot {
    let mut c: CurveSnapshot = serde_json::from_str(include_str!(
        "../../../data/fixtures/phase2_curve_snapshot.json"
    ))
    .expect("curve");
    c.config.tip_lamports = tip;
    c.config.tip_tier = if tip == 100000 {
        TipTier::X1
    } else {
        TipTier::X5
    };
    c.context.as_of_utc = at.into();
    c.p_hat = 0.95;
    c.p_interval_95 = [0.91, 0.99];
    c.data_age_s = Some(1.0);
    c.observation_window = Some(["2026-10-04T23:59:00Z".into(), "2026-10-05T00:00:00Z".into()]);
    c
}
async fn cohort(store: &Store, at: &str) {
    for tip in [100000, 500000] {
        store
            .save_curve_snapshot(&curve(tip, at))
            .await
            .expect("snapshot");
    }
}
#[tokio::test]
async fn historical_threshold_is_conditional_and_future_favorable_curves_are_not_used() {
    let dir = tempfile::tempdir().expect("dir");
    let store = Store::open(&dir.path().join("test.db"), 16 * 1024 * 1024)
        .await
        .expect("store");
    cohort(&store, "2026-10-05T00:00:00Z").await;
    let r = evaluate(&store, &capture(), &request())
        .await
        .expect("receipt");
    assert_eq!(r.spend_above_threshold_lamports, 400000);
    assert_eq!(r.fees_lamports, 5000);
    assert_eq!(r.compared_transactions, 1);
    assert_eq!(
        r.rows[0]
            .comparison
            .as_ref()
            .expect("comparison")
            .age_at_transaction_s,
        2.0
    );
    cohort(&store, "2026-10-05T00:00:02Z").await;
    assert_eq!(
        evaluate(&store, &capture(), &request())
            .await
            .expect("future excluded")
            .spend_above_threshold_lamports,
        400000
    );
    let mut low = curve(100000, "2026-10-05T00:00:00.500Z");
    low.p_hat = 0.6;
    low.p_interval_95 = [0.5, 0.7];
    store
        .save_curve_snapshot(&low)
        .await
        .expect("new poor support");
    assert_eq!(
        evaluate(&store, &capture(), &request())
            .await
            .expect("no cherry picking")
            .compared_transactions,
        0
    );
    store.close().await;
}
#[tokio::test]
async fn missing_stale_and_mismatched_evidence_never_becomes_a_wallet_recommendation() {
    let dir = tempfile::tempdir().expect("dir");
    let store = Store::open(&dir.path().join("test.db"), 16 * 1024 * 1024)
        .await
        .expect("store");
    assert_eq!(
        evaluate(&store, &capture(), &request())
            .await
            .expect("missing")
            .compared_transactions,
        0
    );
    cohort(&store, "2026-10-05T00:00:00Z").await;
    for i in 0..8 {
        let mut c = capture();
        let mut q = request();
        match i {
            0 => c.source = Source::Replay,
            1 => q.region = "another-region".into(),
            2 => c.rows[0].regime_id = Some("other-regime".into()),
            3 => q.horizon_slots = 2,
            4 => c.rows[0].size_class = Some(SizeClass::Large),
            5 => c.rows[0].route = Some(Route::BeamHttp),
            6 => c.rows[0].chain_time_utc = None,
            _ => q.max_curve_age_s = 1,
        }
        c.rows[0].transaction.source = c.source;
        let r = evaluate(&store, &c, &q).await.expect("descriptive");
        assert_eq!(r.compared_transactions, 0, "case {i}");
        assert_eq!(r.rows[0].paid_tip_lamports, Some(500000));
    }
    store.close().await;
}
#[tokio::test]
async fn duplicate_delivery_failed_execution_inner_payment_and_conflicts_have_distinct_accounting()
{
    let dir = tempfile::tempdir().expect("dir");
    let store = Store::open(&dir.path().join("test.db"), 16 * 1024 * 1024)
        .await
        .expect("store");
    let mut c = capture();
    let mut second = c.rows[0].clone();
    second.transaction.received.mono_ns = 99;
    second.transaction.observer = ObserverKind::Mirage;
    c.rows.push(second);
    let r = evaluate(&store, &c, &request()).await.expect("duplicate");
    assert_eq!(r.transactions, 1);
    assert_eq!(r.duplicates_removed, 1);
    assert_eq!(r.fees_lamports, 5000);
    c.rows[0].transaction.block_id = None;
    let mut conflicting = c.rows[1].clone();
    conflicting.transaction.block_id = Some("candidate-B".into());
    c.rows.push(conflicting);
    assert!(evaluate(&store, &c, &request()).await.is_err());
    c.rows.pop();
    c.rows[1].transaction.success = false;
    assert!(evaluate(&store, &c, &request()).await.is_err());
    c.rows.pop();
    c.rows[0].transaction.success = false;
    let r = evaluate(&store, &c, &request()).await.expect("failed");
    assert_eq!(r.known_paid_tips_lamports, 0);
    assert_eq!(r.visible_failed_share, Some(1.0));
    assert_eq!(r.fees_lamports, 5000);
    c.rows[0].transaction.success = true;
    let mut inner = c.rows[0].transaction.instructions[2].clone();
    inner.inner_index = Some(0);
    c.rows[0].transaction.instructions.push(inner);
    let r = evaluate(&store, &c, &request())
        .await
        .expect("unknown inner");
    assert_eq!(r.unknown_tip_payments, 1);
    assert_eq!(r.rows[0].paid_tip_lamports, None);
    c.rows[0].transaction.instructions.pop();
    c.rows[0].transaction.instructions[2].accounts[0] = 1;
    assert_eq!(
        evaluate(&store, &c, &request())
            .await
            .expect("different payer")
            .rows[0]
            .paid_tip_lamports,
        None
    );
    store.close().await;
}
#[tokio::test]
async fn real_wallet_capture_is_descriptive_and_replay_labels_are_preserved() {
    let dir = tempfile::tempdir().expect("dir");
    let store = Store::open(&dir.path().join("test.db"), 16 * 1024 * 1024)
        .await
        .expect("store");
    let mut transactions = Vec::new();
    for line in include_str!("../../../data/fixtures/mirage_transactions_sample.jsonl").lines() {
        let v: serde_json::Value = serde_json::from_str(line).expect("recording");
        let received = ReceiveTime {
            clock_id: "real-recording-replay".into(),
            mono_ns: 0,
            wall_utc: v["received_at"].as_str().expect("received").into(),
        };
        if let Some(tx) =
            alight_tape::captured_transaction(&v, Source::Replay, received).expect("parse")
        {
            transactions.push(tx);
        }
    }
    let tx = transactions.remove(1);
    let capture = WalletHistoryCapture {
        schema_version: 1,
        source: Source::Replay,
        wallet: tx.account_keys[0].clone(),
        from_utc: "2026-10-04T00:40:00Z".into(),
        through_utc: "2026-10-04T00:45:00Z".into(),
        tip_recipients: vec![],
        rows: vec![WalletHistoryRow {
            transaction: tx,
            chain_time_utc: None,
            route: None,
            size_class: None,
            regime_id: None,
        }],
    };
    let r = evaluate(&store, &capture, &request())
        .await
        .expect("real wallet");
    assert_eq!(r.source, Source::Replay);
    assert_eq!(r.transactions, 1);
    assert_eq!(r.compared_transactions, 0);
    assert!(!r.limits.is_empty());
    validate(&capture).expect("capture");
    let hash = store.save_wallet_capture(&capture).await.expect("save");
    assert!(
        store
            .wallet_capture(Source::Live, &capture.wallet, &hash)
            .await
            .expect("isolation")
            .is_none()
    );
    store.close().await;
    let store = Store::open_read_only(&dir.path().join("test.db"))
        .await
        .expect("read only");
    assert!(
        store
            .wallet_capture(Source::Replay, &capture.wallet, &hash)
            .await
            .expect("persisted")
            .is_some()
    );
    assert!(store.save_wallet_capture(&capture).await.is_err());
    store.close().await;
}
#[test]
fn invalid_wallet_live_import_future_chain_time_and_oversized_capture_fail_closed() {
    let mut c = capture();
    c.source = Source::Live;
    assert!(validate(&c).is_err());
    let mut c = capture();
    c.rows[0].chain_time_utc = Some("2026-10-05T00:00:03Z".into());
    assert!(validate(&c).is_err());
    let mut c = capture();
    c.wallet = "not-a-wallet".into();
    assert!(validate(&c).is_err());
    let mut c = capture();
    c.rows = vec![c.rows[0].clone(); 1001];
    assert!(validate(&c).is_err());
}

#[tokio::test]
async fn ambiguous_same_time_cells_and_future_training_windows_cannot_support_savings() {
    let dir = tempfile::tempdir().expect("dir");
    let store = Store::open(&dir.path().join("test.db"), 16 * 1024 * 1024)
        .await
        .expect("store");
    cohort(&store, "2026-10-05T00:00:00Z").await;
    let mut changed = curve(100000, "2026-10-05T00:00:00Z");
    changed.p_hat = 0.96;
    changed.p_interval_95 = [0.92, 0.99];
    store
        .save_curve_snapshot(&changed)
        .await
        .expect("ambiguous same cell/time");
    assert_eq!(
        evaluate(&store, &capture(), &request())
            .await
            .expect("ambiguity excluded")
            .spend_above_threshold_lamports,
        0
    );
    cohort(&store, "2026-10-05T00:00:00.500Z").await;
    let mut future = curve(100000, "2026-10-05T00:00:00.750Z");
    future.observation_window =
        Some(["2026-10-05T00:00:00Z".into(), "2026-10-05T00:00:02Z".into()]);
    store
        .save_curve_snapshot(&future)
        .await
        .expect("future window fixture");
    store
        .save_curve_snapshot(&curve(500000, "2026-10-05T00:00:00.750Z"))
        .await
        .expect("actual cell");
    assert_eq!(
        evaluate(&store, &capture(), &request())
            .await
            .expect("future training excluded")
            .spend_above_threshold_lamports,
        0
    );
    let mut empty = capture();
    empty.tip_recipients.clear();
    assert_eq!(
        evaluate(&store, &empty, &request())
            .await
            .expect("no recipient coverage")
            .rows[0]
            .paid_tip_lamports,
        None
    );
    store.close().await;
}
