use alight_store::Store;
use alight_types::*;

fn tip(index: u64) -> PassiveTip {
    PassiveTip {
        source: Source::Sim,
        observer: ObserverKind::Grpc,
        signature: format!("synthetic-{index}"),
        slot: 100,
        block_id: Some("block-a".into()),
        index_in_block: Some(index),
        index_scope: IndexScope::ProviderReported,
        recipient: "synthetic-recipient".into(),
        tip_lamports: Some(100_000),
        requested_tip_lamports: 100_000,
        fee_lamports: 5000,
        cu_price_micro_lamports: None,
        cu_limit: None,
        success: true,
        received: ReceiveTime {
            clock_id: "sim".into(),
            mono_ns: index,
            wall_utc: "2026-10-05T00:00:00Z".into(),
        },
    }
}
#[tokio::test]
async fn tape_is_bounded_deduplicated_scoped_and_survives_restart_without_training() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("phase3.db");
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("store");
    let limits = TapeLimits {
        retention_s: 60,
        max_rows: 100,
        max_bytes: 4096,
    };
    let records = (0..100).map(tip).collect::<Vec<_>>();
    let now = "2026-10-05T00:00:00Z";
    let usage = store
        .save_passive_tips(&records, &limits, now)
        .await
        .expect("byte quota");
    assert!(usage.rows < 100 && usage.rows > 0);
    assert!(usage.payload_bytes <= 4096);
    assert!(
        store
            .training_canaries(Source::Sim)
            .await
            .expect("owned population")
            .is_empty()
    );
    let mut latest = tip(99);
    latest.received.wall_utc = "2026-10-05T00:00:01Z".into();
    let duplicate = store
        .save_passive_tips(&[latest.clone()], &limits, &latest.received.wall_utc)
        .await
        .expect("duplicate");
    assert_eq!(duplicate.rows, usage.rows);
    latest.tip_lamports = Some(999);
    assert!(
        store
            .save_passive_tips(&[latest], &limits, now)
            .await
            .is_err()
    );
    let mut fork = tip(99);
    fork.block_id = Some("block-b".into());
    let row_limits = TapeLimits {
        max_rows: 2,
        max_bytes: 4096,
        ..limits.clone()
    };
    assert_eq!(
        store
            .save_passive_tips(&[fork], &row_limits, now)
            .await
            .expect("fork and row cap")
            .rows,
        2
    );
    let mut other_source = tip(99);
    other_source.source = Source::Replay;
    store
        .save_passive_tips(&[other_source], &row_limits, now)
        .await
        .expect("source partition");
    store.close().await;
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("restart");
    let rows = store
        .passive_tips(Source::Sim, now, now, 100)
        .await
        .expect("read");
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].block_id, rows[1].block_id);
    assert_eq!(
        store
            .passive_tips(Source::Replay, now, now, 100)
            .await
            .expect("replay partition")
            .len(),
        1
    );
    let mut new = tip(101);
    new.received.wall_utc = "2026-10-05T00:02:00Z".into();
    assert_eq!(
        store
            .save_passive_tips(&[new.clone()], &row_limits, &new.received.wall_utc)
            .await
            .expect("retention")
            .rows,
        1
    );
    assert_eq!(
        store
            .passive_tips(Source::Replay, now, now, 100)
            .await
            .expect("partition unchanged")
            .len(),
        1
    );
    assert_eq!(
        store
            .prune_passive_tips(Source::Sim, &row_limits, "2026-10-05T00:03:01Z")
            .await
            .expect("idle retention"),
        1
    );
    assert_eq!(
        store
            .passive_tips(Source::Sim, now, "2026-10-05T00:03:01Z", 100)
            .await
            .expect("idle empty")
            .len(),
        0
    );
}
