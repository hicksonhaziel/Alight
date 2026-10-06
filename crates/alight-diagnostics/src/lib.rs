//! Source-scoped signals, durable change actions and evidence comparisons. No network or signer.
pub mod backfill;
pub mod extraction;
pub mod fidelity;
pub mod observers;
pub mod simulation;
use alight_model::regime::Detector;
use alight_store::{Store, StoreError, content_hash};
use alight_types::*;
use chrono::{DateTime, Duration};
use serde_json::json;
pub fn utc(text: &str) -> Result<DateTime<chrono::FixedOffset>, StoreError> {
    DateTime::parse_from_rfc3339(text).map_err(|_| StoreError::Invalid)
}
/// Persists the window, detects chronologically and commits change actions without editing claims.
pub async fn process_window(
    store: &Store,
    window: &SignalWindow,
) -> Result<Option<RegimeChange>, StoreError> {
    let inserted = store.save_diagnostic_window(window).await?;
    let checkpoint = store
        .detector_checkpoint(window.source, window.origin)
        .await?;
    if checkpoint.as_ref().is_some_and(|(id, _)| id == &window.id) {
        return Ok(None);
    }
    let mut detector = checkpoint
        .as_ref()
        .map(|(_, state)| serde_json::from_value::<Detector>(state.clone()))
        .transpose()?
        .unwrap_or_default();
    if !inserted
        && detector.last_time().is_some_and(|t| {
            utc(t).is_ok_and(|t| utc(&window.through_utc).is_ok_and(|now| now <= t))
        })
    {
        return Ok(None);
    }
    let votes = detector.push(window).map_err(|_| StoreError::Invalid)?;
    let change = if votes.is_empty() {
        None
    } else {
        let old = store
            .regime_changes(window.source, &window.through_utc)
            .await?
            .into_iter()
            .find(|r| r.origin == window.origin);
        let historical = window.origin == SignalOrigin::Backfill;
        let annotation = window
            .epoch
            .map(|epoch| match old.as_ref().and_then(|r| r.epoch) {
                Some(previous) if previous != epoch => format!(
                    "Epoch {previous} → {epoch}; calendar context does not identify an upgrade"
                ),
                _ => {
                    format!("Observed epoch {epoch}; calendar context does not identify an upgrade")
                }
            });
        let prefix = match window.source {
            Source::Live => "live",
            Source::Sim => "sim",
            Source::Replay => "replay",
        };
        let id = content_hash(&(window.source, &window.id, &votes))?;
        Some(RegimeChange {
            id:id.clone(),source:window.source,origin:window.origin,
            previous_regime_id:old.map_or(if window.source==Source::Sim {"sim-r0"}else{"phase1-unclassified"}.into(),|r|r.regime_id),
            regime_id:format!("{prefix}-{}",&id[7..19]),detected_at_utc:window.through_utc.clone(),start_slot:window.end_slot,
            signal_window_id:window.id.clone(),confidence:votes.iter().map(|v|v.short_run_probability).fold(1.0,f64::min),
            annotation,epoch:window.epoch,votes,old_effective_n_cap:0.0,exploration_fraction:if historical{0.0}else{1.0},
            exploration_until_utc:(utc(&window.through_utc)?+Duration::minutes(if historical{0}else{30})).to_rfc3339(),
            policy:if historical {"phase5-history-v1: historical detection only; no active model, exploration or forecast changes"}
                else {"phase5-reset-v1: immutable old labels; zero old sample mass; 30-minute uniform exploration; existing budget caps"}.into()
        })
    };
    let mut grades = Vec::new();
    if let Some(change) = &change
        && change.origin != SignalOrigin::Backfill
    {
        let samples = store.grading_canaries(window.source).await?;
        let entries = store
            .straddling_forecasts(window.source, &change.detected_at_utc)
            .await?;
        let at = change.detected_at_utc.clone();
        grades = tokio::task::spawn_blocking(move || {
            entries
                .into_iter()
                .map(|entry| {
                    let mut grade = alight_model::scoring::grade(&entry, &samples, &at)
                        .map_err(|_| StoreError::Invalid)?;
                    grade.status = ForecastStatus::Voided;
                    grade.reason = Some("regime_changed".into());
                    grade.through_change_scores = grade.scores.take();
                    Ok(grade)
                })
                .collect::<Result<Vec<_>, StoreError>>()
        })
        .await
        .map_err(|_| StoreError::Invalid)??;
    }
    let alert = change.as_ref().map(|c| Alert {
        schema_version: 1,
        id: format!("regime-{}", c.id),
        source: c.source,
        rule: AlertRule::RegimeChange,
        subject: c.regime_id.clone(),
        at_utc: c.detected_at_utc.clone(),
        summary: if c.origin == SignalOrigin::Backfill {
            "Historical measured signal change; active models unaffected"
        } else {
            "Measured signal change: old evidence reset and bounded exploration enabled"
        }
        .into(),
        details: json!(c),
    });
    let committed = store
        .commit_detection(
            window,
            checkpoint.as_ref().map(|(id, _)| id.as_str()),
            &serde_json::to_value(detector)?,
            change.as_ref(),
            &grades,
            alert.as_ref(),
        )
        .await?;
    Ok(if committed { change } else { None })
}
/// Reads bounded original evidence; passive observations never become model training labels.
pub async fn refresh(
    store: &Store,
    source: Source,
    as_of: &str,
    expected: &[ObserverKind],
) -> Result<DiagnosticsPage, StoreError> {
    let canaries = store.workbench_canaries(source, as_of, 100).await?;
    let mut evidence = Vec::new();
    for c in &canaries {
        if let Some(sig) = &c.canary.signature {
            evidence.extend(store.workbench_observations(source, sig, as_of).await?);
        }
    }
    let (comparisons, pairs, disagreements) =
        observers::compare(&canaries, &evidence, source, as_of, expected)?;
    for event in &disagreements {
        store.save_disagreement(event).await?;
    }
    let start = (utc(as_of)? - Duration::hours(1)).to_rfc3339();
    let tape = store.fidelity_tips(source, &start, as_of, 1000).await?;
    let fidelity_canaries: Vec<_> = canaries
        .iter()
        .filter(|r| {
            utc(&r.canary.send_wall_utc).is_ok_and(|t| utc(&start).is_ok_and(|start| t >= start))
        })
        .cloned()
        .collect();
    let page=DiagnosticsPage {source,as_of_utc:as_of.into(),signals:store.diagnostic_windows(source,None,as_of,100).await?,
        regimes:store.regime_changes(source,as_of).await?,observers:comparisons,pairs,disagreements:store.disagreement_events(source,as_of).await?,
        expected_observers:expected.to_vec(),owned_window_n:canaries.len() as u32,
        fidelity:fidelity::compare(&fidelity_canaries,&tape,source),backfills:store.backfill_reports(source).await?,
        alerts:store.diagnostic_alerts(source,as_of).await?,
        limits:vec!["Latest 100 owned canaries; receive lag is relative to the earliest comparable observer on this host, not network propagation or validator independence".into(),
            "Missing evidence after 30 seconds is a diagnostic, never an EXPIRED outcome".into(),
            "Only comparable block identities establish agreement; incomplete identities and unlike index scopes remain separate".into()]};
    store.save_diagnostics(&page).await?;
    Ok(page)
}
