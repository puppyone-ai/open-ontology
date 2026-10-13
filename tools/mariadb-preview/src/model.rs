//! One real model origin. Cache hits do not open a socket. 401 and timeout are separate calls.

use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

use crate::config::{self, Config, Phase, Secret, sha256_hex, write_bytes};

const MODEL: &str = "grok-4.7";

pub async fn decide(
    config: &Config,
    columns: &Value,
    schema_digest: &str,
    identity_hash: &str,
) -> Phase {
    let token = match Secret::load(&config.model_token_file) {
        Ok(token) => token,
        Err(err) => return save_fail(config, &err),
    };
    let prompt = decisions_prompt(columns, schema_digest, false, None);
    let key = cache_key(&prompt, identity_hash, schema_digest, "decisions");
    let first = post_cached(config, &token, &prompt, &key, true).await;
    let first = match first {
        Ok(call) => call,
        Err(err) => return save_fail(config, &config::redact(&err, &[token.expose()])),
    };
    let _ = store_body(config, "decisions-http-body", &first.body, token.expose());
    let mut extracted = chat_content(&first.body).unwrap_or_default();
    let (stripped, mut fence) = strip_one_fence(&extracted);
    extracted = stripped;
    let mut decisions = parse_decisions(&extracted);
    let mut attempts = json!([{
        "socket": first.socket,
        "status": first.status,
        "cache": first.cache,
        "fence": fence,
        "parsed": decisions.is_ok(),
    }]);
    if decisions.is_err() || first.status != 200 {
        let reason = decisions
            .as_ref()
            .err()
            .map(|err| err.to_string())
            .unwrap_or_else(|| format!("status {}", first.status));
        let retry_prompt = decisions_prompt(columns, schema_digest, true, Some(&reason));
        let retry_key = cache_key(
            &retry_prompt,
            identity_hash,
            schema_digest,
            "decisions-retry",
        );
        match post_cached(config, &token, &retry_prompt, &retry_key, true).await {
            Ok(second) => {
                let _ = store_body(
                    config,
                    "decisions-retry-http-body",
                    &second.body,
                    token.expose(),
                );
                if let Ok(content) = chat_content(&second.body) {
                    let (stripped, again) = strip_one_fence(&content);
                    fence = fence || again;
                    decisions = parse_decisions(&stripped);
                }
                attempts.as_array_mut().unwrap().push(json!({
                    "socket": second.socket,
                    "status": second.status,
                    "cache": second.cache,
                    "parsed": decisions.is_ok(),
                }));
            }
            Err(err) => {
                attempts
                    .as_array_mut()
                    .unwrap()
                    .push(json!({ "error": config::redact(&err, &[token.expose()]) }));
            }
        }
    }
    let second = post_cached(config, &token, &prompt, &key, false).await;
    let cache_hit = match &second {
        Ok(call) => call.socket == "not_opened" && call.cache == "hit" && call.body == first.body,
        Err(_) => false,
    };
    let description = describe(config, &token, identity_hash, schema_digest).await;
    let phase = Phase::new(
        "model",
        decisions.is_ok() && first.status == 200 && cache_hit,
        json!({
            "configured_origin": config.model_origin,
            "dial_host": config.model_dial_host,
            "attempts": attempts,
            "fence_stripped": fence,
            "decisions": decisions.as_ref().ok(),
            "decisions_error": decisions.as_ref().err().map(|err| config::redact(err, &[token.expose()])),
            "cache_hit_socket": second.as_ref().map(|call| call.socket).unwrap_or("error"),
            "cache_hit": cache_hit,
            "description": description,
            "declared_cost": { "status": "unknown" },
        }),
    );
    let _ = phase.save(&config.evidence);
    if let Ok(value) = &decisions {
        let _ = config::write_json(&config.evidence.join("decisions.json"), value);
    }
    phase
}

pub async fn negative(config: &Config) -> Phase {
    let unauthorized = post(
        config,
        "invalid",
        "Reply with the single word pong.",
        Duration::from_secs(20),
    )
    .await;
    let timeout_probe = hang().await;
    let status = unauthorized.as_ref().map(|call| call.status).unwrap_or(0);
    let ok = status == 401 && timeout_probe.get("kind").and_then(Value::as_str) == Some("timeout");
    let phase = Phase::new(
        "model-negative",
        ok,
        json!({
            "unauthorized_status": status,
            "unauthorized_error": unauthorized.as_ref().err().map(|err| config::redact(err, &[])),
            "timeout": timeout_probe,
            "declared_cost": { "status": "unknown" },
        }),
    );
    let _ = phase.save(&config.evidence);
    phase
}

async fn describe(
    config: &Config,
    token: &Secret,
    identity_hash: &str,
    schema_digest: &str,
) -> Value {
    let schemas = catalog_schema_label(config);
    let prompt = format!(
        "Write one short paragraph naming the logical tables in schema {schemas}. Do not include SQL or sample cell values."
    );
    let key = cache_key(&prompt, identity_hash, schema_digest, "description");
    match post_cached(config, token, &prompt, &key, true).await {
        Ok(call) => {
            let redacted =
                store_body(config, "description-http-body", &call.body, token.expose()).is_err();
            json!({
                "socket": call.socket,
                "status": call.status,
                "cache": call.cache,
                "body_sha256": sha256_hex(&call.body),
                "token_redacted": redacted,
            })
        }
        Err(err) => json!({ "error": config::redact(&err, &[token.expose()]) }),
    }
}

struct Call {
    status: u16,
    body: Vec<u8>,
    socket: &'static str,
    cache: &'static str,
}

async fn post_cached(
    config: &Config,
    token: &Secret,
    prompt: &str,
    key: &str,
    open_socket: bool,
) -> Result<Call, String> {
    let cache_path = config
        .evidence
        .join("model-cache")
        .join(format!("{key}.body"));
    if !open_socket {
        let body = std::fs::read(&cache_path).map_err(|_| "cache miss".to_string())?;
        return Ok(Call {
            status: 200,
            body,
            socket: "not_opened",
            cache: "hit",
        });
    }
    let call = post(config, token.expose(), prompt, Duration::from_secs(60)).await?;
    if call.status == 200 {
        let _ = write_bytes(&cache_path, &call.body);
    }
    Ok(Call {
        status: call.status,
        body: call.body,
        socket: "opened",
        cache: "miss",
    })
}

async fn post(
    config: &Config,
    bearer: &str,
    prompt: &str,
    limit: Duration,
) -> Result<RawCall, String> {
    let payload = serde_json::to_vec(&json!({
        "model": MODEL,
        "temperature": 0,
        "messages": [{ "role": "user", "content": prompt }],
    }))
    .map_err(|err| err.to_string())?;
    timeout(limit, post_tcp(config, bearer, &payload))
        .await
        .map_err(|_| "timeout".to_string())?
}

struct RawCall {
    status: u16,
    body: Vec<u8>,
}

async fn post_tcp(config: &Config, bearer: &str, payload: &[u8]) -> Result<RawCall, String> {
    let mut stream = TcpStream::connect((config.model_dial_host.as_str(), config.model_port))
        .await
        .map_err(|err| format!("connect {err}"))?;
    let head = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        config.model_path,
        config.model_dial_host,
        bearer,
        payload.len()
    );
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(|err| err.to_string())?;
    stream
        .write_all(payload)
        .await
        .map_err(|err| err.to_string())?;
    let mut raw = Vec::new();
    loop {
        let mut buf = [0u8; 8192];
        let count = stream.read(&mut buf).await.map_err(|err| err.to_string())?;
        if count == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..count]);
        if raw.len() > 2_000_000 {
            return Err("response cap".to_string());
        }
    }
    parse_http(&raw)
}

fn parse_http(raw: &[u8]) -> Result<RawCall, String> {
    let split = raw
        .windows(4)
        .position(|mark| mark == b"\r\n\r\n")
        .ok_or("http head")?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| "http head utf8")?;
    let mut lines = head.lines();
    let status_line = lines.next().ok_or("http status")?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|text| text.parse().ok())
        .ok_or("http status")?;
    let mut length = None;
    let mut chunked = false;
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            length = value.trim().parse().ok();
        }
        if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
            chunked = true;
        }
    }
    let rest = &raw[split + 4..];
    let body = if chunked {
        decode_chunks(rest)?
    } else if let Some(length) = length {
        rest.get(..length).ok_or("http body short")?.to_vec()
    } else {
        rest.to_vec()
    };
    if body.len() > 2_000_000 {
        return Err("response cap".to_string());
    }
    Ok(RawCall { status, body })
}

fn decode_chunks(mut rest: &[u8]) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    loop {
        let line_end = rest
            .windows(2)
            .position(|mark| mark == b"\r\n")
            .ok_or("chunk size")?;
        let size_text = std::str::from_utf8(&rest[..line_end]).map_err(|_| "chunk size")?;
        let size = usize::from_str_radix(size_text.trim().split(';').next().unwrap_or(""), 16)
            .map_err(|_| "chunk size")?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            break;
        }
        if rest.len() < size + 2 {
            return Err("chunk short".to_string());
        }
        body.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
        if body.len() > 2_000_000 {
            return Err("response cap".to_string());
        }
    }
    Ok(body)
}

async fn hang() -> Value {
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(err) => return json!({ "kind": "bind", "fact": err.to_string() }),
    };
    let port = match listener.local_addr() {
        Ok(addr) => addr.port(),
        Err(err) => return json!({ "kind": "bind", "fact": err.to_string() }),
    };
    let accepted = tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 2048];
            let _ = socket.read(&mut buf).await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
    let result = timeout(Duration::from_millis(800), async {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.map_err(|err| err.to_string())?;
        let payload = b"{}";
        let head = format!(
            "POST /hang HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        stream.write_all(head.as_bytes()).await.map_err(|err| err.to_string())?;
        stream.write_all(payload).await.map_err(|err| err.to_string())?;
        let mut buf = [0u8; 64];
        let _ = stream.read(&mut buf).await.map_err(|err| err.to_string())?;
        Ok::<(), String>(())
    })
    .await;
    accepted.abort();
    match result {
        Err(_) => json!({ "kind": "timeout" }),
        Ok(Err(err)) if err.contains("refused") => {
            json!({ "kind": "connection_refused", "fact": err })
        }
        Ok(Err(err)) => json!({ "kind": "timeout", "fact": err }),
        Ok(Ok(())) => json!({ "kind": "responded" }),
    }
}

fn catalog_schema_label(config: &Config) -> String {
    match config.selection.as_ref() {
        Some(selection) => {
            let names = selection
                .requested
                .iter()
                .map(|object| object.schema.clone())
                .collect::<Vec<_>>();
            let label = config::schema_label(&names);
            if label.is_empty() {
                config::SCHEMA.to_string()
            } else {
                label
            }
        }
        None => config::SCHEMA.to_string(),
    }
}

fn cache_key(prompt: &str, identity_hash: &str, schema_digest: &str, role: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prompt.as_bytes());
    hasher.update(b"\n");
    hasher.update(identity_hash.as_bytes());
    hasher.update(b"\n");
    hasher.update(schema_digest.as_bytes());
    hasher.update(b"\n");
    hasher.update(role.as_bytes());
    sha256_hex(&hasher.finalize())
}

pub fn strip_one_fence(text: &str) -> (String, bool) {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return (trimmed.to_string(), false);
    };
    let rest = rest
        .strip_prefix("json")
        .unwrap_or(rest)
        .trim_start_matches('\n');
    let Some(end) = rest.rfind("```") else {
        return (trimmed.to_string(), false);
    };
    (rest[..end].trim().to_string(), true)
}

fn parse_decisions(text: &str) -> Result<Value, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|err| format!("decisions json: {err}"))?;
    if value.is_array() {
        return Ok(value);
    }
    value
        .get("decisions")
        .filter(|item| item.is_array())
        .cloned()
        .ok_or_else(|| "decisions shape".to_string())
}

fn chat_content(body: &[u8]) -> Result<String, String> {
    let value: Value = serde_json::from_slice(body).map_err(|err| err.to_string())?;
    value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "content".to_string())
}

fn decisions_prompt(
    columns: &Value,
    schema_digest: &str,
    stricter: bool,
    previous: Option<&str>,
) -> String {
    let mut text = format!(
        "Return one JSON array of classification decisions for these columns. Echo schema_hash {schema_digest} and each fact_fingerprint exactly. Keys on every object are exactly dataset, column, schema_hash, fact_fingerprint, pii, semantic_type, reviewer. reviewer is an object with by and at. by is w39.operator. at is 2026-10-10. pii is none, personal, sensitive, or restricted. semantic_type is identifier, foreign_key, entity_name, code, status_code, category, boolean_flag, money, quantity, ratio, event_time, date, audit_time, geo, address, phone, email, person_name, free_text, json, or system. If semantic_type is money, include unit USD. Never use the unit string unknown. Do not include cell values.\n"
    );
    if stricter {
        text.push_str("The previous response was rejected. Return only the JSON array, one object per column, with those keys only.\n");
        if let Some(previous) = previous {
            text.push_str("rejection: ");
            text.push_str(&previous.chars().take(180).collect::<String>());
            text.push('\n');
        }
    }
    if let Some(rows) = columns.as_array() {
        for row in rows {
            let dataset = row.get("dataset").and_then(Value::as_str).unwrap_or("");
            let column = row.get("column").and_then(Value::as_str).unwrap_or("");
            let fingerprint = row
                .get("fact_fingerprint")
                .and_then(Value::as_str)
                .unwrap_or("");
            let data_type = row.get("data_type").and_then(Value::as_str).unwrap_or("");
            let nullable = row.get("nullable").and_then(Value::as_bool).unwrap_or(true);
            let key = row.get("key").and_then(Value::as_bool).unwrap_or(false);
            text.push_str(&format!(
                "{dataset}.{column} type={data_type} nullable={nullable} key={key} fact_fingerprint={fingerprint}\n"
            ));
        }
    }
    text
}

fn store_body(config: &Config, name: &str, body: &[u8], token: &str) -> Result<(), String> {
    if !token.is_empty()
        && body
            .windows(token.len())
            .any(|window| window == token.as_bytes())
    {
        let text = String::from_utf8_lossy(body).replace(token, "[redacted]");
        write_bytes(&config.evidence.join("model").join(name), text.as_bytes())?;
        return Err("redacted".to_string());
    }
    write_bytes(&config.evidence.join("model").join(name), body)
}

fn save_fail(config: &Config, fact: &str) -> Phase {
    let phase = Phase::new("model", false, json!({ "error": fact }));
    let _ = phase.save(&config.evidence);
    phase
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_fence_is_removed_once() {
        let (text, stripped) = strip_one_fence("```json\n[1]\n```");
        assert!(stripped);
        assert_eq!(text, "[1]");
        let (plain, stripped) = strip_one_fence("[1]");
        assert!(!stripped);
        assert_eq!(plain, "[1]");
    }
}
