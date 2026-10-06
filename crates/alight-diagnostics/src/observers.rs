//! Comparisons require original evidence, comparable host clocks and explicit identities.
use crate::{extraction::quantile, utc};
use alight_store::{StoreError, content_hash};
use alight_types::*;
use std::collections::{BTreeMap, BTreeSet};

fn agreement(a: &ObserverEvent, b: &ObserverEvent) -> Option<bool> {
    if a.slot.zip(b.slot).is_some_and(|(a, b)| a != b)
        || a.block_id
            .as_ref()
            .zip(b.block_id.as_ref())
            .is_some_and(|(a, b)| a != b)
        || a.success.zip(b.success).is_some_and(|(a, b)| a != b)
    {
        return Some(false);
    }
    (a.slot.is_some()
        && b.slot.is_some()
        && a.block_id.is_some()
        && b.block_id.is_some()
        && a.success.is_some()
        && b.success.is_some())
    .then_some(true)
}
#[derive(Default)]
struct Metrics {
    frames: u32,
    receive: Vec<f64>,
    send: Vec<f64>,
    missing: u32,
    conflicts: u32,
    clocks: u32,
}
pub type Comparison = (
    Vec<ObserverComparison>,
    Vec<ObserverPair>,
    Vec<DisagreementEvent>,
);
/// Lag is milliseconds on a shared monotonic clock. Missing evidence is never a terminal outcome.
pub fn compare(
    canaries: &[WorkbenchCanary],
    evidence: &[ObserverEvent],
    source: Source,
    as_of: &str,
    expected: &[ObserverKind],
) -> Result<Comparison, StoreError> {
    let now = utc(as_of)?;
    let expected: BTreeSet<_> = expected.iter().copied().collect();
    let mut metrics: BTreeMap<_, Metrics> =
        expected.iter().map(|k| (*k, Metrics::default())).collect();
    let mut pairs = BTreeMap::new();
    for &a in &expected {
        for &b in expected.range((std::ops::Bound::Excluded(a), std::ops::Bound::Unbounded)) {
            pairs.insert(
                (a, b),
                ObserverPair {
                    a,
                    b,
                    compared: 0,
                    agreed: 0,
                    disagreed: 0,
                    incomplete: 0,
                },
            );
        }
    }
    let mut events = Vec::new();
    for row in canaries.iter().filter(|r| r.canary.source == source) {
        let c = &row.canary;
        let Some(sig) = &c.signature else {
            continue;
        };
        let mut groups: BTreeMap<ObserverKind, Vec<&ObserverEvent>> = BTreeMap::new();
        for e in evidence
            .iter()
            .filter(|e| e.source == source && &e.signature == sig && expected.contains(&e.observer))
        {
            if utc(&e.received.wall_utc)? > now {
                continue;
            }
            groups.entry(e.observer).or_default().push(e);
        }
        let mut first = BTreeMap::new();
        let mut conflict = false;
        for (&kind, rows) in &mut groups {
            rows.sort_by_key(|e| {
                utc(&e.received.wall_utc)
                    .map(|t| t.timestamp_millis())
                    .unwrap_or(i64::MAX)
            });
            let e = rows[0];
            first.insert(kind, e);
            let m = metrics.entry(kind).or_default();
            m.frames += rows.len() as u32;
            if rows.iter().any(|b| agreement(e, b) == Some(false)) {
                m.conflicts += 1;
                conflict = true;
            }
            if e.received.clock_id == c.clock_id && e.received.mono_ns >= c.send_mono_ns {
                m.send
                    .push((e.received.mono_ns - c.send_mono_ns) as f64 / 1e6);
            }
        }
        for (&kind, e) in &first {
            let peers: Vec<_> = first
                .values()
                .filter(|p| p.received.clock_id == e.received.clock_id)
                .collect();
            let m = metrics.entry(kind).or_default();
            if peers.len() >= 2 {
                let earliest = peers
                    .iter()
                    .map(|p| p.received.mono_ns)
                    .min()
                    .unwrap_or(e.received.mono_ns);
                m.receive.push((e.received.mono_ns - earliest) as f64 / 1e6);
            } else {
                m.clocks += 1;
            }
        }
        for (&(a, b), pair) in &mut pairs {
            if let (Some(e), Some(f)) = (first.get(&a), first.get(&b)) {
                pair.compared += 1;
                match agreement(e, f) {
                    Some(true) => pair.agreed += 1,
                    Some(false) => {
                        pair.disagreed += 1;
                        conflict = true;
                        metrics.entry(a).or_default().conflicts += 1;
                        metrics.entry(b).or_default().conflicts += 1;
                    }
                    None => pair.incomplete += 1,
                }
            }
        }
        let grace = (now - utc(&c.send_wall_utc)?).num_seconds() >= 30;
        let eligible = grace
            && (!first.is_empty()
                || row.finalized
                    && matches!(c.outcome, Some(Outcome::LandedOk | Outcome::LandedFailed)));
        let missing: Vec<_> = expected
            .iter()
            .filter(|k| eligible && !first.contains_key(k))
            .copied()
            .collect();
        for k in &missing {
            metrics.entry(*k).or_default().missing += 1;
        }
        if conflict || !missing.is_empty() {
            let refs: Vec<_> = groups
                .values()
                .flatten()
                .map(|e| e.raw_ref.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .take(200)
                .collect();
            let kind = if conflict {
                "conflicting_candidate_or_result"
            } else {
                "missing_observer_after_grace"
            };
            events.push(DisagreementEvent {
                id: content_hash(&(source, &c.id, kind, &missing, &refs))?,
                source,
                canary_id: c.id.clone(),
                signature: sig.clone(),
                detected_at_utc: as_of.into(),
                kind: kind.into(),
                missing_observers: missing,
                evidence_refs: refs,
            });
        }
    }
    Ok((
        metrics
            .into_iter()
            .map(|(observer, m)| ObserverComparison {
                observer,
                observations: m.frames,
                comparable_lags: m.receive.len() as u32,
                receive_lag_p50_ms: quantile(&m.receive, 0.5),
                receive_lag_p95_ms: quantile(&m.receive, 0.95),
                send_to_seen_p50_ms: quantile(&m.send, 0.5),
                send_to_seen_p95_ms: quantile(&m.send, 0.95),
                missing_owned: m.missing,
                conflicts: m.conflicts,
                incomparable_clocks: m.clocks,
            })
            .collect(),
        pairs.into_values().collect(),
        events,
    ))
}
