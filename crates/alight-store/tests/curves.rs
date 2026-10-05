use alight_model::estimate;
use alight_store::Store;
use alight_types::{CanaryConfig, CurveContext, FeeBucket, Route, SizeClass, Source, TipTier};

#[tokio::test]
async fn curves_reopen_idempotently_with_source_isolation_and_corruption_detection() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("curves.db");
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("store");
    let config = CanaryConfig {
        route: Route::Rpc,
        tip_lamports: 0,
        cu_price_micro_lamports: 0,
        cu_limit: 25_000,
        fee_bucket: FeeBucket::Zero,
        tip_tier: TipTier::None,
        size_class: SizeClass::Small,
    };
    let context = CurveContext {
        source: Source::Sim,
        regime_id: "r0".into(),
        region: "synthetic-local".into(),
        as_of_utc: "2026-10-05T00:00:00Z".into(),
    };
    let snapshot = estimate(&[], &config, &context, 1).expect("curve");
    let id = store.save_curve_snapshot(&snapshot).await.expect("write");
    assert_eq!(
        store
            .save_curve_snapshot(&snapshot)
            .await
            .expect("duplicate"),
        id
    );
    store.close().await;
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("reopen");
    let rows = store
        .curve_snapshots(Source::Sim, "r0", 10)
        .await
        .expect("history");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        serde_json::to_value(&rows[0]).expect("stored"),
        serde_json::to_value(&snapshot).expect("original")
    );
    assert!(
        store
            .curve_snapshots(Source::Live, "r0", 10)
            .await
            .expect("live")
            .is_empty()
    );
    assert!(
        store
            .curve_snapshots(Source::Sim, "r1", 10)
            .await
            .expect("regime")
            .is_empty()
    );
    assert!(
        store
            .training_canaries(Source::Live)
            .await
            .expect("no passive training")
            .is_empty()
    );
    let connection = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .expect("connection");
    sqlx::query("UPDATE curve_snapshots SET payload_json='{}' WHERE snapshot_id=?")
        .bind(id)
        .execute(&connection)
        .await
        .expect("corrupt");
    assert!(store.curve_snapshots(Source::Sim, "r0", 10).await.is_err());
    connection.close().await;
    store.close().await;
}
