//! Recorded REST arrays and WS swap/control frames; provider errors never expose URLs.
use crate::{EconError, positive_decimal};
use alight_types::{BlurEvent, BlurPool, MarketCandle, MarketTrade, Source};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, de::DeserializeOwned};
use std::{collections::BTreeSet, time::Duration};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
};

const BYTE_CAP: usize = 512 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Deserialize)]
struct ProviderTrade {
    pool: String,
    mint: Option<String>,
    signature: String,
    slot: u64,
    block_time: i64,
    tx_index: u32,
    ix_index: u32,
    inner_ix_index: Option<i32>,
    price_usd: String,
    candle_ok: Option<bool>,
    base_amount: u64,
    quote_amount: u64,
    base_reserve: u64,
    quote_reserve: u64,
    fee_amount: u64,
}
impl ProviderTrade {
    fn normalize(self, source: Source, pool: Option<&BlurPool>) -> Result<MarketTrade, EconError> {
        let mint = match (self.mint, pool) {
            (Some(m), Some(p)) if m != p.mint => return Err(EconError::Invalid),
            (Some(m), _) => m,
            (None, Some(p)) => p.mint.clone(),
            _ => return Err(EconError::Invalid),
        };
        if self.pool.is_empty()
            || mint.is_empty()
            || self.signature.is_empty()
            || self.pool.len() > 128
            || mint.len() > 128
            || self.signature.len() > 128
            || self.block_time < 0
            || pool.is_some_and(|p| p.pool != self.pool)
        {
            return Err(EconError::Invalid);
        }
        positive_decimal(&self.price_usd)?;
        Ok(MarketTrade {
            source,
            pool: self.pool,
            mint,
            signature: self.signature,
            slot: self.slot,
            block_time_unix_s: self.block_time,
            tx_index: self.tx_index,
            ix_index: self.ix_index,
            inner_ix_index: self.inner_ix_index,
            price_usd: self.price_usd,
            candle_ok: self.candle_ok.unwrap_or(true),
            base_amount: self.base_amount,
            quote_amount: self.quote_amount,
            base_reserve: self.base_reserve,
            quote_reserve: self.quote_reserve,
            fee_amount: self.fee_amount,
        })
    }
}
#[derive(Deserialize)]
struct ProviderCandle {
    time: i64,
    open: String,
    high: String,
    low: String,
    close: String,
    volume: String,
    trades: u32,
}
fn decode<T: DeserializeOwned>(raw: &[u8]) -> Result<T, EconError> {
    if raw.len() > BYTE_CAP {
        return Err(EconError::TooLarge);
    }
    serde_json::from_slice(raw).map_err(|_| EconError::Invalid)
}

/// Normalize exact JSON integers and USD-price text; source is chosen by the caller.
pub fn trades(raw: &[u8], pool: &BlurPool, source: Source) -> Result<Vec<MarketTrade>, EconError> {
    let rows: Vec<ProviderTrade> = decode(raw)?;
    rows.into_iter()
        .map(|t| t.normalize(source, Some(pool)))
        .collect()
}
/// Minute candles are not used as observations for slot-scale price movement.
pub fn candles(
    raw: &[u8],
    pool: &BlurPool,
    source: Source,
) -> Result<Vec<MarketCandle>, EconError> {
    let rows: Vec<ProviderCandle> = decode(raw)?;
    rows.into_iter()
        .map(|c| {
            for price in [&c.open, &c.high, &c.low, &c.close] {
                positive_decimal(price)?;
            }
            let volume = c.volume.parse::<f64>().map_err(|_| EconError::Invalid)?;
            if c.time < 0 || !volume.is_finite() || volume < 0.0 {
                return Err(EconError::Invalid);
            }
            Ok(MarketCandle {
                source,
                pool: pool.pool.clone(),
                mint: pool.mint.clone(),
                time_unix_s: c.time,
                open: c.open,
                high: c.high,
                low: c.low,
                close: c.close,
                volume: c.volume,
                trades: c.trades,
            })
        })
        .collect()
}

/// Parse a real WS frame. Connection messages are control evidence, never price samples.
pub fn frame(raw: &[u8], source: Source) -> Result<BlurEvent, EconError> {
    #[derive(Deserialize)]
    struct Kind {
        r#type: String,
        region: Option<String>,
    }
    let kind: Kind = decode(raw)?;
    match kind.r#type.as_str() {
        "connected" => Ok(BlurEvent::Connected {
            source,
            region: kind.region,
        }),
        "swap" => Ok(BlurEvent::Trade {
            trade: decode::<ProviderTrade>(raw)?.normalize(source, None)?,
        }),
        _ => Err(EconError::Invalid),
    }
}

/// Replay uses the authoritative raw_json, ignoring the potentially rounded JS data preview.
pub fn replay_frame(envelope: &[u8]) -> Result<BlurEvent, EconError> {
    #[derive(Deserialize)]
    struct Envelope {
        raw_json: String,
    }
    let e: Envelope = decode(envelope)?;
    frame(e.raw_json.as_bytes(), Source::Replay)
}

/// Historical REST fixture output is always replay evidence, including separate candles.
pub type ReplayMarket = (Vec<BlurPool>, Vec<MarketTrade>, Vec<MarketCandle>);

/// Normalize a historical REST capture, never its original live source label.
pub fn replay_rest(raw: &[u8]) -> Result<ReplayMarket, EconError> {
    #[derive(Deserialize)]
    struct PoolCapture {
        pool_metadata: BlurPool,
        trades: serde_json::Value,
        candles: serde_json::Value,
    }
    #[derive(Deserialize)]
    struct Capture {
        schema_version: u32,
        pools: Vec<PoolCapture>,
    }
    let capture: Capture = decode(raw)?;
    if capture.schema_version != 1 || capture.pools.len() > 100 {
        return Err(EconError::Invalid);
    }
    let (mut pools, mut all_trades, mut all_candles) = (Vec::new(), Vec::new(), Vec::new());
    for p in capture.pools {
        validate_pool(&p.pool_metadata)?;
        all_trades.extend(trades(
            &serde_json::to_vec(&p.trades).map_err(|_| EconError::Invalid)?,
            &p.pool_metadata,
            Source::Replay,
        )?);
        all_candles.extend(candles(
            &serde_json::to_vec(&p.candles).map_err(|_| EconError::Invalid)?,
            &p.pool_metadata,
            Source::Replay,
        )?);
        pools.push(p.pool_metadata);
    }
    Ok((pools, all_trades, all_candles))
}
fn validate_pool(p: &BlurPool) -> Result<(), EconError> {
    if [&p.pool, &p.mint, &p.quote_mint, &p.dex]
        .iter()
        .any(|s| s.is_empty() || s.len() > 128)
    {
        return Err(EconError::Invalid);
    }
    positive_decimal(&p.price_usd)?;
    positive_decimal(&p.tvl_usd)?;
    Ok(())
}

/// Explicit construction does not connect. No caller in sim/replay needs this client.
pub struct BlurClient {
    rest: reqwest::Url,
    ws: reqwest::Url,
    token: reqwest::header::HeaderValue,
    client: reqwest::Client,
    tracked_pools: BTreeSet<String>,
}
impl BlurClient {
    /// HTTPS/WSS endpoints and a nonempty pool filter are required; credentials stay private.
    pub fn new(
        rest: &str,
        ws: &str,
        token: &str,
        dex: &str,
        tracked_pools: &[String],
    ) -> Result<Self, EconError> {
        Self::build(rest, ws, token, dex, tracked_pools, false)
    }
    fn build(
        rest: &str,
        ws: &str,
        token: &str,
        dex: &str,
        tracked_pools: &[String],
        local_test: bool,
    ) -> Result<Self, EconError> {
        let mut rest = reqwest::Url::parse(rest).map_err(|_| EconError::Configuration)?;
        let mut ws = reqwest::Url::parse(ws).map_err(|_| EconError::Configuration)?;
        for (url, scheme) in [(&rest, "https"), (&ws, "wss")] {
            if (url.scheme() != scheme && !(local_test && url.host_str() == Some("127.0.0.1")))
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
            {
                return Err(EconError::Configuration);
            }
        }
        if token.trim().is_empty()
            || dex.is_empty()
            || tracked_pools.is_empty()
            || tracked_pools.len() > 100
            || tracked_pools.iter().any(|p| p.is_empty() || p.len() > 128)
        {
            return Err(EconError::Configuration);
        }
        let mut header =
            reqwest::header::HeaderValue::from_str(token).map_err(|_| EconError::Configuration)?;
        header.set_sensitive(true);
        rest.set_query(None);
        rest.set_path(&format!("{}/", rest.path().trim_end_matches('/')));
        ws.set_query(None);
        // Only captured filter parameters; tracked pool filtering is local until a pool filter is validated.
        ws.query_pairs_mut()
            .append_pair("api_key", token)
            .append_pair("chain", "solana")
            .append_pair("type", "swap")
            .append_pair("dex", dex)
            .append_pair("metadata", "false");
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| EconError::Transport)?;
        Ok(Self {
            rest,
            ws,
            token: header,
            client,
            tracked_pools: tracked_pools.iter().cloned().collect(),
        })
    }
    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Vec<u8>, EconError> {
        let url = self.rest.join(path).map_err(|_| EconError::Configuration)?;
        let mut response = self
            .client
            .get(url)
            .query(query)
            .header("x-api-key", self.token.clone())
            .send()
            .await
            .map_err(|_| EconError::Transport)?;
        if !response.status().is_success() {
            return Err(EconError::Http(response.status().as_u16()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| EconError::Transport)? {
            if bytes.len() + chunk.len() > BYTE_CAP {
                return Err(EconError::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
    /// Recorded selection request: twelve pools by daily USD volume, using the three captured DEXes.
    pub async fn pools(&self) -> Result<Vec<BlurPool>, EconError> {
        let raw = self
            .get(
                "data/pools",
                &[
                    ("chain", "solana".into()),
                    ("limit", "12".into()),
                    ("sort", "volume_usd".into()),
                    ("order", "desc".into()),
                    ("dex", "raydium_clmm,orca_whirlpool,pumpswap".into()),
                ],
            )
            .await?;
        let pools: Vec<BlurPool> = decode(&raw)?;
        for pool in &pools {
            validate_pool(pool)?;
        }
        Ok(pools)
    }
    /// Fetch the recorded three most recent trades. Small REST snapshots may be sparse.
    pub async fn recent_trades(&self, pool: &BlurPool) -> Result<Vec<MarketTrade>, EconError> {
        validate_pool(pool)?;
        let raw = self
            .get(
                "data/token/trades",
                &[
                    ("chain", "solana".into()),
                    ("address", pool.mint.clone()),
                    ("pool", pool.pool.clone()),
                    ("limit", "3".into()),
                ],
            )
            .await?;
        trades(&raw, pool, Source::Live)
    }
    /// Captured request has interval=1m,count=3; responses can contain an extra current candle.
    pub async fn recent_candles(&self, pool: &BlurPool) -> Result<Vec<MarketCandle>, EconError> {
        validate_pool(pool)?;
        let raw = self
            .get(
                "data/token/ohlcv",
                &[
                    ("chain", "solana".into()),
                    ("address", pool.mint.clone()),
                    ("pool", pool.pool.clone()),
                    ("interval", "1m".into()),
                    ("count", "3".into()),
                ],
            )
            .await?;
        candles(&raw, pool, Source::Live)
    }
    /// Retry closed/stale sockets with capped backoff. Stop/consumer closure ends all retries.
    /// No history replay is claimed; each disconnect splits downstream market coverage.
    pub async fn run(
        &self,
        tx: mpsc::Sender<BlurEvent>,
        mut stop: watch::Receiver<bool>,
    ) -> Result<(), EconError> {
        let mut attempt = 0u32;
        while !*stop.borrow() {
            let started = std::time::Instant::now();
            let session = self.session(&tx);
            let reason = tokio::select! {
                r=session => r,
                _=stop.changed()=>return Ok(()),
                _=tx.closed()=>return Ok(()),
            };
            if matches!(reason, Err(EconError::Closed)) {
                return Ok(());
            }
            let reason = match reason {
                Err(EconError::Invalid) => "blur_schema",
                Err(EconError::TooLarge) => "blur_frame_limit",
                _ => "blur_disconnected_or_stale",
            };
            tokio::select! {
                r=tx.send(BlurEvent::Disconnected { source: Source::Live, reason: reason.into() }) => r.map_err(|_| EconError::Closed)?,
                _=stop.changed()=>return Ok(()),
            }
            if started.elapsed() >= Duration::from_secs(30) {
                attempt = 0;
            }
            let delay = Duration::from_millis((250u64 << attempt.min(7)).min(30_000));
            attempt = attempt.saturating_add(1);
            tokio::select! { _=tokio::time::sleep(delay)=>{}, _=stop.changed()=>return Ok(()), _=tx.closed()=>return Ok(()) }
        }
        Ok(())
    }
    async fn session(&self, tx: &mpsc::Sender<BlurEvent>) -> Result<(), EconError> {
        let config = WebSocketConfig::default()
            .max_message_size(Some(BYTE_CAP))
            .max_frame_size(Some(BYTE_CAP));
        let (mut socket, _) = tokio::time::timeout(
            REQUEST_TIMEOUT,
            connect_async_with_config(self.ws.as_str(), Some(config), false),
        )
        .await
        .map_err(|_| EconError::Transport)?
        .map_err(|_| EconError::Transport)?;
        let mut last_data = tokio::time::Instant::now();
        loop {
            let message =
                tokio::time::timeout_at(last_data + Duration::from_secs(30), socket.next())
                    .await
                    .map_err(|_| EconError::Transport)?
                    .ok_or(EconError::Transport)?
                    .map_err(|_| EconError::Transport)?;
            let event = match message {
                Message::Text(text) => Some(frame(text.as_bytes(), Source::Live)?),
                Message::Binary(bytes) => Some(frame(&bytes, Source::Live)?),
                Message::Ping(bytes) => {
                    socket
                        .send(Message::Pong(bytes))
                        .await
                        .map_err(|_| EconError::Transport)?;
                    None
                }
                Message::Close(_) => return Err(EconError::Transport),
                _ => None,
            };
            if let Some(event) = event {
                if let BlurEvent::Trade { trade } = &event {
                    if !self.tracked_pools.contains(&trade.pool) {
                        continue;
                    }
                    last_data = tokio::time::Instant::now();
                }
                tx.send(event).await.map_err(|_| EconError::Closed)?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, extract::Query, http::HeaderMap, routing::get};
    use std::collections::BTreeMap;

    async fn recorded_response(
        headers: HeaderMap,
        Query(query): Query<BTreeMap<String, String>>,
        path: &'static str,
    ) -> Json<serde_json::Value> {
        assert_eq!(headers.get("x-api-key").expect("API key"), "local-test");
        assert_eq!(query.get("chain").expect("chain"), "solana");
        let fixture: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../data/fixtures/blur_sample.json"))
                .expect("fixture");
        let first = &fixture["pools"][0];
        let result = if path == "pools" {
            assert_eq!(query.get("limit").expect("limit"), "12");
            assert_eq!(query.get("sort").expect("sort"), "volume_usd");
            assert_eq!(query.get("order").expect("order"), "desc");
            assert_eq!(
                query.get("dex").expect("dex"),
                "raydium_clmm,orca_whirlpool,pumpswap"
            );
            serde_json::Value::Array(
                fixture["pools"]
                    .as_array()
                    .expect("pools")
                    .iter()
                    .map(|p| p["pool_metadata"].clone())
                    .collect(),
            )
        } else {
            assert_eq!(
                query.get("pool").expect("pool"),
                first["pool_metadata"]["pool"].as_str().expect("pool")
            );
            assert_eq!(
                query.get("address").expect("mint"),
                first["pool_metadata"]["mint"].as_str().expect("mint")
            );
            if path == "trades" {
                assert_eq!(query.get("limit").expect("limit"), "3");
            } else {
                assert_eq!(query.get("interval").expect("interval"), "1m");
                assert_eq!(query.get("count").expect("count"), "3");
            }
            first[path].clone()
        };
        Json(result)
    }

    #[tokio::test]
    async fn rest_uses_captured_paths_queries_auth_and_response_shapes_on_loopback() {
        let app = Router::new()
            .route(
                "/data/pools",
                get(|headers, query| recorded_response(headers, query, "pools")),
            )
            .route(
                "/data/token/trades",
                get(|headers, query| recorded_response(headers, query, "trades")),
            )
            .route(
                "/data/token/ohlcv",
                get(|headers, query| recorded_response(headers, query, "candles")),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listen");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        let client = BlurClient::build(
            &format!("http://{address}"),
            &format!("ws://{address}"),
            "local-test",
            "raydium_clmm",
            &["test-pool".into()],
            true,
        )
        .expect("client");
        let pools = client.pools().await.expect("pools");
        assert_eq!(pools.len(), 3);
        let trades = client.recent_trades(&pools[0]).await.expect("trades");
        assert_eq!(trades.len(), 3);
        assert!(trades.iter().all(|t| t.source == Source::Live));
        assert_eq!(
            client
                .recent_candles(&pools[0])
                .await
                .expect("candles")
                .len(),
            4
        );
        // These labels describe the client's live path over a local fixture server, not provider evidence.
        server.abort();
        assert!(
            BlurClient::new(
                "http://127.0.0.1:1",
                "ws://127.0.0.1:1",
                "local-test",
                "raydium_clmm",
                &["pool".into()]
            )
            .is_err()
        );
        let error = BlurClient::new(
            "https://example.test",
            "wss://example.test",
            "bad\ncredential",
            "raydium_clmm",
            &["pool".into()],
        )
        .err()
        .expect("bad key");
        assert!(!error.to_string().contains("credential\n"));
    }
    #[tokio::test]
    async fn closed_socket_records_gap_reconnects_and_stops_without_external_network() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listen");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (stream, _) = listener.accept().await.expect("accept");
                let mut ws = tokio_tungstenite::accept_async(stream)
                    .await
                    .expect("handshake");
                ws.send(Message::Text(
                    r#"{"type":"connected","region":"test"}"#.into(),
                ))
                .await
                .expect("send");
                ws.close(None).await.expect("close");
            }
        });
        let client = BlurClient::build(
            &format!("http://{address}"),
            &format!("ws://{address}"),
            "local-test",
            "raydium_clmm",
            &["test-pool".into()],
            true,
        )
        .expect("client");
        let (tx, mut rx) = mpsc::channel(4);
        let (stop_tx, stop_rx) = watch::channel(false);
        let worker = tokio::spawn(async move { client.run(tx, stop_rx).await });
        let mut connected = 0;
        let mut gaps = 0;
        while connected < 2 {
            match tokio::time::timeout(Duration::from_secs(3), rx.recv())
                .await
                .expect("deadline")
                .expect("event")
            {
                BlurEvent::Connected { .. } => connected += 1,
                BlurEvent::Disconnected { .. } => gaps += 1,
                _ => panic!("unexpected trade"),
            }
        }
        assert!(gaps >= 1);
        stop_tx.send(true).expect("stop");
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .expect("stop deadline")
            .expect("join")
            .expect("worker");
        server.await.expect("server");
    }
}
