//! The engine's model runtime against the real `model_egress.py` and a fake
//! upstream on loopback (no network, no real model): the references reach the
//! egress, the egress reads the key and the endpoint itself, a call goes
//! through and is metered, and neither the key nor the endpoint is ever in a
//! file or a log the engine writes.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use oo_docs::extract::llm::config::{ModelRuntime, validate_put};
use oo_docs::extract::llm::prompt::ChatRequest;
use serde_json::json;

const KEY: &str = "fake-upstream-key-0123456789abcdef";

fn script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/model-egress/model_egress.py")
}

fn private(path: &std::path::Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

fn python_ok() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// One-shot OpenAI style upstream; remembers the request head it saw.
fn upstream() -> (u16, Arc<Mutex<String>>, std::thread::JoinHandle<()>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(String::new()));
    let s2 = seen.clone();
    let h = std::thread::spawn(move || {
        // The egress asks `/models` first only when probing; here one POST.
        for conn in l.incoming().take(1) {
            let mut c = conn.unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = c.read(&mut buf).unwrap();
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got).into_owned();
                if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                    let want: usize = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|v| v.trim().parse().ok())
                        })
                        .unwrap_or(0);
                    if rest.len() >= want {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            *s2.lock().unwrap() = String::from_utf8_lossy(&got).into_owned();
            let body = r#"{"id":"x","object":"chat.completion","model":"m-1","choices":[{"index":0,"message":{"role":"assistant","content":"{\"entities\":[],\"relations\":[]}"},"finish_reason":"stop"}],"usage":{"prompt_tokens":11,"completion_tokens":5}}"#;
            let r = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            c.write_all(r.as_bytes()).unwrap();
        }
    });
    (port, seen, h)
}

#[tokio::test]
async fn the_runtime_starts_the_egress_from_references_and_a_call_goes_through() {
    if !python_ok() || !script().is_file() {
        eprintln!("python3 or the egress script is missing: skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("oo-egress-it-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (up_port, seen, up) = upstream();
    let key_file = dir.join("key");
    let endpoint_file = dir.join("endpoint");
    private(&key_file, KEY);
    private(&endpoint_file, &format!("http://127.0.0.1:{up_port}/v1"));

    let rt = ModelRuntime::open(dir.clone(), Some(script()), None);
    assert_eq!(rt.status(), "not_configured");
    let body = json!({"provider": "openai-compatible",
        "base_url": format!("file:{}", endpoint_file.display()),
        "model": "m-1", "key_ref": format!("file:{}", key_file.display())});
    let parts = validate_put(&body).unwrap();
    let rt2 = rt.clone();
    let cfg = tokio::task::spawn_blocking(move || rt2.put(1, parts))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cfg.revision, 2);
    assert_eq!(
        rt.status(),
        "ok",
        "egress did not start: {:?}",
        rt.last_error()
    );

    let port = rt.port().expect("a port");
    let req = ChatRequest {
        model: "m-1".into(),
        system: "s".into(),
        user: "u".into(),
    };
    let reply = port.complete(&req).await.unwrap();
    assert!(reply.content.contains("entities"));
    assert_eq!((reply.tokens_in, reply.tokens_out), (11, 5));
    let head = seen.lock().unwrap().clone();
    assert!(
        head.contains(&format!("Bearer {KEY}")),
        "the egress, not the engine, sends the key"
    );
    up.join().unwrap();

    // Metered by the egress, owner only.
    let meter = std::fs::read_to_string(dir.join("model-usage.jsonl")).unwrap();
    assert!(meter.contains("\"prompt_tokens\": 11") || meter.contains("\"prompt_tokens\":11"));
    // The engine's own files hold references only: no key, no endpoint value.
    for f in ["model.json", "model-egress.json"] {
        let text = std::fs::read_to_string(dir.join(f)).unwrap();
        assert!(!text.contains(KEY), "{f} holds the key");
        assert!(
            !text.contains(&up_port.to_string()),
            "{f} holds the endpoint"
        );
        let mode = std::fs::metadata(dir.join(f)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{f}");
    }

    // A reference the egress refuses: unavailable, with its fixed code.
    std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let parts = validate_put(&body).unwrap();
    let rt3 = rt.clone();
    tokio::task::spawn_blocking(move || rt3.put(2, parts))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rt.status(), "unavailable");
    assert_eq!(rt.last_error().as_deref(), Some("SECRET_PERMISSIONS"));
    assert!(rt.port().is_none());
    rt.stop();
    let _ = std::fs::remove_dir_all(dir);
}
