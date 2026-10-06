//! Pure bounded historical reconstruction; network access belongs to explicit CLI jobs.
use crate::{extraction::measure, process_window, utc};
use alight_store::{Store, StoreError, content_hash};
use alight_types::*;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct History {
    pub requested_days: u32,
    pub as_of_utc: String,
    #[serde(with = "alight_types::decimal_u64")]
    pub first_available_slot: u64,
    pub requests: u32,
    pub blocks: Vec<BlockMetaEvent>,
}
/// Evenly spaced samples with a 200 ms scheduling assumption; timestamps determine actual coverage.
pub fn plan(first: u64, tip: u64, days: u32, max_requests: u32) -> Result<Vec<u64>, StoreError> {
    if !(42..=56).contains(&days) || !(2..=1024).contains(&max_requests) || first > tip {
        return Err(StoreError::Invalid);
    }
    let span = u64::from(days) * 86400 * 5;
    let start = tip.saturating_sub(span).max(first);
    let mut slots = std::collections::BTreeSet::new();
    for i in 0..max_requests {
        slots.insert(
            start
                + u64::try_from(
                    u128::from(tip - start) * u128::from(i) / u128::from(max_requests - 1),
                )
                .map_err(|_| StoreError::Invalid)?,
        );
    }
    Ok(slots.into_iter().collect())
}
/// Sparse timestamp windows are Replay/backfill evidence and cannot rename a Live regime.
pub async fn reconstruct(store: &Store, history: &History) -> Result<BackfillReport, StoreError> {
    if !(42..=56).contains(&history.requested_days)
        || history.requests > 1024
        || history.blocks.len() > 1024
    {
        return Err(StoreError::Invalid);
    }
    let now = utc(&history.as_of_utc)?;
    let mut blocks = history.blocks.clone();
    blocks.sort_by_key(|b| b.slot);
    let mut unique = std::collections::BTreeMap::new();
    for b in blocks {
        if b.source != Source::Replay || b.slot < history.first_available_slot {
            return Err(StoreError::Invalid);
        }
        unique
            .entry(b.slot)
            .and_modify(|old: &mut Option<BlockMetaEvent>| {
                if old.as_ref().is_some_and(|old| {
                    old.block_id != b.block_id || old.block_time_unix_s != b.block_time_unix_s
                }) {
                    *old = None;
                }
            })
            .or_insert(Some(b));
    }
    let missing = unique
        .values()
        .filter(|b| b.as_ref().is_none_or(|b| b.block_time_unix_s.is_none()))
        .count();
    let valid: Vec<_> = unique
        .into_values()
        .flatten()
        .filter(|b| b.block_time_unix_s.is_some())
        .collect();
    for pair in valid.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let (Some(start), Some(end)) = (a.block_time_unix_s, b.block_time_unix_s) else {
            continue;
        };
        if b.slot - a.slot < 128 || end - start < 10 {
            continue;
        }
        let Some(from) = chrono::DateTime::from_timestamp(start, 0) else {
            continue;
        };
        let Some(through) = chrono::DateTime::from_timestamp(end, 0) else {
            continue;
        };
        if through > now || from < now - chrono::Duration::days(i64::from(history.requested_days)) {
            continue;
        }
        let mut w = SignalWindow {
            id: String::new(),
            source: Source::Replay,
            origin: SignalOrigin::Backfill,
            from_utc: from.to_rfc3339(),
            through_utc: through.to_rfc3339(),
            start_slot: Some(a.slot),
            end_slot: Some(b.slot),
            epoch: None,
            measures: vec![measure(
                SignalKind::SlotMs,
                Some((end - start) as f64 * 1000.0 / (b.slot - a.slot) as f64),
                "ms/slot",
                2,
                "",
                "Sparse historical chain timestamps; interval average, bounded RPC sample",
            )],
        };
        w.id = content_hash(&w)?;
        process_window(store, &w).await?;
    }
    let stamp = |b: &BlockMetaEvent| {
        b.block_time_unix_s
            .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
            .map(|t| t.to_rfc3339())
    };
    let from = valid.first().and_then(stamp);
    let through = valid.last().and_then(stamp);
    let complete = from.as_deref().is_some_and(|f| {
        utc(f)
            .is_ok_and(|f| f <= now - chrono::Duration::days(i64::from(history.requested_days) - 1))
    }) && through
        .as_deref()
        .is_some_and(|t| utc(t).is_ok_and(|t| (now - t).num_hours() <= 24));
    let mut report=BackfillReport{id:String::new(),source:Source::Replay,requested_days:history.requested_days,
        first_available_slot:history.first_available_slot,from_utc:from,through_utc:through,
        timestamps:valid.len() as u32,missing_timestamps:(missing as u32).saturating_add(history.requests.saturating_sub(history.blocks.len() as u32)),requests:history.requests,
        status:if complete{"SAMPLED_COVERAGE"}else{"RETENTION_OR_SAMPLING_LIMIT"}.into(),
        limitation:"Sparse interval averages; sampling assumes 200 ms only to select slots. Observed timestamps establish coverage. Null timestamps and forks are excluded. Named historical steps require measured detections; no mainnet backfill has been run while collection is paused.".into()};
    report.id = content_hash(&report)?;
    store.save_backfill_report(&report).await?;
    Ok(report)
}
