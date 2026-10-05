//! Send transports are private: callers can only send an engine-prepared, budgeted canary.
use alight_ingest::Config;
use alight_types::{Route, RouteRejection};
use base64::Engine;
use reqwest::{Client, Url};
use serde_json::{Value, json};
use solami::{Off, On, Solami, SwqosClient, VersionedTransaction};
use std::time::Duration;

pub(crate) enum SendResult {
    Accepted,
    Rejected(RouteRejection),
    Unknown(&'static str),
}
type Beam = Solami<Off, Off, On<SwqosClient>>;
pub(crate) struct Routes {
    http: Client,
    beam: Option<Beam>,
}
impl Routes {
    pub(crate) fn new() -> Result<Self, &'static str> {
        Ok(Self {
            http: Client::builder()
                .timeout(Duration::from_secs(12))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| "configuration")?,
            beam: None,
        })
    }
    pub(crate) async fn send(
        &mut self,
        config: &Config,
        route: Route,
        tx: &VersionedTransaction,
    ) -> SendResult {
        let Some(signature) = tx.signatures.first() else {
            return SendResult::Rejected(RouteRejection::InvalidTransaction);
        };
        if route == Route::BeamQuic {
            if self.beam.is_none() {
                let Some(key) = config.get("SOLAMI_SWQOS_KEY") else {
                    return SendResult::Rejected(RouteRejection::Authentication);
                };
                // SDK's base58 constructor panics on malformed keys; validate before invoking it.
                if solami::Keypair::try_from_base58_string(key).is_err() {
                    return SendResult::Rejected(RouteRejection::Authentication);
                }
                let endpoint = config
                    .get("SOLAMI_BEAM_QUIC")
                    .unwrap_or("beam.solami.dev:11000");
                if ![
                    "beam.solami.dev:11000",
                    "ams.beam.solami.dev:11000",
                    "fra.beam.solami.dev:11000",
                    "nyc.beam.solami.dev:11000",
                ]
                .contains(&endpoint)
                {
                    return SendResult::Rejected(RouteRejection::LocalPolicy);
                }
                match tokio::time::timeout(
                    Duration::from_secs(15),
                    solami::builder()
                        .with_beam(key)
                        // The live tip list has 15 entries; this SDK embeds only 10.
                        // The engine already validated the recipient against the fetched list.
                        .skip_precheck()
                        .beam_endpoint(endpoint)
                        .build(),
                )
                .await
                {
                    Ok(Ok(client)) => self.beam = Some(client),
                    _ => return SendResult::Unknown("QUIC_CONNECT"),
                }
            }
            let Some(beam) = &self.beam else {
                return SendResult::Unknown("QUIC_CONNECT");
            };
            let result = tokio::time::timeout(Duration::from_secs(12), beam.beam(tx)).await;
            return match result {
                Ok(Ok(ack)) if ack == *signature => SendResult::Accepted,
                _ => {
                    self.beam = None;
                    SendResult::Unknown("QUIC_SEND")
                }
            };
        }
        // Provider clarification: HTTP Beam uses the normal RPC endpoint with a tip.
        // The governed builder includes a Beam tip for BeamHttp and none for Rpc.
        let (url, token) = (config.get("SOLAMI_RPC_URL"), config.get("SOLAMI_RPC_TOKEN"));
        let Some(url) = url.and_then(|s| Url::parse(s).ok()).filter(|u| {
            u.scheme() == "https"
                && u.username().is_empty()
                && u.password().is_none()
                && u.host_str()
                    .is_some_and(|h| h == "solami.dev" || h.ends_with(".solami.dev"))
        }) else {
            return SendResult::Rejected(RouteRejection::LocalPolicy);
        };
        let Ok(wire) = bincode::serialize(tx) else {
            return SendResult::Rejected(RouteRejection::InvalidTransaction);
        };
        let body = json!({"jsonrpc":"2.0","id":1,"method":"sendTransaction","params":[base64::engine::general_purpose::STANDARD.encode(wire),{"encoding":"base64","skipPreflight":false,"preflightCommitment":"confirmed","maxRetries":0}]});
        let mut request = self.http.post(url).json(&body);
        if let Some(token) = token {
            request = request.header("x-api-key", token);
        }
        let Ok(mut response) = request.send().await else {
            return SendResult::Unknown("HTTP_NETWORK");
        };
        match response.status().as_u16() {
            401 | 403 => return SendResult::Rejected(RouteRejection::Authentication),
            429 => return SendResult::Rejected(RouteRejection::RateLimited),
            status if !(200..300).contains(&status) => return SendResult::Unknown("HTTP_STATUS"),
            _ => {}
        }
        let mut bytes = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) if bytes.len() + chunk.len() <= 64 * 1024 => bytes.extend(chunk),
                Ok(None) => break,
                _ => return SendResult::Unknown("HTTP_BODY"),
            }
        }
        let Ok(body) = serde_json::from_slice::<Value>(&bytes) else {
            return SendResult::Unknown("HTTP_FORMAT");
        };
        classify(&body, &signature.to_string())
    }
}
fn classify(body: &Value, expected: &str) -> SendResult {
    if body["result"].as_str() == Some(expected) {
        return SendResult::Accepted;
    }
    let error = &body["error"];
    // Map only explicit refusals. Unrecognized responses retain uncertainty and their reservation.
    match error["code"].as_i64() {
        Some(-32600 | -32602) => SendResult::Rejected(RouteRejection::InvalidTransaction),
        Some(-32601) => SendResult::Rejected(RouteRejection::LocalPolicy),
        Some(-32002)
            if error["data"]["err"].is_string()
                && error["data"]["err"] == "InsufficientFundsForFee" =>
        {
            SendResult::Rejected(RouteRejection::InsufficientFunds)
        }
        Some(-32002) => SendResult::Rejected(RouteRejection::InvalidTransaction),
        _ => SendResult::Unknown("RPC_RESPONSE"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrecognized_send_ack_and_server_failure_are_not_rejections() {
        assert!(matches!(
            classify(&json!({"result":"different"}), "expected"),
            SendResult::Unknown(_)
        ));
        assert!(matches!(
            classify(&json!({"error":{"code":-32005}}), "expected"),
            SendResult::Unknown(_)
        ));
        assert!(matches!(
            classify(&json!({"error":{"code":-32602}}), "expected"),
            SendResult::Rejected(RouteRejection::InvalidTransaction)
        ));
    }
}
