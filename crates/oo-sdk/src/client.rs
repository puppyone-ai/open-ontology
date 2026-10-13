//! One query attempt, then the narrow admission retry.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::header::{
    ACCEPT, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, RETRY_AFTER, TRANSFER_ENCODING,
};
use reqwest::{Certificate, Method, StatusCode};

use oo_core::api::{ErrorCode, QueryRequest, QueryResponse};

use crate::config::{ClientConfig, RetryPolicy, RootTrust};
use crate::endpoint::Endpoint;
use crate::error::{
    AdmissionWait, GatewayError, LocalError, ProtocolError, QueryError, TransportError,
    classify_transport,
};
use crate::token::BearerToken;
use crate::wire::{decode_error, decode_response, encode_request, scrub_text};

const BACKOFF_BASE: Duration = Duration::from_millis(200);

pub struct Client {
    http: reqwest::Client,
    endpoint: Endpoint,
    token: BearerToken,
    config: ClientConfig,
    auth_cache_remaining_ms: Mutex<Option<u128>>,
}

impl Client {
    /// Accepts an origin and credentials the caller has already read.
    ///
    /// This function does not open a file and does not connect. The bearer,
    /// trust PEM, and client identity are in-memory values. A caller that is
    /// handed a path has to reject the mode, the owner, and a symlink before
    /// it builds a client. The `public_call` example does that, and only then
    /// calls `new`. Passing bytes that were read earlier is outside that file
    /// gate: this library never sees the path.
    pub fn new(base: &str, token: BearerToken, config: ClientConfig) -> Result<Self, QueryError> {
        let endpoint = Endpoint::parse(base, config.transport())
            .map_err(|error| QueryError::Local(LocalError::Endpoint(error)))?;
        let http = build_http(&config)?;
        Ok(Self {
            http,
            endpoint,
            token,
            config,
            auth_cache_remaining_ms: Mutex::new(None),
        })
    }

    /// Remaining server lookup-cache time from the last response that sent it.
    pub fn auth_cache_remaining_ms(&self) -> Option<u128> {
        self.auth_cache_remaining_ms
            .lock()
            .ok()
            .and_then(|slot| *slot)
    }

    fn note_auth_cache(&self, headers: &HeaderMap) {
        let Some(ms) = note_header_remaining(headers) else {
            return;
        };
        if let Ok(mut slot) = self.auth_cache_remaining_ms.lock() {
            *slot = Some(ms);
        }
    }

    pub fn query_url(&self) -> &str {
        self.endpoint.as_str()
    }

    pub fn retry_policy(&self) -> RetryPolicy {
        self.config.retry_policy()
    }

    pub async fn query(&self, request: QueryRequest) -> Result<QueryResponse, QueryError> {
        let sql = request.sql.as_str().to_owned();
        let body = encode_request(&request, self.config.byte_caps().request)?;
        let started = Instant::now();
        let mut extra_used = 0u8;
        loop {
            let remaining = self
                .config
                .timeouts()
                .overall
                .saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(QueryError::Transport(TransportError::Timeout));
            }
            let per_attempt = remaining.min(self.config.timeouts().request);
            match self.once(&body, &sql, per_attempt).await {
                Ok(response) => return Ok(response),
                Err(error) => {
                    if extra_used >= self.config.retry_policy().max_extra_attempts()
                        || !admission_retry(&error)
                    {
                        return Err(error);
                    }
                    let wait = match retry_delay(&error, extra_used, self.config.retry_policy()) {
                        Some(wait) => wait,
                        None => return Err(error),
                    };
                    let remaining = self
                        .config
                        .timeouts()
                        .overall
                        .saturating_sub(started.elapsed());
                    if wait > remaining || remaining.is_zero() {
                        return Err(error);
                    }
                    extra_used = extra_used.saturating_add(1);
                    if !wait.is_zero() {
                        tokio::time::sleep(wait).await;
                    }
                }
            }
        }
    }

    async fn once(
        &self,
        body: &[u8],
        sql: &str,
        per_attempt: Duration,
    ) -> Result<QueryResponse, QueryError> {
        let response = self
            .http
            .post(self.endpoint.url().clone())
            .timeout(per_attempt)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .header(AUTHORIZATION, self.token.authorization())
            .body(body.to_vec())
            .send()
            .await
            .map_err(|error| {
                if error.is_redirect() {
                    QueryError::Protocol(ProtocolError::Redirect)
                } else {
                    QueryError::Transport(classify_transport(&error))
                }
            })?;
        if response.url().as_str() != self.endpoint.as_str() {
            return Err(QueryError::Protocol(ProtocolError::Redirect));
        }
        let status = response.status();
        let headers = response.headers().clone();
        self.note_auth_cache(&headers);
        let retry_after = parse_retry_after(&headers);
        let bytes = read_capped(response, self.config.byte_caps().response).await?;
        interpret(
            status,
            &headers,
            retry_after,
            &bytes,
            self.token.secret(),
            sql,
            self.endpoint.as_str(),
        )
    }

    pub async fn tables(&self) -> Result<serde_json::Value, QueryError> {
        self.get_object("/v1/tables").await
    }

    pub async fn describe(&self, table: &str) -> Result<serde_json::Value, QueryError> {
        self.get_object(&one_public_path("/v1/tables", table)?)
            .await
    }

    pub async fn sql_validate(&self, sql: &str) -> Result<serde_json::Value, QueryError> {
        let body = serde_json::json!({"sql": sql});
        self.post_object("/v1/sql/validate", &body).await
    }

    pub async fn freshness(&self) -> Result<serde_json::Value, QueryError> {
        self.get_object("/v1/freshness").await
    }

    pub async fn knowledge(&self, path: &str) -> Result<serde_json::Value, QueryError> {
        self.get_object(&public_path("/v1/knowledge", path)?).await
    }

    pub async fn knowledge_search(&self, needle: &str) -> Result<serde_json::Value, QueryError> {
        let body = serde_json::json!({"q": needle});
        self.post_object("/v1/knowledge/search", &body).await
    }

    pub async fn operations(&self) -> Result<serde_json::Value, QueryError> {
        self.get_object("/v1/operations").await
    }

    pub async fn query_status(&self, query_id: &str) -> Result<serde_json::Value, QueryError> {
        self.get_object(&one_public_path(&id_prefix(query_id), query_id)?)
            .await
    }

    pub async fn cancel(&self, query_id: &str) -> Result<serde_json::Value, QueryError> {
        let path = format!(
            "{}/cancel",
            one_public_path(&id_prefix(query_id), query_id)?
        );
        self.post_object(&path, &serde_json::json!({})).await
    }

    async fn get_object(&self, path: &str) -> Result<serde_json::Value, QueryError> {
        self.exchange(Method::GET, path, None).await
    }

    async fn post_object(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, QueryError> {
        let bytes =
            serde_json::to_vec(body).map_err(|_| QueryError::Local(LocalError::RequestRejected))?;
        self.exchange(Method::POST, path, Some(bytes)).await
    }

    async fn exchange(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<serde_json::Value, QueryError> {
        let url = self
            .endpoint
            .sibling(path)
            .map_err(|error| QueryError::Local(LocalError::Endpoint(error)))?;
        let per_attempt = self.config.timeouts().request;
        let mut request = self
            .http
            .request(method, url.clone())
            .timeout(per_attempt)
            .header(ACCEPT, "application/json")
            .header(AUTHORIZATION, self.token.authorization());
        if let Some(bytes) = body {
            request = request.header(CONTENT_TYPE, "application/json").body(bytes);
        }
        let response = request.send().await.map_err(|error| {
            if error.is_redirect() {
                QueryError::Protocol(ProtocolError::Redirect)
            } else {
                QueryError::Transport(classify_transport(&error))
            }
        })?;
        if response.url().as_str() != url.as_str() {
            return Err(QueryError::Protocol(ProtocolError::Redirect));
        }
        let status = response.status();
        let headers = response.headers().clone();
        self.note_auth_cache(&headers);
        let retry_after = parse_retry_after(&headers);
        let bytes = read_capped(response, self.config.byte_caps().response).await?;
        interpret_value(
            status,
            &headers,
            retry_after,
            &bytes,
            self.token.secret(),
            url.as_str(),
        )
    }
}

impl std::fmt::Debug for Client {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Client")
            .field("token", &self.token)
            .field("retry", &self.config.retry_policy())
            .finish()
    }
}

fn build_http(config: &ClientConfig) -> Result<reqwest::Client, QueryError> {
    let timeouts = config.timeouts();
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .referer(false)
        .user_agent("oo-sdk/0.1")
        .connection_verbose(false)
        .https_only(config.transport() == crate::config::TransportMode::Production)
        .connect_timeout(timeouts.connect)
        .read_timeout(timeouts.read)
        .timeout(timeouts.request)
        .pool_idle_timeout(Duration::from_secs(15));
    if let RootTrust::OnlyPem(pem) = config.trust() {
        let certs =
            Certificate::from_pem_bundle(pem).map_err(|_| QueryError::Local(LocalError::Trust))?;
        if certs.is_empty() {
            return Err(QueryError::Local(LocalError::Trust));
        }
        builder = builder.tls_certs_only(certs);
    }
    if let Some(pem) = config.identity() {
        let identity =
            reqwest::Identity::from_pem(pem).map_err(|_| QueryError::Local(LocalError::Trust))?;
        builder = builder.identity(identity);
    }
    builder
        .build()
        .map_err(|_| QueryError::Local(LocalError::Client))
}

fn admission_retry(error: &QueryError) -> bool {
    let Some(gateway) = error.gateway() else {
        return false;
    };
    gateway.retryable()
        && matches!(
            gateway.code(),
            ErrorCode::RateLimited | ErrorCode::Overloaded
        )
        && gateway.code().http_status() == gateway.status()
}

fn retry_delay(error: &QueryError, extra_used: u8, policy: RetryPolicy) -> Option<Duration> {
    let gateway = error.gateway()?;
    match gateway.admission_wait() {
        AdmissionWait::Overflow => None,
        AdmissionWait::Decimal(after) => {
            if after >= policy.max_wait() {
                None
            } else {
                Some(after)
            }
        }
        AdmissionWait::Backoff => {
            let shift = u32::from(extra_used.min(4));
            let backoff = BACKOFF_BASE.saturating_mul(1u32 << shift);
            Some(backoff.min(policy.max_wait()))
        }
    }
}

const RETRY_AFTER_SCAN: usize = 4_096;

fn parse_retry_after(headers: &HeaderMap) -> AdmissionWait {
    let values: Vec<_> = headers.get_all(RETRY_AFTER).iter().collect();
    if values.len() != 1 {
        return AdmissionWait::Backoff;
    }
    let Ok(text) = values[0].to_str() else {
        return AdmissionWait::Backoff;
    };
    if text.is_empty() {
        return AdmissionWait::Backoff;
    }
    let scanned = text.len().min(RETRY_AFTER_SCAN);
    if !text.as_bytes()[..scanned]
        .iter()
        .all(|byte| byte.is_ascii_digit())
    {
        return AdmissionWait::Backoff;
    }
    if text.len() > RETRY_AFTER_SCAN {
        return AdmissionWait::Overflow;
    }
    let significant = text.trim_start_matches('0');
    if significant.is_empty() {
        return AdmissionWait::Decimal(Duration::ZERO);
    }
    if significant.len() > 20 {
        return AdmissionWait::Overflow;
    }
    match significant.parse::<u64>() {
        Ok(seconds) => AdmissionWait::Decimal(Duration::from_secs(seconds)),
        Err(_) => AdmissionWait::Overflow,
    }
}

async fn read_capped(mut response: reqwest::Response, cap: usize) -> Result<Vec<u8>, QueryError> {
    let lengths: Vec<_> = response.headers().get_all(CONTENT_LENGTH).iter().collect();
    if lengths.len() > 1 {
        return Err(QueryError::Protocol(ProtocolError::ConflictingFraming));
    }
    let chunked = response.headers().get(TRANSFER_ENCODING).is_some();
    if chunked && !lengths.is_empty() {
        return Err(QueryError::Protocol(ProtocolError::ConflictingFraming));
    }
    if let Some(len) = response.content_length()
        && len > cap as u64
    {
        return Err(QueryError::Protocol(ProtocolError::DeclaredTooLarge));
    }
    let mut buf = Vec::new();
    loop {
        let next = response
            .chunk()
            .await
            .map_err(|error| QueryError::Transport(classify_transport(&error)))?;
        let Some(chunk) = next else {
            break;
        };
        if chunk.len() > cap || buf.len() > cap - chunk.len() {
            return Err(QueryError::Protocol(ProtocolError::BodyTooLarge));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

fn interpret(
    status: StatusCode,
    headers: &HeaderMap,
    admission: AdmissionWait,
    bytes: &[u8],
    token: &str,
    sql: &str,
    endpoint: &str,
) -> Result<QueryResponse, QueryError> {
    let code = status.as_u16();
    if (300..400).contains(&code) {
        return Err(QueryError::Protocol(ProtocolError::Redirect));
    }
    if !json_content_type(headers) {
        return Err(QueryError::Protocol(ProtocolError::ContentType));
    }
    if code == 200 {
        return decode_response(bytes);
    }
    let envelope = decode_error(bytes)?;
    if envelope.error.code().http_status() != code || !envelope.consistent() {
        return Err(QueryError::Protocol(ProtocolError::Status { status: code }));
    }
    let query_id = envelope.error.query_id().cloned();
    let hint = envelope
        .error
        .hint()
        .map(|text| scrub_text(text.as_str(), token, sql, endpoint));
    Err(QueryError::Gateway(GatewayError::new(
        code,
        envelope.error.code(),
        envelope.error.retryable(),
        query_id,
        admission,
        scrub_text(envelope.error.message().as_str(), token, sql, endpoint),
        hint,
    )))
}

fn interpret_value(
    status: StatusCode,
    headers: &HeaderMap,
    admission: AdmissionWait,
    bytes: &[u8],
    token: &str,
    endpoint: &str,
) -> Result<serde_json::Value, QueryError> {
    let code = status.as_u16();
    if (300..400).contains(&code) {
        return Err(QueryError::Protocol(ProtocolError::Redirect));
    }
    if !json_content_type(headers) {
        return Err(QueryError::Protocol(ProtocolError::ContentType));
    }
    if code == 200 {
        let value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|_| QueryError::Protocol(ProtocolError::Rejected))?;
        if !value.is_object() {
            return Err(QueryError::Protocol(ProtocolError::Rejected));
        }
        return Ok(value);
    }
    let envelope = decode_error(bytes)?;
    if envelope.error.code().http_status() != code || !envelope.consistent() {
        return Err(QueryError::Protocol(ProtocolError::Status { status: code }));
    }
    let query_id = envelope.error.query_id().cloned();
    let hint = envelope
        .error
        .hint()
        .map(|text| scrub_text(text.as_str(), token, "", endpoint));
    Err(QueryError::Gateway(GatewayError::new(
        code,
        envelope.error.code(),
        envelope.error.retryable(),
        query_id,
        admission,
        scrub_text(envelope.error.message().as_str(), token, "", endpoint),
        hint,
    )))
}

fn one_public_path(prefix: &str, segment: &str) -> Result<String, QueryError> {
    public_path(prefix, segment)
}

fn id_prefix(query_id: &str) -> String {
    if query_id.starts_with("s_") {
        "/v1/operations".to_string()
    } else {
        "/v1/queries".to_string()
    }
}

fn note_header_remaining(headers: &HeaderMap) -> Option<u128> {
    let value = headers.get("x-oo-auth-cache-remaining-ms")?;
    let text = value.to_str().ok()?;
    text.parse().ok()
}

fn public_path(prefix: &str, path: &str) -> Result<String, QueryError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path.contains('%')
    {
        return Err(QueryError::Local(LocalError::RequestRejected));
    }
    for segment in path.split('/') {
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || !segment.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'-')
            })
        {
            return Err(QueryError::Local(LocalError::RequestRejected));
        }
    }
    Ok(format!("{prefix}/{path}"))
}

fn json_content_type(headers: &HeaderMap) -> bool {
    let Some(value) = headers.get(CONTENT_TYPE) else {
        return false;
    };
    let Ok(text) = value.to_str() else {
        return false;
    };
    let mut parts = text.split(';');
    let Some(essence) = parts.next() else {
        return false;
    };
    if !essence.trim().eq_ignore_ascii_case("application/json") {
        return false;
    }
    for param in parts {
        let param = param.trim();
        let Some((name, raw)) = param.split_once('=') else {
            return false;
        };
        if !name.trim().eq_ignore_ascii_case("charset") {
            return false;
        }
        let charset = raw.trim().trim_matches('"');
        if !charset.eq_ignore_ascii_case("utf-8") {
            return false;
        }
    }
    true
}
