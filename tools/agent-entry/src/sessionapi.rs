use std::fs::{self, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::process::Command;

use crate::config::Config;
use crate::error::{conflict, unavailable, Fault};
use crate::httputil::{cookie_pair, status_line};

#[derive(Clone, Debug)]
pub struct SessionDoor {
    pub cookie: String,
    pub csrf: String,
    pub conversation_id: String,
}

pub async fn open_conversation(config: &Config, principal: &str) -> Result<SessionDoor, Fault> {
    let port = read_port(&config.session_port_file)?;
    let grant_path = Path::new(&config.state_dir).join(format!("grant-{principal}"));
    let _ = fs::remove_file(&grant_path);
    issue_grant(config, principal, &grant_path).await?;
    let grant = fs::read_to_string(&grant_path).map_err(|_| unavailable("grant"))?;
    let _ = fs::remove_file(&grant_path);
    let grant = grant.trim();
    if grant.len() != 64 || !grant.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
        return Err(unavailable("grant"));
    }
    let (status, headers, body) = session_http(port, "POST", "/control/v1/entry/session/consume", "", "", &json!({"grant": grant}).to_string()).await?;
    if status != 200 {
        return Err(unavailable("consume"));
    }
    let cookie = headers.iter().find_map(|value| cookie_pair(value, "oo-entry")).ok_or_else(|| unavailable("consume"))?;
    let parsed: Value = serde_json::from_slice(&body).map_err(|_| unavailable("consume"))?;
    let csrf = parsed.get("csrf").and_then(Value::as_str).ok_or_else(|| unavailable("consume"))?.to_owned();
    let (status, _, body) = session_http(port, "POST", "/control/v1/conversations", &cookie, &csrf, "{}").await?;
    if status != 201 {
        return Err(unavailable("conversation"));
    }
    let parsed: Value = serde_json::from_slice(&body).map_err(|_| unavailable("conversation"))?;
    let conversation_id = parsed.get("conversation_id").and_then(Value::as_str).ok_or_else(|| unavailable("conversation"))?.to_owned();
    if parsed.get("production_signed").and_then(Value::as_bool) != Some(false) {
        return Err(conflict("production_signed"));
    }
    Ok(SessionDoor { cookie, csrf, conversation_id })
}

pub async fn release_close(config: &Config, door: &SessionDoor) -> Result<(u16, Value), Fault> {
    let port = read_port(&config.session_port_file)?;
    let path = format!("/control/v1/conversations/{}/turns", door.conversation_id);
    let body = json!({"turn_id": "release-fixture", "text": crate::turn::RELEASE_FIXTURE}).to_string();
    let (status, _, turned) = session_http(port, "POST", &path, &door.cookie, &door.csrf, &body).await?;
    if status != 200 {
        return Err(conflict("release_fixture"));
    }
    let _ = turned;
    let cancel = format!("/control/v1/conversations/{}/cancel", door.conversation_id);
    let (status, _, body) = session_http(port, "POST", &cancel, &door.cookie, &door.csrf, "{}").await?;
    let receipt: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    Ok((status, receipt))
}

pub fn receipt_closed(status: u16, receipt: &Value) -> bool {
    status == 200
        && receipt.get("phase").and_then(Value::as_str) == Some("closed")
        && receipt.get("production_signed").and_then(Value::as_bool) == Some(false)
        && receipt.get("deployment").and_then(Value::as_str) == Some("localprocess")
        && receipt.get("retryable").and_then(Value::as_bool) == Some(false)
        && s8_absent(receipt)
}

fn s8_absent(receipt: &Value) -> bool {
    let Some(steps) = receipt.get("steps").and_then(Value::as_array) else {
        return false;
    };
    if steps.len() != 11 {
        return false;
    }
    steps.iter().any(|step| {
        step.get("id").and_then(Value::as_str) == Some("s8")
            && step.get("status").and_then(Value::as_str) == Some("confirmed")
            && step.get("actual_absence").and_then(Value::as_bool) == Some(true)
    })
}

async fn issue_grant(config: &Config, principal: &str, grant_out: &Path) -> Result<(), Fault> {
    let bytes = fs::read(&config.session_config_path).map_err(|_| unavailable("session_config"))?;
    let mut value: Value = serde_json::from_slice(&bytes).map_err(|_| unavailable("session_config"))?;
    value["issue"] = json!({
        "principal_id": principal,
        "project_id": "shop",
        "grant_out": grant_out,
    });
    let temp = Path::new(&config.state_dir).join(format!("issue-{principal}.json"));
    let _ = fs::remove_file(&temp);
    {
        let mut file = OpenOptions::new().create_new(true).write(true).mode(0o600).open(&temp).map_err(|_| unavailable("issue"))?;
        use std::io::Write;
        file.write_all(&serde_json::to_vec(&value).map_err(|_| unavailable("issue"))?).map_err(|_| unavailable("issue"))?;
        file.sync_all().map_err(|_| unavailable("issue"))?;
    }
    let mut child = Command::new(&config.session_bin)
        .arg("issue-grant")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| unavailable("issue"))?;
    let mut stdin = child.stdin.take().ok_or_else(|| unavailable("issue"))?;
    let payload = fs::read(&temp).map_err(|_| unavailable("issue"))?;
    stdin.write_all(&payload).await.map_err(|_| unavailable("issue"))?;
    drop(stdin);
    let finished = tokio::time::timeout(Duration::from_secs(30), child.wait()).await.map_err(|_| unavailable("issue"))?.map_err(|_| unavailable("issue"))?;
    let _ = fs::remove_file(&temp);
    if finished.success() { Ok(()) } else { Err(unavailable("issue")) }
}

fn read_port(path: &str) -> Result<u16, Fault> {
    let text = fs::read_to_string(path).map_err(|_| unavailable("session_port"))?;
    text.trim().parse().map_err(|_| unavailable("session_port"))
}

async fn session_http(port: u16, method: &str, path: &str, cookie: &str, csrf: &str, body: &str) -> Result<(u16, Vec<String>, Vec<u8>), Fault> {
    let mut extra = Vec::new();
    if !cookie.is_empty() {
        extra.push(("cookie", format!("oo-entry={cookie}")));
    }
    if !csrf.is_empty() {
        extra.push(("x-oo-csrf", csrf.to_owned()));
    }
    let pairs: Vec<(&str, &str)> = extra.iter().map(|(key, value)| (*key, value.as_str())).collect();
    let request = build_request(method, path, &pairs, body.as_bytes());
    let mut sock = TcpStream::connect(("127.0.0.1", port)).await.map_err(|_| unavailable("session"))?;
    use tokio::io::AsyncReadExt;
    sock.write_all(&request).await.map_err(|_| unavailable("session"))?;
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(60), async {
        let mut chunk = [0_u8; 8192];
        loop {
            let n = sock.read(&mut chunk).await.map_err(|_| unavailable("session"))?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.len() > 1_048_576 {
                break;
            }
        }
        Ok::<(), Fault>(())
    })
    .await
    .map_err(|_| unavailable("session"))??;
    let (status, headers, body) = status_line(&buf).ok_or_else(|| unavailable("session"))?;
    let set_cookies: Vec<String> = headers.iter().filter(|(name, _)| name == "set-cookie").map(|(_, value)| value.clone()).collect();
    Ok((status, set_cookies, body))
}

fn build_request(method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("{method} {path} HTTP/1.1\r\nhost: 127.0.0.1\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n", body.len());
    for (name, value) in headers {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}
