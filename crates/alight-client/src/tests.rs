use super::*;
use axum::{Json, Router, http::StatusCode, routing::get};
use serde_json::json;
#[test]
fn rejects_endpoint_credentials_and_remote_plaintext_without_echoing_them() {
    for endpoint in [
        "http://remote.example",
        "https://user:secret@example.com",
        "https://example.com/?key=secret",
        "https://example.com/path",
    ] {
        let error = Client::new(endpoint, Source::Sim, None)
            .err()
            .expect("reject");
        assert!(!error.to_string().contains("secret"));
    }
    assert!(Client::new("http://[::1]:8080", Source::Sim, None).is_ok());
}
#[tokio::test]
async fn typed_errors_redact_messages_and_check_source_and_decimal_wire_values() {
    let app=Router::new().route("/v1/health",get(||async{(StatusCode::UNAUTHORIZED,Json(json!({"source":"sim","code":"UNAUTHORIZED","message":"private-provider-message"})))}))
        .route("/v1/clock",get(||async{Json(json!({"source":"live","method":"test","mean_slot_ms":null,"window":{"sampled_slots":0,"ambiguous_candidate_slots":0,"dead_slots":0,"candidate_parent_skipped_slots":0},"minimum_slot_distance":1,"minimum_chain_seconds":1}))}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    let client = Client::new(&endpoint, Source::Sim, None).expect("client");
    let e = client.health().await.expect_err("error");
    assert!(matches!(e, ClientError::Api { status: 401, .. }));
    assert!(!e.to_string().contains("private"));
    assert!(matches!(
        client.clock().await,
        Err(ClientError::SourceMismatch)
    ));
    assert!(matches!(
        client
            .prove(&ProveRequest {
                request_id: "test".into(),
                forecast_hash: "x".into(),
                n: 40,
                seed: None
            })
            .await,
        Err(ClientError::Configuration)
    ));
    server.abort();
    let _ = server.await;
}
