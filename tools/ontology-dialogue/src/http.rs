//! One HTTPS POST. No proxy, no redirect, no retry, no second endpoint.

use std::time::{Duration, Instant};

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};

use crate::error::Fault;

pub const USER_AGENT: &str = "oo-ontology-dialogue/v1";

pub struct ChatReply {
    pub status: u16,
    pub body: Vec<u8>,
}

pub async fn post_once(
    url: &str,
    ca_pem: &[u8],
    bearer: &str,
    body: &[u8],
    deadline: Duration,
    max_bytes: usize,
    max_chunks: usize,
) -> Result<ChatReply, Fault> {
    let certs = reqwest::Certificate::from_pem_bundle(ca_pem).map_err(|_| Fault::config())?;
    if certs.is_empty() {
        return Err(Fault::config());
    }
    let parsed = reqwest::Url::parse(url).map_err(|_| Fault::config())?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .https_only(true)
        .http1_only()
        .referer(false)
        .connection_verbose(false)
        .user_agent(USER_AGENT)
        .tls_certs_only(certs)
        .connect_timeout(deadline)
        .timeout(deadline)
        .build()
        .map_err(|_| Fault::config())?;
    let mut authorization = HeaderValue::from_str(&format!("Bearer {bearer}")).map_err(|_| Fault::config())?;
    authorization.set_sensitive(true);
    let started = Instant::now();
    let response = client
        .post(parsed.clone())
        .timeout(deadline)
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, authorization)
        .body(body.to_vec())
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(_) => return Err(Fault::provider()),
    };
    if response.url() != &parsed {
        return Err(Fault::provider());
    }
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    if header_has_secret(&headers, bearer) {
        return Err(Fault::secret());
    }
    let mut response = response;
    let bytes = read_body(&mut response, started, deadline, max_bytes, max_chunks, bearer).await?;
    if (300..400).contains(&status) {
        return Err(Fault::provider());
    }
    Ok(ChatReply { status, body: bytes })
}

async fn read_body(
    response: &mut reqwest::Response,
    started: Instant,
    deadline: Duration,
    max_bytes: usize,
    max_chunks: usize,
    bearer: &str,
) -> Result<Vec<u8>, Fault> {
    let mut buf = Vec::new();
    let mut chunks = 0_usize;
    loop {
        let remaining = deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return secret_or(Fault::provider(), &buf, bearer);
        }
        let next = tokio::time::timeout(remaining, response.chunk()).await;
        let chunk = match next {
            Ok(Ok(Some(chunk))) => chunk,
            Ok(Ok(None)) => break,
            Ok(Err(_)) | Err(_) => return secret_or(Fault::provider(), &buf, bearer),
        };
        chunks = chunks.saturating_add(1);
        if chunks > max_chunks || buf.len().saturating_add(chunk.len()) > max_bytes {
            if contains_slice(&buf, bearer.as_bytes()) || contains_slice(&chunk, bearer.as_bytes()) {
                return Err(Fault::secret());
            }
            return Err(Fault::budget());
        }
        buf.extend_from_slice(&chunk);
    }
    if contains_slice(&buf, bearer.as_bytes()) {
        return Err(Fault::secret());
    }
    Ok(buf)
}

fn secret_or(fault: Fault, buf: &[u8], bearer: &str) -> Result<Vec<u8>, Fault> {
    if contains_slice(buf, bearer.as_bytes()) {
        Err(Fault::secret())
    } else {
        Err(fault)
    }
}

fn header_has_secret(headers: &HeaderMap, bearer: &str) -> bool {
    headers.iter().any(|(name, value)| {
        name.as_str() != "authorization"
            && value
                .as_bytes()
                .windows(bearer.len())
                .any(|window| window == bearer.as_bytes())
    })
}

fn contains_slice(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}
