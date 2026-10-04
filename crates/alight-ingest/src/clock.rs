//! Rolling chain-time clock. Coarse Unix seconds require a sufficiently long window.
use alight_types::{BlockMetaEvent, SlotWindow};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct SlotClock {
    samples: BTreeMap<u64, Option<(String, i64)>>,
}
impl SlotClock {
    pub fn push(&mut self, event: &BlockMetaEvent) {
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
    }
    /// Mean ms/slot includes skipped slots and is unavailable below 64 slots / 10 chain seconds.
    pub fn mean_slot_ms(&self) -> Option<f64> {
        let mut valid = self
            .samples
            .iter()
            .filter_map(|(slot, v)| v.as_ref().map(|(_, time)| (*slot, *time)));
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
