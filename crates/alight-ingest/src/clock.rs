//! Rolling chain-time clock. Coarse Unix seconds require a sufficiently long window.
use alight_types::{BlockMetaEvent, SlotEvent, SlotStatus, SlotWindow};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct SlotClock {
    samples: BTreeMap<u64, Option<(String, i64)>>,
    dead: BTreeSet<u64>,
    parent_skips: BTreeSet<u64>,
}
impl SlotClock {
    pub fn push(&mut self, event: &BlockMetaEvent) {
        if let Some(parent) = event.parent_slot.filter(|p| *p < event.slot) {
            for slot in parent
                .saturating_add(1)
                .max(event.slot.saturating_sub(2048))..event.slot
            {
                self.parent_skips.insert(slot);
            }
        }
        let Some(time) = event.block_time_unix_s else {
            return;
        };
        self.samples
            .entry(event.slot)
            .and_modify(|entry| {
                if entry
                    .as_ref()
                    .is_some_and(|(id, t)| id != &event.block_id || *t != time)
                {
                    *entry = None;
                }
            })
            .or_insert(Some((event.block_id.clone(), time)));
        while self.samples.len() > 2048 {
            self.samples.pop_first();
        }
        if let Some((first, _)) = self.samples.first_key_value() {
            self.dead.retain(|s| s >= first);
            self.parent_skips.retain(|s| s >= first);
        }
    }
    /// Slot-level DEAD excludes ambiguous candidate timing; it is not a transaction outcome.
    pub fn push_status(&mut self, event: &SlotEvent) {
        if event.status == SlotStatus::Dead {
            self.dead.insert(event.slot);
        }
        while self.dead.len() > 2048 {
            self.dead.pop_first();
        }
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"sampled_slots":self.samples.len(),"ambiguous_candidate_slots":self.samples.values().filter(|v|v.is_none()).count(),
            "dead_slots":self.dead.len(),"candidate_parent_skipped_slots":self.parent_skips.len()})
    }
    /// Mean ms/slot includes skipped slots and is unavailable below 64 slots / 10 chain seconds.
    pub fn mean_slot_ms(&self) -> Option<f64> {
        let mut valid = self.samples.iter().filter_map(|(slot, v)| {
            if self.dead.contains(slot) {
                None
            } else {
                v.as_ref().map(|(_, time)| (*slot, *time))
            }
        });
        let (first, first_time) = valid.next()?;
        let (last, last_time) = valid.next_back()?;
        if last - first < 64 || last_time - first_time < 10 {
            return None;
        }
        SlotWindow {
            start_slot: first,
            end_slot: last,
            start_unix_s: first_time,
            end_unix_s: last_time,
        }
        .mean_slot_ms()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_grpc_window_matches_independent_rpc_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../data/fixtures/clock_rpc_validation.json"
        ))
        .expect("fixture");
        let start: BlockMetaEvent =
            serde_json::from_value(fixture["start"].clone()).expect("start");
        let end: BlockMetaEvent = serde_json::from_value(fixture["end"].clone()).expect("end");
        let mut clock = SlotClock::default();
        clock.push(&start);
        clock.push(&end);
        let estimate = clock.mean_slot_ms().expect("window");
        let independent = fixture["rpc_mean_slot_ms"].as_f64().expect("RPC estimate");
        assert!(
            (estimate - independent).abs() <= fixture["tolerance_ms"].as_f64().expect("tolerance")
        );
        let mut fork = end.clone();
        fork.block_id = "simulated-fork".into();
        clock.push(&fork);
        assert!(clock.mean_slot_ms().is_none());
        assert_eq!(clock.summary()["ambiguous_candidate_slots"], 1);
    }
    #[test]
    fn simulated_dead_and_parent_skips_are_counted_without_creating_outcomes() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../data/fixtures/clock_rpc_validation.json"
        ))
        .expect("fixture");
        let start: BlockMetaEvent =
            serde_json::from_value(fixture["start"].clone()).expect("start");
        let mut end: BlockMetaEvent = serde_json::from_value(fixture["end"].clone()).expect("end");
        end.parent_slot = Some(end.slot - 4);
        let mut clock = SlotClock::default();
        clock.push(&start);
        clock.push(&end);
        clock.push(&end);
        assert_eq!(clock.summary()["candidate_parent_skipped_slots"], 3);
        clock.push_status(&SlotEvent {
            slot: end.slot,
            block_id: None,
            status: SlotStatus::Dead,
            received: end.received.clone(),
            leader: None,
            source: alight_types::Source::Sim,
            raw_ref: "simulation".into(),
        });
        assert_eq!(clock.summary()["dead_slots"], 1);
        assert!(clock.mean_slot_ms().is_none());
    }
}
