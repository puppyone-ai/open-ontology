//! HTTP/1.1 on 127.0.0.1. One request per connection. No axum.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use session_transition::is_token;

use crate::close;
use crate::config::{self, Binding, Config, CONTRACT, DEPLOYMENT, PRODUCTION_SIGNED};
use crate::error::Fault;
use crate::lifecycle::{self, endpoint_of, load_head, phase_label, runtime_db};
use crate::store::{self, HttpRow};

struct Incoming {
    method: String,
    path: String,
    query: String,
    cookie: String,
    csrf: String,
    body: Vec<u8>,
}

pub async fn serve(cfg: Config) -> Result<(), Fault> {
    if config::startup_gate(&cfg.profile).is_err() {
        return Err(Fault::Forbidden("profile"));
    }
    if cfg.bind != "127.0.0.1:0" {
        return Err(Fault::Malformed("bind"));
    }
    let listener = TcpListener::bind(&cfg.bind).await.map_err(|_| Fault::Unavailable("bind"))?;
    let port = listener.local_addr().map_err(|_| Fault::Unavailable("bind"))?.port();
    fs::create_dir_all(&cfg.state_dir).map_err(|_| Fault::Unavailable("port"))?;
    let port_path = Path::new(&cfg.state_dir).join("port.txt");
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o644)
        .open(&port_path)
        .map_err(|_| Fault::Unavailable("port"))?;
    writeln!(file, "{port}").map_err(|_| Fault::Unavailable("port"))?;
    file.sync_all().map_err(|_| Fault::Unavailable("port"))?;
    let line = serde_json::json!({
        "contract": CONTRACT,
        "listening": true,
        "port": port,
        "production_signed": PRODUCTION_SIGNED,
        "deployment": DEPLOYMENT,
    });
    println!("{line}");
    let _ = std::io::stdout().flush();
    loop {
        let (sock, _) = listener.accept().await.map_err(|_| Fault::Unavailable("accept"))?;
        let cfg = cfg.clone();
        tokio::spawn(async move {
            let _ = dispatch(sock, &cfg).await;
        });
    }
}

async fn dispatch(mut sock: TcpStream, cfg: &Config) -> Result<(), Fault> {
    let incoming = match read_request(&mut sock).await {
        Ok(incoming) => incoming,
        Err(fault) => {
            write_fault(&mut sock, &fault).await?;
            return Ok(());
        }
    };
    let result = route(cfg, &incoming).await;
    match result {
        Ok(response) => write_response(&mut sock, response.status, &response.headers, &response.body).await?,
        Err(fault) => write_fault(&mut sock, &fault).await?,
    }
    Ok(())
}

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

async fn route(cfg: &Config, incoming: &Incoming) -> Result<Response, Fault> {
    let parts: Vec<&str> = incoming.path.split('/').filter(|part| !part.is_empty()).collect();
    let value = if incoming.body.is_empty() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_slice(&incoming.body).map_err(|_| Fault::Malformed("json"))?
    };
    if incoming.method == "POST" && config::self_asserted(&value) {
        return Err(Fault::Malformed("self_asserted_rejected"));
    }
    match (incoming.method.as_str(), parts.as_slice()) {
        ("POST", ["control", "v1", "entry", "session", "consume"]) => consume(cfg, &value).await,
        ("GET", ["control", "v1", "entry", "session", "current"]) => {
            let caller = caller(cfg, incoming, false).await?;
            json_ok(200, &serde_json::json!({
                "contract": CONTRACT,
                "principal_id": caller.row.principal_id,
                "project_id": caller.row.project_id,
                "role": caller.row.role_name,
            }))
        }
        ("POST", ["control", "v1", "conversations"]) => {
            let caller = caller(cfg, incoming, true).await?;
            require_match(&caller)?;
            require_cap(&caller.binding, "session.use")?;
            let created = lifecycle::create_conversation(cfg, &caller.binding).await?;
            json_ok(201, &serde_json::json!({
                "contract": CONTRACT,
                "conversation_id": created.conversation_id,
                "session_id": created.session_id,
                "revision": created.revision,
                "phase": created.phase,
                "production_signed": PRODUCTION_SIGNED,
            }))
        }
        ("GET", ["control", "v1", "conversations"]) => {
            let caller = caller(cfg, incoming, false).await?;
            let endpoint = endpoint_of(cfg);
            let db = runtime_db(cfg, &endpoint);
            let rows = store::list_conversations(&db, &caller.row.project_id, &caller.row.principal_id).await?;
            let items: Vec<_> = rows
                .into_iter()
                .map(|row| serde_json::json!({"conversation_id": row.conversation_id, "session_id": row.session_id}))
                .collect();
            json_ok(200, &serde_json::json!({"contract": CONTRACT, "conversations": items}))
        }
        ("GET", ["control", "v1", "conversations", id]) => {
            let caller = caller(cfg, incoming, false).await?;
            conversation_detail(cfg, &caller, id).await
        }
        ("GET", ["control", "v1", "conversations", id, "events"]) => {
            let caller = caller(cfg, incoming, false).await?;
            list_events(cfg, &caller, id, &incoming.query).await
        }
        ("POST", ["control", "v1", "conversations", id, "turns"]) => {
            let caller = caller(cfg, incoming, true).await?;
            require_match(&caller)?;
            require_cap(&caller.binding, "session.use")?;
            let turn_id = value.get("turn_id").and_then(|item| item.as_str()).ok_or(Fault::Malformed("turn"))?;
            let text = value.get("text").and_then(|item| item.as_str()).ok_or(Fault::Malformed("turn"))?;
            let turned = lifecycle::begin_turn(cfg, &caller.binding, id, turn_id, text).await?;
            json_ok(200, &serde_json::json!({
                "contract": CONTRACT,
                "turn_id": turned.turn_id,
                "revision": turned.revision,
                "phase": turned.phase,
                "event_seq": turned.event_seq,
            }))
        }
        ("POST", ["control", "v1", "conversations", id, "cancel"]) => {
            let caller = caller(cfg, incoming, true).await?;
            close_with(cfg, id, &caller, false).await
        }
        ("POST", ["control", "v1", "conversations", id, "terminate"]) => {
            let caller = caller(cfg, incoming, true).await?;
            require_match(&caller)?;
            require_cap(&caller.binding, "session.terminate")?;
            close_with(cfg, id, &caller, true).await
        }
        ("GET", ["control", "v1", "conversations", id, "outputs", name]) => {
            let caller = caller(cfg, incoming, false).await?;
            output(cfg, &caller, id, name).await
        }
        _ => Err(Fault::NotFound),
    }
}

struct Caller {
    row: HttpRow,
    binding: Binding,
}

async fn caller(cfg: &Config, incoming: &Incoming, mutation: bool) -> Result<Caller, Fault> {
    let secret = cookie_value(&incoming.cookie).ok_or(Fault::Unauthenticated)?;
    let endpoint = endpoint_of(cfg);
    let db = runtime_db(cfg, &endpoint);
    let row = store::load_session(&db, &config::digest_text(secret)?).await?;
    if mutation {
        let csrf = if incoming.csrf.is_empty() { None } else { Some(incoming.csrf.as_str()) };
        let presented = csrf.ok_or(Fault::Forbidden("csrf"))?;
        if !eq_code(&config::digest_text(presented)?, &row.csrf_sha256) {
            return Err(Fault::Forbidden("csrf"));
        }
    }
    let bindings = config::load_bindings(&cfg.bindings_path)?;
    let binding = config::find_binding(&bindings, &row.principal_id, &row.project_id)
        .cloned()
        .ok_or(Fault::Forbidden("permission_changed"))?;
    Ok(Caller { row, binding })
}

fn require_match(caller: &Caller) -> Result<(), Fault> {
    let hash = config::claims_hash(&caller.binding.claims)?;
    if caller.binding.role == caller.row.role_name
        && hash == caller.row.claims_hash
        && caller.binding.policy_version == caller.row.policy_version
        && caller.binding.knowledge_version == caller.row.knowledge_version
        && caller.binding.config_version == caller.row.config_version
    {
        Ok(())
    } else {
        Err(Fault::Forbidden("permission_changed"))
    }
}

fn require_cap(binding: &Binding, cap: &str) -> Result<(), Fault> {
    if binding.capabilities.iter().any(|item| item == cap) {
        Ok(())
    } else {
        Err(Fault::Forbidden("capability"))
    }
}

async fn consume(cfg: &Config, value: &serde_json::Value) -> Result<Response, Fault> {
    let grant = value.get("grant").and_then(|item| item.as_str()).ok_or(Fault::Malformed("grant"))?;
    if grant.len() != 64 || !grant.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
        return Err(Fault::Malformed("grant"));
    }
    let grant_hash = config::digest_text(grant)?;
    let endpoint = endpoint_of(cfg);
    let db = runtime_db(cfg, &endpoint);
    let (principal, project) = store::load_grant(&db, &grant_hash).await?;
    let bindings = config::load_bindings(&cfg.bindings_path)?;
    let binding = config::find_binding(&bindings, &principal, &project).ok_or(Fault::Forbidden("binding_mismatch"))?;
    let claims_hash = config::claims_hash(&binding.claims)?;
    let session_secret = lifecycle::random_hex(32)?;
    let csrf_secret = lifecycle::random_hex(32)?;
    let row = HttpRow {
        principal_id: binding.principal_id.clone(),
        project_id: binding.project_id.clone(),
        role_name: binding.role.clone(),
        claims_hash,
        policy_version: binding.policy_version.clone(),
        knowledge_version: binding.knowledge_version.clone(),
        config_version: binding.config_version.clone(),
        csrf_sha256: config::digest_text(&csrf_secret)?,
    };
    store::consume_grant(&db, &grant_hash, &row, &config::digest_text(&session_secret)?).await?;
    let body = serde_json::to_vec(&serde_json::json!({
        "contract": CONTRACT,
        "principal_id": row.principal_id,
        "project_id": row.project_id,
        "role": row.role_name,
        "csrf": csrf_secret,
        "production_signed": PRODUCTION_SIGNED,
    }))
    .map_err(|_| Fault::Unavailable("json"))?;
    Ok(Response {
        status: 200,
        headers: vec![(
            "set-cookie".to_owned(),
            format!("oo-entry={session_secret}; HttpOnly; Path=/"),
        )],
        body,
    })
}

async fn conversation_detail(cfg: &Config, caller: &Caller, id: &str) -> Result<Response, Fault> {
    if !is_token(id) {
        return Err(Fault::Malformed("conversation_id"));
    }
    let endpoint = endpoint_of(cfg);
    let db = runtime_db(cfg, &endpoint);
    let conversation = lifecycle::own_conversation(&db, &caller.binding, id).await?;
    let resource = store::load_resource(&db, &conversation.project_id, &conversation.session_id).await?;
    let head = load_head(cfg, &conversation.project_id, &conversation.session_id).await?;
    json_ok(200, &serde_json::json!({
        "contract": CONTRACT,
        "conversation_id": conversation.conversation_id,
        "session_id": conversation.session_id,
        "phase": phase_label(head.phase),
        "stage": resource.stage,
        "revision": head.revision,
        "production_signed": PRODUCTION_SIGNED,
    }))
}

async fn list_events(cfg: &Config, caller: &Caller, id: &str, query: &str) -> Result<Response, Fault> {
    if !is_token(id) {
        return Err(Fault::Malformed("conversation_id"));
    }
    let (after, limit) = event_query(query)?;
    let endpoint = endpoint_of(cfg);
    let db = runtime_db(cfg, &endpoint);
    let conversation = lifecycle::own_conversation(&db, &caller.binding, id).await?;
    let rows = store::list_events(&db, &conversation.project_id, id, after, i64::from(limit)).await?;
    let events: Vec<_> = rows
        .into_iter()
        .map(|row| serde_json::json!({"seq": row.seq, "event_id": row.event_id, "kind": row.kind, "body": row.body}))
        .collect();
    json_ok(200, &serde_json::json!({"contract": CONTRACT, "events": events}))
}

fn event_query(query: &str) -> Result<(i32, i32), Fault> {
    let mut after = 0i32;
    let mut limit = 50i32;
    if query.is_empty() {
        return Ok((after, limit));
    }
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(Fault::Malformed("query"));
        };
        let parsed: i32 = value.parse().map_err(|_| Fault::Malformed("query"))?;
        if key == "after" && parsed >= 0 {
            after = parsed;
        } else if key == "limit" && (1..=50).contains(&parsed) {
            limit = parsed;
        } else {
            return Err(Fault::Malformed("query"));
        }
    }
    Ok((after, limit))
}

async fn output(cfg: &Config, caller: &Caller, id: &str, name: &str) -> Result<Response, Fault> {
    if !is_token(id) || (name != "outputs" && name != "transcript") {
        return Err(Fault::Malformed("output"));
    }
    let endpoint = endpoint_of(cfg);
    let db = runtime_db(cfg, &endpoint);
    let conversation = lifecycle::own_conversation(&db, &caller.binding, id).await?;
    let (sha, bytes, resource) = store::load_output(&db, &conversation.project_id, &conversation.session_id, name).await?;
    let hash = config::claims_hash(&caller.binding.claims)?;
    if caller.binding.role != resource.role_name
        || hash != resource.claims_hash
        || caller.binding.policy_version != resource.policy_version
        || caller.binding.knowledge_version != resource.knowledge_version
        || caller.binding.config_version != resource.config_version
    {
        return Err(Fault::Forbidden("permission_changed"));
    }
    Ok(Response {
        status: 200,
        headers: vec![
            ("content-type".to_owned(), "application/octet-stream".to_owned()),
            ("x-oo-sha256".to_owned(), sha),
        ],
        body: bytes,
    })
}

async fn close_with(cfg: &Config, id: &str, caller: &Caller, admin: bool) -> Result<Response, Fault> {
    if !is_token(id) {
        return Err(Fault::Malformed("conversation_id"));
    }
    let receipt = close::cancel_session(cfg, id, &caller.row.principal_id, &caller.row.project_id, admin).await?;
    let status = if receipt.acceptable() { 200 } else { 409 };
    let body = serde_json::to_vec(&receipt).map_err(|_| Fault::Unavailable("json"))?;
    Ok(Response { status, headers: Vec::new(), body })
}

fn json_ok(status: u16, value: &serde_json::Value) -> Result<Response, Fault> {
    let body = serde_json::to_vec(value).map_err(|_| Fault::Unavailable("json"))?;
    Ok(Response { status, headers: Vec::new(), body })
}

fn cookie_value(header: &str) -> Option<&str> {
    header.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        if name.trim() == "oo-entry" { Some(value.trim()) } else { None }
    })
}

fn eq_code(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.bytes().zip(right.bytes()).fold(0u8, |acc, (one, other)| acc | (one ^ other)) == 0
}

async fn read_request(sock: &mut TcpStream) -> Result<Incoming, Fault> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    let header_end = loop {
        if buf.len() > 65_536 {
            return Err(Fault::Malformed("too_large"));
        }
        let read = tokio::time::timeout(Duration::from_secs(10), sock.read(&mut tmp))
            .await
            .map_err(|_| Fault::Malformed("timeout"))?
            .map_err(|_| Fault::Malformed("read"))?;
        if read == 0 {
            return Err(Fault::Malformed("eof"));
        }
        buf.extend_from_slice(&tmp[..read]);
        if let Some(pos) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            break pos;
        }
    };
    let head = std::str::from_utf8(&buf[..header_end]).map_err(|_| Fault::Malformed("headers"))?;
    let mut lines = head.split("\r\n");
    let request = lines.next().ok_or(Fault::Malformed("request"))?;
    let mut fields = request.split(' ');
    let method = fields.next().ok_or(Fault::Malformed("request"))?.to_owned();
    let target = fields.next().ok_or(Fault::Malformed("request"))?;
    if method != "GET" && method != "POST" {
        return Err(Fault::Malformed("method"));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut content_length = 0usize;
    let mut cookie = String::new();
    let mut csrf = String::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err(Fault::Malformed("headers"));
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        if name == "content-length" {
            content_length = value.parse().map_err(|_| Fault::Malformed("content_length"))?;
        } else if name == "cookie" {
            cookie = value.to_owned();
        } else if name == "x-oo-csrf" {
            csrf = value.to_owned();
        }
    }
    if content_length > 65_536 {
        return Err(Fault::Malformed("too_large"));
    }
    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < content_length {
        let read = tokio::time::timeout(Duration::from_secs(10), sock.read(&mut tmp))
            .await
            .map_err(|_| Fault::Malformed("timeout"))?
            .map_err(|_| Fault::Malformed("read"))?;
        if read == 0 {
            return Err(Fault::Malformed("eof"));
        }
        body.extend_from_slice(&tmp[..read]);
    }
    body.truncate(content_length);
    Ok(Incoming {
        method,
        path: path.to_owned(),
        query: query.to_owned(),
        cookie,
        csrf,
        body,
    })
}

async fn write_fault(sock: &mut TcpStream, fault: &Fault) -> Result<(), Fault> {
    let body = serde_json::to_vec(&serde_json::json!({
        "contract": CONTRACT,
        "reason": fault.reason(),
        "production_signed": PRODUCTION_SIGNED,
    }))
    .map_err(|_| Fault::Unavailable("json"))?;
    write_response(sock, fault.status(), &[], &body).await
}

async fn write_response(sock: &mut TcpStream, status: u16, headers: &[(String, String)], body: &[u8]) -> Result<(), Fault> {
    let reason = reason_phrase(status);
    let mut head = format!("HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n", body.len());
    if headers.iter().any(|(name, _)| name == "content-type") {
        head = format!("HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nconnection: close\r\n", body.len());
    }
    for (name, value) in headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    sock.write_all(head.as_bytes()).await.map_err(|_| Fault::Unavailable("write"))?;
    sock.write_all(body).await.map_err(|_| Fault::Unavailable("write"))?;
    sock.flush().await.map_err(|_| Fault::Unavailable("write"))?;
    Ok(())
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        422 => "Unprocessable Content",
        503 => "Service Unavailable",
        _ => "Error",
    }
}
