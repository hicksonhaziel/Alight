//! Explicit fixture regeneration with an unfunded, public test seed. Never reads ENV or signs.
use alight_canary::{builder::Wallet, policy::Policy};
use alight_types::{FeeBucket, Route};
use base64::Engine;
use serde_json::json;
use solami::{Keypair, Transaction, VersionedTransaction};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wallet = Wallet::from_keypair(Keypair::new_from_array([1; 32]));
    let policy = Policy::new(1, 0.30)?;
    let mut rows = Vec::new();
    for cell in policy.cells() {
        let mut config = cell.clone();
        config.cu_price_micro_lamports = match config.fee_bucket {
            FeeBucket::Zero => 0,
            FeeBucket::LocalMedian => 100,
            FeeBucket::LocalP90 => 900,
        };
        let tip = if config.route == Route::Rpc {
            None
        } else {
            Some(solami::TIP_ACCOUNTS[0])
        };
        let message = wallet.message(
            &config,
            "golden-canary-v1",
            &bs58::encode([2u8; 32]).into_string(),
            tip,
        )?;
        let bytes = bincode::serialize(&VersionedTransaction::from(Transaction::new_unsigned(
            message,
        )))?;
        rows.push(json!({"config":config,"unsigned_base64":base64::engine::general_purpose::STANDARD.encode(bytes)}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"source":"sim","id":"golden-canary-v1","rows":rows}))?
    );
    Ok(())
}
