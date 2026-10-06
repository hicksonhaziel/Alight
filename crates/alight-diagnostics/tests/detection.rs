use alight_diagnostics::{extraction::measure, process_window, simulation::scenario};
use alight_model::regime::Detector;
use alight_store::Store;
use alight_types::*;
use chrono::{Duration, TimeZone, Utc};
fn window(i: u32, clock: f64, rate: f64) -> SignalWindow {
    let at = Utc
        .with_ymd_and_hms(2026, 10, 5, 0, 0, 0)
        .single()
        .expect("date")
        + Duration::seconds(i64::from(i) * 30);
    SignalWindow {
        id: format!("w-{i}"),
        source: Source::Sim,
        origin: SignalOrigin::Simulation,
        from_utc: (at - Duration::seconds(30)).to_rfc3339(),
        through_utc: at.to_rfc3339(),
        start_slot: None,
        end_slot: None,
        epoch: None,
        measures: vec![
            measure(
                SignalKind::SlotMs,
                Some(clock),
                "ms/slot",
                128,
                "",
                "synthetic",
            ),
            measure(
                SignalKind::ReferenceLandingRate,
                Some(rate),
                "fraction",
                40,
                "",
                "synthetic",
            ),
        ],
    }
}
#[test]
fn registered_stationary_false_alarm_and_restart_checks() {
    for seed in 1u64..=20 {
        let mut state = seed;
        let mut detector = Detector::default();
        for i in 0..600 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let noise = (state >> 32) as f64 / u32::MAX as f64 * 8.0 - 4.0;
            let w = window(i, 400.0 + noise, 0.95 + noise / 1000.0);
            assert!(
                detector.push(&w).expect("valid").is_empty(),
                "false alarm seed {seed} window {i}"
            );
            if i == 299 {
                detector = serde_json::from_value(serde_json::to_value(detector).expect("encode"))
                    .expect("restart");
            }
        }
    }
}
#[test]
fn clock_only_steps_are_detected_but_observer_only_changes_are_not_network_changes() {
    let mut clock = Detector::default();
    let mut observer = Detector::default();
    let mut detections = Vec::new();
    for i in 0..160 {
        let mut w = window(i, if i < 80 { 400.0 } else { 300.0 }, 0.95);
        w.measures.truncate(1);
        if !clock.push(&w).expect("valid").is_empty() {
            detections.push(i);
        }
        w.measures = vec![measure(
            SignalKind::ObserverLagMs,
            Some(if i < 80 { 10.0 } else { 100.0 }),
            "ms",
            40,
            "",
            "synthetic",
        )];
        assert!(observer.push(&w).expect("valid").is_empty());
    }
    assert_eq!(detections.len(), 1, "{detections:?}");
    assert!((80..=88).contains(&detections[0]));
    let mut foreign = window(161, 300.0, 0.95);
    foreign.source = Source::Replay;
    foreign.origin = SignalOrigin::Replay;
    assert!(clock.push(&foreign).is_err());
}
#[tokio::test]
async fn registered_shifts_persist_actions_and_restart_without_duplicate_events() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("test.db");
    let store = Store::open(&path, 64 * 1024 * 1024).await.expect("store");
    let report = scenario(&store, 42, "2026-10-05T00:00:00Z")
        .await
        .expect("scenario");
    assert_eq!(report.changes.len(), 2, "{:?}", report.changes);
    for (change, injected) in report.changes.iter().zip(&report.injected_windows) {
        let at = chrono::DateTime::parse_from_rfc3339(&change.detected_at_utc).expect("date");
        let start = chrono::DateTime::parse_from_rfc3339("2026-10-05T00:00:00Z").expect("date");
        let delay = (at - start).num_seconds() / 30 - i64::from(*injected);
        assert!((0..=8).contains(&delay), "delay={delay}");
        assert_eq!(change.old_effective_n_cap, 0.0);
        assert_eq!(change.exploration_fraction, 1.0);
        assert_eq!(
            (chrono::DateTime::parse_from_rfc3339(&change.exploration_until_utc).expect("date")
                - at)
                .num_minutes(),
            30
        );
    }
    assert_eq!(
        store
            .pending_alerts(Source::Sim)
            .await
            .expect("alerts")
            .len(),
        2
    );
    let current =
        alight_forecast::current_context(&store, Source::Sim, "local", "2026-10-05T02:00:00Z")
            .await
            .expect("context");
    assert_eq!(current.regime_id, report.changes[1].regime_id);
    let alert = store
        .pending_alerts(Source::Sim)
        .await
        .expect("alerts")
        .remove(0);
    assert!(store.claim_alert_delivery(&alert.id).await.expect("claim"));
    assert!(
        !store
            .claim_alert_delivery(&alert.id)
            .await
            .expect("duplicate claim")
    );
    assert_eq!(
        store
            .pending_alerts(Source::Sim)
            .await
            .expect("claimed excluded")
            .len(),
        1
    );
    store.close().await;
    let reopened = Store::open(&path, 64 * 1024 * 1024).await.expect("restart");
    assert!(
        scenario(&reopened, 42, "2026-10-05T00:00:00Z")
            .await
            .expect("retry")
            .changes
            .is_empty()
    );
    assert_eq!(
        reopened
            .regime_changes(Source::Sim, "2026-10-05T02:00:00Z")
            .await
            .expect("changes")
            .len(),
        2
    );
    let mut bad = window(241, f64::INFINITY, 0.95);
    assert!(process_window(&reopened, &bad).await.is_err());
    bad.measures[0].value = None;
    bad.measures[0].unavailable_reason = Some("unavailable".into());
    assert!(process_window(&reopened, &bad).await.is_ok());
}
