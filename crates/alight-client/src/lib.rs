//! Source-scoped HTTP client. Money uses lamports or micro-lamports per CU in shared types.
//! No automatic retries: a lost response after a write must be reconciled by its saved id.
pub use alight_types::*;
use reqwest::{Method, Url, header};
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;

const MAX_RESPONSE: usize = 8 * 1024 * 1024;
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("Invalid API endpoint, source or operator configuration")]
    Configuration,
    #[error("API request failed or timed out; reconcile writes before retrying")]
    Transport,
    #[error("API rejected request (HTTP {status}, {code})")]
    Api { status: u16, code: String },
    #[error("API response failed contract or size validation")]
    Contract,
    #[error("API response source differs from the requested source")]
    SourceMismatch,
}
pub type Result<T> = std::result::Result<T, ClientError>;

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    endpoint: Url,
    source: Source,
    operator: Option<header::HeaderValue>,
}
impl Client {
    /// HTTPS endpoint, or loopback HTTP for local development; provider keys never belong here.
    pub fn new(endpoint: &str, source: Source, operator: Option<&str>) -> Result<Self> {
        let endpoint = Url::parse(endpoint).map_err(|_| ClientError::Configuration)?;
        let loopback = matches!(
            endpoint.host_str(),
            Some("localhost" | "127.0.0.1" | "[::1]")
        );
        if !(endpoint.scheme() == "https" || endpoint.scheme() == "http" && loopback)
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || !matches!(endpoint.path(), "" | "/")
            || endpoint.host_str().is_none()
        {
            return Err(ClientError::Configuration);
        }
        let operator = operator
            .map(|key| {
                if !(16..=256).contains(&key.len()) || !key.bytes().all(|b| b.is_ascii_graphic()) {
                    return Err(ClientError::Configuration);
                }
                let mut value = header::HeaderValue::from_str(&format!("Bearer {key}"))
                    .map_err(|_| ClientError::Configuration)?;
                value.set_sensitive(true);
                Ok(value)
            })
            .transpose()?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ClientError::Configuration)?;
        Ok(Self {
            http,
            endpoint,
            source,
            operator,
        })
    }
    pub fn source(&self) -> Source {
        self.source
    }
    fn scope(&self, source: Source) -> Result<()> {
        if source == self.source {
            Ok(())
        } else {
            Err(ClientError::SourceMismatch)
        }
    }
    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Vec<u8>>,
    ) -> Result<T> {
        let mut url = self
            .endpoint
            .join(path)
            .map_err(|_| ClientError::Configuration)?;
        if !query.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        }
        if url.query().is_some_and(|s| s.len() > 8192)
            || body.as_ref().is_some_and(|b| b.len() > 65536)
        {
            return Err(ClientError::Configuration);
        }
        let write = method != Method::GET;
        let mut request = self
            .http
            .request(method, url)
            .header(header::ACCEPT, "application/json");
        if write {
            request = request.header(
                header::AUTHORIZATION,
                self.operator
                    .as_ref()
                    .ok_or(ClientError::Configuration)?
                    .clone(),
            );
        }
        if let Some(body) = body {
            request = request
                .header(header::CONTENT_TYPE, "application/json")
                .body(body);
        }
        let mut response = request.send().await.map_err(|_| ClientError::Transport)?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|n| n > MAX_RESPONSE as u64)
        {
            return Err(ClientError::Contract);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| ClientError::Transport)? {
            if bytes.len() + chunk.len() > MAX_RESPONSE {
                return Err(ClientError::Contract);
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            let error: ApiErrorResponse =
                serde_json::from_slice(&bytes).map_err(|_| ClientError::Contract)?;
            self.scope(error.source)?;
            let code = if error.code.len() <= 64
                && error
                    .code
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b == b'_')
            {
                error.code
            } else {
                "API_ERROR".into()
            };
            return Err(ClientError::Api {
                status: status.as_u16(),
                code,
            });
        }
        serde_json::from_slice(&bytes).map_err(|_| ClientError::Contract)
    }
    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        self.request(Method::GET, path, query, None).await
    }
    async fn post<T: DeserializeOwned>(&self, path: &str, value: &impl Serialize) -> Result<T> {
        self.request(
            Method::POST,
            path,
            &[],
            Some(serde_json::to_vec(value).map_err(|_| ClientError::Contract)?),
        )
        .await
    }
    pub async fn health(&self) -> Result<ApiHealth> {
        let r: ApiHealth = self.get("/v1/health", &[]).await?;
        self.scope(r.source)?;
        Ok(r)
    }
    pub async fn clock(&self) -> Result<ApiClock> {
        let r: ApiClock = self.get("/v1/clock", &[]).await?;
        self.scope(r.source)?;
        Ok(r)
    }
    pub async fn leaders(&self) -> Result<ApiTelemetry> {
        let r: ApiTelemetry = self.get("/v1/leaders", &[]).await?;
        self.scope(r.source)?;
        Ok(r)
    }
    pub async fn observers(&self) -> Result<ObserverHealthPage> {
        let r: ObserverHealthPage = self.get("/v1/observers", &[]).await?;
        self.scope(r.source)?;
        Ok(r)
    }
    pub async fn diagnostics(&self) -> Result<DiagnosticsPage> {
        let r: DiagnosticsPage = self.get("/v1/diagnostics", &[]).await?;
        self.scope(r.source)?;
        for source in r
            .signals
            .iter()
            .map(|s| s.source)
            .chain(r.regimes.iter().map(|s| s.source))
            .chain(r.alerts.iter().map(|s| s.source))
            .chain(r.backfills.iter().map(|s| s.source))
            .chain(r.disagreements.iter().map(|s| s.source))
        {
            self.scope(source)?;
        }
        Ok(r)
    }
    /// Read-only probability/economics preview; no ledger row or model is written.
    pub async fn quote(&self, request: &QuoteServiceRequest) -> Result<QuotePreview> {
        self.scope(request.model.context.source)?;
        let query = serde_json::to_string(request).map_err(|_| ClientError::Contract)?;
        let r: QuotePreview = self.get("/v1/quote", &[("request", query)]).await?;
        self.scope(r.quote.context.source)?;
        Ok(r)
    }
    /// Authenticated immutable forecast; the returned hash is the input to Prove.
    pub async fn freeze_quote(&self, request: &QuoteServiceRequest) -> Result<ForecastEntry> {
        self.scope(request.model.context.source)?;
        let r: ForecastEntry = self.post("/v1/quote", request).await?;
        self.scope(r.forecast.source)?;
        Ok(r)
    }
    /// Sends held-out canaries through the daemon governor; sim executes synthetic outcomes.
    pub async fn prove(&self, request: &ProveRequest) -> Result<ProveReport> {
        let r: ProveReport = self.post("/v1/prove", request).await?;
        self.scope(r.lock.source)?;
        Ok(r)
    }
    /// Quote then send a fixed canary cell through Prove. This is not a swap sender.
    pub async fn quote_then_send(
        &self,
        request: &QuoteServiceRequest,
        request_id: &str,
        n: u32,
    ) -> Result<ProveReport> {
        if !(1..=400).contains(&n)
            || request_id.is_empty()
            || request_id.len() > 64
            || !request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(ClientError::Configuration);
        }
        let forecast = self.freeze_quote(request).await?;
        self.prove(&ProveRequest {
            request_id: request_id.into(),
            forecast_hash: forecast.hash,
            n,
            seed: None,
        })
        .await
    }
    pub async fn prove_report(&self, id: &str) -> Result<ProveReport> {
        if id.is_empty()
            || id.len() > 100
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(ClientError::Configuration);
        }
        let r: ProveReport = self.get(&format!("/v1/prove/{id}"), &[]).await?;
        self.scope(r.lock.source)?;
        Ok(r)
    }
    pub async fn proves(&self, limit: u32) -> Result<ProvePage> {
        let r: ProvePage = self
            .get("/v1/proves", &[("limit", limit.to_string())])
            .await?;
        self.scope(r.source)?;
        Ok(r)
    }
    pub async fn curves(&self, limit: u32) -> Result<CurvePage> {
        let r: CurvePage = self
            .get("/v1/curve", &[("limit", limit.to_string())])
            .await?;
        self.scope(r.source)?;
        Ok(r)
    }
    pub async fn ledger(&self, after: u64, limit: u32) -> Result<LedgerPage> {
        let r: LedgerPage = self
            .get(
                "/v1/ledger",
                &[("after", after.to_string()), ("limit", limit.to_string())],
            )
            .await?;
        self.scope(r.source)?;
        Ok(r)
    }
    pub async fn verify_ledger(&self) -> Result<LedgerVerification> {
        let r: LedgerVerification = self.get("/v1/ledger/verify", &[]).await?;
        self.scope(r.source)?;
        if !r.verified {
            return Err(ClientError::Contract);
        }
        Ok(r)
    }
    /// Passive tape timestamps are RFC3339 UTC; the API caps windows at one day.
    /// Read-only receipt from an exact imported capture. No provider fetch or signing.
    pub async fn receipt(
        &self,
        wallet: &str,
        capture: &str,
        scope: &WalletReceiptRequest,
    ) -> Result<WalletReceipt> {
        if !(32..=44).contains(&wallet.len())
            || !wallet
                .bytes()
                .all(|b| b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz".contains(&b))
            || capture.len() != 71
            || !capture.starts_with("sha256:")
            || !capture[7..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(ClientError::Configuration);
        }
        let r: WalletReceipt = self
            .request(
                Method::GET,
                &format!("/v1/receipt/{wallet}"),
                &[
                    ("capture", capture.into()),
                    ("region", scope.region.clone()),
                    ("target_p", scope.target_p.to_string()),
                    ("horizon_slots", scope.horizon_slots.to_string()),
                    ("max_curve_age_s", scope.max_curve_age_s.to_string()),
                ],
                None,
            )
            .await?;
        self.scope(r.source)?;
        Ok(r)
    }
    pub async fn tape(&self, from: &str, through: &str, limit: u32) -> Result<TapePage> {
        let r: TapePage = self
            .get(
                "/v1/tape",
                &[
                    ("from", from.into()),
                    ("through", through.into()),
                    ("limit", limit.to_string()),
                ],
            )
            .await?;
        self.scope(r.source)?;
        Ok(r)
    }
}

#[cfg(test)]
mod tests;
