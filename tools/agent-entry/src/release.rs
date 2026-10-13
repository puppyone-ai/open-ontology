//! Client and private fixed producer for `getActiveRolePack`.
//! The create body does not supply the digest. Product release-control-api is not edited.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::config::is_hex64;
use crate::error::{conflict, malformed, unavailable, Fault};
use crate::httputil::{header, status_line};
use crate::mapping::token_ok;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivePack {
    pub operation_id: String,
    pub project_id: String,
    pub role: String,
    pub pack_sha256: String,
    pub source_git_commit: String,
    pub policy: String,
    pub knowledge: String,
    pub content_hash: String,
    pub published: bool,
}

pub fn loopback_origin(origin: &str) -> Result<u16, Fault> {
    let rest = origin.strip_prefix("http://127.0.0.1:").ok_or_else(|| malformed("release"))?;
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(malformed("release"));
    }
    let port: u16 = rest.parse().map_err(|_| malformed("release"))?;
    if port == 0 {
        return Err(malformed("release"));
    }
    Ok(port)
}

pub fn parse_active_pack(body: &[u8], project_id: &str, role: &str) -> Result<ActivePack, Fault> {
    let value: Value = serde_json::from_slice(body).map_err(|_| conflict("pack_unpublished"))?;
    if value.get("operation").and_then(Value::as_str) != Some("getActiveRolePack") {
        return Err(conflict("pack_unpublished"));
    }
    if value.get("published").and_then(Value::as_bool) != Some(true) {
        return Err(conflict("pack_unpublished"));
    }
    let pack = ActivePack {
        operation_id: required(&value, "operation_id")?,
        project_id: required(&value, "project_id")?,
        role: required(&value, "role")?,
        pack_sha256: required(&value, "pack_sha256")?,
        source_git_commit: required(&value, "source_git_commit")?,
        policy: required(&value, "policy")?,
        knowledge: required(&value, "knowledge")?,
        content_hash: required(&value, "content_hash")?,
        published: true,
    };
    if pack.project_id != project_id || pack.role != role || !pack_fields_ok(&pack) {
        return Err(conflict("pack_unpublished"));
    }
    Ok(pack)
}

pub async fn fetch_active_pack(origin: &str, project_id: &str, role: &str) -> Result<ActivePack, Fault> {
    if !token_ok(project_id) || !token_ok(role) {
        return Err(conflict("pack_unpublished"));
    }
    let port = loopback_origin(origin)?;
    let request = format!(
        "GET /control/v1/releases/active/pack?project_id={project_id}&role={role} HTTP/1.1\r\nhost: 127.0.0.1:{port}\r\naccept: application/json\r\nconnection: close\r\n\r\n"
    );
    let (status, body) = http_exchange(port, request.as_bytes()).await?;
    if status != 200 {
        return Err(conflict("pack_unpublished"));
    }
    parse_active_pack(&body, project_id, role)
}

pub async fn serve_private_release(state_path: PathBuf) -> Result<u16, Fault> {
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|_| unavailable("release_bind"))?;
    let port = listener.local_addr().map_err(|_| unavailable("release_bind"))?.port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { continue };
            let state_path = state_path.clone();
            tokio::spawn(async move {
                let Ok(incoming) = crate::httputil::read_request(&mut sock, 8192).await else { return };
                let (status, body) = producer_response(&state_path, &incoming.method, &incoming.path, &incoming.query);
                let _ = write_http(&mut sock, status, &body).await;
            });
        }
    });
    Ok(port)
}

fn required(value: &Value, key: &str) -> Result<String, Fault> {
    value.get(key).and_then(Value::as_str).filter(|text| !text.is_empty()).map(str::to_owned).ok_or_else(|| conflict("pack_unpublished"))
}

fn pack_fields_ok(pack: &ActivePack) -> bool {
    token_ok(&pack.operation_id)
        && pack.project_id == "shop"
        && matches!(pack.role.as_str(), "oo_r_wide" | "oo_r_narrow")
        && is_hex64(&pack.pack_sha256)
        && hex40(&pack.source_git_commit)
        && token_ok(&pack.policy)
        && token_ok(&pack.knowledge)
        && is_hex64(&pack.content_hash)
}

fn hex40(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

async fn http_exchange(port: u16, request: &[u8]) -> Result<(u16, Vec<u8>), Fault> {
    let mut sock = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(("127.0.0.1", port)))
        .await
        .map_err(|_| unavailable("release"))?
        .map_err(|_| unavailable("release"))?;
    tokio::time::timeout(Duration::from_secs(5), sock.write_all(request)).await.map_err(|_| unavailable("release"))?.map_err(|_| unavailable("release"))?;
    read_response(&mut sock).await
}

async fn read_response(sock: &mut TcpStream) -> Result<(u16, Vec<u8>), Fault> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            return Err(unavailable("release"));
        }
        let read = tokio::time::timeout(left, sock.read(&mut chunk)).await.map_err(|_| unavailable("release"))?;
        let n = read.map_err(|_| unavailable("release"))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > 65_536 {
            return Err(malformed("release"));
        }
        if let Some((status, headers, body)) = status_line(&buf)
            && let Some(len) = header(&headers, "content-length").and_then(|value| value.parse::<usize>().ok())
            && body.len() >= len
        {
            return Ok((status, body[..len].to_vec()));
        }
    }
    let (status, _headers, body) = status_line(&buf).ok_or_else(|| unavailable("release"))?;
    Ok((status, body))
}

async fn write_http(sock: &mut TcpStream, status: u16, body: &str) -> Result<(), Fault> {
    let reason = if status == 200 { "OK" } else { "Conflict" };
    let bytes = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    sock.write_all(bytes.as_bytes()).await.map_err(|_| unavailable("release"))
}

fn producer_response(path: &Path, method: &str, request_path: &str, query: &str) -> (u16, String) {
    if method != "GET" || request_path != "/control/v1/releases/active/pack" {
        return (409, unpublished());
    }
    let Some(project_id) = query_value(query, "project_id") else { return (409, unpublished()) };
    let Some(role) = query_value(query, "role") else { return (409, unpublished()) };
    let Ok(bytes) = std::fs::read(path) else { return (409, unpublished()) };
    let Ok(state) = serde_json::from_slice::<ReleaseState>(&bytes) else { return (409, unpublished()) };
    let Some(row) = state.packs.into_iter().find(|item| item.project_id == project_id && item.role == role) else {
        return (409, unpublished());
    };
    if !row.published || !row_ok(&row) {
        return (409, unpublished());
    }
    let body = json!({
        "contract": "agent-entry-v1",
        "operation": "getActiveRolePack",
        "operation_id": row.operation_id,
        "project_id": row.project_id,
        "role": row.role,
        "pack_sha256": row.pack_sha256,
        "source_git_commit": row.source_git_commit,
        "policy": row.policy,
        "knowledge": row.knowledge,
        "content_hash": row.content_hash,
        "published": true,
        "producer": "private_fixed",
        "product_route": "absent",
    })
    .to_string();
    (200, body)
}

fn unpublished() -> String {
    json!({
        "contract": "agent-entry-v1",
        "operation": "getActiveRolePack",
        "published": false,
        "error": "pack_unpublished",
        "producer": "private_fixed",
        "product_route": "absent",
    })
    .to_string()
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key && !value.is_empty()).then(|| value.to_owned())
    })
}

fn row_ok(row: &ReleaseRow) -> bool {
    token_ok(&row.operation_id)
        && row.project_id == "shop"
        && matches!(row.role.as_str(), "oo_r_wide" | "oo_r_narrow")
        && is_hex64(&row.pack_sha256)
        && hex40(&row.source_git_commit)
        && token_ok(&row.policy)
        && token_ok(&row.knowledge)
        && is_hex64(&row.content_hash)
}

#[derive(Deserialize)]
struct ReleaseState {
    packs: Vec<ReleaseRow>,
}

#[derive(Deserialize)]
struct ReleaseRow {
    project_id: String,
    role: String,
    published: bool,
    operation_id: String,
    pack_sha256: String,
    source_git_commit: String,
    policy: String,
    knowledge: String,
    content_hash: String,
}
