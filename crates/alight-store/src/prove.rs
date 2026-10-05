use crate::{Store, StoreError, canonical, content_hash, label};
use alight_types::*;
use sqlx::Row;

impl Store {
    /// Immutable lock and initial report commit together, acquiring a source/cell freeze.
    pub async fn lock_prove(&self, lock: &ProveLock) -> Result<ProveReport, StoreError> {
        if !(1..=400).contains(&lock.n) || lock.id.is_empty() || lock.id.len() > 100 {
            return Err(StoreError::Invalid);
        }
        let lock_hash = content_hash(lock)?;
        let cell = content_hash(&(&lock.config, &lock.regime_id, &lock.region))?;
        let source = label(lock.source)?;
        let report = ProveReport {
            lock: lock.clone(),
            lock_hash: lock_hash.clone(),
            as_of_utc: lock.locked_at_utc.clone(),
            state: ProveState::Locked,
            attempts: 0,
            resolved: 0,
            successes: 0,
            unresolved: 0,
            observed_rate: None,
            wilson_interval_95: None,
            verdict: ProveVerdict::Inconclusive,
            reason: Some("awaiting_held_out_canaries".into()),
        };
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO prove_sessions(id,source,forecast_hash,cell_key,lock_hash,lock_json) VALUES(?,?,?,?,?,?)")
            .bind(&lock.id).bind(&source).bind(&lock.forecast_hash).bind(&cell).bind(lock_hash).bind(canonical(lock)?).execute(&mut *tx).await?;
        let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM prove_cells WHERE source=?")
            .bind(&source)
            .fetch_one(&mut *tx)
            .await?;
        if active >= 8 {
            return Err(StoreError::Invalid);
        }
        sqlx::query("INSERT INTO prove_cells(source,cell_key,session_id) VALUES(?,?,?)")
            .bind(source)
            .bind(cell)
            .bind(&lock.id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO prove_state(session_id,status,updated_utc,report_json) VALUES(?,?,?,?)",
        )
        .bind(&lock.id)
        .bind(label(report.state)?)
        .bind(&report.as_of_utc)
        .bind(canonical(&report)?)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(report)
    }

    /// Source-scoped durable report; rejects altered locks or a report with a different claim.
    pub async fn prove(&self, source: Source, id: &str) -> Result<Option<ProveReport>, StoreError> {
        let row=sqlx::query("SELECT p.lock_hash,p.lock_json,s.report_json FROM prove_sessions p JOIN prove_state s ON s.session_id=p.id WHERE p.source=? AND p.id=?")
            .bind(label(source)?).bind(id).fetch_optional(&self.pool).await?;
        row.map(|r| {
            let lock: ProveLock = serde_json::from_str(&r.try_get::<String, _>("lock_json")?)?;
            let report: ProveReport =
                serde_json::from_str(&r.try_get::<String, _>("report_json")?)?;
            if content_hash(&lock)? != r.try_get::<String, _>("lock_hash")?
                || lock.source != source
                || content_hash(&report.lock)? != content_hash(&lock)?
                || report.lock_hash != content_hash(&lock)?
            {
                return Err(StoreError::Invalid);
            }
            Ok(report)
        })
        .transpose()
    }

    /// Bounded Prove reports. Active sessions precede old unresolved reports so a backlog
    /// cannot starve a newly locked cell. Completion may retain unresolved owned outcomes.
    pub async fn proves(
        &self,
        source: Source,
        limit: u32,
        active_only: bool,
    ) -> Result<Vec<ProveReport>, StoreError> {
        if limit == 0 || limit > 100 {
            return Err(StoreError::Invalid);
        }
        let ids:Vec<String>=sqlx::query_scalar("SELECT p.id FROM prove_sessions p JOIN prove_state s ON s.session_id=p.id WHERE p.source=? AND (?=0 OR s.status NOT IN ('COMPLETE','VOIDED') OR json_extract(s.report_json,'$.unresolved')>0) ORDER BY CASE WHEN s.status IN ('COMPLETE','VOIDED') THEN 1 ELSE 0 END,json_extract(p.lock_json,'$.locked_at_utc'),p.id LIMIT ?")
            .bind(label(source)?).bind(active_only).bind(limit).fetch_all(&self.pool).await?;
        let mut result = Vec::new();
        for id in ids {
            result.push(self.prove(source, &id).await?.ok_or(StoreError::Invalid)?);
        }
        Ok(result)
    }
    pub async fn save_prove_report(&self, report: &ProveReport) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        // Acquire the writer lock before comparing progress, so a stale worker cannot
        // undo a completion, a regime void, or outcomes saved by another connection.
        let changed = sqlx::query("UPDATE prove_state SET status=status WHERE session_id=?")
            .bind(&report.lock.id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if changed != 1 {
            return Err(StoreError::Invalid);
        }
        let previous_json: String =
            sqlx::query_scalar("SELECT report_json FROM prove_state WHERE session_id=?")
                .bind(&report.lock.id)
                .fetch_one(&mut *tx)
                .await?;
        let previous: ProveReport = serde_json::from_str(&previous_json)?;
        let timestamp =
            |s: &str| chrono::DateTime::parse_from_rfc3339(s).map_err(|_| StoreError::Invalid);
        if report.lock_hash != previous.lock_hash
            || content_hash(&report.lock)? != previous.lock_hash
            || content_hash(&previous.lock)? != previous.lock_hash
            || timestamp(&report.as_of_utc)? < timestamp(&previous.as_of_utc)?
            || report.attempts < previous.attempts
            || report.resolved < previous.resolved
            || previous.state == ProveState::Voided && report.state != ProveState::Voided
            || previous.state == ProveState::Complete
                && !matches!(report.state, ProveState::Complete | ProveState::Voided)
            || report.attempts > report.lock.n
            || report.successes > report.resolved
            || report.resolved + report.unresolved != report.attempts
        {
            return Err(StoreError::Invalid);
        }
        sqlx::query(
            "UPDATE prove_state SET status=?,updated_utc=?,report_json=? WHERE session_id=?",
        )
        .bind(label(report.state)?)
        .bind(&report.as_of_utc)
        .bind(canonical(report)?)
        .bind(&report.lock.id)
        .execute(&mut *tx)
        .await?;
        if matches!(report.state, ProveState::Complete | ProveState::Voided) {
            sqlx::query("DELETE FROM prove_cells WHERE session_id=?")
                .bind(&report.lock.id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
    /// Complete prospective membership, not all coincidentally matching canaries.
    pub async fn prove_canaries(
        &self,
        source: Source,
        id: &str,
    ) -> Result<Vec<TrainingCanary>, StoreError> {
        let rows=sqlx::query("SELECT c.payload_json,c.finalized,COALESCE(m.payload_json,'{}') AS covariates FROM prove_attempts p JOIN prove_sessions s ON s.id=p.session_id JOIN canaries c ON c.id=p.canary_id LEFT JOIN canary_model_context m ON m.canary_id=c.id WHERE s.source=? AND p.session_id=? ORDER BY p.ordinal")
            .bind(label(source)?).bind(id).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|r| {
                Ok(TrainingCanary {
                    canary: serde_json::from_str(&r.try_get::<String, _>("payload_json")?)?,
                    finalized: r.try_get("finalized")?,
                    covariates: serde_json::from_str(&r.try_get::<String, _>("covariates")?)?,
                })
            })
            .collect()
    }
    /// Simulated member and held-out link commit atomically, after a durable Sim reservation.
    pub async fn save_prove_simulated(
        &self,
        lock: &ProveLock,
        ordinal: u32,
        sample: &TrainingCanary,
    ) -> Result<(), StoreError> {
        let c = &sample.canary;
        let cost = i64::try_from(
            (u128::from(c.config.cu_price_micro_lamports) * u128::from(c.config.cu_limit))
                .div_ceil(1_000_000)
                + u128::from(c.config.tip_lamports)
                + 5000,
        )
        .map_err(|_| StoreError::Invalid)?;
        if lock.source != Source::Sim
            || c.source != Source::Sim
            || c.config != lock.config
            || c.regime_id != lock.regime_id
            || ordinal >= lock.n
        {
            return Err(StoreError::Invalid);
        }
        let mut tx = self.pool.begin().await?;
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM prove_cells WHERE source='sim' AND session_id=?",
        )
        .bind(&lock.id)
        .fetch_one(&mut *tx)
        .await?;
        let reserved: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM budget_reservations WHERE id=? AND source='sim' AND route=? AND lamports=?",
        )
        .bind(&c.id)
        .bind(label(c.config.route)?)
        .bind(cost)
        .fetch_one(&mut *tx)
        .await?;
        let stored: String = sqlx::query_scalar("SELECT lock_hash FROM prove_sessions WHERE id=?")
            .bind(&lock.id)
            .fetch_one(&mut *tx)
            .await?;
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM prove_attempts WHERE session_id=?")
                .bind(&lock.id)
                .fetch_one(&mut *tx)
                .await?;
        if exists != 1
            || reserved != 1
            || content_hash(lock)? != stored
            || count != i64::from(ordinal)
        {
            return Err(StoreError::Invalid);
        }
        sqlx::query("INSERT INTO canaries(id,source,signature,outcome,finalized,payload_json) VALUES(?,'sim',?,?,?,?)")
            .bind(&c.id).bind(&c.signature).bind(c.outcome.map(label).transpose()?).bind(sample.finalized).bind(serde_json::to_string(c)?).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO canary_model_context(canary_id,payload_json) VALUES(?,?)")
            .bind(&c.id)
            .bind(serde_json::to_string(&sample.covariates)?)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO prove_attempts(session_id,ordinal,canary_id) VALUES(?,?,?)")
            .bind(&lock.id)
            .bind(ordinal)
            .bind(&c.id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
