//! L5 over MCP (B-S7-04, B-S7-07) against a mock docs engine on loopback: the `suggest` and `context_check` tools with
//! their annotations, the share credential as the only bearer, the scope refusal, and `puppyone://share/<sid>/updates`
//! (read, subscribe, a notification within 2 s of an appended entry).

#![forbid(unsafe_code)]
#![allow(deprecated)]

use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use rmcp::ClientHandler;
use rmcp::ClientLifecycleMode;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation, ProtocolVersion,
    ReadResourceRequestParams, ResourceUpdatedNotificationParam, SubscribeRequestParams,
};
use rmcp::service::{ClientServiceExt, NotificationContext, RoleClient};
use rmcp::transport::TokioChildProcess;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::process::Command;

const SID: &str = "00000000-0000-4000-8000-000000000001";
const CRED: &str = "oodr_test_credential_value";

#[derive(Default)]
struct Engine {
    /// `METHOD path|authorization|body` of every request.
    seen: Mutex<Vec<(String, String, String)>>,
    updates: AtomicUsize,
    suggest_status: AtomicUsize,
}

async fn serve(engine: Arc<Engine>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            let engine = engine.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16384];
                let mut n = 0;
                loop {
                    let r = s.read(&mut buf[n..]).await.unwrap_or(0);
                    if r == 0 {
                        break;
                    }
                    n += r;
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    if let Some(h) = text.find("\r\n\r\n") {
                        let cl = text
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if n >= h + 4 + cl {
                            break;
                        }
                    }
                }
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                let first = text.lines().next().unwrap_or("").to_owned();
                let auth = text
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("authorization:"))
                    .unwrap_or("")
                    .to_owned();
                let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_owned();
                let path = first.split(' ').nth(1).unwrap_or("").to_owned();
                engine
                    .seen
                    .lock()
                    .unwrap()
                    .push((first.clone(), auth, body));
                let (status, payload): (u16, Value) = if first
                    .starts_with("POST /v1/docs/suggestions")
                {
                    match engine.suggest_status.load(Ordering::SeqCst) {
                        0 => (
                            200,
                            json!({"share_id": SID, "accepted": 1, "rejected": 0, "results": [{"status": "auto_accepted"}]}),
                        ),
                        _ => (
                            403,
                            json!({"error": {"code": "DOCS_SUGGEST_SCOPE_MISSING"}}),
                        ),
                    }
                } else if first.starts_with("POST") && path.contains(":check") {
                    (
                        200,
                        json!({"exit_code": 10, "action": "refresh_recommended", "items": []}),
                    )
                } else if path.contains("/updates") {
                    let n = engine.updates.load(Ordering::SeqCst);
                    let items: Vec<Value> = (1..=n)
                        .map(|i| json!({"seq": i, "level": "warn"}))
                        .collect();
                    (200, json!({"items": items, "next_cursor": null}))
                } else {
                    (404, json!({"error": {"code": "DOCS_ENGINE_NOT_FOUND"}}))
                };
                let body = payload.to_string();
                let reason = if status == 200 { "OK" } else { "Error" };
                let resp = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    port
}

struct Notes {
    tx: tokio::sync::mpsc::UnboundedSender<String>,
}

impl ClientHandler for Notes {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new("oo-l5-client", "0.1.0"),
        )
        .with_protocol_version(ProtocolVersion::V_2025_11_25)
    }

    fn on_resource_updated(
        &self,
        params: ResourceUpdatedNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + rmcp::service::MaybeSendFuture + '_ {
        let _ = self.tx.send(params.uri);
        std::future::ready(())
    }
}

async fn session(
    port: u16,
) -> (
    rmcp::service::RunningService<RoleClient, Notes>,
    tokio::sync::mpsc::UnboundedReceiver<String>,
) {
    let dir = std::env::temp_dir().join(format!("oo-l5-mcp-{}-{port}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cred = dir.join("credential");
    std::fs::write(&cred, format!("{CRED}\n")).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcp-gateway"));
    command
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("OO_MCP_DOCS_URL", format!("http://127.0.0.1:{port}"))
        .env("OO_MCP_DOCS_CREDENTIAL_FILE", &cred)
        .env("OO_MCP_DOCS_SHARE_ID", SID)
        .kill_on_drop(true);
    let (transport, _) = TokioChildProcess::builder(command).spawn().unwrap();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let running = Notes { tx }
        .serve_with_lifecycle(transport, ClientLifecycleMode::Initialize)
        .await
        .unwrap();
    (running, rx)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tools_and_scope() {
    let engine = Arc::new(Engine::default());
    let port = serve(engine.clone()).await;
    let (running, _rx) = session(port).await;
    let tools = running.peer().list_all_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(
        names,
        ["suggest", "context_check"],
        "docs mode advertises only the L5 tools"
    );
    let s = tools
        .iter()
        .find(|t| t.name == "suggest")
        .unwrap()
        .annotations
        .clone()
        .unwrap();
    assert_eq!(
        (s.read_only_hint, s.destructive_hint, s.idempotent_hint),
        (Some(false), Some(false), Some(true))
    );
    let c = tools
        .iter()
        .find(|t| t.name == "context_check")
        .unwrap()
        .annotations
        .clone()
        .unwrap();
    assert_eq!(c.read_only_hint, Some(true));

    let args = json!({"kind": "new_fact", "subject": "星火计划", "predicate": "attr.status", "value": "进行中",
                      "rationale": "周会", "idempotency_key": "idem-0001"});
    let r = running
        .peer()
        .call_tool(
            CallToolRequestParams::new("suggest").with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    let r = serde_json::to_value(r).unwrap();
    assert_eq!(
        r["structuredContent"]["results"][0]["status"], "auto_accepted",
        "{r}"
    );
    {
        let seen = engine.seen.lock().unwrap();
        let (first, auth, body) = seen.last().unwrap();
        assert!(first.starts_with("POST /v1/docs/suggestions "), "{first}");
        assert!(
            auth.contains(&format!("Bearer {CRED}")),
            "the share's credential is the bearer"
        );
        let b: Value = serde_json::from_str(body).unwrap();
        assert_eq!(b["protocol"], "oo.suggest/v1");
        assert!(
            b.get("share_id").is_none(),
            "the share comes from the credential"
        );
    }
    // The engine refuses (no suggest:write): the tool answers with an error result, not a success.
    engine.suggest_status.store(1, Ordering::SeqCst);
    let r = running
        .peer()
        .call_tool(
            CallToolRequestParams::new("suggest").with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    let r = serde_json::to_value(r).unwrap();
    assert_eq!(r["isError"], true, "{r}");
    assert_eq!(
        r["structuredContent"]["error"]["code"],
        "DOCS_SUGGEST_SCOPE_MISSING"
    );
    // Arguments are checked before they leave: a share_id argument is refused.
    let mut bad = args.as_object().unwrap().clone();
    bad.insert(
        "share_id".into(),
        json!("00000000-0000-4000-8000-000000000002"),
    );
    assert!(
        running
            .peer()
            .call_tool(CallToolRequestParams::new("suggest").with_arguments(bad))
            .await
            .is_err()
    );
    // context_check goes to the share's own :check.
    let r = running
        .peer()
        .call_tool(CallToolRequestParams::new("context_check"))
        .await
        .unwrap();
    let r = serde_json::to_value(r).unwrap();
    assert_eq!(r["structuredContent"]["exit_code"], 10);
    let seen = engine.seen.lock().unwrap();
    assert!(
        seen.iter()
            .any(|(f, _, _)| f.starts_with(&format!("POST /v1/docs/shares/{SID}:check?since=0 ")))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_resource_is_readable_and_notifies_within_two_seconds() {
    let engine = Arc::new(Engine::default());
    let port = serve(engine.clone()).await;
    let (running, mut rx) = session(port).await;
    let uri = format!("puppyone://share/{SID}/updates");
    let resources = running.peer().list_all_resources().await.unwrap();
    assert!(resources.iter().any(|r| r.uri == uri));
    // Another share's updates are not readable.
    let other = "puppyone://share/00000000-0000-4000-8000-000000000002/updates";
    assert!(
        running
            .peer()
            .read_resource(ReadResourceRequestParams::new(other))
            .await
            .is_err()
    );
    let read = running
        .peer()
        .read_resource(ReadResourceRequestParams::new(uri.clone()))
        .await
        .unwrap();
    assert!(serde_json::to_string(&read).unwrap().contains("items"));

    running
        .peer()
        .subscribe(SubscribeRequestParams::new(uri.clone()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await; // the watcher has its first reading
    let t0 = Instant::now();
    engine.updates.store(1, Ordering::SeqCst); // the engine appended an entry
    let got = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("no notification within 2 s");
    assert_eq!(got.as_deref(), Some(uri.as_str()));
    assert!(t0.elapsed() < Duration::from_secs(2));
}
