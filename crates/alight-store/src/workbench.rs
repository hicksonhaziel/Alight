use crate::{Store, StoreError, label};
use alight_types::*;
use sqlx::Row;

impl Store {
    /// Newest source-scoped owned rows, bounded to 100; no provider calls or writes.
    pub async fn workbench_canaries(
        &self,
        source: Source,
        as_of: &str,
        limit: u32,
    ) -> Result<Vec<WorkbenchCanary>, StoreError> {
        if !(1..=100).contains(&limit) {
            return Err(StoreError::Invalid);
        }
        let rows = sqlx::query("SELECT c.payload_json,c.finalized,p.session_id,(SELECT json_extract(r.payload_json,'$.reason') FROM resolution_history r WHERE r.canary_id=c.id ORDER BY r.id DESC LIMIT 1) AS reason FROM canaries c LEFT JOIN prove_attempts p ON p.canary_id=c.id WHERE c.source=? AND julianday(json_extract(c.payload_json,'$.send_wall_utc'))<=julianday(?) ORDER BY julianday(json_extract(c.payload_json,'$.send_wall_utc')) DESC,c.id DESC LIMIT ?")
            .bind(label(source)?).bind(as_of).bind(limit).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|r| {
                let canary: Canary =
                    serde_json::from_str(&r.try_get::<String, _>("payload_json")?)?;
                if canary.source != source {
                    return Err(StoreError::Invalid);
                }
                Ok(WorkbenchCanary {
                    canary,
                    finalized: r.try_get("finalized")?,
                    prove_id: r.try_get("session_id")?,
                    resolution_reason: r.try_get("reason")?,
                })
            })
            .collect()
    }
    /// Latest grade per forecast, bounded to 100 and filtered through the experiment clock.
    pub async fn workbench_grades(
        &self,
        source: Source,
        as_of: &str,
    ) -> Result<Vec<ForecastGrade>, StoreError> {
        let rows: Vec<String> = sqlx::query_scalar("SELECT g.payload_json FROM forecast_grades g JOIN forecast_ledger f ON f.hash=g.forecast_hash WHERE f.source=? AND julianday(g.graded_at_utc)<=julianday(?) AND g.id=(SELECT MAX(g2.id) FROM forecast_grades g2 WHERE g2.forecast_hash=g.forecast_hash AND julianday(g2.graded_at_utc)<=julianday(?)) ORDER BY g.id DESC LIMIT 100")
            .bind(label(source)?).bind(as_of).bind(as_of).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|r| serde_json::from_str(r).map_err(StoreError::from))
            .collect()
    }
    /// Observed labels from owned canaries only; no detector or backfill claim.
    pub async fn workbench_regimes(
        &self,
        source: Source,
        as_of: &str,
    ) -> Result<Vec<WorkbenchRegime>, StoreError> {
        let rows = sqlx::query("SELECT json_extract(payload_json,'$.regime_id') AS regime,MIN(json_extract(payload_json,'$.send_wall_utc')) AS first,MAX(json_extract(payload_json,'$.send_wall_utc')) AS last,COUNT(*) AS n FROM canaries WHERE source=? AND julianday(json_extract(payload_json,'$.send_wall_utc'))<=julianday(?) GROUP BY regime ORDER BY last DESC LIMIT 50")
            .bind(label(source)?).bind(as_of).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|r| {
                Ok(WorkbenchRegime {
                    id: r.try_get("regime")?,
                    first_observed_utc: r.try_get("first")?,
                    last_observed_utc: r.try_get("last")?,
                    canaries: u64::try_from(r.try_get::<i64, _>("n")?)
                        .map_err(|_| StoreError::Invalid)?,
                })
            })
            .collect()
    }
    /// Last 50 gap episodes, with source and observation-clock boundaries preserved.
    pub async fn workbench_gaps(
        &self,
        source: Source,
        as_of: &str,
    ) -> Result<Vec<ObserverGap>, StoreError> {
        let rows = sqlx::query("SELECT observer,started_utc,ended_utc,reason FROM gaps WHERE source=? AND julianday(started_utc)<=julianday(?) ORDER BY id DESC LIMIT 50")
            .bind(label(source)?).bind(as_of).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|r| {
                Ok(ObserverGap {
                    source,
                    observer: serde_json::from_value(serde_json::Value::String(
                        r.try_get("observer")?,
                    ))?,
                    start_utc: r.try_get("started_utc")?,
                    end_utc: r.try_get("ended_utc")?,
                    reason: r.try_get("reason")?,
                })
            })
            .collect()
    }
    /// A source-scoped canary identity is required before exposing signature observations.
    pub async fn workbench_canary(
        &self,
        source: Source,
        id: &str,
    ) -> Result<Option<Canary>, StoreError> {
        let row: Option<String> =
            sqlx::query_scalar("SELECT payload_json FROM canaries WHERE source=? AND id=?")
                .bind(label(source)?)
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        row.map(|r| serde_json::from_str(&r).map_err(StoreError::from))
            .transpose()
    }
    /// At most 200 original observation records; no enrichment or provider request.
    pub async fn workbench_observations(
        &self,
        source: Source,
        signature: &str,
        as_of: &str,
    ) -> Result<Vec<ObserverEvent>, StoreError> {
        let rows: Vec<String> = sqlx::query_scalar("SELECT payload_json FROM observations WHERE source=? AND signature=? AND julianday(json_extract(payload_json,'$.data.received.wall_utc'))<=julianday(?) ORDER BY julianday(json_extract(payload_json,'$.data.received.wall_utc')) LIMIT 200")
            .bind(label(source)?).bind(signature).bind(as_of).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|r| match serde_json::from_str(r)? {
                IngestEvent::Observation(e) => Ok(e),
                _ => Err(StoreError::Invalid),
            })
            .collect()
    }
}
