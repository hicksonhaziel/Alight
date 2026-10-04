//! Corroborates an already observed public transaction; never creates a canary record.
use alight_ingest::{Config, rpc::corroborate, stream::ReceiveClock};
use alight_store::Store;
use alight_types::*;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = Config::load_observer()?;
    let store = Store::open(std::path::Path::new("data/alight.db"), 512 * 1024 * 1024).await?;
    let observation = store
        .recent_observation(ObserverKind::Grpc, Source::Live)
        .await?
        .ok_or("no observed transaction")?;
    let observations = store
        .observations(Source::Live, &observation.signature)
        .await?;
    let absent = std::env::args().skip(1).any(|s| s == "--absent");
    let signature = if absent {
        bs58::encode([0u8; 64]).into_string()
    } else {
        observation.signature.clone()
    };
    let proof = corroborate(
        &config,
        Source::Live,
        &signature,
        observation.slot.ok_or("missing slot")?,
        &observations,
        &ReceiveClock::new(),
    )
    .await?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"source":"live","read_only_probe":true,"expected_absent":absent,"canary_created":false,"canaries_sent":0,"verdict":if proof.proof.as_ref().is_some_and(|r|r.landing.is_none()==absent){"PASS"}else{"INCONCLUSIVE"},"rpc":proof.proof,"canonical_blocks":proof.canonical_blocks,"captured_fields":proof.raw})
        )?
    );
    store.close().await;
    Ok(())
}
