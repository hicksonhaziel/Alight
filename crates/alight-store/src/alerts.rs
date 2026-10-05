use crate::{Store, StoreError, canonical, content_hash, label};
use alight_types::{Alert, AlertState, Source};
use sqlx::Row;
impl Store {
    pub async fn alert_state(
        &self,
        source: Source,
    ) -> Result<Option<(String, AlertState)>, StoreError> {
        let row = sqlx::query("SELECT hash,payload_json FROM alert_state WHERE source=?")
            .bind(label(source)?)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| {
            let state: AlertState = serde_json::from_str(&r.try_get::<String, _>("payload_json")?)?;
            let hash: String = r.try_get("hash")?;
            if content_hash(&state)? != hash {
                return Err(StoreError::Invalid);
            }
            Ok((hash, state))
        })
        .transpose()
    }
    /// Compare-and-swap state plus new alerts atomically, with 10,000-row history cap.
    pub async fn save_alerts(
        &self,
        source: Source,
        previous_hash: Option<&str>,
        state: &AlertState,
        alerts: &[Alert],
    ) -> Result<bool, StoreError> {
        let payload = canonical(state)?;
        if payload.len() > 512 * 1024
            || alerts.len() > 128
            || alerts.iter().any(|a| a.source != source)
        {
            return Err(StoreError::Invalid);
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO alert_state(source,hash,payload_json) VALUES(?,'','{}') ON CONFLICT(source) DO NOTHING").bind(label(source)?).execute(&mut *tx).await?;
        let hash: String = sqlx::query_scalar("SELECT hash FROM alert_state WHERE source=?")
            .bind(label(source)?)
            .fetch_one(&mut *tx)
            .await?;
        if previous_hash.unwrap_or("") != hash {
            tx.rollback().await?;
            return Ok(false);
        }
        for alert in alerts {
            let bytes = canonical(alert)?;
            if bytes.len() > 16384 {
                return Err(StoreError::Invalid);
            }
            sqlx::query("INSERT INTO alerts(id,source,at_utc,payload_json) VALUES(?,?,?,?)")
                .bind(&alert.id)
                .bind(label(source)?)
                .bind(&alert.at_utc)
                .bind(bytes)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("UPDATE alert_state SET hash=?,payload_json=? WHERE source=?")
            .bind(content_hash(state)?)
            .bind(payload)
            .bind(label(source)?)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM alerts WHERE id IN (SELECT id FROM alerts WHERE source=? ORDER BY at_utc DESC,id DESC LIMIT -1 OFFSET 10000)").bind(label(source)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(true)
    }
    pub async fn alert_delivery(&self, id: &str, status: &str) -> Result<(), StoreError> {
        if !["DELIVERED", "FAILED", "NO_ENDPOINT"].contains(&status) {
            return Err(StoreError::Invalid);
        }
        sqlx::query("UPDATE alerts SET delivery_status=? WHERE id=? AND delivery_status='PENDING'")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    /// Distinct recent canaries whose latest resolver evidence records observer disagreement.
    pub async fn recent_disagreements(
        &self,
        source: Source,
        from: &str,
        through: &str,
    ) -> Result<u32, StoreError> {
        let count:i64=sqlx::query_scalar("SELECT COUNT(*) FROM canaries c JOIN resolution_history r ON r.rowid=(SELECT MAX(r2.rowid) FROM resolution_history r2 WHERE r2.canary_id=c.id) WHERE c.source=? AND r.at_utc>=? AND r.at_utc<=? AND json_extract(r.payload_json,'$.reason') IN ('observer_disagreement','rpc_disagreement_or_incomplete')")
            .bind(label(source)?).bind(from).bind(through).fetch_one(&self.pool).await?;
        u32::try_from(count).map_err(|_| StoreError::Invalid)
    }
}
