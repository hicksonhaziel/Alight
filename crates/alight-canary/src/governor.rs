use alight_store::{Store, StoreError};
use alight_types::{BudgetLimits, BudgetReservation, Route, RunMode, Source};
use chrono::{DateTime, Utc};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BudgetError {
    #[error("mode does not permit budget reservations")]
    ReadOnly,
    #[error("budget cap, duplicate reservation, or backward clock rejected")]
    Denied,
    #[error("invalid budget configuration or reservation")]
    Invalid,
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Exact SOL decimal -> lamports conversion; sub-lamport amounts are rejected.
pub fn sol_to_lamports(text: &str) -> Result<u64, BudgetError> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > 9
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(BudgetError::Invalid);
    }
    let whole: u64 = whole.parse().map_err(|_| BudgetError::Invalid)?;
    let fraction: u64 = if fraction.is_empty() {
        0
    } else {
        fraction.parse().map_err(|_| BudgetError::Invalid)?
    };
    whole
        .checked_mul(1_000_000_000)
        .and_then(|v| {
            v.checked_add(
                fraction * 10u64.pow(9 - (text.split_once('.').map_or(0, |(_, s)| s.len()) as u32)),
            )
        })
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or(BudgetError::Invalid)
}

pub struct Governor {
    store: Store,
    limits: BudgetLimits,
    mode: RunMode,
}
/// Non-cloneable authorization. A future sender must consume it once before signing.
pub struct Permit {
    reservation: BudgetReservation,
}
impl Permit {
    pub fn reservation(&self) -> &BudgetReservation {
        &self.reservation
    }
    pub fn consume(self) -> BudgetReservation {
        self.reservation
    }
}
impl Governor {
    /// Caps are worst-case lamports; burst_window_ms is a rolling interval in ms.
    pub fn new(
        store: Store,
        mode: RunMode,
        daily_sol: Option<&str>,
        burst_sol: Option<&str>,
        burst_window_ms: u64,
    ) -> Result<Self, BudgetError> {
        let daily = sol_to_lamports(daily_sol.unwrap_or("0.20"))?;
        let burst = sol_to_lamports(burst_sol.unwrap_or("0.05"))?;
        if daily == 0
            || burst == 0
            || burst > daily
            || burst_window_ms == 0
            || burst_window_ms > 86_400_000
        {
            return Err(BudgetError::Invalid);
        }
        Ok(Self {
            store,
            mode,
            limits: BudgetLimits {
                daily_lamports: daily,
                burst_lamports: burst,
                window_ms: burst_window_ms,
            },
        })
    }
    /// Atomically reserves worst-case lamports across every route, before signing.
    /// Unknown send results remain charged across restarts. Observe/replay always refuse.
    pub async fn reserve(
        &self,
        id: &str,
        route: Route,
        lamports: u64,
        utc_ms: u64,
    ) -> Result<Permit, BudgetError> {
        let source = match self.mode {
            RunMode::Live => Source::Live,
            RunMode::Sim => Source::Sim,
            _ => return Err(BudgetError::ReadOnly),
        };
        if id.is_empty() || id.len() > 128 {
            return Err(BudgetError::Invalid);
        }
        let ms = i64::try_from(utc_ms).map_err(|_| BudgetError::Invalid)?;
        let date = DateTime::<Utc>::from_timestamp_millis(ms).ok_or(BudgetError::Invalid)?;
        let reservation = BudgetReservation {
            id: id.into(),
            source,
            route,
            day: date.format("%Y-%m-%d").to_string(),
            created_ms: utc_ms,
            lamports,
        };
        if !self
            .store
            .reserve_budget(&reservation, &self.limits)
            .await?
        {
            return Err(BudgetError::Denied);
        }
        Ok(Permit { reservation })
    }
}
