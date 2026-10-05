//! Run against alightd --mode sim; only the daemon can sign/send live canaries.
use alight_client::*;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("Usage: quote_then_send ENDPOINT OPERATOR_KEY_FILE (sim only)".into());
    }
    if std::fs::metadata(&args[1])?.len() > 258 {
        return Err("Invalid operator key file".into());
    }
    let key = std::fs::read_to_string(&args[1])?;
    let client = Client::new(&args[0], Source::Sim, Some(key.trim()))?;
    let health = client.health().await?;
    let curves = client.curves(243).await?;
    let candidates = curves
        .curves
        .iter()
        .filter(|c| {
            c.horizon_slots == 4
                && c.config.route == Route::BeamHttp
                && c.config.size_class == SizeClass::Small
        })
        .map(|c| c.config.clone())
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err("No supported Beam HTTP small canary candidates".into());
    }
    let request = QuoteServiceRequest {
        model: ModelQuoteRequest {
            context: CurveContext {
                source: Source::Sim,
                regime_id: curves.regime_id,
                region: health.region,
                as_of_utc: health.as_of_utc,
            },
            candidates,
            covariates: ModelCovariates {
                congestion: Some(0.0),
            },
            leader_class_next: vec![],
            target: PredictionTarget::Probability {
                target_p: 0.5,
                horizon_slots: 4,
            },
        },
        ttl_s: 120,
        economics: None,
        frozen_model_hash: None,
    };
    let preview = client.quote(&request).await?;
    if preview.quote.recommendation.is_none() {
        return Err("Insufficient canary evidence".into());
    }
    let forecast = client.freeze_quote(&request).await?;
    // Persist this identifier before requesting a send. Reconcile this same hash/id on a lost response.
    println!(
        "{}",
        serde_json::json!({"source":"sim","forecast_hash":forecast.hash,"request_id":"rust-example"})
    );
    let report = client
        .prove(&ProveRequest {
            request_id: "rust-example".into(),
            forecast_hash: forecast.hash,
            n: 40,
            seed: Some(42),
        })
        .await?;
    println!(
        "{}",
        serde_json::json!({"source":"sim","report":report,"ledger":client.verify_ledger().await?,"transactions_sent":0})
    );
    Ok(())
}
