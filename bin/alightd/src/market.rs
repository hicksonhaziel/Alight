//! Observe/live Blur worker. No signing configuration or transaction sender is constructed.
use alight_econ::{
    EconError,
    blur::BlurClient,
    series::{PriceSeries, SeriesSettings},
};
use alight_ingest::{Config, clock::SlotClock, stream};
use alight_store::{Store, StoreError};
use alight_types::{BlurEvent, BlurPool, CurveContext, ObserverKind, Source};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::sync::{RwLock, mpsc, watch};

fn series(
    pools: &[BlurPool],
    context: &CurveContext,
    start: u64,
) -> Result<BTreeMap<String, PriceSeries>, StoreError> {
    pools
        .iter()
        .map(|pool| {
            Ok((
                pool.pool.clone(),
                PriceSeries::new(pool, context, start, SeriesSettings::default())
                    .map_err(|_| StoreError::Invalid)?,
            ))
        })
        .collect()
}

async fn publish(
    store: &Store,
    clock: &RwLock<SlotClock>,
    rows: &BTreeMap<String, PriceSeries>,
) -> Result<(), StoreError> {
    let now = stream::utc_now();
    // A restored historical clock is insufficient for current USD estimates.
    let health = store
        .observer_health(ObserverKind::Grpc, Source::Live)
        .await?;
    let fresh = health["last_receive_utc"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .is_some_and(|at| {
            (chrono::Utc::now() - at.with_timezone(&chrono::Utc)).num_milliseconds() >= 0
                && (chrono::Utc::now() - at.with_timezone(&chrono::Utc)).num_milliseconds()
                    <= 10_000
        });
    if !fresh {
        return Ok(());
    }
    let Some(ms) = clock.read().await.mean_slot_ms() else {
        return Ok(());
    };
    for row in rows.values() {
        let snapshot = row
            .snapshot(&[0, 1, 2, 3, 4, 5, 6, 7, 8], ms, &now)
            .map_err(|_| StoreError::Invalid)?;
        store.save_market_snapshot(&snapshot).await?;
    }
    Ok(())
}

/// Discover at most three liquid pools, keep one filtered socket and bounded 300-second series.
/// Access failures retry every 300 seconds; missing keys disable only this optional worker.
pub async fn run(
    config: Arc<Config>,
    store: Store,
    clock: Arc<RwLock<SlotClock>>,
    mut stop: watch::Receiver<bool>,
) -> Result<(), StoreError> {
    let enabled = match config.get("ALIGHT_BLUR_ENABLED") {
        None | Some("true") => true,
        Some("false") => false,
        _ => return Err(StoreError::Invalid),
    };
    let token = config
        .get("SOLAMI_BLUR_TOKEN")
        .or(config.get("SOLAMI_DATA_API_TOKEN"))
        .or(config.get("SOLAMI_API_KEY"));
    let Some(token) = token.filter(|_| enabled) else {
        let _ = stop.changed().await;
        return Ok(());
    };
    let rest = config
        .get("SOLAMI_BLUR_URL")
        .unwrap_or("https://api.solami.dev");
    let ws = config
        .get("SOLAMI_BLUR_WS_URL")
        .unwrap_or("wss://ws.solami.dev/data/subscribe");
    let discovery = BlurClient::discovery(rest, ws, token).map_err(|_| StoreError::Invalid)?;
    loop {
        let selected =
            tokio::select! { _=stop.changed()=>return Ok(()), result=discovery.pools()=>result };
        let pools = match selected {
            Ok(pools) => pools
                .into_iter()
                .filter(|p| p.tvl_usd.parse::<f64>().is_ok_and(|n| n >= 100_000.0))
                .take(3)
                .collect::<Vec<_>>(),
            Err(error) => {
                let seconds = if matches!(error, EconError::Http(401..=403)) {
                    300
                } else {
                    60
                };
                // Typed errors contain neither URLs nor credentials.
                eprintln!("Blur unavailable: {error}; retry in {seconds} s");
                tokio::select! { _=stop.changed()=>return Ok(()), _=tokio::time::sleep(Duration::from_secs(seconds))=>continue }
            }
        };
        if pools.is_empty() {
            eprintln!("Blur pool discovery has no liquid candidates; retry in 60 s");
            tokio::select! { _=stop.changed()=>return Ok(()), _=tokio::time::sleep(Duration::from_secs(60))=>continue }
        }
        let now = stream::utc_now();
        let mut context =
            alight_forecast::current_context(&store, Source::Live, "local", &now).await?;
        let start = store
            .active_regime(Source::Live, &now)
            .await?
            .and_then(|r| r.start_slot)
            .unwrap_or(0);
        let mut rows = series(&pools, &context, start)?;
        // Initial REST history is small and never bridges a subsequent socket gap.
        for pool in &pools {
            let trades = tokio::select! { _=stop.changed()=>return Ok(()), result=discovery.recent_trades(pool)=>result };
            if let (Ok(trades), Some(row)) = (trades, rows.get_mut(&pool.pool)) {
                for trade in trades {
                    let _ = row.ingest(trade, &stream::utc_now());
                }
            }
        }
        let ids = pools.iter().map(|p| p.pool.clone()).collect::<Vec<_>>();
        let dex = pools
            .iter()
            .map(|p| p.dex.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let client =
            BlurClient::new(rest, ws, token, &dex, &ids).map_err(|_| StoreError::Invalid)?;
        let (tx, mut rx) = mpsc::channel(128);
        let subscription = client.run(tx, stop.clone());
        tokio::pin!(subscription);
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        println!(
            "{}",
            serde_json::json!({"kind":"BLUR","status":"SUBSCRIBING","source":"live","pools":ids.len(),"signing_enabled":false})
        );
        loop {
            tokio::select! {
                _=stop.changed()=>{
                    for row in rows.values_mut() { row.disconnect(); }
                    publish(&store, &clock, &rows).await?;
                    return Ok(());
                },
                _=&mut subscription=>return if *stop.borrow() {Ok(())} else {Err(StoreError::Invalid)},
                _=interval.tick()=>{
                    let now = stream::utc_now();
                    let next = alight_forecast::current_context(&store, Source::Live, "local", &now).await?;
                    if next.regime_id != context.regime_id {
                        let start = store.active_regime(Source::Live, &now).await?.and_then(|r| r.start_slot).unwrap_or(0);
                        rows = series(&pools, &next, start)?;
                        context = next;
                    }
                    publish(&store, &clock, &rows).await?;
                },
                event=rx.recv()=>match event {
                    Some(BlurEvent::Trade { trade }) => if let Some(row) = rows.get_mut(&trade.pool)
                        && row.ingest(trade, &stream::utc_now()).is_err() {
                        row.disconnect();
                        publish(&store, &clock, &rows).await?;
                    },
                    Some(BlurEvent::Disconnected { reason, .. }) => {
                        for row in rows.values_mut() { row.disconnect(); }
                        publish(&store, &clock, &rows).await?;
                        eprintln!("Blur gap: {reason}");
                    },
                    Some(BlurEvent::Connected { .. }) => {},
                    None => return if *stop.borrow() {Ok(())} else {Err(StoreError::Invalid)},
                }
            }
        }
    }
}
