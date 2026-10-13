//! Private PostgreSQL proof that the gateway loads a sealed compiler RolePack.
//!
//! Cargo leaves the test ignored until `OO_A341_LIVE=1`. The packs come from
//! the fixed `role-pack-bundle` binary. This file does not run `OO_A326_LIVE`.

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use oo_gateway::http_gateway::{
    CurrentTuple, Endpoint, IssueGrant, TokenKind, harness_install, harness_issue,
    harness_revoke_token,
};
use oo_gateway::pack_catalog::PackCatalog;
use serde_json::Value;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

const AUDIENCE: &str = "https://gateway.loopback.invalid/v1";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires OO_A341_LIVE=1, the private PostgreSQL, and role-pack-bundle"]
async fn compiled_role_pack_metadata_hides_unclassified_pii() {
    let live = std::env::var("OO_A341_LIVE").unwrap_or_default();
    assert_eq!(
        live, "1",
        "OO_A341_LIVE=1 is required; refusing a green skip"
    );
    let host = required_env("OO_A341_PG_HOST");
    let port: u16 = required_env("OO_A341_PG_PORT").parse().expect("port");
    let database = required_env("OO_A341_PG_DATABASE");
    let admin_path = PathBuf::from(required_env("OO_A341_ADMIN_PASSWORD_FILE"));
    let runtime_path = PathBuf::from(required_env("OO_A341_RUNTIME_PASSWORD_FILE"));
    let ca_path = PathBuf::from(required_env("OO_A341_CA_FILE"));
    let marker_path = PathBuf::from(required_env("OO_A341_MARKER"));
    let fixture_dir = PathBuf::from(required_env("OO_A341_FIXTURE_DIR"));
    std::fs::write(&marker_path, b"ran=1\n").expect("marker");

    let work = std::env::temp_dir().join(format!("oo-a341-compiled-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).expect("work dir");
    let (picker_text, picker_digest) =
        compile_pack(&work, &fixture_dir, "visibility-picker.json", "picker.json");
    let (auditor_text, auditor_digest) = compile_pack(
        &work,
        &fixture_dir,
        "visibility-auditor.json",
        "auditor.json",
    );
    assert_ne!(
        picker_digest, auditor_digest,
        "the two roles sealed the same digest"
    );
    assert!(
        !picker_text.contains("unclassified_pii"),
        "picker pack names the hidden column"
    );
    assert!(
        !auditor_text.contains("unclassified_pii"),
        "auditor pack names the hidden column"
    );
    assert!(
        !picker_text.contains("auditor_phone"),
        "picker pack names the auditor column"
    );
    assert!(
        !auditor_text.contains("picker_note"),
        "auditor pack names the picker column"
    );
    refuse_hidden(&work, &fixture_dir);

    let picker_artifact: Value = serde_json::from_str(&picker_text).expect("picker artifact");
    let auditor_artifact: Value = serde_json::from_str(&auditor_text).expect("auditor artifact");
    assert_eq!(
        picker_artifact["pack_digest"].as_str(),
        Some(picker_digest.as_str())
    );
    assert_eq!(
        auditor_artifact["pack_digest"].as_str(),
        Some(auditor_digest.as_str())
    );
    let loaded = load_only(
        &work,
        &[
            ("picker.json", &picker_text),
            ("auditor.json", &auditor_text),
        ],
    );
    let picker_meta = loaded
        .find("depot", "oo_r_picker", "pv1", "kv1")
        .expect("picker pack");
    let auditor_meta = loaded
        .find("depot", "oo_r_auditor", "pv1", "kv1")
        .expect("auditor pack");
    assert_eq!(picker_meta.pack_digest, picker_digest);
    assert_eq!(auditor_meta.pack_digest, auditor_digest);
    assert_eq!(
        column_names(&picker_meta.columns),
        vec!["order_id", "picker_note"]
    );
    assert_eq!(
        column_names(&auditor_meta.columns),
        vec!["auditor_phone", "order_id"]
    );
    assert!(
        picker_meta
            .columns
            .iter()
            .all(|column| column.table == "shop.orders")
    );
    assert!(
        auditor_meta
            .columns
            .iter()
            .all(|column| column.table == "shop.orders")
    );

    let reasons = work.join("reasons");
    assert_eq!(
        reason(
            &reasons,
            &mutate(&picker_text, |value| {
                value["pack_digest"] = Value::String("ab".repeat(32));
            })
        ),
        "pack_digest"
    );
    assert_eq!(
        reason(
            &reasons,
            &mutate(&picker_text, |value| {
                let _ = value.as_object_mut().expect("object").remove("compiler");
            })
        ),
        "legacy_unsupported"
    );
    assert_eq!(
        reason(
            &reasons,
            &mutate(&picker_text, |value| {
                value["files"][0]["sha256"] = Value::String("0".repeat(64));
            })
        ),
        "source_digest"
    );
    assert_eq!(
        reason(
            &reasons,
            &mutate(&picker_text, |value| {
                let copy = value["files"][0].clone();
                value["files"].as_array_mut().expect("files").push(copy);
            })
        ),
        "path_duplicate"
    );
    assert_eq!(
        reason(
            &reasons,
            &mutate(&picker_text, |value| {
                value["total_bytes"] = Value::from(1_u64);
            })
        ),
        "budget"
    );

    let mut admin = read_secret(&admin_path);
    let mut runtime = read_secret(&runtime_path);
    let ca = std::fs::read(&ca_path).expect("ca");
    let endpoint = Endpoint {
        host: host.clone(),
        port,
        database: database.clone(),
    };
    let admin_password = trim_secret(&admin);
    harness_install(endpoint.clone(), admin_password, &ca)
        .await
        .expect("install");
    let mut picker = harness_issue(
        endpoint.clone(),
        admin_password,
        &ca,
        client_grant("oo_r_picker", "picker-user"),
    )
    .await
    .expect("issue picker");
    let mut auditor = harness_issue(
        endpoint.clone(),
        admin_password,
        &ca,
        client_grant("oo_r_auditor", "auditor-user"),
    )
    .await
    .expect("issue auditor");
    let mut session = harness_issue(endpoint.clone(), admin_password, &ca, session_grant())
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
    assert_process_refused(
        &runtime_path,
        &ca_path,
        &host,
        port,
        &database,
        &mutate(&picker_text, |value| {
            value["pack_digest"] = Value::String("ab".repeat(32));
        }),
    );
    assert_process_refused(
        &runtime_path,
        &ca_path,
        &host,
        port,
        &database,
        &mutate(&picker_text, |value| {
            let _ = value.as_object_mut().expect("object").remove("compiler");
        }),
    );

    let mut gateway = Running {
        child: spawn_gateway(&config),
    };
    let addr = wait_addr(&config);
    let mut picker_token = picker.0.clone();
    let mut auditor_token = auditor.0.clone();
    let mut session_token = session.0.clone();

    let (status, body) = http(addr, "GET", "/health", None, None);
    assert_eq!(status, 200, "health");
    assert_boundary(&body);
    let (status, ready) = http(addr, "GET", "/ready", None, None);
    assert_eq!(status, 200, "ready");
    let ready_json: Value = serde_json::from_slice(&ready).expect("ready json");
    assert_eq!(ready_json["database"], "not_probed");

    let (status, body) = http(addr, "GET", "/v1/pack/metadata", Some(&picker_token), None);
    assert_eq!(status, 200, "picker metadata");
    assert_role_metadata(
        &body,
        &picker_artifact,
        &picker_digest,
        "picker-user",
        "oo_r_picker",
        &["order_id", "picker_note"],
        &picker_token,
    );
    assert!(
        !body_text(&body).contains("auditor_phone"),
        "auditor column returned"
    );
    let origin = Instant::now();

    let mut revoke_admin = read_secret(&admin_path);
    harness_revoke_token(
        endpoint.clone(),
        trim_secret(&revoke_admin),
        &ca,
        "depot".to_owned(),
        picker.1.clone(),
    )
    .await
    .expect("revoke");
    revoke_admin.zeroize();
    sleep_until(origin + Duration::from_millis(1_000));
    let (status, body) = http(
        addr,
        "GET",
        "/v1/session/current",
        Some(&picker_token),
        None,
    );
    assert_eq!(status, 200, "warm cache inside the original deadline");
    assert_eq!(body_json(&body)["role"], "oo_r_picker");
    sleep_until(origin + Duration::from_millis(5_200));
    let (status, _) = http(addr, "GET", "/v1/pack/metadata", Some(&picker_token), None);
    assert_eq!(status, 401, "revoked at the original deadline");

    drop(gateway);
    gateway = Running {
        child: spawn_gateway(&config),
    };
    let addr = wait_addr(&config);
    let (status, _) = http(addr, "GET", "/v1/pack/metadata", Some(&picker_token), None);
    assert_eq!(status, 401, "revoked token stays refused after restart");

    let (status, body) = http(addr, "GET", "/v1/pack/metadata", Some(&auditor_token), None);
    assert_eq!(status, 200, "auditor metadata");
    assert_role_metadata(
        &body,
        &auditor_artifact,
        &auditor_digest,
        "auditor-user",
        "oo_r_auditor",
        &["auditor_phone", "order_id"],
        &auditor_token,
    );
    assert!(
        !body_text(&body).contains("picker_note"),
        "picker column returned"
    );
    let outage_origin = Instant::now();
    std::fs::write(
        config.join("endpoint.json"),
        format!("{{\"host\":\"127.0.0.1\",\"port\":1,\"database\":\"{database}\"}}\n"),
    )
    .expect("outage");
    sleep_until(outage_origin + Duration::from_millis(1_000));
    let (status, _) = http(addr, "GET", "/v1/pack/metadata", Some(&auditor_token), None);
    assert_eq!(
        status, 200,
        "warm cache does not consult the rewritten endpoint"
    );
    sleep_until(outage_origin + Duration::from_millis(5_200));
    let (status, _) = http(addr, "GET", "/v1/pack/metadata", Some(&auditor_token), None);
    assert_eq!(status, 401, "closed database after the deadline");
    let (status, ready) = http(addr, "GET", "/ready", None, None);
    assert_eq!(status, 200, "ready during outage");
    assert_eq!(body_json(&ready)["database"], "not_probed");

    drop(gateway);
    gateway = Running {
        child: spawn_gateway(&config),
    };
    let addr = wait_addr(&config);
    let (status, _) = http(addr, "GET", "/v1/pack/metadata", Some(&auditor_token), None);
    assert_eq!(status, 401, "restart during the outage fails closed");
    std::fs::write(
        config.join("endpoint.json"),
        format!("{{\"host\":\"{host}\",\"port\":{port},\"database\":\"{database}\"}}\n"),
    )
    .expect("restore");
    let (status, body) = http(addr, "GET", "/v1/pack/metadata", Some(&auditor_token), None);
    assert_eq!(status, 200, "restored endpoint");
    assert_role_metadata(
        &body,
        &auditor_artifact,
        &auditor_digest,
        "auditor-user",
        "oo_r_auditor",
        &["auditor_phone", "order_id"],
        &auditor_token,
    );

    for (name, value) in [
        ("X-OO-Role", "oo_r_picker"),
        ("X-OO-Project", "other-project"),
        ("X-OO-Scope", "session"),
    ] {
        let (status, body) = http(
            addr,
            "GET",
            "/v1/pack/metadata",
            Some(&auditor_token),
            Some((name, value)),
        );
        assert_eq!(status, 403, "{name}");
        let text = body_text(&body);
        assert!(
            !text.contains("picker_note") && !text.contains("auditor_phone"),
            "forged header returned a column"
        );
        assert!(
            !text.contains("does not authenticate"),
            "forged header returned pack text"
        );
        assert!(!text.contains(&auditor_token), "bearer echoed");
        assert_eq!(error_code(&body), "FORBIDDEN");
    }

    let (status, body) = http(
        addr,
        "GET",
        "/v1/session/current",
        Some(&session_token),
        None,
    );
    assert_eq!(status, 401, "session token");
    assert_eq!(error_code(&body), "UNAUTHENTICATED");
    assert!(
        !body_text(&body).contains(&session_token),
        "session bearer echoed"
    );

    let (status, _) = http(addr, "GET", "/v1/pack/metadata", None, None);
    assert_eq!(status, 401, "missing bearer");

    let (status, body) = http(
        addr,
        "GET",
        "/health?marker=query-must-not-echo",
        None,
        None,
    );
    assert_eq!(status, 400, "query string");
    assert_eq!(error_code(&body), "QUERY_NOT_ALLOWED");
    assert!(
        !body_text(&body).contains("query-must-not-echo"),
        "query echoed"
    );

    let (status, body) = post(
        addr,
        "/v1/sql",
        Some(&auditor_token),
        "{\"sql\":\"select 1\"}",
    );
    assert_eq!(status, 501, "sql");
    assert_eq!(error_code(&body), "SQL_UNSUPPORTED");
    assert!(!body_text(&body).contains("select 1"), "sql body echoed");
    assert!(
        !body_text(&body).contains(&auditor_token),
        "sql bearer echoed"
    );
    let (status, body) = post(addr, "/v1/sql", None, "{}");
    assert_eq!(status, 501, "sql without a bearer is still unsupported");
    assert_eq!(error_code(&body), "SQL_UNSUPPORTED");

    let stderr = std::fs::read_to_string(config.join("stderr.txt")).unwrap_or_default();
    assert!(
        !stderr.contains(trim_secret(&runtime)),
        "password in stderr"
    );
    assert!(
        !stderr.contains("ooct_") && !stderr.contains("oost_"),
        "bearer prefix in stderr"
    );
    let cmdline = std::fs::read(format!("/proc/{}/cmdline", gateway.child.id())).expect("cmdline");
    let joined = String::from_utf8_lossy(&cmdline);
    assert!(!joined.contains(trim_secret(&runtime)), "password in argv");
    assert!(
        !joined.contains("ooct_") && !joined.contains("oost_"),
        "bearer in argv"
    );

    runtime.zeroize();
    picker.0.zeroize();
    auditor.0.zeroize();
    session.0.zeroize();
    picker_token.zeroize();
    auditor_token.zeroize();
    session_token.zeroize();
    drop(gateway);
    std::fs::write(&marker_path, b"ran=1\ncompleted=1\n").expect("marker done");
}

fn compile_pack(
    work: &Path,
    fixture_dir: &Path,
    visibility: &str,
    output_name: &str,
) -> (String, String) {
    let output = work.join(output_name);
    let digest_path = work.join(format!("{output_name}.digest"));
    let produced = Command::new(required_env("OO_A341_FIXTURE_BIN"))
        .arg("compile")
        .arg("--compiler")
        .arg(required_env("OO_A341_ROLE_PACK_BIN"))
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
    let tool_stderr = String::from_utf8_lossy(&produced.stderr).into_owned();
    assert!(
        produced.status.success(),
        "compile {visibility} failed: {tool_stderr}"
    );
    let text = std::fs::read_to_string(&output).expect("compiled pack");
    let digest = std::fs::read_to_string(&digest_path)
        .expect("digest")
        .trim()
        .to_owned();
    assert_eq!(digest.len(), 64, "compiler digest length");
    assert!(
        digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
    (text, digest)
}

fn refuse_hidden(work: &Path, fixture_dir: &Path) {
    let stderr_path = work.join("hidden.stderr");
    let produced = Command::new(required_env("OO_A341_FIXTURE_BIN"))
        .arg("refuse")
        .arg("--compiler")
        .arg(required_env("OO_A341_ROLE_PACK_BIN"))
        .arg("--effective")
        .arg(fixture_dir.join("effective.json"))
        .arg("--visibility")
        .arg(fixture_dir.join("visibility-hidden.json"))
        .arg("--stderr")
        .arg(&stderr_path)
        .stdin(Stdio::null())
        .output()
        .expect("fixture spawn");
    let tool_stderr = String::from_utf8_lossy(&produced.stderr).into_owned();
    assert!(
        produced.status.success(),
        "hidden visibility was not refused: {tool_stderr}"
    );
    let compiler = std::fs::read_to_string(&stderr_path).unwrap_or_default();
    assert!(
        compiler.contains("classification.unknown_pii"),
        "compiler stderr missed unknown PII"
    );
    assert!(
        !work.join(".hidden.bundle").exists(),
        "refused bundle was left behind"
    );
}

fn load_only(work: &Path, files: &[(&str, &str)]) -> PackCatalog {
    let dir = work.join("loaded");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("load dir");
    for (name, text) in files {
        std::fs::write(dir.join(name), text).expect("pack");
    }
    PackCatalog::load_dir(&dir).expect("compiler packs load")
}

fn reason(dir: &Path, text: &str) -> &'static str {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).expect("reason dir");
    std::fs::write(dir.join("picker.json"), text).expect("tampered pack");
    PackCatalog::load_dir(dir)
        .expect_err("tampered pack loaded")
        .reason
}

fn mutate(text: &str, edit: impl FnOnce(&mut Value)) -> String {
    let mut value: Value = serde_json::from_str(text).expect("artifact json");
    edit(&mut value);
    serde_json::to_string(&value).expect("rewritten artifact")
}

fn column_names(columns: &[oo_gateway::pack_catalog::PackColumn]) -> Vec<&str> {
    columns.iter().map(|column| column.name.as_str()).collect()
}

fn assert_role_metadata(
    body: &[u8],
    artifact: &Value,
    digest: &str,
    principal: &str,
    role: &str,
    columns: &[&str],
    token: &str,
) {
    let text = body_text(body);
    assert!(!text.contains(token), "bearer echoed");
    assert!(
        !text.contains("does not authenticate"),
        "pack prose returned"
    );
    assert!(!text.contains("unclassified_pii"), "hidden column returned");
    let meta = body_json(body);
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
        got.push(column["name"].as_str().expect("column name"));
    }
    assert_eq!(got.as_slice(), columns);
    let http_files = meta["files"].as_array().expect("http files");
    let files = artifact["files"].as_array().expect("artifact files");
    assert_eq!(http_files.len(), files.len());
    for http_file in http_files {
        assert!(http_file.get("text").is_none(), "file text field");
        let path = http_file["path"].as_str().expect("path");
        let declared = files
            .iter()
            .find(|file| file["path"] == path)
            .expect("declared file");
        let file_text = declared["text"].as_str().expect("file text");
        let sha = http_file["sha256"].as_str().expect("sha");
        assert_eq!(sha, declared["sha256"].as_str().expect("declared sha"));
        assert_eq!(sha, sha256_hex(file_text.as_bytes()));
        assert_eq!(
            http_file["bytes"].as_u64().expect("bytes"),
            file_text.len() as u64
        );
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn trim_secret(value: &str) -> &str {
    value.trim_end_matches(['\n', '\r'])
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
            sandbox_id: Some("sb-1".to_owned()),
            session_id: Some("ss-1".to_owned()),
        },
    }
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn read_secret(path: &Path) -> String {
    let text = std::fs::read_to_string(path).expect("secret file");
    if text.is_empty() {
        panic!("empty secret");
    }
    text
}

fn write_config(
    runtime_file: &Path,
    ca_file: &Path,
    host: &str,
    port: u16,
    database: &str,
    packs: &[(&str, &str)],
) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oo-a341-http-{}-{}",
        std::process::id(),
        packs.len()
    ));
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

fn assert_process_refused(
    runtime_file: &Path,
    ca_file: &Path,
    host: &str,
    port: u16,
    database: &str,
    pack: &str,
) {
    let dir = write_config(
        runtime_file,
        ca_file,
        host,
        port,
        database,
        &[("picker.json", pack)],
    );
    let mut running = Running {
        child: spawn_gateway(&dir),
    };
    let started = Instant::now();
    let code = loop {
        if let Some(status) = running.child.try_wait().expect("wait") {
            break status.code();
        }
        if dir.join("listen.addr").exists() || started.elapsed() > Duration::from_secs(5) {
            panic!("tampered pack listened or hung");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(code, Some(1), "tampered pack exit");
    assert!(
        !dir.join("listen.addr").exists(),
        "listen.addr after refusal"
    );
    let stderr = std::fs::read_to_string(dir.join("stderr.txt")).unwrap_or_default();
    assert!(stderr.contains("oo-gateway: packs"), "refusal stderr");
}

struct Running {
    child: Child,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_gateway(dir: &Path) -> Child {
    let bin = env!("CARGO_BIN_EXE_oo-gateway");
    let _ = std::fs::remove_file(dir.join("listen.addr"));
    let stderr = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("stderr.txt"))
        .expect("stderr");
    let path = std::env::var("PATH").unwrap_or_default();
    Command::new(bin)
        .arg(dir)
        .env_clear()
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("spawn gateway")
}

fn wait_addr(dir: &Path) -> SocketAddr {
    let path = dir.join("listen.addr");
    let started = Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(addr) = text.trim().parse::<SocketAddr>()
        {
            return addr;
        }
        if started.elapsed() > Duration::from_secs(8) {
            panic!("gateway did not write listen.addr");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn sleep_until(deadline: Instant) {
    let now = Instant::now();
    if deadline > now {
        std::thread::sleep(deadline - now);
    }
}

fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    extra: Option<(&str, &str)>,
) -> (u16, Vec<u8>) {
    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    if let Some(token) = bearer {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    if let Some((name, value)) = extra {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    exchange(addr, request.as_bytes())
}

fn post(addr: SocketAddr, path: &str, bearer: Option<&str>, body: &str) -> (u16, Vec<u8>) {
    let length = body.len();
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {length}\r\n"
    );
    if let Some(token) = bearer {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    exchange(addr, request.as_bytes())
}

fn exchange(addr: SocketAddr, request: &[u8]) -> (u16, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(8)))
        .expect("read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(8)))
        .expect("write timeout");
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

fn assert_boundary(body: &[u8]) {
    let value = body_json(body);
    assert_eq!(value["authentication"], "loopback-http");
    assert_eq!(value["external_mtls"], false);
    assert_eq!(value["oauth"], false);
    assert_eq!(value["idp"], false);
}

fn error_code(body: &[u8]) -> String {
    let value = body_json(body);
    assert!(value.get("query_id").is_none(), "query_id");
    let error = value["error"].as_object().expect("error");
    assert!(error.get("query_id").is_none(), "query_id in error");
    assert_eq!(error.len(), 4);
    error["code"].as_str().expect("code").to_owned()
}
