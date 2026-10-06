use alight_api::{ApiState, Options, router};
use alight_store::Store;
use alight_types::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tower::ServiceExt;
fn signature(body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(b"local-webhook-api-test-secret").expect("key");
    mac.update(body);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
#[tokio::test]
async fn hmac_ingress_requires_authentication_and_owned_identity_and_remains_disabled_in_sim() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../data/fixtures/webhook_sample.json"))
            .expect("capture");
    let body = serde_json::to_vec(&fixture["deliveries"][0]["data"]).expect("body");
    let dir = tempfile::tempdir().expect("temp");
    let store = Store::open(&dir.path().join("webhook.db"), 64 * 1024 * 1024)
        .await
        .expect("store");
    let state = ApiState::new(
        store.clone(),
        Options {
            webhook_secret: Some("local-webhook-api-test-secret".into()),
            requests_per_second: 200,
            ..Default::default()
        },
    )
    .expect("observe without network");
    let app = router(state);
    let call = |bytes: Vec<u8>, sig: String| {
        app.clone().oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhook")
                .header("content-type", "application/json")
                .header("x-webhook-signature", sig)
                .body(Body::from(bytes))
                .expect("request"),
        )
    };
    let denied = call(body.clone(), "0".repeat(64)).await.expect("response");
    assert_eq!(denied.status(), 401);
    assert_eq!(
        store
            .counts(Source::Live)
            .await
            .expect("counts")
            .observations,
        0
    );
    let ignored = call(body.clone(), signature(&body)).await.expect("ignored");
    assert_eq!(ignored.status(), 200);
    let response: serde_json::Value =
        serde_json::from_slice(&to_bytes(ignored.into_body(), 65536).await.expect("body"))
            .expect("json");
    assert_eq!(response["status"], "IGNORED_UNOWNED");
    let mut c = alight_sim::generate(&alight_sim::Parameters {
        canaries: 1,
        ..Default::default()
    })
    .expect("base")
    .canaries
    .remove(0);
    c.source = Source::Live;
    c.id = "owned-fixture-canary".into();
    c.signature = fixture["deliveries"][0]["data"]["signature"]
        .as_str()
        .map(str::to_owned);
    c.send_wall_utc = "2026-10-04T00:23:00Z".into();
    c.sent_slot = 453093630;
    c.outcome = None;
    store.insert_canary(&c).await.expect("owned");
    // Ingress has a two-per-second mutation cap independent of operator access.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let accepted = call(body.clone(), signature(&body))
        .await
        .expect("accepted");
    assert_eq!(accepted.status(), 200);
    assert_eq!(
        store
            .counts(Source::Live)
            .await
            .expect("counts")
            .observations,
        1
    );
    let mut altered = body;
    altered.push(b' ');
    assert_eq!(
        call(altered, "0".repeat(64))
            .await
            .expect("reject")
            .status(),
        401
    );
    let stored = store
        .observations(Source::Live, c.signature.as_deref().expect("sig"))
        .await
        .expect("evidence");
    assert_eq!(stored.len(), 1);
    assert!(stored[0].block_id.is_none());
    assert_eq!(stored[0].observer, ObserverKind::Webhook);
    assert!(
        ApiState::new(
            store,
            Options {
                mode: RunMode::Sim,
                webhook_secret: Some("local-webhook-api-test-secret".into()),
                ..Default::default()
            }
        )
        .is_err()
    );
}
