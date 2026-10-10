use alight_tape::{receipts::validate, rpc_history::transaction};
use alight_types::*;
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str::<Value>(include_str!(
        "../../../data/fixtures/rpc_wallet_transaction.json"
    ))
    .expect("real RPC fixture")["response"]["result"]
        .clone()
}
fn received() -> ReceiveTime {
    ReceiveTime {
        clock_id: "rpc-history-test".into(),
        mono_ns: 0,
        wall_utc: "2026-10-10T00:00:00Z".into(),
    }
}
#[test]
fn real_v0_rpc_preserves_fees_and_chain_time_without_inventing_route_or_index() {
    let value = fixture();
    let row = transaction(&value, received())
        .expect("parse")
        .expect("row");
    assert_eq!(row.transaction.fee_lamports, 98880);
    assert_eq!(row.transaction.slot, 453221278);
    assert_eq!(row.transaction.source, Source::Replay);
    assert_eq!(row.transaction.observer, ObserverKind::Rpc);
    assert_eq!(row.transaction.index_scope, IndexScope::Unknown);
    assert!(row.transaction.index_in_block.is_none());
    assert!(row.route.is_none() && row.regime_id.is_none() && row.size_class.is_none());
    let loaded = &value["meta"]["loadedAddresses"];
    assert_eq!(
        row.transaction.account_keys.len(),
        value["transaction"]["message"]["accountKeys"]
            .as_array()
            .expect("keys")
            .len()
            + loaded["writable"].as_array().expect("writable").len()
            + loaded["readonly"].as_array().expect("readonly").len()
    );
    let at = chrono::DateTime::parse_from_rfc3339(row.chain_time_utc.as_ref().expect("time"))
        .expect("UTC");
    assert_eq!(at.timestamp(), 1791107490);
    let mut capture = WalletHistoryCapture {
        schema_version: 1,
        source: Source::Replay,
        wallet: row.transaction.account_keys[0].clone(),
        from_utc: "2026-10-04T00:00:00Z".into(),
        through_utc: "2026-10-04T23:59:59Z".into(),
        tip_recipients: vec![],
        rows: vec![row],
    };
    validate(&capture).expect("actual late fetch timestamp is not historical chain time");
    capture.rows[0].chain_time_utc = None;
    assert!(
        validate(&capture).is_err(),
        "no chain time cannot enter an earlier window"
    );
}

#[test]
fn null_missing_fees_unsupported_version_and_unresolved_accounts_never_become_zero_fees() {
    assert!(
        transaction(&Value::Null, received())
            .expect("null")
            .is_none()
    );
    for mutation in 0..5 {
        let mut value = fixture();
        match mutation {
            0 => {
                value["meta"].as_object_mut().expect("meta").remove("fee");
            }
            1 => value["meta"] = Value::Null,
            2 => value["version"] = json!(1),
            3 => value["transaction"]["message"]["instructions"][0]["programIdIndex"] = json!(1000),
            _ => {
                value["meta"]
                    .as_object_mut()
                    .expect("meta")
                    .remove("loadedAddresses");
                value["transaction"]["message"]["addressTableLookups"] = json!([{}]);
            }
        }
        assert!(
            transaction(&value, received()).is_err(),
            "mutation {mutation}"
        );
    }
    let mut value = fixture();
    value["meta"]["err"] = json!({"InstructionError":[0,"Custom"]});
    let row = transaction(&value, received())
        .expect("failed parse")
        .expect("row");
    assert!(!row.transaction.success);
    assert_eq!(row.transaction.fee_lamports, 98880);
}
