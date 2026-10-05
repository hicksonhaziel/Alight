use crate::{Store, StoreError, label};
use alight_types::{CurveSnapshot, Source, TrainingCanary};
use chrono::DateTime;
use sha2::{Digest, Sha256};

impl Store {
    /// Content-addressed curve history; repeated identical snapshots are idempotent.
    /// as_of_ms is Unix UTC milliseconds. This method performs no live collection.
    pub async fn save_curve_snapshot(
        &self,
        snapshot: &CurveSnapshot,
    ) -> Result<String, StoreError> {
        snapshot.validate().map_err(|_| StoreError::Invalid)?;
        let as_of_ms = DateTime::parse_from_rfc3339(&snapshot.context.as_of_utc)
            .map_err(|_| StoreError::Invalid)?
            .timestamp_millis();
        let payload = serde_json::to_string(snapshot)?;
        let id = format!("sha256:{:x}", Sha256::digest(payload.as_bytes()));
        sqlx::query("INSERT INTO curve_snapshots(snapshot_id,source,regime_id,as_of_ms,methodology_hash,payload_json) VALUES(?,?,?,?,?,?) ON CONFLICT(snapshot_id) DO NOTHING")
            .bind(&id).bind(label(snapshot.context.source)?).bind(&snapshot.context.regime_id)
            .bind(as_of_ms).bind(&snapshot.methodology_hash).bind(payload).execute(&self.pool).await?;
        Ok(id)
    }

    /// Reads at most 10,000 historical rows for one source and regime, verifying stored bytes.
    pub async fn curve_snapshots(
        &self,
        source: Source,
        regime: &str,
        limit: u32,
    ) -> Result<Vec<CurveSnapshot>, StoreError> {
        use sqlx::Row;
        if limit == 0 || limit > 10_000 {
            return Err(StoreError::Invalid);
        }
        let rows = sqlx::query("SELECT snapshot_id,payload_json FROM curve_snapshots WHERE source=? AND regime_id=? ORDER BY as_of_ms DESC,snapshot_id LIMIT ?")
            .bind(label(source)?).bind(regime).bind(limit).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|row| {
                let text: String = row.try_get("payload_json")?;
                let id: String = row.try_get("snapshot_id")?;
                if id != format!("sha256:{:x}", Sha256::digest(text.as_bytes())) {
                    return Err(StoreError::Invalid);
                }
                let snapshot: CurveSnapshot = serde_json::from_str(&text)?;
                snapshot.validate().map_err(|_| StoreError::Invalid)?;
                Ok(snapshot)
            })
            .collect()
    }

    /// Source-filtered owned records with explicit finalization. Passive observations never enter training.
    pub async fn training_canaries(
        &self,
        source: Source,
    ) -> Result<Vec<TrainingCanary>, StoreError> {
        use sqlx::Row;
        let rows =
            sqlx::query("SELECT finalized,payload_json FROM canaries WHERE source=? ORDER BY id")
                .bind(label(source)?)
                .fetch_all(&self.pool)
                .await?;
        rows.iter()
            .map(|row| {
                let text: String = row.try_get("payload_json")?;
                Ok(TrainingCanary {
                    canary: serde_json::from_str(&text)?,
                    finalized: row.try_get("finalized")?,
                })
            })
            .collect()
    }
}
