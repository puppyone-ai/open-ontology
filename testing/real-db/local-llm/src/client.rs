use std::time::Duration;

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue};

use crate::allow::allow;
use crate::fault::Fault;
use crate::limits::Limits;

pub struct HttpReply {
    pub status: u16,
    pub body: Vec<u8>,
}

pub fn origin_ok(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("https://") else {
        return false;
    };
    if rest.is_empty() || rest.contains('@') || rest.contains('?') || rest.contains('#') || rest.contains('\\') {
        return false;
    }
    let (hostport, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    if path != "/" && !path.is_empty() {
        return false;
    }
    !hostport.is_empty() && !hostport.starts_with(':') && !hostport.ends_with(':')
}

pub fn join_origin(origin: &str, path: &str) -> Result<String, Fault> {
    if !origin_ok(origin) || allow("GET", path).or_else(|| allow("POST", path)).is_none() {
        return Err(Fault::config());
    }
    let mut base = origin.to_string();
    if base.ends_with('/') {
        base.pop();
    }
    base.push_str(path);
    Ok(base)
}

pub async fn https_once(
    origin: &str,
    path: &str,
    ca_pem: &[u8],
    bearer: Option<&str>,
    body: Option<&[u8]>,
    limits: Limits,
) -> Result<HttpReply, Fault> {
    let url_text = join_origin(origin, path)?;
    if let (Some(secret), Some(bytes)) = (bearer, body)
        && contains_bytes(bytes, secret.as_bytes())
    {
        return Err(Fault::secret());
    }
    let url = reqwest::Url::parse(&url_text).map_err(|_| Fault::config())?;
    let certs = reqwest::Certificate::from_pem_bundle(ca_pem).map_err(|_| Fault::tls())?;
    if certs.is_empty() {
        return Err(Fault::tls());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .https_only(true)
        .http1_only()
        .referer(false)
        .connection_verbose(false)
        .user_agent("oo-local-llm-models/v1")
        .tls_certs_only(certs)
        .connect_timeout(limits.deadline)
        .timeout(limits.deadline)
        .build()
        .map_err(|_| Fault::tls())?;
    let mut request = if let Some(body) = body {
        client
            .post(url.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(body.to_vec())
    } else {
        client.get(url.clone())
    };
    request = request.timeout(limits.deadline);
    if let Some(secret) = bearer {
        let mut header = HeaderValue::from_str(&format!("Bearer {secret}")).map_err(|_| Fault::config())?;
        header.set_sensitive(true);
        request = request.header(AUTHORIZATION, header);
    }
    let response = request.send().await.map_err(|_| Fault::upstream())?;
    if response.url() != &url || (300..400).contains(&response.status().as_u16()) {
        return Err(Fault::upstream());
    }
    let status = response.status().as_u16();
    let bytes = read_capped(response, limits.deadline, limits.max_response_bytes, limits.max_response_chunks).await?;
    if let Some(secret) = bearer
        && contains_bytes(&bytes, secret.as_bytes())
    {
        return Err(Fault::secret());
    }
    Ok(HttpReply { status, body: bytes })
}

async fn read_capped(
    mut response: reqwest::Response,
    deadline: Duration,
    max_bytes: usize,
    max_chunks: usize,
) -> Result<Vec<u8>, Fault> {
    let started = std::time::Instant::now();
    let mut buf = Vec::new();
    let mut chunks = 0_usize;
    loop {
        let remaining = deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(Fault::upstream());
        }
        let next = tokio::time::timeout(remaining, response.chunk()).await;
        let chunk = match next {
            Ok(Ok(Some(chunk))) => chunk,
            Ok(Ok(None)) => break,
            Ok(Err(_)) | Err(_) => return Err(Fault::upstream()),
        };
        chunks = chunks.saturating_add(1);
        if chunks > max_chunks || buf.len().saturating_add(chunk.len()) > max_bytes {
            return Err(Fault::response_budget());
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}
