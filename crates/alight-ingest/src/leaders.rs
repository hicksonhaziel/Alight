//! Epoch schedules with measured, epoch-scoped stake and skip-rate classes.
use crate::{Config, HttpProbe, ProbeError};
use alight_types::{LeaderClass, Tercile};
use base64::Engine;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Write;

pub struct LeaderSchedule {
    pub epoch: u64,
    pub first_slot: u64,
    pub slots_in_epoch: u64,
    slots: BTreeMap<u64, String>,
    classes: BTreeMap<String, LeaderClass>,
    pub raw: Value,
}

fn tercile(value: f64, values: &[f64]) -> Tercile {
    if values.is_empty() {
        return Tercile::Unknown;
    }
    if values.first() == values.last() {
        return Tercile::Middle;
    }
    if value <= values[(values.len() - 1) / 3] {
        Tercile::Low
    } else if value <= values[2 * (values.len() - 1) / 3] {
        Tercile::Middle
    } else {
        Tercile::High
    }
}

impl LeaderSchedule {
    /// Lossless compressed provider recording; fits the store's bounded evidence cap.
    pub fn evidence(&self) -> Result<Value, ProbeError> {
        let bytes = serde_json::to_vec(&self.raw).map_err(|_| ProbeError::Format)?;
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(&bytes).map_err(|_| ProbeError::Format)?;
        let compressed = gzip.finish().map_err(|_| ProbeError::Format)?;
        Ok(
            json!({"kind":"epoch_leaders","epoch":self.epoch.to_string(),"encoding":"gzip+base64","body":base64::engine::general_purpose::STANDARD.encode(compressed)}),
        )
    }
    /// Requires a complete schedule; absent metrics remain explicitly unknown.
    pub fn from_rpc(raw: Value) -> Result<Self, ProbeError> {
        let info = &raw["epoch_info"];
        let epoch = info["epoch"].as_u64().ok_or(ProbeError::Format)?;
        let absolute = info["absoluteSlot"].as_u64().ok_or(ProbeError::Format)?;
        let index = info["slotIndex"].as_u64().ok_or(ProbeError::Format)?;
        let first_slot = absolute.checked_sub(index).ok_or(ProbeError::Format)?;
        let slots_in_epoch = info["slotsInEpoch"]
            .as_u64()
            .filter(|s| *s > 0 && *s <= 1_000_000)
            .ok_or(ProbeError::Format)?;
        let schedule = raw["getLeaderSchedule"]
            .as_object()
            .ok_or(ProbeError::Format)?;
        let mut slots = BTreeMap::new();
        for (identity, relative_slots) in schedule {
            if bs58::decode(identity)
                .into_vec()
                .map_or(true, |b| b.len() != 32)
            {
                return Err(ProbeError::Format);
            }
            for relative in relative_slots.as_array().ok_or(ProbeError::Format)? {
                let relative = relative
                    .as_u64()
                    .filter(|s| *s < slots_in_epoch)
                    .ok_or(ProbeError::Format)?;
                let slot = first_slot.checked_add(relative).ok_or(ProbeError::Format)?;
                if slots.insert(slot, identity.clone()).is_some() {
                    return Err(ProbeError::Format);
                }
            }
        }
        if slots.len() as u64 != slots_in_epoch {
            return Err(ProbeError::Format);
        }
        let mut stakes = BTreeMap::<String, u64>::new();
        for group in ["current", "delinquent"] {
            for vote in raw["getVoteAccounts"][group]
                .as_array()
                .into_iter()
                .flatten()
            {
                let identity = vote["nodePubkey"].as_str().ok_or(ProbeError::Format)?;
                let stake = vote["activatedStake"].as_u64().ok_or(ProbeError::Format)?;
                let sum = stakes.entry(identity.into()).or_default();
                *sum = sum.checked_add(stake).ok_or(ProbeError::Format)?;
            }
        }
        let mut skips = BTreeMap::new();
        let production = &raw["getBlockProduction"]["value"];
        // A snapshot from another epoch must not classify the current schedule.
        if production["range"]["firstSlot"].as_u64() == Some(first_slot)
            && production["range"]["lastSlot"]
                .as_u64()
                .is_some_and(|s| s >= first_slot && s <= absolute)
        {
            for (identity, counts) in production["byIdentity"].as_object().into_iter().flatten() {
                let assigned = counts[0].as_u64().ok_or(ProbeError::Format)?;
                let produced = counts[1]
                    .as_u64()
                    .filter(|v| *v <= assigned)
                    .ok_or(ProbeError::Format)?;
                if assigned >= 16 {
                    skips.insert(
                        identity.clone(),
                        (assigned - produced) as f64 / assigned as f64,
                    );
                }
            }
        }
        let mut stake_values: Vec<f64> = stakes
            .iter()
            .filter(|(k, _)| schedule.contains_key(*k))
            .map(|(_, s)| *s as f64)
            .collect();
        let mut skip_values: Vec<f64> = skips
            .iter()
            .filter(|(k, _)| schedule.contains_key(*k))
            .map(|(_, s)| *s)
            .collect();
        stake_values.sort_by(f64::total_cmp);
        skip_values.sort_by(f64::total_cmp);
        let classes = schedule
            .keys()
            .map(|identity| {
                (
                    identity.clone(),
                    LeaderClass {
                        leader: identity.clone(),
                        stake_tercile: stakes
                            .get(identity)
                            .map_or(Tercile::Unknown, |s| tercile(*s as f64, &stake_values)),
                        skip_rate_tercile: skips
                            .get(identity)
                            .map_or(Tercile::Unknown, |s| tercile(*s, &skip_values)),
                    },
                )
            })
            .collect();
        Ok(Self {
            epoch,
            first_slot,
            slots_in_epoch,
            slots,
            classes,
            raw,
        })
    }

    pub fn covers(&self, slot: u64) -> bool {
        self.slots.contains_key(&slot)
    }
    /// Next n assigned slots after the observed cursor; clipped to the recorded epoch.
    pub fn next_slots(&self, slot: u64, n: usize) -> Vec<alight_types::LeaderSlot> {
        self.slots
            .range(slot.saturating_add(1)..)
            .take(n.min(8))
            .filter_map(|(slot, identity)| {
                self.classes
                    .get(identity)
                    .map(|class| alight_types::LeaderSlot {
                        slot: *slot,
                        class: class.clone(),
                    })
            })
            .collect()
    }

    /// Next n leader rotations strictly after slot, clipped at the recorded epoch boundary.
    pub fn next_leaders(&self, slot: u64, n: usize) -> Vec<LeaderClass> {
        let mut result = Vec::new();
        let mut previous = self.slots.get(&slot);
        for (_, identity) in self.slots.range(slot.saturating_add(1)..) {
            if previous != Some(identity) {
                if result.len() == n {
                    break;
                }
                if let Some(class) = self.classes.get(identity) {
                    result.push(class.clone());
                }
                previous = Some(identity);
            }
        }
        result
    }

    pub async fn fetch(config: &Config) -> Result<Self, ProbeError> {
        let http = HttpProbe::new()?;
        let info = http
            .rpc(config, "getEpochInfo", json!([{"commitment":"finalized"}]))
            .await?;
        let start = info["absoluteSlot"]
            .as_u64()
            .and_then(|s| s.checked_sub(info["slotIndex"].as_u64()?))
            .ok_or(ProbeError::Format)?;
        let schedule = http
            .rpc(
                config,
                "getLeaderSchedule",
                json!([start,{"commitment":"finalized"}]),
            )
            .await?;
        let votes = http
            .rpc(
                config,
                "getVoteAccounts",
                json!([{"commitment":"finalized"}]),
            )
            .await?;
        let production = http.rpc(config,"getBlockProduction",json!([{"commitment":"finalized","range":{"firstSlot":start,"lastSlot":info["absoluteSlot"]}}])).await?;
        Self::from_rpc(
            json!({"epoch_info":info,"getLeaderSchedule":schedule,"getVoteAccounts":votes,"getBlockProduction":production}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recorded_epoch_has_exact_slot_mapping_and_epoch_scoped_classes() {
        let gzip = flate2::read::GzDecoder::new(
            include_bytes!("../../../data/fixtures/leader_schedule.json.gz").as_slice(),
        );
        let raw: Value = serde_json::from_reader(gzip).expect("recording");
        let schedule = LeaderSchedule::from_rpc(raw.clone()).expect("schedule");
        let first_identity = raw["getLeaderSchedule"]
            .as_object()
            .expect("map")
            .iter()
            .find(|(_, s)| s.as_array().expect("slots").contains(&json!(0)))
            .expect("first leader")
            .0;
        assert_eq!(schedule.slots[&schedule.first_slot], *first_identity);
        assert!(schedule.covers(schedule.first_slot + schedule.slots_in_epoch - 1));
        assert!(!schedule.covers(schedule.first_slot + schedule.slots_in_epoch));
        let runway = schedule.next_slots(schedule.first_slot, 8);
        assert_eq!(runway.len(), 8);
        for (i, row) in runway.iter().enumerate() {
            assert_eq!(row.slot, schedule.first_slot + i as u64 + 1);
            assert_eq!(row.class.leader, schedule.slots[&row.slot]);
            assert!(serde_json::to_value(row).expect("wire")["slot"].is_string());
        }
        assert_eq!(
            schedule
                .next_slots(schedule.first_slot + schedule.slots_in_epoch - 2, 8)
                .len(),
            1
        );
        assert_eq!(schedule.next_leaders(schedule.first_slot, 3).len(), 3);
        let mut no_metrics = raw;
        no_metrics["getVoteAccounts"] = Value::Null;
        no_metrics["getBlockProduction"] = Value::Null;
        let schedule = LeaderSchedule::from_rpc(no_metrics).expect("unknown metrics");
        assert!(
            schedule
                .next_leaders(schedule.first_slot, 3)
                .iter()
                .all(|c| c.stake_tercile == Tercile::Unknown
                    && c.skip_rate_tercile == Tercile::Unknown)
        );
    }
}
