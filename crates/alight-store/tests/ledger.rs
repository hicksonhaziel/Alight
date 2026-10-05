use alight_model::{pooled, scoring};
use alight_store::Store;
use alight_types::*;
use serde_json::json;

fn config() -> CanaryConfig {
    serde_json::from_value(json!({"route":"rpc","tip_lamports":"0","cu_price_micro_lamports":"0","cu_limit":25000,"fee_bucket":"zero","tip_tier":"none","size_class":"small"})).expect("config")
}
fn sample(id: &str, sent: &str, success: bool) -> TrainingCanary {
    let canary:Canary=serde_json::from_value(json!({"id":id,"source":"sim","config":config(),"policy_id":"uniform-fixture","assignment_prob":1.0,"uniform_arm":true,"regime_id":"r0","sent_slot":"100","clock_id":"fixture","send_mono_ns":"0","send_wall_utc":sent,"signature":null,"blockhash":"synthetic","last_valid_block_height":"116","leader_class_next":[],"outcome":if success { "LANDED_OK" } else { "EXPIRED" },"landed_slot":if success {Some("101")} else {None},"landed_block_id":null,"landed_index":null,"landed_index_scope":null,"observer_first_seen":{},"resolved_at_utc":"2026-10-05T00:03:00Z"})).expect("canary");
    TrainingCanary {
        canary,
        finalized: true,
        covariates: ModelCovariates::default(),
    }
}

#[tokio::test]
async fn ledger_restarts_detects_tamper_and_grades_only_later_final_outcomes() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("ledger.db");
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("store");
    let context = CurveContext {
        source: Source::Sim,
        regime_id: "r0".into(),
        region: "synthetic-local".into(),
        as_of_utc: "2026-10-05T00:00:00Z".into(),
    };
    let model = pooled::fit(&[], &context, 2).expect("empty model");
    let model_hash = store.save_models(&[model]).await.expect("model");
    let request = ModelQuoteRequest {
        context: context.clone(),
        candidates: vec![config()],
        covariates: ModelCovariates::default(),
        leader_class_next: vec![],
        target: PredictionTarget::Probability {
            target_p: 0.7,
            horizon_slots: 2,
        },
    };
    let quote = ModelQuote {
        contract_version: 1,
        methodology_hash: alight_model::methodology_hash(),
        context: context.clone(),
        target: request.target.clone(),
        evidence: Evidence::Measured,
        recommendation: Some(CurvePrediction {
            config: config(),
            p_hat: 0.75,
            p_interval_95: [0.2, 0.9],
            evidence: Evidence::Measured,
            n_effective: 40.0,
            data_age_s: Some(0.0),
            observation_window: Some([context.as_of_utc.clone(), context.as_of_utc.clone()]),
            unresolved_share: 0.0,
            samples_needed: None,
            m1_weight: 0.5,
        }),
        latency: None,
        samples_needed: None,
        reason: None,
    };
    let forecast = Forecast {
        id: "f1".into(),
        source: Source::Sim,
        created_at_utc: context.as_of_utc.clone(),
        expires_at_utc: "2026-10-05T00:20:00Z".into(),
        regime_id: "r0".into(),
        methodology_hash: quote.methodology_hash.clone(),
        model_snapshot_hash: model_hash,
        request,
        quote,
        baselines: vec![],
        economics: None,
        requested_economics: None,
    };
    let entry = store.append_forecast(&forecast).await.expect("append");
    assert_eq!(store.verify_ledger(Source::Sim).await.expect("verify"), 1);
    assert!(store.append_forecast(&forecast).await.is_err());
    store.close().await;
    let store = Store::open(&path, 16 * 1024 * 1024).await.expect("reopen");
    assert_eq!(
        store.verify_ledger(Source::Sim).await.expect("restarted"),
        1
    );
    let samples = [
        sample("later-yes", "2026-10-05T00:01:00Z", true),
        sample("later-no", "2026-10-05T00:02:00Z", false),
        sample("before", "2026-10-04T23:59:00Z", true),
    ];
    let grade = scoring::grade(&entry, &samples, "2026-10-05T00:30:00Z").expect("grade");
    assert_eq!(grade.status, ForecastStatus::Scored);
    let scores = grade.scores.as_ref().expect("scores");
    assert_eq!(scores.n, 2);
    assert!((scores.brier - 0.3125).abs() < 1e-12);
    assert!((scores.log_loss - 0.8369882167858358).abs() < 1e-12);
    assert_eq!(scores.interval_coverage, Some(true));
    store.save_grade(&grade).await.expect("grade write");
    assert_eq!(store.grades(Source::Sim).await.expect("grades").len(), 1);
    let mut shifted = samples.to_vec();
    shifted[1].canary.regime_id = "r1".into();
    let grade = scoring::grade(&entry, &shifted, "2026-10-05T00:30:00Z").expect("void");
    assert_eq!(grade.status, ForecastStatus::Voided);
    assert!(grade.scores.is_none());
    assert!(grade.through_change_scores.is_some());
    let connection = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .expect("sql");
    let original: String = sqlx::query_scalar("SELECT payload_json FROM model_snapshots LIMIT 1")
        .fetch_one(&connection)
        .await
        .expect("frozen model");
    sqlx::query("UPDATE model_snapshots SET payload_json='[]'")
        .execute(&connection)
        .await
        .expect("model tamper");
    assert!(store.verify_ledger(Source::Sim).await.is_err());
    sqlx::query("UPDATE model_snapshots SET payload_json=?")
        .bind(original)
        .execute(&connection)
        .await
        .expect("restore test model");
    assert_eq!(
        store
            .verify_ledger(Source::Sim)
            .await
            .expect("restored chain"),
        1
    );
    assert!(
        sqlx::query("DELETE FROM forecast_ledger")
            .execute(&connection)
            .await
            .is_err()
    );
    sqlx::query("DROP TRIGGER forecast_no_update")
        .execute(&connection)
        .await
        .expect("test tamper bypass");
    sqlx::query("UPDATE forecast_ledger SET payload_json='{}'")
        .execute(&connection)
        .await
        .expect("tamper");
    assert!(store.verify_ledger(Source::Sim).await.is_err());
    connection.close().await;
    store.close().await;
}

#[test]
fn hand_computed_reliability_brier_log_loss() {
    let scores = scoring::scores(&[(0.8, 1.0), (0.2, 0.0)], Some([0.45, 0.55]))
        .expect("scores")
        .expect("nonempty");
    assert!((scores.brier - 0.04).abs() < 1e-12);
    assert!((scores.log_loss - 0.2231435513142097).abs() < 1e-12);
    assert!((scores.expected_calibration_error - 0.2).abs() < 1e-12);
    assert_eq!(scores.reliability.len(), 2);
    assert_eq!(scores.interval_coverage, Some(true));
}
