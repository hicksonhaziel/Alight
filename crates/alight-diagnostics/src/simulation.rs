use crate::{extraction::measure, process_window, utc};
use alight_store::{Store, StoreError, content_hash};
use alight_types::*;
use chrono::Duration;
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct ScenarioReport {
    pub source: Source,
    pub seed: String,
    pub injected_windows: Vec<u32>,
    pub signals: u32,
    pub changes: Vec<RegimeChange>,
    pub network_requests: u32,
    pub transactions_sent: u32,
}
/// Explicit synthetic signal sequence, isolated from ordinary canary training and provider evidence.
pub async fn scenario(store: &Store, seed: u64, start: &str) -> Result<ScenarioReport, StoreError> {
    let start = utc(start)?;
    let mut state = seed;
    let mut changes = Vec::new();
    for i in 0u32..240 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let noise = (state >> 32) as f64 / u32::MAX as f64 * 8.0 - 4.0;
        let (slot, landing) = if i < 80 {
            (400.0, 0.95)
        } else if i < 160 {
            (300.0, 0.70)
        } else {
            (250.0, 0.90)
        };
        let through = start + Duration::seconds(i64::from(i) * 30);
        let mut w = SignalWindow {
            id: String::new(),
            source: Source::Sim,
            origin: SignalOrigin::Simulation,
            from_utc: (through - Duration::seconds(30)).to_rfc3339(),
            through_utc: through.to_rfc3339(),
            start_slot: Some(10000 + u64::from(i) * 256),
            end_slot: Some(10255 + u64::from(i) * 256),
            epoch: Some(7 + u64::from(i) / 160),
            measures: vec![
                measure(
                    SignalKind::SlotMs,
                    Some(slot + noise),
                    "ms/slot",
                    128,
                    "",
                    "Seeded synthetic clock",
                ),
                measure(
                    SignalKind::ReferenceLandingRate,
                    Some(landing + noise / 1000.0),
                    "fraction",
                    40,
                    "",
                    "Seeded synthetic reference rate",
                ),
            ],
        };
        for (kind, unit, reason) in [
            (
                SignalKind::SlotP95Ms,
                "ms/slot",
                "scenario_does_not_model_this_signal",
            ),
            (
                SignalKind::SkipRate,
                "fraction",
                "scenario_does_not_model_this_signal",
            ),
            (
                SignalKind::BlockFullness,
                "fraction",
                "no_provider_block_sample_in_sim",
            ),
            (
                SignalKind::NonVoteShare,
                "fraction",
                "no_provider_block_sample_in_sim",
            ),
            (
                SignalKind::ObserverLagMs,
                "ms",
                "no_provider_delivery_in_sim",
            ),
        ] {
            w.measures.push(measure(
                kind,
                None,
                unit,
                0,
                reason,
                "Synthetic scenario coverage",
            ));
        }
        w.id = content_hash(&w)?;
        if let Some(c) = process_window(store, &w).await? {
            changes.push(c);
        }
    }
    Ok(ScenarioReport {
        source: Source::Sim,
        seed: seed.to_string(),
        injected_windows: vec![80, 160],
        signals: 240,
        changes,
        network_requests: 0,
        transactions_sent: 0,
    })
}
