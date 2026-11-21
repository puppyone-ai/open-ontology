//! Make what the CLI does to the database pipeline visible to the desktop app (docs-api 1.13).
//!
//! * `publish` tells a running `oo serve` (`POST /v1/docs/events:publish`) that a database source or the
//!   pipeline changed. It is best effort: it never fails the command, prints nothing, gives up after two
//!   seconds and is skipped when no serve runs.
//! * `record_stack` keeps `<state dir>/stacks.json`, the stack files used from any directory, so the app
//!   can find them.
//!
//! Nothing here carries a secret: a source is named, never described.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use super::client::read_connection;
use super::secrets;

/// `stacks.json` protocol marker.
pub const STACKS_PROTOCOL: &str = "oo.stacks/v1";
/// Most stack files kept.
pub const MAX_STACKS: usize = 20;
const TIMEOUT: Duration = Duration::from_secs(2);

/// `OO_DOCS_STATE_DIR`, else the default state directory of `oo serve`.
pub fn default_state() -> Option<PathBuf> {
    std::env::var_os("OO_DOCS_STATE_DIR")
        .map(PathBuf::from)
        .or_else(secrets::default_state_dir)
}

/// The absolute form of a stack path (canonical when the file exists).
pub fn absolute(stack: &Path) -> PathBuf {
    std::fs::canonicalize(stack).unwrap_or_else(|_| {
        if stack.is_absolute() {
            stack.to_path_buf()
        } else {
            std::env::current_dir().map_or_else(|_| stack.to_path_buf(), |d| d.join(stack))
        }
    })
}

/// Add `stack` to `<state>/stacks.json` (most recent first, at most 20, one entry per canonical path).
pub fn record_stack(state: &Path, stack: &Path) -> std::io::Result<()> {
    secrets::ensure_state_dir(state).map_err(|e| std::io::Error::other(e.0))?;
    let path = absolute(stack).display().to_string();
    let now = secrets::rfc3339(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    );
    let mut stacks: Vec<Value> = read_stacks(state)
        .into_iter()
        .filter(|e| e["path"] != path.as_str())
        .collect();
    stacks.insert(0, json!({"path": path, "last_used": now}));
    stacks.truncate(MAX_STACKS);
    let doc = json!({"protocol": STACKS_PROTOCOL, "stacks": stacks});
    secrets::write_secret_atomic(&state.join("stacks.json"), &doc.to_string())
}

/// The entries of `<state>/stacks.json`; empty when it is missing or not ours.
pub fn read_stacks(state: &Path) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(state.join("stacks.json")) else {
        return Vec::new();
    };
    let Ok(doc) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    if doc["protocol"] != STACKS_PROTOCOL {
        return Vec::new();
    }
    doc["stacks"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|e| e["path"].is_string() && e["last_used"].is_string())
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// `POST /v1/docs/events:publish` to the serve of `state`; returns whether the engine accepted it.
pub fn publish(state: &Path, kind: &str, data: &Value) -> bool {
    let Ok((port, token)) = read_connection(state) else {
        return false;
    };
    post(
        port,
        &token,
        &json!({"type": kind, "data": data}).to_string(),
    )
    .unwrap_or(false)
}

fn post(port: u16, token: &str, body: &str) -> std::io::Result<bool> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, TIMEOUT)?;
    s.set_read_timeout(Some(TIMEOUT))?;
    s.set_write_timeout(Some(TIMEOUT))?;
    let request = format!(
        "POST /v1/docs/events:publish HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\n\
         OO-Docs-Client: cli\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(request.as_bytes())?;
    let mut head = [0u8; 16];
    s.read_exact(&mut head)?;
    // "HTTP/1.1 200 ..."
    Ok(head.starts_with(b"HTTP/1.1 200"))
}

/// A source of the stack was added, removed, or the stack was initialized.
pub fn source_changed(state: Option<&Path>, name: &str, action: &str, stack: &Path) {
    let stack = absolute(stack);
    if let Some(state) = state {
        let _ = record_stack(state, &stack);
        let _ = publish(
            state,
            "db.source_changed",
            &json!({"name": name, "action": action, "stack": stack.display().to_string()}),
        );
    }
}

/// `oo init|up|down|build` ended.
pub fn pipeline_changed(
    state: Option<&Path>,
    command: &str,
    ok: bool,
    stack: &Path,
    profile: Option<&str>,
    published: bool,
) {
    let stack = absolute(stack);
    let Some(state) = state else { return };
    if stack.exists() {
        let _ = record_stack(state, &stack);
    }
    let mut data = json!({"command": command, "ok": ok, "stack": stack.display().to_string(), "published": published});
    if let Some(p) = profile {
        data["profile"] = json!(p);
    }
    let _ = publish(state, "db.pipeline_changed", &data);
}

/// Whether the text or JSON report of a finished `oo build` says the data was published.
pub fn build_published(body: &str) -> bool {
    match serde_json::from_str::<Value>(body) {
        Ok(v) => v["status"] == "complete" && v["publication"] == "published",
        Err(_) => body.contains("Publication: \"published\""),
    }
}

/// The profile name a JSON build report carries.
pub fn report_profile(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()?
        .get("profile")?
        .as_str()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::mpsc;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "oo-live-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_registry_is_private_deduplicated_and_most_recent_first() {
        let root = tmp("reg");
        let state = root.join("state");
        let a = root.join("a.yaml");
        let b = root.join("b.yaml");
        std::fs::write(&a, "x").unwrap();
        std::fs::write(&b, "x").unwrap();
        record_stack(&state, &a).unwrap();
        record_stack(&state, &b).unwrap();
        // The same file through another spelling is the same entry, and moves to the front.
        std::fs::create_dir_all(root.join("sub")).unwrap();
        record_stack(&state, &root.join("sub/../a.yaml")).unwrap();
        record_stack(&state, &a).unwrap();
        let got = read_stacks(&state);
        let paths: Vec<&str> = got.iter().map(|e| e["path"].as_str().unwrap()).collect();
        let ca = std::fs::canonicalize(&a).unwrap().display().to_string();
        let cb = std::fs::canonicalize(&b).unwrap().display().to_string();
        assert_eq!(paths, [ca.as_str(), cb.as_str()]);
        let doc: Value =
            serde_json::from_str(&std::fs::read_to_string(state.join("stacks.json")).unwrap())
                .unwrap();
        assert_eq!(doc["protocol"], STACKS_PROTOCOL);
        assert!(
            doc["stacks"][0]["last_used"]
                .as_str()
                .unwrap()
                .ends_with('Z')
        );
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&state.join("stacks.json")), 0o600);
        assert_eq!(mode(&state), 0o700);
        assert!(!state.join("stacks.tmp").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_registry_keeps_twenty_and_ignores_a_foreign_file() {
        let root = tmp("cap");
        let state = root.join("state");
        for i in 0..25 {
            record_stack(&state, &root.join(format!("s{i}.yaml"))).unwrap();
        }
        let got = read_stacks(&state);
        assert_eq!(got.len(), MAX_STACKS);
        assert!(got[0]["path"].as_str().unwrap().ends_with("s24.yaml"));
        std::fs::write(
            state.join("stacks.json"),
            r#"{"protocol":"other","stacks":[1]}"#,
        )
        .unwrap();
        assert!(read_stacks(&state).is_empty());
        std::fs::write(state.join("stacks.json"), "not json").unwrap();
        assert!(read_stacks(&state).is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    /// A serve.json + private token file for a listener on `port`.
    fn serve_state(root: &Path, port: u16) -> PathBuf {
        let state = root.join("state");
        secrets::ensure_state_dir(&state).unwrap();
        let token = state.join("control.token");
        secrets::write_secret_file(&token, "oodt_test\n").unwrap();
        let doc = json!({"protocol": secrets::SERVE_PROTOCOL, "port": port, "token_file": token.display().to_string()});
        secrets::write_secret_file(&state.join("serve.json"), &doc.to_string()).unwrap();
        state
    }

    #[test]
    fn publish_posts_the_event_to_the_running_serve() {
        let root = tmp("pub");
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let state = serve_state(&root, l.local_addr().unwrap().port());
        let (tx, rx) = mpsc::channel();
        let t = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap();
            tx.send(String::from_utf8_lossy(&buf[..n]).into_owned())
                .unwrap();
            s.write_all(
                b"HTTP/1.1 200 OK\r\ncontent-length: 9\r\nconnection: close\r\n\r\n{\"seq\":1}",
            )
            .unwrap();
        });
        let stack = root.join("stack.yaml");
        std::fs::write(&stack, "x").unwrap();
        source_changed(Some(&state), "shop", "added", &stack);
        t.join().unwrap();
        let req = rx.recv().unwrap();
        assert!(
            req.starts_with("POST /v1/docs/events:publish HTTP/1.1\r\n"),
            "{req}"
        );
        assert!(req.contains("Authorization: Bearer oodt_test\r\n"));
        assert!(req.contains("OO-Docs-Client: cli\r\n"));
        let body: Value = serde_json::from_str(req.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["type"], "db.source_changed");
        assert_eq!(body["data"]["name"], "shop");
        assert_eq!(body["data"]["action"], "added");
        assert_eq!(
            body["data"]["stack"],
            std::fs::canonicalize(&stack).unwrap().display().to_string()
        );
        // The registry was updated too.
        assert_eq!(read_stacks(&state).len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn publish_is_silent_without_a_serve_and_does_not_hang_on_a_silent_one() {
        let root = tmp("none");
        let state = root.join("state");
        assert!(!publish(&state, "db.pipeline_changed", &json!({})));
        // A listener that never answers: the call gives up after the timeout.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let state = serve_state(&root, l.local_addr().unwrap().port());
        let started = std::time::Instant::now();
        assert!(!publish(&state, "db.pipeline_changed", &json!({})));
        assert!(started.elapsed() < Duration::from_secs(4));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_pipeline_event_names_the_profile_only_when_known() {
        let root = tmp("pipe");
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let state = serve_state(&root, l.local_addr().unwrap().port());
        let (tx, rx) = mpsc::channel();
        let t = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut s, _) = l.accept().unwrap();
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap();
                tx.send(String::from_utf8_lossy(&buf[..n]).into_owned())
                    .unwrap();
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 9\r\n\r\n{\"seq\":1}");
            }
        });
        let stack = root.join("stack.yaml");
        pipeline_changed(Some(&state), "up", false, &stack, None, false);
        pipeline_changed(Some(&state), "build", true, &stack, Some("shop"), true);
        t.join().unwrap();
        let data = |r: String| -> Value {
            serde_json::from_str::<Value>(r.split("\r\n\r\n").nth(1).unwrap()).unwrap()["data"]
                .clone()
        };
        let first = data(rx.recv().unwrap());
        assert_eq!(first["command"], "up");
        assert_eq!(first["ok"], false);
        assert_eq!(first["published"], false);
        assert!(first.get("profile").is_none());
        let second = data(rx.recv().unwrap());
        assert_eq!(
            (second["profile"].as_str(), second["published"].as_bool()),
            (Some("shop"), Some(true))
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_build_report_says_published_only_when_complete() {
        assert!(build_published(
            r#"{"status":"complete","publication":"published"}"#
        ));
        assert!(!build_published(
            r#"{"status":"blocked","publication":"published"}"#
        ));
        assert!(!build_published(
            r#"{"status":"complete","publication":"not_published"}"#
        ));
        assert!(build_published(
            "done\nPublication: \"published\". Gateway: \"verified\"."
        ));
        assert!(!build_published(
            "Publication: \"not_published\". Gateway: x"
        ));
        assert_eq!(
            report_profile(r#"{"profile":"shop"}"#).as_deref(),
            Some("shop")
        );
        assert_eq!(report_profile("text"), None);
    }
}
