//! Bounded, deduplicated per-pool USD-price evidence with explicit gap segments.
use crate::{EconError, positive_decimal, utc};
use alight_types::{
    BlurPool, CurveContext, DelayCostPoint, DelayCostSnapshot, MarketTrade, Source,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct SeriesSettings {
    pub window_s: u32,
    pub max_trades: usize,
    pub max_age_s: f64,
    pub minimum_pairs: u32,
    pub upper_quantile: f64,
}
impl Default for SeriesSettings {
    fn default() -> Self {
        Self {
            window_s: 300,
            max_trades: 10_000,
            max_age_s: 30.0,
            minimum_pairs: 30,
            upper_quantile: 0.90,
        }
    }
}
#[derive(Clone)]
struct Retained {
    trade: MarketTrade,
    received_ms: i64,
    segment: u64,
}
type TradeKey = (String, u32, Option<i32>);
type OrderKey = (u64, u32, u32, Option<i32>, String);
struct Price {
    value: f64,
    segment: u64,
}
pub struct PriceSeries {
    pool: String,
    mint: String,
    source: Source,
    regime_id: String,
    regime_start_slot: u64,
    settings: SeriesSettings,
    trades: BTreeMap<TradeKey, Retained>,
    order: BTreeMap<OrderKey, TradeKey>,
    segment: u64,
    disconnected: bool,
    gap_after_slot: Option<u64>,
    latest_received_ms: Option<i64>,
}
impl PriceSeries {
    /// Window/age are seconds; regime_start_slot prevents old-regime market pairs.
    pub fn new(
        pool: &BlurPool,
        context: &CurveContext,
        regime_start_slot: u64,
        settings: SeriesSettings,
    ) -> Result<Self, EconError> {
        utc(&context.as_of_utc)?;
        if pool.pool.is_empty()
            || pool.mint.is_empty()
            || context.regime_id.is_empty()
            || settings.window_s == 0
            || settings.window_s > 86400
            || !(2..=100_000).contains(&settings.max_trades)
            || settings.minimum_pairs < 2
            || settings.minimum_pairs as usize >= settings.max_trades
            || !settings.max_age_s.is_finite()
            || settings.max_age_s <= 0.0
            || !settings.upper_quantile.is_finite()
            || !(0.5..1.0).contains(&settings.upper_quantile)
        {
            return Err(EconError::Invalid);
        }
        Ok(Self {
            pool: pool.pool.clone(),
            mint: pool.mint.clone(),
            source: context.source,
            regime_id: context.regime_id.clone(),
            regime_start_slot,
            settings,
            trades: BTreeMap::new(),
            order: BTreeMap::new(),
            segment: 0,
            disconnected: false,
            gap_after_slot: None,
            latest_received_ms: None,
        })
    }
    /// Mark unavailable immediately; future pairs cannot bridge this transport gap.
    pub fn disconnect(&mut self) {
        if !self.disconnected {
            self.segment = self.segment.saturating_add(1);
            self.gap_after_slot = self.order.last_key_value().map(|(key, _)| key.0);
        }
        self.disconnected = true;
    }
    /// Ingest at its actual receive UTC time; returns false for repeats/unusable prices.
    pub fn ingest(&mut self, trade: MarketTrade, received_at_utc: &str) -> Result<bool, EconError> {
        let received = utc(received_at_utc)?;
        if trade.source != self.source
            || trade.pool != self.pool
            || trade.mint != self.mint
            || trade.signature.is_empty()
            || trade.block_time_unix_s < 0
            || trade.block_time_unix_s > received.timestamp()
        {
            return Err(EconError::Invalid);
        }
        positive_decimal(&trade.price_usd)?;
        let key = (
            trade.signature.clone(),
            trade.ix_index,
            trade.inner_ix_index,
        );
        if let Some(existing) = self.trades.get(&key) {
            if existing.trade != trade {
                return Err(EconError::Conflict);
            }
            return Ok(false);
        }
        if !trade.candle_ok
            || trade.slot < self.regime_start_slot
            || self.gap_after_slot.is_some_and(|floor| trade.slot <= floor)
            || self.latest_received_ms.is_some_and(|latest| {
                trade.block_time_unix_s.saturating_mul(1000)
                    < latest - i64::from(self.settings.window_s) * 1000
            })
        {
            return Ok(false);
        }
        let order = (
            trade.slot,
            trade.tx_index,
            trade.ix_index,
            trade.inner_ix_index,
            trade.signature.clone(),
        );
        self.latest_received_ms = Some(
            self.latest_received_ms
                .unwrap_or(received.timestamp_millis())
                .max(received.timestamp_millis()),
        );
        self.order.insert(order, key.clone());
        self.trades.insert(
            key,
            Retained {
                trade,
                received_ms: received.timestamp_millis(),
                segment: self.segment,
            },
        );
        self.disconnected = false;
        let lower = self.latest_received_ms.ok_or(EconError::Invalid)? / 1000
            - i64::from(self.settings.window_s);
        self.trades
            .retain(|_, row| row.trade.block_time_unix_s >= lower);
        self.order.retain(|_, key| self.trades.contains_key(key));
        while self.trades.len() > self.settings.max_trades {
            let (_, key) = self.order.pop_first().ok_or(EconError::Invalid)?;
            self.trades.remove(&key);
        }
        Ok(true)
    }
    /// Snapshot exact slot-distance pairs; measured_slot_ms converts observed slots to ms.
    /// Pair counts overlap and are not independent effective sample counts.
    pub fn snapshot(
        &self,
        delays_slots: &[u32],
        measured_slot_ms: f64,
        as_of_utc: &str,
    ) -> Result<DelayCostSnapshot, EconError> {
        let now = utc(as_of_utc)?;
        if delays_slots.is_empty()
            || delays_slots.len() > 1000
            || !measured_slot_ms.is_finite()
            || measured_slot_ms <= 0.0
            || measured_slot_ms > 10_000.0
        {
            return Err(EconError::Invalid);
        }
        let lower = now.timestamp() - i64::from(self.settings.window_s);
        let mut prices = BTreeMap::new();
        let mut count = 0;
        let (mut first, mut last) = (None::<i64>, None::<i64>);
        for key in self.order.values() {
            let row = self.trades.get(key).ok_or(EconError::Invalid)?;
            if row.received_ms > now.timestamp_millis()
                || row.trade.block_time_unix_s < lower
                || row.trade.block_time_unix_s > now.timestamp()
            {
                continue;
            }
            // Old segments stay bounded in memory, but never supply current economics.
            if row.segment != self.segment {
                continue;
            }
            count += 1;
            let time_ms = row
                .trade
                .block_time_unix_s
                .checked_mul(1000)
                .ok_or(EconError::Invalid)?;
            first = Some(first.map_or(time_ms, |n| n.min(time_ms)));
            last = Some(last.map_or(time_ms, |n| n.max(time_ms)));
            // Last provider-ordered observed trade per slot. No price carry-forward through missing slots.
            prices.insert(
                row.trade.slot,
                Price {
                    value: positive_decimal(&row.trade.price_usd)?,
                    segment: row.segment,
                },
            );
        }
        let mut delays = delays_slots.to_vec();
        delays.sort_unstable();
        delays.dedup();
        let mut points = Vec::new();
        for delay in delays {
            let mut returns = Vec::new();
            if delay != 0 {
                for (slot, from) in &prices {
                    if let Some(to) = slot
                        .checked_add(u64::from(delay))
                        .and_then(|slot| prices.get(&slot))
                        .filter(|to| to.segment == from.segment)
                    {
                        let value = (to.value / from.value - 1.0).abs() * 10_000.0;
                        if !value.is_finite() {
                            return Err(EconError::Invalid);
                        }
                        returns.push(value);
                    }
                }
            }
            let pairs = returns.len() as u32;
            let enough = delay == 0 || pairs >= self.settings.minimum_pairs;
            let stability = if pairs >= self.settings.minimum_pairs {
                let (early, late) = returns.split_at(returns.len() / 2);
                let a = quantile(early, 0.5)?;
                let b = quantile(late, 0.5)?;
                Some((b - a).abs() / a.max(b).max(1e-9))
            } else {
                None
            };
            points.push(DelayCostPoint {
                delay_slots: delay,
                delay_ms: f64::from(delay) * measured_slot_ms,
                median_bps: if delay == 0 {
                    Some(0.0)
                } else if enough {
                    Some(quantile(&returns, 0.5)?)
                } else {
                    None
                },
                upper_bps: if delay == 0 {
                    Some(0.0)
                } else if enough {
                    Some(quantile(&returns, self.settings.upper_quantile)?)
                } else {
                    None
                },
                pairs,
                split_half_relative_change: stability,
            });
        }
        let age = last.map(|last| (now.timestamp_millis() - last) as f64 / 1000.0);
        let sparse = prices.len() < 2 || points.iter().any(|p| p.median_bps.is_none());
        Ok(DelayCostSnapshot {
            source: self.source,
            regime_id: self.regime_id.clone(),
            pool: self.pool.clone(),
            mint: self.mint.clone(),
            as_of_utc: as_of_utc.into(),
            window_s: self.settings.window_s,
            measured_slot_ms,
            upper_quantile: self.settings.upper_quantile,
            max_age_s: self.settings.max_age_s,
            minimum_pairs: self.settings.minimum_pairs,
            observation_window: first.zip(last).map(|(a, b)| {
                [
                    chrono::DateTime::from_timestamp_millis(a)
                        .map(|t| t.to_rfc3339())
                        .unwrap_or_default(),
                    chrono::DateTime::from_timestamp_millis(b)
                        .map(|t| t.to_rfc3339())
                        .unwrap_or_default(),
                ]
            }),
            data_age_s: age,
            trade_samples: count,
            slot_samples: prices.len() as u32,
            disconnected: self.disconnected,
            stale: self.disconnected || age.is_none_or(|age| age > self.settings.max_age_s),
            sparse,
            points,
        })
    }
    /// Number of retained trades; both price storage and deduplication share this hard bound.
    pub fn retained_trades(&self) -> usize {
        self.trades.len()
    }
}
fn quantile(values: &[f64], q: f64) -> Result<f64, EconError> {
    if values.is_empty() {
        return Err(EconError::Invalid);
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    Ok(sorted[((sorted.len() as f64 * q).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1)])
}
