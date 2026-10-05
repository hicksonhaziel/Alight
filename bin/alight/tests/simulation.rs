use std::process::Command;

#[test]
fn offline_cli_replays_identical_snapshots_and_rejects_tampering() {
    let dir = tempfile::tempdir().expect("directory");
    let report = dir.path().join("report.json");
    let replay = dir.path().join("replay.json");
    let database = dir.path().join("sim.db");
    let executable = env!("CARGO_BIN_EXE_alight");
    let result = Command::new(executable)
        .args(["sim", "--canaries", "81", "--seed", "42", "--output"])
        .arg(&report)
        .arg("--database")
        .arg(&database)
        .env_remove("SOLAMI_API_KEY")
        .env_remove("ALIGHT_WALLET_PRIVATE_KEY")
        .output()
        .expect("sim");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = Command::new(executable)
        .args(["replay-model", "--input"])
        .arg(&report)
        .arg("--output")
        .arg(&replay)
        .arg("--database")
        .arg(&database)
        .output()
        .expect("replay");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        std::fs::read(&report).expect("original"),
        std::fs::read(&replay).expect("replayed bytes")
    );
    let mut data: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&report).expect("read")).expect("json");
    data["snapshots"][0]["p_hat"] = serde_json::json!(0.1234567);
    std::fs::write(&report, serde_json::to_vec(&data).expect("encode")).expect("tamper");
    let result = Command::new(executable)
        .args(["replay-model", "--input"])
        .arg(&report)
        .arg("--database")
        .arg(&database)
        .output()
        .expect("reject");
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("disagree"));
}
