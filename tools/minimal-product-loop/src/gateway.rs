//! Local HTTP door in front of the existing `query-pg` binary.
//!
//! The binary calls `DedicatedSession`. This module does not parse or execute SQL.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::source::hex;

#[derive(Clone, Debug)]
pub struct QueryPgOutcome {
    pub exit: i32,
    pub http_status: u16,
    pub body: Vec<u8>,
    pub query_id: String,
    pub rows: Vec<Vec<Value>>,
    pub original_sha256: Option<String>,
    pub stderr: String,
}

pub fn map_stdout(sql: &str, stdout: &str, exit_code: i32) -> QueryPgOutcome {
    let query_id = query_id(sql);
    let mut rows = Vec::new();
    let mut summary = None;
    for line in stdout.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match value.get("record").and_then(Value::as_str) {
            Some("row") => {
                if let Some(cells) = value.get("cells").and_then(Value::as_array) {
                    rows.push(cells.iter().map(cell_value).collect());
                }
            }
            Some("summary") => summary = Some(value),
            _ => {}
        }
    }
    let original_sha256 = summary.as_ref().and_then(|item| item.get("original_sha256")).and_then(Value::as_str).map(str::to_owned);
    let stop = summary.as_ref().and_then(|item| item.get("stop")).and_then(Value::as_str);
    let status = summary.as_ref().and_then(|item| item.get("status")).and_then(Value::as_str);
    if exit_code == 0 && status == Some("succeeded") {
        let body = success_body(&query_id, &rows, summary.as_ref());
        return QueryPgOutcome { exit: exit_code, http_status: 200, body, query_id, rows, original_sha256, stderr: String::new() };
    }
    let (code, http_status) = match exit_code {
        6 => ("STATEMENT_NOT_ALLOWED", 400),
        5 => ("SCHEMA_INVALID", 400),
        7 => ("AUDIT_UNAVAILABLE", 503),
        4 => ("UNAUTHENTICATED", 401),
        _ if stop == Some("timeout") || stop == Some("canceled") => ("TIMEOUT", 408),
        _ => ("INTERNAL", 500),
    };
    let mut outcome = refused(sql, exit_code, code, http_status);
    outcome.rows = rows;
    outcome.original_sha256 = original_sha256;
    outcome
}

fn refused(sql: &str, exit_code: i32, code: &str, http_status: u16) -> QueryPgOutcome {
    let query_id = query_id(sql);
    let retryable = matches!(code, "AUDIT_UNAVAILABLE" | "INTERNAL");
    let body = serde_json::to_vec(&json!({
        "error": {
            "code": code,
            "message": "the gateway refused the statement",
            "hint": "the refusal is the query-pg exit",
            "retryable": retryable,
            "query_id": query_id
        }
    }))
    .unwrap_or_default();
    QueryPgOutcome {
        exit: exit_code,
        http_status,
        body,
        query_id,
        rows: Vec::new(),
        original_sha256: None,
        stderr: String::new(),
    }
}

fn success_body(query_id: &str, rows: &[Vec<Value>], summary: Option<&Value>) -> Vec<u8> {
    let width = rows.first().map(Vec::len).unwrap_or(0);
    let columns: Vec<Value> = (0..width)
        .map(|index| {
            json!({
                "name": format!("c{}", index + 1),
                "type": "text"
            })
        })
        .collect();
    let text_rows: Vec<Vec<Value>> = rows
        .iter()
        .map(|row| row.iter().map(cell_text).collect())
        .collect();
    let truncated = summary.and_then(|item| item.get("truncated")).and_then(Value::as_bool).unwrap_or(false);
    serde_json::to_vec(&json!({
        "query_id": query_id,
        "columns": columns,
        "rows": text_rows,
        "returned_rows": text_rows.len(),
        "truncated": truncated,
        "duration_ms": 0,
        "freshness": [],
        "sources": [],
        "warnings": [],
        "knowledge_version": "draft-unreviewed",
        "policy_version": "development-preview"
    }))
    .unwrap_or_default()
}

fn cell_value(cell: &Value) -> Value {
    match cell.get("kind").and_then(Value::as_str) {
        Some("null") => Value::Null,
        Some("bool") => cell.get("value").cloned().unwrap_or(Value::Null),
        Some("int") | Some("text") => cell.get("value").cloned().unwrap_or(Value::Null),
        Some("decimal") => Value::String(decimal_text(cell)),
        _ => Value::Null,
    }
}

fn cell_text(cell: &Value) -> Value {
    match cell {
        Value::Null => Value::Null,
        Value::Bool(value) => Value::Bool(*value),
        Value::String(value) => Value::String(value.clone()),
        other => Value::String(other.to_string()),
    }
}

fn decimal_text(cell: &Value) -> String {
    let negative = cell.get("negative").and_then(Value::as_bool).unwrap_or(false);
    let coefficient = cell.get("coefficient").and_then(Value::as_str).unwrap_or("0");
    let scale = cell.get("scale").and_then(Value::as_i64).unwrap_or(0);
    if scale <= 0 {
        return if negative { format!("-{coefficient}") } else { coefficient.to_owned() };
    }
    let scale = scale as usize;
    let digits = if coefficient.len() > scale {
        coefficient.to_owned()
    } else {
        format!("{:0>width$}", coefficient, width = scale + 1)
    };
    let split = digits.len() - scale;
    let text = format!("{}.{}", &digits[..split], &digits[split..]);
    if negative { format!("-{text}") } else { text }
}

pub fn query_id(sql: &str) -> String {
    let digest = hex(sql.as_bytes());
    format!("q_{}", &digest[..8])
}

fn execution_id() -> Result<String, &'static str> {
    let mut random = [0_u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(|_| "query.id")?;
    Ok(format!("q_{}", random.iter().map(|byte| format!("{byte:02x}")).collect::<String>()))
}

pub fn run_query_pg(binary: &Path, request_dir: &Path, policy: &Value, sql: &str, timeout_ms: u64) -> Result<QueryPgOutcome, &'static str> {
    if !binary.is_file() {
        return Err("query_pg.absent");
    }
    fs::create_dir_all(request_dir).map_err(|_| "request")?;
    let execution_id = execution_id()?;
    let audit = request_dir.join(format!("{execution_id}.audit.jsonl"));
    if audit.exists() {
        return Err("audit.exists");
    }
    let mut document = policy.clone();
    let object = document.as_object_mut().ok_or("policy")?;
    object.insert("version".to_owned(), json!("oo.gateway.query-pg/v1"));
    object.insert("audit_file".to_owned(), json!(audit.display().to_string()));
    object.insert(
        "statement".to_owned(),
        json!({
            "source": "request",
            "sql": sql,
            "max_rows": 20,
            "max_result_bytes": 8192,
            "wall_clock_ms": timeout_ms
        }),
    );
    let request_path = request_dir.join(format!("{execution_id}.json"));
    let bytes = serde_json::to_vec(&document).map_err(|_| "request")?;
    fs::write(&request_path, &bytes).map_err(|_| "request")?;
    let mut child = Command::new(binary)
        .arg("--request")
        .arg(&request_path)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "spawn")?;
    let started = Instant::now();
    let limit = Duration::from_millis(timeout_ms.saturating_add(2_000));
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|_| "spawn")? {
            break status;
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            let mut mapped = refused(sql, 8, "TIMEOUT", 408);
            mapped.stderr = "timeout".to_owned();
            return Ok(mapped);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout {
        let _ = pipe.read_to_string(&mut stdout);
    }
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr {
        let _ = pipe.read_to_string(&mut stderr);
    }
    let code = status.code().unwrap_or(1);
    let mut mapped = map_stdout(sql, &stdout, code);
    // This receipt binds the unique execution to the exact native request and
    // native audit. query-pg checks policies against the local mapping catalog.
    let context = json!({
        "protocol": "oo.minimal-product-loop.query-context/v1",
        "query_id": execution_id,
        "database": policy.get("endpoint").and_then(|value| value.get("database")),
        "host": policy.get("endpoint").and_then(|value| value.get("host")),
        "generation": policy.get("policies").and_then(Value::as_array).and_then(|rows| rows.first()).and_then(|row| row.get("generation")),
        "policy_id": policy.get("identity").and_then(|value| value.get("policy_id")),
        "request_sha256": hex(&bytes),
        "audit_sha256": fs::read(&audit).ok().map(|bytes| hex(&bytes)),
        "sql_sha256": hex(sql.as_bytes()),
        "native_exit": code,
        "sealed_import": request_dir.parent().and_then(|parent| fs::read(parent.join("query-binding.json")).ok()).and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    });
    fs::write(request_dir.join(format!("{execution_id}.context.json")), serde_json::to_vec_pretty(&context).map_err(|_| "audit.context")?).map_err(|_| "audit.context")?;
    mapped.query_id = execution_id.clone();
    if let Ok(mut body) = serde_json::from_slice::<Value>(&mapped.body) {
        if let Some(error) = body.get_mut("error") {
            error["query_id"] = json!(execution_id);
        } else {
            body["query_id"] = json!(execution_id);
        }
        mapped.body = serde_json::to_vec(&body).map_err(|_| "response")?;
    }
    if stderr.starts_with("query-pg: ") && stderr.len() < 80 {
        mapped.stderr = stderr.trim().to_owned();
    }
    Ok(mapped)
}

#[derive(Clone, Debug)]
pub struct CapturedQuery {
    pub sql: String,
    pub query_id: String,
    pub http_status: u16,
    pub exit: i32,
    pub rows: Vec<Vec<Value>>,
}

#[cfg(test)]
pub(crate) fn serve(
    listener: std::net::TcpListener,
    binary: std::path::PathBuf,
    policy: Value,
    token: String,
    request_dir: std::path::PathBuf,
    captured: std::sync::Arc<std::sync::Mutex<Vec<CapturedQuery>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let _ = listener.set_nonblocking(true);
    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = handle(&mut stream, &binary, &policy, &token, &request_dir, &captured);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }
}

pub(crate) fn handle<S: Read + Write>(
    stream: &mut S,
    binary: &Path,
    policy: &Value,
    token: &str,
    request_dir: &Path,
    captured: &std::sync::Mutex<Vec<CapturedQuery>>,
) -> Result<(), ()> {
    let buffer = read_message(stream, 96 * 1024)?;
    let end = buffer.windows(4).position(|window| window == b"\r\n\r\n").ok_or(())?;
    let head = std::str::from_utf8(&buffer[..end]).map_err(|_| ())?;
    let need = content_length(head).unwrap_or(0);
    let body_end = end.saturating_add(4).saturating_add(need).min(buffer.len());
    let body = std::str::from_utf8(&buffer[end + 4..body_end]).map_err(|_| ())?;
    if !head.starts_with("POST /v1/query ") {
        return write_raw(stream, 404, b"{\"error\":{\"code\":\"SCHEMA_INVALID\",\"message\":\"the gateway refused the statement\",\"hint\":\"path\",\"retryable\":false}}");
    }
    if !authorized(head, token) {
        return write_raw(stream, 401, b"{\"error\":{\"code\":\"UNAUTHENTICATED\",\"message\":\"the gateway refused the statement\",\"hint\":\"token file\",\"retryable\":false}}");
    }
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        let outcome = refused("empty", 5, "SCHEMA_INVALID", 400);
        return write_raw(stream, outcome.http_status, &outcome.body);
    };
    let Some(sql) = value.get("sql").and_then(Value::as_str).filter(|sql| !sql.is_empty() && sql.len() <= 65_536) else {
        let outcome = refused("empty", 5, "SCHEMA_INVALID", 400);
        return write_raw(stream, outcome.http_status, &outcome.body);
    };
    let timeout = value.get("timeout_ms").and_then(Value::as_u64).unwrap_or(30_000).clamp(1, 120_000);
    let outcome = match run_query_pg(binary, request_dir, policy, sql, timeout) {
        Ok(outcome) => {
            if let Ok(mut guard) = captured.lock() {
                guard.push(CapturedQuery {
                    sql: sql.to_owned(),
                    query_id: outcome.query_id.clone(),
                    http_status: outcome.http_status,
                    exit: outcome.exit,
                    rows: outcome.rows.clone(),
                });
            }
            outcome
        }
        Err("query_pg.absent" | "audit.exists") => {
            let outcome = refused(sql, 5, "SCHEMA_INVALID", 400);
            return write_raw(stream, outcome.http_status, &outcome.body);
        }
        Err(_) => {
            let outcome = refused(sql, 1, "INTERNAL", 500);
            return write_raw(stream, outcome.http_status, &outcome.body);
        }
    };
    write_raw(stream, outcome.http_status, &outcome.body)
}

pub(crate) fn read_message<S: Read>(stream: &mut S, cap: usize) -> Result<Vec<u8>, ()> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        if complete(&buf) {
            break;
        }
        if buf.len() >= cap {
            return Err(());
        }
        let read = stream.read(&mut tmp).map_err(|_| ())?;
        if read == 0 {
            break;
        }
        let room = cap - buf.len();
        if read > room {
            return Err(());
        }
        buf.extend_from_slice(&tmp[..read]);
    }
    if complete(&buf) { Ok(buf) } else { Err(()) }
}

fn complete(buf: &[u8]) -> bool {
    let Some(end) = buf.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let head = std::str::from_utf8(&buf[..end]).unwrap_or("");
    let need = content_length(head).unwrap_or(0);
    buf.len() >= end + 4 + need
}

fn content_length(head: &str) -> Option<usize> {
    for line in head.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().ok();
        }
    }
    None
}

fn authorized(head: &str, token: &str) -> bool {
    let expected = format!("Bearer {token}");
    head.lines().any(|line| {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        name.eq_ignore_ascii_case("authorization") && value.trim() == expected
    })
}

pub(crate) fn write_raw<S: Write>(stream: &mut S, status: u16, body: &[u8]) -> Result<(), ()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        408 => "Request Timeout",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let header = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    stream.write_all(header.as_bytes()).map_err(|_| ())?;
    stream.write_all(body).map_err(|_| ())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{map_stdout, refused, serve};
    use serde_json::Value;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn timeout_refusal_is_not_retryable() {
        let stdout = "{\"record\":\"summary\",\"status\":\"failed\",\"stop\":\"timeout\",\"truncated\":false}\n";
        let outcome = map_stdout("shop.entry", stdout, 8);
        assert_eq!(outcome.http_status, 408);
        let body: Value = serde_json::from_slice(&outcome.body).expect("json");
        assert_eq!(body["error"]["code"], "TIMEOUT");
        assert_eq!(body["error"]["retryable"], false);
        assert_eq!(outcome.query_id.len(), 10);
        assert!(outcome.query_id.starts_with("q_"));
        let killed = refused("shop.entry", 8, "TIMEOUT", 408);
        let killed_body: Value = serde_json::from_slice(&killed.body).expect("json");
        assert_eq!(killed.http_status, 408);
        assert_eq!(killed_body["error"]["code"], "TIMEOUT");
        assert_eq!(killed_body["error"]["retryable"], false);
    }

    #[test]
    fn rows_keep_text_bool_and_exact_decimal() {
        let stdout = "\
{\"record\":\"row\",\"turn\":0,\"cells\":[{\"kind\":\"int\",\"value\":\"90000\"},{\"kind\":\"bool\",\"value\":true},{\"kind\":\"decimal\",\"negative\":false,\"coefficient\":\"12345678901234\",\"scale\":4},{\"kind\":\"decimal\",\"negative\":true,\"coefficient\":\"1\",\"scale\":4}]}\n\
{\"record\":\"summary\",\"status\":\"succeeded\",\"truncated\":false,\"original_sha256\":\"abc\"}\n";
        let outcome = map_stdout("shop.entry", stdout, 0);
        assert_eq!(outcome.http_status, 200);
        assert_eq!(outcome.rows[0][0], Value::String("90000".to_owned()));
        assert_eq!(outcome.rows[0][1], Value::Bool(true));
        assert_eq!(outcome.rows[0][2], Value::String("1234567890.1234".to_owned()));
        assert_eq!(outcome.rows[0][3], Value::String("-0.0001".to_owned()));
        let body: Value = serde_json::from_slice(&outcome.body).expect("json");
        assert_eq!(body["returned_rows"], 1);
        assert_eq!(body["rows"][0][1], true);
        assert_eq!(body["columns"][0]["name"], "c1");
        assert_eq!(body["columns"][0]["type"], "text");
        assert_eq!(body["knowledge_version"], "draft-unreviewed");
        assert_eq!(body["policy_version"], "development-preview");
        let guard = map_stdout("shop.entry", "", 6);
        let guard_body: Value = serde_json::from_slice(&guard.body).expect("json");
        assert_eq!(guard.http_status, 400);
        assert_eq!(guard_body["error"]["code"], "STATEMENT_NOT_ALLOWED");
        assert_eq!(guard_body["error"]["retryable"], false);
    }

    #[test]
    fn absent_query_pg_answers_without_a_capture() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let stop = Arc::new(AtomicBool::new(false));
        let captured = Arc::new(Mutex::new(Vec::new()));
        let thread_stop = Arc::clone(&stop);
        let thread_captured = Arc::clone(&captured);
        let handle = thread::spawn(move || {
            serve(
                listener,
                PathBuf::from("/no/such/query-pg"),
                serde_json::json!({}),
                "dev-token".to_owned(),
                PathBuf::from("/tmp/oo-a405-no-query"),
                thread_captured,
                thread_stop,
            );
        });
        let mut client = connect(port);
        write_request(&mut client, port, "Bearer other", b"{}");
        let denied = read_response(&mut client);
        assert!(denied.contains("UNAUTHENTICATED"), "{denied}");
        let mut client = connect(port);
        let body = b"{\"sql\":\"shop.entry\",\"timeout_ms\":1000,\"format\":\"json\"}";
        write_request(&mut client, port, "Bearer dev-token", body);
        let response = read_response(&mut client);
        assert!(response.contains("SCHEMA_INVALID"), "{response}");
        assert!(response.contains("\"retryable\":false"), "{response}");
        assert!(captured.lock().expect("lock").is_empty());
        stop.store(true, Ordering::Relaxed);
        handle.join().expect("join");
    }

    fn connect(port: u16) -> TcpStream {
        for _ in 0..50 {
            if let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) {
                stream.set_read_timeout(Some(Duration::from_secs(2))).expect("timeout");
                return stream;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("listener did not accept");
    }

    fn write_request(stream: &mut TcpStream, port: u16, authorization: &str, body: &[u8]) {
        let head = format!(
            "POST /v1/query HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: {authorization}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).expect("write");
        stream.write_all(body).expect("write");
    }

    fn read_response(stream: &mut TcpStream) -> String {
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        response
    }
}
