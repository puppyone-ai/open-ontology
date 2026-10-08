//! Private PostgreSQL differential for the stdio server.
//!
//! Cargo leaves this ignored until `OO_A388_LIVE=1`. The gateway, the compiler,
//! and the MCP peer are separate processes. This file does not sign remote OAuth.

#![forbid(unsafe_code)]
#![cfg(feature = "live")]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use oo_gateway::http_gateway::{
    harness_install, harness_issue, CurrentTuple, Endpoint, IssueGrant, TokenKind,
};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, ErrorCode,
    Implementation, ProtocolVersion, ReadResourceRequestParams, ResourceContents,
};
use rmcp::service::{ClientInitializeError, ClientServiceExt, RoleClient, RunningService, ServiceError};
use rmcp::transport::TokioChildProcess;
use rmcp::ClientLifecycleMode;
use serde_json::{Map as JsonMap, Value};
use tokio::io::AsyncReadExt;
use zeroize::Zeroize;

const AUDIENCE: &str = "https://gateway.loopback.invalid/v1";
const SQL_MARKER: &str = "select a388_stdio_marker";
const CANARY: &str = "CANARY_A388_NOT_EXPORTED";

struct GatewayProc {
    child: Child,
}

impl Drop for GatewayProc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires OO_A388_LIVE=1, private PostgreSQL, oo-gateway, and role-pack-bundle"]
async fn stdio_matches_live_gateway_for_both_protocol_versions() {
    let live = std::env::var("OO_A388_LIVE").unwrap_or_default();
    assert_eq!(live, "1", "OO_A388_LIVE=1 is required; refusing a green skip");
    const { assert!(cfg!(target_os = "linux"), "the proc checks run on linux") };

    let host = required_env("OO_A388_PG_HOST");
    let port: u16 = required_env("OO_A388_PG_PORT").parse().expect("port");
    let database = required_env("OO_A388_PG_DATABASE");
    let admin_path = PathBuf::from(required_env("OO_A388_ADMIN_PASSWORD_FILE"));
    let runtime_path = PathBuf::from(required_env("OO_A388_RUNTIME_PASSWORD_FILE"));
    let ca_path = PathBuf::from(required_env("OO_A388_CA_FILE"));
    let marker_path = PathBuf::from(required_env("OO_A388_MARKER"));
    let fixture_dir = PathBuf::from(required_env("OO_A388_FIXTURE_DIR"));
    std::fs::write(&marker_path, b"ran=1\n").expect("marker");

    let work = std::env::temp_dir().join(format!("oo-a388-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).expect("work dir");
    let (picker_text, picker_digest) = compile_pack(&work, &fixture_dir, "visibility-picker.json", "picker.json");
    let (auditor_text, auditor_digest) = compile_pack(&work, &fixture_dir, "visibility-auditor.json", "auditor.json");
    assert_ne!(picker_digest, auditor_digest, "the two roles sealed the same digest");
    assert!(!picker_text.contains("unclassified_pii"));
    assert!(!auditor_text.contains("unclassified_pii"));
    assert!(!picker_text.contains("auditor_phone"));
    assert!(!auditor_text.contains("picker_note"));
    refuse_hidden(&work, &fixture_dir);

    let mut admin = read_secret(&admin_path);
    let mut runtime = read_secret(&runtime_path);
    let ca = std::fs::read(&ca_path).expect("ca");
    let endpoint = Endpoint {
        host: host.clone(),
        port,
        database: database.clone(),
    };
    harness_install(endpoint.clone(), trim_secret(&admin), &ca)
        .await
        .expect("install");
    let mut picker = harness_issue(
        endpoint.clone(),
        trim_secret(&admin),
        &ca,
        client_grant("oo_r_picker", "picker-user"),
    )
    .await
    .expect("issue picker");
    let mut auditor = harness_issue(
        endpoint.clone(),
        trim_secret(&admin),
        &ca,
        client_grant("oo_r_auditor", "auditor-user"),
    )
    .await
    .expect("issue auditor");
    let mut session_token = harness_issue(endpoint, trim_secret(&admin), &ca, session_grant())
        .await
        .expect("issue session");
    admin.zeroize();

    let config = write_config(
        &runtime_path,
        &ca_path,
        &host,
        port,
        &database,
        &[
            ("picker.json", picker_text.as_str()),
            ("auditor.json", auditor_text.as_str()),
        ],
    );
    let gateway = GatewayProc {
        child: spawn_gateway(&config),
    };
    let addr = wait_addr(&config);
    let gateway_port = addr.port();
    let url = format!("http://127.0.0.1:{gateway_port}");

    let (status, picker_http) = http(gateway_port, "GET", "/v1/pack/metadata", Some(picker.0.as_str()), None);
    assert_eq!(status, 200, "picker http metadata");
    let (status, auditor_http) = http(gateway_port, "GET", "/v1/pack/metadata", Some(auditor.0.as_str()), None);
    assert_eq!(status, 200, "auditor http metadata");
    let (status, picker_session_http) = http(
        gateway_port,
        "GET",
        "/v1/session/current",
        Some(picker.0.as_str()),
        None,
    );
    assert_eq!(status, 200, "picker http session");
    let (status, session_http) = http(
        gateway_port,
        "GET",
        "/v1/session/current",
        Some(session_token.0.as_str()),
        None,
    );
    assert_eq!(status, 401, "session http");
    let (status, sql_http) = post(
        gateway_port,
        "/v1/sql",
        Some(picker.0.as_str()),
        &format!("{{\"sql\":\"{SQL_MARKER}\"}}"),
    );
    assert_eq!(status, 501, "sql http");
    assert_role(&picker_http, &picker_digest, "picker-user", "oo_r_picker", &["order_id", "picker_note"]);
    assert_role(
        &auditor_http,
        &auditor_digest,
        "auditor-user",
        "oo_r_auditor",
        &["auditor_phone", "order_id"],
    );
    assert!(!body_text(&picker_http).contains("auditor_phone"));
    assert!(!body_text(&auditor_http).contains("picker_note"));
    assert!(!body_text(&picker_http).contains("unclassified_pii"));
    assert!(!body_text(&auditor_http).contains("unclassified_pii"));
    assert!(!body_text(&picker_http).contains("does not authenticate"));
    assert_error_envelope(&session_http, "UNAUTHENTICATED");
    assert_error_envelope(&sql_http, "SQL_UNSUPPORTED");
    assert!(!body_text(&sql_http).contains(SQL_MARKER), "sql body echoed");
    assert!(!body_text(&picker_http).contains(&picker.0), "bearer echoed");
    assert!(!body_text(&session_http).contains(&session_token.0), "session bearer echoed");

    let token_path = work.join("token");
    publish_token(&token_path, &picker.0);
    let initialized = spawn_mcp(
        &url,
        &token_path,
        ClientLifecycleMode::Initialize,
        ProtocolVersion::V_2025_11_25,
    )
    .await
    .expect("initialize 2025-11-25");
    let info = initialized.running.peer().peer_info().expect("peer info");
    assert_eq!(info.protocol_version, ProtocolVersion::V_2025_11_25);
    let server_info = info.server_info.as_ref().expect("server info");
    assert_eq!(server_info.name, "oo-mcp-gateway");
    let instructions = info.instructions.as_deref().unwrap_or("");
    assert!(instructions.contains("OAuth is not implemented"));
    assert!(instructions.contains("Remote filesystem export is disabled"));

    let picker_meta = initialized
        .running
        .peer()
        .call_tool(CallToolRequestParams::new("pack_metadata"))
        .await
        .expect("picker pack_metadata");
    assert_same(&picker_meta, &picker_http, false);
    let picker_now = initialized
        .running
        .peer()
        .call_tool(CallToolRequestParams::new("session_current"))
        .await
        .expect("picker session");
    assert_same(&picker_now, &picker_session_http, false);

    publish_token(&token_path, &auditor.0);
    let auditor_meta = initialized
        .running
        .peer()
        .call_tool(CallToolRequestParams::new("pack_metadata"))
        .await
        .expect("auditor pack_metadata");
    assert_same(&auditor_meta, &auditor_http, false);
    assert!(!structured_text(&auditor_meta).contains("picker_note"));
    assert!(!structured_text(&picker_meta).contains("auditor_phone"));

    let listed = initialized
        .running
        .peer()
        .list_resources(None)
        .await
        .expect("resources");
    assert_eq!(listed.ttl_ms, Some(0));
    assert_eq!(listed.cache_scope, Some(rmcp::model::CacheScope::Private));
    let http_files = body_json(&auditor_http)["files"].as_array().expect("files").clone();
    assert_eq!(listed.resources.len(), http_files.len());
    let first = http_files.first().expect("pack file");
    let path = first["path"].as_str().expect("path");
    let uri = format!("oo://knowledge/{path}");
    assert!(listed.resources.iter().any(|resource| resource.uri == uri));
    let read = initialized
        .running
        .peer()
        .read_resource(ReadResourceRequestParams::new(uri))
        .await
        .expect("read resource");
    assert_eq!(read.ttl_ms, Some(0));
    assert_eq!(read.cache_scope, Some(rmcp::model::CacheScope::Private));
    match read.contents.as_slice() {
        [ResourceContents::TextResourceContents { mime_type, text, .. }] => {
            assert_eq!(mime_type.as_deref(), Some("application/json"));
            let parsed: Value = serde_json::from_str(text).expect("resource json");
            assert_eq!(&parsed, first);
            assert!(parsed.get("text").is_none(), "file prose returned");
        }
        _ => panic!("resource was not a single json text content"),
    }

    let mut sql_args = JsonMap::new();
    sql_args.insert("sql".to_owned(), Value::String(SQL_MARKER.to_owned()));
    let sql = initialized
        .running
        .peer()
        .call_tool(CallToolRequestParams::new("query").with_arguments(sql_args))
        .await
        .expect("query");
    assert_same(&sql, &sql_http, true);
    assert!(!structured_text(&sql).contains(SQL_MARKER));

    publish_token(&token_path, &session_token.0);
    let denied = initialized
        .running
        .peer()
        .call_tool(CallToolRequestParams::new("session_current"))
        .await
        .expect("session tool");
    assert_same(&denied, &session_http, true);

    let canary = work.join("canary.txt");
    std::fs::write(&canary, CANARY.as_bytes()).expect("canary");
    let export_args = {
        let mut args = JsonMap::new();
        args.insert("path".to_owned(), Value::String(canary.display().to_string()));
        args
    };
    let export = initialized
        .running
        .peer()
        .call_tool(CallToolRequestParams::new("export").with_arguments(export_args))
        .await
        .expect_err("export");
    match export {
        ServiceError::McpError(error) => {
            assert_eq!(error.code, ErrorCode::METHOD_NOT_FOUND);
            assert!(!error.message.contains(CANARY));
        }
        other => panic!("export error was {other}"),
    }
    let file_uri = initialized
        .running
        .peer()
        .read_resource(ReadResourceRequestParams::new("file:///tmp/oo-a388-export"))
        .await
        .expect_err("file uri");
    match file_uri {
        ServiceError::McpError(error) => assert_eq!(error.code, ErrorCode::INVALID_PARAMS),
        other => panic!("file uri error was {other}"),
    }
    assert_eq!(std::fs::read(&canary).expect("canary remains"), CANARY.as_bytes());

    let init_stderr = initialized.finish().await;
    assert_stderr(&init_stderr, &runtime, SQL_MARKER);
    assert!(init_stderr.contains("method=initialize status=200"));
    assert!(init_stderr.contains("method=tools/call status=200"));
    assert!(init_stderr.contains("method=tools/call status=501"));
    assert!(init_stderr.contains("method=tools/call status=401"));
    assert!(init_stderr.contains("method=tools/call status=32601"));
    assert!(init_stderr.contains("method=resources/read status=200"));

    publish_token(&token_path, &picker.0);
    let discovered = spawn_mcp(
        &url,
        &token_path,
        ClientLifecycleMode::Discover {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        },
        ProtocolVersion::V_2026_07_28,
    )
    .await
    .expect("discover 2026-07-28");
    let info = discovered.running.peer().peer_info().expect("discover peer");
    assert_eq!(info.protocol_version, ProtocolVersion::V_2026_07_28);
    let discovered_meta = discovered
        .running
        .peer()
        .call_tool(CallToolRequestParams::new("pack_metadata"))
        .await
        .expect("discover pack");
    assert_same(&discovered_meta, &picker_http, false);
    let discover_stderr = discovered.finish().await;
    assert_stderr(&discover_stderr, &runtime, SQL_MARKER);
    assert!(discover_stderr.contains("method=server/discover status=200"));

    assert_process_surface(gateway.child.id(), &runtime);
    let gateway_stderr = std::fs::read(config.join("stderr.txt")).unwrap_or_default();
    assert_absent(&gateway_stderr, trim_secret(&runtime));
    assert_absent(&gateway_stderr, "ooct_");
    assert_absent(&gateway_stderr, "oost_");

    runtime.zeroize();
    picker.0.zeroize();
    auditor.0.zeroize();
    session_token.0.zeroize();
    picker.1.zeroize();
    auditor.1.zeroize();
    session_token.1.zeroize();
    drop(gateway);
    let _ = std::fs::remove_dir_all(&work);
    let _ = std::fs::remove_dir_all(&config);
    std::fs::write(&marker_path, b"ran=1\ncompleted=1\n").expect("marker done");
}

fn assert_same(got: &CallToolResult, http_body: &[u8], expect_error: bool) {
    let http: Value = serde_json::from_slice(http_body).expect("http json");
    assert!(http.get("query_id").is_none(), "query_id");
    assert!(http.get("warnings").is_none(), "warnings");
    assert_eq!(got.is_error, Some(expect_error));
    assert_eq!(got.structured_content.as_ref(), Some(&http));
}

fn structured_text(got: &CallToolResult) -> String {
    got.structured_content
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default()
}

fn assert_role(body: &[u8], digest: &str, principal: &str, role: &str, columns: &[&str]) {
    let meta = body_json(body);
    assert!(meta.get("query_id").is_none(), "query_id");
    assert!(meta.get("warnings").is_none(), "warnings");
    assert_eq!(meta["project_id"], "depot");
    assert_eq!(meta["principal_id"], principal);
    assert_eq!(meta["role"], role);
    assert_eq!(meta["scope"], "client");
    assert_eq!(meta["policy_version"], "pv1");
    assert_eq!(meta["knowledge_version"], "kv1");
    assert_eq!(meta["authentication"], "loopback-http");
    assert_eq!(meta["external_mtls"], false);
    assert_eq!(meta["oauth"], false);
    assert_eq!(meta["idp"], false);
    assert_eq!(meta["protocol"], "oo.builder.role-pack/v1");
    assert_eq!(meta["pack_digest"], digest);
    let mut got = Vec::new();
    for column in meta["columns"].as_array().expect("columns") {
        assert_eq!(column["table"], "shop.orders");
        got.push(column["name"].as_str().expect("column name").to_owned());
    }
    assert_eq!(got, columns);
}

fn assert_error_envelope(body: &[u8], code: &str) {
    let value = body_json(body);
    assert!(value.get("query_id").is_none(), "query_id");
    assert!(value.get("warnings").is_none(), "warnings");
    let error = value["error"].as_object().expect("error");
    assert_eq!(error.len(), 4);
    assert_eq!(error["code"], code);
    assert!(error.contains_key("message"));
    assert!(error.contains_key("hint"));
    assert!(error.contains_key("retryable"));
}

fn assert_stderr(stderr: &str, runtime: &str, sql: &str) {
    assert!(
        stderr.contains("oo-mcp-gateway: stdio ready oauth=false remote_export=disabled"),
        "ready line missing"
    );
    assert!(!stderr.contains("jsonrpc"), "protocol bytes on stderr");
    assert!(!stderr.contains(sql), "sql logged");
    assert_absent(stderr.as_bytes(), trim_secret(runtime));
    assert_absent(stderr.as_bytes(), "ooct_");
    assert_absent(stderr.as_bytes(), "oost_");
    assert_absent(stderr.as_bytes(), CANARY);
}

fn assert_process_surface(pid: u32, runtime: &str) {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).expect("cmdline");
    let environ = std::fs::read(format!("/proc/{pid}/environ")).expect("environ");
    for surface in [&cmdline, &environ] {
        assert_absent(surface, trim_secret(runtime));
        assert_absent(surface, "ooct_");
        assert_absent(surface, "oost_");
    }
}

fn assert_absent(haystack: &[u8], needle: &str) {
    let needle = needle.as_bytes();
    if needle.is_empty() {
        return;
    }
    assert!(
        !haystack.windows(needle.len()).any(|window| window == needle),
        "secret material appeared on a process surface"
    );
}

async fn spawn_mcp(
    url: &str,
    token_file: &Path,
    lifecycle: ClientLifecycleMode,
    version: ProtocolVersion,
) -> Result<Session, ClientInitializeError> {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_mcp-gateway"));
    command.env_clear();
    command.env("PATH", std::env::var("PATH").unwrap_or_default());
    command.env("OO_MCP_GATEWAY_URL", url);
    command.env("OO_MCP_TOKEN_FILE", token_file);
    command.env("OO_MCP_TOKEN", "ooct_decoy_must_not_be_logged");
    command.kill_on_drop(true);
    let (transport, stderr) = TokioChildProcess::builder(command)
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mcp-gateway");
    let pid = transport.id().expect("pid");
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).expect("mcp cmdline");
    let environ = std::fs::read(format!("/proc/{pid}/environ")).expect("mcp environ");
    assert_absent(&cmdline, "ooct_");
    assert_absent(&cmdline, "oost_");
    let mut file = std::fs::read(token_file).expect("token file");
    let token_bytes = trim_secret(std::str::from_utf8(&file).expect("token utf8")).as_bytes();
    assert!(!token_bytes.is_empty(), "empty token");
    assert!(
        !cmdline.windows(token_bytes.len()).any(|window| window == token_bytes),
        "token file contents in argv"
    );
    assert!(
        !environ.windows(token_bytes.len()).any(|window| window == token_bytes),
        "token file contents in environ"
    );
    file.zeroize();
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

fn client_config(version: ProtocolVersion) -> ClientConfig {
    ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("oo-a388-live-client", "0.1.0"),
    )
    .with_protocol_version(version)
}

fn publish_token(path: &Path, token: &str) {
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .expect("token file");
    writeln!(file, "{token}").expect("token write");
    drop(file);
    std::fs::rename(&tmp, path).expect("token rename");
}

fn compile_pack(work: &Path, fixture_dir: &Path, visibility: &str, output_name: &str) -> (String, String) {
    let output = work.join(output_name);
    let digest_path = work.join(format!("{output_name}.digest"));
    let produced = Command::new(required_env("OO_A388_FIXTURE_BIN"))
        .arg("compile")
        .arg("--compiler")
        .arg(required_env("OO_A388_ROLE_PACK_BIN"))
        .arg("--effective")
        .arg(fixture_dir.join("effective.json"))
        .arg("--visibility")
        .arg(fixture_dir.join(visibility))
        .arg("--output")
        .arg(&output)
        .arg("--digest")
        .arg(&digest_path)
        .stdin(Stdio::null())
        .output()
        .expect("fixture spawn");
    let tool_stderr = String::from_utf8_lossy(&produced.stderr);
    assert!(produced.status.success(), "compile {visibility} failed: {tool_stderr}");
    let text = std::fs::read_to_string(&output).expect("compiled pack");
    let digest = std::fs::read_to_string(&digest_path).expect("digest").trim().to_owned();
    assert_eq!(digest.len(), 64, "compiler digest length");
    (text, digest)
}

fn refuse_hidden(work: &Path, fixture_dir: &Path) {
    let stderr_path = work.join("hidden.stderr");
    let produced = Command::new(required_env("OO_A388_FIXTURE_BIN"))
        .arg("refuse")
        .arg("--compiler")
        .arg(required_env("OO_A388_ROLE_PACK_BIN"))
        .arg("--effective")
        .arg(fixture_dir.join("effective.json"))
        .arg("--visibility")
        .arg(fixture_dir.join("visibility-hidden.json"))
        .arg("--stderr")
        .arg(&stderr_path)
        .stdin(Stdio::null())
        .output()
        .expect("fixture spawn");
    let tool_stderr = String::from_utf8_lossy(&produced.stderr);
    assert!(produced.status.success(), "hidden visibility was not refused: {tool_stderr}");
    let compiler = std::fs::read_to_string(&stderr_path).unwrap_or_default();
    assert!(compiler.contains("classification.unknown_pii"), "compiler stderr missed unknown PII");
}

fn write_config(
    runtime_file: &Path,
    ca_file: &Path,
    host: &str,
    port: u16,
    database: &str,
    packs: &[(&str, &str)],
) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oo-a388-gw-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("packs")).expect("config dir");
    std::fs::copy(runtime_file, dir.join("runtime.pass")).expect("runtime copy");
    std::fs::copy(ca_file, dir.join("ca.crt")).expect("ca copy");
    std::fs::write(
        dir.join("endpoint.json"),
        format!("{{\"host\":\"{host}\",\"port\":{port},\"database\":\"{database}\"}}\n"),
    )
    .expect("endpoint");
    for (name, text) in packs {
        std::fs::write(dir.join("packs").join(name), text).expect("pack copy");
    }
    let config = format!(
        "{{\"listen\":\"127.0.0.1:0\",\"audience\":\"{AUDIENCE}\",\"runtime_user\":\"oo_token_runtime\",\"password_file\":\"runtime.pass\",\"ca_file\":\"ca.crt\",\"endpoint_file\":\"endpoint.json\",\"pack_dir\":\"packs\",\"body_limit\":1048576,\"request_timeout_ms\":5000,\"global_concurrency\":1}}\n"
    );
    std::fs::write(dir.join("config.json"), config).expect("config");
    dir
}

fn spawn_gateway(dir: &Path) -> Child {
    let bin = required_env("OO_A388_GATEWAY_BIN");
    let _ = std::fs::remove_file(dir.join("listen.addr"));
    let stderr = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("stderr.txt"))
        .expect("stderr");
    Command::new(bin)
        .arg(dir)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("spawn gateway")
}

fn wait_addr(dir: &Path) -> std::net::SocketAddr {
    let path = dir.join("listen.addr");
    let started = Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(addr) = text.trim().parse()
        {
            return addr;
        }
        if started.elapsed() > Duration::from_secs(8) {
            panic!("gateway did not write listen.addr");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn http(port: u16, method: &str, path: &str, bearer: Option<&str>, extra: Option<(&str, &str)>) -> (u16, Vec<u8>) {
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    if let Some(token) = bearer {
        request.push_str("Authorization: Bearer ");
        request.push_str(token);
        request.push_str("\r\n");
    }
    if let Some((name, value)) = extra {
        request.push_str(name);
        request.push_str(": ");
        request.push_str(value);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    exchange(port, request.as_bytes())
}

fn post(port: u16, path: &str, bearer: Option<&str>, body: &str) -> (u16, Vec<u8>) {
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(token) = bearer {
        request.push_str("Authorization: Bearer ");
        request.push_str(token);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    request.push_str(body);
    exchange(port, request.as_bytes())
}

fn exchange(port: u16, request: &[u8]) -> (u16, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(8))).expect("read timeout");
    stream.set_write_timeout(Some(Duration::from_secs(8))).expect("write timeout");
    stream.write_all(request).expect("write");
    let mut bytes = Vec::new();
    let _ = stream.read_to_end(&mut bytes);
    parse_response(&bytes)
}

fn parse_response(bytes: &[u8]) -> (u16, Vec<u8>) {
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("header terminator");
    let head = std::str::from_utf8(&bytes[..split]).expect("header");
    let rest = &bytes[split + 4..];
    let status: u16 = head
        .lines()
        .next()
        .expect("status")
        .split_whitespace()
        .nth(1)
        .expect("code")
        .parse()
        .expect("number");
    let length = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            value.trim().parse::<usize>().ok()
        } else {
            None
        }
    });
    let body = match length {
        Some(length) => rest[..length.min(rest.len())].to_vec(),
        None => rest.to_vec(),
    };
    (status, body)
}

fn body_text(body: &[u8]) -> String {
    String::from_utf8_lossy(body).into_owned()
}

fn body_json(body: &[u8]) -> Value {
    serde_json::from_slice(body).expect("json")
}

fn client_grant(role: &str, principal: &str) -> IssueGrant {
    IssueGrant {
        kind: TokenKind::Client,
        client_ttl_ms: None,
        current: CurrentTuple {
            project_id: "depot".to_owned(),
            principal_id: principal.to_owned(),
            role: role.to_owned(),
            claims: vec![("region".to_owned(), "north".to_owned())],
            policy_version: "pv1".to_owned(),
            knowledge_version: "kv1".to_owned(),
            audience_url: AUDIENCE.to_owned(),
            sandbox_id: None,
            session_id: None,
        },
    }
}

fn session_grant() -> IssueGrant {
    IssueGrant {
        kind: TokenKind::Session,
        client_ttl_ms: None,
        current: CurrentTuple {
            project_id: "depot".to_owned(),
            principal_id: "picker-user".to_owned(),
            role: "oo_r_picker".to_owned(),
            claims: vec![("region".to_owned(), "north".to_owned())],
            policy_version: "pv1".to_owned(),
            knowledge_version: "kv1".to_owned(),
            audience_url: AUDIENCE.to_owned(),
            sandbox_id: Some("sb-a388".to_owned()),
            session_id: Some("ss-a388".to_owned()),
        },
    }
}

fn trim_secret(value: &str) -> &str {
    value.trim_end_matches(['\n', '\r'])
}

fn read_secret(path: &Path) -> String {
    let text = std::fs::read_to_string(path).expect("secret file");
    if text.is_empty() {
        panic!("empty secret");
    }
    text
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}
