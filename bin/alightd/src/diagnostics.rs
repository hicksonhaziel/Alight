use alight_diagnostics::{extraction, process_window, refresh};
use alight_ingest::{Config, HttpProbe, stream};
use alight_store::{Store, StoreError};
use alight_types::*;
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;
/// Starts only with the explicit observe/live daemon, under its existing cancellation lifetime.
pub async fn run(
    config: Arc<Config>,
    store: Store,
    mut stop: watch::Receiver<bool>,
) -> Result<(), StoreError> {
    let probe = HttpProbe::new().map_err(|_| StoreError::Invalid)?;
    let mut expected = vec![ObserverKind::Grpc, ObserverKind::Rpc];
    if config.get("SOLAMI_MIRAGE_SUBSCRIPTION_ID").is_some() {
        expected.push(ObserverKind::Mirage);
    }
    if config.get("SOLAMI_WEBHOOK_SECRET").is_some() {
        expected.push(ObserverKind::Webhook);
    }
    let capacity = config
        .get("ALIGHT_SLOT_COMPUTE_LIMIT")
        .map(str::parse::<u64>)
        .transpose()
        .map_err(|_| StoreError::Invalid)?;
    let provenance = config.get("ALIGHT_SLOT_COMPUTE_LIMIT_PROVENANCE");
    if capacity.is_some_and(|c| c == 0) || capacity.is_some() != provenance.is_some() {
        return Err(StoreError::Invalid);
    }
    let mut timer = tokio::time::interval(Duration::from_secs(15));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut sampled = None;
    loop {
        tokio::select! {_=stop.changed()=>return Ok(()),_=timer.tick()=>{}}
        let now = stream::utc_now();
        let blocks = store
            .block_samples(ObserverKind::Grpc, Source::Live)
            .await?;
        let statuses = store.diagnostic_slot_events(Source::Live).await?;
        let sealed =
            extraction::clock_windows(&blocks, &statuses, Source::Live, SignalOrigin::Live)?;
        // One finalized full block per approximately 120 seconds. Original transaction bodies are not saved.
        let epoch = tokio::select! {_=stop.changed()=>return Ok(()),v=probe.rpc(&config,"getEpochInfo",serde_json::json!([{"commitment":"finalized"}]))=>v};
        let current_epoch = epoch.as_ref().ok().and_then(|e| e["epoch"].as_u64());
        let epoch_start = epoch
            .as_ref()
            .ok()
            .and_then(|e| e["absoluteSlot"].as_u64().zip(e["slotIndex"].as_u64()))
            .and_then(|(a, b)| a.checked_sub(b));
        if let Some(slot) = epoch.ok().and_then(|e| e["absoluteSlot"].as_u64())
            && sampled.is_none_or(|old: u64| slot.saturating_sub(old) >= 480)
        {
            sampled = Some(slot);
            let response = tokio::select! {_=stop.changed()=>return Ok(()),v=probe.rpc(&config,"getBlock",serde_json::json!([slot,{"commitment":"finalized","encoding":"json","transactionDetails":"full","rewards":false,"maxSupportedTransactionVersion":1}]))=>v};
            if let Ok(body) = response
                && let Ok(sample) =
                    extraction::sample_block(&body, Source::Live, slot, &now, capacity, provenance)
            {
                store
                    .save_block_diagnostic(Source::Live, slot, &sample.block_id, &sample)
                    .await?;
            }
        }
        let samples: Vec<BlockDiagnostic> = store.block_diagnostics(Source::Live).await?;
        let training = store.training_canaries(Source::Live).await?;
        let checkpoint = store
            .detector_checkpoint(Source::Live, SignalOrigin::Live)
            .await?;
        let last = checkpoint
            .as_ref()
            .and_then(|(_, v)| v["last_utc"].as_str())
            .map(alight_diagnostics::utc)
            .transpose()?;
        for mut w in sealed {
            // Enrichment must use evidence available at the historical window cutoff, never today's measurements.
            if store
                .diagnostic_windows(Source::Live, Some(SignalOrigin::Live), &w.through_utc, 1)
                .await?
                .iter()
                .any(|old| old.end_slot == w.end_slot)
            {
                continue;
            }
            let cutoff = alight_diagnostics::utc(&w.through_utc)?;
            if last.is_some_and(|t| cutoff <= t) {
                continue;
            }
            let fresh = (alight_diagnostics::utc(&now)? - cutoff).num_seconds() <= 120;
            w.epoch = if epoch_start.zip(w.start_slot).is_some_and(|(a, b)| b >= a) {
                current_epoch
            } else {
                None
            };
            let historical: Vec<_> = samples
                .iter()
                .filter(|s| {
                    alight_diagnostics::utc(&s.at_utc)
                        .is_ok_and(|t| t <= cutoff && (cutoff - t).num_seconds() <= 300)
                })
                .cloned()
                .collect();
            w.measures.extend(extraction::block_measures(&historical));
            w.measures.push(extraction::reference_landing(
                &training,
                Source::Live,
                &w.through_utc,
            )?);
            let cutoff_page = if fresh {
                Some(refresh(&store, Source::Live, &w.through_utc, &expected).await?)
            } else {
                None
            };
            let lags: Vec<_> = cutoff_page
                .iter()
                .flat_map(|p| &p.observers)
                .filter_map(|o| o.receive_lag_p95_ms)
                .collect();
            w.measures.push(extraction::measure(
                SignalKind::ObserverLagMs,
                if fresh {
                    extraction::quantile(&lags, 0.95)
                } else {
                    None
                },
                "ms",
                if fresh { lags.len() } else { 0 },
                "no_contemporaneous_comparable_observers",
                "P95 across observer p95 relative host receive lags; diagnostic only",
            ));
            w.id = alight_store::content_hash(&("live-enriched-v1", &w))?;
            if let Some(change) = process_window(&store, &w).await? {
                println!(
                    "{}",
                    serde_json::json!({"kind":"REGIME_DETECTED","event":change})
                );
            }
        }
        refresh(&store, Source::Live, &now, &expected).await?;
    }
}
