//! Offline anchor preparation only. No signing identity, governor permit or route sender is loaded.
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
/// require a future governed send. This module exposes no sign or broadcast function.
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
