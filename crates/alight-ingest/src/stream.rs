//! Native read-only observer workers; no signing identities are loaded.
use crate::{Config, adapter::normalize};
use alight_store::Store;
use alight_types::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{SecondsFormat, Utc};
use futures_util::{SinkExt, StreamExt};
use prost::Message;
use serde_json::{Value, json};
use solami::{
    geyser::{self, subscribe_update::UpdateOneof},
    grpc::connect_public,
};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message as WsMessage, protocol::WebSocketConfig},
};

pub fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub struct ReceiveClock {
    id: String,
    start: Instant,
}
impl Default for ReceiveClock {
    fn default() -> Self {
        Self::new()
    }
}
impl ReceiveClock {
    pub fn new() -> Self {
        let (id, start) = alight_types::process_clock_origin();
        Self { id, start }
    }
    pub fn receive(&self) -> ReceiveTime {
        ReceiveTime {
            clock_id: self.id.clone(),
            mono_ns: u64::try_from(self.start.elapsed().as_nanos()).unwrap_or(u64::MAX),
            wall_utc: utc_now(),
        }
    }
}

pub struct Frame {
    pub observer: ObserverKind,
    pub event: IngestEvent,
    pub raw: Value,
    pub passive_tips: Result<Vec<PassiveTip>, &'static str>,
}

// Credentials never implement Debug/Serialize and are never placed into error strings.
pub struct GrpcOptions {
    pub url: String,
    pub token: String,
    pub accounts: Vec<String>,
    pub tip_accounts: Vec<String>,
    pub replay: bool,
    pub force_disconnect_after: Option<Duration>,
}
pub struct MirageOptions {
    pub url: String,
    pub tip_accounts: Vec<String>,
}

pub async fn options(
    config: &Config,
) -> Result<(GrpcOptions, Option<MirageOptions>), &'static str> {
    let url = config
        .get("SOLAMI_GRPC_URL")
        .ok_or("missing gRPC endpoint")?
        .to_owned();
    let parsed = reqwest::Url::parse(&url).map_err(|_| "invalid gRPC endpoint")?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() {
        return Err("HTTPS gRPC endpoint required");
    }
    let token = config
        .get("SOLAMI_GRPC_TOKEN")
        .or_else(|| config.get("SOLAMI_API_KEY"))
        .ok_or("missing gRPC credential")?
        .to_owned();
    let api = config
        .get("SOLAMI_DATA_API_URL")
        .ok_or("missing metadata endpoint")?;
    let tips = crate::HttpProbe::new()
        .map_err(|_| "metadata client")?
        .get(
            &format!("{}/onchain/tip-addresses", api.trim_end_matches('/')),
            None,
        )
        .await
        .map_err(|_| "tip-address request failed")?;
    let mut accounts = Vec::new();
    for t in tips.as_array().ok_or("invalid tip-address list")? {
        let t = t.as_str().ok_or("invalid tip address")?;
        if bs58::decode(t)
            .into_vec()
            .map_err(|_| "invalid tip address")?
            .len()
            != 32
        {
            return Err("invalid tip address");
        }
        accounts.push(t.to_owned());
    }
    let tip_accounts = accounts.clone();
    if let Some(payer) = config.get("ALIGHT_CANARY_PUBKEY") {
        if bs58::decode(payer)
            .into_vec()
            .map_err(|_| "invalid public payer")?
            .len()
            != 32
        {
            return Err("invalid public payer");
        }
        accounts.push(payer.to_owned());
    }
    if accounts.is_empty() {
        return Err("account filter must not be empty");
    }
    let replay = config.get("ALIGHT_GRPC_REPLAY_FROM_SLOT") == Some("true");
    let mirage = if config.get("ALIGHT_MIRAGE_ENABLED") == Some("false") {
        None
    } else {
        match (
            config.get("SOLAMI_MIRAGE_URL"),
            config.get("SOLAMI_MIRAGE_TOKEN"),
        ) {
            (Some(url), Some(token)) => {
                let mut url = reqwest::Url::parse(url).map_err(|_| "invalid Mirage endpoint")?;
                if url.scheme() != "wss" {
                    return Err("TLS Mirage endpoint required");
                }
                let pairs: Vec<(String, String)> = url
                    .query_pairs()
                    .filter(|(k, _)| k != "api_key")
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect();
                url.set_query(None);
                url.query_pairs_mut()
                    .extend_pairs(pairs)
                    .append_pair("api_key", token);
                Some(MirageOptions {
                    url: url.to_string(),
                    tip_accounts: tip_accounts.clone(),
                })
            }
            _ => None,
        }
    };
    Ok((
        GrpcOptions {
            url,
            token,
            accounts,
            tip_accounts,
            replay,
            force_disconnect_after: None,
        },
        mirage,
    ))
}

fn selected_update(update: geyser::SubscribeUpdate) -> Result<Option<Value>, &'static str> {
    Ok(match update.update_oneof {
        Some(UpdateOneof::Slot(s)) => Some(
            json!({"kind":"slot","slot":s.slot.to_string(),"parent":s.parent.map(|v|v.to_string()),"status_code":s.status}),
        ),
        Some(UpdateOneof::BlockMeta(b)) => Some(
            json!({"kind":"block_meta","slot":b.slot.to_string(),"blockhash":b.blockhash,
            "parentSlot":b.parent_slot.to_string(),"parentBlockhash":b.parent_blockhash,"blockTime":b.block_time.map(|t|json!({"timestamp":t.timestamp.to_string()})),
            "blockHeight":b.block_height.map(|h|json!({"blockHeight":h.block_height.to_string()})),"executedTransactionCount":b.executed_transaction_count.to_string()}),
        ),
        Some(UpdateOneof::Transaction(t)) => {
            let info = t.transaction.ok_or("missing transaction info")?;
            if info.signature.len() != 64 {
                return Err("invalid transaction signature");
            }
            Some(
                json!({"kind":"transaction","slot":t.slot.to_string(),"signature":bs58::encode(info.signature).into_string(),
                "index":info.index.to_string(),"failed":info.meta.map(|m|m.err.is_some())}),
            )
        }
        _ => None,
    })
}

/// Resolves v0 lookup keys after static keys, preserving the provider's index scope.
fn passive_update(
    update: &geyser::SubscribeUpdate,
    observer: ObserverKind,
    received: &ReceiveTime,
) -> Result<Option<PassiveTransaction>, &'static str> {
    let Some(UpdateOneof::Transaction(t)) = &update.update_oneof else {
        return Ok(None);
    };
    let info = t.transaction.as_ref().ok_or("tape_missing_transaction")?;
    let message = info
        .transaction
        .as_ref()
        .and_then(|t| t.message.as_ref())
        .ok_or("tape_missing_message")?;
    let meta = info.meta.as_ref().ok_or("tape_missing_meta")?;
    let account_keys = message
        .account_keys
        .iter()
        .chain(&meta.loaded_writable_addresses)
        .chain(&meta.loaded_readonly_addresses)
        .map(|key| {
            if key.len() != 32 {
                return Err("tape_invalid_account");
            }
            Ok(bs58::encode(key).into_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if account_keys.len() > 256
        || message.instructions.len()
            + meta
                .inner_instructions
                .iter()
                .map(|g| g.instructions.len())
                .sum::<usize>()
            > 4096
    {
        return Err("tape_parser_bound");
    }
    let mut instructions = message
        .instructions
        .iter()
        .enumerate()
        .map(|(i, ix)| PassiveInstruction {
            program_id_index: ix.program_id_index,
            accounts: ix.accounts.iter().map(|i| u32::from(*i)).collect(),
            data_base64: STANDARD.encode(&ix.data),
            outer_index: i as u32,
            inner_index: None,
        })
        .collect::<Vec<_>>();
    for group in &meta.inner_instructions {
        for (i, ix) in group.instructions.iter().enumerate() {
            instructions.push(PassiveInstruction {
                program_id_index: ix.program_id_index,
                accounts: ix.accounts.iter().map(|i| u32::from(*i)).collect(),
                data_base64: STANDARD.encode(&ix.data),
                outer_index: group.index,
                inner_index: Some(i as u32),
            });
        }
    }
    Ok(Some(PassiveTransaction {
        source: Source::Live,
        observer,
        signature: bs58::encode(&info.signature).into_string(),
        slot: t.slot,
        block_id: None,
        index_in_block: Some(info.index),
        index_scope: IndexScope::ProviderReported,
        success: meta.err.is_none(),
        fee_lamports: meta.fee,
        account_keys,
        instructions,
        received: received.clone(),
    }))
}

async fn emit(
    update: geyser::SubscribeUpdate,
    observer: ObserverKind,
    clock: &ReceiveClock,
    tx: &mpsc::Sender<Frame>,
    tip_accounts: &[String],
) -> Result<(), &'static str> {
    let received = clock.receive();
    let passive_tips = passive_update(&update, observer, &received).and_then(|transaction| {
        transaction.map_or(Ok(vec![]), |t| {
            alight_tape::parse(&t, tip_accounts).map_err(|_| "tape_invalid_transaction")
        })
    });
    let Some(raw) = selected_update(update)? else {
        return Ok(());
    };
    let (event, raw) = normalize(&raw, observer, Source::Live, received)
        .map_err(|_| "observer_schema")?
        .ok_or("observer_schema")?;
    tx.send(Frame {
        observer,
        event,
        raw,
        passive_tips,
    })
    .await
    .map_err(|_| "writer_closed")
}

/// Retries use capped exponential backoff; disconnect gaps persist before the retry.
pub async fn grpc_worker(
    options: GrpcOptions,
    store: Store,
    tx: mpsc::Sender<Frame>,
    mut stop: watch::Receiver<bool>,
) -> Result<(), alight_store::StoreError> {
    let mut attempt = 0;
    let mut replay = options.replay;
    let mut force = options.force_disconnect_after;
    while !*stop.borrow() {
        store
            .open_gap(
                ObserverKind::Grpc,
                Source::Live,
                &utc_now(),
                "grpc_connecting_or_disconnected",
            )
            .await?;
        let started = Instant::now();
        let mut session_stop = stop.clone();
        let session = grpc_session(
            &options,
            &store,
            &tx,
            &mut session_stop,
            replay,
            force.take(),
        );
        let reason = tokio::select! {r=session=>r.err(),_=stop.changed()=>None};
        if *stop.borrow() {
            break;
        }
        if reason == Some("writer_closed") {
            break;
        }
        if reason == Some("replay_unavailable") {
            replay = false;
        }
        if started.elapsed() > Duration::from_secs(30) {
            attempt = 0;
        }
        attempt = (attempt + 1).min(7);
        let delay = Duration::from_millis((250u64 << attempt).min(30_000));
        tokio::select! {_=tokio::time::sleep(delay)=>{},_=stop.changed()=>{}}
    }
    Ok(())
}

async fn grpc_session(
    options: &GrpcOptions,
    store: &Store,
    tx: &mpsc::Sender<Frame>,
    stop: &mut watch::Receiver<bool>,
    replay: bool,
    force: Option<Duration>,
) -> Result<(), &'static str> {
    let mut client = connect_public(&options.token, &options.url)
        .await
        .map_err(|_| "grpc_connection")?;
    let from_slot = if replay {
        store
            .cursor(ObserverKind::Grpc, Source::Live)
            .await
            .map_err(|_| "database")?
            .map(|s| s.saturating_sub(64))
    } else {
        None
    };
    let mut request = solami::SubscriptionBuilder::new()
        .commitment(solami::CommitmentLevel::Processed)
        .slots(
            "clock",
            solami::SubscribeRequestFilterSlots {
                filter_by_commitment: Some(false),
                interslot_updates: Some(true),
            },
        )
        .blocks_meta("blocks")
        .transactions(
            "payer_and_tips",
            solami::TxFilter {
                vote: Some(false),
                failed: None,
                signature: None,
                account_include: options.accounts.clone(),
                account_exclude: vec![],
                account_required: vec![],
            },
        )
        .build();
    request.from_slot = from_slot;
    let (sink, mut updates) =
        tokio::time::timeout(Duration::from_secs(15), client.subscribe(request))
            .await
            .map_err(|_| "grpc_subscription_timeout")?
            .map_err(|_| {
                if from_slot.is_some() {
                    "replay_unavailable"
                } else {
                    "grpc_subscription"
                }
            })?;
    let clock = ReceiveClock::new();
    let deadline =
        tokio::time::Instant::now() + force.unwrap_or(Duration::from_secs(365 * 24 * 3600));
    loop {
        let update = tokio::select! {r=tokio::time::timeout(Duration::from_secs(30),updates.message())=>r.map_err(|_|"grpc_stale")?.map_err(|_|"grpc_stream")?.ok_or("grpc_closed")?,_=stop.changed()=>return Ok(()),_=tokio::time::sleep_until(deadline)=>return Err("forced_disconnect")};
        if matches!(update.update_oneof, Some(UpdateOneof::Ping(_))) {
            sink.send(geyser::SubscribeRequest {
                ping: Some(geyser::SubscribeRequestPing { id: 1 }),
                ..Default::default()
            })
            .await
            .map_err(|_| "grpc_ping")?;
        } else {
            emit(
                update,
                ObserverKind::Grpc,
                &clock,
                tx,
                &options.tip_accounts,
            )
            .await?;
        }
    }
}

pub async fn mirage_worker(
    options: MirageOptions,
    store: Store,
    tx: mpsc::Sender<Frame>,
    mut stop: watch::Receiver<bool>,
) -> Result<(), alight_store::StoreError> {
    let mut attempt = 0;
    while !*stop.borrow() {
        store
            .open_gap(
                ObserverKind::Mirage,
                Source::Live,
                &utc_now(),
                "mirage_connecting_or_disconnected",
            )
            .await?;
        let started = Instant::now();
        let mut session_stop = stop.clone();
        let session = mirage_session(&options, &tx, &mut session_stop);
        let reason = tokio::select! {r=session=>r.err(),_=stop.changed()=>None};
        if *stop.borrow() || reason == Some("writer_closed") {
            break;
        }
        if started.elapsed() > Duration::from_secs(30) {
            attempt = 0;
        }
        attempt = (attempt + 1).min(7);
        tokio::select! {_=tokio::time::sleep(Duration::from_millis((250u64<<attempt).min(30_000)))=>{},_=stop.changed()=>{}}
    }
    Ok(())
}

async fn mirage_session(
    options: &MirageOptions,
    tx: &mpsc::Sender<Frame>,
    stop: &mut watch::Receiver<bool>,
) -> Result<(), &'static str> {
    let cfg = WebSocketConfig::default()
        .max_message_size(Some(1024 * 1024))
        .max_frame_size(Some(1024 * 1024));
    let (mut ws, _) = tokio::time::timeout(
        Duration::from_secs(15),
        connect_async_with_config(&options.url, Some(cfg), false),
    )
    .await
    .map_err(|_| "mirage_timeout")?
    .map_err(|_| "mirage_connection")?;
    let clock = ReceiveClock::new();
    loop {
        let message = tokio::select! {r=tokio::time::timeout(Duration::from_secs(30),ws.next())=>r.map_err(|_|"mirage_stale")?.ok_or("mirage_closed")?.map_err(|_|"mirage_stream")?,_=stop.changed()=>return Ok(())};
        match message {
            WsMessage::Binary(bytes) => {
                let update = geyser::SubscribeUpdate::decode(bytes).map_err(|_| "mirage_schema")?;
                // The wire protocol is the same Yellowstone protobuf; retain Mirage provenance.
                emit(
                    update,
                    ObserverKind::Mirage,
                    &clock,
                    tx,
                    &options.tip_accounts,
                )
                .await?;
            }
            WsMessage::Ping(bytes) => ws
                .send(WsMessage::Pong(bytes))
                .await
                .map_err(|_| "mirage_ping")?,
            WsMessage::Close(_) => return Err("mirage_closed"),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solami::solana::storage::confirmed_block as proto;

    #[test]
    fn native_transaction_preserves_loaded_keys_full_width_index_and_execution_fee() {
        let mut transfer = 2u32.to_le_bytes().to_vec();
        transfer.extend(9897u64.to_le_bytes());
        let update = geyser::SubscribeUpdate {
            update_oneof: Some(UpdateOneof::Transaction(
                geyser::SubscribeUpdateTransaction {
                    slot: u64::MAX,
                    transaction: Some(geyser::SubscribeUpdateTransactionInfo {
                        signature: vec![7; 64],
                        index: u64::MAX,
                        transaction: Some(proto::Transaction {
                            message: Some(proto::Message {
                                account_keys: vec![vec![0; 32], vec![1; 32]],
                                instructions: vec![proto::CompiledInstruction {
                                    program_id_index: 0,
                                    accounts: vec![1, 2],
                                    data: transfer,
                                }],
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                        meta: Some(proto::TransactionStatusMeta {
                            fee: u64::MAX,
                            loaded_writable_addresses: vec![vec![2; 32]],
                            loaded_readonly_addresses: vec![vec![3; 32]],
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                },
            )),
            ..Default::default()
        };
        let received = ReceiveTime {
            clock_id: "fixture".into(),
            mono_ns: 1,
            wall_utc: "2026-10-05T00:00:00Z".into(),
        };
        let t = passive_update(&update, ObserverKind::Grpc, &received)
            .expect("normalize")
            .expect("transaction");
        assert_eq!(t.account_keys.len(), 4);
        assert_eq!(t.fee_lamports, u64::MAX);
        let tips = alight_tape::parse(&t, &[bs58::encode([2u8; 32]).into_string()]).expect("parse");
        assert_eq!(tips[0].tip_lamports, Some(9897));
        assert_eq!(tips[0].index_in_block, Some(u64::MAX));
        assert_eq!(tips[0].index_scope, IndexScope::ProviderReported);
        assert_eq!(tips[0].block_id, None);
        let selected = selected_update(update)
            .expect("existing observer subset")
            .expect("frame");
        assert_eq!(selected["index"], u64::MAX.to_string());
        assert_eq!(selected["failed"], false);
        assert!(selected.get("instructions").is_none());
    }
}
