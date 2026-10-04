//! Version 1 wire contracts established during Phase 0; implementations follow later.
mod contracts;
pub use contracts::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Live,
    Sim,
    Replay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    BeamQuic,
    BeamHttp,
    Rpc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Outcome {
    LandedOk,
    LandedFailed,
    LandedThenDropped,
    Expired,
    Rejected,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    Pass,
    Fail,
    Inconclusive,
    Missing,
}

/// Expiry evidence uses block heights, not slot counts or wall-clock timeouts.
#[derive(Debug, Clone, Copy)]
pub struct ExpiryProof {
    pub last_valid_block_height: u64,
    pub observed_block_height: u64,
    pub absent_at_required_commitment: bool,
}

impl ExpiryProof {
    /// Returns true only after validity ended and commitment-level absence was checked.
    pub fn is_expired(self) -> bool {
        self.observed_block_height > self.last_valid_block_height
            && self.absent_at_required_commitment
    }
}

/// A chain-time window with inclusive observed endpoints; timestamps are Unix seconds.
#[derive(Debug, Clone, Copy)]
pub struct SlotWindow {
    pub start_slot: u64,
    pub end_slot: u64,
    pub start_unix_s: i64,
    pub end_unix_s: i64,
}

impl SlotWindow {
    /// Mean milliseconds per slot over a window, including skipped slots in the denominator.
    /// Coarse block timestamps require a long window and do not measure first-seen latency.
    pub fn mean_slot_ms(self) -> Option<f64> {
        let slots = self.end_slot.checked_sub(self.start_slot)?;
        let seconds = self.end_unix_s.checked_sub(self.start_unix_s)?;
        if slots == 0 || seconds <= 0 {
            return None;
        }
        Some(seconds as f64 * 1000.0 / slots as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_needs_both_strict_height_and_absence() {
        for (height, absent, expected) in [
            (99, true, false),
            (100, true, false),
            (101, false, false),
            (101, true, true),
        ] {
            assert_eq!(
                ExpiryProof {
                    last_valid_block_height: 100,
                    observed_block_height: height,
                    absent_at_required_commitment: absent
                }
                .is_expired(),
                expected
            );
        }
    }

    #[test]
    fn clock_uses_slot_distance_including_skips() {
        let window = SlotWindow {
            start_slot: 100,
            end_slot: 112,
            start_unix_s: 10,
            end_unix_s: 13,
        };
        assert_eq!(window.mean_slot_ms(), Some(250.0));
        assert_eq!(
            SlotWindow {
                end_slot: 100,
                ..window
            }
            .mean_slot_ms(),
            None
        );
        assert_eq!(
            SlotWindow {
                end_unix_s: 9,
                ..window
            }
            .mean_slot_ms(),
            None
        );
        assert_eq!(
            SlotWindow {
                start_slot: 113,
                ..window
            }
            .mean_slot_ms(),
            None
        );
    }
}
