use crate::{Store, StoreError, label};
use alight_types::*;
use chrono::DateTime;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::Row;

pub const GENESIS: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
pub fn canonical<T: Serialize>(value: &T) -> Result<String, StoreError> {
    Ok(serde_json::to_string(&serde_json::to_value(value)?)?)
}
pub fn content_hash<T: Serialize>(value: &T) -> Result<String, StoreError> {
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(canonical(value)?.as_bytes())
    ))
}
fn chain_hash(source: &str, sequence: i64, previous: &str, payload: &str) -> String {
    format!(
        "sha256:{:x}",
        Sha256::digest(format!(
            "alight.forecast.v1\0{source}\0{sequence}\0{previous}\0{payload}"
        ))
    )
}
fn time(text: &str) -> Result<chrono::DateTime<chrono::Utc>, StoreError> {
    DateTime::parse_from_rfc3339(text)
        .map(|t| t.with_timezone(&chrono::Utc))
        .map_err(|_| StoreError::Invalid)
}

impl Store {
    /// Bounded ledger page by sequence, preserving source and each row's hash/link.
    pub async fn forecast_page(
        &self,
        source: Source,
        after: u64,
        limit: u32,
    ) -> Result<Vec<ForecastEntry>, StoreError> {
        if limit == 0 || limit > 100 {
            return Err(StoreError::Invalid);
        }
        let after = i64::try_from(after).map_err(|_| StoreError::Invalid)?;
        let ids:Vec<String>=sqlx::query_scalar("SELECT hash FROM forecast_ledger WHERE source=? AND sequence>? ORDER BY sequence LIMIT ?")
            .bind(label(source)?).bind(after).bind(limit).fetch_all(&self.pool).await?;
        let mut entries = Vec::new();
        for id in ids {
            entries.push(
                self.forecast(source, &id)
                    .await?
                    .ok_or(StoreError::Invalid)?,
            );
        }
        Ok(entries)
    }
    /// Reads one source-scoped forecast by its ID or chain hash and checks its payload/link.
    pub async fn forecast(
        &self,
        source: Source,
        id_or_hash: &str,
    ) -> Result<Option<ForecastEntry>, StoreError> {
        let source_label = label(source)?;
        let row = sqlx::query("SELECT sequence,prev_hash,hash,payload_json FROM forecast_ledger WHERE source=? AND (forecast_id=? OR hash=?) LIMIT 1")
            .bind(&source_label).bind(id_or_hash).bind(id_or_hash).fetch_optional(&self.pool).await?;
        row.map(|row| {
            let payload: String = row.try_get("payload_json")?;
            let forecast: Forecast = serde_json::from_str(&payload)?;
            let sequence: i64 = row.try_get("sequence")?;
            let prev_hash: String = row.try_get("prev_hash")?;
            let hash: String = row.try_get("hash")?;
            if sequence <= 0
                || forecast.source != source
                || canonical(&forecast)? != payload
                || hash != chain_hash(&source_label, sequence, &prev_hash, &payload)
            {
                return Err(StoreError::Invalid);
            }
            Ok(ForecastEntry {
                sequence: sequence as u64,
                prev_hash,
                hash,
                forecast,
            })
        })
        .transpose()
    }
    pub async fn forecasts_to_grade(
        &self,
        source: Source,
        limit: u32,
    ) -> Result<Vec<ForecastEntry>, StoreError> {
        if limit == 0 || limit > 1000 {
            return Err(StoreError::Invalid);
        }
        let rows=sqlx::query("SELECT f.sequence,f.prev_hash,f.hash,f.payload_json FROM forecast_ledger f WHERE f.source=? AND NOT EXISTS(SELECT 1 FROM forecast_grades g WHERE g.forecast_hash=f.hash AND json_extract(g.payload_json,'$.status') IN ('SCORED','VOIDED')) ORDER BY f.sequence LIMIT ?").bind(label(source)?).bind(limit).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|r| {
                Ok(ForecastEntry {
                    sequence: r.try_get::<i64, _>("sequence")? as u64,
                    prev_hash: r.try_get("prev_hash")?,
                    hash: r.try_get("hash")?,
                    forecast: serde_json::from_str(&r.try_get::<String, _>("payload_json")?)?,
                })
            })
            .collect()
    }
    pub async fn has_signal(
        &self,
        source: Source,
        day: &str,
        methodology: &str,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM signal_reports WHERE source=? AND day=? AND methodology_hash=?)").bind(label(source)?).bind(day).bind(methodology).fetch_one(&self.pool).await.map_err(StoreError::from)
    }
    /// Writes a complete frozen model document. Its content hash is used by every forecast.
    pub async fn save_models(&self, models: &[ModelFit]) -> Result<String, StoreError> {
        let first = models.first().ok_or(StoreError::Invalid)?;
        if models.iter().any(|m| {
            m.context.source != first.context.source
                || m.context.regime_id != first.context.regime_id
                || m.context.as_of_utc != first.context.as_of_utc
        }) {
            return Err(StoreError::Invalid);
        }
        time(&first.context.as_of_utc)?;
        let hash = content_hash(&models)?;
        sqlx::query("INSERT INTO model_snapshots(hash,source,regime_id,as_of_utc,payload_json) VALUES(?,?,?,?,?) ON CONFLICT(hash) DO NOTHING")
            .bind(&hash).bind(label(first.context.source)?).bind(&first.context.regime_id).bind(&first.context.as_of_utc).bind(canonical(&models)?).execute(&self.pool).await?;
        Ok(hash)
    }
    pub async fn load_models(&self, hash: &str) -> Result<Vec<ModelFit>, StoreError> {
        let payload: String =
            sqlx::query_scalar("SELECT payload_json FROM model_snapshots WHERE hash=?")
                .bind(hash)
                .fetch_one(&self.pool)
                .await?;
        let models: Vec<ModelFit> = serde_json::from_str(&payload)?;
        if content_hash(&models)? != hash {
            return Err(StoreError::Invalid);
        }
        Ok(models)
    }
    /// Atomic append includes a durable head, which also detects tail deletion during verification.
    pub async fn append_forecast(&self, forecast: &Forecast) -> Result<ForecastEntry, StoreError> {
        let created = time(&forecast.created_at_utc)?;
        if time(&forecast.expires_at_utc)? <= created
            || forecast.id.is_empty()
            || forecast.source != forecast.request.context.source
            || forecast.source != forecast.quote.context.source
            || forecast.regime_id != forecast.request.context.regime_id
            || forecast.regime_id != forecast.quote.context.regime_id
            || time(&forecast.request.context.as_of_utc)? != created
            || forecast.methodology_hash != forecast.quote.methodology_hash
            || canonical(&forecast.request.target)? != canonical(&forecast.quote.target)?
        {
            return Err(StoreError::Invalid);
        }
        if forecast.quote.evidence == Evidence::Insufficient
            && forecast.quote.recommendation.is_some()
        {
            return Err(StoreError::Invalid);
        }
        if forecast.economics.is_some() != forecast.requested_economics.is_some() {
            return Err(StoreError::Invalid);
        }
        if let Some(economics) = &forecast.economics {
            if canonical(&economics.model_quote)? != canonical(&forecast.quote)?
                || economics.economics.is_some() == economics.fallback_reason.is_some()
            {
                return Err(StoreError::Invalid);
            }
            if let Some(summary) = &economics.economics {
                if summary.market.source != forecast.source
                    || summary.market.regime_id != forecast.regime_id
                    || forecast
                        .requested_economics
                        .as_ref()
                        .map(canonical)
                        .transpose()?
                        != Some(canonical(&summary.inputs)?)
                    || summary.market.pool != summary.inputs.pool
                    || time(&summary.market.as_of_utc)? > created
                    || !summary.recommendation.qualifies
                    || forecast
                        .quote
                        .recommendation
                        .as_ref()
                        .map(canonical)
                        .transpose()?
                        != Some(canonical(&summary.recommendation.prediction)?)
                {
                    return Err(StoreError::Invalid);
                }
                let saved: String =
                    sqlx::query_scalar("SELECT payload_json FROM market_snapshots WHERE hash=?")
                        .bind(content_hash(&summary.market)?)
                        .fetch_one(&self.pool)
                        .await?;
                if saved != canonical(&summary.market)? {
                    return Err(StoreError::Invalid);
                }
            }
        }
        let models = self.load_models(&forecast.model_snapshot_hash).await?;
        if models.iter().any(|m| {
            m.context.source != forecast.source
                || m.context.regime_id != forecast.regime_id
                || time(&m.context.as_of_utc).map_or(true, |t| t > created)
        }) {
            return Err(StoreError::Invalid);
        }
        for b in &forecast.baselines {
            if let Some(hash) = &b.model_snapshot_hash {
                let frozen = self.load_models(hash).await?;
                if frozen.iter().any(|m| {
                    m.context.source != forecast.source
                        || time(&m.context.as_of_utc).map_or(true, |t| t > created)
                }) {
                    return Err(StoreError::Invalid);
                }
            }
        }
        let source = label(forecast.source)?;
        let payload = canonical(forecast)?;
        let mut tx = self.pool.begin().await?;
        // Acquire the writer lock before reading the previous head, including other connections.
        sqlx::query("INSERT INTO forecast_heads(source,sequence,hash) VALUES(?,0,?) ON CONFLICT(source) DO NOTHING").bind(&source).bind(GENESIS).execute(&mut *tx).await?;
        let row = sqlx::query("SELECT sequence,hash FROM forecast_heads WHERE source=?")
            .bind(&source)
            .fetch_one(&mut *tx)
            .await?;
        let previous: String = row.try_get("hash")?;
        let sequence = row
            .try_get::<i64, _>("sequence")?
            .checked_add(1)
            .ok_or(StoreError::Invalid)?;
        let hash = chain_hash(&source, sequence, &previous, &payload);
        sqlx::query("INSERT INTO forecast_ledger(source,sequence,forecast_id,prev_hash,hash,payload_json) VALUES(?,?,?,?,?,?)").bind(&source).bind(sequence).bind(&forecast.id).bind(&previous).bind(&hash).bind(payload).execute(&mut *tx).await?;
        sqlx::query("UPDATE forecast_heads SET sequence=?,hash=? WHERE source=?")
            .bind(sequence)
            .bind(&hash)
            .bind(&source)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(ForecastEntry {
            sequence: sequence as u64,
            prev_hash: previous,
            hash,
            forecast: forecast.clone(),
        })
    }
    /// Verifies every canonical payload/link and the persisted head within a consistent read transaction.
    pub async fn verify_ledger(&self, source: Source) -> Result<u64, StoreError> {
        let source = label(source)?;
        let mut tx = self.pool.begin().await?;
        let rows=sqlx::query("SELECT sequence,prev_hash,hash,payload_json FROM forecast_ledger WHERE source=? ORDER BY sequence").bind(&source).fetch_all(&mut *tx).await?;
        let mut previous = GENESIS.to_owned();
        let mut sequence = 0i64;
        let mut checked_models = std::collections::BTreeSet::new();
        let mut checked_markets = std::collections::BTreeSet::new();
        for row in rows {
            sequence += 1;
            let payload: String = row.try_get("payload_json")?;
            let forecast: Forecast = serde_json::from_str(&payload)?;
            let hash: String = row.try_get("hash")?;
            if row.try_get::<i64, _>("sequence")? != sequence
                || row.try_get::<String, _>("prev_hash")? != previous
                || canonical(&forecast)? != payload
                || label(forecast.source)? != source
                || hash != chain_hash(&source, sequence, &previous, &payload)
            {
                return Err(StoreError::Invalid);
            }
            for model_hash in std::iter::once(&forecast.model_snapshot_hash).chain(
                forecast
                    .baselines
                    .iter()
                    .filter_map(|b| b.model_snapshot_hash.as_ref()),
            ) {
                if checked_models.insert(model_hash.clone()) {
                    let document: String =
                        sqlx::query_scalar("SELECT payload_json FROM model_snapshots WHERE hash=?")
                            .bind(model_hash)
                            .fetch_one(&mut *tx)
                            .await?;
                    let models: Vec<ModelFit> = serde_json::from_str(&document)?;
                    if content_hash(&models)? != *model_hash {
                        return Err(StoreError::Invalid);
                    }
                }
            }
            if let Some(summary) = forecast
                .economics
                .as_ref()
                .and_then(|e| e.economics.as_ref())
            {
                let market_hash = content_hash(&summary.market)?;
                if checked_markets.insert(market_hash.clone()) {
                    let document: String = sqlx::query_scalar(
                        "SELECT payload_json FROM market_snapshots WHERE hash=?",
                    )
                    .bind(market_hash)
                    .fetch_one(&mut *tx)
                    .await?;
                    if document != canonical(&summary.market)? {
                        return Err(StoreError::Invalid);
                    }
                }
            }
            previous = hash;
        }
        let head = sqlx::query("SELECT sequence,hash FROM forecast_heads WHERE source=?")
            .bind(&source)
            .fetch_optional(&mut *tx)
            .await?;
        if let Some(head) = head {
            if head.try_get::<i64, _>("sequence")? != sequence
                || head.try_get::<String, _>("hash")? != previous
            {
                return Err(StoreError::Invalid);
            }
        } else if sequence != 0 {
            return Err(StoreError::Invalid);
        }
        tx.commit().await?;
        Ok(sequence as u64)
    }
    /// Source-filtered forecast rows, oldest first. Verify the ledger before exporting them.
    pub async fn forecasts(&self, source: Source) -> Result<Vec<ForecastEntry>, StoreError> {
        let rows=sqlx::query("SELECT sequence,prev_hash,hash,payload_json FROM forecast_ledger WHERE source=? ORDER BY sequence").bind(label(source)?).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|r| {
                Ok(ForecastEntry {
                    sequence: r.try_get::<i64, _>("sequence")? as u64,
                    prev_hash: r.try_get("prev_hash")?,
                    hash: r.try_get("hash")?,
                    forecast: serde_json::from_str(&r.try_get::<String, _>("payload_json")?)?,
                })
            })
            .collect()
    }
    pub async fn save_grade(&self, grade: &ForecastGrade) -> Result<(), StoreError> {
        time(&grade.graded_at_utc)?;
        sqlx::query("INSERT INTO forecast_grades(forecast_hash,graded_at_utc,payload_json) VALUES(?,?,?) ON CONFLICT(forecast_hash,graded_at_utc) DO NOTHING").bind(&grade.forecast_hash).bind(&grade.graded_at_utc).bind(canonical(grade)?).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn grades(&self, source: Source) -> Result<Vec<ForecastGrade>, StoreError> {
        let values:Vec<String>=sqlx::query_scalar("SELECT g.payload_json FROM forecast_grades g JOIN forecast_ledger f ON f.hash=g.forecast_hash WHERE f.source=? ORDER BY g.id").bind(label(source)?).fetch_all(&self.pool).await?;
        values
            .iter()
            .map(|v| serde_json::from_str(v).map_err(StoreError::from))
            .collect()
    }
    pub async fn save_signal(&self, report: &SignalReport) -> Result<String, StoreError> {
        time(&report.as_of_utc)?;
        let hash = content_hash(report)?;
        sqlx::query("INSERT INTO signal_reports(hash,source,day,as_of_utc,methodology_hash,payload_json) VALUES(?,?,?,?,?,?) ON CONFLICT(hash) DO NOTHING").bind(&hash).bind(label(report.source)?).bind(&report.day).bind(&report.as_of_utc).bind(&report.methodology_hash).bind(canonical(report)?).execute(&self.pool).await?;
        Ok(hash)
    }
    /// Simulator/replay import is deliberately barred from creating live training outcomes.
    pub async fn import_training(&self, sample: &TrainingCanary) -> Result<(), StoreError> {
        let c = &sample.canary;
        if c.source == Source::Live {
            return Err(StoreError::Invalid);
        }
        let payload = serde_json::to_string(c)?;
        let context = serde_json::to_string(&sample.covariates)?;
        let mut tx = self.pool.begin().await?;
        let old = sqlx::query("SELECT c.payload_json,c.finalized,m.payload_json AS model_context FROM canaries c LEFT JOIN canary_model_context m ON m.canary_id=c.id WHERE c.id=?")
            .bind(&c.id).fetch_optional(&mut *tx).await?;
        if let Some(old) = old
            && (old.try_get::<String, _>("payload_json")? != payload
                || old.try_get::<bool, _>("finalized")? != sample.finalized
                || old
                    .try_get::<Option<String>, _>("model_context")?
                    .as_deref()
                    != Some(context.as_str()))
        {
            return Err(StoreError::Invalid);
        }
        sqlx::query("INSERT INTO canaries(id,source,signature,outcome,finalized,payload_json) VALUES(?,?,?,?,?,?) ON CONFLICT(id) DO NOTHING").bind(&c.id).bind(label(c.source)?).bind(&c.signature).bind(c.outcome.map(label).transpose()?).bind(sample.finalized).bind(payload).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO canary_model_context(canary_id,payload_json) VALUES(?,?) ON CONFLICT(canary_id) DO NOTHING").bind(&c.id).bind(context).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
}
