//! The loopback reverse proxy of `oo serve` (10 §8.6 point 6).
//!
//! One request per client connection. The guards run in the engine's order
//! (`Origin`, `Host`, control token) so a browser page or a DNS-rebinding
//! name never reaches the engine; an accepted request is forwarded with the
//! control token replaced by the engine token and `Connection: close`, and the
//! engine's response is copied back verbatim until it closes, which is also
//! how an event stream (SSE) passes through. When the engine is not reachable
//! the proxy answers by itself: `GET /v1/docs/health` with
//! `status = unavailable` (so Desktop can show the state and reconnect),
//! everything else with `503 DOCS_ENGINE_UNAVAILABLE`.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

/// Largest request head.
pub const MAX_HEAD: usize = 64 * 1024;
/// Largest request body (the engine's own ceiling).
pub const MAX_BODY: usize = 1024 * 1024;
const HEAD_TIMEOUT: Duration = Duration::from_secs(10);

/// Where the engine currently listens, shared with the supervisor.
#[derive(Default)]
pub struct EngineLink {
    addr: Mutex<Option<SocketAddr>>,
}

impl EngineLink {
    /// The engine accepts connections at `addr`, or is down (`None`).
    pub fn set(&self, addr: Option<SocketAddr>) {
        *self.addr.lock().unwrap_or_else(|p| p.into_inner()) = addr;
    }

    /// Current address.
    pub fn get(&self) -> Option<SocketAddr> {
        *self.addr.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Host side effect of registering a local Drive (10 §8.6 point 6): make the
/// Drive's directory visible to the engine and return the path the engine sees.
/// Only the compose runtime has one; a host-process engine sees the host path.
pub trait DriveMounts: Send + Sync {
    /// Blocking: may recreate the engine container.
    fn mount(&self, host_root: &str) -> Result<String, String>;

    /// The model's `file:` references (endpoint and key files, owner-only) the
    /// engine's egress process will read: make each file visible read-only at
    /// the same absolute path inside the engine (compose runtime). Blocking;
    /// may recreate the engine container. A host-process engine reads the host
    /// path itself, so the default does nothing.
    fn mount_secret_files(&self, _files: &[String]) -> Result<(), String> {
        Ok(())
    }
}

/// The `file:` references (absolute host paths) of a `PUT /v1/docs/config/model`
/// body (`base_url` and `key_ref`); the values themselves are never read here.
pub fn model_secret_files(body: &[u8]) -> Vec<String> {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Vec::new();
    };
    ["base_url", "key_ref"]
        .iter()
        .filter_map(|k| v.get(*k)?.as_str()?.strip_prefix("file:"))
        .filter(|p| p.starts_with('/'))
        .map(str::to_owned)
        .collect()
}

/// What the proxy needs to serve.
pub struct ProxyConfig {
    /// Port the daemon listens on (the `Host` value must carry it).
    pub port: u16,
    /// SHA-256 of the control token.
    pub token_sha256: [u8; 32],
    /// Token the engine expects.
    pub engine_token: String,
    /// Engine address.
    pub engine: Arc<EngineLink>,
    /// Drive mounts (compose runtime), applied to `POST /v1/docs/drives`.
    pub drives: Option<Arc<dyn DriveMounts>>,
}

/// `POST /v1/docs/drives` body with `config.root` replaced; `None` when the
/// body has no string root (the engine will reject it itself).
pub fn rewrite_drive_root(
    body: &[u8],
    map: impl FnOnce(&str) -> Result<String, String>,
) -> Option<Result<Vec<u8>, String>> {
    let mut v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let root = v.get("config")?.get("root")?.as_str()?.to_owned();
    Some(map(&root).map(|new_root| {
        v["config"]["root"] = serde_json::Value::String(new_root);
        v.to_string().into_bytes()
    }))
}

/// The `file:` credential of a `POST /v1/docs/drives` body of an object-store
/// Drive (`config.secret_ref`): the engine reads it itself, so under the
/// compose runtime the file must be visible there (D83).
pub fn drive_secret_files(body: &[u8]) -> Vec<String> {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Vec::new();
    };
    v.get("config")
        .and_then(|c| c.get("secret_ref"))
        .and_then(|r| r.as_str())
        .and_then(|r| r.strip_prefix("file:"))
        .filter(|p| p.starts_with('/'))
        .map(|p| vec![p.to_owned()])
        .unwrap_or_default()
}

/// A parsed request head.
#[derive(Debug, PartialEq, Eq)]
pub struct Head {
    pub method: String,
    pub target: String,
    /// Header names lowercased, values trimmed, in order.
    pub headers: Vec<(String, String)>,
    /// Bytes after the blank line already read.
    pub rest: Vec<u8>,
}

impl Head {
    /// First value of a header (name lowercase).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Path without the query.
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("")
    }
}

/// Parse a head from `buf`, which must contain `\r\n\r\n`.
pub fn parse_head(buf: &[u8]) -> Result<Head, &'static str> {
    let end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("incomplete head")?;
    let text = std::str::from_utf8(&buf[..end]).map_err(|_| "head is not UTF-8")?;
    let mut lines = text.split("\r\n");
    let request = lines.next().ok_or("empty request")?;
    let mut parts = request.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err("malformed request line");
    };
    if !version.starts_with("HTTP/1.") {
        return Err("only HTTP/1.x is served");
    }
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err("malformed method");
    }
    if !target.starts_with('/') {
        return Err("request target must be a path");
    }
    let mut headers = Vec::new();
    for line in lines {
        let (k, v) = line.split_once(':').ok_or("malformed header")?;
        if k.is_empty() || k.contains(' ') {
            return Err("malformed header name");
        }
        headers.push((k.to_ascii_lowercase(), v.trim().to_owned()));
    }
    Ok(Head {
        method: method.to_owned(),
        target: target.to_owned(),
        headers,
        rest: buf[end + 4..].to_vec(),
    })
}

/// Constant-time equality of two digests.
pub fn ct_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// `Authorization: Bearer <token>` matches `expected`.
pub fn token_ok(header: Option<&str>, expected: &[u8; 32]) -> bool {
    let Some(rest) = header.and_then(|h| h.strip_prefix("Bearer ")) else {
        return false;
    };
    let digest: [u8; 32] = Sha256::digest(rest.trim().as_bytes()).into();
    ct_eq(&digest, expected)
}

/// `Host` is one of the loopback names with the daemon's port.
pub fn host_ok(host: Option<&str>, port: u16) -> bool {
    let Some(h) = host else { return false };
    [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ]
    .iter()
    .any(|a| a.eq_ignore_ascii_case(h))
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

pub(super) fn response(status: u16, body: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status} {}\r\ncontent-type: application/json; charset=utf-8\r\ncontent-length: {}\r\noo-docs-api: {}\r\nconnection: close\r\n",
        reason(status),
        body.len(),
        super::secrets::API_VERSION,
    );
    for (k, v) in extra {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    out.push_str(body);
    out.into_bytes()
}

pub(super) fn error(
    status: u16,
    code: &str,
    message: &str,
    retryable: bool,
    extra: &[(&str, &str)],
) -> Vec<u8> {
    let body = json!({
        "code": code,
        "message": message,
        "retryable": retryable,
        "details": {},
    })
    .to_string();
    response(status, &body, extra)
}

/// Header names the proxy never forwards (hop by hop, or replaced).
const DROP: [&str; 9] = [
    "host",
    "authorization",
    "connection",
    "keep-alive",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "upgrade",
    "x-oo-public-host",
];

/// A refs credential (`oodr_...`) on a path the engine lets a credential call:
/// the proxy forwards it untouched and the engine authenticates it (10 §7.8).
pub fn credential_call(head: &Head) -> bool {
    let bearer = head
        .header("authorization")
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or("");
    if !bearer.starts_with("oodr_") && !bearer.starts_with("ooa_") {
        return false;
    }
    let p = head.path();
    // An Agent token (`ooa_`) is authenticated and scope-checked by the engine on every route.
    if bearer.starts_with("ooa_") {
        return p.starts_with("/v1/docs/");
    }
    let segs: Vec<&str> = p.trim_start_matches('/').split('/').collect();
    match (head.method.as_str(), segs.as_slice()) {
        ("GET", ["v1", "docs", "refs", _]) => true,
        ("POST", ["v1", "docs", "shares", last]) => last.ends_with(":check"),
        ("GET", ["v1", "docs", "shares", _, "updates"]) => true,
        ("GET", ["v1", "docs", "shares", _]) => true,
        ("GET", ["v1", "docs", "entities", _]) => true,
        ("POST", ["v1", "docs", "suggestions"]) => true,
        _ => false,
    }
}

/// The request to send to the engine.
pub fn upstream_request(
    head: &Head,
    body: &[u8],
    cfg: &ProxyConfig,
    engine: SocketAddr,
) -> Vec<u8> {
    let mut out = format!("{} {} HTTP/1.1\r\n", head.method, head.target).into_bytes();
    let mut put = |k: &str, v: &str| out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
    put("host", &engine.to_string());
    match head.header("authorization") {
        Some(h) if credential_call(head) => put("authorization", h),
        _ => put("authorization", &format!("Bearer {}", cfg.engine_token)),
    }
    put("x-oo-public-host", &format!("127.0.0.1:{}", cfg.port));
    put("connection", "close");
    for (k, v) in &head.headers {
        if DROP.contains(&k.as_str()) {
            continue;
        }
        put(k, v);
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

async fn read_head(client: &mut TcpStream) -> Result<Head, Vec<u8>> {
    let bad = |m: &str| error(400, "DOCS_ENGINE_BAD_REQUEST", m, false, &[]);
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];
    loop {
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            return parse_head(&buf).map_err(bad);
        }
        if buf.len() > MAX_HEAD {
            return Err(bad("request head too large"));
        }
        let n = match tokio::time::timeout(HEAD_TIMEOUT, client.read(&mut chunk)).await {
            Ok(Ok(n)) if n > 0 => n,
            _ => return Err(Vec::new()),
        };
        buf.extend_from_slice(&chunk[..n]);
    }
}

async fn handle(mut client: TcpStream, cfg: Arc<ProxyConfig>, mut stop: watch::Receiver<bool>) {
    let out = match serve_one(&mut client, &cfg, &mut stop).await {
        Ok(()) => return,
        Err(bytes) => bytes,
    };
    if !out.is_empty() {
        let _ = client.write_all(&out).await;
    }
    let _ = client.shutdown().await;
}

/// `Ok` when the response was already streamed; `Err(bytes)` is a response
/// the proxy itself must send (empty: just close).
async fn serve_one(
    client: &mut TcpStream,
    cfg: &ProxyConfig,
    stop: &mut watch::Receiver<bool>,
) -> Result<(), Vec<u8>> {
    let head = read_head(client).await?;
    if head.header("origin").is_some() {
        return Err(error(
            403,
            "DOCS_ENGINE_FORBIDDEN_ORIGIN",
            "requests with an Origin header are refused",
            false,
            &[],
        ));
    }
    if !host_ok(head.header("host"), cfg.port) {
        return Err(error(
            400,
            "DOCS_ENGINE_BAD_HOST",
            "Host is not allowed",
            false,
            &[],
        ));
    }
    if head.method == "GET" && head.path() == "/livez" {
        return Err(
            b"HTTP/1.1 204 No Content\r\noo-docs-api: 1.0\r\nconnection: close\r\n\r\n".to_vec(),
        );
    }
    let ontology = head.path().starts_with("/v1/ontology/");
    let agent = head
        .header("authorization")
        .is_some_and(|h| h.starts_with("Bearer ooa_"));
    if !(ontology && agent)
        && !token_ok(head.header("authorization"), &cfg.token_sha256)
        && !credential_call(&head)
    {
        return Err(error(
            401,
            "DOCS_ENGINE_UNAUTHENTICATED",
            "a valid bearer token is required",
            false,
            &[("www-authenticate", "Bearer")],
        ));
    }
    if head.header("transfer-encoding").is_some() {
        return Err(error(
            400,
            "DOCS_ENGINE_BAD_REQUEST",
            "chunked request bodies are not accepted",
            false,
            &[],
        ));
    }
    let length: usize = match head.header("content-length") {
        None => 0,
        Some(v) => v.parse().map_err(|_| {
            error(
                400,
                "DOCS_ENGINE_BAD_REQUEST",
                "bad Content-Length",
                false,
                &[],
            )
        })?,
    };
    if length > MAX_BODY {
        return Err(error(
            413,
            "DOCS_ENGINE_PAYLOAD_TOO_LARGE",
            "request body exceeds 1 MiB",
            false,
            &[],
        ));
    }
    let mut body = head.rest.clone();
    body.truncate(length);
    while body.len() < length {
        let mut chunk = vec![0u8; (length - body.len()).min(64 * 1024)];
        let n = match tokio::time::timeout(HEAD_TIMEOUT, client.read(&mut chunk)).await {
            Ok(Ok(n)) if n > 0 => n,
            _ => return Err(Vec::new()),
        };
        body.extend_from_slice(&chunk[..n]);
    }

    if ontology {
        return Err(super::federate::serve(&head, &body, cfg).await);
    }
    let mut head = head;
    if let Some(drives) = cfg.drives.clone()
        && head.method == "POST"
        && head.path() == "/v1/docs/drives"
    {
        let files = drive_secret_files(&body);
        if !files.is_empty() {
            let drives = drives.clone();
            let mounted = tokio::task::spawn_blocking(move || drives.mount_secret_files(&files))
                .await
                .unwrap_or_else(|_| Err("mount task failed".into()));
            if let Err(why) = mounted {
                return Err(error(502, "DOCS_ENGINE_UNAVAILABLE", &why, true, &[]));
            }
        }
        let b = body.clone();
        let rewritten =
            tokio::task::spawn_blocking(move || rewrite_drive_root(&b, |root| drives.mount(root)))
                .await
                .ok()
                .flatten();
        match rewritten {
            Some(Ok(new_body)) => {
                for (k, v) in &mut head.headers {
                    if k == "content-length" {
                        *v = new_body.len().to_string();
                    }
                }
                body = new_body;
            }
            Some(Err(why)) => {
                return Err(error(422, "DOCS_SOURCE_ROOT_MISSING", &why, true, &[]));
            }
            None => {}
        }
    }

    // The engine's egress process lives in the engine container: the files the
    // model configuration names must be mounted there before it starts.
    if let Some(drives) = cfg.drives.clone()
        && head.method == "PUT"
        && head.path() == "/v1/docs/config/model"
    {
        let files = model_secret_files(&body);
        if !files.is_empty() {
            let mounted = tokio::task::spawn_blocking(move || drives.mount_secret_files(&files))
                .await
                .unwrap_or_else(|_| Err("mount task failed".into()));
            if let Err(why) = mounted {
                return Err(error(502, "DOCS_ENGINE_UNAVAILABLE", &why, true, &[]));
            }
        }
    }

    let unavailable = |head: &Head| {
        if head.method == "GET" && head.path() == "/v1/docs/health" {
            response(
                200,
                &json!({"status": "unavailable", "code": "DOCS_ENGINE_UNAVAILABLE"}).to_string(),
                &[],
            )
        } else {
            error(
                503,
                "DOCS_ENGINE_UNAVAILABLE",
                "the docs engine is not reachable",
                true,
                &[("retry-after", "1")],
            )
        }
    };
    let Some(addr) = cfg.engine.get() else {
        return Err(unavailable(&head));
    };
    let mut upstream =
        match tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(addr)).await {
            Ok(Ok(s)) => s,
            _ => return Err(unavailable(&head)),
        };
    if upstream
        .write_all(&upstream_request(&head, &body, cfg, addr))
        .await
        .is_err()
    {
        return Err(unavailable(&head));
    }
    // The write half stays open: a hyper server treats a half-closed
    // connection as an aborted request and drops the response.
    let (mut ur, _uw) = upstream.split();
    let (mut cr, mut cw) = client.split();
    // Copy the response; stop when the engine closes, the client goes away or
    // the daemon shuts down. A client that disconnects shows up as a read of
    // zero bytes on its side.
    // An engine that accepts and dies before it says anything (it is being
    // killed) must look like an unavailable engine, not like a dropped
    // connection.
    let mut first = [0u8; 8192];
    let n = tokio::select! {
        r = ur.read(&mut first) => r.unwrap_or(0),
        _ = stop.changed() => return Ok(()),
    };
    if n == 0 {
        return Err(unavailable(&head));
    }
    if cw.write_all(&first[..n]).await.is_err() {
        return Ok(());
    }
    let mut sink = [0u8; 256];
    tokio::select! {
        _ = tokio::io::copy(&mut ur, &mut cw) => {}
        _ = async { while matches!(cr.read(&mut sink).await, Ok(n) if n > 0) {} } => {}
        _ = stop.changed() => {}
    }
    let _ = cw.shutdown().await;
    Ok(())
}

/// Accept connections until `stop` flips; then wait up to `grace` for the
/// requests in flight and drop the rest.
pub async fn serve(
    listener: TcpListener,
    cfg: Arc<ProxyConfig>,
    mut stop: watch::Receiver<bool>,
    grace: Duration,
) {
    let inflight = Arc::new(AtomicUsize::new(0));
    let (abort_tx, abort_rx) = watch::channel(false);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { continue };
                let (cfg, inflight, mut abort) = (cfg.clone(), inflight.clone(), abort_rx.clone());
                inflight.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    tokio::select! {
                        _ = handle(stream, cfg, abort.clone()) => {}
                        _ = abort.changed() => {}
                    }
                    inflight.fetch_sub(1, Ordering::SeqCst);
                });
            }
            _ = stop.changed() => break,
        }
    }
    drop(listener);
    let deadline = tokio::time::Instant::now() + grace;
    while inflight.load(Ordering::SeqCst) > 0 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let _ = abort_tx.send(true);
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_drive_credential_file_is_mounted_but_an_env_or_name_reference_is_not() {
        assert_eq!(
            drive_secret_files(br#"{"config":{"bucket":"b","secret_ref":"file:/h/.config/open-ontology/secrets/src_a_s3"}}"#),
            ["/h/.config/open-ontology/secrets/src_a_s3"]
        );
        assert!(drive_secret_files(br#"{"config":{"secret_ref":"env:X"}}"#).is_empty());
        assert!(drive_secret_files(br#"{"config":{"secret_ref":"name"}}"#).is_empty());
        assert!(drive_secret_files(br#"{"config":{"root":"/x"}}"#).is_empty());
        assert!(drive_secret_files(b"nope").is_empty());
    }

    #[test]
    fn model_secret_files_come_from_file_references_only() {
        let body = br#"{"provider":"openai-compatible","base_url":"file:/home/o/.config/e","model":"m","key_ref":"file:/home/o/.config/k"}"#;
        assert_eq!(
            model_secret_files(body),
            ["/home/o/.config/e", "/home/o/.config/k"]
        );
        let env = br#"{"base_url":"https://api.example.com","key_ref":"env:KEY"}"#;
        assert!(model_secret_files(env).is_empty());
        assert!(model_secret_files(br#"{"key_ref":"file:relative"}"#).is_empty());
        assert!(model_secret_files(b"not json").is_empty());
    }

    use super::*;

    fn cfg(port: u16, engine: Option<SocketAddr>) -> Arc<ProxyConfig> {
        let link = Arc::new(EngineLink::default());
        link.set(engine);
        Arc::new(ProxyConfig {
            port,
            token_sha256: Sha256::digest(b"oodt_secret").into(),
            engine_token: "oodi_engine".into(),
            engine: link,
            drives: None,
        })
    }

    struct FakeMounts;
    impl DriveMounts for FakeMounts {
        fn mount(&self, host_root: &str) -> Result<String, String> {
            if host_root.starts_with("/nope") {
                Err("source root is missing or not a directory".into())
            } else {
                Ok("/drives/abc".into())
            }
        }
    }

    #[test]
    fn drive_roots_are_rewritten_only_when_there_is_a_string_root() {
        let ok = |r: &str| Ok(format!("/drives{r}"));
        let out = rewrite_drive_root(br#"{"name":"n","config":{"root":"/h","x":1}}"#, ok)
            .unwrap()
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["config"]["root"], "/drives/h");
        assert_eq!(v["config"]["x"], 1);
        assert_eq!(v["name"], "n");
        assert!(rewrite_drive_root(br#"{"config":{}}"#, ok).is_none());
        assert!(rewrite_drive_root(b"not json", ok).is_none());
        assert!(rewrite_drive_root(br#"{"config":{"root":3}}"#, ok).is_none());
        assert!(
            rewrite_drive_root(br#"{"config":{"root":"/x"}}"#, |_| Err("no".into()))
                .unwrap()
                .is_err()
        );
    }

    #[tokio::test]
    async fn registering_a_drive_goes_through_the_mount_hook() {
        let (engine, seen) = fake_engine().await;
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let mut c = Arc::try_unwrap(cfg(port, Some(engine))).ok().unwrap();
        c.drives = Some(Arc::new(FakeMounts));
        let l = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
        let addr = l.local_addr().unwrap();
        let (tx, rx) = watch::channel(false);
        let h = tokio::spawn(serve(l, Arc::new(c), rx, Duration::from_secs(2)));
        let post = |body: &str| {
            format!(
                "POST /v1/docs/drives HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer oodt_secret\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
        };
        let r = raw(
            addr,
            &post(r#"{"name":"d","backend":"fs","config":{"root":"/host/dir"}}"#),
        )
        .await;
        assert!(r.starts_with("HTTP/1.1 200"), "{r}");
        let upstream = seen.lock().unwrap().join("\n");
        assert!(
            upstream.contains(r#""root":"/drives/abc""#) && !upstream.contains("/host/dir"),
            "{upstream}"
        );
        assert!(
            upstream.to_lowercase().contains(&format!(
                "content-length: {}",
                r#"{"backend":"fs","config":{"root":"/drives/abc"},"name":"d"}"#.len()
            )),
            "{upstream}"
        );
        let r = raw(
            addr,
            &post(r#"{"name":"d","backend":"fs","config":{"root":"/nope/x"}}"#),
        )
        .await;
        assert!(
            r.starts_with("HTTP/1.1 422") && r.contains("DOCS_SOURCE_ROOT_MISSING"),
            "{r}"
        );
        let _ = tx.send(true);
        let _ = h.await;
    }

    #[test]
    fn heads_parse_and_malformed_ones_are_refused() {
        let h = parse_head(
            b"POST /v1/docs/drives?x=1 HTTP/1.1\r\nHost: a\r\nContent-Length: 2\r\n\r\n{}",
        )
        .unwrap();
        assert_eq!((h.method.as_str(), h.path()), ("POST", "/v1/docs/drives"));
        assert_eq!(h.header("content-length"), Some("2"));
        assert_eq!(h.rest, b"{}");
        for bad in [
            &b"GET http://evil/ HTTP/1.1\r\n\r\n"[..],
            b"GET / SPDY/3\r\n\r\n",
            b"get / HTTP/1.1\r\n\r\n",
            b"GET /\r\n\r\n",
            b"GET / HTTP/1.1\r\nbroken\r\n\r\n",
            b"GET / HTTP/1.1\r\n",
        ] {
            assert!(parse_head(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn host_and_token_guards() {
        assert!(host_ok(Some("127.0.0.1:8123"), 8123));
        assert!(host_ok(Some("LOCALHOST:8123"), 8123));
        assert!(!host_ok(Some("127.0.0.1:9"), 8123));
        assert!(!host_ok(Some("evil.example:8123"), 8123));
        assert!(!host_ok(Some("127.0.0.1.evil.example:8123"), 8123));
        assert!(!host_ok(None, 8123));
        let want: [u8; 32] = Sha256::digest(b"oodt_secret").into();
        assert!(token_ok(Some("Bearer oodt_secret"), &want));
        assert!(!token_ok(Some("Bearer oodt_secreT"), &want));
        assert!(!token_ok(Some("oodt_secret"), &want));
        assert!(!token_ok(Some("Basic oodt_secret"), &want));
        assert!(!token_ok(None, &want));
    }

    #[test]
    fn upstream_request_swaps_credentials_and_drops_hop_headers() {
        let head = parse_head(
            b"POST /v1/docs/drives HTTP/1.1\r\nHost: 127.0.0.1:1\r\nAuthorization: Bearer oodt_secret\r\nConnection: keep-alive\r\nIdempotency-Key: k\r\nX-OO-Public-Host: evil\r\nContent-Length: 2\r\n\r\n",
        )
        .unwrap();
        let c = cfg(1, None);
        let req = upstream_request(&head, b"{}", &c, "127.0.0.1:9".parse().unwrap());
        let text = String::from_utf8(req).unwrap();
        assert!(text.contains("authorization: Bearer oodi_engine\r\n"));
        assert!(!text.contains("oodt_secret"));
        assert!(text.contains("connection: close\r\n"));
        assert!(!text.contains("keep-alive"));
        assert!(text.contains("host: 127.0.0.1:9\r\n"));
        assert!(text.contains("x-oo-public-host: 127.0.0.1:1\r\n"));
        assert!(!text.contains("evil"));
        assert!(text.contains("idempotency-key: k\r\n"));
        assert!(text.ends_with("\r\n\r\n{}"));
    }

    async fn raw(addr: SocketAddr, request: &str) -> String {
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(request.as_bytes()).await.unwrap();
        let mut out = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut out)).await;
        String::from_utf8_lossy(&out).into_owned()
    }

    /// A stand-in engine: records the request it got, answers per path.
    async fn fake_engine() -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                let log = log.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    loop {
                        let n = s.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    // Like hyper: a client that half-closes before the response
                    // is going away, so answer nothing.
                    let mut probe = [0u8; 1];
                    if let Ok(Ok(0)) =
                        tokio::time::timeout(Duration::from_millis(150), s.read(&mut probe)).await
                    {
                        return;
                    }
                    let text = String::from_utf8_lossy(&buf).into_owned();
                    log.lock().unwrap().push(text.clone());
                    if text.starts_with("GET /v1/docs/events") {
                        let _ = s
                            .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n")
                            .await;
                        for i in 1..=3 {
                            let _ = s
                                .write_all(format!("id: {i}\ndata: {{\"seq\":{i}}}\n\n").as_bytes())
                                .await;
                            let _ = s.flush().await;
                            tokio::time::sleep(Duration::from_millis(60)).await;
                        }
                    } else {
                        let _ = s
                            .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 11\r\nconnection: close\r\n\r\n{\"ok\":true}\n")
                            .await;
                    }
                });
            }
        });
        (addr, seen)
    }

    #[tokio::test]
    async fn requests_are_forwarded_and_event_streams_pass_through() {
        let (engine, seen) = fake_engine().await;
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let c = cfg(port, Some(engine));
        let l = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
        let addr = l.local_addr().unwrap();
        let (tx, rx) = watch::channel(false);
        let h = tokio::spawn(serve(l, c, rx, Duration::from_secs(2)));
        let host = format!("127.0.0.1:{port}");
        let auth = "Authorization: Bearer oodt_secret\r\n";

        let r = raw(
            addr,
            &format!("GET /v1/docs/drives HTTP/1.1\r\nHost: {host}\r\n{auth}\r\n"),
        )
        .await;
        assert!(
            r.starts_with("HTTP/1.1 200") && r.ends_with("{\"ok\":true}\n"),
            "{r}"
        );
        let r = raw(
            addr,
            &format!("GET /v1/docs/events?since=0 HTTP/1.1\r\nHost: {host}\r\n{auth}\r\n"),
        )
        .await;
        for i in 1..=3 {
            assert!(r.contains(&format!("id: {i}")), "{r}");
        }
        let log = seen.lock().unwrap().clone();
        assert_eq!(log.len(), 2);
        assert!(log[0].contains("authorization: Bearer oodi_engine"));
        assert!(!log[0].contains("oodt_secret"));
        let _ = tx.send(true);
        h.await.unwrap();
    }

    #[tokio::test]
    async fn an_engine_that_dies_without_answering_is_unavailable_not_a_dropped_connection() {
        let dying = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let engine = dying.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = dying.accept().await else {
                    return;
                };
                let mut b = [0u8; 4096];
                let _ = s.read(&mut b).await;
                drop(s);
            }
        });
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let c = cfg(port, Some(engine));
        let l = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
        let addr = l.local_addr().unwrap();
        let (tx, rx) = watch::channel(false);
        let h = tokio::spawn(serve(l, c, rx, Duration::from_secs(1)));
        let host = format!("127.0.0.1:{port}");
        let r = raw(addr, &format!("GET /v1/docs/health HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer oodt_secret\r\n\r\n")).await;
        assert!(
            r.starts_with("HTTP/1.1 200") && r.contains("\"unavailable\""),
            "{r}"
        );
        let r = raw(addr, &format!("GET /v1/docs/drives HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer oodt_secret\r\n\r\n")).await;
        assert!(r.starts_with("HTTP/1.1 503"), "{r}");
        let _ = tx.send(true);
        h.await.unwrap();
    }

    #[tokio::test]
    async fn the_proxy_recovers_when_the_engine_comes_back() {
        let (engine, _) = fake_engine().await;
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let c = cfg(port, None);
        let link = c.engine.clone();
        let l = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
        let addr = l.local_addr().unwrap();
        let (tx, rx) = watch::channel(false);
        let h = tokio::spawn(serve(l, c, rx, Duration::from_secs(1)));
        let req = format!(
            "GET /v1/docs/drives HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer oodt_secret\r\n\r\n"
        );
        assert!(raw(addr, &req).await.starts_with("HTTP/1.1 503"));
        link.set(Some(engine));
        assert!(raw(addr, &req).await.starts_with("HTTP/1.1 200"));
        link.set(None);
        assert!(raw(addr, &req).await.starts_with("HTTP/1.1 503"));
        let _ = tx.send(true);
        h.await.unwrap();
    }
}
