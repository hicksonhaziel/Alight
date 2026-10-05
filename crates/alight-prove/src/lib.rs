//! Frozen, prospective held-out experiments. Live signing stays in alight-canary.
use alight_canary::governor::{BudgetError, Governor};
use alight_store::{Store, StoreError, content_hash};
use alight_types::*;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProveError {
    #[error("invalid Prove request or unsupported forecast")]
    Invalid,
    #[error("mode cannot start this Prove run")]
    ReadOnly,
    #[error("forecast is expired, changed, or cell is already locked")]
    Conflict,
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Budget(#[from] BudgetError),
}
fn utc(s: &str) -> Result<DateTime<Utc>, ProveError> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| ProveError::Invalid)
}
pub fn methodology_hash() -> String {
    format!(
        "sha256:{:x}",
        Sha256::digest(include_bytes!("../../../docs/prove-methodology.md"))
    )
}

/// Lock one existing supported forecast. Retries retain the original timestamp and claim.
pub async fn lock(
    store: &Store,
    mode: RunMode,
    request: &ProveRequest,
    now: &str,
    simulation: Option<SimProofEnvironment>,
) -> Result<ProveReport, ProveError> {
    let source = match mode {
        RunMode::Live => Source::Live,
        RunMode::Sim => Source::Sim,
        _ => return Err(ProveError::ReadOnly),
    };
    if request.request_id.is_empty()
        || request.request_id.len() > 64
        || !request
            .request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || !(1..=400).contains(&request.n)
    {
        return Err(ProveError::Invalid);
    }
    if source == Source::Live && (request.seed.is_some() || simulation.is_some())
        || source == Source::Sim && simulation.is_none()
    {
        return Err(ProveError::Invalid);
    }
    if let Some(env) = &simulation {
        alight_sim::Parameters {
            canaries: 1,
            slot_ms: env.slot_ms,
            congestion: env.congestion,
            tip_slope: env.tip_slope,
            fee_slope: env.fee_slope,
            never_land_mass: env.never_land_mass,
            continuous_latency: env.continuous_latency,
            ..Default::default()
        }
        .validate()
        .map_err(|_| ProveError::Invalid)?;
    }
    let id = format!(
        "prove-{}-{}",
        alight_store::label(source)?,
        request.request_id
    );
    if let Some(previous) = store.prove(source, &id).await? {
        if previous.lock.forecast_hash != request.forecast_hash
            || previous.lock.n != request.n
            || previous.lock.seed
                != if source == Source::Sim {
                    Some(request.seed.unwrap_or(42))
                } else {
                    None
                }
        {
            return Err(ProveError::Conflict);
        }
        return Ok(previous);
    }
    let entry = store
        .forecast(source, &request.forecast_hash)
        .await?
        .ok_or(ProveError::Invalid)?;
    if entry.hash != request.forecast_hash
        || utc(&entry.forecast.expires_at_utc)? <= utc(now)?
        || utc(&entry.forecast.created_at_utc)? > utc(now)?
    {
        return Err(ProveError::Conflict);
    }
    let recommendation = entry
        .forecast
        .quote
        .recommendation
        .as_ref()
        .ok_or(ProveError::Invalid)?;
    if !matches!(
        recommendation.evidence,
        Evidence::Measured | Evidence::Interpolated
    ) || !recommendation.p_hat.is_finite()
        || !(0.0..=1.0).contains(&recommendation.p_hat)
        || recommendation.n_effective < 30.0
    {
        return Err(ProveError::Invalid);
    }
    if recommendation.config.route == Route::Rpc && recommendation.config.tip_lamports != 0
        || recommendation.config.route != Route::Rpc && recommendation.config.tip_lamports < 100000
        || recommendation.config.cu_limit
            != alight_canary::policy::cu_limit(recommendation.config.size_class)
    {
        return Err(ProveError::Invalid);
    }
    let claimed_probability = match entry.forecast.request.target {
        PredictionTarget::Probability { .. } => recommendation.p_hat,
        PredictionTarget::LatencyQuantile { quantile, .. } => quantile,
    };
    let lock = ProveLock {
        id,
        source,
        forecast_hash: entry.hash,
        model_snapshot_hash: entry.forecast.model_snapshot_hash,
        methodology_hash: methodology_hash(),
        config: recommendation.config.clone(),
        regime_id: entry.forecast.regime_id,
        region: entry.forecast.request.context.region,
        locked_at_utc: now.into(),
        expires_at_utc: entry.forecast.expires_at_utc,
        target: entry.forecast.request.target,
        claimed_probability,
        n: request.n,
        seed: if source == Source::Sim {
            Some(request.seed.unwrap_or(42))
        } else {
            None
        },
        simulation,
    };
    match store.lock_prove(&lock).await {
        Ok(report) => Ok(report),
        Err(error) => {
            if let Some(previous) = store.prove(source, &lock.id).await?
                && previous.lock.forecast_hash == lock.forecast_hash
                && previous.lock.n == lock.n
                && previous.lock.seed == lock.seed
            {
                return Ok(previous);
            }
            if matches!(error, StoreError::Invalid)
                || matches!(&error,StoreError::Database(sqlx_error) if sqlx_error.as_database_error().is_some_and(|e|e.is_unique_violation()))
            {
                return Err(ProveError::Conflict);
            }
            Err(error.into())
        }
    }
}

/// Wilson 95% interval for k successes out of n; no interval for an empty denominator.
pub fn wilson(k: u32, n: u32) -> Option<[f64; 2]> {
    if n == 0 || k > n {
        return None;
    }
    let z = 1.959963984540054;
    let z2 = z * z;
    let p = f64::from(k) / f64::from(n);
    let n = f64::from(n);
    let center = (p + z2 / (2.0 * n)) / (1.0 + z2 / n);
    let half = z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / (1.0 + z2 / n);
    Some([(center - half).max(0.0), (center + half).min(1.0)])
}
fn success(
    sample: &TrainingCanary,
    lock: &ProveLock,
    now: &str,
) -> Result<Option<bool>, ProveError> {
    let c = &sample.canary;
    if c.source != lock.source
        || c.config != lock.config
        || c.regime_id != lock.regime_id
        || utc(&c.send_wall_utc)? <= utc(&lock.locked_at_utc)?
        || utc(&c.send_wall_utc)? > utc(&lock.expires_at_utc)?
    {
        return Err(ProveError::Invalid);
    }
    if !sample.finalized
        || c.resolved_at_utc
            .as_deref()
            .map(utc)
            .transpose()?
            .is_none_or(|t| utc(now).map_or(true, |n| t > n))
    {
        return Ok(None);
    }
    match c.outcome {
        Some(Outcome::Expired | Outcome::Rejected | Outcome::LandedThenDropped) => Ok(Some(false)),
        Some(Outcome::LandedOk | Outcome::LandedFailed) => {
            let slots = c
                .landed_slot
                .and_then(|s| s.checked_sub(c.sent_slot))
                .ok_or(ProveError::Invalid)?;
            Ok(match lock.target {
                PredictionTarget::Probability { horizon_slots, .. } => {
                    Some(slots <= u64::from(horizon_slots))
                }
                PredictionTarget::LatencyQuantile {
                    max_slots: Some(max),
                    ..
                } => Some(slots as f64 <= max),
                PredictionTarget::LatencyQuantile {
                    max_ms: Some(max), ..
                } => c
                    .observer_first_seen
                    .values()
                    .filter(|t| t.clock_id == c.clock_id && t.mono_ns >= c.send_mono_ns)
                    .map(|t| (t.mono_ns - c.send_mono_ns) as f64 / 1e6)
                    .min_by(f64::total_cmp)
                    .map(|ms| ms <= max),
                _ => return Err(ProveError::Invalid),
            })
        }
        _ => Ok(None),
    }
}
/// Grades only membership-linked outcomes; expiry and missing clocks never create failures.
pub fn grade(
    lock: &ProveLock,
    samples: &[TrainingCanary],
    now: &str,
    current_regime: &str,
) -> Result<ProveReport, ProveError> {
    if samples.len() > lock.n as usize {
        return Err(ProveError::Invalid);
    }
    utc(now)?;
    let mut ids = std::collections::BTreeSet::new();
    let mut resolved = 0;
    let mut successes = 0;
    for s in samples {
        if !ids.insert(&s.canary.id) {
            return Err(ProveError::Invalid);
        }
        if let Some(ok) = success(s, lock, now)? {
            resolved += 1;
            successes += u32::from(ok);
        }
    }
    let attempts = samples.len() as u32;
    let interval = wilson(successes, resolved);
    let complete = attempts == lock.n && resolved == lock.n;
    let shifted = current_regime != lock.regime_id;
    let expired = utc(now)? > utc(&lock.expires_at_utc)?;
    let verdict = if shifted || !complete || lock.n < 30 {
        ProveVerdict::Inconclusive
    } else if interval
        .is_some_and(|[lo, hi]| lo <= lock.claimed_probability && lock.claimed_probability <= hi)
    {
        ProveVerdict::Consistent
    } else {
        ProveVerdict::Inconsistent
    };
    let reason = if shifted {
        Some("regime_changed".into())
    } else if lock.n < 30 {
        Some("requested_n_below_30".into())
    } else if !complete {
        Some(
            if expired {
                "expired_with_partial_or_unresolved_evidence"
            } else {
                "awaiting_held_out_outcomes"
            }
            .into(),
        )
    } else {
        None
    };
    Ok(ProveReport {
        lock: lock.clone(),
        lock_hash: content_hash(lock)?,
        as_of_utc: now.into(),
        state: if shifted {
            ProveState::Voided
        } else if complete || expired {
            ProveState::Complete
        } else {
            ProveState::Running
        },
        attempts,
        resolved,
        successes,
        unresolved: attempts - resolved,
        observed_rate: (resolved > 0).then(|| f64::from(successes) / f64::from(resolved)),
        wilson_interval_95: interval,
        verdict,
        reason,
    })
}
pub async fn refresh(
    store: &Store,
    report: &ProveReport,
    now: &str,
    regime: &str,
) -> Result<ProveReport, ProveError> {
    let samples = store
        .prove_canaries(report.lock.source, &report.lock.id)
        .await?;
    let next = grade(
        &report.lock,
        &samples,
        now,
        if report.state == ProveState::Voided {
            "permanently_voided"
        } else {
            regime
        },
    )?;
    store.save_prove_report(&next).await?;
    Ok(next)
}

/// Runs prospective Sim attempts with the same durable caps; an interrupted run resumes
/// membership already saved instead of duplicating any attempt or reservation.
pub async fn run_sim(
    store: &Store,
    report: &ProveReport,
    daily: Option<&str>,
    burst: Option<&str>,
) -> Result<ProveReport, ProveError> {
    if report.lock.source != Source::Sim {
        return Err(ProveError::ReadOnly);
    }
    if report.state == ProveState::Voided
        || report.state == ProveState::Complete && report.attempts < report.lock.n
    {
        return Ok(report.clone());
    }
    let existing = store.prove_canaries(Source::Sim, &report.lock.id).await?;
    let mut latest = utc(&report.as_of_utc)?;
    for sample in &existing {
        latest = latest.max(utc(sample
            .canary
            .resolved_at_utc
            .as_deref()
            .ok_or(ProveError::Invalid)?)?);
    }
    if existing.len() >= report.lock.n as usize {
        return refresh(store, report, &latest.to_rfc3339(), &report.lock.regime_id).await;
    }
    let governor = Governor::new(store.clone(), RunMode::Sim, daily, burst, 60000)?;
    let cost = u64::try_from(
        (u128::from(report.lock.config.cu_price_micro_lamports)
            * u128::from(report.lock.config.cu_limit))
        .div_ceil(1_000_000)
            + u128::from(report.lock.config.tip_lamports)
            + 5000,
    )
    .map_err(|_| ProveError::Invalid)?;
    for ordinal in existing.len() as u32..report.lock.n {
        let sample =
            alight_sim::simulate_locked(&report.lock, ordinal).map_err(|_| ProveError::Invalid)?;
        let sent = utc(&sample.canary.send_wall_utc)?;
        if sent > utc(&report.lock.expires_at_utc)? {
            latest =
                latest.max(utc(&report.lock.expires_at_utc)? + chrono::Duration::milliseconds(1));
            break;
        }
        let millis = u64::try_from(sent.timestamp_millis()).map_err(|_| ProveError::Invalid)?;
        let permit = if let Some(p) = governor
            .resume_sim(&sample.canary.id, sample.canary.config.route, cost)
            .await?
        {
            Ok(p)
        } else {
            governor
                .reserve(&sample.canary.id, sample.canary.config.route, cost, millis)
                .await
        };
        match permit {
            Ok(permit) => {
                if permit.consume().source != Source::Sim {
                    return Err(ProveError::Invalid);
                }
            }
            Err(BudgetError::Denied) => {
                let mut next =
                    refresh(store, report, &latest.to_rfc3339(), &report.lock.regime_id).await?;
                next.state = ProveState::BudgetCapped;
                next.reason = Some("durable_daily_or_burst_cap".into());
                store.save_prove_report(&next).await?;
                return Ok(next);
            }
            Err(e) => return Err(e.into()),
        }
        store
            .save_prove_simulated(&report.lock, ordinal, &sample)
            .await?;
        latest = latest.max(utc(sample
            .canary
            .resolved_at_utc
            .as_deref()
            .ok_or(ProveError::Invalid)?)?);
    }
    refresh(store, report, &latest.to_rfc3339(), &report.lock.regime_id).await
}
