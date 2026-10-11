use alight_api::{ApiState, Options, router};
use alight_ingest::{
    data_api_history::{DataApiCapture, DataApiHistoryProvider, HistoryFuture},
    wallet_history::HistoryCapture,
};
use alight_store::Store;
use alight_types::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};
use tower::ServiceExt;

struct Recorded {
    calls: Arc<AtomicU32>,
    entry: DataApiCapture,
    fail: bool,
}
impl DataApiHistoryProvider for Recorded {
    fn fetch<'a>(&'a self, _wallet: &'a str) -> HistoryFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                Err(alight_ingest::ProbeError::Http(402).into())
            } else {
                Ok(self.entry.clone())
            }
        })
    }
}
fn recorded(calls: Arc<AtomicU32>, fail: bool) -> Recorded {
    let f: Value = serde_json::from_str(include_str!(
        "../../../data/fixtures/data_api_wallet_history.json"
    ))
    .expect("fixture");
    let rows = f["finalized_transactions"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|tx| {
            alight_tape::rpc_history::transaction(
                &tx["result"],
                ReceiveTime {
                    clock_id: "wallet-data-api-rpc".into(),
                    mono_ns: 0,
                    wall_utc: f["captured_at_utc"].as_str().expect("time").into(),
                },
            )
            .expect("transaction")
            .expect("available")
        })
        .collect();
    Recorded {
        calls,
        fail,
        entry: DataApiCapture {
            history: HistoryCapture {
                capture: WalletHistoryCapture {
                    schema_version: 1,
                    source: Source::Replay,
                    wallet: f["wallet"].as_str().expect("wallet").into(),
                    from_utc: "2026-10-09T00:00:00Z".into(),
                    through_utc: "2026-10-09T23:59:59Z".into(),
                    tip_recipients: vec![],
                    rows,
                },
                provider_requests: 5,
                address_signatures: 2,
                outside_window: 0,
                not_fee_payer: 0,
                unavailable: 0,
            },
            trade_rows: 2,
            unique_signatures: 2,
            venue_tips_sol: "0".into(),
            fetched_at_utc: f["captured_at_utc"].as_str().expect("time").into(),
        },
    }
}
async fn get(app: &axum::Router, path: &str) -> (u16, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 1_048_576)
        .await
        .expect("body");
    (status, serde_json::from_slice(&bytes).expect("json"))
}
#[tokio::test]
async fn live_receipt_is_cached_pinnable_read_only_and_has_no_invented_failure_rate_or_frontier() {
    let dir = tempfile::tempdir().expect("dir");
    let store = Store::open(&dir.path().join("live.db"), 16 * 1024 * 1024)
        .await
        .expect("store");
    let calls = Arc::new(AtomicU32::new(0));
    let provider = recorded(calls.clone(), false);
    let wallet = provider.entry.history.capture.wallet.clone();
    let expected_fee: u64 = provider
        .entry
        .history
        .capture
        .rows
        .iter()
        .map(|r| r.transaction.fee_lamports)
        .sum();
    let app = router(
        ApiState::new(
            store.clone(),
            Options {
                wallet_history: Some(Arc::new(provider)),
                requests_per_second: 200,
                ..Default::default()
            },
        )
        .expect("state"),
    );
    let path = format!("/v1/receipt/{wallet}");
    assert_eq!(get(&app, &format!("{path}?target_p=0")).await.0, 400);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let (status, report) = get(&app, &path).await;
    assert_eq!(status, 200);
    assert_eq!(report["source"], "live");
    assert_eq!(report["transactions"], 2);
    assert_eq!(report["fees_lamports"], expected_fee.to_string());
    assert!(report["visible_failed_share"].is_null());
    assert_eq!(report["compared_transactions"], 0);
    assert!(
        report["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .all(|r| r["route"].is_null() && r["comparison"].is_null())
    );
    assert!(
        report["limits"]
            .as_array()
            .expect("limits")
            .iter()
            .any(|v| v.as_str().expect("limit").contains("Data API trade index"))
    );
    assert_eq!(get(&app, &path).await.1, report);
    assert_eq!(
        get(
            &app,
            &format!(
                "{path}?capture={}",
                report["capture_hash"].as_str().expect("hash")
            )
        )
        .await
        .1,
        report
    );
    assert_eq!(
        get(&app, &format!("{path}?capture=sha256:{}", "0".repeat(64)))
            .await
            .0,
        404
    );
    assert_eq!(
        get(&app, "/v1/receipt/11111111111111111111111111111111")
            .await
            .0,
        429
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        store
            .latest_wallet_capture(Source::Live, &wallet)
            .await
            .expect("no writes")
            .is_none()
    );
    assert_eq!(
        store.counts(Source::Live).await.expect("counts").canaries,
        0
    );
    assert_eq!(store.verify_ledger(Source::Live).await.expect("ledger"), 0);
    store.close().await;
}
#[tokio::test]
async fn access_failure_remains_unavailable_and_offline_modes_reject_network_provider_configuration()
 {
    let dir = tempfile::tempdir().expect("dir");
    let store = Store::open(&dir.path().join("failure.db"), 16 * 1024 * 1024)
        .await
        .expect("store");
    let calls = Arc::new(AtomicU32::new(0));
    let provider = recorded(calls.clone(), true);
    let wallet = provider.entry.history.capture.wallet.clone();
    assert!(
        ApiState::new(
            store.clone(),
            Options {
                mode: RunMode::Replay,
                wallet_history: Some(Arc::new(recorded(calls.clone(), false))),
                ..Default::default()
            }
        )
        .is_err()
    );
    let app = router(
        ApiState::new(
            store.clone(),
            Options {
                wallet_history: Some(Arc::new(provider)),
                ..Default::default()
            },
        )
        .expect("state"),
    );
    let (status, error) = get(&app, &format!("/v1/receipt/{wallet}")).await;
    assert_eq!(status, 503);
    assert_eq!(error["code"], "HISTORY_UNAVAILABLE");
    assert!(error.get("fees_lamports").is_none());
    assert_eq!(get(&app, &format!("/v1/receipt/{wallet}")).await.0, 429);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.counts(Source::Live).await.expect("counts").canaries,
        0
    );
    store.close().await;
}
