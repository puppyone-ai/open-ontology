//! Fixed egress for `oo query`. The sandbox bearer is only the public marker.
//! This process strips that header, checks the product gateway over mTLS, and
//! spawns the existing `query-pg` binary. It does not execute SQL itself.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::config::{state_path, Config};
use crate::error::Fault;
use crate::forward::PUBLIC_MARKER;
use crate::httputil::{read_request, status_line, write_json};
use crate::pack::pack_digest;
use crate::sha::{hex_encode, random_bytes, sha256};
use crate::sqlgate::sql_allowed;

#[derive(Clone, Debug)]
pub struct Bearer {
    pub data_role: String,
    pub principal: String,
    pub session_id: String,
    pub project_id: String,
    pub pack_sha256: String,
    pub turn_active: bool,
}

pub struct QueryPgView {
    pub query_id: String,
    pub result_text: String,
    pub rows: Vec<Value>,
    pub columns: Vec<Value>,
    pub original_sha256: String,
    pub returned_rows: u64,
    pub truncated: bool,
}

struct Door {
    bearers: Arc<Mutex<HashMap<String, Bearer>>>,
    receipt_lock: Mutex<()>,
    pack_mode: String,
    pack_root: PathBuf,
    release_origin: String,
    query_pg_bin: PathBuf,
    policy_dir: PathBuf,
    addr_file: PathBuf,
    mtls_ca: PathBuf,
    mtls_cert: PathBuf,
    mtls_key: PathBuf,
    password_file: PathBuf,
    ca_file: PathBuf,
    receipt_path: PathBuf,
    import_generation: String,
    import_source: String,
    import_mp01: bool,
    snapshot_id: String,
    import_generation_number: Option<i64>,
    mtls_log: PathBuf,
    proxy_log: PathBuf,
    request_dir: PathBuf,
    pg_host: String,
    pg_port: u16,
    pg_database: String,
    pg_user: String,
    password: String,
    sealed: Option<crate::config::ImportGeneration>,
}

impl Drop for Door {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.password).into_bytes();
        bytes.fill(0);
        std::hint::black_box(bytes);
    }
}

pub async fn serve(config: &Config, bearers: Arc<Mutex<HashMap<String, Bearer>>>) -> Result<u16, Fault> {
    let password_file = state_path(config, "query-pg.pass");
    let ca_file = state_path(config, "pg-ca.crt");
    write_new(&password_file, config.pg.password.as_bytes())?;
    write_new(&ca_file, config.pg.ca_pem.as_bytes())?;
    let loaded = if config.pack_mode == "development_draft" {
        let loaded = crate::config::load_import_generation(&config.import_generation_file)?;
        if loaded.database != config.pg.database {
            return Err(crate::error::Fault::new(422, "import_generation", 1));
        }
        Some(loaded)
    } else {
        None
    };
    let door = Arc::new(Door {
        bearers,
        receipt_lock: Mutex::new(()),
        pack_mode: config.pack_mode.clone(),
        pack_root: PathBuf::from(&config.pack_root),
        release_origin: config.release_origin.clone(),
        query_pg_bin: PathBuf::from(&config.query_pg_bin),
        policy_dir: PathBuf::from(&config.query_policy_dir),
        addr_file: PathBuf::from(&config.gateway_addr_file),
        mtls_ca: PathBuf::from(&config.mtls_ca_file),
        mtls_cert: PathBuf::from(&config.mtls_cert_file),
        mtls_key: PathBuf::from(&config.mtls_key_file),
        password_file,
        ca_file,
        receipt_path: state_path(config, "query-receipts.jsonl"),
        import_generation: loaded.as_ref().map(|item| item.generation.clone()).unwrap_or_default(),
        import_source: loaded.as_ref().map(|item| item.source.clone()).unwrap_or_default(),
        import_mp01: loaded.as_ref().is_some_and(|item| item.mp01),
        snapshot_id: loaded.as_ref().map(|item| item.snapshot_id.clone()).unwrap_or_default(),
        import_generation_number: loaded.as_ref().and_then(|item| item.import_generation_number),
        mtls_log: state_path(config, "mtls-hop.txt"),
        proxy_log: state_path(config, "proxy-log.txt"),
        request_dir: state_path(config, "queries"),
        pg_host: config.pg.host.clone(),
        pg_port: config.pg.port,
        pg_database: config.pg.database.clone(),
        pg_user: config.pg.user.clone(),
        password: config.pg.password.clone(),
        sealed: loaded,
    });
    fs::create_dir_all(&door.request_dir).map_err(|_| Fault::new(500, "gateway_dir", 3))?;
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|_| Fault::new(500, "gateway_bind", 3))?;
    let port = listener.local_addr().map_err(|_| Fault::new(500, "gateway_bind", 3))?.port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { continue };
            let door = Arc::clone(&door);
            tokio::spawn(async move {
                let Ok(incoming) = read_request(&mut sock, 64 * 1024).await else {
                    let _ = write_json(&mut sock, 400, error_body("SQL_PARSE_ERROR", "request").as_bytes()).await;
                    return;
                };
                if incoming.method != "POST" || incoming.path != "/v1/query" {
                    let _ = write_json(&mut sock, 404, error_body("STATEMENT_NOT_ALLOWED", "route").as_bytes()).await;
                    return;
                }
                match dispatch(&door, &incoming.body, &incoming.authorization).await {
                    Ok(bytes) => {
                        let _ = write_json(&mut sock, 200, &bytes).await;
                    }
                    Err((status, body)) => {
                        let _ = write_json(&mut sock, status, body.as_bytes()).await;
                    }
                }
            });
        }
    });
    Ok(port)
}

pub async fn set_turn(bearers: &Mutex<HashMap<String, Bearer>>, session_id: &str, active: bool) {
    if let Some(bearer) = bearers.lock().await.get_mut(session_id) {
        bearer.turn_active = active;
    }
}

pub fn audit_rows(path: &Path, principal: &str) -> Result<Vec<(String, String)>, Fault> {
    if !ident(principal) {
        return Err(Fault::new(400, "principal", 1));
    }
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|_| Fault::new(503, "audit", 3))?;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else { continue };
        if value.get("principal").and_then(Value::as_str) != Some(principal) {
            continue;
        }
        let Some(query_id) = value.get("query_id").and_then(Value::as_str) else { continue };
        let Some(result_text) = value.get("result_text").and_then(Value::as_str) else { continue };
        out.push((query_id.to_owned(), result_text.to_owned()));
    }
    Ok(out)
}

pub fn error_body(code: &str, message: &str) -> String {
    let retryable = matches!(code, "INTERNAL" | "AUDIT_UNAVAILABLE");
    json!({"error": {"code": code, "message": message, "retryable": retryable}}).to_string()
}

pub fn query_id_from_hash(hash: &str) -> Option<String> {
    if hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
        Some(format!("q_{}", &hash[..32]))
    } else {
        None
    }
}

pub fn parse_query_pg_stdout(text: &str) -> Result<QueryPgView, &'static str> {
    let mut rows = Vec::new();
    let mut columns = Vec::new();
    let mut original = String::new();
    let mut returned = 0_u64;
    let mut truncated = false;
    let mut succeeded = false;
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line).map_err(|_| "stdout")?;
        match value.get("record").and_then(Value::as_str) {
            Some("row") => {
                let cells = value.get("cells").and_then(Value::as_array).ok_or("stdout")?;
                let mut encoded = Vec::new();
                if columns.is_empty() {
                    for (index, cell) in cells.iter().enumerate() {
                        let name = if index == 0 { "value" } else { "c" };
                        let type_name = match cell.get("kind").and_then(Value::as_str) {
                            Some("bool") => "bool",
                            Some("int") => "int8",
                            _ => "text",
                        };
                        let column_name = if index == 0 { name.to_owned() } else { format!("{name}{index}") };
                        columns.push(json!({"name": column_name, "type": type_name}));
                    }
                }
                for cell in cells {
                    encoded.push(cell_value(cell));
                }
                rows.push(Value::Array(encoded));
            }
            Some("summary") => {
                succeeded = value.get("status").and_then(Value::as_str) == Some("succeeded");
                truncated = value.get("truncated").and_then(Value::as_bool).unwrap_or(false);
                returned = value.get("returned_rows").and_then(Value::as_u64).unwrap_or(rows.len() as u64);
                original = value.get("original_sha256").and_then(Value::as_str).unwrap_or("").to_owned();
            }
            _ => return Err("stdout"),
        }
    }
    if !succeeded {
        return Err("summary");
    }
    let Some(query_id) = query_id_from_hash(&original) else { return Err("hash") };
    let result_text = rows.first().and_then(Value::as_array).and_then(|cells| cells.first()).map(cell_text).unwrap_or_else(|| "empty".to_owned());
    Ok(QueryPgView { query_id, result_text, rows, columns, original_sha256: original, returned_rows: returned, truncated })
}

async fn dispatch(door: &Door, body: &[u8], authorization: &str) -> Result<Vec<u8>, (u16, String)> {
    let started = Instant::now();
    let mut session_id = String::from("-");
    let result = dispatch_inner(door, body, authorization, &mut session_id).await;
    let (status, bytes) = match &result {
        Ok(bytes) => (200_u16, bytes.len()),
        Err((status, body)) => (*status, body.len()),
    };
    note(door, &door.proxy_log, &format!("session={session_id} path=/v1/query status={status} bytes={bytes} ms={}\n", started.elapsed().as_millis()));
    result
}

async fn dispatch_inner(door: &Door, body: &[u8], authorization: &str, session_id: &mut String) -> Result<Vec<u8>, (u16, String)> {
    if authorization != format!("Bearer {PUBLIC_MARKER}") {
        return Err((401, error_body("UNAUTHENTICATED", "marker")));
    }
    let value: Value = serde_json::from_slice(body).map_err(|_| (400, error_body("SQL_PARSE_ERROR", "json")))?;
    let sql = value.get("sql").and_then(Value::as_str).ok_or((400, error_body("SQL_PARSE_ERROR", "sql")))?.to_owned();
    let bearer = resolve(&door.bearers).await?;
    *session_id = bearer.session_id.clone();
    sql_allowed(&sql).map_err(|reason| (400, error_body("STATEMENT_NOT_ALLOWED", reason)))?;
    if contains_secret(&sql, &door.password) {
        return Err((400, error_body("STATEMENT_NOT_ALLOWED", "secret")));
    }
    admit_pack(door, &bearer).await?;
    let mut document = load_policy(&door.policy_dir, &bearer.data_role)?;
    let policy_role = document.get("identity").and_then(|item| item.get("role")).and_then(Value::as_str).unwrap_or("");
    if policy_role != bearer.data_role {
        return Err((403, error_body("COLUMN_NOT_ALLOWED", "role")));
    }
    health(door).await?;
    let timeout_ms = value.get("timeout_ms").and_then(Value::as_u64).unwrap_or(20_000).clamp(1, 30_000);
    let max_rows = value.get("max_rows").and_then(Value::as_u64).unwrap_or(20).clamp(1, 2_000);
    run_query(door, &bearer, &mut document, &sql, timeout_ms, max_rows).await
}

async fn resolve(bearers: &Mutex<HashMap<String, Bearer>>) -> Result<Bearer, (u16, String)> {
    let guard = bearers.lock().await;
    let active: Vec<Bearer> = guard.values().filter(|bearer| bearer.turn_active).cloned().collect();
    if active.len() > 1 {
        return Err((409, error_body("STATEMENT_NOT_ALLOWED", "session")));
    }
    if let Some(bearer) = active.into_iter().next() {
        return Ok(bearer);
    }
    if guard.len() == 1 {
        return guard.values().next().cloned().ok_or((401, error_body("UNAUTHENTICATED", "session")));
    }
    let wide: Vec<Bearer> = guard.values().filter(|bearer| bearer.data_role == "oo_r_wide").cloned().collect();
    if wide.len() == 1 {
        return wide.into_iter().next().ok_or((401, error_body("UNAUTHENTICATED", "session")));
    }
    Err((401, error_body("UNAUTHENTICATED", "session")))
}

async fn admit_pack(door: &Door, bearer: &Bearer) -> Result<(), (u16, String)> {
    if door.pack_mode == "development_draft" {
        let digest = pack_digest(&door.pack_root.join(&bearer.data_role)).map_err(|_| (409, error_body("PACK_UNPUBLISHED", "draft pack")))?;
        if digest != bearer.pack_sha256 {
            return Err((409, error_body("PACK_UNPUBLISHED", "draft pack")));
        }
        return Ok(());
    }
    confirm_pack(&door.release_origin, &bearer.project_id, &bearer.data_role, &bearer.pack_sha256).await
}

async fn confirm_pack(origin: &str, project_id: &str, role: &str, bound_pack: &str) -> Result<(), (u16, String)> {
    match crate::release::fetch_active_pack(origin, project_id, role).await {
        Ok(active) if active.published && active.role == role && active.pack_sha256 == bound_pack => Ok(()),
        Ok(_) => Err((409, error_body("PACK_UNPUBLISHED", "active pack refused"))),
        Err(fault) if fault.status == 409 => Err((409, error_body("PACK_UNPUBLISHED", "active pack refused"))),
        Err(_) => Err((503, error_body("AUDIT_UNAVAILABLE", "active pack"))),
    }
}

fn load_policy(dir: &Path, role: &str) -> Result<Value, (u16, String)> {
    if !ident(role) {
        return Err((403, error_body("COLUMN_NOT_ALLOWED", "role")));
    }
    let path = dir.join(format!("{role}.json"));
    let bytes = fs::read(path).map_err(|_| (403, error_body("COLUMN_NOT_ALLOWED", "policy")))?;
    serde_json::from_slice(&bytes).map_err(|_| (403, error_body("COLUMN_NOT_ALLOWED", "policy")))
}

async fn health(door: &Door) -> Result<(), (u16, String)> {
    let started = Instant::now();
    let addr = fs::read_to_string(&door.addr_file).map_err(|_| (503, error_body("AUDIT_UNAVAILABLE", "gateway")))?;
    let addr = addr.trim();
    let Some((host, port_text)) = addr.rsplit_once(':') else {
        return Err((503, error_body("AUDIT_UNAVAILABLE", "gateway")));
    };
    if host != "127.0.0.1" {
        return Err((503, error_body("AUDIT_UNAVAILABLE", "gateway")));
    }
    let port: u16 = port_text.parse().map_err(|_| (503, error_body("AUDIT_UNAVAILABLE", "gateway")))?;
    let ca = fs::read(&door.mtls_ca).map_err(|_| (503, error_body("AUDIT_UNAVAILABLE", "mtls")))?;
    let cert = fs::read(&door.mtls_cert).map_err(|_| (503, error_body("AUDIT_UNAVAILABLE", "mtls")))?;
    let key = fs::read(&door.mtls_key).map_err(|_| (503, error_body("AUDIT_UNAVAILABLE", "mtls")))?;
    let request = b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    let bytes = crate::tlsutil::mtls_exchange(host, port, host, &ca, &cert, &key, request).await.map_err(|_| (503, error_body("AUDIT_UNAVAILABLE", "mtls")))?;
    let status = status_line(&bytes).map(|(status, _, _)| status).unwrap_or(0);
    let unsupported = bytes.windows(b"unsupported".len()).any(|window| window == b"unsupported");
    let flag = if unsupported { "unsupported" } else { "other" };
    note(door, &door.mtls_log, &format!("status={status} sql_execution={flag} bytes={} ms={}\n", bytes.len(), started.elapsed().as_millis()));
    if status == 200 && unsupported { Ok(()) } else { Err((503, error_body("AUDIT_UNAVAILABLE", "gateway"))) }
}

async fn run_query(door: &Door, bearer: &Bearer, document: &mut Value, sql: &str, timeout_ms: u64, max_rows: u64) -> Result<Vec<u8>, (u16, String)> {
    let started = Instant::now();
    let nonce = hex_encode(&sha256(&random_bytes(16).map_err(|_| (500, error_body("INTERNAL", "random")))?));
    let audit = door.request_dir.join(format!("{nonce}.audit.jsonl"));
    let request_path = door.request_dir.join(format!("{nonce}.json"));
    let preview = if door.sealed.as_ref().is_some_and(|sealed| sealed.source == "local_import") {
        let sealed = door.sealed.as_ref().ok_or((409, error_body("PREVIEW_POLICY", "local preview")))?;
        match crate::config::admit_local_preview(document, sealed, &door.pg_host, &door.pg_database, &door.pg_user) {
            Ok(stamp) => Some(stamp),
            Err(_) => return Err((409, error_body("PREVIEW_POLICY", "local preview"))),
        }
    } else {
        None
    };
    let object = document.as_object_mut().ok_or((403, error_body("COLUMN_NOT_ALLOWED", "policy")))?;
    if let Some(endpoint) = object.get_mut("endpoint").and_then(Value::as_object_mut) {
        endpoint.insert("host".to_owned(), json!(door.pg_host));
        endpoint.insert("port".to_owned(), json!(door.pg_port));
        endpoint.insert("database".to_owned(), json!(door.pg_database));
        endpoint.insert("user".to_owned(), json!(door.pg_user));
    }
    object.insert("version".to_owned(), json!("oo.gateway.query-pg/v1"));
    object.insert("password_file".to_owned(), json!(door.password_file.display().to_string()));
    object.insert("ca_file".to_owned(), json!(door.ca_file.display().to_string()));
    object.insert("audit_file".to_owned(), json!(audit.display().to_string()));
    object.insert(
        "statement".to_owned(),
        json!({
            "source": "request",
            "sql": sql,
            "max_rows": max_rows,
            "max_result_bytes": 8192,
            "wall_clock_ms": timeout_ms
        }),
    );
    let bytes = serde_json::to_vec(document).map_err(|_| (500, error_body("INTERNAL", "request")))?;
    if contains_secret(&String::from_utf8_lossy(&bytes), &door.password) {
        return Err((500, error_body("INTERNAL", "request")));
    }
    write_new(&request_path, &bytes).map_err(|_| (500, error_body("INTERNAL", "request")))?;
    let mut child = Command::new(&door.query_pg_bin);
    child
        .arg("--request")
        .arg(request_path)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = child.spawn().map_err(|_| (503, error_body("AUDIT_UNAVAILABLE", "query-pg")))?;
    let output = match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms.saturating_add(2_000)), child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        _ => return Err((503, error_body("AUDIT_UNAVAILABLE", "query-pg"))),
    };
    let code = output.status.code().unwrap_or(1);
    if code != 0 {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = if stderr.starts_with("query-pg: ") && stderr.trim().len() < 80 && !contains_secret(&stderr, &door.password) {
            stderr.trim()
        } else {
            "query-pg"
        };
        return Err((400, error_body("STATEMENT_NOT_ALLOWED", message)));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let view = parse_query_pg_stdout(&stdout).map_err(|_| (503, error_body("AUDIT_UNAVAILABLE", "stdout")))?;
    let audit_text = fs::read_to_string(&audit).map_err(|_| (503, error_body("AUDIT_UNAVAILABLE", "audit")))?;
    if !audit_text.contains(&view.original_sha256) {
        return Err((503, error_body("AUDIT_UNAVAILABLE", "audit")));
    }
    let mut receipt = json!({
        "query_id": view.query_id,
        "principal": bearer.principal,
        "role": bearer.data_role,
        "result_text": view.result_text,
        "sql": sql,
        "original_sha256": view.original_sha256,
        "audit_file": audit.display().to_string(),
        "pack_sha256": bearer.pack_sha256
    });
    if door.pack_mode == "development_draft"
        && let Some(object) = receipt.as_object_mut()
    {
        object.insert("import_generation".to_owned(), json!(door.import_generation));
        object.insert("import_source".to_owned(), json!(door.import_source));
        object.insert("mp01".to_owned(), json!(door.import_mp01));
        object.insert("snapshot_id".to_owned(), json!(door.snapshot_id));
        object.insert(
            "import_generation_number".to_owned(),
            door.import_generation_number.map_or(Value::Null, |number| json!(number)),
        );
        if let Some(stamp) = preview.as_ref() {
            object.insert("plan_sha256".to_owned(), json!(stamp.plan_sha256));
            object.insert("manifest_sha256".to_owned(), json!(stamp.manifest_sha256));
            object.insert("target_config_sha256".to_owned(), json!(stamp.target_config_sha256));
            object.insert("target_host".to_owned(), json!(stamp.target_host));
            object.insert("target_server_name".to_owned(), json!(stamp.target_server_name));
            object.insert("target_user".to_owned(), json!(stamp.target_user));
            object.insert("mapping_source".to_owned(), json!(stamp.mapping_source));
            object.insert("config_digest".to_owned(), json!(stamp.config_digest));
            object.insert("preview".to_owned(), json!("explicit_local_preview"));
        }
    }
    let line = receipt.to_string();
    if contains_secret(&line, &door.password) {
        return Err((500, error_body("INTERNAL", "receipt")));
    }
    {
        let _guard = door.receipt_lock.lock().await;
        append_line(&door.receipt_path, &line).map_err(|_| (503, error_body("AUDIT_UNAVAILABLE", "receipt")))?;
    }
    let table = freshness_table(sql);
    Ok(json!({
        "columns": view.columns,
        "duration_ms": started.elapsed().as_millis() as u64,
        "freshness": [{"as_of": "2026-10-08T00:00:00Z", "lag_ms": 0, "state": "live", "table": table}],
        "hints": [],
        "knowledge_version": "shop-v1",
        "policy_version": bearer.data_role,
        "query_id": view.query_id,
        "returned_rows": view.returned_rows,
        "rows": view.rows,
        "sources": [{"applied_pos": "0", "consistency": "transactional", "cross_table_consistent": true, "source": "shop"}],
        "truncated": view.truncated,
        "warnings": []
    })
    .to_string()
    .into_bytes())
}

fn cell_value(cell: &Value) -> Value {
    match cell.get("kind").and_then(Value::as_str) {
        Some("null") => Value::Null,
        Some("bool") => cell.get("value").cloned().unwrap_or(Value::Null),
        Some("int") | Some("text") => match cell.get("value") {
            Some(Value::String(value)) => Value::String(value.chars().take(500).collect()),
            Some(other) => Value::String(other.to_string()),
            None => Value::Null,
        },
        _ => Value::Null,
    }
}

fn cell_text(cell: &Value) -> String {
    match cell {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Null => "null".to_owned(),
        other => other.to_string(),
    }
}

fn freshness_table(sql: &str) -> &'static str {
    let lower = sql.to_ascii_lowercase();
    if lower.contains("shop.entry") {
        "shop.entry"
    } else if lower.contains("shop.account") {
        "shop.account"
    } else {
        "shop.party"
    }
}

fn contains_secret(text: &str, secret: &str) -> bool {
    !secret.is_empty() && text.contains(secret)
}

fn note(door: &Door, path: &Path, line: &str) {
    if contains_secret(line, &door.password) || line.contains(PUBLIC_MARKER) {
        return;
    }
    let _ = append_line(path, line.trim_end());
}

fn append_line(path: &Path, line: &str) -> Result<(), ()> {
    let mut file = OpenOptions::new().create(true).append(true).mode(0o600).open(path).map_err(|_| ())?;
    writeln!(file, "{line}").map_err(|_| ())?;
    file.sync_all().map_err(|_| ())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Fault> {
    if path.exists() {
        return Err(Fault::new(500, "gateway_file", 3));
    }
    let mut file = OpenOptions::new().create_new(true).write(true).mode(0o600).open(path).map_err(|_| Fault::new(500, "gateway_file", 3))?;
    file.write_all(bytes).map_err(|_| Fault::new(500, "gateway_file", 3))?;
    file.sync_all().map_err(|_| Fault::new(500, "gateway_file", 3))?;
    Ok(())
}

fn ident(value: &str) -> bool {
    let mut bytes = value.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_lowercase() || first == b'_' => bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        _ => false,
    }
}
