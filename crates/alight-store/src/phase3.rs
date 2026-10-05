use crate::{Store, StoreError, canonical, content_hash, label, slot_key};
use alight_types::*;
use chrono::DateTime;
use sqlx::Row;

fn millis(utc: &str) -> Result<i64, StoreError> {
    Ok(DateTime::parse_from_rfc3339(utc)
        .map_err(|_| StoreError::Invalid)?
        .timestamp_millis())
}
fn limits_valid(limits: &TapeLimits) -> Result<(), StoreError> {
    if limits.retention_s == 0
        || limits.retention_s > 7 * 86400
        || limits.max_rows == 0
        || limits.max_rows > 100_000
        || limits.max_bytes < 4096
        || limits.max_bytes > 64 * 1024 * 1024
    {
        return Err(StoreError::Invalid);
    }
    Ok(())
}

impl Store {
    /// Prunes passive rows during idle collection; cutoff is elapsed UTC seconds.
    pub async fn prune_passive_tips(
        &self,
        source: Source,
        limits: &TapeLimits,
        now: &str,
    ) -> Result<u64, StoreError> {
        limits_valid(limits)?;
        let cutoff = millis(now)?
            .checked_sub(i64::from(limits.retention_s) * 1000)
            .ok_or(StoreError::Invalid)?;
        Ok(
            sqlx::query("DELETE FROM passive_tips WHERE source=? AND received_ms<?")
                .bind(label(source)?)
                .bind(cutoff)
                .execute(&self.pool)
                .await?
                .rows_affected(),
        )
    }
    /// Writes at most 256 recipient records, then prunes by UTC age, rows and payload bytes.
    /// Limits apply independently per source. No tape data enter owned training tables.
    pub async fn save_passive_tips(
        &self,
        tips: &[PassiveTip],
        limits: &TapeLimits,
        now: &str,
    ) -> Result<TapeUsage, StoreError> {
        limits_valid(limits)?;
        let first = tips.first().ok_or(StoreError::Invalid)?;
        if tips.len() > 256 || tips.iter().any(|t| t.source != first.source) {
            return Err(StoreError::Invalid);
        }
        let source = label(first.source)?;
        let now_ms = millis(now)?;
        let cutoff = now_ms
            .checked_sub(i64::from(limits.retention_s) * 1000)
            .ok_or(StoreError::Invalid)?;
        let mut tx = self.pool.begin().await?;
        // A write locks the source partition before reads, including competing connections.
        sqlx::query("DELETE FROM passive_tips WHERE source=? AND received_ms<?")
            .bind(&source)
            .bind(cutoff)
            .execute(&mut *tx)
            .await?;
        for tip in tips {
            let received_ms = millis(&tip.received.wall_utc)?;
            if received_ms > now_ms
                || tip.signature.len() > 100
                || tip.signature.is_empty()
                || tip.recipient.len() > 64
                || tip.recipient.is_empty()
                || tip
                    .block_id
                    .as_ref()
                    .is_some_and(|b| b.is_empty() || b.len() > 100)
                || (!tip.success && tip.tip_lamports != Some(0))
                || tip
                    .tip_lamports
                    .is_some_and(|paid| paid > tip.requested_tip_lamports)
            {
                return Err(StoreError::Invalid);
            }
            if received_ms < cutoff {
                continue;
            }
            let payload = canonical(tip)?;
            if payload.len() > 4096 {
                return Err(StoreError::Invalid);
            }
            let observer = label(tip.observer)?;
            let block = tip.block_id.as_deref().unwrap_or("");
            let previous: Option<String> = sqlx::query_scalar("SELECT payload_json FROM passive_tips WHERE source=? AND observer=? AND signature=? AND slot=? AND block_key=? AND recipient=?")
                .bind(&source).bind(&observer).bind(&tip.signature).bind(slot_key(tip.slot)).bind(block).bind(&tip.recipient).fetch_optional(&mut *tx).await?;
            if let Some(previous) = previous {
                let mut old: PassiveTip = serde_json::from_str(&previous)?;
                // A replayed observer frame keeps its first receive time; all evidence must agree.
                old.received = tip.received.clone();
                if canonical(&old)? != payload {
                    return Err(StoreError::Invalid);
                }
                continue;
            }
            sqlx::query("INSERT INTO passive_tips(source,observer,signature,slot,block_key,recipient,received_ms,bytes,payload_json) VALUES(?,?,?,?,?,?,?,?,?)")
                .bind(&source).bind(&observer).bind(&tip.signature).bind(slot_key(tip.slot)).bind(block).bind(&tip.recipient)
                .bind(received_ms).bind(payload.len() as i64).bind(payload).execute(&mut *tx).await?;
        }
        // Retain a deterministic newest suffix whose complete payload fits both caps.
        sqlx::query("DELETE FROM passive_tips WHERE source=? AND rowid IN (SELECT rowid FROM (SELECT rowid,ROW_NUMBER() OVER (ORDER BY received_ms DESC,rowid DESC) AS n,SUM(bytes) OVER (ORDER BY received_ms DESC,rowid DESC) AS total FROM passive_tips WHERE source=?) WHERE n>? OR total>?)")
            .bind(&source).bind(&source).bind(limits.max_rows).bind(limits.max_bytes).execute(&mut *tx).await?;
        let row = sqlx::query("SELECT COUNT(*) AS rows,COALESCE(SUM(bytes),0) AS bytes FROM passive_tips WHERE source=?").bind(&source).fetch_one(&mut *tx).await?;
        let usage = TapeUsage {
            rows: u32::try_from(row.try_get::<i64, _>("rows")?).map_err(|_| StoreError::Invalid)?,
            payload_bytes: u32::try_from(row.try_get::<i64, _>("bytes")?)
                .map_err(|_| StoreError::Invalid)?,
            limits: limits.clone(),
            population: "passive_recipient_transfers_not_owned_training".into(),
        };
        tx.commit().await?;
        Ok(usage)
    }

    /// Reads at most 10,000 passive rows. All timestamps are UTC; values are lamports.
    pub async fn passive_tips(
        &self,
        source: Source,
        from: &str,
        through: &str,
        limit: u32,
    ) -> Result<Vec<PassiveTip>, StoreError> {
        if limit == 0 || limit > 10_000 || millis(from)? > millis(through)? {
            return Err(StoreError::Invalid);
        }
        let rows: Vec<String> = sqlx::query_scalar("SELECT payload_json FROM passive_tips WHERE source=? AND received_ms>=? AND received_ms<=? ORDER BY received_ms DESC,rowid DESC LIMIT ?")
            .bind(label(source)?).bind(millis(from)?).bind(millis(through)?).bind(limit).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|p| serde_json::from_str(&p).map_err(StoreError::from))
            .collect()
    }

    /// Saves an immutable, content-addressed market snapshot with UTC millisecond lookup.
    pub async fn save_market_snapshot(
        &self,
        market: &DelayCostSnapshot,
    ) -> Result<String, StoreError> {
        let payload = canonical(market)?;
        if payload.len() > 64 * 1024
            || market.pool.is_empty()
            || market.regime_id.is_empty()
            || market.points.len() > 256
            || !market.measured_slot_ms.is_finite()
            || market.measured_slot_ms <= 0.0
        {
            return Err(StoreError::Invalid);
        }
        let hash = content_hash(market)?;
        sqlx::query("INSERT INTO market_snapshots(hash,source,regime_id,pool,as_of_ms,payload_json) VALUES(?,?,?,?,?,?) ON CONFLICT(hash) DO NOTHING")
            .bind(&hash).bind(label(market.source)?).bind(&market.regime_id).bind(&market.pool).bind(millis(&market.as_of_utc)?).bind(payload).execute(&self.pool).await?;
        Ok(hash)
    }

    /// Latest same-source and same-regime market document at or before the quote timestamp.
    pub async fn market_snapshot(
        &self,
        source: Source,
        regime: &str,
        pool: &str,
        as_of: &str,
    ) -> Result<Option<DelayCostSnapshot>, StoreError> {
        let row = sqlx::query("SELECT hash,payload_json FROM market_snapshots WHERE source=? AND regime_id=? AND pool=? AND as_of_ms<=? ORDER BY as_of_ms DESC,hash LIMIT 1")
            .bind(label(source)?).bind(regime).bind(pool).bind(millis(as_of)?).fetch_optional(&self.pool).await?;
        row.map(|r| {
            let snapshot: DelayCostSnapshot =
                serde_json::from_str(&r.try_get::<String, _>("payload_json")?)?;
            if content_hash(&snapshot)? != r.try_get::<String, _>("hash")?
                || snapshot.source != source
                || snapshot.regime_id != regime
                || snapshot.pool != pool
            {
                return Err(StoreError::Invalid);
            }
            Ok(snapshot)
        })
        .transpose()
    }
}
