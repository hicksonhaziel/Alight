use crate::{Store, StoreError, canonical, content_hash, label};
use alight_types::*;
use chrono::DateTime;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use sqlx::Row;

fn time(s: &str) -> Result<i64, StoreError> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.timestamp_millis())
        .map_err(|_| StoreError::Invalid)
}
fn bytes<T: Serialize>(v: &T, limit: usize) -> Result<String, StoreError> {
    let s = canonical(v)?;
    if s.len() > limit {
        return Err(StoreError::Invalid);
    }
    Ok(s)
}
impl Store {
    pub async fn diagnostic_alerts(
        &self,
        source: Source,
        as_of: &str,
    ) -> Result<Vec<Alert>, StoreError> {
        time(as_of)?;
        let rows:Vec<String>=sqlx::query_scalar("SELECT payload_json FROM alerts WHERE source=? AND julianday(at_utc)<=julianday(?) ORDER BY julianday(at_utc) DESC,id LIMIT 50")
            .bind(label(source)?).bind(as_of).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|s| serde_json::from_str(s).map_err(StoreError::from))
            .collect()
    }
    pub async fn straddling_forecasts(
        &self,
        source: Source,
        at: &str,
    ) -> Result<Vec<ForecastEntry>, StoreError> {
        let rows:Vec<String>=sqlx::query_scalar("SELECT payload_json FROM forecast_ledger WHERE source=? AND julianday(json_extract(payload_json,'$.created_at_utc'))<julianday(?) AND julianday(json_extract(payload_json,'$.expires_at_utc'))>=julianday(?) ORDER BY sequence LIMIT 10000")
            .bind(label(source)?).bind(at).bind(at).fetch_all(&self.pool).await?;
        let mut entries = Vec::new();
        for row in rows {
            let forecast: Forecast = serde_json::from_str(&row)?;
            if let Some(entry) = self.forecast(source, &forecast.id).await? {
                entries.push(entry);
            }
        }
        Ok(entries)
    }
    /// Persists immutable diagnostic windows, retaining the latest 4,096 per source/origin.
    pub async fn save_diagnostic_window(&self, w: &SignalWindow) -> Result<bool, StoreError> {
        if !w.origin.matches(w.source)
            || w.id.len() > 160
            || w.id.is_empty()
            || time(&w.from_utc)? > time(&w.through_utc)?
            || w.measures.len() > 16
            || w.start_slot.zip(w.end_slot).is_some_and(|(a, b)| a > b)
        {
            return Err(StoreError::Invalid);
        }
        let mut names = std::collections::BTreeSet::new();
        for m in &w.measures {
            if !names.insert(m.kind)
                || m.unit.len() > 32
                || m.provenance.len() > 512
                || m.value.is_some_and(|v| !v.is_finite() || m.n == 0)
                || (m.value.is_none() && m.unavailable_reason.is_none())
            {
                return Err(StoreError::Invalid);
            }
        }
        let payload = bytes(w, 16384)?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE diagnostic_windows SET through_utc=through_utc WHERE id=?")
            .bind(&w.id)
            .execute(&mut *tx)
            .await?;
        let previous: Option<String> =
            sqlx::query_scalar("SELECT payload_json FROM diagnostic_windows WHERE id=?")
                .bind(&w.id)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(previous) = previous {
            if previous != payload {
                return Err(StoreError::Invalid);
            }
            return Ok(false);
        }
        sqlx::query("INSERT INTO diagnostic_windows VALUES(?,?,?,?,?)")
            .bind(&w.id)
            .bind(label(w.source)?)
            .bind(label(w.origin)?)
            .bind(&w.through_utc)
            .bind(payload)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM diagnostic_windows WHERE id IN (SELECT id FROM diagnostic_windows WHERE source=? AND origin=? ORDER BY julianday(through_utc) DESC,id DESC LIMIT -1 OFFSET 4096)")
            .bind(label(w.source)?).bind(label(w.origin)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(true)
    }
    pub async fn diagnostic_windows(
        &self,
        source: Source,
        origin: Option<SignalOrigin>,
        as_of: &str,
        limit: u32,
    ) -> Result<Vec<SignalWindow>, StoreError> {
        if !(1..=4096).contains(&limit) {
            return Err(StoreError::Invalid);
        }
        time(as_of)?;
        let rows:Vec<String>=sqlx::query_scalar("SELECT payload_json FROM diagnostic_windows WHERE source=? AND (? IS NULL OR origin=?) AND julianday(through_utc)<=julianday(?) ORDER BY julianday(through_utc) DESC,id DESC LIMIT ?")
            .bind(label(source)?).bind(origin.map(label).transpose()?).bind(origin.map(label).transpose()?).bind(as_of).bind(limit).fetch_all(&self.pool).await?;
        rows.iter()
            .rev()
            .map(|r| serde_json::from_str(r).map_err(StoreError::from))
            .collect()
    }
    pub async fn detector_checkpoint(
        &self,
        source: Source,
        origin: SignalOrigin,
    ) -> Result<Option<(String, Value)>, StoreError> {
        let r = sqlx::query(
            "SELECT window_id,payload_json FROM detector_checkpoints WHERE source=? AND origin=?",
        )
        .bind(label(source)?)
        .bind(label(origin)?)
        .fetch_optional(&self.pool)
        .await?;
        r.map(|r| {
            Ok((
                r.try_get("window_id")?,
                serde_json::from_str(&r.try_get::<String, _>("payload_json")?)?,
            ))
        })
        .transpose()
    }
    /// CAS checkpoint and all regime actions commit together. Forecast bodies never change.
    pub async fn commit_detection(
        &self,
        w: &SignalWindow,
        previous: Option<&str>,
        state: &Value,
        change: Option<&RegimeChange>,
        grades: &[ForecastGrade],
        alert: Option<&Alert>,
    ) -> Result<bool, StoreError> {
        let payload = bytes(state, 1024 * 1024)?;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE detector_checkpoints SET window_id=window_id WHERE source=? AND origin=?",
        )
        .bind(label(w.source)?)
        .bind(label(w.origin)?)
        .execute(&mut *tx)
        .await?;
        let old: Option<String> = sqlx::query_scalar(
            "SELECT window_id FROM detector_checkpoints WHERE source=? AND origin=?",
        )
        .bind(label(w.source)?)
        .bind(label(w.origin)?)
        .fetch_optional(&mut *tx)
        .await?;
        if old.as_deref() != previous {
            tx.rollback().await?;
            return Ok(false);
        }
        let saved: Option<String> =
            sqlx::query_scalar("SELECT payload_json FROM diagnostic_windows WHERE id=?")
                .bind(&w.id)
                .fetch_optional(&mut *tx)
                .await?;
        if saved.as_deref() != Some(canonical(w)?.as_str()) {
            return Err(StoreError::Invalid);
        }
        if let Some(c) = change {
            if c.source != w.source
                || c.origin != w.origin
                || c.signal_window_id != w.id
                || c.old_effective_n_cap != 0.0
                || c.exploration_fraction
                    != if c.origin == SignalOrigin::Backfill {
                        0.0
                    } else {
                        1.0
                    }
                || c.votes.is_empty()
                || !c.confidence.is_finite()
                || !(0.0..=1.0).contains(&c.confidence)
                || time(&c.exploration_until_utc)?
                    != time(&c.detected_at_utc)?
                        + if c.origin == SignalOrigin::Backfill {
                            0
                        } else {
                            1_800_000
                        }
            {
                return Err(StoreError::Invalid);
            }
            sqlx::query("INSERT INTO regime_changes VALUES(?,?,?,?,?)")
                .bind(&c.id)
                .bind(label(c.source)?)
                .bind(&c.detected_at_utc)
                .bind(&c.regime_id)
                .bind(bytes(c, 16384)?)
                .execute(&mut *tx)
                .await?;
            if c.origin == SignalOrigin::Backfill && !grades.is_empty() {
                return Err(StoreError::Invalid);
            }
            for grade in grades {
                let source: Option<String> =
                    sqlx::query_scalar("SELECT source FROM forecast_ledger WHERE hash=?")
                        .bind(&grade.forecast_hash)
                        .fetch_optional(&mut *tx)
                        .await?;
                if source.as_deref() != Some(label(w.source)?.as_str())
                    || grade.status != ForecastStatus::Voided
                    || grade.reason.as_deref() != Some("regime_changed")
                {
                    return Err(StoreError::Invalid);
                }
                sqlx::query("INSERT INTO forecast_grades(forecast_hash,graded_at_utc,payload_json) VALUES(?,?,?) ON CONFLICT DO NOTHING")
                    .bind(&grade.forecast_hash).bind(&grade.graded_at_utc).bind(bytes(grade,512*1024)?).execute(&mut *tx).await?;
            }
            if c.origin != SignalOrigin::Backfill {
                // Beyond the bounded detailed scorer, every crossing claim still receives a void grade.
                // No invented through-change score is inserted when it has not been computed.
                sqlx::query("INSERT INTO forecast_grades(forecast_hash,graded_at_utc,payload_json) SELECT f.hash,?,json_object('forecast_hash',f.hash,'graded_at_utc',?,'status','VOIDED','unresolved',0,'scores',NULL,'through_change_scores',NULL,'latency_coverage',NULL,'baselines',json('[]'),'reason','regime_changed') FROM forecast_ledger f WHERE f.source=? AND julianday(json_extract(f.payload_json,'$.created_at_utc'))<julianday(?) AND julianday(json_extract(f.payload_json,'$.expires_at_utc'))>=julianday(?) AND NOT EXISTS(SELECT 1 FROM forecast_grades g WHERE g.forecast_hash=f.hash AND g.graded_at_utc=? AND json_extract(g.payload_json,'$.status')='VOIDED') ON CONFLICT DO NOTHING")
                    .bind(&c.detected_at_utc).bind(&c.detected_at_utc).bind(label(c.source)?).bind(&c.detected_at_utc).bind(&c.detected_at_utc).bind(&c.detected_at_utc).execute(&mut *tx).await?;
            }
            let alert = alert
                .filter(|a| a.source == w.source && a.rule == AlertRule::RegimeChange)
                .ok_or(StoreError::Invalid)?;
            sqlx::query("INSERT INTO alerts(id,source,at_utc,payload_json) VALUES(?,?,?,?) ON CONFLICT DO NOTHING")
                .bind(&alert.id).bind(label(alert.source)?).bind(&alert.at_utc).bind(bytes(alert,16384)?).execute(&mut *tx).await?;
            // The dedicated event already emitted the regime alert; align the rule baseline to prevent a duplicate.
            if c.origin != SignalOrigin::Backfill
                && let Some(row) =
                    sqlx::query("SELECT payload_json FROM alert_state WHERE source=?")
                        .bind(label(w.source)?)
                        .fetch_optional(&mut *tx)
                        .await?
            {
                let mut s: AlertState =
                    serde_json::from_str(&row.try_get::<String, _>("payload_json")?)?;
                s.regime_id = Some(c.regime_id.clone());
                sqlx::query("UPDATE alert_state SET hash=?,payload_json=? WHERE source=?")
                    .bind(content_hash(&s)?)
                    .bind(canonical(&s)?)
                    .bind(label(w.source)?)
                    .execute(&mut *tx)
                    .await?;
            }
        } else if !grades.is_empty() || alert.is_some() {
            return Err(StoreError::Invalid);
        }
        sqlx::query("INSERT INTO detector_checkpoints VALUES(?,?,?,?) ON CONFLICT(source,origin) DO UPDATE SET window_id=excluded.window_id,payload_json=excluded.payload_json")
            .bind(label(w.source)?).bind(label(w.origin)?).bind(&w.id).bind(payload).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(true)
    }
    pub async fn regime_changes(
        &self,
        source: Source,
        as_of: &str,
    ) -> Result<Vec<RegimeChange>, StoreError> {
        let rows:Vec<String>=sqlx::query_scalar("SELECT payload_json FROM regime_changes WHERE source=? AND julianday(detected_at_utc)<=julianday(?) ORDER BY julianday(detected_at_utc) DESC LIMIT 100")
            .bind(label(source)?).bind(as_of).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|s| serde_json::from_str(s).map_err(StoreError::from))
            .collect()
    }
    /// Backfill events never become the live model's active regime.
    pub async fn active_regime(
        &self,
        source: Source,
        as_of: &str,
    ) -> Result<Option<RegimeChange>, StoreError> {
        Ok(self
            .regime_changes(source, as_of)
            .await?
            .into_iter()
            .find(|r| r.origin != SignalOrigin::Backfill))
    }
    pub async fn save_disagreement(&self, event: &DisagreementEvent) -> Result<bool, StoreError> {
        time(&event.detected_at_utc)?;
        if event.evidence_refs.len() > 200
            || event.missing_observers.len() > 4
            || event.canary_id.len() > 160
        {
            return Err(StoreError::Invalid);
        }
        let inserted=sqlx::query("INSERT INTO observer_disagreements(id,source,canary_id,detected_at_utc,payload_json,last_observed_utc) VALUES(?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET last_observed_utc=excluded.last_observed_utc WHERE julianday(excluded.last_observed_utc)>julianday(observer_disagreements.last_observed_utc)")
            .bind(&event.id).bind(label(event.source)?).bind(&event.canary_id).bind(&event.detected_at_utc).bind(bytes(event,16384)?).bind(&event.detected_at_utc).execute(&self.pool).await?.rows_affected()>0;
        Ok(inserted)
    }
    pub async fn disagreement_events(
        &self,
        source: Source,
        as_of: &str,
    ) -> Result<Vec<DisagreementEvent>, StoreError> {
        time(as_of)?;
        let rows:Vec<String>=sqlx::query_scalar("SELECT payload_json FROM observer_disagreements WHERE source=? AND julianday(detected_at_utc)<=julianday(?) ORDER BY julianday(detected_at_utc) DESC,id LIMIT 100")
            .bind(label(source)?).bind(as_of).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|s| serde_json::from_str(s).map_err(StoreError::from))
            .collect()
    }
    pub async fn diagnostic_canary(
        &self,
        source: Source,
        signature: &str,
    ) -> Result<Option<Canary>, StoreError> {
        let row: Option<String> = sqlx::query_scalar(
            "SELECT payload_json FROM canaries WHERE source=? AND signature=? LIMIT 1",
        )
        .bind(label(source)?)
        .bind(signature)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|s| serde_json::from_str(&s).map_err(StoreError::from))
            .transpose()
    }
    pub async fn save_diagnostics(&self, page: &DiagnosticsPage) -> Result<(), StoreError> {
        time(&page.as_of_utc)?;
        sqlx::query("INSERT INTO diagnostic_snapshots VALUES(?,?,?) ON CONFLICT(source) DO UPDATE SET as_of_utc=excluded.as_of_utc,payload_json=excluded.payload_json WHERE julianday(excluded.as_of_utc)>=julianday(diagnostic_snapshots.as_of_utc)")
            .bind(label(page.source)?).bind(&page.as_of_utc).bind(bytes(page,2*1024*1024)?).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn diagnostics(
        &self,
        source: Source,
        as_of: &str,
    ) -> Result<Option<DiagnosticsPage>, StoreError> {
        let payload:Option<String>=sqlx::query_scalar("SELECT payload_json FROM diagnostic_snapshots WHERE source=? AND julianday(as_of_utc)<=julianday(?)")
            .bind(label(source)?).bind(as_of).fetch_optional(&self.pool).await?;
        payload
            .map(|p| serde_json::from_str(&p).map_err(StoreError::from))
            .transpose()
    }
    pub async fn save_backfill_report(&self, report: &BackfillReport) -> Result<(), StoreError> {
        if report.source != Source::Replay || report.requested_days > 56 || report.requests > 1024 {
            return Err(StoreError::Invalid);
        }
        sqlx::query("INSERT INTO backfill_reports VALUES(?,?,?) ON CONFLICT DO NOTHING")
            .bind(&report.id)
            .bind(label(report.source)?)
            .bind(bytes(report, 16384)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn backfill_reports(
        &self,
        source: Source,
    ) -> Result<Vec<BackfillReport>, StoreError> {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT payload_json FROM backfill_reports WHERE source=? ORDER BY rowid DESC LIMIT 20",
        )
        .bind(label(source)?)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|s| serde_json::from_str(s).map_err(StoreError::from))
            .collect()
    }
    pub async fn pending_alerts(&self, source: Source) -> Result<Vec<Alert>, StoreError> {
        let rows:Vec<String>=sqlx::query_scalar("SELECT payload_json FROM alerts WHERE source=? AND delivery_status='PENDING' ORDER BY at_utc,id LIMIT 128")
            .bind(label(source)?).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|s| serde_json::from_str(s).map_err(StoreError::from))
            .collect()
    }
    pub async fn diagnostic_slot_events(
        &self,
        source: Source,
    ) -> Result<Vec<SlotEvent>, StoreError> {
        let rows:Vec<String>=sqlx::query_scalar("SELECT payload_json FROM slot_events WHERE source=? AND observer='grpc' ORDER BY slot DESC LIMIT 4096")
            .bind(label(source)?).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|s| match serde_json::from_str(s)? {
                IngestEvent::Slot(s) => Ok(s),
                _ => Err(StoreError::Invalid),
            })
            .collect()
    }
    pub async fn save_block_diagnostic<T: Serialize>(
        &self,
        source: Source,
        slot: u64,
        block_id: &str,
        sample: &T,
    ) -> Result<(), StoreError> {
        sqlx::query("INSERT INTO block_diagnostic_samples VALUES(?,?,?,?) ON CONFLICT DO NOTHING")
            .bind(label(source)?)
            .bind(crate::slot_key(slot))
            .bind(block_id)
            .bind(bytes(sample, 16384)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn block_diagnostics<T: DeserializeOwned>(
        &self,
        source: Source,
    ) -> Result<Vec<T>, StoreError> {
        let rows:Vec<String>=sqlx::query_scalar("SELECT payload_json FROM block_diagnostic_samples WHERE source=? ORDER BY slot DESC LIMIT 100")
            .bind(label(source)?).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|s| serde_json::from_str(s).map_err(StoreError::from))
            .collect()
    }
}
