//! One query attempt, then the narrow admission retry.

use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::header::{
    ACCEPT, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, RETRY_AFTER, TRANSFER_ENCODING,
};
use reqwest::{Certificate, Method, StatusCode};

use oo_core::api::{ErrorCode, QueryRequest, QueryResponse};

use crate::config::{ClientConfig, RetryPolicy, RootTrust, QUERY_RESPONSE_RESERVE};
use crate::endpoint::Endpoint;
use crate::error::{
    AdmissionWait, GatewayError, LocalError, ProtocolError, QueryError, TransportError,
    classify_transport,
};
use crate::token::BearerToken;
use crate::wire::{decode_error, decode_response, encode_request, scrub_text};

const BACKOFF_BASE: Duration = Duration::from_millis(200);
use crate::publication::{PublicationBinding, QueryReceipt, EXPECTED, EXPECTED_VECTOR};

pub struct Client {
    http: reqwest::Client,
    endpoint: Endpoint,
    token: BearerToken,
    config: ClientConfig,
    auth_cache_remaining_ms: Mutex<Option<u128>>,
    publication_binding: Option<PublicationBinding>,
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
        let http = build_http(&config, &endpoint)?;
        Ok(Self {
            http,
            endpoint,
            token,
            config,
            auth_cache_remaining_ms: Mutex::new(None),
            publication_binding: None,
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

    /// Binding constrains reads; it never replaces the bearer or selects a role.
    pub fn with_publication_binding(mut self, binding: PublicationBinding) -> Self {
        self.publication_binding = Some(binding);
        self
    }

    fn binding_headers(&self, mut request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(binding) = &self.publication_binding {
            request = request.header(EXPECTED, &binding.publication_id);
            if let Some(vector) = &binding.data_vector_sha256 {
                request = request.header(EXPECTED_VECTOR, vector);
            }
        }
        request
    }

    pub async fn query(&self, request: QueryRequest) -> Result<QueryResponse, QueryError> {
        self.query_with_receipt(request).await.map(|result| result.response)
    }

    pub async fn query_with_receipt(&self, mut request: QueryRequest) -> Result<QueryReceipt, QueryError> {
        let sql = request.sql.as_str().to_owned();
        // Validate before attempting a socket, then recompute the shared remaining
        // budget after each admission-only retry. Never replay a timed-out query.
        encode_request(&request, self.config.byte_caps().request)?;
        let requested_budget = request.timeout_ms;
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
            let server_ms = per_attempt.saturating_sub(QUERY_RESPONSE_RESERVE).as_millis().max(1);
            request.timeout_ms = Some(requested_budget.unwrap_or(30_000)
                .min(30_000).min(server_ms.min(u128::from(u32::MAX)) as u32));
            let body = encode_request(&request, self.config.byte_caps().request)?;
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
    ) -> Result<QueryReceipt, QueryError> {
        let request = self
            .http
            .post(self.endpoint.url().clone())
            .timeout(per_attempt)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .header(AUTHORIZATION, self.token.authorization())
            .body(body.to_vec());
        let response = self.binding_headers(request)
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
        let response = interpret(
            status,
            &headers,
            retry_after,
            &bytes,
            self.token.secret(),
            sql,
            self.endpoint.as_str(),
        )?;
        let publication_binding = self.publication_binding.as_ref()
            .map(|binding| binding.receipt(&headers)).transpose()?;
        Ok(QueryReceipt { response, publication_binding })
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
        self.knowledge_search_page(needle, None, 0, 20).await
    }

    pub async fn knowledge_search_page(&self, needle: &str, path_prefix: Option<&str>,
                                      offset: u32, limit: u32) -> Result<serde_json::Value, QueryError> {
        let body = serde_json::json!({"q": needle, "path_prefix": path_prefix, "offset": offset, "limit": limit});
        let response = self.post_object("/v1/knowledge/search", &body).await?;
        // An old Gateway may ignore unknown options; never pass its first page off as a later page.
        if (path_prefix.is_some() || offset != 0 || limit != 20)
            && (response["offset"].as_u64() != Some(u64::from(offset))
                || response["limit"].as_u64() != Some(u64::from(limit))
                || !response["has_more"].is_boolean() || !response["total"].is_u64()) {
            return Err(QueryError::Protocol(ProtocolError::Rejected));
        }
        Ok(response)
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
        self.exchange(Method::GET, path, None, "").await
    }

    async fn post_object(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, QueryError> {
        let bytes =
            serde_json::to_vec(body).map_err(|_| QueryError::Local(LocalError::RequestRejected))?;
        self.exchange(
            Method::POST, path, Some(bytes),
            body.get("sql").and_then(serde_json::Value::as_str).unwrap_or(""),
        ).await
    }

    async fn exchange(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        sql: &str,
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
        let response = self.binding_headers(request).send().await.map_err(|error| {
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
        let mut value = interpret_value(
            status,
            &headers,
            retry_after,
            &bytes,
            self.token.secret(),
            sql,
            url.as_str(),
        )?;
        if let Some(binding) = &self.publication_binding {
            let receipt = binding.receipt(&headers)?;
            value["publication_binding"] = serde_json::to_value(receipt)
                .map_err(|_| QueryError::Protocol(ProtocolError::Rejected))?;
        }
        Ok(value)
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

fn build_http(config: &ClientConfig, endpoint: &Endpoint) -> Result<reqwest::Client, QueryError> {
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
    if let Some(ip) = config.connect_ip() {
        // Numeric URL hosts bypass DNS resolution, so never silently ignore an override.
        let host = endpoint.url().domain().ok_or(QueryError::Local(LocalError::Config(
            "connect-ip requires a gateway DNS hostname, not an IP literal",
        )))?;
        if endpoint.url().scheme() == "http" && !ip.is_loopback() {
            return Err(QueryError::Local(LocalError::Config(
                "plaintext connect-ip must be loopback",
            )));
        }
        builder = builder.resolve(host, SocketAddr::new(ip, 0));
    }
    if let Some(interface) = config.interface() {
        #[cfg(any(
            target_os = "android", target_os = "fuchsia", target_os = "illumos",
            target_os = "ios", target_os = "linux", target_os = "macos",
            target_os = "solaris", target_os = "tvos", target_os = "visionos",
            target_os = "watchos",
        ))]
        {
            builder = builder.interface(interface);
        }
        #[cfg(not(any(
            target_os = "android", target_os = "fuchsia", target_os = "illumos",
            target_os = "ios", target_os = "linux", target_os = "macos",
            target_os = "solaris", target_os = "tvos", target_os = "visionos",
            target_os = "watchos",
        )))]
        {
            let _ = interface;
            return Err(QueryError::Local(LocalError::Config(
                "interface binding is unsupported on this platform",
            )));
        }
    }
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
    // gateway.example resets a ClientHello whose first share is the short
    // X25519 key. The hybrid share is already preferred for --connect-ip and
    // --interface; use it for every connection so a normal profile connects too.
    // routed_tls clones the provider and does not change the process default.
    builder = builder.tls_backend_preconfigured(routed_tls(config)?);
    builder
        .build()
        .map_err(|_| QueryError::Local(LocalError::Client))
}

fn routed_tls(config: &ClientConfig) -> Result<tokio_rustls::rustls::ClientConfig, QueryError> {
    use rustls_platform_verifier::BuilderVerifierExt;
    use tokio_rustls::rustls::{self, crypto::CryptoProvider};
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};

    let trust_error = || QueryError::Local(LocalError::Trust);
    let mut provider = CryptoProvider::get_default()
        .map(|provider| (**provider).clone())
        .unwrap_or_else(rustls::crypto::aws_lc_rs::default_provider);
    // The public gateway resets a short X25519-first ClientHello. Prefer the
    // existing hybrid share, without removing groups or changing process defaults.
    provider.kx_groups.sort_by_key(|group| group.name() != rustls::NamedGroup::X25519MLKEM768);
    let tls = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(provider))
        .with_safe_default_protocol_versions()
        .map_err(|_| trust_error())?;
    let tls = match config.trust() {
        RootTrust::Platform => tls.with_platform_verifier().map_err(|_| trust_error())?,
        RootTrust::OnlyPem(pem) => {
            let mut roots = rustls::RootCertStore::empty();
            for cert in CertificateDer::pem_slice_iter(pem) {
                roots.add(cert.map_err(|_| trust_error())?).map_err(|_| trust_error())?;
            }
            if roots.is_empty() {
                return Err(trust_error());
            }
            tls.with_root_certificates(roots)
        }
    };
    let mut tls = match config.identity() {
        Some(pem) => {
            // Match reqwest's last-key selection; its parser has already validated the PEM.
            let certs = CertificateDer::pem_slice_iter(pem)
                .collect::<Result<Vec<_>, _>>().map_err(|_| trust_error())?;
            let key = PrivateKeyDer::pem_slice_iter(pem)
                .collect::<Result<Vec<_>, _>>().map_err(|_| trust_error())?
                .pop().ok_or_else(trust_error)?;
            tls.with_client_auth_cert(certs, key).map_err(|_| trust_error())?
        }
        None => tls.with_no_client_auth(),
    };
    tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(tls)
}

#[cfg(test)]
mod route_tls_tests {
    use super::*;
    use tokio_rustls::rustls::{self, crypto::CryptoProvider};

    #[test]
    fn routed_hello_prefers_existing_hybrid_without_changing_global_provider() {
        let before = CryptoProvider::get_default().map(std::sync::Arc::as_ptr);
        let tls = routed_tls(&ClientConfig::production()).unwrap();
        assert_eq!(tls.crypto_provider().kx_groups[0].name(), rustls::NamedGroup::X25519MLKEM768);
        assert!(tls.enable_sni);
        let mut connection = rustls::ClientConnection::new(
            std::sync::Arc::new(tls), "localhost".try_into().unwrap(),
        ).unwrap();
        let mut hello = Vec::new();
        connection.write_tls(&mut hello).unwrap();
        assert!(hello.len() > 1200, "hybrid ClientHello must include its full key share");
        assert_eq!(before, CryptoProvider::get_default().map(std::sync::Arc::as_ptr));
    }
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
    sql: &str,
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
        if !value.is_object() || value.get("error").is_some() {
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
