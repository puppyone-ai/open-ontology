#![forbid(unsafe_code)]

mod idp;

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use agent_entry::{cookie_pair, header, https_exchange, receipt_closed, status_line, ParsedHttp, PARENT_ACCEPTANCE, QUESTION_CLARIFY, QUESTION_COUNT, QUESTION_PHONE};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use rustls::{ClientConfig, RootCertStore};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_postgres::config::SslMode;
use tokio_postgres_rustls::MakeRustlsConnect;

#[tokio::test]
#[ignore]
async fn real_agent_loop() {
    assert_eq!(std::env::var("OO_A397_LIVE").unwrap_or_default(), "1", "set OO_A397_LIVE=1");
    let env = Env::load();
    fs::create_dir_all(&env.evidence).unwrap();
    fs::create_dir_all(&env.state).unwrap();
    let mut failures = Vec::new();
    let mut chain = Chain::default();
    phase(&env, "binaries");
    if let Err(err) = require_binaries() {
        failures.push(err);
        fail_closed(&env, &mut failures, &secret_list(&env), &chain);
        return;
    }
    chain.binary = true;
    phase(&env, "production");
    if let Err(err) = production_refuses(&env).await {
        failures.push(err);
    }
    phase(&env, "shop");
    let admin_pass = read_secret(&env.pass("session-admin"));
    let gateway_pass = read_secret(&env.pass("gateway"));
    let identity_pass = read_secret(&env.pass("identity"));
    let mut secrets = secret_list(&env);
    if let Err(err) = load_shop(&env, &admin_pass).await {
        failures.push(err);
        fail_closed(&env, &mut failures, &secrets, &chain);
        return;
    }
    phase(&env, "init-pg");
    if let Err(err) = install_catalog(&env, &admin_pass, &secrets).await {
        failures.push(err);
    }
    phase(&env, "catalog-lock");
    if let Err(err) = grant_session_locks(&env).await {
        failures.push(err);
        fail_closed(&env, &mut failures, &secrets, &chain);
        return;
    }
    phase(&env, "operators");
    let (wide_token, wide_hash) = match mint_operator(&env.state.join("operators/wide.token")) {
        Ok(pair) => pair,
        Err(err) => {
            failures.push(err);
            fail_closed(&env, &mut failures, &secrets, &chain);
            return;
        }
    };
    let (narrow_token, narrow_hash) = match mint_operator(&env.state.join("operators/narrow.token")) {
        Ok(pair) => pair,
        Err(err) => {
            failures.push(err);
            fail_closed(&env, &mut failures, &secrets, &chain);
            return;
        }
    };
    secrets.push(wide_token.clone());
    secrets.push(narrow_token.clone());
    keep_active_release_identity_path(&env, &identity_pass).await;
    phase(&env, "session-api");
    let _session_proc = match boot_session(&env, &admin_pass).await {
        Ok(proc) => proc,
        Err(err) => {
            failures.push(err);
            fail_closed(&env, &mut failures, &secrets, &chain);
            return;
        }
    };
    phase(&env, "release");
    let commit = fs::read_to_string(env.evidence.join("source-commit.txt")).unwrap_or_default();
    let commit = commit.trim().to_owned();
    if commit.len() != 40 || !commit.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
        failures.push("source commit is not 40 lowercase hex".to_owned());
        fail_closed(&env, &mut failures, &secrets, &chain);
        return;
    }
    let release_state = env.state.join("release-state.json");
    if let Err(err) = write_release(&release_state, &[]) {
        failures.push(err);
        fail_closed(&env, &mut failures, &secrets, &chain);
        return;
    }
    let release_port = match agent_entry::serve_private_release(release_state.clone()).await {
        Ok(port) => port,
        Err(err) => {
            failures.push(format!("release producer {err}"));
            fail_closed(&env, &mut failures, &secrets, &chain);
            return;
        }
    };
    let release_origin = format!("http://127.0.0.1:{release_port}");
    phase(&env, "product-gateway");
    let product_gateway = match boot_product_gateway(&env, &secrets).await {
        Ok(proc) => Some(proc),
        Err(err) => {
            failures.push(err);
            None
        }
    };
    let _hold_gateway = std::hint::black_box(&product_gateway);
    phase(&env, "agent-entry");
    let entry = match boot_entry(&env, &gateway_pass, &release_origin, &wide_hash, &narrow_hash).await {
        Ok(entry) => entry,
        Err(err) => {
            failures.push(err);
            fail_closed(&env, &mut failures, &secrets, &chain);
            return;
        }
    };
    let _hold_entry = std::hint::black_box(&entry.proc);
    let port = match wait_port(&env.state.join("entry/port.txt"), 30).await {
        Ok(port) => port,
        Err(err) => {
            failures.push(err);
            fail_closed(&env, &mut failures, &secrets, &chain);
            return;
        }
    };
    phase(&env, "draft");
    let _ = fs::write(env.evidence.join("pack-mode.txt"), "development_draft\nactive_publication_not_required\n");
    if let Err(err) = write_release(&release_state, &[
        release_row("oo_r_wide", true, &entry.wide_sha, &commit),
        release_row("oo_r_narrow", true, &entry.narrow_sha, &commit),
    ]) {
        failures.push(err);
    }
    phase(&env, "auth");
    let audit_before = audit_count(&env, None);
    let mut auth_ok = true;
    let unknown = agent_entry::hex_encode(&agent_entry::sha256(b"a397-unknown-operator-token"));
    let self_body = json!({"principal_id": "wideowner"}).to_string();
    let pack_body = json!({"pack_sha256": entry.wide_sha, "source_git_commit": commit}).to_string();
    let probes = [
        ("missing", "", "{}"),
        ("marker", "public-proxy-marker", "{}"),
        ("unknown", unknown.as_str(), "{}"),
        ("self", wide_token.as_str(), self_body.as_str()),
        ("self-pack", wide_token.as_str(), pack_body.as_str()),
    ];
    let expects = [401_u16, 401, 401, 422, 422];
    for (index, probe) in probes.iter().enumerate() {
        let (label, bearer, body) = *probe;
        let expect = expects[index];
        match call_bearer(port, "POST", "/entry/v1/sessions", bearer, body).await {
            Ok((status, _)) if status == expect => {}
            Ok((status, resp)) => {
                auth_ok = false;
                failures.push(format!("auth {label} {status} {}", clip(&body_text(&resp))));
            }
            Err(err) => {
                auth_ok = false;
                failures.push(err);
            }
        }
    }
    let audit_after_auth = audit_count(&env, None);
    if audit_after_auth != audit_before {
        auth_ok = false;
        failures.push(format!("auth refusal wrote query audit {audit_before} -> {audit_after_auth}"));
    }
    phase(&env, "wide-session");
    let wide = match create_session(port, &wide_token, &entry.wide_sha).await {
        Ok(session) => session,
        Err(err) => {
            failures.push(err);
            fail_closed(&env, &mut failures, &secrets, &chain);
            return;
        }
    };
    if auth_ok {
        chain.auth = true;
    }
    let _ = fs::write(
        env.evidence.join("release-binding.json"),
        json!({
            "operation": "getActiveRolePack",
            "bind": "bindActiveReleasePack",
            "source_git_commit": commit,
            "policy": "pol-1",
            "knowledge": "know-1",
            "wide_pack_sha256": entry.wide_sha,
            "narrow_pack_sha256": entry.narrow_sha,
            "create_pack_sha256": wide.pack_sha256,
            "parent_acceptance": false
        })
        .to_string(),
    );
    phase(&env, "sdk-fixture");
    if let Err(err) = sdk_fixture(&env, &wide).await {
        failures.push(err);
    }
    phase(&env, "narrow-phone");
    let narrow = match create_session(port, &narrow_token, &entry.narrow_sha).await {
        Ok(session) => Some(session),
        Err(err) => {
            failures.push(err);
            None
        }
    };
    let narrow_before = audit_count(&env, Some("narrowowner"));
    if let Some(narrow) = &narrow {
        match model_turn(port, narrow, &narrow_token, QUESTION_PHONE, 900).await {
            Ok(events) => {
                note_model(&mut chain, &events);
                publish_events(&env, "phone-events.json", &events);
                if events.iter().any(|event| event["kind"] == "turn_finished") {
                    failures.push("phone denial emitted turn_finished".to_owned());
                }
                if !events.iter().any(|event| event["kind"] == "tool_call_started" || event["kind"] == "tool_call_finished") {
                    failures.push("phone denial did not call the query tool".to_owned());
                }
            }
            Err(err) => failures.push(err),
        }
    }
    let narrow_after = audit_count(&env, Some("narrowowner"));
    if narrow_after != narrow_before {
        failures.push(format!("phone denial inserted query audit {narrow_before} -> {narrow_after}"));
    }
    phase(&env, "clarify");
    let wide_before_clarify = audit_count(&env, Some("wideowner"));
    match model_turn(port, &wide, &wide_token, QUESTION_CLARIFY, 900).await {
        Ok(events) => {
            note_model(&mut chain, &events);
            publish_events(&env, "clarify-events.json", &events);
            if events.iter().any(|event| event["kind"] == "tool_call_started" || event["kind"] == "turn_finished") {
                failures.push("clarification called a tool or finished".to_owned());
            }
            if !events.iter().any(|event| event["kind"] == "clarification") {
                failures.push("clarification event missing".to_owned());
            }
        }
        Err(err) => failures.push(err),
    }
    let wide_after_clarify = audit_count(&env, Some("wideowner"));
    if wide_after_clarify != wide_before_clarify {
        failures.push("clarification inserted a query audit row".to_owned());
    }
    phase(&env, "count");
    let before_count = audit_count(&env, Some("wideowner"));
    match model_turn(port, &wide, &wide_token, QUESTION_COUNT, 900).await {
        Ok(events) => {
            note_model(&mut chain, &events);
            if events.iter().any(|event| event["kind"] == "tool_call_started" || event["kind"] == "tool_call_finished") {
                chain.tool = true;
            }
            publish_events(&env, "count-events.json", &events);
            match events.iter().find(|event| event["kind"] == "turn_finished") {
                Some(event) if event["complete"] == true => {
                    let query_id = event["query_id"].as_str().unwrap_or("");
                    if let Err(err) = assert_gold_row(&env, query_id, &wide.pack_sha256) {
                        failures.push(err);
                    } else {
                        chain.query = true;
                        chain.audit = true;
                    }
                    let saw_200 = events.iter().any(|item| item["model_http_status"] == 200);
                    let _ = fs::write(env.evidence.join("model-http-200-seen.txt"), if saw_200 { "true\n" } else { "false\n" });
                }
                Some(_) => failures.push("count turn_finished was not complete".to_owned()),
                None => failures.push("count turn did not finish from the audit citation".to_owned()),
            }
            if audit_count(&env, Some("wideowner")) > before_count {
                chain.query = true;
            }
        }
        Err(err) => failures.push(err),
    }
    phase(&env, "revoke");
    if let Err(err) = revoke_wide(&env, &wide, &entry.narrow_sha, &commit).await {
        failures.push(err);
    }
    phase(&env, "cancel");
    if let Err(err) = cancel_turn(port, &wide, &wide_token).await {
        failures.push(err);
    }
    phase(&env, "sigkill");
    if let Err(err) = sigkill_turn(port, &wide, &wide_token, &env).await {
        failures.push(err);
    }
    phase(&env, "close");
    match call_bearer(port, "POST", &format!("/entry/v1/sessions/{}/close", wide.id), &wide_token, "{}").await {
        Ok((status, body)) => {
            let text = body_text(&body);
            let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            publish_safe(&env, "close.json", &text, &secrets);
            if status == 200 && receipt_closed(status, &value["receipt"]) {
                chain.close = true;
            } else {
                failures.push(format!("close {status} {}", clip(&text)));
            }
        }
        Err(err) => failures.push(err),
    }
    if let Ok(path) = fs::read_to_string(env.state.join("entry/upstream-path.txt")) {
        publish_safe(&env, "upstream-path.txt", &path, &secrets);
    }
    for name in ["mtls-hop.txt", "proxy-log.txt", "query-receipts.jsonl"] {
        if let Ok(text) = fs::read_to_string(env.state.join("entry").join(name)) {
            publish_safe(&env, name, &text, &secrets);
        }
    }
    fail_closed(&env, &mut failures, &secrets, &chain);
}

struct Env {
    evidence: PathBuf,
    state: PathBuf,
}

impl Env {
    fn load() -> Self {
        Self { evidence: PathBuf::from(req("OO_A397_EVIDENCE")), state: PathBuf::from(req("OO_A397_STATE")) }
    }

    fn pass(&self, name: &str) -> PathBuf {
        PathBuf::from(req("OO_A397_SECRET_DIR")).join(format!("{name}.pass"))
    }
}

fn req(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("missing {key}"))
}

fn phase(env: &Env, name: &str) {
    let _ = fs::write(env.evidence.join("phase.txt"), format!("{name}\n"));
}

#[derive(Clone, Copy, Default)]
struct Chain {
    binary: bool,
    auth: bool,
    model: bool,
    tool: bool,
    query: bool,
    audit: bool,
    close: bool,
}

fn fail_closed(env: &Env, failures: &mut Vec<String>, secrets: &[String], chain: &Chain) {
    for (name, ok) in [
        ("binary", chain.binary),
        ("auth", chain.auth),
        ("model", chain.model),
        ("tool", chain.tool),
        ("query", chain.query),
        ("audit", chain.audit),
        ("close", chain.close),
    ] {
        if !ok {
            let item = format!("missing {name}");
            if !failures.iter().any(|have| have == &item) {
                failures.push(item);
            }
        }
    }
    finish(env, failures, secrets, chain);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn finish(env: &Env, failures: &[String], secrets: &[String], chain: &Chain) {
    let body = json!({
        "parent_acceptance": PARENT_ACCEPTANCE,
        "production_signed": false,
        "producer_query": "query-pg",
        "product_http_sql": "unsupported",
        "product_route": "mtls_health",
        "pack_mode": "development_draft",
        "deployment": "localprocess",
        "runsc": "absent_development_local_process",
        "mp01": false,
        "import_source": "private_fixture",
        "chain": {
            "binary": chain.binary,
            "auth": chain.auth,
            "model": chain.model,
            "tool": chain.tool,
            "query": chain.query,
            "audit": chain.audit,
            "close": chain.close
        },
        "failures": failures,
    });
    publish_safe(env, "result.json", &body.to_string(), secrets);
}

fn require_binaries() -> Result<(), String> {
    for key in [
        "OO_A397_ENTRY_BIN",
        "OO_A397_OO_BIN",
        "OO_A397_QUERY_PG",
        "OO_A397_OO_GATEWAY",
        "OO_A397_PI",
        "OO_A397_NODE",
        "OO_A397_SESSION_BIN",
        "OO_A397_INIT_PG",
        "OO_A397_UPSTREAM_TOKEN",
    ] {
        let path = req(key);
        if !Path::new(&path).is_file() {
            return Err(format!("missing binary {key}"));
        }
    }
    Ok(())
}

fn mint_operator(path: &Path) -> Result<(String, String), String> {
    let mut bytes = [0_u8; 32];
    std::io::Read::read_exact(&mut File::open("/dev/urandom").map_err(|err| err.to_string())?, &mut bytes).map_err(|err| err.to_string())?;
    let token = agent_entry::hex_encode(&bytes);
    if token.len() < 43 || token == "public-proxy-marker" {
        return Err("operator token".to_owned());
    }
    let hash = agent_entry::hex_encode(&agent_entry::sha256(token.as_bytes()));
    write_0600(path, &token)?;
    Ok((token, hash))
}

fn note_model(chain: &mut Chain, events: &[Value]) {
    if events.iter().any(|event| {
        matches!(event["kind"].as_str(), Some("assistant_delta" | "model_stop" | "tool_call_started" | "tool_call_finished" | "clarification"))
    }) {
        chain.model = true;
    }
}

async fn keep_active_release_identity_path(env: &Env, identity_pass: &str) {
    if std::env::var_os("OO_A397_RUN_IDENTITY").is_some() {
        let _ = boot_identity(env, identity_pass).await;
    }
}

struct Proc(Option<std::process::Child>);

impl Drop for Proc {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

async fn production_refuses(env: &Env) -> Result<(), String> {
    let dir = env.state.join("production");
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let config = dir.join("config.json");
    write_json(&config, &dummy_config(&dir))?;
    let stderr = env.evidence.join("production.stderr");
    let stdout = env.evidence.join("production.stdout");
    let status = Command::new(req("OO_A397_ENTRY_BIN"))
        .arg("serve")
        .stdin(File::open(&config).map_err(|err| err.to_string())?)
        .stdout(File::create(&stdout).map_err(|err| err.to_string())?)
        .stderr(File::create(&stderr).map_err(|err| err.to_string())?)
        .status()
        .map_err(|err| err.to_string())?;
    let code = status.code().ok_or("production signal")?;
    let err = fs::read_to_string(&stderr).unwrap_or_default();
    if code != 2 || !(err.contains("runsc_absent") || err.contains("production_driver_absent")) {
        return Err(format!("production exit {code} {}", clip(&err)));
    }
    if dir.join("port.txt").exists() {
        return Err("production wrote a port".to_owned());
    }
    Ok(())
}

fn dummy_config(dir: &Path) -> Value {
    let hash = "ab".repeat(32);
    json!({
        "contract": "agent-entry-v1",
        "profile": "production",
        "state_dir": dir,
        "identity_origin": "https://127.0.0.1:9",
        "identity_server_name": "127.0.0.1",
        "identity_ca_pem": "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n",
        "session_bin": "/bin/true",
        "session_config_path": "/bin/true",
        "session_port_file": "/bin/true",
        "mapping_path": "/bin/true",
        "pack_root": "/bin",
        "release_origin": "http://127.0.0.1:9",
        "pack_mode": "active_release",
        "query_pg_bin": "/bin/true",
        "query_policy_dir": "/bin",
        "gateway_addr_file": "/bin/true",
        "mtls_ca_file": "/bin/true",
        "mtls_cert_file": "/bin/true",
        "mtls_key_file": "/bin/true",
        "oo_bin": "/bin/true",
        "pi_executable": "/bin/true",
        "pi_executable_sha256": hash,
        "pi_runtime": "/bin/true",
        "pi_runtime_sha256": hash,
        "upstream_token_file": "/bin/true",
        "pg": {"host": "oo-a397-pg", "port": 5432, "database": "oo_a397", "user": "oo_gateway_login", "password": "unused", "ca_pem": "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n"}
    })
}

async fn load_shop(env: &Env, admin_pass: &str) -> Result<(), String> {
    let client = pg(env, "oo_session_admin", admin_pass).await?;
    client.batch_execute(SHOP_SQL).await.map_err(|err| format!("shop {err}"))?;
    let party = client.query_one("SELECT display_name, active FROM shop.party WHERE party_key = 'p00000'", &[]).await.map_err(|err| err.to_string())?;
    let display: Option<String> = party.get(0);
    let active: bool = party.get(1);
    if display.is_some() || !active {
        return Err("party 0 gold mismatch".to_owned());
    }
    for k in 0..8 {
        let row = client.query_one("SELECT qty, amount::text FROM shop.entry WHERE entry_id = $1", &[&k]).await.map_err(|err| err.to_string())?;
        let qty: i64 = row.get(0);
        let amount: Option<String> = row.get(1);
        if qty != agent_entry::qty(k) || amount != agent_entry::amount_text(k) {
            return Err(format!("entry {k} gold mismatch"));
        }
    }
    let count: i64 = client.query_one("SELECT count(*) FROM shop.entry", &[]).await.map_err(|err| err.to_string())?.get(0);
    if count != 90_000 {
        return Err(format!("entry count {count}"));
    }
    Ok(())
}

struct IdentityProc {
    _idp: idp::Idp,
    _server: Proc,
    _admin_cookie: String,
    _admin_csrf: String,
    _wide_cookie: String,
    _wide_csrf: String,
    _narrow_cookie: String,
    _narrow_csrf: String,
}

async fn boot_identity(env: &Env, identity_pass: &str) -> Result<IdentityProc, String> {
    let idp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|err| err.to_string())?;
    let idp_port = idp_listener.local_addr().map_err(|err| err.to_string())?.port();
    let identity_port = reserve_port().await?;
    let issuer = format!("https://127.0.0.1:{idp_port}");
    let origin = format!("https://127.0.0.1:{identity_port}");
    let redirect = format!("{origin}/control/v1/auth/callback");
    let dir = env.state.join("identity");
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let secret = read_secret(&PathBuf::from(req("OO_A397_CLIENT_SECRET")));
    write_0600(&dir.join("client.secret"), &secret)?;
    write_0600(&dir.join("pg.pass"), identity_pass)?;
    let ca = fs::read(req("OO_A397_CA")).map_err(|err| err.to_string())?;
    let commit = config_commit(&issuer, &redirect);
    write_json(&dir.join("config.json"), &identity_config(&dir, &origin, identity_port, &issuer, &redirect, &commit, env))?;
    let cert = fs::read(req("OO_A397_TLS_CERT")).map_err(|err| err.to_string())?;
    let key = fs::read(req("OO_A397_TLS_KEY")).map_err(|err| err.to_string())?;
    let jwt = fs::read(req("OO_A397_JWT_PKCS8")).map_err(|err| err.to_string())?;
    let idp = idp::start(idp_listener, idp::Parts {
        cert_pem: &cert,
        key_pem: &key,
        issuer: &issuer,
        client_id: "oo-a397-client",
        client_secret: &secret,
        redirect_uri: &redirect,
        pkcs8_der: &jwt,
    }).await;
    let bin = req("OO_A397_IDENTITY_BIN");
    let install_err = env.evidence.join("identity-install.stderr");
    let code = run_to_exit(&bin, &["install", "--config", dir.join("config.json").to_str().unwrap()], &install_err)?;
    if code != 0 {
        return Err(format!("identity install {code} {}", clip(&fs::read_to_string(&install_err).unwrap_or_default())));
    }
    let bootstrap = dir.join("bootstrap.token");
    let code = run_to_exit(&bin, &["bootstrap-issue", "--config", dir.join("config.json").to_str().unwrap(), "--out", bootstrap.to_str().unwrap()], &env.evidence.join("identity-bootstrap.stderr"))?;
    if code != 0 {
        return Err("identity bootstrap failed".to_owned());
    }
    let token = fs::read_to_string(&bootstrap).map_err(|err| err.to_string())?;
    let token = token.trim();
    let log = env.evidence.join("identity-serve.stdout");
    let server = spawn_no_stdin(&bin, &["serve", "--config", dir.join("config.json").to_str().unwrap()], &log, &env.evidence.join("identity-serve.stderr"))?;
    if !wait_contains(&log, "listening", 30).await {
        return Err("identity did not listen".to_owned());
    }
    idp.set_subject("subadmin");
    let (admin_cookie, admin_csrf) = login(&ca, identity_port, "subadmin", Some(token)).await?;
    idp.set_subject("subwide");
    let (wide_cookie, wide_csrf) = login(&ca, identity_port, "subwide", None).await?;
    idp.set_subject("subnarrow");
    let (narrow_cookie, narrow_csrf) = login(&ca, identity_port, "subnarrow", None).await?;
    Ok(IdentityProc {
        _idp: idp,
        _server: server,
        _admin_cookie: admin_cookie,
        _admin_csrf: admin_csrf,
        _wide_cookie: wide_cookie,
        _wide_csrf: wide_csrf,
        _narrow_cookie: narrow_cookie,
        _narrow_csrf: narrow_csrf,
    })
}

async fn boot_session(env: &Env, admin_pass: &str) -> Result<Proc, String> {
    let dir = env.state.join("session");
    fs::create_dir_all(dir.join("run")).map_err(|err| err.to_string())?;
    write_json(&dir.join("bindings.json"), &json!({"bindings": [
        binding("wideowner"),
        binding("narrowowner")
    ]}))?;
    let config = session_config(&dir, env, admin_pass)?;
    write_json(&dir.join("config.json"), &config)?;
    let bin = req("OO_A397_SESSION_BIN");
    let code = run_stdin(&bin, "install", &dir.join("config.json"), &env.evidence.join("session-install.stdout"), &env.evidence.join("session-install.stderr"))?;
    if code != 0 {
        return Err(format!("session install {code}"));
    }
    let proc = spawn_stdin(&bin, "serve", &dir.join("config.json"), &env.evidence.join("session-serve.stdout"), &env.evidence.join("session-serve.stderr"))?;
    wait_port(&dir.join("run/port.txt"), 30).await?;
    Ok(proc)
}

struct EntryBoot {
    proc: Proc,
    wide_sha: String,
    narrow_sha: String,
}

async fn boot_entry(env: &Env, gateway_pass: &str, release_origin: &str, wide_hash: &str, narrow_hash: &str) -> Result<EntryBoot, String> {
    let dir = env.state.join("entry");
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let packs = dir.join("packs");
    fs::create_dir_all(packs.join("oo_r_wide/tables")).map_err(|err| err.to_string())?;
    fs::create_dir_all(packs.join("oo_r_narrow/tables")).map_err(|err| err.to_string())?;
    fs::write(packs.join("oo_r_wide/role.txt"), b"wide\n").map_err(|err| err.to_string())?;
    fs::write(packs.join("oo_r_narrow/role.txt"), b"narrow\n").map_err(|err| err.to_string())?;
    fs::write(packs.join("oo_r_wide/README.md"), b"Shop development_draft preview. Not an active production publication.\nSchema shop contains tables party, account, and entry.\nRead tables/ before writing SQL. national_id is not in this role.\n").map_err(|err| err.to_string())?;
    fs::write(packs.join("oo_r_narrow/README.md"), b"Shop development_draft preview for the narrow role. Not an active production publication.\nSchema shop contains tables party, account, and entry.\n").map_err(|err| err.to_string())?;
    fs::write(packs.join("oo_r_wide/tables/party.md"), b"shop.party columns: party_key, display_name, active, phone.\n").map_err(|err| err.to_string())?;
    fs::write(packs.join("oo_r_wide/tables/account.md"), b"shop.account columns: account_id, party_key, opened, label.\n").map_err(|err| err.to_string())?;
    fs::write(packs.join("oo_r_wide/tables/entry.md"), b"shop.entry columns: entry_id, account_id, qty, amount, memo.\n").map_err(|err| err.to_string())?;
    fs::write(packs.join("oo_r_narrow/tables/party.md"), b"shop.party columns: party_key, display_name.\n").map_err(|err| err.to_string())?;
    fs::write(packs.join("oo_r_narrow/tables/account.md"), b"shop.account columns: account_id, party_key, opened.\n").map_err(|err| err.to_string())?;
    fs::write(packs.join("oo_r_narrow/tables/entry.md"), b"shop.entry columns: entry_id, account_id.\n").map_err(|err| err.to_string())?;
    write_policies(&dir.join("policies"))?;
    let wide_sha = agent_entry::pack_digest(&packs.join("oo_r_wide")).map_err(|_| "pack".to_owned())?;
    let narrow_sha = agent_entry::pack_digest(&packs.join("oo_r_narrow")).map_err(|_| "pack".to_owned())?;
    write_json(&dir.join("mapping.json"), &json!({"rows": [
        {"subject": "subwide", "project_id": "shop", "management_role": "business_owner", "data_role": "oo_r_wide", "session_principal": "wideowner", "operator_token_sha256": wide_hash},
        {"subject": "subnarrow", "project_id": "shop", "management_role": "auditor", "data_role": "oo_r_narrow", "session_principal": "narrowowner", "operator_token_sha256": narrow_hash}
    ]}))?;
    let generation = agent_entry::hex_encode(&agent_entry::sha256(b"a397-private-fixture-not-mp01"));
    let generation_path = dir.join("import-generation.json");
    write_json(&generation_path, &json!({
        "contract": "agent-entry-import-generation",
        "source": "private_fixture",
        "mp01": false,
        "generation": generation,
        "database": "oo_a397"
    }))?;
    let _ = fs::write(env.evidence.join("mp01.txt"), "false\nprivate_fixture\nnot_mp01\n");
    let _ = fs::write(env.evidence.join("import-generation.txt"), format!("{generation}\n"));
    let ca = fs::read_to_string(req("OO_A397_CA")).map_err(|err| err.to_string())?;
    let pi = req("OO_A397_PI");
    let node = req("OO_A397_NODE");
    let config = json!({
        "contract": "agent-entry-v1",
        "profile": "localprocess",
        "state_dir": dir,
        "identity_origin": "https://127.0.0.1:9",
        "identity_server_name": "127.0.0.1",
        "identity_ca_pem": ca,
        "session_bin": req("OO_A397_SESSION_BIN"),
        "session_config_path": env.state.join("session/config.json"),
        "session_port_file": env.state.join("session/run/port.txt"),
        "mapping_path": dir.join("mapping.json"),
        "pack_root": packs,
        "release_origin": release_origin,
        "pack_mode": "development_draft",
        "query_pg_bin": req("OO_A397_QUERY_PG"),
        "query_policy_dir": dir.join("policies"),
        "gateway_addr_file": env.state.join("product-gateway/listen.addr"),
        "mtls_ca_file": req("OO_A397_CA"),
        "mtls_cert_file": env.state.join("product-gateway/client.crt"),
        "mtls_key_file": env.state.join("product-gateway/client.key"),
        "oo_bin": req("OO_A397_OO_BIN"),
        "pi_executable": pi,
        "pi_executable_sha256": file_sha(&PathBuf::from(&pi))?,
        "pi_runtime": node,
        "pi_runtime_sha256": file_sha(&PathBuf::from(req("OO_A397_NODE")))?,
        "upstream_token_file": req("OO_A397_UPSTREAM_TOKEN"),
        "import_generation_file": generation_path,
        "pg": {
            "host": req("OO_A397_PG_HOST"),
            "port": req("OO_A397_PG_PORT").parse::<u16>().map_err(|_| "pg port")?,
            "database": "oo_a397",
            "user": "oo_gateway_login",
            "password": gateway_pass,
            "ca_pem": ca
        }
    });
    write_json(&dir.join("config.json"), &config)?;
    let proc = spawn_stdin(&req("OO_A397_ENTRY_BIN"), "serve", &dir.join("config.json"), &env.evidence.join("entry-serve.stdout"), &env.evidence.join("entry-serve.stderr"))?;
    Ok(EntryBoot { proc, wide_sha, narrow_sha })
}

struct SessionRef {
    id: String,
    pack_sha256: String,
}

async fn create_session(port: u16, bearer: &str, expected_pack: &str) -> Result<SessionRef, String> {
    let (status, body) = call_bearer(port, "POST", "/entry/v1/sessions", bearer, "{}").await?;
    if status != 201 {
        return Err(format!("create {status} {}", clip(&body_text(&body))));
    }
    let value: Value = serde_json::from_slice(&body).map_err(|err| err.to_string())?;
    if value["production_signed"] != false || value["parent_acceptance"] != false || value["deployment"] != "localprocess" {
        return Err("create flags".to_owned());
    }
    let pack = value["pack_sha256"].as_str().unwrap_or("");
    if pack != expected_pack || value["operation"] != "createSession" {
        return Err(format!("create pack {pack}"));
    }
    Ok(SessionRef { id: value["session_id"].as_str().unwrap_or("").to_owned(), pack_sha256: pack.to_owned() })
}

fn release_row(role: &str, published: bool, pack: &str, commit: &str) -> Value {
    release_row_commit(role, published, pack, commit, pack)
}

fn release_row_commit(role: &str, published: bool, pack: &str, commit: &str, content: &str) -> Value {
    let operation_id = if role == "oo_r_wide" { "rel-shop-wide" } else { "rel-shop-narrow" };
    json!({
        "project_id": "shop",
        "role": role,
        "published": published,
        "operation_id": operation_id,
        "pack_sha256": pack,
        "source_git_commit": commit,
        "policy": "pol-1",
        "knowledge": "know-1",
        "content_hash": content,
    })
}

fn write_release(path: &Path, rows: &[Value]) -> Result<(), String> {
    write_json(path, &json!({"packs": rows}))
}

fn write_policies(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    write_json(&dir.join("oo_r_wide.json"), &policy_doc("oo_r_wide", "wide", vec![
        table("party", vec![col("party_key", "text"), col("display_name", "text"), col("active", "bool"), col("phone", "text")]),
        table("account", vec![col("account_id", "int8"), col("party_key", "text"), col("opened", "bool"), col("label", "text")]),
        table("entry", vec![col("entry_id", "int8"), col("account_id", "int8"), col("qty", "int8"), col("memo", "text")]),
    ]))?;
    write_json(&dir.join("oo_r_narrow.json"), &policy_doc("oo_r_narrow", "narrow", vec![
        table("party", vec![col("party_key", "text"), col("display_name", "text")]),
        table("account", vec![col("account_id", "int8"), col("party_key", "text"), col("opened", "bool")]),
        table("entry", vec![col("entry_id", "int8"), col("account_id", "int8")]),
    ]))?;
    Ok(())
}

fn policy_doc(role: &str, slug: &str, tables: Vec<Value>) -> Value {
    let digest = "0123456789abcdef".repeat(4);
    let policies: Vec<Value> = tables.into_iter().map(|table| json!({
        "project": "shop",
        "config_digest": digest,
        "generation": 1,
        "role": slug,
        "login": "oo_gateway_login",
        "table": table
    })).collect();
    json!({
        "endpoint": {
            "host": req("OO_A397_PG_HOST"),
            "port": req("OO_A397_PG_PORT").parse::<u16>().unwrap_or(5432),
            "database": "oo_a397",
            "user": "oo_gateway_login"
        },
        "identity": {"project": "shop", "role": role, "policy_id": digest},
        "policies": policies,
        "catalog": {
            "max_tables": 8,
            "max_columns": 64,
            "max_functions": 1,
            "max_operators": 54,
            "large_table_row_threshold": 1000000,
            "unknown_rows": "refuse"
        }
    })
}

fn table(name: &str, columns: Vec<Value>) -> Value {
    json!({"namespace": "shop", "name": name, "columns": columns})
}

fn col(name: &str, semantic: &str) -> Value {
    json!({"name": name, "status": "confirmed", "semantic_type": semantic, "pii": {"class": "none"}})
}

async fn install_catalog(env: &Env, admin_pass: &str, secrets: &[String]) -> Result<(), String> {
    let request = json!({
        "protocol": "oo.pipeline.init-pg/v1",
        "target": {
            "host": req("OO_A397_PG_HOST"),
            "port": req("OO_A397_PG_PORT").parse::<u16>().map_err(|_| "pg port")?,
            "database": "oo_a397",
            "administrator": "oo_fixture_admin",
            "password_file": PathBuf::from(req("OO_A397_SECRET_DIR")).join("fixture-admin.pass"),
            "ca_file": req("OO_A397_CA")
        },
        "connect_ms": 30000
    });
    let stdout = env.state.join("init-pg.stdout");
    let stderr = env.state.join("init-pg.stderr");
    let code = run_piped(&req("OO_A397_INIT_PG"), request.to_string().as_bytes(), &stdout, &stderr, 180).await?;
    let err = fs::read_to_string(&stderr).unwrap_or_default();
    publish_safe(env, "init-pg.stderr", &clip(&err), secrets);
    publish_safe(env, "init-pg.exit", &format!("{code}\n"), secrets);
    if code == 0 {
        return Ok(());
    }
    if oo_schema_present(env, admin_pass).await {
        publish_safe(env, "init-pg.txt", &format!("exit {code}; _oo already present\n"), secrets);
        return Ok(());
    }
    Err(format!("init-pg exit {code} {}", clip(&err)))
}

async fn grant_session_locks(env: &Env) -> Result<(), String> {
    let pass = read_secret(&env.pass("fixture-admin"));
    let client = pg(env, "oo_fixture_admin", &pass).await.map_err(|_| "grant lock connect".to_owned())?;
    client
        .batch_execute(
            "GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_xact_lock(bigint) TO oo_session_admin, oo_session_api_admin, oo_token_admin, oo_token_runtime; GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_xact_lock(integer, integer) TO oo_token_admin, oo_token_runtime",
        )
        .await
        .map_err(|_| "grant lock refused".to_owned())?;
    Ok(())
}

async fn oo_schema_present(env: &Env, admin_pass: &str) -> bool {
    let Ok(client) = pg(env, "oo_session_admin", admin_pass).await else { return false };
    client.query_opt("SELECT 1 FROM pg_namespace WHERE nspname = '_oo'", &[]).await.ok().flatten().is_some()
}

async fn boot_product_gateway(env: &Env, secrets: &[String]) -> Result<Proc, String> {
    let dir = env.state.join("product-gateway");
    fs::create_dir_all(dir.join("packs")).map_err(|err| err.to_string())?;
    write_0600(&dir.join("runtime.pass"), "local-runtime-marker\n")?;
    fs::copy(req("OO_A397_CA"), dir.join("pg-ca.crt")).map_err(|err| err.to_string())?;
    fs::copy(req("OO_A397_CA"), dir.join("client-ca.crt")).map_err(|err| err.to_string())?;
    let endpoint = format!("{{\"host\":\"{}\",\"port\":{},\"database\":\"oo_a397\"}}\n", req("OO_A397_PG_HOST"), req("OO_A397_PG_PORT"));
    fs::write(dir.join("endpoint.json"), endpoint).map_err(|err| err.to_string())?;
    mint_cert(&dir, "server", "2", "basicConstraints=CA:FALSE\nsubjectAltName=IP:127.0.0.1\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n")?;
    mint_cert(&dir, "client", "3", "basicConstraints=CA:FALSE\nextendedKeyUsage=clientAuth\nsubjectAltName=URI:urn:oo:workload:agent-entry\n")?;
    let config = "{\"listen\":\"127.0.0.1:0\",\"transport\":\"tls\",\"audience\":\"https://oo-a397.example/v1\",\"runtime_user\":\"oo_token_runtime\",\"password_file\":\"runtime.pass\",\"ca_file\":\"pg-ca.crt\",\"endpoint_file\":\"endpoint.json\",\"pack_dir\":\"packs\",\"body_limit\":1048576,\"request_timeout_ms\":5000,\"global_concurrency\":2,\"tls_key_file\":\"server.key\",\"tls_chain_file\":\"server.crt\",\"client_ca_file\":\"client-ca.crt\",\"handshake_timeout_ms\":5000}\n";
    fs::write(dir.join("config.json"), config).map_err(|err| err.to_string())?;
    let stderr = env.state.join("oo-gateway.stderr");
    let stdout = env.state.join("oo-gateway.stdout");
    let proc = spawn_no_stdin(&req("OO_A397_OO_GATEWAY"), &[dir.to_str().ok_or("path")?], &stdout, &stderr)?;
    if let Err(err) = wait_listen(&dir.join("listen.addr"), 30).await {
        let logged = fs::read_to_string(&stderr).unwrap_or_default();
        publish_safe(env, "oo-gateway.stderr", &clip(&logged), secrets);
        return Err(format!("{err} {}", clip(&logged)));
    }
    Ok(proc)
}

fn mint_cert(dir: &Path, name: &str, serial: &str, ext: &str) -> Result<(), String> {
    let key = dir.join(format!("{name}.key"));
    let csr = dir.join(format!("{name}.csr"));
    let crt = dir.join(format!("{name}.crt"));
    let extfile = dir.join(format!("{name}.ext"));
    fs::write(&extfile, ext).map_err(|err| err.to_string())?;
    let ca = req("OO_A397_CA");
    let ca_key = PathBuf::from(&ca).parent().unwrap_or(Path::new("/secrets")).join("ca.key");
    let req_status = Command::new("openssl")
        .args(["req", "-newkey", "rsa:2048", "-nodes", "-keyout", key.to_str().ok_or("path")?, "-out", csr.to_str().ok_or("path")?, "-subj", "/CN=127.0.0.1"])
        .status()
        .map_err(|err| err.to_string())?;
    if !req_status.success() {
        return Err(format!("openssl req {name}"));
    }
    let sign = Command::new("openssl")
        .args(["x509", "-req", "-in", csr.to_str().ok_or("path")?, "-CA", &ca, "-CAkey", ca_key.to_str().ok_or("path")?, "-set_serial", serial, "-out", crt.to_str().ok_or("path")?, "-days", "7", "-extfile", extfile.to_str().ok_or("path")?])
        .status()
        .map_err(|err| err.to_string())?;
    if !sign.success() {
        return Err(format!("openssl x509 {name}"));
    }
    Ok(())
}

async fn wait_listen(path: &Path, secs: u64) -> Result<(), String> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        if let Ok(text) = fs::read_to_string(path)
            && let Some((_, port)) = text.trim().rsplit_once(':')
            && port.parse::<u16>().ok().is_some_and(|port| port != 0)
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err(format!("no listen {}", path.display()))
}

async fn run_piped(bin: &str, stdin_bytes: &[u8], stdout: &Path, stderr: &Path, secs: u64) -> Result<i32, String> {
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(File::create(stdout).map_err(|err| err.to_string())?)
        .stderr(File::create(stderr).map_err(|err| err.to_string())?)
        .spawn()
        .map_err(|err| err.to_string())?;
    child.stdin.take().ok_or("stdin")?.write_all(stdin_bytes).map_err(|err| err.to_string())?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
            return status.code().ok_or_else(|| "signal".to_owned());
        }
        if start.elapsed() > Duration::from_secs(secs) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("timeout".to_owned());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn other_sha(current: &str) -> String {
    let mut bytes = current.as_bytes().to_vec();
    if let Some(first) = bytes.first_mut() {
        *first = if *first == b'0' { b'1' } else { b'0' };
    }
    String::from_utf8(bytes).unwrap_or_else(|_| "11".repeat(32))
}

async fn revoke_wide(env: &Env, wide: &SessionRef, narrow_sha: &str, commit: &str) -> Result<(), String> {
    let before = audit_count(env, Some("wideowner"));
    let role = env.state.join("entry/packs/oo_r_wide/role.txt");
    let mut text = fs::read_to_string(&role).map_err(|err| err.to_string())?;
    text.push_str("revoked\n");
    fs::write(&role, text).map_err(|err| err.to_string())?;
    write_release(&env.state.join("release-state.json"), &[
        release_row("oo_r_wide", true, &other_sha(&wide.pack_sha256), commit),
        release_row("oo_r_narrow", true, narrow_sha, commit),
    ])?;
    let token = env.state.join(format!("entry/sessions/{}/gateway.token", wide.id));
    let origin = gateway_origin(&env.evidence.join("entry-serve.stdout"))?;
    let stdout = env.evidence.join("revocation.stdout");
    let stderr = env.evidence.join("revocation.stderr");
    let status = Command::new(req("OO_A397_OO_BIN"))
        .args(["query", "--json", "--auth", "external", "--token-file", token.to_str().unwrap(), "--gateway", &origin, "--transport", "local-development", "SELECT count(*) FROM shop.entry"])
        .stdout(File::create(&stdout).map_err(|err| err.to_string())?)
        .stderr(File::create(&stderr).map_err(|err| err.to_string())?)
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        return Err("revoked pack still queried".to_owned());
    }
    let after = audit_count(env, Some("wideowner"));
    if after != before {
        return Err(format!("revocation inserted query audit {before} -> {after}"));
    }
    let _ = fs::write(env.evidence.join("revocation.txt"), "old pack refused\n");
    Ok(())
}

async fn sdk_fixture(env: &Env, wide: &SessionRef) -> Result<(), String> {
    let before = audit_count(env, Some("wideowner"));
    let token = env.state.join(format!("entry/sessions/{}/gateway.token", wide.id));
    let origin = gateway_origin(&env.evidence.join("entry-serve.stdout"))?;
    let stdout = env.evidence.join("sdk-fixture.stdout");
    let stderr = env.evidence.join("sdk-fixture.stderr");
    let status = Command::new(req("OO_A397_OO_BIN"))
        .args(["query", "--json", "--auth", "external", "--token-file", token.to_str().unwrap(), "--gateway", &origin, "--transport", "local-development", "SELECT count(*) FROM shop.entry"])
        .stdout(File::create(&stdout).map_err(|err| err.to_string())?)
        .stderr(File::create(&stderr).map_err(|err| err.to_string())?)
        .status()
        .map_err(|err| err.to_string())?;
    if !status.success() {
        let err = fs::read_to_string(&stderr).unwrap_or_default();
        let out = fs::read_to_string(&stdout).unwrap_or_default();
        return Err(format!("sdk fixture {} {}", clip(&err), clip(&out)));
    }
    let text = fs::read_to_string(&stdout).map_err(|err| err.to_string())?;
    if !text.contains("90000") || !text.contains("query_id") {
        return Err("sdk fixture body".to_owned());
    }
    let after = audit_count(env, Some("wideowner"));
    if after != before + 1 {
        return Err(format!("sdk fixture audit {before} -> {after}"));
    }
    let _ = fs::write(env.evidence.join("sdk-fixture.txt"), "sdk_fixture_query\n");
    Ok(())
}

async fn model_turn(port: u16, session: &SessionRef, bearer: &str, question: &str, secs: u64) -> Result<Vec<Value>, String> {
    let (status, body) = call_bearer(port, "POST", &format!("/entry/v1/sessions/{}/turns", session.id), bearer, &json!({"text": question}).to_string()).await?;
    if status != 202 {
        return Err(format!("turn {status} {}", clip(&body_text(&body))));
    }
    let accepted: Value = serde_json::from_slice(&body).map_err(|err| err.to_string())?;
    if accepted["complete"] != false || !accepted["model_http_status"].is_null() {
        return Err("202 already claimed completion".to_owned());
    }
    if accepted["harness_pid"].as_u64().unwrap_or(0) == 0 {
        return Err("missing harness pid".to_owned());
    }
    let turn_id = accepted["turn_id"].as_str().filter(|value| !value.is_empty()).ok_or("turn id")?.to_owned();
    wait_events(port, &session.id, bearer, secs, &turn_id).await
}

async fn cancel_turn(port: u16, session: &SessionRef, bearer: &str) -> Result<(), String> {
    let (status, body) = call_bearer(port, "POST", &format!("/entry/v1/sessions/{}/turns", session.id), bearer, &json!({"text": QUESTION_COUNT}).to_string()).await?;
    if status != 202 {
        return Err(format!("cancel turn {status} {}", clip(&body_text(&body))));
    }
    let accepted: Value = serde_json::from_slice(&body).map_err(|err| err.to_string())?;
    let turn_id = accepted["turn_id"].as_str().filter(|value| !value.is_empty()).ok_or("turn id")?.to_owned();
    let (status, body) = call_bearer(port, "POST", &format!("/entry/v1/sessions/{}/cancel", session.id), bearer, "{}").await?;
    let text = body_text(&body);
    if status != 200 || !text.contains("\"turn_finished\":false") || !text.contains("cancelled") {
        return Err(format!("cancel {status} {}", clip(&text)));
    }
    let events = wait_events(port, &session.id, bearer, 60, &turn_id).await?;
    if events.iter().any(|event| event["kind"] == "turn_finished") || !events.iter().any(|event| event["kind"] == "incomplete") {
        return Err("cancel did not stay incomplete".to_owned());
    }
    Ok(())
}

async fn sigkill_turn(port: u16, session: &SessionRef, bearer: &str, env: &Env) -> Result<(), String> {
    let (status, body) = call_bearer(port, "POST", &format!("/entry/v1/sessions/{}/turns", session.id), bearer, &json!({"text": QUESTION_COUNT}).to_string()).await?;
    if status != 202 {
        return Err(format!("sigkill turn {status} {}", clip(&body_text(&body))));
    }
    let accepted: Value = serde_json::from_slice(&body).map_err(|err| err.to_string())?;
    let turn_id = accepted["turn_id"].as_str().filter(|value| !value.is_empty()).ok_or("turn id")?.to_owned();
    let pid = accepted["harness_pid"].as_u64().ok_or("pid")?;
    let kill = Command::new("/bin/kill").args(["-KILL", &pid.to_string()]).status().map_err(|err| err.to_string())?;
    if !kill.success() {
        return Err("sigkill failed".to_owned());
    }
    let events = wait_events(port, &session.id, bearer, 60, &turn_id).await?;
    publish_events(env, "sigkill-events.json", &events);
    if events.iter().any(|event| event["kind"] == "turn_finished") || !events.iter().any(|event| event["kind"] == "incomplete") {
        return Err("sigkill still looked complete".to_owned());
    }
    Ok(())
}

async fn wait_events(port: u16, id: &str, bearer: &str, secs: u64, turn_id: &str) -> Result<Vec<Value>, String> {
    let start = Instant::now();
    let mut mine = Vec::new();
    while start.elapsed() < Duration::from_secs(secs) {
        let (status, body) = call_bearer(port, "GET", &format!("/entry/v1/sessions/{id}/events?after=0"), bearer, "").await?;
        if status != 200 {
            return Err(format!("events {status}"));
        }
        let value: Value = serde_json::from_slice(&body).map_err(|err| err.to_string())?;
        let last = value["events"].as_array().cloned().unwrap_or_default();
        mine = last.into_iter().filter(|event| event.get("turn_id").and_then(Value::as_str) == Some(turn_id)).collect();
        if mine.iter().any(|event| matches!(event["kind"].as_str(), Some("turn_finished" | "incomplete" | "clarification"))) {
            return Ok(mine);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Ok(mine)
}

fn assert_gold_row(env: &Env, query_id: &str, pack: &str) -> Result<(), String> {
    let text = fs::read_to_string(env.state.join("entry/query-receipts.jsonl")).map_err(|err| err.to_string())?;
    let mut matched = None;
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else { continue };
        if value.get("query_id").and_then(Value::as_str) == Some(query_id) {
            matched = Some(value);
            break;
        }
    }
    let row = matched.ok_or("cited query is not in the audit")?;
    let sql = row.get("sql").and_then(Value::as_str).unwrap_or("");
    let result = row.get("result_text").and_then(Value::as_str).unwrap_or("");
    let stored = row.get("pack_sha256").and_then(Value::as_str).unwrap_or("");
    let hash = row.get("original_sha256").and_then(Value::as_str).unwrap_or("");
    let audit = row.get("audit_file").and_then(Value::as_str).unwrap_or("");
    let lower = sql.to_ascii_lowercase();
    let audit_text = fs::read_to_string(audit).unwrap_or_default();
    let generation = row.get("import_generation").and_then(Value::as_str).unwrap_or("");
    if result != "90000" || stored != pack || !lower.contains("count") || !lower.contains("shop.entry") || lower.contains("phone") || lower.contains("national_id") || hash.len() != 64 || !audit_text.contains(hash) || row.get("mp01") != Some(&Value::Bool(false)) || row.get("import_source").and_then(Value::as_str) != Some("private_fixture") || generation.len() != 64 || row.get("snapshot_id").and_then(Value::as_str) != Some("") || !row.get("import_generation_number").is_some_and(Value::is_null) {
        return Err("audit row is not the entry count for the bound pack".to_owned());
    }
    let _ = fs::write(env.evidence.join("count-query-id.txt"), format!("{query_id}\n{stored}\n{hash}\n"));
    Ok(())
}

fn audit_count(env: &Env, principal: Option<&str>) -> u64 {
    let Ok(text) = fs::read_to_string(env.state.join("entry/query-receipts.jsonl")) else {
        return 0;
    };
    let mut count = 0_u64;
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if let Some(name) = principal {
            let Ok(value) = serde_json::from_str::<Value>(line) else { continue };
            if value.get("principal").and_then(Value::as_str) != Some(name) {
                continue;
            }
        }
        count += 1;
    }
    count
}

async fn pg(env: &Env, user: &str, password: &str) -> Result<tokio_postgres::Client, String> {
    let _ = env;
    let ca = fs::read(req("OO_A397_CA")).map_err(|err| err.to_string())?;
    let certificate = CertificateDer::from_pem_slice(&ca).map_err(|_| "ca")?;
    let mut roots = RootCertStore::empty();
    roots.add(certificate).map_err(|_| "ca")?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let config = ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|_| "tls")?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let tls = MakeRustlsConnect::new(config);
    let mut pg = tokio_postgres::Config::new();
    pg.user(user);
    pg.password(password);
    pg.host(req("OO_A397_PG_HOST"));
    pg.port(req("OO_A397_PG_PORT").parse().map_err(|_| "pg port")?);
    pg.dbname("oo_a397");
    pg.ssl_mode(SslMode::Require);
    let (client, connection) = pg.connect(tls).await.map_err(|err| format!("pg {err}"))?;
    tokio::spawn(async move { let _ = connection.await; });
    Ok(client)
}

async fn login(ca: &[u8], identity_port: u16, subject: &str, bootstrap: Option<&str>) -> Result<(String, String), String> {
    let mut path = "/control/v1/auth/login?return_to=/app".to_owned();
    if let Some(token) = bootstrap {
        path.push_str("&bootstrap_token=");
        path.push_str(&url_encode(token));
    }
    let (status, headers, _) = https(identity_port, ca, "GET", &path, "").await?;
    if status != 302 {
        return Err(format!("login {subject} {status}"));
    }
    let location = header(&headers, "location").ok_or("login location")?.to_owned();
    let login_cookie = headers.iter().filter(|(name, _)| name == "set-cookie").find_map(|(_, value)| cookie_pair(value, "__Host-oo-login")).ok_or("login cookie")?;
    let (host, port, next) = split_url(&location)?;
    if host != "127.0.0.1" {
        return Err("idp host".to_owned());
    }
    let (status, headers, _) = https(port, ca, "GET", &next, "").await?;
    if status != 302 {
        return Err(format!("authorize {status}"));
    }
    let callback = header(&headers, "location").ok_or("callback")?.to_owned();
    let (_, callback_port, callback_path) = split_url(&callback)?;
    let cookie_header = format!("cookie: __Host-oo-login={login_cookie}\r\n");
    let (status, headers, _) = https_raw(callback_port, ca, "GET", &callback_path, &cookie_header).await?;
    if status != 302 {
        return Err(format!("callback {status}"));
    }
    let session = headers.iter().filter(|(name, _)| name == "set-cookie").find_map(|(_, value)| cookie_pair(value, "__Host-oo-session")).ok_or("session cookie")?;
    let (status, _, body) = https_raw(identity_port, ca, "GET", "/control/v1/identity/current", &format!("cookie: __Host-oo-session={session}\r\n")).await?;
    if status != 200 {
        return Err(format!("current {subject} {status}"));
    }
    let value: Value = serde_json::from_slice(&body).map_err(|err| err.to_string())?;
    if value["subject"] != subject || value["data_grants"] != "none" || value["authority"] != "management_session" || value["install_state"] != "initialized" {
        return Err(format!("current body {subject}"));
    }
    let csrf = value["csrf_token"].as_str().unwrap_or("").to_owned();
    if csrf.len() < 20 {
        return Err("csrf".to_owned());
    }
    Ok((session, csrf))
}

async fn https(port: u16, ca: &[u8], method: &str, path: &str, extra: &str) -> Result<ParsedHttp, String> {
    https_raw(port, ca, method, path, extra).await
}

async fn https_raw(port: u16, ca: &[u8], method: &str, path: &str, extra: &str) -> Result<ParsedHttp, String> {
    let request = format!("{method} {path} HTTP/1.1\r\nhost: 127.0.0.1:{port}\r\nconnection: close\r\n{extra}\r\n");
    let bytes = https_exchange("127.0.0.1", port, "127.0.0.1", ca, request.as_bytes()).await.map_err(|err| err.to_string())?;
    let (status, headers, body) = status_line(&bytes).ok_or("https parse")?;
    Ok((status, headers, body))
}

async fn call_bearer(port: u16, method: &str, path: &str, bearer: &str, body: &str) -> Result<(u16, Vec<u8>), String> {
    let (status, _, body) = exchange(port, method, path, bearer, body).await?;
    Ok((status, body))
}

async fn exchange(port: u16, method: &str, path: &str, bearer: &str, body: &str) -> Result<(u16, Vec<(String, String)>, Vec<u8>), String> {
    let mut extra = String::new();
    if !bearer.is_empty() {
        extra.push_str("authorization: Bearer ");
        extra.push_str(bearer);
        extra.push_str("\r\n");
    }
    let request = format!("{method} {path} HTTP/1.1\r\nhost: 127.0.0.1\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n{extra}\r\n{body}", body.len());
    let mut sock = TcpStream::connect(("127.0.0.1", port)).await.map_err(|err| err.to_string())?;
    sock.write_all(request.as_bytes()).await.map_err(|err| err.to_string())?;
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(180), async {
        let mut chunk = [0_u8; 8192];
        loop {
            let n = sock.read(&mut chunk).await.map_err(|err| err.to_string())?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.len() > 2_000_000 {
                break;
            }
        }
        Ok::<(), String>(())
    }).await.map_err(|_| "http deadline")??;
    let (status, headers, body) = status_line(&buf).ok_or("http parse")?;
    Ok((status, headers, body))
}

fn identity_config(dir: &Path, origin: &str, port: u16, issuer: &str, redirect: &str, commit: &str, env: &Env) -> Value {
    let _ = fs::write(env.state.join("identity/origin.txt"), format!("{origin}\n"));
    json!({
        "https_origin": origin,
        "listen_host": "127.0.0.1",
        "listen_port": port,
        "tls_cert_file": req("OO_A397_TLS_CERT"),
        "tls_key_file": req("OO_A397_TLS_KEY"),
        "issuer": issuer,
        "audience": "oo-a397-audience",
        "client_id": "oo-a397-client",
        "client_secret_file": dir.join("client.secret"),
        "redirect_uris": [redirect],
        "config_commit": commit,
        "revision": 1,
        "idp_ca_file": req("OO_A397_CA"),
        "pg": {
            "host": req("OO_A397_PG_HOST"),
            "port": req("OO_A397_PG_PORT").parse::<u16>().unwrap_or(0),
            "database": "oo_a397",
            "user": "oo_identity_admin",
            "password_file": dir.join("pg.pass"),
            "ca_file": req("OO_A397_CA")
        },
        "bindings": [
            {"subject": "subadmin", "project_id": "shop", "role": "admin"},
            {"subject": "subnarrow", "project_id": "shop", "role": "auditor"},
            {"subject": "subwide", "project_id": "shop", "role": "business_owner"}
        ],
        "bootstrap": {"subject": "subadmin", "project_id": "shop", "role": "admin"},
        "idle_seconds": 3600,
        "absolute_seconds": 86400,
        "oauth_ttl_seconds": 300,
        "bootstrap_ttl_seconds": 600,
        "clock_skew_seconds": 60
    })
}

fn config_commit(issuer: &str, redirect: &str) -> String {
    let value = json!({
        "audience": "oo-a397-audience",
        "bindings": [
            {"project_id": "shop", "role": "admin", "subject": "subadmin"},
            {"project_id": "shop", "role": "auditor", "subject": "subnarrow"},
            {"project_id": "shop", "role": "business_owner", "subject": "subwide"}
        ],
        "bootstrap": {"customer_owner": false, "fixture_class": "explicit_test_fixture", "project_id": "shop", "role": "admin", "subject": "subadmin"},
        "client_id": "oo-a397-client",
        "issuer": issuer,
        "protocol": "oo.control-identity/config-commit/v1",
        "redirect_uris": [redirect],
        "revision": 1
    });
    agent_entry::hex_encode(&agent_entry::sha256(value.to_string().as_bytes()))
}

fn session_config(dir: &Path, env: &Env, admin_pass: &str) -> Result<Value, String> {
    let ca = fs::read_to_string(req("OO_A397_CA")).map_err(|err| err.to_string())?;
    Ok(json!({
        "contract": "session-control-api-v1",
        "profile": "localprocess",
        "bind": "127.0.0.1:0",
        "owner_uuid": "eef831aa-dba4-4af2-a78b-541bc9e0a387",
        "state_dir": dir.join("run"),
        "bindings_path": dir.join("bindings.json"),
        "audience_url": "https://oo-a397.example/v1",
        "pg": {
            "host": req("OO_A397_PG_HOST"),
            "port": req("OO_A397_PG_PORT").parse::<u16>().map_err(|_| "pg port")?,
            "database": "oo_a397",
            "ca_pem": ca,
            "api_admin_password": read_secret(&env.pass("api-admin")),
            "api_runtime_password": read_secret(&env.pass("api-runtime")),
            "session_admin_password": admin_pass,
            "session_runtime_password": read_secret(&env.pass("runtime")),
            "cancel_password": read_secret(&env.pass("cancel")),
            "query_password": read_secret(&env.pass("query")),
            "token_admin_password": read_secret(&env.pass("token-admin")),
            "token_runtime_password": read_secret(&env.pass("token-runtime"))
        }
    }))
}

fn binding(principal: &str) -> Value {
    json!({
        "principal_id": principal,
        "project_id": "shop",
        "role": "analyst",
        "claims": [["dept", "shop"]],
        "policy_version": "pol-1",
        "knowledge_version": "know-1",
        "config_version": "cfg-1",
        "capabilities": ["session.use", "session.terminate"]
    })
}

fn gateway_origin(log: &Path) -> Result<String, String> {
    let text = fs::read_to_string(log).map_err(|err| err.to_string())?;
    for line in text.lines() {
        if let Ok(value) = serde_json::from_str::<Value>(line)
            && let Some(port) = value.get("gateway_port").and_then(Value::as_u64)
        {
            return Ok(format!("http://127.0.0.1:{port}"));
        }
    }
    Err("gateway port".to_owned())
}

async fn reserve_port() -> Result<u16, String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|err| err.to_string())?;
    let port = listener.local_addr().map_err(|err| err.to_string())?.port();
    drop(listener);
    Ok(port)
}

async fn wait_port(path: &Path, secs: u64) -> Result<u16, String> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        if let Ok(text) = fs::read_to_string(path)
            && let Ok(port) = text.trim().parse::<u16>()
            && port != 0
        {
            return Ok(port);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err(format!("no port {}", path.display()))
}

async fn wait_contains(path: &Path, needle: &str, secs: u64) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        if fs::read_to_string(path).ok().is_some_and(|text| text.contains(needle)) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

fn run_to_exit(bin: &str, args: &[&str], stderr: &Path) -> Result<i32, String> {
    let status = Command::new(bin).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(File::create(stderr).map_err(|err| err.to_string())?).status().map_err(|err| err.to_string())?;
    status.code().ok_or_else(|| "signal".to_owned())
}

fn run_stdin(bin: &str, command: &str, config: &Path, stdout: &Path, stderr: &Path) -> Result<i32, String> {
    let status = Command::new(bin)
        .arg(command)
        .stdin(File::open(config).map_err(|err| err.to_string())?)
        .stdout(File::create(stdout).map_err(|err| err.to_string())?)
        .stderr(File::create(stderr).map_err(|err| err.to_string())?)
        .status()
        .map_err(|err| err.to_string())?;
    status.code().ok_or_else(|| "signal".to_owned())
}

fn spawn_stdin(bin: &str, command: &str, config: &Path, stdout: &Path, stderr: &Path) -> Result<Proc, String> {
    let child = Command::new(bin)
        .arg(command)
        .stdin(File::open(config).map_err(|err| err.to_string())?)
        .stdout(File::create(stdout).map_err(|err| err.to_string())?)
        .stderr(File::create(stderr).map_err(|err| err.to_string())?)
        .spawn()
        .map_err(|err| err.to_string())?;
    Ok(Proc(Some(child)))
}

fn spawn_no_stdin(bin: &str, args: &[&str], stdout: &Path, stderr: &Path) -> Result<Proc, String> {
    let child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(File::create(stdout).map_err(|err| err.to_string())?)
        .stderr(File::create(stderr).map_err(|err| err.to_string())?)
        .spawn()
        .map_err(|err| err.to_string())?;
    Ok(Proc(Some(child)))
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    write_0600(path, &value.to_string())
}

fn write_0600(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let _ = fs::remove_file(path);
    let mut file = OpenOptions::new().create_new(true).write(true).mode(0o600).open(path).map_err(|err| err.to_string())?;
    file.write_all(text.as_bytes()).map_err(|err| err.to_string())?;
    Ok(())
}

fn read_secret(path: &Path) -> String {
    let mut text = fs::read_to_string(path).unwrap_or_else(|_| panic!("secret file {}", path.display()));
    if text.ends_with('\n') {
        text.pop();
    }
    text
}

fn file_sha(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    Ok(agent_entry::hex_encode(&agent_entry::sha256(&bytes)))
}

fn secret_list(env: &Env) -> Vec<String> {
    ["session-admin", "gateway", "identity", "api-admin", "api-runtime", "runtime", "cancel", "query", "token-admin", "token-runtime", "fixture-admin"]
        .into_iter()
        .map(|name| read_secret(&env.pass(name)))
        .filter(|secret| secret.len() >= 8)
        .collect()
}

fn publish_events(env: &Env, name: &str, events: &[Value]) {
    let _ = fs::write(env.evidence.join(name), serde_json::to_string_pretty(events).unwrap_or_default());
}

fn publish_safe(env: &Env, name: &str, text: &str, secrets: &[String]) {
    if secrets.iter().any(|secret| text.contains(secret)) {
        let _ = fs::write(env.evidence.join(name), "withheld\n");
    } else {
        let _ = fs::write(env.evidence.join(name), text);
    }
}

fn body_text(body: &[u8]) -> String {
    String::from_utf8_lossy(body).into_owned()
}

fn clip(text: &str) -> String {
    text.chars().take(300).collect()
}

fn split_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url.strip_prefix("https://").ok_or("url")?;
    let (hostport, path) = rest.split_once('/').ok_or("url")?;
    let (host, port) = hostport.rsplit_once(':').ok_or("port")?;
    Ok((host.to_owned(), port.parse().map_err(|_| "port".to_owned())?, format!("/{path}")))
}

fn url_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(char::from(b"0123456789ABCDEF"[(byte >> 4) as usize]));
            out.push(char::from(b"0123456789ABCDEF"[(byte & 0x0f) as usize]));
        }
    }
    out
}

const SHOP_SQL: &str = "\
DROP SCHEMA IF EXISTS shop CASCADE;
DROP SCHEMA IF EXISTS oo_agent_audit CASCADE;
CREATE SCHEMA IF NOT EXISTS shop;
CREATE TABLE IF NOT EXISTS shop.party (party_key text PRIMARY KEY, display_name text, active boolean NOT NULL, phone text NOT NULL, national_id text NOT NULL);
CREATE TABLE IF NOT EXISTS shop.account (account_id bigint PRIMARY KEY, party_key text NOT NULL, opened boolean NOT NULL, label text);
CREATE TABLE IF NOT EXISTS shop.entry (entry_id bigint PRIMARY KEY, account_id bigint NOT NULL, qty bigint NOT NULL, amount numeric(18,4), memo text);
INSERT INTO shop.party SELECT 'p' || lpad(i::text, 5, '0'), CASE WHEN i % 17 = 0 THEN NULL ELSE 'n' || lpad(i::text, 5, '0') END, i % 2 = 0, 'tel-' || lpad(i::text, 5, '0'), 'nid-' || lpad(i::text, 5, '0') FROM generate_series(0, 1999) AS i;
INSERT INTO shop.account SELECT j, 'p' || lpad((j % 2000)::text, 5, '0'), j % 3 <> 0, CASE WHEN j % 19 = 0 THEN NULL ELSE 'a' || j::text END FROM generate_series(0, 7999) AS j;
INSERT INTO shop.entry SELECT k, k % 8000, CASE k WHEN 0 THEN '-9223372036854775808'::bigint WHEN 1 THEN 9223372036854775807::bigint WHEN 2 THEN 0::bigint ELSE (k - 45000)::bigint END, CASE WHEN k = 3 THEN 1234567890.1234 WHEN k = 4 THEN -1234567890.1234 WHEN k = 5 THEN 0.0001 WHEN k = 6 THEN -0.0001 WHEN k % 29 = 0 THEN NULL ELSE ((k % 10000) - 5000)::numeric / 10000 END, CASE WHEN k % 13 = 0 THEN NULL ELSE 'm' || k::text END FROM generate_series(0, 89999) AS k;
GRANT USAGE ON SCHEMA shop TO oo_r_wide, oo_r_narrow;
GRANT SELECT (party_key, display_name, active, phone) ON shop.party TO oo_r_wide;
GRANT SELECT (account_id, party_key, opened, label) ON shop.account TO oo_r_wide;
GRANT SELECT (entry_id, account_id, qty, amount, memo) ON shop.entry TO oo_r_wide;
GRANT SELECT (party_key, display_name) ON shop.party TO oo_r_narrow;
GRANT SELECT (account_id, party_key, opened) ON shop.account TO oo_r_narrow;
GRANT SELECT (entry_id, account_id) ON shop.entry TO oo_r_narrow;
CREATE SCHEMA IF NOT EXISTS oo_agent_audit;
CREATE TABLE IF NOT EXISTS oo_agent_audit.query (query_id text PRIMARY KEY, data_role text NOT NULL, principal_id text NOT NULL, sql_text text NOT NULL, result_text text NOT NULL, pack_sha256 text NOT NULL, created_at timestamptz NOT NULL DEFAULT now());
REVOKE ALL ON SCHEMA oo_agent_audit FROM PUBLIC;
REVOKE ALL ON oo_agent_audit.query FROM PUBLIC;
GRANT USAGE ON SCHEMA oo_agent_audit TO oo_gateway_login;
GRANT SELECT, INSERT ON oo_agent_audit.query TO oo_gateway_login;
";
