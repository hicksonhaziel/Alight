//! SQLite WAL persistence. Event evidence and cursors commit in the same transaction.
use alight_types::{
    BudgetLimits, BudgetReservation, Canary, IngestEvent, ObserverEvent, ObserverKind, Source,
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{path::Path, time::Duration};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database operation failed")]
    Database(#[from] sqlx::Error),
    #[error("database migration failed")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("record JSON is invalid")]
    Json(#[from] serde_json::Error),
    #[error("database filesystem operation failed")]
    Io(#[from] std::io::Error),
    #[error("invalid record or storage limit")]
    Invalid,
}

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

#[derive(Debug, Serialize)]
pub struct Counts {
    pub slot_events: i64,
    pub blocks: i64,
    pub observations: i64,
    pub gaps: i64,
    pub open_gaps: i64,
    pub canaries: i64,
    pub budget_reserved_lamports: i64,
}

pub fn label<T: Serialize>(value: T) -> Result<String, StoreError> {
    serde_json::to_value(value)?
        .as_str()
        .map(str::to_owned)
        .ok_or(StoreError::Invalid)
}

/// Decimal padding makes SQLite lexical cursor ordering lossless across the full u64 range.
pub fn slot_key(slot: u64) -> String {
    format!("{slot:020}")
}

/// SHA-256 of a captured provider subset encoded as canonical serde JSON, not a wire-frame hash.
pub fn raw_ref(raw: &Value) -> Result<String, StoreError> {
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(raw)?)
    ))
}

impl Store {
    /// Opens a file database with FULL synchronous WAL. max_bytes bounds main-database pages.
    pub async fn open(path: &Path, max_bytes: u64) -> Result<Self, StoreError> {
        if !(16 * 1024 * 1024..=16 * 1024 * 1024 * 1024).contains(&max_bytes) {
            return Err(StoreError::Invalid);
        }
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        let page_size: i64 = sqlx::query_scalar("PRAGMA page_size")
            .fetch_one(&pool)
            .await?;
        let limit = max_bytes / u64::try_from(page_size).map_err(|_| StoreError::Invalid)?;
        sqlx::query(&format!("PRAGMA max_page_count={limit}"))
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA journal_size_limit=8388608")
            .execute(&pool)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// A new process records a restart gap from each previous last receive time.
    pub async fn start_run(&self, id: &str, mode: &str, utc: &str) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO gaps(source,observer,started_utc,ended_utc,reason) SELECT source,observer,last_receive_utc,?,'collector_restart' FROM observer_state")
            .bind(utc).execute(&mut *tx).await?;
        sqlx::query("UPDATE runs SET ended_utc=?,status='interrupted' WHERE ended_utc IS NULL")
            .bind(utc)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO runs(id,mode,started_utc) VALUES(?,?,?)")
            .bind(id)
            .bind(mode)
            .bind(utc)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn end_run(&self, id: &str, utc: &str) -> Result<(), StoreError> {
        sqlx::query("UPDATE runs SET ended_utc=?,status='stopped' WHERE id=?")
            .bind(utc)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Opens one durable gap per observer. Repeated failures do not create a gap storm.
    pub async fn open_gap(
        &self,
        observer: ObserverKind,
        source: Source,
        utc: &str,
        reason: &str,
    ) -> Result<(), StoreError> {
        sqlx::query("INSERT INTO gaps(source,observer,started_utc,reason) SELECT ?,?,?,? WHERE NOT EXISTS(SELECT 1 FROM gaps WHERE source=? AND observer=? AND ended_utc IS NULL)")
            .bind(label(source)?).bind(label(observer)?).bind(utc).bind(reason).bind(label(source)?).bind(label(observer)?).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn close_gap(
        &self,
        observer: ObserverKind,
        source: Source,
        utc: &str,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE gaps SET ended_utc=? WHERE source=? AND observer=? AND ended_utc IS NULL",
        )
        .bind(utc)
        .bind(label(source)?)
        .bind(label(observer)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn cursor(
        &self,
        observer: ObserverKind,
        source: Source,
    ) -> Result<Option<u64>, StoreError> {
        let value: Option<Option<String>> = sqlx::query_scalar(
            "SELECT cursor_slot FROM observer_state WHERE source=? AND observer=?",
        )
        .bind(label(source)?)
        .bind(label(observer)?)
        .fetch_optional(&self.pool)
        .await?;
        value
            .flatten()
            .map(|s| s.parse().map_err(|_| StoreError::Invalid))
            .transpose()
    }

    /// Idempotently preserves the first receive time; different forks/statuses remain separate.
    pub async fn record(
        &self,
        observer: ObserverKind,
        event: &IngestEvent,
        raw: &Value,
    ) -> Result<bool, StoreError> {
        let payload = serde_json::to_string(event)?;
        let (source, slot, receive, reference) = match event {
            IngestEvent::Slot(e) => (e.source, Some(e.slot), &e.received, &e.raw_ref),
            IngestEvent::BlockMeta(e) => (e.source, Some(e.slot), &e.received, &e.raw_ref),
            IngestEvent::Observation(e) => {
                if observer != e.observer {
                    return Err(StoreError::Invalid);
                }
                (e.source, e.slot, &e.received, &e.raw_ref)
            }
        };
        if *reference != raw_ref(raw)? {
            return Err(StoreError::Invalid);
        }
        let raw_json = serde_json::to_string(raw)?;
        if raw_json.len() > 1024 * 1024 {
            return Err(StoreError::Invalid);
        }
        let source = label(source)?;
        let observer = label(observer)?;
        let mut tx = self.pool.begin().await?;
        let affected = match event {
            IngestEvent::Slot(e) => {
                sqlx::query("INSERT OR IGNORE INTO slot_events VALUES(?,?,?,?,?,?)")
                    .bind(&source)
                    .bind(&observer)
                    .bind(slot_key(e.slot))
                    .bind(e.block_id.as_deref().unwrap_or(""))
                    .bind(label(e.status)?)
                    .bind(&payload)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected()
            }
            IngestEvent::BlockMeta(e) => {
                sqlx::query("INSERT OR IGNORE INTO blocks VALUES(?,?,?,?,?)")
                    .bind(&source)
                    .bind(&observer)
                    .bind(slot_key(e.slot))
                    .bind(&e.block_id)
                    .bind(&payload)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected()
            }
            IngestEvent::Observation(e) => {
                sqlx::query("INSERT OR IGNORE INTO observations VALUES(?,?,?,?,?,?,?,?,?)")
                    .bind(&source)
                    .bind(&observer)
                    .bind(&e.signature)
                    .bind(e.slot.map(slot_key).unwrap_or_default())
                    .bind(e.block_id.as_deref().unwrap_or(""))
                    .bind(label(e.index_scope)?)
                    .bind(e.index_in_block.map(|n| n.to_string()).unwrap_or_default())
                    .bind(e.success.map(|b| b.to_string()).unwrap_or_default())
                    .bind(&payload)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected()
            }
        };
        if affected > 0 {
            sqlx::query("INSERT OR IGNORE INTO raw_evidence VALUES(?,?,?)")
                .bind(reference)
                .bind(&raw_json)
                .bind(raw_json.len() as i64)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("INSERT INTO observer_state VALUES(?,?,?,?) ON CONFLICT(source,observer) DO UPDATE SET cursor_slot=CASE WHEN excluded.cursor_slot IS NULL THEN observer_state.cursor_slot WHEN observer_state.cursor_slot IS NULL THEN excluded.cursor_slot ELSE MAX(observer_state.cursor_slot,excluded.cursor_slot) END,last_receive_utc=excluded.last_receive_utc")
            .bind(&source).bind(&observer).bind(slot.map(slot_key)).bind(&receive.wall_utc).execute(&mut *tx).await?;
        // A gap closes only once a normalized frame and cursor are durably saved.
        sqlx::query(
            "UPDATE gaps SET ended_utc=? WHERE source=? AND observer=? AND ended_utc IS NULL",
        )
        .bind(&receive.wall_utc)
        .bind(&source)
        .bind(&observer)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(affected > 0)
    }

    pub async fn counts(&self, source: Source) -> Result<Counts, StoreError> {
        let s = label(source)?;
        let count = |table: &str| format!("SELECT COUNT(*) FROM {table} WHERE source=?");
        Ok(Counts {
            slot_events: sqlx::query_scalar(&count("slot_events"))
                .bind(&s)
                .fetch_one(&self.pool)
                .await?,
            blocks: sqlx::query_scalar(&count("blocks"))
                .bind(&s)
                .fetch_one(&self.pool)
                .await?,
            observations: sqlx::query_scalar(&count("observations"))
                .bind(&s)
                .fetch_one(&self.pool)
                .await?,
            gaps: sqlx::query_scalar(&count("gaps"))
                .bind(&s)
                .fetch_one(&self.pool)
                .await?,
            open_gaps: sqlx::query_scalar(
                "SELECT COUNT(*) FROM gaps WHERE source=? AND ended_utc IS NULL",
            )
            .bind(&s)
            .fetch_one(&self.pool)
            .await?,
            canaries: sqlx::query_scalar(&count("canaries"))
                .bind(&s)
                .fetch_one(&self.pool)
                .await?,
            budget_reserved_lamports: sqlx::query_scalar(
                "SELECT COALESCE(SUM(lamports),0) FROM budget_reservations WHERE source=?",
            )
            .bind(&s)
            .fetch_one(&self.pool)
            .await?,
        })
    }

    pub async fn insert_canary(&self, canary: &Canary) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO canaries(id,source,signature,outcome,payload_json) VALUES(?,?,?,?,?)",
        )
        .bind(&canary.id)
        .bind(label(canary.source)?)
        .bind(&canary.signature)
        .bind(canary.outcome.map(label).transpose()?)
        .bind(serde_json::to_string(canary)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    /// Includes unresolved and provisional landings so later fork evidence can correct them.
    pub async fn pending_canaries(&self) -> Result<Vec<Canary>, StoreError> {
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT payload_json FROM canaries WHERE finalized=0 ORDER BY id")
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|s| serde_json::from_str(&s).map_err(StoreError::from))
            .collect()
    }

    /// Enriches only an unambiguous same-observer block candidate; raw observations never change.
    pub async fn observations(
        &self,
        source: Source,
        signature: &str,
    ) -> Result<Vec<ObserverEvent>, StoreError> {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT payload_json FROM observations WHERE source=? AND signature=?",
        )
        .bind(label(source)?)
        .bind(signature)
        .fetch_all(&self.pool)
        .await?;
        let mut result = Vec::new();
        for json in rows {
            let IngestEvent::Observation(mut e) = serde_json::from_str(&json)? else {
                return Err(StoreError::Invalid);
            };
            if e.block_id.is_none()
                && let Some(slot) = e.slot
            {
                let blocks: Vec<String> = sqlx::query_scalar(
                    "SELECT block_id FROM blocks WHERE source=? AND observer=? AND slot=?",
                )
                .bind(label(source)?)
                .bind(label(e.observer)?)
                .bind(slot_key(slot))
                .fetch_all(&self.pool)
                .await?;
                if blocks.len() == 1 {
                    e.block_id = blocks.first().cloned();
                }
            }
            result.push(e);
        }
        Ok(result)
    }

    /// Atomic reservation; uncertain spend stays charged. Returns false on cap/clock rejection.
    pub async fn reserve_budget(
        &self,
        reservation: &BudgetReservation,
        limits: &BudgetLimits,
    ) -> Result<bool, StoreError> {
        let to_i64 = |n| i64::try_from(n).map_err(|_| StoreError::Invalid);
        let id = &reservation.id;
        let source = reservation.source;
        let route = label(reservation.route)?;
        let day = &reservation.day;
        let now_ms = to_i64(reservation.created_ms)?;
        let amount = to_i64(reservation.lamports)?;
        let daily = to_i64(limits.daily_lamports)?;
        let burst = to_i64(limits.burst_lamports)?;
        let window_ms = to_i64(limits.window_ms)?;
        if now_ms < 0 || amount <= 0 || daily < amount || burst < amount || window_ms <= 0 {
            return Ok(false);
        }
        let result = sqlx::query("INSERT INTO budget_reservations SELECT ?,?,?,?,?,? WHERE ? >= COALESCE((SELECT MAX(created_ms) FROM budget_reservations WHERE source=?),0) AND COALESCE((SELECT SUM(lamports) FROM budget_reservations WHERE source=? AND day=?),0) <= ? AND COALESCE((SELECT SUM(lamports) FROM budget_reservations WHERE source=? AND created_ms>=?),0) <= ? AND ?=strftime('%Y-%m-%d',?/1000,'unixepoch') ON CONFLICT(id) DO NOTHING")
            .bind(id).bind(label(source)?).bind(route).bind(day).bind(now_ms).bind(amount)
            .bind(now_ms).bind(label(source)?).bind(label(source)?).bind(day).bind(daily-amount)
            .bind(label(source)?).bind(now_ms.saturating_sub(window_ms)).bind(burst-amount).bind(day).bind(now_ms).execute(&self.pool).await?;
        Ok(result.rows_affected() == 1)
    }

    /// Saves corrected outcomes plus their complete evidence in one durable transaction.
    pub async fn save_resolution(
        &self,
        canary: &Canary,
        finalized: bool,
        utc: &str,
        evidence: &Value,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO resolution_history(canary_id,at_utc,payload_json) VALUES(?,?,?)")
            .bind(&canary.id)
            .bind(utc)
            .bind(serde_json::to_string(evidence)?)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE canaries SET outcome=?,finalized=?,payload_json=? WHERE id=?")
            .bind(canary.outcome.map(label).transpose()?)
            .bind(finalized)
            .bind(serde_json::to_string(canary)?)
            .bind(&canary.id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn observer_health(
        &self,
        observer: ObserverKind,
        source: Source,
    ) -> Result<Value, StoreError> {
        let last: Option<String> = sqlx::query_scalar(
            "SELECT last_receive_utc FROM observer_state WHERE source=? AND observer=?",
        )
        .bind(label(source)?)
        .bind(label(observer)?)
        .fetch_optional(&self.pool)
        .await?;
        let open: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gaps WHERE source=? AND observer=? AND ended_utc IS NULL",
        )
        .bind(label(source)?)
        .bind(label(observer)?)
        .fetch_one(&self.pool)
        .await?;
        Ok(
            serde_json::json!({"last_receive_utc":last,"open_gaps":open,"cursor_slot":self.cursor(observer,source).await?.map(|s|s.to_string())}),
        )
    }

    /// Bounded candidate metadata for clock warm-up and independent timestamp checks.
    pub async fn block_samples(
        &self,
        observer: ObserverKind,
        source: Source,
    ) -> Result<Vec<alight_types::BlockMetaEvent>, StoreError> {
        let rows:Vec<String>=sqlx::query_scalar("SELECT payload_json FROM blocks WHERE source=? AND observer=? ORDER BY slot DESC LIMIT 2048")
            .bind(label(source)?).bind(label(observer)?).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|s| match serde_json::from_str(&s)? {
                IngestEvent::BlockMeta(e) => Ok(e),
                _ => Err(StoreError::Invalid),
            })
            .collect()
    }

    pub async fn budget_by_route(&self, source: Source, day: &str) -> Result<Value, StoreError> {
        let rows=sqlx::query("SELECT route,SUM(lamports) AS total FROM budget_reservations WHERE source=? AND day=? GROUP BY route")
            .bind(label(source)?).bind(day).fetch_all(&self.pool).await?;
        let mut result = serde_json::Map::new();
        for row in rows {
            let route: String = row.try_get("route")?;
            let total: i64 = row.try_get("total")?;
            result.insert(route, Value::String(total.to_string()));
        }
        Ok(Value::Object(result))
    }

    pub async fn last_receive(&self, source: Source) -> Result<Option<String>, StoreError> {
        Ok(
            sqlx::query("SELECT MAX(last_receive_utc) AS utc FROM observer_state WHERE source=?")
                .bind(label(source)?)
                .fetch_one(&self.pool)
                .await?
                .try_get("utc")?,
        )
    }
}
