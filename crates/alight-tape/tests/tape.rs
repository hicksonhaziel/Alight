use alight_tape::{captured_transaction, parse};
use alight_types::*;
use base64::{Engine, engine::general_purpose::STANDARD};

fn received() -> ReceiveTime {
    ReceiveTime {
        clock_id: "fixture".into(),
        mono_ns: 1,
        wall_utc: "2026-10-05T00:00:00Z".into(),
    }
}
fn transfer(outer: u32, inner: Option<u32>, amount: u64) -> PassiveInstruction {
    let mut data = 2u32.to_le_bytes().to_vec();
    data.extend(amount.to_le_bytes());
    PassiveInstruction {
        program_id_index: 0,
        accounts: vec![1, 2],
        data_base64: STANDARD.encode(data),
        outer_index: outer,
        inner_index: inner,
    }
}
fn tx() -> PassiveTransaction {
    PassiveTransaction {
        source: Source::Sim,
        observer: ObserverKind::Grpc,
        signature: bs58::encode([1u8; 64]).into_string(),
        slot: u64::MAX,
        block_id: None,
        index_in_block: Some(u64::MAX),
        index_scope: IndexScope::ProviderReported,
        success: true,
        fee_lamports: 5001,
        account_keys: vec![
            "11111111111111111111111111111111".into(),
            bs58::encode([2u8; 32]).into_string(),
            bs58::encode([3u8; 32]).into_string(),
        ],
        instructions: vec![transfer(0, None, 100_000)],
        received: received(),
    }
}
#[test]
fn real_captured_shape_preserves_lookup_keys_fees_and_provider_positions() {
    let mut transactions = Vec::new();
    for line in include_str!("../../../data/fixtures/mirage_transactions_sample.jsonl").lines() {
        let value = serde_json::from_str(line).expect("fixture JSON");
        if let Some(t) =
            captured_transaction(&value, Source::Replay, received()).expect("recorded shape")
        {
            transactions.push(t);
        }
    }
    assert_eq!(transactions.len(), 3);
    assert_eq!(transactions[0].account_keys.len(), 26);
    assert_eq!(transactions[0].fee_lamports, 41334);
    assert_eq!(transactions[1].account_keys.len(), 50);
    assert_eq!(transactions[2].account_keys.len(), 62);
    // A controlled fixture recipient tests parsing, not a published Beam tip claim.
    let recipient = transactions[1].account_keys[25].clone();
    let tips = parse(&transactions[1], &[recipient]).expect("transfer");
    assert_eq!(tips.len(), 1);
    assert_eq!(tips[0].requested_tip_lamports, 9897);
    assert_eq!(tips[0].tip_lamports, Some(9897));
    assert_eq!(tips[0].index_in_block, Some(672));
    assert_eq!(tips[0].index_scope, IndexScope::ProviderReported);
    assert_eq!(tips[0].source, Source::Replay);
    assert!(
        parse(&transactions[1], &[])
            .expect("no recipients")
            .is_empty()
    );
    for t in &transactions {
        parse(t, &[]).expect("all recorded instructions");
    }
}
#[test]
fn aggregation_failed_execution_and_inner_uncertainty_are_distinct() {
    let mut t = tx();
    t.instructions.push(transfer(1, None, 200_000));
    let recipients = vec![t.account_keys[2].clone()];
    let tip = parse(&t, &recipients).expect("outer").remove(0);
    assert_eq!(tip.tip_lamports, Some(300_000));
    let json = serde_json::to_value(&tip).expect("wire");
    assert_eq!(json["slot"], u64::MAX.to_string());
    assert_eq!(json["index_in_block"], u64::MAX.to_string());
    t.instructions.push(transfer(1, Some(0), 7));
    let tip = parse(&t, &recipients).expect("inner").remove(0);
    assert_eq!(tip.requested_tip_lamports, 300_007);
    assert_eq!(tip.tip_lamports, None);
    t.success = false;
    let tip = parse(&t, &recipients).expect("failed").remove(0);
    assert_eq!(tip.tip_lamports, Some(0));
    assert_eq!(tip.fee_lamports, 5001);
}
#[test]
fn instruction_bounds_conflicts_and_overflow_do_not_fabricate_amounts() {
    let mut t = tx();
    let recipients = vec![t.account_keys[2].clone()];
    t.instructions.push(t.instructions[0].clone());
    assert!(parse(&t, &recipients).is_err());
    t.instructions = vec![transfer(0, None, u64::MAX), transfer(1, None, 1)];
    assert!(parse(&t, &recipients).is_err());
    t.instructions = vec![transfer(0, None, 100)];
    t.instructions[0].accounts[1] = 3;
    assert!(parse(&t, &recipients).is_err());
    t.instructions[0].accounts[1] = 2;
    t.instructions[0].data_base64 = STANDARD.encode([2u8]);
    assert!(parse(&t, &recipients).is_err());
}
#[test]
fn self_transfer_preserves_intent_but_does_not_count_as_paid_tip() {
    let mut t = tx();
    t.instructions[0].accounts[0] = 2;
    let tip = parse(&t, &[t.account_keys[2].clone()])
        .expect("self transfer")
        .remove(0);
    assert_eq!(tip.requested_tip_lamports, 100000);
    assert_eq!(tip.tip_lamports, Some(0));
}
#[test]
fn seeded_transfer_and_exact_compute_budget_fields_are_supported() {
    let mut t = tx();
    let mut data = 11u32.to_le_bytes().to_vec();
    data.extend(123u64.to_le_bytes());
    data.extend(3u64.to_le_bytes());
    data.extend(b"abc");
    data.extend([7u8; 32]);
    t.instructions[0].accounts = vec![1, 1, 2];
    t.instructions[0].data_base64 = STANDARD.encode(data);
    t.account_keys
        .push("ComputeBudget111111111111111111111111111111".into());
    for (outer, data) in [
        (1, [vec![2], 25000u32.to_le_bytes().to_vec()].concat()),
        (2, [vec![3], u64::MAX.to_le_bytes().to_vec()].concat()),
    ] {
        t.instructions.push(PassiveInstruction {
            program_id_index: 3,
            accounts: vec![],
            data_base64: STANDARD.encode(data),
            outer_index: outer,
            inner_index: None,
        });
    }
    let tip = parse(&t, &[t.account_keys[2].clone()])
        .expect("seeded")
        .remove(0);
    assert_eq!(tip.tip_lamports, Some(123));
    assert_eq!(tip.cu_limit, Some(25000));
    assert_eq!(tip.cu_price_micro_lamports, Some(u64::MAX));
}
