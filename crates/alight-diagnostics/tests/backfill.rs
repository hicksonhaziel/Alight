use alight_diagnostics::{
    backfill::{History, plan, reconstruct},
    extraction,
};
use alight_store::Store;
use alight_types::*;
use chrono::{Duration, TimeZone, Utc};
fn block(slot: u64, seconds: i64) -> BlockMetaEvent {
    BlockMetaEvent {
        source: Source::Replay,
        slot,
        block_id: format!("synthetic-historical-block-{slot}"),
        parent_slot: Some(slot - 1),
        parent_block_id: None,
        block_time_unix_s: Some(seconds),
        block_height: None,
        executed_transactions: None,
        received: ReceiveTime {
            clock_id: "historical".into(),
            mono_ns: 0,
            wall_utc: chrono::DateTime::from_timestamp(seconds, 0)
                .expect("date")
                .to_rfc3339(),
        },
        raw_ref: "synthetic-history-test".into(),
    }
}
#[tokio::test]
async fn historical_clock_steps_are_replay_only_and_retention_limits_are_explicit() {
    let dir = tempfile::tempdir().expect("temp");
    let store = Store::open(&dir.path().join("history.db"), 64 * 1024 * 1024)
        .await
        .expect("store");
    let start = Utc
        .with_ymd_and_hms(2026, 8, 1, 0, 0, 0)
        .single()
        .expect("date");
    let mut slot = 9007199254740993;
    let mut blocks = vec![block(slot, start.timestamp())];
    for i in 0..320 {
        let ms = match i {
            0..80 => 400,
            80..160 => 350,
            160..240 => 300,
            _ => 250,
        };
        slot += 14_400_000 / ms;
        blocks.push(block(
            slot,
            (start + Duration::hours((i + 1) * 4)).timestamp(),
        ));
    }
    let as_of = (start + Duration::hours(1280)).to_rfc3339();
    let history = History {
        requested_days: 56,
        as_of_utc: as_of.clone(),
        first_available_slot: 9007199254740993,
        requests: 321,
        blocks,
    };
    let report = reconstruct(&store, &history).await.expect("reconstruct");
    assert_eq!(report.status, "RETENTION_OR_SAMPLING_LIMIT");
    assert_eq!(report.timestamps, 321);
    let changes = store
        .regime_changes(Source::Replay, &as_of)
        .await
        .expect("changes");
    assert_eq!(changes.len(), 3, "{changes:?}");
    assert!(changes.iter().all(|c| c.origin == SignalOrigin::Backfill));
    assert!(
        store
            .active_regime(Source::Replay, &as_of)
            .await
            .expect("active")
            .is_none()
    );
    assert!(
        store
            .active_regime(Source::Live, &as_of)
            .await
            .expect("live")
            .is_none()
    );
    assert!(
        serde_json::to_string(&report)
            .expect("json")
            .contains("\"first_available_slot\":\"9007199254740993\"")
    );
    assert!(
        reconstruct(&store, &history).await.is_ok(),
        "idempotent history replay"
    );
    let slots = plan(100, 1_000_000, 56, 1024).expect("plan");
    assert_eq!(slots.len(), 1024);
    assert_eq!(slots[0], 100);
    assert_eq!(slots[1023], 1_000_000);
    assert!(plan(200, 100, 56, 512).is_err());
    assert!(plan(100, 200, 57, 512).is_err());
    assert!(plan(100, 200, 56, 1025).is_err());
}
#[test]
fn coarse_chain_seconds_and_candidate_conflicts_are_handled_before_clock_signals() {
    let mut blocks: Vec<_> = (1000..1800)
        .map(|slot| block(slot, 1_700_000_000 + ((slot - 1000) / 4) as i64))
        .collect();
    let mut fork = blocks[200].clone();
    fork.block_id = "other-candidate".into();
    blocks.push(fork);
    let windows = extraction::clock_windows(&blocks, &[], Source::Replay, SignalOrigin::Replay)
        .expect("extract");
    assert!(!windows.is_empty());
    assert!(
        windows
            .iter()
            .all(|w| w.measures[0].value.is_some_and(|v| (v - 250.0).abs() < 5.0))
    );
    assert!(windows.iter().all(|w| {
        w.measures
            .iter()
            .find(|m| m.kind == SignalKind::SlotP95Ms)
            .expect("p95")
            .value
            .is_some()
    }));
    assert!(extraction::clock_windows(&blocks, &[], Source::Live, SignalOrigin::Replay).is_err());
}
