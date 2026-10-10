//! Offline, bounded fee-payer receipts. Neither history capture nor evaluation sends transactions.
use crate::{TapeError, parse};
use alight_store::{Store, StoreError, canonical, content_hash};
use alight_types::*;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub enum ReceiptError {
    #[error("invalid wallet capture or receipt scope")]
    Invalid,
    #[error("conflicting wallet history for one signature")]
    Conflict,
    #[error("invalid wallet transaction")]
    Transaction(#[from] TapeError),
    #[error("historical wallet evidence is unavailable")]
    Store(#[from] StoreError),
}
fn utc(time: &str) -> Result<DateTime<Utc>, ReceiptError> {
    DateTime::parse_from_rfc3339(time)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| ReceiptError::Invalid)
}
fn public_key(text: &str) -> bool {
    text.len() <= 44 && bs58::decode(text).into_vec().is_ok_and(|v| v.len() == 32)
}
fn name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b))
}
/// Validates a maximum 24-hour, 1,000-transaction partial capture; times are UTC.
/// Captured transactions must have this wallet as fee payer. Imports cannot assert Live history.
pub fn validate(capture: &WalletHistoryCapture) -> Result<(), ReceiptError> {
    let from = utc(&capture.from_utc)?;
    let through = utc(&capture.through_utc)?;
    if capture.schema_version != 1
        || capture.source == Source::Live
        || !public_key(&capture.wallet)
        || from > through
        || through - from > chrono::Duration::days(1)
        || capture.rows.len() > 1000
        || capture.tip_recipients.len() > 256
        || capture.tip_recipients.iter().any(|r| !public_key(r))
        || canonical(capture)?.len() > 4 * 1024 * 1024
    {
        return Err(ReceiptError::Invalid);
    }
    for row in &capture.rows {
        let tx = &row.transaction;
        let received = utc(&tx.received.wall_utc)?;
        if tx.source != capture.source
            || tx.account_keys.first() != Some(&capture.wallet)
            || (row.chain_time_utc.is_none() && (received < from || received > through))
            || row.regime_id.as_ref().is_some_and(|r| !name(r))
        {
            return Err(ReceiptError::Invalid);
        }
        if let Some(at) = &row.chain_time_utc {
            let time = utc(at)?;
            if time < from || time > through || time > received {
                return Err(ReceiptError::Invalid);
            }
        }
        parse(tx, &capture.tip_recipients)?;
    }
    Ok(())
}
fn identity(row: &WalletHistoryRow) -> Result<String, ReceiptError> {
    // Delivery timestamps/index scopes do not duplicate fees. Different execution evidence conflicts.
    let tx = &row.transaction;
    Ok(canonical(
        &serde_json::json!({"slot":tx.slot.to_string(),"success":tx.success,
        "fee":tx.fee_lamports.to_string(),"accounts":tx.account_keys,"instructions":tx.instructions,
        "chain_time":row.chain_time_utc,"route":row.route,"size":row.size_class,"regime":row.regime_id}),
    )?)
}
fn paid(
    capture: &WalletHistoryCapture,
    row: &WalletHistoryRow,
    tips: &[PassiveTip],
) -> Result<Option<u64>, ReceiptError> {
    if !row.transaction.success {
        return Ok(Some(0));
    }
    if capture.tip_recipients.is_empty() {
        return Ok(None);
    }
    let mut total = 0u64;
    for tip in tips {
        let Some(amount) = tip.tip_lamports else {
            return Ok(None);
        };
        // Attribution is stricter than the passive tape. A fee payer need not own CPI/other-account transfers.
        for ix in &row.transaction.instructions {
            if row
                .transaction
                .account_keys
                .get(ix.program_id_index as usize)
                .map(String::as_str)
                != Some("11111111111111111111111111111111")
            {
                continue;
            }
            if ix
                .accounts
                .last()
                .and_then(|i| row.transaction.account_keys.get(*i as usize))
                == Some(&tip.recipient)
                && ix
                    .accounts
                    .first()
                    .and_then(|i| row.transaction.account_keys.get(*i as usize))
                    != Some(&capture.wallet)
            {
                return Ok(None);
            }
        }
        total = total.checked_add(amount).ok_or(ReceiptError::Invalid)?;
    }
    Ok(Some(total))
}

/// Evaluates a replay/Sim capture against same-source historical curves. All amounts are lamports;
/// the horizon is slots and maximum evidence age is seconds. This function performs no network I/O.
pub async fn evaluate(
    store: &Store,
    capture: &WalletHistoryCapture,
    request: &WalletReceiptRequest,
) -> Result<WalletReceipt, ReceiptError> {
    validate(capture)?;
    if !name(&request.region)
        || !request.target_p.is_finite()
        || !(0.0..=1.0).contains(&request.target_p)
        || request.target_p == 0.0
        || !(1..=32).contains(&request.horizon_slots)
        || !(1..=3600).contains(&request.max_curve_age_s)
    {
        return Err(ReceiptError::Invalid);
    }
    let mut unique = BTreeMap::<String, &WalletHistoryRow>::new();
    for row in &capture.rows {
        if let Some(old) = unique.get(&row.transaction.signature) {
            if identity(old)? != identity(row)?
                || old
                    .transaction
                    .block_id
                    .as_ref()
                    .zip(row.transaction.block_id.as_ref())
                    .is_some_and(|(a, b)| a != b)
            {
                return Err(ReceiptError::Conflict);
            }
            // Keep known block identity so a delivery lacking it cannot mask later fork conflicts.
            if old.transaction.block_id.is_none() && row.transaction.block_id.is_some() {
                unique.insert(row.transaction.signature.clone(), row);
            }
        } else {
            unique.insert(row.transaction.signature.clone(), row);
        }
    }
    let count = unique.len() as u32;
    let mut report = WalletReceipt {
        schema_version: 1, source: capture.source, capture_hash: content_hash(capture)?, wallet: capture.wallet.clone(),
        from_utc: capture.from_utc.clone(), through_utc: capture.through_utc.clone(), request: request.clone(),
        population: "partial_captured_fee_payer_transactions".into(), transactions: count,
        duplicates_removed: capture.rows.len() as u32 - count, failed_transactions: 0, visible_failed_share: None,
        unknown_tip_payments: 0, compared_transactions: 0, fees_lamports: 0, known_paid_tips_lamports: 0,
        spend_above_threshold_lamports: 0, threshold_definition: "Lowest stored MEASURED tip whose lower 95% probability bound meets the requested target, with matched route/workload/fees/region/regime/horizon. This is a supported-tip threshold, not a dollar-economic knee.".into(), rows: vec![],
        limits: vec![
            "Partial captured history; unlanded submissions and traffic outside the filters are missing. This report has survivorship bias.".into(),
            "Failed share counts visible executions only; it is not the wallet's submission failure rate. Fees are charged once per signature, including failed executions.".into(),
            "Known paid tips exclude unknown inner/other-account payments. Transfer intent is not proof of executed payment.".into(),
            "Only explicitly evidenced Beam transport/workload/regime can be compared. A tip recipient does not establish the send route.".into(),
            "Historical comparisons never use future curves or today's quote; missing chain time/frontiers remain descriptive.".into(),
            "Spend above a supported tip is conditional arithmetic, not proof of waste or no probability gain. Uncontrolled wallet workloads may rationally pay more.".into(),
            "Canary curves do not establish real-swap performance; source, region, sample size, intervals and evidence age scope every comparison.".into(),
            "The Solami Data API wallet-history adapter and complete wallet coverage remain unverified.".into(),
        ],
    };
    let mut cohorts = BTreeMap::new();
    for row in unique.values() {
        let tx = &row.transaction;
        let tips = parse(tx, &capture.tip_recipients)?;
        let payment = paid(capture, row, &tips)?;
        let mut output = WalletReceiptRow {
            signature: tx.signature.clone(),
            chain_time_utc: row.chain_time_utc.clone(),
            success: tx.success,
            route: row.route,
            fee_lamports: tx.fee_lamports,
            paid_tip_lamports: payment,
            comparison: None,
            unavailable_reason: Some(
                "Missing supported historical Beam frontier or transaction context".into(),
            ),
        };
        report.fees_lamports = report
            .fees_lamports
            .checked_add(tx.fee_lamports)
            .ok_or(ReceiptError::Invalid)?;
        report.failed_transactions += u32::from(!tx.success);
        if let Some(amount) = payment {
            report.known_paid_tips_lamports = report
                .known_paid_tips_lamports
                .checked_add(amount)
                .ok_or(ReceiptError::Invalid)?;
        } else {
            report.unknown_tip_payments += 1;
        }
        if let (
            true,
            Some(amount),
            Some(at),
            Some(route @ (Route::BeamQuic | Route::BeamHttp)),
            Some(size),
            Some(regime),
            Some(tip),
        ) = (
            tx.success,
            payment,
            &row.chain_time_utc,
            row.route,
            row.size_class,
            &row.regime_id,
            tips.first(),
        ) {
            let key = (regime.clone(), at.clone());
            if !cohorts.contains_key(&key) {
                cohorts.insert(
                    key.clone(),
                    store
                        .receipt_curves(
                            capture.source,
                            regime,
                            &request.region,
                            request.horizon_slots,
                            at,
                        )
                        .await?,
                );
            }
            let curves = cohorts.get(&key).ok_or(ReceiptError::Invalid)?;
            let matched = |c: &CurveSnapshot| {
                c.config.route == route
                    && c.config.size_class == size
                    && Some(c.config.cu_limit) == tip.cu_limit
                    && Some(c.config.cu_price_micro_lamports) == tip.cu_price_micro_lamports
            };
            let mut multiplicity = BTreeMap::new();
            for (_, c) in curves {
                *multiplicity.entry(canonical(&c.config)?).or_insert(0u32) += 1;
            }
            let unambiguous = |c: &CurveSnapshot| {
                multiplicity.get(&canonical(&c.config).unwrap_or_default()) == Some(&1)
            };
            let actual_supported = curves
                .iter()
                .any(|(_, c)| matched(c) && unambiguous(c) && c.config.tip_lamports == amount);
            let mut eligible = Vec::new();
            for (id, c) in curves {
                let elapsed =
                    (utc(at)? - utc(&c.context.as_of_utc)?).num_milliseconds() as f64 / 1000.0;
                let age = c.data_age_s.map(|age| age + elapsed);
                let historical_window = c.observation_window.as_ref().is_some_and(|window| {
                    utc(&window[0])
                        .ok()
                        .zip(utc(&window[1]).ok())
                        .zip(utc(&c.context.as_of_utc).ok())
                        .is_some_and(|((start, end), as_of)| start <= end && end <= as_of)
                });
                if actual_supported
                    && unambiguous(c)
                    && historical_window
                    && matched(c)
                    && c.evidence == Evidence::Measured
                    && c.p_interval_95[0] >= request.target_p
                    && elapsed >= 0.0
                    && age.is_some_and(|age| age <= f64::from(request.max_curve_age_s))
                {
                    eligible.push((id, c, age.ok_or(ReceiptError::Invalid)?));
                }
            }
            eligible.sort_by_key(|(id, c, _)| (c.config.tip_lamports, *id));
            if let Some((id, curve, age)) = eligible.first() {
                let above = amount.saturating_sub(curve.config.tip_lamports);
                output.comparison = Some(ReceiptComparison {
                    snapshot_id: (*id).clone(),
                    snapshot: (*curve).clone(),
                    age_at_transaction_s: *age,
                    spend_above_threshold_lamports: above,
                });
                output.unavailable_reason = None;
                report.compared_transactions += 1;
                report.spend_above_threshold_lamports = report
                    .spend_above_threshold_lamports
                    .checked_add(above)
                    .ok_or(ReceiptError::Invalid)?;
            }
        }
        report.rows.push(output);
    }
    report.visible_failed_share =
        (count > 0).then(|| f64::from(report.failed_transactions) / f64::from(count));
    Ok(report)
}
