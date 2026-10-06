use alight_ingest::{Config, stream};
use alight_notify::{Format, Webhook};
use alight_store::{Store, StoreError};
use alight_types::Source;
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;
/// Local persisted alerts always run; outbound delivery requires an explicitly configured URL.
pub async fn run(
    config: Arc<Config>,
    store: Store,
    mut stop: watch::Receiver<bool>,
) -> Result<(), StoreError> {
    let daily_cap = alight_canary::governor::sol_to_lamports(
        config.get("ALIGHT_DAILY_BUDGET_SOL").unwrap_or("0.20"),
    )
    .map_err(|_| StoreError::Invalid)?;
    let format = match config.get("ALIGHT_ALERT_FORMAT").unwrap_or("webhook") {
        "webhook" => Format::Webhook,
        "discord" => Format::Discord,
        "slack" => Format::Slack,
        _ => return Err(StoreError::Invalid),
    };
    let sender = config
        .get("ALIGHT_ALERT_WEBHOOK_URL")
        .filter(|s| !s.is_empty())
        .map(|s| Webhook::new(s, format))
        .transpose()
        .map_err(|_| StoreError::Invalid)?;
    let mut interval = tokio::time::interval(Duration::from_secs(10));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {_=stop.changed()=>return Ok(()),_=interval.tick()=>{}}
        match alight_notify::poll(&store, Source::Live, "local", &stream::utc_now(), daily_cap)
            .await
        {
            Ok(_) => {
                for alert in store.pending_alerts(Source::Live).await? {
                    if !store.claim_alert_delivery(&alert.id).await? {
                        continue;
                    }
                    let status = if let Some(sender) = &sender {
                        if sender.send(&alert).await.is_ok() {
                            "DELIVERED"
                        } else {
                            "FAILED"
                        }
                    } else {
                        "NO_ENDPOINT"
                    };
                    store.alert_delivery(&alert.id, status).await?;
                    println!(
                        "{}",
                        serde_json::json!({"kind":"ALERT","event":alert,"delivery_status":status})
                    );
                }
            }
            Err(_) => eprintln!(
                "Alert evaluation failed; next scheduled evaluation will use persisted state"
            ),
        }
    }
}
