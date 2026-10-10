use std::process::Command;
#[test]
fn offline_judge_doctor_ignores_broken_env_and_signing_configuration() {
    let dir = tempfile::tempdir().expect("dir");
    std::fs::write(dir.path().join(".env"), "invalid broken 'unterminated").expect("env");
    for mode in ["sim", "replay"] {
        let r = Command::new(env!("CARGO_BIN_EXE_alight"))
            .args(["doctor", "--mode", mode])
            .current_dir(dir.path())
            .env("ALIGHT_CANARY_KEYPAIR", "must-not-load")
            .output()
            .expect("doctor");
        assert!(r.status.success());
        let value: serde_json::Value = serde_json::from_slice(&r.stdout).expect("JSON");
        assert_eq!(value["signing_enabled"], false);
        assert_eq!(value["provider_requests"], 0);
        assert_eq!(value["canaries_sent"], 0);
    }
    let r = Command::new(env!("CARGO_BIN_EXE_alight"))
        .args(["doctor", "--mode", "sim", "--network"])
        .current_dir(dir.path())
        .output()
        .expect("doctor");
    assert_eq!(r.status.code(), Some(2));
}

#[test]
fn observe_doctor_accepts_only_read_configuration_without_loading_or_printing_keys() {
    let dir = tempfile::tempdir().expect("dir");
    let r = Command::new(env!("CARGO_BIN_EXE_alight"))
        .args(["doctor", "--mode", "observe"])
        .current_dir(dir.path())
        .env_clear()
        .env("SOLAMI_RPC_URL", "https://read-only.invalid")
        .env("SOLAMI_GRPC_URL", "https://stream.invalid")
        .env("SOLAMI_GRPC_TOKEN", "local-test-read-access")
        .env("SOLAMI_DATA_API_URL", "https://metadata.invalid")
        .env("ALIGHT_CANARY_KEYPAIR", "must-not-load-signing-identity")
        .env("SOLAMI_SWQOS_KEY", "must-not-load-route-identity")
        .output()
        .expect("doctor");
    assert!(r.status.success());
    let value: serde_json::Value = serde_json::from_slice(&r.stdout).expect("JSON");
    assert_eq!(value["status"], "CONFIGURED_READ_ONLY");
    assert_eq!(value["credentials_present"]["ALIGHT_CANARY_KEYPAIR"], false);
    assert_eq!(value["credentials_present"]["SOLAMI_SWQOS_KEY"], false);
    assert_eq!(value["signing_enabled"], false);
    assert_eq!(value["provider_requests"], 0);
    assert_eq!(value["canaries_sent"], 0);
    let output = String::from_utf8(r.stdout).expect("UTF8");
    assert!(!output.contains("local-test-read-access"));
    assert!(!output.contains("must-not-load"));
    assert!(!output.contains("https://"));
}

#[test]
fn observe_doctor_rejects_missing_metadata_and_unsupported_stream_aliases() {
    let dir = tempfile::tempdir().expect("dir");
    for (grpc_key, metadata) in [("SOLAMI_GRPC_URL", false), ("SOLAMI_STREAM_URL", true)] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_alight"));
        command
            .args(["doctor", "--mode", "observe"])
            .current_dir(dir.path())
            .env_clear()
            .env("SOLAMI_RPC_URL", "https://read-only.invalid")
            .env(grpc_key, "https://stream.invalid")
            .env("SOLAMI_GRPC_TOKEN", "local-test-read-access");
        if metadata {
            command.env("SOLAMI_DATA_API_URL", "https://metadata.invalid");
        }
        let output = command.output().expect("doctor");
        assert_eq!(output.status.code(), Some(3));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON");
        assert_eq!(value["status"], "MISSING_READ_CONFIGURATION");
        assert_eq!(value["provider_requests"], 0);
        assert_eq!(value["signing_enabled"], false);
        let missing = value["missing_configuration"].as_array().expect("missing");
        assert!(missing.contains(&serde_json::json!(if metadata {
            "SOLAMI_GRPC_URL"
        } else {
            "SOLAMI_DATA_API_URL"
        })));
    }
}
