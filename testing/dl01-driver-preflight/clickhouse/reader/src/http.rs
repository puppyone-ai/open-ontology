use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::fail::{fail, Gate};

pub struct HttpOutcome {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub elapsed_ms: u128,
    pub error: Option<String>,
    pub exception_code: Option<i64>,
}

pub fn password_header(path: &Path) -> Gate<HeaderValue> {
    let mut text = fs::read_to_string(path).map_err(|error| fail(format!("password file: {error}")))?;
    let value = HeaderValue::from_str(text.trim()).map_err(|_| {
        fail("password file is not a valid HTTP header value")
    });
    text.zeroize();
    value
}

pub fn read_trimmed(path: &Path) -> Gate<String> {
    let shown = path.display().to_string();
    let text = fs::read_to_string(path).map_err(|error| fail(format!("read {shown}: {error}")))?;
    Ok(text.trim().to_string())
}

pub fn build_client(timeout: Duration) -> Gate<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .http1_only()
        .pool_max_idle_per_host(0)
        .connect_timeout(Duration::from_secs(5))
        .timeout(timeout)
        .build()
        .map_err(|error| fail(format!("http client: {error}")))
}

pub fn endpoint_url(endpoint: &str, params: &[(String, String)]) -> Gate<String> {
    let mut url = endpoint.trim_end_matches('/').to_string();
    if !url.starts_with("http://") {
        return Err(fail("endpoint must be an http:// URL"));
    }
    if params.is_empty() {
        return Ok(url + "/");
    }
    url.push_str("/?");
    for (index, (key, value)) in params.iter().enumerate() {
        if !safe_token(key) || !safe_token(value) {
            return Err(fail(format!("refusing query parameter {key}")));
        }
        if index > 0 {
            url.push('&');
        }
        url.push_str(key);
        url.push('=');
        url.push_str(value);
    }
    Ok(url)
}

fn safe_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

pub fn reader_headers(user: &str, password: &HeaderValue) -> Gate<HeaderMap> {
    let mut headers = HeaderMap::new();
    let name = HeaderValue::from_str(user).map_err(|_| fail("user name is not a valid header"))?;
    headers.insert(HeaderName::from_static("x-clickhouse-user"), name);
    headers.insert(HeaderName::from_static("x-clickhouse-key"), password.clone());
    Ok(headers)
}

pub async fn execute(
    client: &reqwest::Client,
    url: &str,
    headers: HeaderMap,
    sql: &str,
    cap: usize,
) -> HttpOutcome {
    let started = Instant::now();
    let sent = client.post(url).headers(headers).body(sql.to_string()).send().await;
    match sent {
        Ok(response) => read_response(response, started, cap).await,
        Err(error) => HttpOutcome {
            status: 0,
            headers: Vec::new(),
            body: Vec::new(),
            elapsed_ms: started.elapsed().as_millis(),
            error: Some(format!("connection_error {error}")),
            exception_code: None,
        },
    }
}

async fn read_response(response: reqwest::Response, started: Instant, cap: usize) -> HttpOutcome {
    let status = response.status().as_u16();
    let headers = visible_headers(response.headers());
    let exception_code = headers
        .iter()
        .find(|(name, _)| name == "x-clickhouse-exception-code")
        .and_then(|(_, value)| value.parse().ok());
    let mut body = Vec::new();
    let mut error = None;
    let mut response = response;
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len().saturating_add(chunk.len()) > cap {
                    error = Some(format!("response exceeded {cap} bytes; discarded"));
                    body.clear();
                    break;
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(err) => {
                error = Some(format!("body read failed: {err}"));
                body.clear();
                break;
            }
        }
    }
    let code = exception_code.or_else(|| code_in_body(&body));
    HttpOutcome {
        status,
        headers,
        body,
        elapsed_ms: started.elapsed().as_millis(),
        error,
        exception_code: code,
    }
}

fn visible_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, value) in headers {
        let key = name.as_str().to_ascii_lowercase();
        if key.contains("key") || key.contains("authorization") || key.contains("cookie") || key.contains("password") {
            continue;
        }
        let text = value.to_str().unwrap_or("").to_string();
        out.push((key, text));
    }
    out
}

pub fn code_in_body(body: &[u8]) -> Option<i64> {
    let text = String::from_utf8_lossy(body);
    let marker = "Code:";
    let start = text.find(marker)?;
    let rest = text[start + marker.len()..].trim_start();
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().ok()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn outcome_json(command: &str, query_id: &str, outcome: &HttpOutcome, extra: Map<String, Value>) -> Value {
    let mut body = Map::new();
    body.insert("command".into(), json!(command));
    body.insert("query_id".into(), json!(query_id));
    body.insert("status".into(), json!(outcome.status));
    body.insert("elapsed_ms".into(), json!(outcome.elapsed_ms));
    body.insert("exception_code".into(), json!(outcome.exception_code));
    body.insert("body_bytes".into(), json!(outcome.body.len()));
    body.insert("body_sha256".into(), json!(sha256_hex(&outcome.body)));
    body.insert("error".into(), json!(outcome.error));
    body.insert(
        "headers".into(),
        json!(outcome
            .headers
            .iter()
            .map(|(key, value)| json!({"name": key, "value": value}))
            .collect::<Vec<_>>()),
    );
    for (key, value) in extra {
        body.insert(key, value);
    }
    Value::Object(body)
}

pub fn write_json(path: &Path, value: &Value) -> Gate<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| fail(format!("mkdir: {error}")))?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|error| fail(format!("json: {error}")))?;
    let shown = path.display().to_string();
    fs::write(path, text + "\n").map_err(|error| fail(format!("write {shown}: {error}")))
}

pub fn write_bytes(path: &Path, bytes: &[u8]) -> Gate<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| fail(format!("mkdir: {error}")))?;
    }
    let shown = path.display().to_string();
    fs::write(path, bytes).map_err(|error| fail(format!("write {shown}: {error}")))
}
