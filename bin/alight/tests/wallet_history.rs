use std::process::Command;

#[test]
fn rpc_history_rejects_invalid_scope_before_any_provider_request_or_output() {
    let dir = tempfile::tempdir().expect("dir");
    let recipients = dir.path().join("recipients.json");
    let output = dir.path().join("capture.json");
    std::fs::write(&recipients, "[]").expect("recipients");
    let wallet = serde_json::from_str::<serde_json::Value>(include_str!(
        "../../../data/fixtures/rpc_wallet_transaction.json"
    ))
    .expect("fixture")["response"]["result"]["transaction"]["message"]["accountKeys"][0]
        .as_str()
        .expect("payer")
        .to_owned();
    for (wallet, limit, through) in [
        ("invalid-wallet", "4", "2026-10-04T23:59:59Z"),
        (wallet.as_str(), "33", "2026-10-04T23:59:59Z"),
        (wallet.as_str(), "4", "2026-10-06T23:59:59Z"),
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_alight"))
            .current_dir(dir.path())
            .env_clear()
            .args([
                "receipt",
                "fetch-rpc",
                "--wallet",
                wallet,
                "--recipients",
                recipients.to_str().expect("path"),
                "--from",
                "2026-10-04T00:00:00Z",
                "--through",
                through,
                "--limit",
                limit,
                "--output",
                output.to_str().expect("path"),
            ])
            .output()
            .expect("capture command");
        assert_eq!(result.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&result.stderr).contains("invalid wallet history"));
        assert!(!output.exists());
    }
}
