//! Real `rmcp` child-process checks that do not need PostgreSQL.
//!
//! `2026-07-28` is reached with `server/discover`. Initialize accepts only
//! `2025-11-25`.

#![forbid(unsafe_code)]

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientConfig, ErrorCode, Implementation,
    PaginatedRequestParams, ProtocolVersion,
};
use rmcp::service::{ClientInitializeError, ClientServiceExt, RoleClient, RunningService, ServiceError};
use rmcp::transport::TokioChildProcess;
use rmcp::ClientLifecycleMode;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

struct Session {
    running: RunningService<RoleClient, ClientConfig>,
    stderr: tokio::task::JoinHandle<Vec<u8>>,
}

impl Session {
    async fn finish(self) -> String {
        drop(self.running);
        let bytes = self.stderr.await.expect("stderr task");
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

fn client_config(version: ProtocolVersion) -> ClientConfig {
    ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("oo-a388-client", "0.1.0"),
    )
    .with_protocol_version(version)
}

async fn spawn(lifecycle: ClientLifecycleMode, version: ProtocolVersion) -> Result<Session, ClientInitializeError> {
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("oo-a388-proto-{}-{seq}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let token = dir.join("token");
    std::fs::write(&token, b"local-proof-token\n").expect("token file");
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcp-gateway"));
    command.env_clear();
    command.env("PATH", std::env::var("PATH").unwrap_or_default());
    command.env("OO_MCP_GATEWAY_URL", "http://127.0.0.1:9");
    command.env("OO_MCP_TOKEN_FILE", &token);
    command.env("OO_MCP_TOKEN", "ooct_decoy_must_not_be_logged");
    command.kill_on_drop(true);
    let (transport, stderr) = TokioChildProcess::builder(command)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn mcp-gateway");
    assert_process_has_no_bearer(transport.id().expect("pid"), &token);
    let mut stderr = stderr.expect("stderr pipe");
    let stderr = tokio::spawn(async move {
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes).await;
        bytes
    });
    let running = client_config(version)
        .serve_with_lifecycle(transport, lifecycle)
        .await?;
    Ok(Session { running, stderr })
}

fn assert_process_has_no_bearer(pid: u32, token_file: &Path) {
    if !cfg!(target_os = "linux") {
        let _ = (pid, token_file);
        return;
    }
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).expect("cmdline");
    let environ = std::fs::read(format!("/proc/{pid}/environ")).expect("environ");
    assert!(!contains_bytes(&cmdline, b"ooct_"), "bearer prefix in argv");
    assert!(!contains_bytes(&cmdline, b"local-proof-token"), "token file contents in argv");
    assert!(!contains_bytes(&environ, b"local-proof-token"), "token file contents in environ");
    let file = token_file.to_string_lossy();
    assert!(
        contains_bytes(&environ, file.as_bytes()),
        "token path missing from environ"
    );
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

fn assert_stderr(stderr: &str) {
    assert!(
        stderr.contains("oo-mcp-gateway: stdio ready oauth=false remote_export=disabled"),
        "ready line missing"
    );
    assert!(!stderr.contains("jsonrpc"), "protocol bytes on stderr");
    assert!(!stderr.contains("ooct_"), "decoy bearer logged");
    assert!(!stderr.contains("local-proof-token"), "token file contents logged");
}

fn assert_tools(tools: &[rmcp::model::Tool]) {
    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert_eq!(names, ["session_current", "pack_metadata", "query"]);
    for tool in tools {
        let annotations = tool.annotations.as_ref().expect("annotations");
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.idempotent_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(false));
        assert_eq!(tool.input_schema.get("additionalProperties"), Some(&serde_json::Value::Bool(false)));
    }
    let query = tools.iter().find(|tool| tool.name == "query").expect("query");
    assert_eq!(
        query.input_schema.get("required").and_then(|value| value.as_array()),
        Some(&vec![serde_json::Value::String("sql".to_owned())])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initialize_previous_version_lists_gateway_tools_only() {
    let session = spawn(
        ClientLifecycleMode::Initialize,
        ProtocolVersion::V_2025_11_25,
    )
    .await
    .expect("initialize 2025-11-25");
    let info = session.running.peer().peer_info().expect("peer info");
    assert_eq!(info.protocol_version, ProtocolVersion::V_2025_11_25);
    let tools = session.running.peer().list_tools(None).await.expect("tools");
    assert_tools(&tools.tools);
    assert_eq!(tools.ttl_ms, Some(0));
    assert_eq!(tools.cache_scope, Some(rmcp::model::CacheScope::Private));
    let prompts = session.running.peer().list_prompts(None).await.expect("prompts");
    assert!(prompts.prompts.is_empty(), "prompts were invented");
    let cursor = session
        .running
        .peer()
        .list_tools(Some(PaginatedRequestParams::default().with_cursor(Some("next".to_owned()))))
        .await
        .expect_err("cursor");
    match cursor {
        ServiceError::McpError(error) => assert_eq!(error.code, ErrorCode::INVALID_PARAMS),
        other => panic!("cursor error was {other}"),
    }
    let export = session
        .running
        .peer()
        .call_tool(CallToolRequestParams::new("export"))
        .await
        .expect_err("export");
    match export {
        ServiceError::McpError(error) => {
            assert_eq!(error.code, ErrorCode::METHOD_NOT_FOUND);
            assert_eq!(error.code.0, -32601);
        }
        other => panic!("export error was {other}"),
    }
    let stderr = session.finish().await;
    assert_stderr(&stderr);
    assert!(stderr.contains("method=initialize status=200"), "initialize log");
    assert!(stderr.contains("method=tools/list status=200"), "tools log");
    assert!(stderr.contains("method=tools/call status=32601"), "export log");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discover_target_version_lists_the_same_tools() {
    let session = spawn(
        ClientLifecycleMode::Discover {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        },
        ProtocolVersion::V_2026_07_28,
    )
    .await
    .expect("discover 2026-07-28");
    let info = session.running.peer().peer_info().expect("peer info");
    assert_eq!(info.protocol_version, ProtocolVersion::V_2026_07_28);
    let tools = session.running.peer().list_tools(None).await.expect("tools");
    assert_tools(&tools.tools);
    let stderr = session.finish().await;
    assert_stderr(&stderr);
    assert!(stderr.contains("method=server/discover status=200"), "discover log");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initialize_rejects_unknown_and_non_handshake_versions() {
    let cases = [
        ("\"1999-01-01\"", None),
        ("\"2025-06-18\"", Some(ProtocolVersion::V_2025_06_18)),
        ("\"2026-07-28\"", Some(ProtocolVersion::V_2026_07_28)),
    ];
    for (encoded, constant) in cases {
        let version = match constant {
            Some(version) => version,
            None => serde_json::from_str(encoded).expect("unknown version"),
        };
        let error = match spawn(ClientLifecycleMode::Initialize, version).await {
            Err(error) => error,
            Ok(_) => panic!("{encoded} was accepted"),
        };
        match error {
            ClientInitializeError::JsonRpcError(error) => {
                assert_eq!(error.code, ErrorCode::UNSUPPORTED_PROTOCOL_VERSION, "{encoded}");
                assert_eq!(error.code.0, -32022, "{encoded}");
            }
            other => panic!("{encoded} failed as {other}"),
        }
    }
}
