use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn start(db: &std::path::Path, key: &std::path::Path) -> (Server, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_alightd"))
        .args([
            "--mode",
            "sim",
            "--sim-canaries",
            "900",
            "--seed",
            "42",
            "--bind",
            "127.0.0.1:0",
            "--db",
        ])
        .arg(db)
        .arg("--operator-key-file")
        .arg(key)
        .args(["--run-for", "60"])
        .env("SOLAMI_RPC_URL", "invalid-provider-url")
        .env("SOLAMI_GRPC_URL", "invalid-provider-url")
        .env(
            "ALIGHT_CANARY_KEYPAIR",
            "invalid-signing-key-must-never-be-loaded",
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("daemon");
    let mut line = String::new();
    BufReader::new(child.stdout.take().expect("stdout"))
        .read_line(&mut line)
        .expect("ready line");
    let ready: Value =
        serde_json::from_str(&line).expect("Sim starts despite poisoned provider/signing config");
    assert_eq!(ready["mode"], "sim");
    assert_eq!(ready["signing_enabled"], false);
    assert_eq!(ready["network_provider_requests"], 0);
    let base = format!("http://{}", ready["bind"].as_str().expect("bind"));
    (Server(child), base)
}
#[tokio::test]
async fn actual_sim_daemon_quotes_proves_and_preserves_clock_and_spend_on_restart() {
    let dir = tempfile::tempdir().expect("dir");
    let db = dir.path().join("sim-api.db");
    let key_path = dir.path().join("operator-key");
    let key = "daemon-local-test-operator-key-42";
    std::fs::write(&key_path, key).expect("key file");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("client");
    let (server, base) = start(&db, &key_path);
    let health: Value = client
        .get(format!("{base}/v1/health"))
        .send()
        .await
        .expect("health")
        .json()
        .await
        .expect("JSON");
    let curves: Value = client
        .get(format!("{base}/v1/curve"))
        .send()
        .await
        .expect("curves")
        .json()
        .await
        .expect("JSON");
    let candidates = curves["curves"]
        .as_array()
        .expect("curves")
        .iter()
        .filter(|c| c["horizon_slots"] == 4)
        .map(|c| c["config"].clone())
        .collect::<Vec<_>>();
    assert!(!candidates.is_empty());
    let request = json!({"model":{"context":{"source":"sim","regime_id":"sim-r0","region":"local","as_of_utc":health["as_of_utc"]},"candidates":candidates,"covariates":{"congestion":0.0},"leader_class_next":[],"target":{"kind":"probability","target_p":0.5,"horizon_slots":4}},"ttl_s":120,"economics":null,"frozen_model_hash":null});
    let response = client
        .post(format!("{base}/v1/quote"))
        .bearer_auth(key)
        .json(&request)
        .send()
        .await
        .expect("freeze");
    assert_eq!(response.status().as_u16(), 201);
    let forecast: Value = response.json().await.expect("forecast");
    assert!(forecast["forecast"]["quote"]["recommendation"].is_object());
    let prove =
        json!({"request_id":"daemon-forty","forecast_hash":forecast["hash"],"n":40,"seed":"42"});
    let response = client
        .post(format!("{base}/v1/prove"))
        .bearer_auth(key)
        .json(&prove)
        .send()
        .await
        .expect("prove");
    assert_eq!(response.status().as_u16(), 200);
    let report: Value = response.json().await.expect("report");
    assert_eq!(report["attempts"], 40);
    assert_eq!(report["resolved"], 40);
    let health: Value = client
        .get(format!("{base}/v1/health"))
        .send()
        .await
        .expect("health")
        .json()
        .await
        .expect("JSON");
    let spent = health["counts"]["budget_reserved_lamports"].clone();
    assert_ne!(spent, "0");
    assert_eq!(health["canaries_sent"], "0");
    drop(server);
    let (server, base) = start(&db, &key_path);
    let health: Value = client
        .get(format!("{base}/v1/health"))
        .send()
        .await
        .expect("health")
        .json()
        .await
        .expect("JSON");
    assert_eq!(health["as_of_utc"], report["as_of_utc"]);
    let retry: Value = client
        .post(format!("{base}/v1/prove"))
        .bearer_auth(key)
        .json(&prove)
        .send()
        .await
        .expect("retry")
        .json()
        .await
        .expect("report");
    assert_eq!(retry, report);
    let health: Value = client
        .get(format!("{base}/v1/health"))
        .send()
        .await
        .expect("health")
        .json()
        .await
        .expect("JSON");
    assert_eq!(health["counts"]["budget_reserved_lamports"], spent);
    let verified: Value = client
        .get(format!("{base}/v1/ledger/verify"))
        .send()
        .await
        .expect("verify")
        .json()
        .await
        .expect("JSON");
    assert_eq!(verified["verified"], true);
    assert_eq!(verified["entries"], "1");
    drop(server);
}
