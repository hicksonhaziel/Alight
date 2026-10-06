//! Model jobs and forecast lifecycle. No provider client, keys, signer or transaction sender.
mod service;
use alight_model::{estimate, pooled, quote, scoring, signal};
use alight_store::{Store, StoreError, content_hash};
use alight_types::*;
use chrono::{DateTime, Duration, Utc};
pub use service::{issue_combined, preview_combined};
use std::collections::BTreeMap;

fn utc(text: &str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(text)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| StoreError::Invalid)
}
fn context(
    samples: &[TrainingCanary],
    source: Source,
    region: &str,
    as_of: &str,
) -> Result<CurveContext, StoreError> {
    let now = utc(as_of)?;
    let latest = samples
        .iter()
        .filter(|s| s.canary.source == source)
        .filter_map(|s| {
            utc(&s.canary.send_wall_utc)
                .ok()
                .filter(|t| *t <= now)
                .map(|t| (t, &s.canary))
        })
        .max_by_key(|(t, _)| *t);
    Ok(CurveContext {
        source,
        region: region.into(),
        regime_id: latest.map_or("phase1-unclassified".into(), |(_, c)| c.regime_id.clone()),
        as_of_utc: as_of.into(),
    })
}
pub fn candidates(samples: &[TrainingCanary], context: &CurveContext) -> Vec<CanaryConfig> {
    let mut cells = BTreeMap::new();
    for s in samples
        .iter()
        .filter(|s| s.canary.source == context.source && s.canary.regime_id == context.regime_id)
    {
        if let Some(sent) = utc(&s.canary.send_wall_utc)
            .ok()
            .filter(|t| utc(&context.as_of_utc).is_ok_and(|now| *t <= now))
        {
            let key = format!(
                "{:?}:{:?}:{:?}:{:?}:{}",
                s.canary.config.route,
                s.canary.config.fee_bucket,
                s.canary.config.tip_tier,
                s.canary.config.size_class,
                s.canary.config.cu_limit
            );
            let newer = cells
                .get(&key)
                .is_none_or(|(previous, _): &(DateTime<Utc>, CanaryConfig)| *previous < sent);
            if newer {
                cells.insert(key, (sent, s.canary.config.clone()));
            }
        }
    }
    cells.into_values().map(|(_, c)| c).collect()
}
pub async fn current_context(
    store: &Store,
    source: Source,
    region: &str,
    as_of: &str,
) -> Result<CurveContext, StoreError> {
    let mut current = context(
        &store.training_canaries(source).await?,
        source,
        region,
        as_of,
    )?;
    if let Some(regime) = store.active_regime(source, as_of).await? {
        current.regime_id = regime.regime_id;
    }
    Ok(current)
}
/// Periodic snapshot and grader tick; heavy numerical work uses a blocking worker, not ingest tasks.
pub async fn tick(
    store: &Store,
    source: Source,
    region: &str,
    as_of: &str,
) -> Result<serde_json::Value, StoreError> {
    let samples = store.training_canaries(source).await?;
    let mut context = context(&samples, source, region, as_of)?;
    if let Some(regime) = store.active_regime(source, as_of).await? {
        context.regime_id = regime.regime_id;
    }
    let cells = candidates(&samples, &context);
    let entries = store
        .forecasts_to_grade(source, 200)
        .await?
        .into_iter()
        .filter(|e| {
            utc(&e.forecast.created_at_utc)
                .is_ok_and(|created| utc(as_of).is_ok_and(|now| created <= now))
        })
        .collect::<Vec<_>>();
    let ctx = context.clone();
    let now = as_of.to_owned();
    let grading_samples = store.grading_canaries(source).await?;
    let (models, curves, grades) = tokio::task::spawn_blocking(move || -> Result<_, StoreError> {
        let mut models = Vec::new();
        let mut curves = Vec::new();
        for horizon in [1, 2, 4] {
            models
                .push(pooled::fit_blend(&samples, &ctx, horizon).map_err(|_| StoreError::Invalid)?);
            for cell in &cells {
                curves.push(
                    estimate(&samples, cell, &ctx, horizon).map_err(|_| StoreError::Invalid)?,
                );
            }
        }
        let grades = entries
            .iter()
            .map(|e| scoring::grade(e, &grading_samples, &now).map_err(|_| StoreError::Invalid))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((models, curves, grades))
    })
    .await
    .map_err(|_| StoreError::Invalid)??;
    let model_hash = store.save_models(&models).await?;
    for curve in &curves {
        store.save_curve_snapshot(curve).await?;
    }
    for grade in &grades {
        store.save_grade(grade).await?;
    }
    Ok(
        serde_json::json!({"source":source,"regime_id":context.regime_id,"model_snapshot_hash":model_hash,"curves":curves.len(),"grades":grades.len()}),
    )
}
pub async fn issue(
    store: &Store,
    request: ModelQuoteRequest,
    ttl_s: u32,
    frozen_hash: Option<&str>,
    tape: &[TipTapeObservation],
) -> Result<ForecastEntry, StoreError> {
    issue_inner(store, request, ttl_s, frozen_hash, tape, None).await
}
async fn issue_inner(
    store: &Store,
    request: ModelQuoteRequest,
    ttl_s: u32,
    frozen_hash: Option<&str>,
    tape: &[TipTapeObservation],
    economics: Option<&EconomicsInputs>,
) -> Result<ForecastEntry, StoreError> {
    if !(1..=86400).contains(&ttl_s) {
        return Err(StoreError::Invalid);
    }
    let (samples, models, response) = compute(store, &request).await?;
    let model_hash = store.save_models(&models).await?;
    let frozen = if let Some(hash) = frozen_hash {
        Some(store.load_models(hash).await?)
    } else {
        None
    };
    let requested_economics = economics.cloned();
    let (economics, baseline) = if let Some(inputs) = economics {
        let (e, b) = service::combine(
            store,
            &samples,
            &request,
            &models,
            &response,
            inputs,
            tape,
            frozen_hash.zip(frozen.as_deref()),
        )
        .await?;
        (Some(e), b)
    } else {
        (
            None,
            quote::baselines(
                &samples,
                &request,
                &models,
                response.recommendation.as_ref().map(|r| &r.config),
                tape,
                frozen_hash.zip(frozen.as_deref()),
            )
            .map_err(|_| StoreError::Invalid)?,
        )
    };
    let response = economics
        .as_ref()
        .map_or(response.clone(), |e| e.model_quote.clone());
    let id_hash = if let Some(e) = &economics {
        content_hash(&(
            request.clone(),
            ttl_s,
            frozen_hash,
            tape,
            &model_hash,
            &requested_economics,
            e,
        ))?
    } else {
        content_hash(&(request.clone(), ttl_s, frozen_hash, tape, &model_hash))?
    };
    let forecast = Forecast {
        id: format!("q-{id_hash}"),
        source: request.context.source,
        created_at_utc: request.context.as_of_utc.clone(),
        expires_at_utc: (utc(&request.context.as_of_utc)? + Duration::seconds(i64::from(ttl_s)))
            .to_rfc3339(),
        regime_id: request.context.regime_id.clone(),
        methodology_hash: response.methodology_hash.clone(),
        model_snapshot_hash: model_hash,
        request,
        quote: response,
        baselines: baseline,
        economics,
        requested_economics,
    };
    if let Some(previous) = store.forecast(forecast.source, &forecast.id).await? {
        if content_hash(&previous.forecast)? != content_hash(&forecast)? {
            return Err(StoreError::Invalid);
        }
        return Ok(previous);
    }
    match store.append_forecast(&forecast).await {
        Ok(entry) => Ok(entry),
        Err(error) => {
            // A concurrent identical retry may have committed after the initial lookup.
            if let Some(previous) = store.forecast(forecast.source, &forecast.id).await?
                && content_hash(&previous.forecast)? == content_hash(&forecast)?
            {
                return Ok(previous);
            }
            Err(error)
        }
    }
}
async fn compute(
    store: &Store,
    request: &ModelQuoteRequest,
) -> Result<(Vec<TrainingCanary>, Vec<ModelFit>, ModelQuote), StoreError> {
    let samples = store.training_canaries(request.context.source).await?;
    let request = request.clone();
    tokio::task::spawn_blocking(move || {
        let models = [1, 2, 4]
            .into_iter()
            .map(|h| {
                pooled::fit_blend(&samples, &request.context, h).map_err(|_| StoreError::Invalid)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let response =
            quote::quote(&samples, &request, &models).map_err(|_| StoreError::Invalid)?;
        Ok((samples, models, response))
    })
    .await
    .map_err(|_| StoreError::Invalid)?
}
/// Report a completed UTC day once under the registered methodology; manual reports can be refreshed.
pub async fn daily(
    store: &Store,
    source: Source,
    as_of: &str,
) -> Result<Option<String>, StoreError> {
    let day = (utc(as_of)? - Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    if store
        .has_signal(source, &day, &alight_model::methodology_hash())
        .await?
    {
        return Ok(None);
    }
    let report = signal_report(store, source, &day, as_of).await?;
    Ok(Some(store.save_signal(&report).await?))
}
pub async fn signal_report(
    store: &Store,
    source: Source,
    day: &str,
    as_of: &str,
) -> Result<SignalReport, StoreError> {
    let samples = store.training_canaries(source).await?;
    let day = day.to_owned();
    let now = as_of.to_owned();
    tokio::task::spawn_blocking(move || {
        signal::report(&samples, source, &day, &now).map_err(|_| StoreError::Invalid)
    })
    .await
    .map_err(|_| StoreError::Invalid)?
}
