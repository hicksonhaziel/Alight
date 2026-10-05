//! Evidence-based alerts. Durably emit once per active condition; explicit opt-in delivery.
use alight_store::{Store, content_hash};
use alight_types::*;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid alert configuration or evidence")]
    Invalid,
    #[error("Alert storage failed")]
    Store,
    #[error("Alert webhook delivery failed; it will not be automatically retried")]
    Delivery,
}
fn time(s: &str) -> Result<DateTime<chrono::FixedOffset>, Error> {
    DateTime::parse_from_rfc3339(s).map_err(|_| Error::Invalid)
}
fn key(c: &CurveSnapshot) -> Result<String, Error> {
    content_hash(&json!({"config":c.config,"horizon":c.horizon_slots})).map_err(|_| Error::Invalid)
}
fn supported(c: &CurveSnapshot, now: DateTime<chrono::FixedOffset>) -> bool {
    c.n_effective >= 30.0
        && matches!(c.evidence, Evidence::Measured | Evidence::Interpolated)
        && c.data_age_s.is_some_and(|a| a <= 300.0)
        && time(&c.context.as_of_utc).is_ok_and(|t| t <= now && (now - t).num_seconds() <= 120)
}
/// Route alert: non-overlapping 95% intervals for the same cell, minimum n=30 and fresh evidence.
/// Disagreement: >=3 distinct recent canaries. Budget: >=90% of the configured daily lamport cap.
/// Quote drift: later same-cell estimate outside the frozen interval, before expiry, in one regime.
pub fn evaluate(
    previous: &AlertState,
    input: &AlertSnapshot,
) -> Result<(AlertState, Vec<Alert>), Error> {
    let now = time(&input.as_of_utc)?;
    if previous
        .references
        .values()
        .any(|c| c.context.source != input.source)
    {
        return Err(Error::Invalid);
    }
    if input.regime_id.is_empty()
        || input.regime_id.len() > 128
        || input.curves.len() > 81
        || input.forecasts.len() > 100
        || input.curves.iter().any(|c| {
            c.context.source != input.source
                || c.context.regime_id != input.regime_id
                || c.validate().is_err()
        })
        || input
            .forecasts
            .iter()
            .any(|f| f.forecast.source != input.source)
    {
        return Err(Error::Invalid);
    }
    if previous
        .as_of_utc
        .as_deref()
        .map(time)
        .transpose()?
        .is_some_and(|t| t > now)
    {
        return Err(Error::Invalid);
    }
    let mut conditions: BTreeMap<String, (AlertRule, String, String, Value)> = BTreeMap::new();
    let mut references = if previous.regime_id.as_deref() == Some(&input.regime_id) {
        previous.references.clone()
    } else {
        BTreeMap::new()
    };
    for c in &input.curves {
        if !supported(c, now) {
            continue;
        }
        let k = key(c)?;
        let event_key = format!("route/{k}");
        if let Some(old) = references.get(&k)
            && c.context.as_of_utc != old.context.as_of_utc
            && c.p_interval_95[1] < old.p_interval_95[0]
        {
            conditions.insert(event_key.clone(),(AlertRule::RouteDegradation,k.clone(),"Canary landing estimate degraded".into(),json!({"route":c.config.route,"size_class":c.config.size_class,"previous_interval_95":old.p_interval_95,"current_interval_95":c.p_interval_95,"n_effective":c.n_effective,"regime_id":input.regime_id})));
        }
        if !conditions.contains_key(&event_key) {
            references.insert(k, c.clone());
        }
    }
    // Bound references to current observed cells; historical regimes never accumulate here.
    let present = input
        .curves
        .iter()
        .map(key)
        .collect::<Result<BTreeSet<_>, _>>()?;
    references.retain(|k, _| present.contains(k));
    if input.observer_disagreements >= 3 {
        conditions.insert("observers".into(),(AlertRule::ObserverDisagreement,"observers".into(),"Observers disagree on recent canaries".into(),json!({"distinct_canaries":input.observer_disagreements,"window_s":300,"threshold":3})));
    }
    if previous
        .regime_id
        .as_deref()
        .is_some_and(|id| id != input.regime_id)
    {
        conditions.insert(
            format!("regime/{}", input.regime_id),
            (
                AlertRule::RegimeChange,
                "regime".into(),
                "Canary regime changed".into(),
                json!({"previous_regime":previous.regime_id,"current_regime":input.regime_id}),
            ),
        );
    }
    for f in &input.forecasts {
        let forecast = &f.forecast;
        if forecast.regime_id != input.regime_id || time(&forecast.expires_at_utc)? < now {
            continue;
        }
        let Some(pred) = forecast.quote.recommendation.as_ref() else {
            continue;
        };
        if pred.n_effective < 30.0
            || !matches!(pred.evidence, Evidence::Measured | Evidence::Interpolated)
        {
            continue;
        }
        let PredictionTarget::Probability { horizon_slots, .. } = forecast.quote.target else {
            continue;
        };
        for c in &input.curves {
            if supported(c, now)
                && c.horizon_slots == horizon_slots
                && c.config == pred.config
                && time(&c.context.as_of_utc)? > time(&forecast.created_at_utc)?
                && (c.p_hat < pred.p_interval_95[0] || c.p_hat > pred.p_interval_95[1])
            {
                conditions.insert(format!("quote/{}",f.hash),(AlertRule::QuoteDrift,f.hash.clone(),"Canary quote estimate moved outside its frozen interval".into(),json!({"forecast_hash":f.hash,"frozen_interval_95":pred.p_interval_95,"current_p_hat":c.p_hat,"n_effective":c.n_effective})));
            }
        }
    }
    if input.daily_cap_lamports > 0
        && (input.reserved_today_lamports as u128) * 10 >= (input.daily_cap_lamports as u128) * 9
    {
        let day = now.with_timezone(&Utc).format("%Y-%m-%d").to_string();
        conditions.insert(format!("budget/{day}"),(AlertRule::Budget,"daily_budget".into(),"Canary daily budget is near or at its cap".into(),json!({"reserved_lamports":input.reserved_today_lamports.to_string(),"daily_cap_lamports":input.daily_cap_lamports.to_string(),"threshold_fraction":0.9})));
    }
    let mut alerts = Vec::new();
    for (condition, (rule, subject, summary, details)) in &conditions {
        if previous.active.contains(condition) {
            continue;
        }
        let id = content_hash(
            &json!({"source":input.source,"condition":condition,"at_utc":input.as_of_utc}),
        )
        .map_err(|_| Error::Invalid)?;
        alerts.push(Alert {
            schema_version: 1,
            id,
            source: input.source,
            rule: *rule,
            subject: subject.clone(),
            at_utc: input.as_of_utc.clone(),
            summary: summary.clone(),
            details: details.clone(),
        });
    }
    Ok((
        AlertState {
            as_of_utc: Some(input.as_of_utc.clone()),
            regime_id: Some(input.regime_id.clone()),
            references,
            active: conditions.into_keys().collect(),
        },
        alerts,
    ))
}
/// Persist rule transitions and events in one transaction. Restarts preserve episode suppression.
pub async fn emit(store: &Store, input: &AlertSnapshot) -> Result<Vec<Alert>, Error> {
    let saved = store
        .alert_state(input.source)
        .await
        .map_err(|_| Error::Store)?;
    let previous = saved.as_ref().map(|(_, s)| s.clone()).unwrap_or_default();
    let (state, alerts) = evaluate(&previous, input)?;
    if !store
        .save_alerts(
            input.source,
            saved.as_ref().map(|(h, _)| h.as_str()),
            &state,
            &alerts,
        )
        .await
        .map_err(|_| Error::Store)?
    {
        return Ok(vec![]);
    }
    Ok(alerts)
}
/// Build bounded rule input from stored evidence; never fabricate observer agreement or live regimes.
pub async fn poll(
    store: &Store,
    source: Source,
    region: &str,
    now: &str,
    daily_cap_lamports: u64,
) -> Result<Vec<Alert>, Error> {
    let context = alight_forecast::current_context(store, source, region, now)
        .await
        .map_err(|_| Error::Store)?;
    let rows = store
        .curve_snapshots(source, &context.regime_id, 1000)
        .await
        .map_err(|_| Error::Store)?;
    let mut cells = BTreeMap::new();
    for c in rows {
        if c.horizon_slots == 2 && time(&c.context.as_of_utc)? <= time(now)? && cells.len() < 81 {
            cells.entry(key(&c)?).or_insert(c);
        }
    }
    let from = (time(now)? - chrono::Duration::seconds(300))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let disagreements = store
        .recent_disagreements(source, &from, now)
        .await
        .map_err(|_| Error::Store)?;
    let budget = store
        .budget_by_route(
            source,
            &time(now)?
                .with_timezone(&Utc)
                .format("%Y-%m-%d")
                .to_string(),
        )
        .await
        .map_err(|_| Error::Store)?;
    let reserved = budget
        .as_object()
        .ok_or(Error::Invalid)?
        .values()
        .try_fold(0u64, |a, v| {
            v.as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .and_then(|n| a.checked_add(n))
                .ok_or(Error::Invalid)
        })?;
    let forecasts = store
        .forecast_tail(source, 20)
        .await
        .map_err(|_| Error::Store)?;
    emit(
        store,
        &AlertSnapshot {
            source,
            as_of_utc: now.into(),
            regime_id: context.regime_id,
            curves: cells.into_values().collect(),
            forecasts,
            observer_disagreements: disagreements,
            reserved_today_lamports: reserved,
            daily_cap_lamports,
        },
    )
    .await
}
fn text(alert: &Alert) -> String {
    format!(
        "Alight [{:?}] {:?}: {} at {}",
        alert.source, alert.rule, alert.summary, alert.at_utc
    )
}
/// Discord-compatible payload with mentions explicitly disabled.
pub fn discord(alert: &Alert) -> Value {
    json!({"content":text(alert),"allowed_mentions":{"parse":[]}})
}
/// Slack-compatible plain-text blocks, without mrkdwn/mention interpretation.
pub fn slack(alert: &Alert) -> Value {
    json!({"text":text(alert),"blocks":[{"type":"section","text":{"type":"plain_text","text":text(alert),"emoji":false}}]})
}
/// Generic webhook carries the source-scoped Alert, not a captured Solami/provider payload.
pub fn webhook(alert: &Alert) -> Value {
    json!({"type":"alight.alert.v1","data":alert})
}
#[derive(Clone, Copy)]
pub enum Format {
    Webhook,
    Discord,
    Slack,
}
pub struct Webhook {
    http: reqwest::Client,
    url: reqwest::Url,
    format: Format,
}
impl Webhook {
    /// Explicit opt-in URL; HTTPS, or loopback HTTP for induced local tests. Redirects disabled.
    pub fn new(url: &str, format: Format) -> Result<Self, Error> {
        let url = reqwest::Url::parse(url).map_err(|_| Error::Invalid)?;
        if !(url.scheme() == "https"
            || url.scheme() == "http"
                && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::Invalid);
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| Error::Invalid)?;
        Ok(Self { http, url, format })
    }
    /// One attempt, no automatic retry after an uncertain response. Units in payload remain strings.
    pub async fn send(&self, alert: &Alert) -> Result<(), Error> {
        let value = match self.format {
            Format::Webhook => webhook(alert),
            Format::Discord => discord(alert),
            Format::Slack => slack(alert),
        };
        let body = serde_json::to_vec(&value).map_err(|_| Error::Invalid)?;
        if body.len() > 16384 {
            return Err(Error::Invalid);
        }
        let response = self
            .http
            .post(self.url.clone())
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|_| Error::Delivery)?;
        if !response.status().is_success() {
            return Err(Error::Delivery);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests;
