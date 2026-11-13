//! CLI dispatch/teardown contracts; live Docker evidence is a separate test.
use std::process::Command;

#[path = "common/mod.rs"]
mod common;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oo"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn demo_help_is_offline_and_names_its_limits() {
    for command in ["try", "quickstart"] {
        let help = run(&[command, "--help"]);
        assert!(help.status.success());
        assert_eq!(help.stdout, run(&["help", command]).stdout);
    }
    let help = String::from_utf8(run(&["try", "--help"]).stdout).unwrap();
    assert!(help.contains("fixed demo gold"));
    assert!(help.contains("Missing audit_id or published_generation stays null"));
}

#[test]
fn cleanup_of_an_absent_demo_does_not_need_docker() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let state = format!("evidence/demo-absent-{}", std::process::id());
    assert!(!root.join(&state).exists());
    let result = Command::new(env!("CARGO_BIN_EXE_oo"))
        .current_dir(root)
        .args(["quickstart", "--down", "--state", &state, "--json"])
        .output()
        .unwrap();
    assert!(result.status.success(), "{:?}", result.stdout);
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["status"], "not_started");
    assert_eq!(report["resources"], serde_json::json!([]));
    assert!(!root.join(state).exists());
}

#[test]
fn demo_rejects_nonlocal_queries_and_unknown_cleanup_flags() {
    for args in [
        vec!["try", "--gateway", "https://customer.example", "--json"],
        vec!["try", "--role", "owner", "--json"],
        vec!["quickstart", "--down", "--force", "--json"],
        vec!["quickstart", "--down", "--state", "--json"],
    ] {
        let result = run(&args);
        assert_eq!(result.status.code(), Some(2));
        let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(report["class"], "usage");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn try_runs_four_real_client_calls_against_a_scripted_tls_oracle() {
    use std::sync::{Arc, Mutex};
    use tokio::io::AsyncWriteExt;

    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cert = rustls::pki_types::CertificateDer::from(common::pem_block(
        include_str!("../../../contracts/sdk/json-query/fixtures/tls/localhost.pem"),
        "CERTIFICATE",
    ));
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
        common::pem_block(
            include_str!("../../../contracts/sdk/json-query/fixtures/tls/localhost.key"),
            "PRIVATE KEY",
        ),
    ));
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&hits);
    let gold: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../../deploy/lite/demo/gold.json")).unwrap();
    let task = tokio::spawn(async move {
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut tls = acceptor.accept(tcp).await.unwrap();
            let hit = common::read_hit(&mut tls).await.unwrap();
            let reply = if hit.path == "/v1/meta" {
                common::json_response(
                    200,
                    "OK",
                    r#"{"minimum_cli_version":"0.2.0","protocol_version":"0.2"}"#,
                    &[],
                )
            } else {
                assert_eq!(hit.path, "/v1/query");
                let request: serde_json::Value = serde_json::from_slice(&hit.body).unwrap();
                let mut recorded = recorded.lock().unwrap();
                let n = recorded.len();
                assert_eq!(request["sql"], gold[n]["sql"]);
                recorded.push(request);
                if n < 3 {
                    let mut response: serde_json::Value = serde_json::from_str(include_str!(
                        "../../../contracts/sdk/json-query/fixtures/query-response.json"
                    ))
                    .unwrap();
                    response["rows"] = gold[n]["expected"]["analyst"].clone();
                    response["returned_rows"] = serde_json::json!(1);
                    common::json_response(200, "OK", &response.to_string(), &[])
                } else {
                    common::json_response(
                        400,
                        "Bad Request",
                        r#"{"error":{"code":"COLUMN_NOT_ALLOWED","message":"denied","hint":"use visible columns","retryable":false,"query_id":"q_deny001"}}"#,
                        &[],
                    )
                }
            };
            tls.write_all(&reply).await.unwrap();
            tls.flush().await.unwrap();
        }
    });
    let scratch = common::Scratch::new();
    let spec = common::Spec::new(
        &scratch,
        vec![
            "try".into(),
            "--json".into(),
            "--gateway".into(),
            format!("https://localhost:{port}"),
            "--transport".into(),
            "production".into(),
            "--auth".into(),
            "external".into(),
            "--token-file".into(),
            scratch.token_path.display().to_string(),
            "--ca-file".into(),
            common::sdk_tls("ca.pem").display().to_string(),
        ],
    );
    let result = common::run(spec).await;
    task.abort();
    assert_eq!(result.code, 0, "{:?}", result.stdout);
    common::assert_secret_free(&result.stdout, &[]);
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["passed"], true);
    assert_eq!(report["audit_collection"], "not_verified");
    assert_eq!(hits.lock().unwrap().len(), 4);
    for row in report["results"].as_array().unwrap() {
        assert!(row["audit_id"].is_null());
        assert!(row["published_generation"].is_null());
    }
}

#[test]
fn init_connect_flags_check_without_a_terminal_and_never_take_inline_passwords() {
    // Port 1 on loopback: resolves, refuses; the report says so and exits 1.
    let out = run(&[
        "init",
        "--dsn",
        "jdbc:postgresql://127.0.0.1:1/sales",
        "--user",
        "ro",
        "--password-env",
        "OO_TEST_UNSET_PASSWORD",
        "--json",
    ]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["protocol"], "oo.init-connect/v1");
    assert_eq!(doc["understood"]["engine"], "postgres");
    assert_eq!(doc["understood"]["database"], "sales");
    assert_eq!(
        doc["understood"]["password_ref"],
        "env:OO_TEST_UNSET_PASSWORD"
    );
    assert_eq!(doc["checks"][0]["status"], "ok");
    assert_eq!(doc["checks"][1]["name"], "port");
    assert_eq!(doc["checks"][1]["status"], "blocked");
    assert_eq!(doc["written"], false);
    let inline = run(&[
        "init",
        "--dsn",
        "postgres://ro:hunter2@127.0.0.1:1/db",
        "--json",
    ]);
    assert!(!inline.status.success());
    let all = [inline.stdout, inline.stderr].concat();
    assert!(!String::from_utf8_lossy(&all).contains("hunter2"));
    assert!(String::from_utf8_lossy(&all).contains("INLINE_PASSWORD"));
}
