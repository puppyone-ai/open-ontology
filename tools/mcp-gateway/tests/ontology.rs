//! D83 over MCP: the unified ontology tools against a mock `oo serve` on loopback. The Agent token is the only
//! bearer, the tools map one to one onto `/v1/ontology/*`, a refusal is an error result, not an empty one.

#![forbid(unsafe_code)]

use std::sync::{Arc, Mutex};

use rmcp::ClientLifecycleMode;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation, ProtocolVersion,
};
use rmcp::service::ClientServiceExt;
use rmcp::transport::TokioChildProcess;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::process::Command;

const TOKEN: &str = "ooa_mock_agent_token";

async fn serve(seen: Arc<Mutex<Vec<(String, String)>>>) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16384];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                let first = text.lines().next().unwrap_or("").to_owned();
                let auth = text
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("authorization:"))
                    .unwrap_or("")
                    .to_owned();
                seen.lock().unwrap().push((first.clone(), auth));
                let (status, body): (u16, Value) = if first.contains("/v1/ontology/search") {
                    (
                        200,
                        json!({"protocol": "oo.ontology.search/v1", "items": [
                        {"urn": "oo:doc/d/org:acme", "side": "doc", "provenance": {"kind": "file", "path": "a.md"}},
                        {"urn": "oo:db/sales/sales.customers/1", "side": "db", "provenance": {"kind": "table_row", "table": "sales.customers"}}]}),
                    )
                } else if first.starts_with("POST /v1/ontology/query") {
                    (403, json!({"code": "ONTOLOGY_READ_ONLY"}))
                } else {
                    (404, json!({"code": "ONTOLOGY_NOT_FOUND"}))
                };
                let b = body.to_string();
                let _ = s.write_all(format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len()).as_bytes()).await;
            });
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unified_tools_map_onto_the_ontology_surface() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let port = serve(seen.clone()).await;
    let dir = std::env::temp_dir().join(format!("oo-d83-mcp-{}-{port}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let tok = dir.join("token");
    std::fs::write(&tok, format!("{TOKEN}\n")).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcp-gateway"));
    command
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("OO_MCP_ONTOLOGY_URL", format!("http://127.0.0.1:{port}"))
        .env("OO_MCP_ONTOLOGY_TOKEN_FILE", &tok)
        .kill_on_drop(true);
    let (transport, _) = TokioChildProcess::builder(command).spawn().unwrap();
    let cfg = ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("oo-d83-client", "0.1.0"),
    )
    .with_protocol_version(ProtocolVersion::V_2025_11_25);
    let running = cfg
        .serve_with_lifecycle(transport, ClientLifecycleMode::Initialize)
        .await
        .unwrap();
    let tools = running.peer().list_all_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(
        names,
        [
            "ontology_search",
            "ontology_entity",
            "ontology_sources",
            "ontology_query"
        ]
    );
    let r = running
        .peer()
        .call_tool(
            CallToolRequestParams::new("ontology_search")
                .with_arguments(json!({"q": "Acme"}).as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    let text = serde_json::to_string(&r).unwrap();
    assert!(
        text.contains("oo:doc/d/org:acme") && text.contains("oo:db/sales/sales.customers/1"),
        "{text}"
    );
    let w = running
        .peer()
        .call_tool(
            CallToolRequestParams::new("ontology_query")
                .with_arguments(json!({"sql": "DELETE FROM t"}).as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert!(
        serde_json::to_string(&w)
            .unwrap()
            .contains("ONTOLOGY_READ_ONLY")
    );
    let bad = running
        .peer()
        .call_tool(
            CallToolRequestParams::new("ontology_search")
                .with_arguments(json!({"q": "a", "x": 1}).as_object().unwrap().clone()),
        )
        .await;
    assert!(
        bad.is_err(),
        "unknown arguments are refused before they leave"
    );
    assert!(
        seen.lock().unwrap().iter().all(|(_, a)| a.contains(TOKEN)),
        "the Agent token is the only bearer"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
