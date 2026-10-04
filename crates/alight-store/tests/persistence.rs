use alight_store::{Store, raw_ref};
use alight_types::*;
use serde_json::json;

fn observation(raw: &serde_json::Value, utc: &str) -> IngestEvent {
    IngestEvent::Observation(ObserverEvent {
        observer: ObserverKind::Grpc,
        signature: "simulation-signature".into(),
        slot: Some(100),
        block_id: None,
        index_in_block: Some(5),
        index_scope: IndexScope::ProviderReported,
        success: Some(true),
        received: ReceiveTime {
            clock_id: "simulation-clock".into(),
            mono_ns: 100,
            wall_utc: utc.into(),
        },
        source: Source::Sim,
        raw_ref: raw_ref(raw).expect("hash"),
    })
}

#[tokio::test]
async fn reopen_preserves_cursor_and_first_receive_and_deduplicates() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("state.db");
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("open");
    let raw = json!({"simulation":true,"slot":"100","signature":"simulation-signature"});
    assert!(
        store
            .record(
                ObserverKind::Grpc,
                &observation(&raw, "2026-10-04T00:00:01Z"),
                &raw
            )
            .await
            .expect("insert")
    );
    assert!(
        !store
            .record(
                ObserverKind::Grpc,
                &observation(&raw, "2026-10-04T00:00:02Z"),
                &raw
            )
            .await
            .expect("duplicate")
    );
    store.close().await;
    drop(store);
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("reopen");
    assert_eq!(
        store
            .cursor(ObserverKind::Grpc, Source::Sim)
            .await
            .expect("cursor"),
        Some(100)
    );
    assert_eq!(
        store
            .counts(Source::Sim)
            .await
            .expect("counts")
            .observations,
        1
    );
    let rows = store
        .observations(Source::Sim, "simulation-signature")
        .await
        .expect("read");
    assert_eq!(rows[0].received.wall_utc, "2026-10-04T00:00:01Z");
    assert_eq!(
        store
            .recent_observation(ObserverKind::Grpc, Source::Sim)
            .await
            .expect("recent")
            .expect("observation")
            .slot,
        Some(100)
    );
    store
        .start_run("second", "sim", "2026-10-04T00:01:00Z")
        .await
        .expect("run");
    assert_eq!(store.counts(Source::Sim).await.expect("counts").gaps, 1);
}

#[tokio::test]
async fn queued_pre_disconnect_frame_cannot_close_a_new_gap() {
    let dir = tempfile::tempdir().expect("directory");
    let store = Store::open(&dir.path().join("queued.db"), 16 * 1024 * 1024)
        .await
        .expect("store");
    let raw = json!({"source":"sim","slot":"100"});
    store
        .open_gap(
            ObserverKind::Grpc,
            Source::Sim,
            "2026-10-04T00:00:05Z",
            "disconnected",
        )
        .await
        .expect("gap");
    store
        .record(
            ObserverKind::Grpc,
            &observation(&raw, "2026-10-04T00:00:04Z"),
            &raw,
        )
        .await
        .expect("queued old frame");
    assert_eq!(store.counts(Source::Sim).await.expect("count").open_gaps, 1);
    store
        .record(
            ObserverKind::Grpc,
            &observation(&raw, "2026-10-04T00:00:06Z"),
            &raw,
        )
        .await
        .expect("new frame");
    assert_eq!(store.counts(Source::Sim).await.expect("count").open_gaps, 0);
    assert_eq!(
        store
            .observations(Source::Sim, "simulation-signature")
            .await
            .expect("observations")[0]
            .received
            .wall_utc,
        "2026-10-04T00:00:04Z"
    );
}

#[tokio::test]
async fn fork_candidates_never_collapse_or_guess_identity() {
    let dir = tempfile::tempdir().expect("directory");
    let store = Store::open(&dir.path().join("fork.db"), 16 * 1024 * 1024)
        .await
        .expect("open");
    let raw = json!({"simulation":true,"signature":"simulation-signature"});
    store
        .record(
            ObserverKind::Grpc,
            &observation(&raw, "2026-10-04T00:00:01Z"),
            &raw,
        )
        .await
        .expect("observation");
    for block in ["candidate-a", "candidate-b"] {
        let raw = json!({"simulation":true,"block":block});
        let event = IngestEvent::BlockMeta(BlockMetaEvent {
            slot: 100,
            block_id: block.into(),
            parent_slot: Some(99),
            parent_block_id: None,
            block_time_unix_s: Some(1000),
            block_height: Some(90),
            executed_transactions: Some(1),
            received: ReceiveTime {
                clock_id: "c".into(),
                mono_ns: 2,
                wall_utc: "2026-10-04T00:00:01Z".into(),
            },
            source: Source::Sim,
            raw_ref: raw_ref(&raw).expect("hash"),
        });
        store
            .record(ObserverKind::Grpc, &event, &raw)
            .await
            .expect("block");
    }
    assert_eq!(store.counts(Source::Sim).await.expect("counts").blocks, 2);
    assert!(
        store
            .observations(Source::Sim, "simulation-signature")
            .await
            .expect("read")[0]
            .block_id
            .is_none()
    );
    store
        .open_gap(ObserverKind::Grpc, Source::Sim, "start", "disconnect")
        .await
        .expect("gap");
    store
        .open_gap(ObserverKind::Grpc, Source::Sim, "later", "retry")
        .await
        .expect("gap");
    assert_eq!(
        store.counts(Source::Sim).await.expect("counts").open_gaps,
        1
    );
    store
        .close_gap(ObserverKind::Grpc, Source::Sim, "end")
        .await
        .expect("close");
    assert_eq!(
        store.counts(Source::Sim).await.expect("counts").open_gaps,
        0
    );
}
