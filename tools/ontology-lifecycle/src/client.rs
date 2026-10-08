//! Authenticated loopback client for a producer operation that already matched.
//!
//! The cookie and CSRF token never enter an error string. The client refuses
//! every host other than 127.0.0.1 and does not follow a redirect.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpStream};
use std::time::Duration;

use crate::discover::Operation;
use crate::sha256::{hex, sha256};

pub struct Session {
    pub cookie: String,
    pub csrf: String,
    pub origin: String,
}

pub struct SavedEdge {
    pub response_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drive {
    pub status: &'static str,
    pub detail: &'static str,
    pub request_sha256: String,
    pub response_sha256: String,
    pub mutations: u32,
    pub reads: u32,
}

pub fn drive(
    oc: &str,
    operation: &Operation,
    session: &Session,
    endpoint: &str,
    identity: &str,
    previous: Option<&SavedEdge>,
) -> Drive {
    let empty = Drive {
        status: "unauthenticated",
        detail: "session",
        request_sha256: String::new(),
        response_sha256: String::new(),
        mutations: 0,
        reads: 0,
    };
    if !session_ok(session) || !loopback_base(endpoint) || !loopback_base(identity) {
        return empty;
    }
    if !operation.path.starts_with("/control/") || operation.path.contains("..") {
        return Drive { status: "endpoint", detail: "path", ..empty };
    }
    let body = request_body(oc);
    let request_sha256 = hex(&sha256(format!("{}\n{}\n{body}", operation.method, operation.path).as_bytes()));
    if let Some(saved) = previous {
        return replay(operation, session, endpoint, &request_sha256, saved);
    }
    let identity_path = "/control/v1/identity/current";
    let identity_response = match exchange("GET", identity, identity_path, session, "") {
        Ok(response) => response,
        Err(detail) => {
            return Drive {
                status: "unavailable",
                detail,
                request_sha256,
                reads: 1,
                ..empty
            };
        }
    };
    if identity_response.status != 200 || !identity_ok(&identity_response.body, &session.csrf) {
        return Drive {
            status: "unauthenticated",
            detail: "identity",
            request_sha256,
            reads: 1,
            ..empty
        };
    }
    let response = match exchange(&operation.method, endpoint, &operation.path, session, body) {
        Ok(response) => response,
        Err(detail) => {
            return Drive {
                status: "unavailable",
                detail,
                request_sha256,
                reads: 1,
                mutations: 1,
                ..empty
            };
        }
    };
    if response.status != 200 && response.status != 202 {
        return Drive {
            status: "refused",
            detail: "producer",
            request_sha256,
            reads: 1,
            mutations: 1,
            ..empty
        };
    }
    match accept(oc, &response.body) {
        Ok(()) => Drive {
            status: "executed",
            detail: "response_accepted",
            request_sha256,
            response_sha256: hex(&sha256(response.body.as_bytes())),
            mutations: 1,
            reads: 1,
        },
        Err(detail) => Drive {
            status: "refused",
            detail,
            request_sha256,
            response_sha256: hex(&sha256(response.body.as_bytes())),
            mutations: 1,
            reads: 1,
        },
    }
}

fn replay(operation: &Operation, session: &Session, endpoint: &str, request_sha256: &str, saved: &SavedEdge) -> Drive {
    let response = match exchange("GET", endpoint, &operation.path, session, "") {
        Ok(response) => response,
        Err(detail) => {
            return Drive {
                status: "unavailable",
                detail,
                request_sha256: request_sha256.to_owned(),
                response_sha256: String::new(),
                mutations: 0,
                reads: 1,
            };
        }
    };
    let response_sha256 = hex(&sha256(response.body.as_bytes()));
    if response.status == 200 && response_sha256 == saved.response_sha256 {
        Drive {
            status: "recovered",
            detail: "read_back",
            request_sha256: request_sha256.to_owned(),
            response_sha256,
            mutations: 0,
            reads: 1,
        }
    } else {
        Drive {
            status: "conflict",
            detail: "read_back",
            request_sha256: request_sha256.to_owned(),
            response_sha256,
            mutations: 0,
            reads: 1,
        }
    }
}

pub fn session_ok(session: &Session) -> bool {
    token(&session.cookie, 16, 256) && token(&session.csrf, 43, 43) && loopback_origin(&session.origin)
}

fn token(value: &str, min: usize, max: usize) -> bool {
    let len = value.len();
    (min..=max).contains(&len)
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn loopback_origin(origin: &str) -> bool {
    let Some(rest) = origin.strip_prefix("http://127.0.0.1") else {
        return false;
    };
    rest.is_empty() || (rest.starts_with(':') && rest[1..].bytes().all(|byte| byte.is_ascii_digit()) && rest != ":")
}

fn loopback_base(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("http://127.0.0.1:") else {
        return false;
    };
    let port: u16 = match rest.parse() {
        Ok(port) => port,
        Err(_) => return false,
    };
    port != 0
}

fn identity_ok(body: &str, csrf: &str) -> bool {
    body.contains("\"authenticated\":true")
        && !body.contains("\"authenticated\":false")
        && body.contains(&format!("\"csrf\":\"{csrf}\""))
}

fn request_body(oc: &str) -> &'static str {
    match oc {
        "OC-01" => {
            "{\"source\":\"approved_private\",\"tables\":[\"shop.party\",\"shop.account\",\"shop.entry\"]}"
        }
        "OC-02" => "{\"choices\":[\"approve\",\"reject\",\"defer\"]}",
        "OC-03" => "{\"ref\":\"refs/heads/curated\"}",
        "OC-04" => "{\"scope\":\"test_scope\",\"role\":\"oo_r_wide\"}",
        "OC-05" => "{\"role\":\"oo_r_wide\"}",
        _ => "{}",
    }
}

pub fn accept(oc: &str, body: &str) -> Result<(), &'static str> {
    match oc {
        "OC-01" => accept_generated(body),
        "OC-02" => accept_review(body),
        "OC-03" => accept_git(body),
        "OC-04" => accept_release(body),
        "OC-05" => accept_agent(body),
        _ => Err("edge"),
    }
}

fn accept_generated(body: &str) -> Result<(), &'static str> {
    if !body.contains("\"spec_version\":\"oos/v1\"") {
        return Err("spec_version");
    }
    require_hex(body, "ontology_yaml_sha256")?;
    require_hex(body, "source_seal_sha256")?;
    let party = count_of(body, "shop.party")?;
    let account = count_of(body, "shop.account")?;
    let entry = count_of(body, "shop.entry")?;
    if party != 2000 || account != 8000 || entry != 90000 {
        return Err("table_row_counts");
    }
    if body.contains("\"row_count\":100000") || body.contains("\"row_count\": 100000") {
        return Err("per_file_total");
    }
    Ok(())
}

fn accept_review(body: &str) -> Result<(), &'static str> {
    if body.contains("\"ledger_authenticated\":false") || body.contains("\"ledger_authenticated\": false") {
        return Err("ledger");
    }
    require_hex(body, "decisions_yaml_sha256")?;
    require_hex(body, "curated_overlay_sha256")?;
    require_hex(body, "effective_yaml_sha256")?;
    let reseal = count_of(body, "reseal_count")?;
    if reseal < 1 {
        return Err("reseal");
    }
    for word in ["\"approve\"", "\"reject\"", "\"defer\""] {
        if !body.contains(word) {
            return Err("decision");
        }
    }
    Ok(())
}

fn accept_git(body: &str) -> Result<(), &'static str> {
    if body.contains("\"authenticated\":false") || body.contains("\"authenticated\": false") {
        return Err("authenticated");
    }
    if !body.contains("\"authenticated\":true") {
        return Err("authenticated");
    }
    if body.contains("\"new_commit\":null") || body.contains("\"new_commit\": null") {
        return Err("commit");
    }
    require_any(body, &["commit", "new_commit"], &[40, 64])?;
    require_any(body, &["tree", "new_tree"], &[40, 64])?;
    require_any(body, &["expected_head", "old_commit"], &[40, 64])?;
    require_token(body, "ontology_blob", &[40, 64])?;
    require_token(body, "decisions_blob", &[40, 64])?;
    if require_token(body, "curated_blob", &[40, 64]).is_err() {
        require_token(body, "new_blob", &[40, 64])?;
    }
    Ok(())
}

fn require_any(body: &str, keys: &[&str], lengths: &[usize]) -> Result<(), &'static str> {
    if keys.iter().any(|key| require_token(body, key, lengths).is_ok()) {
        Ok(())
    } else {
        Err("hash")
    }
}

fn accept_release(body: &str) -> Result<(), &'static str> {
    if body.contains("\"production_ready\":true") || body.contains("\"certificate_class\":\"production\"") {
        return Err("production");
    }
    if !body.contains("\"certificate_class\":\"test_scope_not_production\"") || !body.contains("\"scope\":\"test_scope\"") {
        return Err("fixture");
    }
    require_token(body, "source_commit", &[40, 64])?;
    for token in ["\"role_fence\"", "\"replay\"", "\"active_cas\"", "\"G1\"", "\"G5a\"", "\"G5b\"", "\"G13\""] {
        if !body.contains(token) {
            return Err("gates");
        }
    }
    Ok(())
}

fn accept_agent(body: &str) -> Result<(), &'static str> {
    if body.contains("\"compiled_pack\":\"published\"")
        || body.contains("\"compiled_pack\":\"not_published\"")
        || body.contains("\"private_fixed\"")
    {
        return Err("pack");
    }
    if !body.contains("\"active\":true") {
        return Err("active");
    }
    require_hex(body, "pack_sha256")?;
    require_token(body, "source_commit", &[40, 64])?;
    require_hex(body, "policy_sha256")?;
    require_hex(body, "knowledge_sha256")?;
    require_hex(body, "content_sha256")?;
    Ok(())
}

fn require_hex(body: &str, key: &str) -> Result<(), &'static str> {
    require_token(body, key, &[64])
}

fn require_token(body: &str, key: &str, lengths: &[usize]) -> Result<(), &'static str> {
    let marker = format!("\"{key}\":\"");
    let Some(start) = body.find(&marker) else {
        return Err("hash");
    };
    let rest = &body[start + marker.len()..];
    let value = rest.split('"').next().unwrap_or("");
    if lengths.contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("hash")
    }
}

fn count_of(body: &str, key: &str) -> Result<u64, &'static str> {
    let quoted = format!("\"{key}\":");
    let Some(start) = body.find(&quoted) else {
        return Err("count");
    };
    let rest = body[start + quoted.len()..].trim_start();
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().map_err(|_| "count")
}

struct Reply {
    status: u16,
    body: String,
}

fn exchange(method: &str, base: &str, path: &str, session: &Session, body: &str) -> Result<Reply, &'static str> {
    let port: u16 = base
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse().ok())
        .ok_or("endpoint")?;
    let mut stream = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(500),
    )
    .map_err(|_| "unavailable")?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).map_err(|_| "unavailable")?;
    stream.set_write_timeout(Some(Duration::from_secs(2))).map_err(|_| "unavailable")?;
    let length = body.len();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: __Host-oo-session={cookie}\r\nX-OO-CSRF: {csrf}\r\nOrigin: {origin}\r\ncontent-type: application/json\r\ncontent-length: {length}\r\nconnection: close\r\n\r\n",
        cookie = session.cookie,
        csrf = session.csrf,
        origin = session.origin,
    );
    stream.write_all(head.as_bytes()).map_err(|_| "unavailable")?;
    stream.write_all(body.as_bytes()).map_err(|_| "unavailable")?;
    stream.flush().map_err(|_| "unavailable")?;
    let _ = stream.shutdown(Shutdown::Write);
    read_reply(&mut stream)
}

fn read_reply(stream: &mut TcpStream) -> Result<Reply, &'static str> {
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 1024];
    loop {
        if buf.len() > 65_536 {
            return Err("malformed");
        }
        let n = stream.read(&mut tmp).map_err(|_| "unavailable")?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let text = String::from_utf8_lossy(&buf);
    let (head, body) = text.split_once("\r\n\r\n").ok_or("malformed")?;
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .ok_or("malformed")?
        .parse()
        .map_err(|_| "malformed")?;
    if let Some(length) = content_length(head)
        && body.len() != length
    {
        return Err("malformed");
    }
    Ok(Reply { status, body: body.to_owned() })
}

fn content_length(head: &str) -> Option<usize> {
    for line in head.split("\r\n").skip(1) {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use super::{accept, drive, session_ok, Drive, SavedEdge, Session};
    use crate::discover::Operation;
    use crate::sha256::{hex, sha256};

    fn csrf() -> String {
        "a".repeat(43)
    }

    fn session(origin: &str) -> Session {
        Session {
            cookie: "cookievaluecookievalue".to_owned(),
            csrf: csrf(),
            origin: origin.to_owned(),
        }
    }

    fn operation() -> Operation {
        Operation {
            id: "materializeGeneratedYaml".to_owned(),
            method: "POST".to_owned(),
            path: "/control/v1/ontology/generated".to_owned(),
        }
    }

    fn hash() -> &'static str {
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    }

    fn good_body() -> String {
        format!(
            "{{\"spec_version\":\"oos/v1\",\"ontology_yaml_sha256\":\"{h}\",\"source_seal_sha256\":\"{h}\",\"table_row_counts\":{{\"shop.party\":2000,\"shop.account\":8000,\"shop.entry\":90000}},\"generated_files\":[{{\"path\":\"ontology/generated/datasets/shop/party.yaml\",\"sha256\":\"{h}\"}}]}}",
            h = hash()
        )
    }

    fn serve(
        listener: TcpListener,
        identity: String,
        post: String,
        fail_identity: bool,
        rounds: usize,
    ) -> thread::JoinHandle<Vec<String>> {
        thread::spawn(move || {
            let mut seen = Vec::new();
            for _ in 0..rounds {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).expect("timeout");
                let mut buf = String::new();
                let _ = stream.read_to_string(&mut buf);
                let first = buf.lines().next().unwrap_or("").to_owned();
                seen.push(first.clone());
                let body = if first.starts_with("GET /control/v1/identity/current") {
                    if fail_identity {
                        reply(401, "{\"authenticated\":false}")
                    } else if buf.contains("Cookie: __Host-oo-session=cookievaluecookievalue")
                        && buf.contains(&format!("X-OO-CSRF: {}", csrf()))
                    {
                        reply(200, &identity)
                    } else {
                        reply(401, "{\"authenticated\":false}")
                    }
                } else if first.starts_with("POST ") || first.starts_with("GET /control/v1/ontology") {
                    reply(200, &post)
                } else {
                    reply(404, "{}")
                };
                let _ = stream.write_all(body.as_bytes());
            }
            seen
        })
    }

    fn reply(status: u16, body: &str) -> String {
        format!(
            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn a_file_row_count_of_the_fixture_total_is_refused() {
        let mut bad = good_body();
        bad.pop();
        bad.push_str(",\"row_count\":100000}");
        assert_eq!(accept("OC-01", &bad), Err("per_file_total"));
        assert_eq!(accept("OC-01", &good_body()), Ok(()));
    }

    #[test]
    fn the_client_posts_only_after_identity_and_keeps_the_chain_edge_local() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let base = format!("http://127.0.0.1:{port}");
        let identity = format!(
            "{{\"contract\":\"oo.control-identity/v1\",\"authenticated\":true,\"csrf\":\"{}\"}}",
            csrf()
        );
        let server = serve(listener, identity, good_body(), false, 2);
        let result = drive("OC-01", &operation(), &session(&base), &base, &base, None);
        let seen = server.join().expect("server");
        assert_eq!(result.status, "executed");
        assert_eq!(result.mutations, 1);
        assert_eq!(seen[0], "GET /control/v1/identity/current HTTP/1.1");
        assert!(seen[1].starts_with("POST /control/v1/ontology/generated "));
        assert_eq!(result.response_sha256, hex(&sha256(good_body().as_bytes())));
    }

    #[test]
    fn identity_failure_does_not_post() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let base = format!("http://127.0.0.1:{port}");
        let server = serve(listener, "{}".to_owned(), good_body(), true, 1);
        let result = drive("OC-01", &operation(), &session(&base), &base, &base, None);
        let seen = server.join().expect("server");
        assert_eq!(result.status, "unauthenticated");
        assert_eq!(result.mutations, 0);
        assert_eq!(seen.len(), 1);
    }

    #[test]
    fn recovery_reads_and_does_not_post_again() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let base = format!("http://127.0.0.1:{port}");
        let body = good_body();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).expect("timeout");
            let mut buf = String::new();
            let _ = stream.read_to_string(&mut buf);
            let response = reply(200, &body);
            let _ = stream.write_all(response.as_bytes());
            buf.lines().next().unwrap_or("").to_owned()
        });
        let saved = SavedEdge { response_sha256: hex(&sha256(good_body().as_bytes())) };
        let result = drive("OC-01", &operation(), &session(&base), &base, &base, Some(&saved));
        let first = server.join().expect("server");
        assert_eq!(result.status, "recovered");
        assert_eq!(result.mutations, 0);
        assert!(first.starts_with("GET /control/v1/ontology/generated "));
    }

    #[test]
    fn a_missing_session_does_not_connect() {
        let session = Session { cookie: String::new(), csrf: csrf(), origin: "http://127.0.0.1:9".to_owned() };
        assert!(!session_ok(&session));
        let result: Drive = drive(
            "OC-01",
            &operation(),
            &session,
            "http://127.0.0.1:9",
            "http://127.0.0.1:9",
            None,
        );
        assert_eq!(result.status, "unauthenticated");
        assert_eq!(result.mutations, 0);
        assert_eq!(result.reads, 0);
    }

    #[test]
    fn a_reviewed_tree_receipt_is_a_commit_and_a_null_commit_is_not() {
        let id = "0123456789abcdef0123456789abcdef01234567";
        let receipt = format!(
            "{{\"authenticated\":true,\"old_commit\":\"{id}\",\"new_commit\":\"{id}\",\"new_tree\":\"{id}\",\"ontology_blob\":\"{id}\",\"decisions_blob\":\"{id}\",\"blobs\":[{{\"new_blob\":\"{id}\"}}]}}"
        );
        assert!(accept("OC-03", &receipt).is_ok());
        let unchanged = receipt.replace(&format!("\"new_commit\":\"{id}\""), "\"new_commit\":null");
        assert_eq!(accept("OC-03", &unchanged), Err("commit"));
        let e57 = receipt.replace("\"authenticated\":true", "\"authenticated\":false");
        assert_eq!(accept("OC-03", &e57), Err("authenticated"));
    }
}
