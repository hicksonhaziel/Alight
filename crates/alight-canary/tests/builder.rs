use alight_canary::{builder::Wallet, policy::Policy};
use alight_types::*;
use base64::Engine;
use serde_json::Value;
use solami::{Keypair, Transaction, VersionedTransaction};
#[test]
fn all_81_wire_goldens_match_assignment_fields_and_account_costs() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../data/fixtures/canary_wire_goldens.json"
    ))
    .expect("goldens");
    let rows = fixture["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 81);
    let wallet = Wallet::from_keypair(Keypair::new_from_array([1; 32]));
    for row in rows {
        let config: CanaryConfig =
            serde_json::from_value(row["config"].clone()).expect("assignment");
        let tip = if config.route == Route::Rpc {
            None
        } else {
            Some(solami::TIP_ACCOUNTS[0])
        };
        let message = wallet
            .message(
                &config,
                "golden-canary-v1",
                &bs58::encode([2u8; 32]).into_string(),
                tip,
            )
            .expect("build");
        let bytes = bincode::serialize(&VersionedTransaction::from(Transaction::new_unsigned(
            message.clone(),
        )))
        .expect("serialize");
        let expected = base64::engine::general_purpose::STANDARD
            .decode(row["unsigned_base64"].as_str().expect("bytes"))
            .expect("base64");
        assert_eq!(bytes, expected);
        assert!(bytes.len() <= 1232);
        assert_eq!(
            &message.instructions[0].data[1..],
            config.cu_limit.to_le_bytes()
        );
        assert_eq!(
            &message.instructions[1].data[1..],
            config.cu_price_micro_lamports.to_le_bytes()
        );
        let writable =
            message.account_keys.len() - message.header.num_readonly_unsigned_accounts as usize;
        let expected = 1
            + usize::from(tip.is_some())
            + if config.size_class == SizeClass::Large {
                4
            } else {
                0
            };
        assert_eq!(writable, expected);
        if tip.is_some() {
            assert_eq!(
                &message.instructions[3].data[4..12],
                config.tip_lamports.to_le_bytes()
            );
        }
    }
    let config = &Policy::new(1, 0.30).expect("policy").cells()[0].clone();
    assert!(
        wallet
            .message(
                config,
                "contains spaces",
                &bs58::encode([2u8; 32]).into_string(),
                Some(solami::TIP_ACCOUNTS[0])
            )
            .is_err()
    );
}
