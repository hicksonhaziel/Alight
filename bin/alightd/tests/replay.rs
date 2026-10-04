use std::process::Command;

#[test]
fn offline_replay_survives_restart_and_deduplicates_without_env() {
    let dir = tempfile::tempdir().expect("dir");
    let db = dir.path().join("replay.db");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/fixtures/grpc_transactions_sample.jsonl");
    for _ in 0..2 {
        let output = Command::new(env!("CARGO_BIN_EXE_alightd"))
            .env_clear()
            .current_dir(dir.path())
            .args(["--mode", "replay", "--replay"])
            .arg(&fixture)
            .arg("--db")
            .arg(&db)
            .output()
            .expect("daemon");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("report");
        assert_eq!(report["mode"], "replay");
        assert_eq!(report["network_requests"], 0);
        assert_eq!(report["canaries_sent"], 0);
        assert_eq!(report["counts"]["observations"], 13);
        assert_eq!(report["counts"]["blocks"], 4);
    }
}
