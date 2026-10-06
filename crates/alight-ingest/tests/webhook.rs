use alight_ingest::webhook;
use alight_types::*;
use hmac::{Hmac, Mac};
use sha2::Sha256;
#[test]
fn actual_captured_enriched_payload_verifies_exact_bytes_and_preserves_missing_identity() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../data/fixtures/webhook_sample.json"))
            .expect("capture");
    let body = serde_json::to_vec(&fixture["deliveries"][0]["data"]).expect("body");
    let secret = b"local-offline-fixture-secret-only";
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("key");
    mac.update(&body);
    let signature = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert!(webhook::verify(secret, &signature, &body));
    let mut altered = body.clone();
    altered.push(b' ');
    assert!(!webhook::verify(secret, &signature, &altered));
    assert!(!webhook::verify(
        secret,
        &format!("sha256={signature}"),
        &body
    ));
    assert!(!webhook::verify(
        b"different-offline-key",
        &signature,
        &body
    ));
    let received = ReceiveTime {
        clock_id: "fixture".into(),
        mono_ns: 123,
        wall_utc: "2026-10-04T00:23:09.058338Z".into(),
    };
    let (event, raw) = webhook::normalize(&body, Source::Replay, received).expect("normalize");
    assert_eq!(event.slot, Some(453093637));
    assert_eq!(event.success, Some(true));
    assert!(event.block_id.is_none());
    assert!(event.index_in_block.is_none());
    assert_eq!(event.index_scope, IndexScope::Unknown);
    assert_eq!(event.source, Source::Replay);
    assert!(raw.get("events").is_none());
    assert!(raw.get("trader").is_none());
}
