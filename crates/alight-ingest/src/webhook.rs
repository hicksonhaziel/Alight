//! Captured Solami enriched webhook contract: hexadecimal HMAC-SHA256 over exact body bytes.
use crate::adapter::AdapterError;
use alight_store::raw_ref;
use alight_types::*;
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
pub fn verify(secret: &[u8], header: &str, body: &[u8]) -> bool {
    if secret.len() < 16 || secret.len() > 256 || header.len() != 64 || body.len() > 65536 {
        return false;
    }
    let mut bytes = [0u8; 32];
    for (i, pair) in header.as_bytes().chunks_exact(2).enumerate() {
        let digit = |b: u8| match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        };
        let (Some(a), Some(b)) = (digit(pair[0]), digit(pair[1])) else {
            return false;
        };
        bytes[i] = a * 16 + b;
    }
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&bytes).is_ok()
}
/// Identity and result only; absent block/index fields stay absent. No trader or payload storage.
pub fn normalize(
    body: &[u8],
    source: Source,
    received: ReceiveTime,
) -> Result<(ObserverEvent, Value), AdapterError> {
    if body.len() > 65536 {
        return Err(AdapterError);
    }
    let input: Value = serde_json::from_slice(body).map_err(|_| AdapterError)?;
    let signature = input["signature"]
        .as_str()
        .filter(|s| s.len() <= 90)
        .ok_or(AdapterError)?;
    if bs58::decode(signature)
        .into_vec()
        .map_err(|_| AdapterError)?
        .len()
        != 64
    {
        return Err(AdapterError);
    }
    let slot = input["slot"].as_u64().ok_or(AdapterError)?;
    let success = match input["status"].as_str() {
        Some("succeeded") => Some(true),
        Some(_) => None,
        None => return Err(AdapterError),
    };
    let raw = json!({"signature":signature,"slot":slot.to_string(),"success":success,"contract":"solami-enriched-captured-v1"});
    let event = ObserverEvent {
        observer: ObserverKind::Webhook,
        source,
        signature: signature.into(),
        slot: Some(slot),
        block_id: None,
        index_in_block: None,
        index_scope: IndexScope::Unknown,
        success,
        received,
        raw_ref: raw_ref(&raw).map_err(|_| AdapterError)?,
    };
    Ok((event, raw))
}
