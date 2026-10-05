//! Offline historical Blur replay. Slot duration is an explicit caller-supplied conversion.
use alight_econ::{
    EconError, blur,
    series::{PriceSeries, SeriesSettings},
};
use alight_types::{BlurEvent, CurveContext, Source};

fn main() -> Result<(), EconError> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("Usage: cargo run -p alight-econ --example replay -- SLOT_MS AS_OF_UTC");
        return Err(EconError::Invalid);
    }
    let slot_ms = args[0].parse().map_err(|_| EconError::Invalid)?;
    let context = CurveContext {
        source: Source::Replay,
        regime_id: "historical-blur-replay".into(),
        region: "recorded-capture".into(),
        as_of_utc: args[1].clone(),
    };
    let (pools, trades, candles) =
        blur::replay_rest(include_bytes!("../../../data/fixtures/blur_sample.json"))?;
    let mut snapshots = Vec::new();
    for pool in &pools {
        let mut series = PriceSeries::new(pool, &context, 0, SeriesSettings::default())?;
        for trade in trades.iter().filter(|t| t.pool == pool.pool) {
            series.ingest(trade.clone(), &context.as_of_utc)?;
        }
        snapshots.push(series.snapshot(&[1, 2, 4], slot_ms, &context.as_of_utc)?);
    }
    let events = include_str!("../../../data/fixtures/blur_ws_sample.jsonl")
        .lines()
        .map(|line| blur::replay_frame(line.as_bytes()))
        .collect::<Result<Vec<_>, _>>()?;
    let output = serde_json::json!({
        "source": "replay", "historical_capture": true, "network_requests": 0, "transactions_sent": 0,
        "rest_pools": pools.len(), "rest_trades": trades.len(), "minute_candles": candles.len(),
        "ws_control_frames": events.iter().filter(|e| matches!(e, BlurEvent::Connected { .. })).count(),
        "ws_swap_frames": events.iter().filter(|e| matches!(e, BlurEvent::Trade { .. })).count(),
        "snapshots": snapshots,
        "ws_trades": events.iter().filter_map(|e| match e { BlurEvent::Trade { trade }=>Some(trade), _=>None }).collect::<Vec<_>>(),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output).map_err(|_| EconError::Invalid)?
    );
    Ok(())
}
