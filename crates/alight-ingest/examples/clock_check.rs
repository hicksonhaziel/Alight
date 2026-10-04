use alight_ingest::{Config, rpc::check_clock};
use alight_store::Store;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = Config::load_observer()?;
    let store = Store::open(std::path::Path::new("data/alight.db"), 512 * 1024 * 1024).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&check_clock(&config, &store).await?)?
    );
    store.close().await;
    Ok(())
}
