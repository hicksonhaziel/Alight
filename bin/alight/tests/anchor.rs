#[test]
fn sending_requires_explicit_opt_in_before_loading_environment_or_opening_databases() {
    let dir = tempfile::tempdir().expect("directory");
    std::fs::write(
        dir.path().join(".env"),
        "malformed environment without equals",
    )
    .expect("file");
    for command in ["send", "run"] {
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_alight"))
            .current_dir(dir.path())
            .args([
                "anchor",
                command,
                "--database",
                "must-not-create.db",
                "--source",
                "live",
            ])
            .output()
            .expect("CLI");
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("--authorize-mainnet true"));
        assert!(!dir.path().join("must-not-create.db").exists());
    }
}
