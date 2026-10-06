use crate::extraction::quantile;
use alight_types::*;
use std::collections::{BTreeMap, BTreeSet};
/// Descriptive positions at equal paid tips and identical index scope, never route efficacy.
pub fn compare(
    canaries: &[WorkbenchCanary],
    tape: &[PassiveTip],
    source: Source,
) -> FidelityDiagnostic {
    let mut cells: BTreeMap<(u64, IndexScope), (Vec<f64>, Vec<f64>)> = BTreeMap::new();
    let owned: BTreeSet<_> = canaries
        .iter()
        .filter(|c| c.canary.source == source)
        .filter_map(|c| c.canary.signature.as_deref())
        .collect();
    for row in canaries.iter().filter(|r| {
        r.canary.source == source && r.finalized && r.canary.outcome == Some(Outcome::LandedOk)
    }) {
        let c = &row.canary;
        if let (Some(i), Some(scope)) = (
            c.landed_index,
            c.landed_index_scope.filter(|s| *s != IndexScope::Unknown),
        ) {
            cells
                .entry((c.config.tip_lamports, scope))
                .or_default()
                .0
                .push(i as f64);
        }
    }
    #[derive(Default)]
    struct Identity<'a> {
        slots: BTreeSet<u64>,
        blocks: BTreeSet<&'a str>,
        results: BTreeSet<bool>,
        positions: BTreeMap<IndexScope, BTreeSet<u64>>,
    }
    let mut identities: BTreeMap<&str, Identity<'_>> = BTreeMap::new();
    for t in tape.iter().filter(|t| t.source == source) {
        let id = identities.entry(&t.signature).or_default();
        id.slots.insert(t.slot);
        id.results.insert(t.success);
        if let Some(block) = t.block_id.as_deref() {
            id.blocks.insert(block);
        }
        if let Some(index) = t.index_in_block {
            id.positions.entry(t.index_scope).or_default().insert(index);
        }
    }
    #[derive(Default)]
    struct Payments<'a> {
        amounts: BTreeMap<&'a str, u64>,
        rows: u32,
    }
    let mut groups: BTreeMap<(&str, IndexScope, u32), Payments<'_>> = BTreeMap::new();
    let mut payment_conflicts = BTreeSet::new();
    let mut unmatched = 0;
    let mut conflicting = 0;
    for t in tape
        .iter()
        .filter(|t| t.source == source && !owned.contains(t.signature.as_str()))
    {
        if identities.get(t.signature.as_str()).is_some_and(|id| {
            id.slots.len() > 1
                || id.blocks.len() > 1
                || id.results.len() > 1
                || id.positions.values().any(|p| p.len() > 1)
        }) {
            conflicting += 1;
            continue;
        }
        let (Some(tip), Some(index)) = (
            t.tip_lamports,
            t.index_in_block.and_then(|i| u32::try_from(i).ok()),
        ) else {
            unmatched += 1;
            continue;
        };
        if !t.success || t.index_scope == IndexScope::Unknown {
            unmatched += 1;
            continue;
        }
        let group = groups
            .entry((&t.signature, t.index_scope, index))
            .or_default();
        group.rows += 1;
        if let Some(previous) = group.amounts.insert(&t.recipient, tip)
            && previous != tip
        {
            payment_conflicts.insert(t.signature.as_str());
        }
    }
    for ((signature, scope, index), payments) in groups {
        let rows = payments.rows;
        let tip = payments
            .amounts
            .values()
            .try_fold(0u64, |n, v| n.checked_add(*v));
        if payment_conflicts.contains(signature) || tip.is_none() {
            conflicting += rows;
            continue;
        }
        if let Some(cell) = tip.and_then(|tip| cells.get_mut(&(tip, scope))) {
            cell.1.push(f64::from(index));
        } else {
            unmatched += rows;
        }
    }
    let comparisons = cells
        .into_iter()
        .filter_map(|((tip_lamports, index_scope), (a, b))| {
            Some(PositionComparison {
                tip_lamports,
                index_scope,
                canaries: a.len() as u32,
                tape_transfers: b.len() as u32,
                canary_p50_index: quantile(&a, 0.5)?,
                tape_p50_index: quantile(&b, 0.5)?,
            })
        })
        .collect();
    FidelityDiagnostic {comparisons,excluded_unmatched:unmatched,excluded_conflicting:conflicting,
        limits:vec!["Equal paid tip and index scope only; workload, route, slot fullness and account contention are uncontrolled".into(),
            "Raw index is descriptive, not normalized block rank or swap performance; passive transfers never train the model".into(),
            "Owned signatures and conflicting passive candidates are excluded; absent data do not imply matching workloads".into(),
            "Tape samples total distinct observed known-recipient payments per signature and index scope; duplicate observer reports are deduplicated. Bounded query coverage may truncate a multi-recipient total".into(),
            "Canaries and tape use the same one-hour window; passive candidate finalization is not independently established by this comparison".into()]}
}
