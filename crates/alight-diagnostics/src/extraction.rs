use alight_store::{StoreError, content_hash};
use alight_types::*;
use std::collections::{BTreeMap, BTreeSet};
/// Parses the official getBlock JSON response into bounded aggregate evidence, discarding bodies.
pub fn sample_block(
    body: &serde_json::Value,
    source: Source,
    slot: u64,
    at: &str,
    capacity: Option<u64>,
    provenance: Option<&str>,
) -> Result<BlockDiagnostic, StoreError> {
    crate::utc(at)?;
    if capacity.is_some_and(|c| c == 0)
        || capacity.is_some() != provenance.is_some()
        || provenance.is_some_and(|p| p.is_empty() || p.len() > 512)
    {
        return Err(StoreError::Invalid);
    }
    let id = body["blockhash"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .ok_or(StoreError::Invalid)?;
    let transactions = body["transactions"]
        .as_array()
        .filter(|t| t.len() <= 10000)
        .ok_or(StoreError::Invalid)?;
    let mut compute = Some(0u64);
    let mut nonvote = Some(0u32);
    for t in transactions {
        compute = compute
            .zip(t["meta"]["computeUnitsConsumed"].as_u64())
            .and_then(|(a, b)| a.checked_add(b));
        let keys = t["transaction"]["message"]["accountKeys"].as_array();
        let instructions = t["transaction"]["message"]["instructions"].as_array();
        let vote = instructions.zip(keys).and_then(|(ix, keys)| {
            let programs: Option<Vec<_>> = ix
                .iter()
                .map(|i| {
                    i["programIdIndex"]
                        .as_u64()
                        .and_then(|i| keys.get(i as usize))
                        .and_then(serde_json::Value::as_str)
                })
                .collect();
            programs.map(|ids| ids.contains(&"Vote111111111111111111111111111111111111111"))
        });
        nonvote = nonvote.zip(vote).map(|(n, v)| n + u32::from(!v));
    }
    Ok(BlockDiagnostic {
        source,
        slot,
        block_id: id.into(),
        at_utc: at.into(),
        transactions: transactions.len() as u32,
        non_vote_transactions: nonvote,
        compute_units: compute,
        compute_capacity: capacity,
        capacity_provenance: provenance.map(str::to_owned),
    })
}
pub fn quantile(values: &[f64], q: f64) -> Option<f64> {
    if values.is_empty() || !(0.0..=1.0).contains(&q) || values.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    Some(v[((v.len() - 1) as f64 * q).round() as usize])
}
pub fn measure(
    kind: SignalKind,
    value: Option<f64>,
    unit: &str,
    n: usize,
    reason: &str,
    provenance: &str,
) -> SignalMeasure {
    SignalMeasure {
        kind,
        value,
        unit: unit.into(),
        n: n as u32,
        unavailable_reason: value.is_none().then(|| reason.into()),
        provenance: provenance.into(),
    }
}
/// Non-overlapping 256-slot bins; coarse chain seconds need >=128 slots and >=10 seconds.
/// Conflicting candidate identities are excluded instead of averaged together.
pub fn clock_windows(
    blocks: &[BlockMetaEvent],
    statuses: &[SlotEvent],
    source: Source,
    origin: SignalOrigin,
) -> Result<Vec<SignalWindow>, StoreError> {
    if !origin.matches(source) {
        return Err(StoreError::Invalid);
    }
    let mut slots: BTreeMap<u64, Option<&BlockMetaEvent>> = BTreeMap::new();
    for b in blocks.iter().filter(|b| b.source == source) {
        slots
            .entry(b.slot)
            .and_modify(|old| {
                if old.is_some_and(|o| {
                    o.block_id != b.block_id || o.block_time_unix_s != b.block_time_unix_s
                }) {
                    *old = None;
                }
            })
            .or_insert(Some(b));
    }
    let last = slots.keys().next_back().copied().unwrap_or(0) / 256;
    let mut bins: BTreeMap<u64, Vec<&BlockMetaEvent>> = BTreeMap::new();
    for (slot, event) in &slots {
        if slot / 256 < last
            && let Some(b) = event
        {
            bins.entry(slot / 256).or_default().push(b);
        }
    }
    let mut out = Vec::new();
    for (bin, points) in bins {
        let valid: Vec<_> = points
            .iter()
            .filter(|b| b.block_time_unix_s.is_some())
            .copied()
            .collect();
        let Some(first) = valid.first() else {
            continue;
        };
        let Some(end) = valid.last() else {
            continue;
        };
        if end.slot - first.slot < 128 {
            continue;
        }
        let interval = SlotWindow {
            start_slot: first.slot,
            end_slot: end.slot,
            start_unix_s: first.block_time_unix_s.unwrap_or(0),
            end_unix_s: end.block_time_unix_s.unwrap_or(0),
        };
        let ms = (interval.end_unix_s - interval.start_unix_s >= 10)
            .then(|| interval.mean_slot_ms())
            .flatten();
        let mut skips = BTreeSet::new();
        for b in &points {
            if let Some(p) = b.parent_slot.filter(|p| *p < b.slot) {
                for s in p.saturating_add(1).max(bin * 256)..b.slot {
                    skips.insert(s);
                }
            }
        }
        for e in statuses
            .iter()
            .filter(|e| e.source == source && e.status == SlotStatus::Dead && e.slot / 256 == bin)
        {
            skips.insert(e.slot);
        }
        let span = end.slot - first.slot + 1;
        let skip = Some(
            skips
                .iter()
                .filter(|s| **s >= first.slot && **s <= end.slot)
                .count() as f64
                / span as f64,
        );
        let intervals: Vec<_> = valid
            .iter()
            .filter_map(|a| {
                valid
                    .iter()
                    .find(|b| b.slot >= a.slot.saturating_add(128))
                    .and_then(|b| {
                        let seconds = b.block_time_unix_s? - a.block_time_unix_s?;
                        (seconds >= 10).then(|| seconds as f64 * 1000.0 / (b.slot - a.slot) as f64)
                    })
            })
            .collect();
        let measures = vec![
            measure(
                SignalKind::SlotMs,
                ms,
                "ms/slot",
                valid.len(),
                "insufficient_chain_timestamp_span",
                "Chain timestamps over a sealed non-overlapping 256-slot bin, including skipped slots in the denominator",
            ),
            measure(
                SignalKind::SlotP95Ms,
                quantile(&intervals, 0.95),
                "ms/slot",
                intervals.len(),
                "insufficient_128_slot_subwindows",
                "P95 of overlapping >=128-slot chain-time averages, not per-slot latency; integer chain seconds",
            ),
            measure(
                SignalKind::SkipRate,
                skip,
                "fraction",
                span as usize,
                "missing_parent_evidence",
                "Explicit parent gaps and DEAD events; filtered stream coverage is not complete chain coverage",
            ),
        ];
        let mut window = SignalWindow {
            id: String::new(),
            source,
            origin,
            from_utc: first.received.wall_utc.clone(),
            through_utc: end.received.wall_utc.clone(),
            start_slot: Some(first.slot),
            end_slot: Some(end.slot),
            epoch: None,
            measures,
        };
        if crate::utc(&window.from_utc)? > crate::utc(&window.through_utc)? {
            continue;
        }
        window.id = content_hash(&window)?;
        out.push(window);
    }
    Ok(out)
}
/// Periodically sampled finalized blocks. Missing transaction meta or capacity stays unavailable.
pub fn block_measures(samples: &[BlockDiagnostic]) -> Vec<SignalMeasure> {
    let complete: Vec<_> = samples
        .iter()
        .filter(|s| {
            s.compute_units.is_some()
                && s.compute_capacity.is_some_and(|c| c > 0)
                && s.capacity_provenance
                    .as_deref()
                    .is_some_and(|p| !p.is_empty())
        })
        .collect();
    let ratios: Vec<_> = complete
        .iter()
        .filter_map(|s| Some(s.compute_units? as f64 / s.compute_capacity? as f64))
        .collect();
    let counts: Vec<_> = samples
        .iter()
        .filter(|s| s.non_vote_transactions.is_some() && s.transactions > 0)
        .collect();
    let transactions: usize = counts.iter().map(|s| s.transactions as usize).sum();
    let nonvote: u64 = counts
        .iter()
        .filter_map(|s| s.non_vote_transactions)
        .map(u64::from)
        .sum();
    vec![
        measure(
            SignalKind::BlockFullness,
            quantile(&ratios, 0.5),
            "fraction",
            complete.len(),
            "missing_complete_cu_meta_or_verified_slot_capacity",
            "Median sampled finalized-block CU consumption / configured verified per-slot capacity; sampling is not full network coverage",
        ),
        measure(
            SignalKind::NonVoteShare,
            (transactions > 0).then(|| nonvote as f64 / transactions as f64),
            "fraction",
            transactions,
            "missing_complete_transaction_instructions",
            "Sampled finalized-block non-vote / transaction count; periodic sample only",
        ),
    ]
}
/// Finalized reference cell only; unresolved observations never become known failures.
pub fn reference_landing(
    samples: &[TrainingCanary],
    source: Source,
    as_of: &str,
) -> Result<SignalMeasure, StoreError> {
    let now = crate::utc(as_of)?;
    let mut n = 0;
    let mut landed = 0;
    for s in samples {
        let c = &s.canary;
        if c.source != source
            || !s.finalized
            || c.config.route != Route::BeamHttp
            || c.config.size_class != SizeClass::Small
            || c.config.tip_lamports != 100_000
            || c.config.fee_bucket != FeeBucket::Zero
            || crate::utc(&c.send_wall_utc)? > now
            || (now - crate::utc(&c.send_wall_utc)?).num_seconds() > 300
            || c.resolved_at_utc
                .as_deref()
                .is_none_or(|t| crate::utc(t).map_or(true, |t| t > now))
        {
            continue;
        }
        if c.outcome.is_none_or(|o| o == Outcome::Unresolved) {
            continue;
        }
        n += 1;
        if matches!(c.outcome, Some(Outcome::LandedOk | Outcome::LandedFailed))
            && c.landed_slot
                .is_some_and(|slot| slot >= c.sent_slot && slot - c.sent_slot <= 2)
        {
            landed += 1;
        }
    }
    Ok(measure(
        SignalKind::ReferenceLandingRate,
        (n >= 30).then(|| landed as f64 / n as f64),
        "fraction",
        n,
        "fewer_than_30_finalized_reference_canaries",
        "Beam HTTP / small / 100000-lamport tip / zero priority / within two slots / last five minutes",
    ))
}
