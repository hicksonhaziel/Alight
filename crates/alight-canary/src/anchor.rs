//! Ledger commitments. Offline preparation is separate from explicitly opted-in governed sending.
use alight_store::{Store, StoreError};
use alight_types::*;
use solami::{Instruction, Pubkey};
use std::str::FromStr;

/// Stable memo with lossless decimal sequence and source-scoped SHA-256 head.
pub fn memo(source: Source, sequence: u64, hash: &str) -> Result<String, StoreError> {
    if sequence == 0
        || hash.len() != 71
        || !hash.starts_with("sha256:")
        || !hash[7..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(StoreError::Invalid);
    }
    Ok(format!(
        "alight.ledger.v1|{}|{sequence}|{hash}",
        alight_store::label(source)?
    ))
}
/// Builds the unsigned Solana memo instruction. Amount is zero lamports; network fees would still
/// require an explicitly authorized governed send.
pub fn instruction(draft: &AnchorDraft) -> Result<Instruction, StoreError> {
    if draft.schema_version != 1
        || draft.status != "PREPARED_UNSIGNED"
        || draft.signing_enabled
        || draft.signature.is_some()
        || draft.explorer_url.is_some()
        || draft.memo != memo(draft.source, draft.sequence, &draft.head_hash)?
    {
        return Err(StoreError::Invalid);
    }
    Ok(Instruction {
        program_id: Pubkey::from_str("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr")
            .map_err(|_| StoreError::Invalid)?,
        accounts: vec![],
        data: draft.memo.as_bytes().to_vec(),
    })
}
/// Reads and verifies the source's current ledger head; neither milliseconds nor spend are guessed.
pub async fn prepare(store: &Store, source: Source) -> Result<AnchorDraft, StoreError> {
    let sequence = store.verify_ledger(source).await?;
    if sequence == 0 {
        return Err(StoreError::Invalid);
    }
    // Read the verified prefix by sequence, rather than an unverified newly appended tail.
    let entry = store
        .forecast_page(source, sequence - 1, 1)
        .await?
        .pop()
        .ok_or(StoreError::Invalid)?;
    if entry.sequence != sequence {
        return Err(StoreError::Invalid);
    }
    let draft = AnchorDraft {
        schema_version: 1,
        source,
        sequence,
        head_hash: entry.hash.clone(),
        memo: memo(source, sequence, &entry.hash)?,
        status: "PREPARED_UNSIGNED".into(),
        signing_enabled: false,
        signature: None,
        explorer_url: None,
    };
    instruction(&draft)?;
    Ok(draft)
}
/// Verifies a draft against a retained historical prefix, permitting later ledger appends.
/// This is offline byte verification, not an RPC proof that the memo landed.
pub async fn verify(store: &Store, draft: &AnchorDraft) -> Result<(), StoreError> {
    instruction(draft)?;
    if store.verify_ledger(draft.source).await? < draft.sequence {
        return Err(StoreError::Invalid);
    }
    let entry = store
        .forecast_page(draft.source, draft.sequence - 1, 1)
        .await?
        .pop()
        .ok_or(StoreError::Invalid)?;
    if entry.sequence != draft.sequence || entry.hash != draft.head_hash {
        return Err(StoreError::Invalid);
    }
    Ok(())
}

use alight_ingest::{Config, HttpProbe, MAINNET_GENESIS};
use base64::Engine;
use bincode::Options;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use solana_hash::Hash;
use solana_message::Message;

#[derive(Debug, thiserror::Error)]
pub enum AnchorError {
    #[error("invalid anchor configuration, commitment or RPC proof")]
    Invalid,
    #[error("mainnet sending was not explicitly enabled")]
    NotAuthorized,
    #[error("anchor already attempted or reserved; reconcile its existing signature")]
    AlreadyAttempted,
    #[error("dedicated wallet reserve or maximum fee gate not met")]
    FundsOrFee,
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Probe(#[from] alight_ingest::ProbeError),
    #[error(transparent)]
    Budget(#[from] crate::governor::BudgetError),
}

pub(crate) fn reservation_id(draft: &AnchorDraft) -> String {
    format!(
        "anchor-{}-{}",
        match draft.source {
            Source::Live => "live",
            Source::Sim => "sim",
            Source::Replay => "replay",
        },
        draft.head_hash.trim_start_matches("sha256:")
    )
}
pub(crate) fn message(
    draft: &AnchorDraft,
    payer: Pubkey,
    blockhash: Hash,
) -> Result<Message, StoreError> {
    Ok(Message::new_with_blockhash(
        &[instruction(draft)?],
        Some(&payer),
        &blockhash,
    ))
}
pub struct AnchorPreflight {
    pub draft: AnchorDraft,
    pub payer: String,
    pub fee_lamports: u64,
    pub balance_lamports: u64,
    pub reserve_lamports: u64,
    pub ready: bool,
    message: Message,
    last_valid_block_height: u64,
}
impl AnchorPreflight {
    /// Public, credential-free review. No signer or budget reservation is created by preflight.
    pub fn summary(&self) -> Value {
        json!({"status":if self.ready {"READY_UNSIGNED"} else {"WAITING_FUNDS_OR_FEE"},"network":"mainnet-beta","commitment":self.draft,"payer":self.payer,"quoted_fee_lamports":self.fee_lamports.to_string(),"balance_lamports":self.balance_lamports.to_string(),"reserve_lamports":self.reserve_lamports.to_string(),"signing_enabled":false,"transactions_sent":0})
    }
}

/// Four bounded read-only RPC calls. Quoted fees are lamports, not a hardcoded fee assumption.
pub async fn preflight(
    ledger: &Store,
    source: Source,
    config: &Config,
    max_fee_lamports: u64,
) -> Result<AnchorPreflight, AnchorError> {
    if !(1..=100_000).contains(&max_fee_lamports) {
        return Err(AnchorError::Invalid);
    }
    let draft = prepare(ledger, source).await?;
    let payer = config
        .get("ALIGHT_CANARY_PUBKEY")
        .ok_or(AnchorError::Invalid)?
        .to_owned();
    let key = Pubkey::from_str(&payer).map_err(|_| AnchorError::Invalid)?;
    let http = HttpProbe::new()?;
    if http
        .rpc(config, "getGenesisHash", json!([]))
        .await?
        .as_str()
        != Some(MAINNET_GENESIS)
    {
        return Err(AnchorError::Invalid);
    }
    let block = http
        .rpc(
            config,
            "getLatestBlockhash",
            json!([{"commitment":"confirmed"}]),
        )
        .await?;
    let hash = block["value"]["blockhash"]
        .as_str()
        .ok_or(AnchorError::Invalid)?;
    let message = message(
        &draft,
        key,
        Hash::from_str(hash).map_err(|_| AnchorError::Invalid)?,
    )?;
    let encoded = base64::engine::general_purpose::STANDARD
        .encode(bincode::serialize(&message).map_err(|_| AnchorError::Invalid)?);
    let fee = http
        .rpc(
            config,
            "getFeeForMessage",
            json!([encoded,{"commitment":"confirmed"}]),
        )
        .await?["value"]
        .as_u64()
        .ok_or(AnchorError::Invalid)?;
    let balance = http
        .rpc(
            config,
            "getBalance",
            json!([payer,{"commitment":"confirmed"}]),
        )
        .await?["value"]
        .as_u64()
        .ok_or(AnchorError::Invalid)?;
    let reserve = config
        .get("ALIGHT_CANARY_RESERVE_LAMPORTS")
        .unwrap_or("1000000")
        .parse::<u64>()
        .map_err(|_| AnchorError::Invalid)?
        .max(1_000_000);
    Ok(AnchorPreflight {
        draft,
        payer,
        fee_lamports: fee,
        balance_lamports: balance,
        reserve_lamports: reserve,
        ready: fee > 0 && fee <= max_fee_lamports && fee <= balance.saturating_sub(reserve),
        message,
        last_valid_block_height: block["value"]["lastValidBlockHeight"]
            .as_u64()
            .ok_or(AnchorError::Invalid)?,
    })
}

/// One attempt per source/head, persisted before broadcast, charged even after an unknown result.
/// The caller must use the collector's budget database, shared with all canary routes.
pub async fn send(
    ledger: &Store,
    budgets: &Store,
    source: Source,
    config: &Config,
    max_fee_lamports: u64,
    authorize_mainnet: bool,
) -> Result<AnchorRecord, AnchorError> {
    if !authorize_mainnet {
        return Err(AnchorError::NotAuthorized);
    }
    let p = preflight(ledger, source, config, max_fee_lamports).await?;
    if !p.ready {
        return Err(AnchorError::FundsOrFee);
    }
    let id = reservation_id(&p.draft);
    if budgets.budget_reservation(&id).await?.is_some() {
        return Err(AnchorError::AlreadyAttempted);
    }
    let wallet = crate::builder::Wallet::from_base58(
        config
            .get("ALIGHT_CANARY_KEYPAIR")
            .ok_or(AnchorError::Invalid)?,
        &p.payer,
    )
    .map_err(|_| AnchorError::Invalid)?;
    let governor = crate::governor::Governor::new(
        budgets.clone(),
        RunMode::Live,
        config.get("ALIGHT_DAILY_BUDGET_SOL"),
        config.get("ALIGHT_BURST_BUDGET_SOL"),
        60_000,
    )?;
    let now = chrono::Utc::now();
    let permit = governor
        .reserve(
            &id,
            Route::Rpc,
            p.fee_lamports,
            u64::try_from(now.timestamp_millis()).map_err(|_| AnchorError::Invalid)?,
        )
        .await?;
    let tx = wallet
        .sign_anchor(permit, &p.draft, p.message, p.fee_lamports)
        .map_err(|_| AnchorError::Invalid)?;
    let wire = bincode::serialize(&tx).map_err(|_| AnchorError::Invalid)?;
    let attempt = AnchorAttempt {
        schema_version: 1,
        reservation_id: id.clone(),
        commitment: p.draft,
        payer: p.payer,
        signature: tx
            .signatures
            .first()
            .ok_or(AnchorError::Invalid)?
            .to_string(),
        prepared_at_utc: now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        wire_sha256: format!("sha256:{:x}", Sha256::digest(&wire)),
        quoted_fee_lamports: p.fee_lamports,
        last_valid_block_height: p.last_valid_block_height,
    };
    budgets.prepare_anchor(&attempt).await?;
    let mut routes = crate::routes::Routes::new().map_err(|_| AnchorError::Invalid)?;
    let status = match routes.send(config, Route::Rpc, &tx).await {
        crate::routes::SendResult::Accepted => "ACCEPTED",
        crate::routes::SendResult::Rejected(_) => "REJECTED",
        crate::routes::SendResult::Unknown(_) => "UNKNOWN",
    };
    let event = AnchorEvent {
        reservation_id: id,
        at_utc: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        status: status.into(),
        confirmed_slot: None,
        actual_fee_lamports: None,
    };
    budgets.anchor_event(&event).await?;
    Ok(AnchorRecord {
        source,
        attempt,
        event,
    })
}

/// Verify exact returned wire bytes and successful execution at finalized commitment.
/// Null/timeout is unavailable evidence, never expiry or nonlanding.
pub async fn verify_network(
    ledger: &Store,
    attempt: &AnchorAttempt,
    config: &Config,
) -> Result<AnchorEvent, AnchorError> {
    verify(ledger, &attempt.commitment).await?;
    let http = HttpProbe::new()?;
    if http
        .rpc(config, "getGenesisHash", json!([]))
        .await?
        .as_str()
        != Some(MAINNET_GENESIS)
    {
        return Err(AnchorError::Invalid);
    }
    let result = http.rpc(config,"getTransaction",json!([attempt.signature,{"encoding":"base64","commitment":"finalized","maxSupportedTransactionVersion":0}])).await?;
    network_event(attempt, &result)
}
fn network_event(attempt: &AnchorAttempt, result: &Value) -> Result<AnchorEvent, AnchorError> {
    let encoded = result["transaction"][0]
        .as_str()
        .ok_or(AnchorError::Invalid)?;
    if result["transaction"][1] != "base64" || encoded.len() > 2048 {
        return Err(AnchorError::Invalid);
    }
    let wire = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| AnchorError::Invalid)?;
    if format!("sha256:{:x}", Sha256::digest(&wire)) != attempt.wire_sha256 {
        return Err(AnchorError::Invalid);
    }
    let tx: solami::VersionedTransaction = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(1232)
        .reject_trailing_bytes()
        .deserialize(&wire)
        .map_err(|_| AnchorError::Invalid)?;
    let payer = Pubkey::from_str(&attempt.payer).map_err(|_| AnchorError::Invalid)?;
    let actual = match &tx.message {
        solana_message::VersionedMessage::Legacy(m) => m,
        _ => return Err(AnchorError::Invalid),
    };
    if tx.signatures.len() != 1
        || tx.signatures[0].to_string() != attempt.signature
        || *actual != message(&attempt.commitment, payer, actual.recent_blockhash.clone())?
        || !tx.signatures[0].verify(
            payer.as_ref(),
            &bincode::serialize(actual).map_err(|_| AnchorError::Invalid)?,
        )
        || !result["meta"].is_object()
        || result["meta"].get("err").is_none()
    {
        return Err(AnchorError::Invalid);
    }
    let fee = result["meta"]["fee"].as_u64().ok_or(AnchorError::Invalid)?;
    if fee != attempt.quoted_fee_lamports {
        return Err(AnchorError::Invalid);
    }
    Ok(AnchorEvent {
        reservation_id: attempt.reservation_id.clone(),
        at_utc: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        status: if result["meta"]["err"].is_null() {
            "FINALIZED"
        } else {
            "FINALIZED_FAILED"
        }
        .into(),
        confirmed_slot: Some(result["slot"].as_u64().ok_or(AnchorError::Invalid)?),
        actual_fee_lamports: Some(fee),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        builder::Wallet,
        governor::{BudgetError, Governor},
    };

    #[tokio::test]
    async fn synthetic_anchor_shares_budget_persists_before_send_and_verifies_exact_wire() {
        // This test signs only a fixed synthetic key and never constructs a transport.
        let dir = tempfile::tempdir().expect("directory");
        let path = dir.path().join("budget.db");
        let store = Store::open(&path, 16 * 1024 * 1024).await.expect("store");
        let governor = Governor::new(
            store.clone(),
            RunMode::Live,
            Some("0.00001"),
            Some("0.00001"),
            60_000,
        )
        .expect("governor");
        let hash = format!("sha256:{}", "a".repeat(64));
        let draft = AnchorDraft {
            schema_version: 1,
            source: Source::Sim,
            sequence: 2,
            head_hash: hash.clone(),
            memo: memo(Source::Sim, 2, &hash).expect("memo"),
            status: "PREPARED_UNSIGNED".into(),
            signing_enabled: false,
            signature: None,
            explorer_url: None,
        };
        let wallet = Wallet::from_keypair(solami::Keypair::new_from_array([23; 32]));
        let id = reservation_id(&draft);
        let now = 1_791_583_200_000;
        let permit = governor
            .reserve(&id, Route::Rpc, 5000, now)
            .await
            .expect("reserve");
        let msg = message(&draft, wallet.public(), Hash::new_from_array([7; 32])).expect("message");
        let tx = wallet
            .sign_anchor(permit, &draft, msg.clone(), 5000)
            .expect("sign");
        let wire = bincode::serialize(&tx).expect("wire");
        let attempt = AnchorAttempt {
            schema_version: 1,
            reservation_id: id.clone(),
            commitment: draft.clone(),
            payer: wallet.public().to_string(),
            signature: tx.signatures[0].to_string(),
            prepared_at_utc: "2026-10-10T00:00:00Z".into(),
            wire_sha256: format!("sha256:{:x}", Sha256::digest(&wire)),
            quoted_fee_lamports: 5000,
            last_valid_block_height: 100,
        };
        store
            .prepare_anchor(&attempt)
            .await
            .expect("persist before broadcast");
        assert!(store.prepare_anchor(&attempt).await.is_err());
        assert!(matches!(
            governor
                .reserve("another-canary", Route::BeamHttp, 6000, now)
                .await,
            Err(BudgetError::Denied)
        ));
        assert!(governor.reserve(&id, Route::Rpc, 5000, now).await.is_err());
        let prepared = store.anchors(Source::Sim).await.expect("record");
        assert_eq!(prepared.len(), 1);
        assert_eq!(prepared[0].event.status, "PREPARED");
        assert!(
            store
                .anchors(Source::Live)
                .await
                .expect("source isolation")
                .is_empty()
        );
        let proof = json!({"transaction":[base64::engine::general_purpose::STANDARD.encode(&wire),"base64"],"slot":101,"meta":{"fee":5000,"err":null}});
        let event = network_event(&attempt, &proof).expect("synthetic finalized proof");
        assert_eq!(event.status, "FINALIZED");
        assert!(network_event(&attempt, &Value::Null).is_err());
        let mut bad = proof.clone();
        bad["meta"]["fee"] = json!(6000);
        assert!(network_event(&attempt, &bad).is_err());
        let mut bad = attempt.clone();
        bad.signature = "another-signature".into();
        assert!(network_event(&bad, &proof).is_err());
        let mut bad = attempt.clone();
        bad.commitment.memo.push('x');
        assert!(network_event(&bad, &proof).is_err());
        let mut failed = proof.clone();
        failed["meta"]["err"] = json!({"InstructionError":[0,"InvalidArgument"]});
        assert_eq!(
            network_event(&attempt, &failed)
                .expect("failed execution")
                .status,
            "FINALIZED_FAILED"
        );
        let permit = governor
            .reserve("wrong-memo-permit", Route::Rpc, 5000, now)
            .await
            .expect("remaining cap");
        assert!(wallet.sign_anchor(permit, &draft, msg, 5000).is_err());
        store.close().await;
        let reopened = Store::open(&path, 16 * 1024 * 1024).await.expect("reopen");
        assert_eq!(
            reopened
                .budget_reservation(&id)
                .await
                .expect("charged across restart")
                .expect("reservation")
                .lamports,
            5000
        );
        reopened.anchor_event(&event).await.expect("append final");
        let mut unknown = event;
        unknown.status = "UNKNOWN".into();
        unknown.confirmed_slot = None;
        unknown.actual_fee_lamports = None;
        reopened
            .anchor_event(&unknown)
            .await
            .expect("final state preserved");
        assert_eq!(
            reopened.anchors(Source::Sim).await.expect("read")[0]
                .event
                .status,
            "FINALIZED"
        );
        reopened.close().await;
    }
}
