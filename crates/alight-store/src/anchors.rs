use crate::{Store, StoreError, canonical, content_hash, label};
use alight_types::*;
use sqlx::Row;

impl Store {
    /// Requires an already charged Live/RPC reservation. This is the last durable gate before send.
    pub async fn prepare_anchor(&self, attempt: &AnchorAttempt) -> Result<(), StoreError> {
        let reservation = self
            .budget_reservation(&attempt.reservation_id)
            .await?
            .ok_or(StoreError::Invalid)?;
        if attempt.schema_version != 1
            || reservation.source != Source::Live
            || reservation.route != Route::Rpc
            || reservation.lamports != attempt.quoted_fee_lamports
            || attempt.signature.is_empty()
            || chrono::DateTime::parse_from_rfc3339(&attempt.prepared_at_utc).is_err()
        {
            return Err(StoreError::Invalid);
        }
        let payload = canonical(attempt)?;
        if payload.len() > 4096 {
            return Err(StoreError::Invalid);
        }
        let mut transaction = self.pool.begin().await?;
        sqlx::query("INSERT INTO ledger_anchor_attempts(reservation_id,source,head_hash,signature,content_hash,payload_json) VALUES(?,?,?,?,?,?)")
            .bind(&attempt.reservation_id).bind(label(attempt.commitment.source)?).bind(&attempt.commitment.head_hash)
            .bind(&attempt.signature).bind(content_hash(attempt)?).bind(payload).execute(&mut *transaction).await?;
        let event = AnchorEvent {
            reservation_id: attempt.reservation_id.clone(),
            at_utc: attempt.prepared_at_utc.clone(),
            status: "PREPARED".into(),
            confirmed_slot: None,
            actual_fee_lamports: None,
        };
        sqlx::query("INSERT INTO ledger_anchor_events(reservation_id,status,payload_json) VALUES(?,'PREPARED',?)")
            .bind(&attempt.reservation_id).bind(canonical(&event)?).execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(())
    }
    /// Append an acknowledgement or RPC-verified final result; final results cannot be overwritten.
    pub async fn anchor_event(&self, event: &AnchorEvent) -> Result<(), StoreError> {
        if ![
            "ACCEPTED",
            "REJECTED",
            "UNKNOWN",
            "FINALIZED",
            "FINALIZED_FAILED",
        ]
        .contains(&event.status.as_str())
            || chrono::DateTime::parse_from_rfc3339(&event.at_utc).is_err()
            || (if event.status.starts_with("FINALIZED") {
                event.confirmed_slot.is_none() || event.actual_fee_lamports.is_none()
            } else {
                event.confirmed_slot.is_some() || event.actual_fee_lamports.is_some()
            })
        {
            return Err(StoreError::Invalid);
        }
        // The INSERT predicate is atomic across concurrent verifiers and restarts.
        sqlx::query("INSERT INTO ledger_anchor_events(reservation_id,status,payload_json) SELECT ?,?,? WHERE EXISTS(SELECT 1 FROM ledger_anchor_attempts WHERE reservation_id=?) AND NOT EXISTS(SELECT 1 FROM ledger_anchor_events WHERE reservation_id=? AND status IN ('FINALIZED','FINALIZED_FAILED'))")
            .bind(&event.reservation_id).bind(&event.status).bind(canonical(event)?).bind(&event.reservation_id).bind(&event.reservation_id).execute(&self.pool).await?;
        Ok(())
    }
    /// At most 100 source-labelled attempts with their latest immutable event, newest first.
    pub async fn anchors(&self, source: Source) -> Result<Vec<AnchorRecord>, StoreError> {
        let rows = sqlx::query("SELECT a.reservation_id,a.head_hash,a.signature,a.content_hash,a.payload_json,e.status,e.payload_json AS event_json FROM ledger_anchor_attempts a JOIN ledger_anchor_events e ON e.id=(SELECT MAX(id) FROM ledger_anchor_events WHERE reservation_id=a.reservation_id) WHERE a.source=? ORDER BY e.id DESC LIMIT 100")
            .bind(label(source)?).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                let attempt: AnchorAttempt =
                    serde_json::from_str(&row.try_get::<String, _>("payload_json")?)?;
                let event: AnchorEvent =
                    serde_json::from_str(&row.try_get::<String, _>("event_json")?)?;
                if content_hash(&attempt)? != row.try_get::<String, _>("content_hash")?
                    || attempt.commitment.source != source
                    || attempt.reservation_id != event.reservation_id
                    || attempt.reservation_id != row.try_get::<String, _>("reservation_id")?
                    || attempt.commitment.head_hash != row.try_get::<String, _>("head_hash")?
                    || attempt.signature != row.try_get::<String, _>("signature")?
                    || event.status != row.try_get::<String, _>("status")?
                {
                    return Err(StoreError::Invalid);
                }
                Ok(AnchorRecord {
                    source,
                    attempt,
                    event,
                })
            })
            .collect()
    }
}
