use crate::{Store, StoreError, canonical, content_hash, label};
use alight_types::*;
use chrono::DateTime;
use sha2::{Digest, Sha256};
use sqlx::Row;

impl Store {
    /// Stores a bounded replay/Sim envelope by content hash; import never creates Live evidence.
    pub async fn save_wallet_capture(
        &self,
        capture: &WalletHistoryCapture,
    ) -> Result<String, StoreError> {
        let payload = canonical(capture)?;
        if capture.source == Source::Live
            || capture.schema_version != 1
            || capture.rows.len() > 1000
            || payload.len() > 4 * 1024 * 1024
        {
            return Err(StoreError::Invalid);
        }
        let hash = content_hash(capture)?;
        sqlx::query("INSERT INTO wallet_history_captures(hash,source,wallet,payload_json) VALUES(?,?,?,?) ON CONFLICT(hash) DO NOTHING")
            .bind(&hash).bind(label(capture.source)?).bind(&capture.wallet).bind(payload).execute(&self.pool).await?;
        Ok(hash)
    }
    /// Reads and verifies one exact same-source, same-wallet capture. No network access.
    pub async fn wallet_capture(
        &self,
        source: Source,
        wallet: &str,
        hash: &str,
    ) -> Result<Option<WalletHistoryCapture>, StoreError> {
        let value: Option<String> = sqlx::query_scalar("SELECT payload_json FROM wallet_history_captures WHERE hash=? AND source=? AND wallet=?")
            .bind(hash).bind(label(source)?).bind(wallet).fetch_optional(&self.pool).await?;
        value
            .map(|value| {
                let capture: WalletHistoryCapture = serde_json::from_str(&value)?;
                if content_hash(&capture)? != hash
                    || capture.source != source
                    || capture.wallet != wallet
                {
                    return Err(StoreError::Invalid);
                }
                Ok(capture)
            })
            .transpose()
    }
    /// Latest historical cohort for one source/vantage/regime/horizon, never after chain time.
    /// Missing or insufficient cells in the newest cohort do not fall back to older favorable rows.
    pub async fn receipt_curves(
        &self,
        source: Source,
        regime: &str,
        region: &str,
        horizon: u32,
        at: &str,
    ) -> Result<Vec<(String, CurveSnapshot)>, StoreError> {
        let ms = DateTime::parse_from_rfc3339(at)
            .map_err(|_| StoreError::Invalid)?
            .timestamp_millis();
        let rows = sqlx::query("SELECT snapshot_id,payload_json FROM curve_snapshots WHERE source=? AND regime_id=? AND as_of_ms=(SELECT MAX(as_of_ms) FROM curve_snapshots WHERE source=? AND regime_id=? AND as_of_ms<=? AND json_extract(payload_json,'$.context.region')=? AND json_extract(payload_json,'$.horizon_slots')=?) AND json_extract(payload_json,'$.context.region')=? AND json_extract(payload_json,'$.horizon_slots')=? ORDER BY snapshot_id LIMIT 1001")
            .bind(label(source)?).bind(regime).bind(label(source)?).bind(regime).bind(ms).bind(region).bind(horizon).bind(region).bind(horizon).fetch_all(&self.pool).await?;
        if rows.len() > 1000 {
            return Err(StoreError::Invalid);
        }
        rows.iter()
            .map(|row| {
                let payload: String = row.try_get("payload_json")?;
                let id: String = row.try_get("snapshot_id")?;
                let curve: CurveSnapshot = serde_json::from_str(&payload)?;
                curve.validate().map_err(|_| StoreError::Invalid)?;
                if id != format!("sha256:{:x}", Sha256::digest(payload.as_bytes()))
                    || curve.context.source != source
                    || curve.context.regime_id != regime
                    || curve.context.region != region
                    || curve.horizon_slots != horizon
                {
                    return Err(StoreError::Invalid);
                }
                Ok((id, curve))
            })
            .collect()
    }
}
