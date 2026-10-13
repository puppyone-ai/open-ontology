//! Private PostgreSQL and HTTPS proof. A missing phase returns without a database.

#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use artifact_control_api::{
    check_dir, load_config, produce, read_scope_files, remove_staging, sha256_hex, token_ok, write_scope_files, Checked, Outcome, Produced, Scope,
    Store, CONTRACT,
};
use serde_json::{json, Value};

const FIXTURE_SHA: &str = "54cabda641bf1a2128e0246a78eb1a761895a0aa5a4226d136ad8aec399ad6eb";
const FIXTURE_NAME: &str = "query-clerk-order_doc-star.stdout";
const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const DIGEST_B: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
const ORIGIN: &str = "https://127.0.0.1:8443";

#[tokio::test]
async fn live_phase() {
    match std::env::var("OO_A398_PHASE").ok().as_deref() {
        None => {}
        Some("write") => write_phase().await,
        Some("restart") => restart_phase().await,
        Some(other) => panic!("phase {other}"),
    }
}

async fn write_phase() {
    let env = Env::load();
    let dirs = prepare_dirs(&env);
    let end = unix_now() + 7200;
    let original = scope_of("clerk", "session_a398", "clerk", "policy_v1", "pack_v1", DIGEST, end);
    write_scope_files(&dirs.scope, &dirs.mac, &original, &fs::read(&env.aead_key).expect("key")).expect("scope");
    fs::copy(env.fixture_dir.join(FIXTURE_NAME), dirs.intake.join(FIXTURE_NAME)).expect("fixture");
    let source_sha = sha256_hex(&fs::read(dirs.intake.join(FIXTURE_NAME)).expect("fixture bytes"));
    assert_eq!(source_sha, FIXTURE_SHA);
    write_config(&env, &dirs);
    let config = load_config(&dirs.config).unwrap_or_else(|fault| panic!("config {}", fault.code));
    install_cli(&env.bin, &dirs.config);
    let store = Store::connect(&config).await.unwrap_or_else(|fault| panic!("connect {}", fault.code));
    let independent = producer_sha(&config);
    let (mut server, line) = spawn_server(&env.bin, &dirs.config, &env.state.join("server.stderr"));
    assert!(line.contains("\"outcome\":\"listening\""), "listen {line}");
    let mut http = Http::open(&env.https_ca);
    http.consume().await;

    let workspace = http.post("/control/v1/artifacts", &accept_body(FIXTURE_NAME, &columns(), "accept-workspace-0001", Some(true))).await;
    assert_eq!(workspace.status, 403);
    assert_eq!(workspace.code(), "workspace.disabled");
    let caller = http.post("/control/v1/artifacts", &format!("{{\"version\":\"{CONTRACT}\",\"principal_id\":\"clerk\",\"source_file\":\"{FIXTURE_NAME}\",\"columns\":[\"order_id\"],\"idempotency_key\":\"accept-caller-json01\"}}")).await;
    assert_eq!(caller.status, 400);
    assert_eq!(caller.code(), "self_asserted");
    assert_eq!(store.active_count("session_a398").await.unwrap(), 0);

    let body_a = accept_body(FIXTURE_NAME, &columns(), "accept-object-a-0001", Some(false));
    let created = http.post("/control/v1/artifacts", &body_a).await;
    assert_eq!(created.status, 201, "{}", created.code());
    let object_a = created.body["object_id"].as_str().expect("object").to_owned();
    let expires = created.body["expires_unix_ms"].as_i64().expect("expires");
    assert_eq!(created.body["source_sha256"], FIXTURE_SHA);
    assert!(created.body["signed_url"].is_null());
    assert_eq!(created.body["session_extended"], false);
    let download = http.get(&format!("/control/v1/artifacts/{object_a}/content")).await;
    assert_eq!(download.status, 200);
    assert!(download.header("location").is_empty());
    assert!(download.header("cache-control").contains("no-store"));
    assert!(download.header("content-type").starts_with("text/csv"));
    assert_eq!(download.header("content-length"), download.bytes.len().to_string());
    let download_sha = sha256_hex(&download.bytes);
    assert_eq!(download_sha, independent);
    assert_eq!(download_sha, created.body["plaintext_sha256"].as_str().unwrap());

    let replay = http.post("/control/v1/artifacts", &body_a).await;
    assert_eq!(replay.status, 200);
    assert_eq!(replay.body["replayed"], true);
    assert_eq!(replay.body["object_id"], object_a);
    let conflict = http.post("/control/v1/artifacts", &accept_body(FIXTURE_NAME, &columns(), "accept-object-a-0001", None)).await;
    assert_eq!(conflict.status, 409);
    assert_eq!(conflict.code(), "idempotency.conflict");

    let list = http.get("/control/v1/artifacts").await;
    assert_eq!(list.status, 200);
    assert!(list.body["signed_url"].is_null());
    assert_eq!(list.body["artifacts"][0]["object_id"], object_a);
    let detail = http.get(&format!("/control/v1/artifacts/{object_a}")).await;
    assert_eq!(detail.status, 200);
    assert_eq!(detail.body["state"], "active");

    let created_b = http.post("/control/v1/artifacts", &accept_body(FIXTURE_NAME, &columns(), "accept-object-b-0001", None)).await;
    assert_eq!(created_b.status, 201, "{}", created_b.code());
    let object_b = created_b.body["object_id"].as_str().expect("object b").to_owned();

    let mut ctx = Ctx { http: &mut http, dirs: &dirs, config: &config, store: &store, original: &original, end, object_a, object_b, download_sha };
    let proved = ctx.prove().await;
    let object_again = ctx.object_a.clone();
    let sha_again = ctx.download_sha.clone();
    let held = held_value(&ctx, &proved, &independent, &source_sha, expires, workspace.status);
    drop(ctx);

    drop(server);
    let (server_again, again_line) = spawn_server(&env.bin, &dirs.config, &env.state.join("server.stderr"));
    server = server_again;
    assert!(again_line.contains("\"outcome\":\"listening\""), "restart {again_line}");
    let restarted = http.get(&format!("/control/v1/artifacts/{object_again}/content")).await;
    assert_eq!(sha256_hex(&restarted.bytes), sha_again);
    drop(server);
    fs::write(env.state.join("held.json"), serde_json::to_vec(&held).unwrap()).unwrap();
    fs::write(env.state.join("phase.txt"), "live_artifact=write_done\n").unwrap();
}

async fn restart_phase() {
    let env = Env::load();
    let held: Value = serde_json::from_slice(&fs::read(env.state.join("held.json")).expect("held")).expect("held json");
    let dirs = Dirs::from_state(&env.state);
    let config = load_config(&dirs.config).unwrap_or_else(|fault| panic!("config {}", fault.code));
    let object_a = held["object_a"].as_str().expect("object").to_owned();
    let expected = held["download_sha256"].as_str().expect("sha").to_owned();
    let expires = held["expires_unix_ms"].as_i64().expect("expires");
    let (server, line) = spawn_server(&env.bin, &dirs.config, &env.state.join("server.stderr"));
    assert!(line.contains("\"outcome\":\"listening\""), "listen {line}");
    let mut http = Http::open(&env.https_ca);
    http.consume().await;
    let download = http.get(&format!("/control/v1/artifacts/{object_a}/content")).await;
    assert_eq!(download.status, 200);
    let download_sha = sha256_hex(&download.bytes);
    assert_eq!(download_sha, expected);
    let store = Store::connect(&config).await.unwrap_or_else(|fault| panic!("connect {}", fault.code));
    let first = store.cleanup_before(&config.object_dir, expires + 1).await.unwrap_or_else(|fault| panic!("cleanup {}", fault.code));
    assert!(first.expired >= 1);
    let second = store.cleanup_before(&config.object_dir, expires + 1).await.unwrap();
    assert_eq!(second.expired, 0);
    assert!(second.idle);
    assert!(!config.object_dir.join(format!("{object_a}.bin")).exists());
    let denied = http.get(&format!("/control/v1/artifacts/{object_a}/content")).await;
    assert_eq!(denied.status, 403);
    assert_eq!(denied.code(), "expired");
    drop(server);

    let mut proof = held;
    proof["restart_match"] = json!(true);
    proof["cleanup_expired"] = json!(first.expired);
    proof["cleanup_idempotent"] = json!(true);
    proof["contract"] = json!(CONTRACT);
    proof["signed_url"] = Value::Null;
    proof["workspace"] = json!("disabled");
    proof["production_signed"] = json!(false);
    proof["runsc"] = json!("not_claimed");
    proof["customer_integration"] = json!("not_claimed");
    proof["parent_complete"] = json!(false);
    let bytes = serde_json::to_vec(&proof).unwrap();
    fs::write(&env.proof, &bytes).unwrap();
    fs::write(env.state.join("proof.json"), &bytes).unwrap();
    fs::write(env.state.join("phase.txt"), "live_artifact=done\n").unwrap();
}

struct Ctx<'a> {
    http: &'a mut Http,
    dirs: &'a Dirs,
    config: &'a artifact_control_api::Config,
    store: &'a Store,
    original: &'a Scope,
    end: i64,
    object_a: String,
    object_b: String,
    download_sha: String,
}

struct Proved {
    role: (u16, u64),
    policy: (u16, u64),
    knowledge: (u16, u64),
    cross: (u16, u64),
    revoke_status: u16,
    revoke_ms: u64,
    anchor: i64,
    signed_status: u16,
}

impl Ctx<'_> {
    async fn prove(&mut self) -> Proved {
        let object_a = self.object_a.clone();
        let object_b = self.object_b.clone();
        let download_sha = self.download_sha.clone();
        let role = deny_then_restore(&mut *self.http, self.dirs, self.config, self.original, &scope_of("clerk", "session_a398", "auditor", "policy_v1", "pack_v1", DIGEST, self.end), &object_a, "forbidden").await;
        let policy = deny_then_restore(&mut *self.http, self.dirs, self.config, self.original, &scope_of("clerk", "session_a398", "clerk", "policy_v2", "pack_v1", DIGEST, self.end), &object_a, "forbidden").await;
        let knowledge = deny_then_restore(&mut *self.http, self.dirs, self.config, self.original, &scope_of("clerk", "session_a398", "clerk", "policy_v1", "pack_v2", DIGEST_B, self.end), &object_a, "forbidden").await;
        let cross = deny_then_restore(&mut *self.http, self.dirs, self.config, self.original, &scope_of("other", "session_a398", "clerk", "policy_v1", "pack_v1", DIGEST, self.end), &object_a, "forbidden").await;
        let revoke_body = format!("{{\"version\":\"{CONTRACT}\",\"idempotency_key\":\"revoke-object-b-0001\"}}");
        let revoked = self.http.post(&format!("/control/v1/artifacts/{object_b}/revoke"), &revoke_body).await;
        assert_eq!(revoked.status, 200);
        assert_eq!(revoked.body["disposition"], "revoked");
        let (revoke_status, revoke_code, revoke_ms) = timed_get(self.http, &format!("/control/v1/artifacts/{object_b}/content")).await;
        assert_eq!(revoke_status, 403);
        assert_eq!(revoke_code, "revoked");
        assert!(revoke_ms < 5000);
        let still = self.http.get(&format!("/control/v1/artifacts/{object_a}/content")).await;
        assert_eq!(sha256_hex(&still.bytes), download_sha);
        let signed = self.http.get(&format!("/control/v1/artifacts/{object_a}/content?sig=old-url")).await;
        assert_eq!(signed.status, 400);
        assert_eq!(signed.code(), "malformed");
        let anchor = self.store.anchor_end("session_a398").await.unwrap();
        let secret = self.http.post("/control/v1/artifacts", &accept_body(FIXTURE_NAME, &poison_columns(), "accept-secret-scan01", None)).await;
        assert_eq!(secret.status, 409, "{}", secret.code());
        assert_eq!(secret.body["code"], "artifact_loss");
        assert_eq!(secret.body["reason"], "secret_scan");
        assert_eq!(secret.body["session_extended"], false);
        checksum_loss(self.config, self.store, &anchor).await;
        assert_eq!(self.store.anchor_end("session_a398").await.unwrap(), anchor);
        assert!(self.store.loss_count("session_a398").await.unwrap() >= 2);
        self.store.extension_is_rejected("session_a398").await.unwrap_or_else(|fault| panic!("extend {}", fault.code));
        self.store.loss_delete_is_rejected().await.unwrap_or_else(|fault| panic!("loss delete {}", fault.code));
        write_scope_files(&self.dirs.scope, &self.dirs.mac, &scope_of("clerk", "session_a398", "clerk", "policy_v1", "pack_v1", DIGEST, self.end + 1000), &self.config.aead_key).unwrap();
        let extended = self.http.post("/control/v1/artifacts", &accept_body(FIXTURE_NAME, &columns(), "accept-extend-end001", None)).await;
        assert_eq!(extended.status, 409);
        assert_eq!(extended.code(), "session.extended");
        write_scope_files(&self.dirs.scope, &self.dirs.mac, self.original, &self.config.aead_key).unwrap();
        assert_eq!(self.store.anchor_end("session_a398").await.unwrap(), anchor);
        assert_eq!(read_scope_files(&self.dirs.scope, &self.dirs.mac, &self.config.aead_key).unwrap().absolute_end_unix, self.end);
        let cleanup = self.http.post("/control/v1/cleanup", &format!("{{\"version\":\"{CONTRACT}\"}}")).await;
        assert_eq!(cleanup.status, 200);
        assert_eq!(cleanup.body["idle"], true);
        assert_eq!(cleanup.body["expired"], 0);
        assert_eq!(self.store.active_count("session_a398").await.unwrap(), 1);
        Proved { role, policy, knowledge, cross, revoke_status, revoke_ms, anchor, signed_status: signed.status }
    }
}

fn held_value(ctx: &Ctx<'_>, proved: &Proved, independent: &str, source_sha: &str, expires: i64, workspace_status: u16) -> Value {
    json!({
        "object_a": ctx.object_a,
        "download_sha256": ctx.download_sha,
        "producer_csv_sha256": independent,
        "source_sha256": source_sha,
        "expires_unix_ms": expires,
        "anchor_end": proved.anchor,
        "match": true,
        "process_restart_match": true,
        "revoke_status": proved.revoke_status,
        "revoke_ms": proved.revoke_ms,
        "role_status": proved.role.0,
        "role_ms": proved.role.1,
        "policy_status": proved.policy.0,
        "policy_ms": proved.policy.1,
        "knowledge_status": proved.knowledge.0,
        "knowledge_ms": proved.knowledge.1,
        "cross_person_status": proved.cross.0,
        "cross_person_ms": proved.cross.1,
        "secret_scan": "secret_scan",
        "checksum": "checksum",
        "session_extended": false,
        "workspace_status": workspace_status,
        "caller_json": "self_asserted",
        "signed_query_status": proved.signed_status,
        "cleanup_idle": true
    })
}

async fn deny_then_restore(http: &mut Http, dirs: &Dirs, config: &artifact_control_api::Config, original: &Scope, changed: &Scope, object_id: &str, code: &str) -> (u16, u64) {
    write_scope_files(&dirs.scope, &dirs.mac, changed, &config.aead_key).unwrap();
    let (status, got, ms) = timed_get(http, &format!("/control/v1/artifacts/{object_id}/content")).await;
    assert_eq!(status, 403);
    assert_eq!(got, code);
    assert!(ms < 5000, "{code} {ms}");
    write_scope_files(&dirs.scope, &dirs.mac, original, &config.aead_key).unwrap();
    let restored = http.get(&format!("/control/v1/artifacts/{object_id}/content")).await;
    assert_eq!(restored.status, 200);
    (status, ms)
}

async fn checksum_loss(config: &artifact_control_api::Config, store: &Store, anchor: &i64) {
    let produced = produce(config, FIXTURE_NAME, &columns()).unwrap_or_else(|fault| panic!("produce {}", fault.code));
    let Produced::Files { dir, source_sha256 } = produced else {
        panic!("producer failed");
    };
    assert_eq!(source_sha256, FIXTURE_SHA);
    let csv_path = dir.join("export.csv");
    let mut csv = fs::read(&csv_path).unwrap();
    assert!(!csv.is_empty());
    csv[0] ^= 0x01;
    fs::write(&csv_path, &csv).unwrap();
    let checked = check_dir(&dir, &config.aead_key, &source_sha256).unwrap_or_else(|fault| panic!("check {}", fault.code));
    remove_staging(&config.object_dir, &dir).unwrap();
    let Checked::Reject { reason, source_sha256 } = checked else {
        panic!("checksum was accepted");
    };
    assert_eq!(reason, "checksum");
    let outcome = store.record_loss(&config_scope(config), reason, &source_sha256, "accept-checksum-0001", &sha256_hex(b"checksum-probe")).await.unwrap();
    let Outcome::Loss(body) = outcome else {
        panic!("checksum was stored");
    };
    assert_eq!(body["reason"], "checksum");
    assert_eq!(body["session_extended"], false);
    assert_eq!(store.anchor_end("session_a398").await.unwrap(), *anchor);
}

fn config_scope(config: &artifact_control_api::Config) -> Scope {
    read_scope_files(&config.scope_file, &config.scope_mac_file, &config.aead_key).unwrap()
}

fn producer_sha(config: &artifact_control_api::Config) -> String {
    let produced = produce(config, FIXTURE_NAME, &columns()).unwrap_or_else(|fault| panic!("producer {}", fault.code));
    let Produced::Files { dir, source_sha256 } = produced else {
        panic!("producer failed");
    };
    let checked = check_dir(&dir, &config.aead_key, &source_sha256).unwrap_or_else(|fault| panic!("check {}", fault.code));
    remove_staging(&config.object_dir, &dir).unwrap();
    match checked {
        Checked::Ready(ready) => ready.plaintext_sha256,
        Checked::Reject { reason, .. } => panic!("independent export {reason}"),
    }
}

struct Server {
    child: Child,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_server(bin: &Path, config: &Path, stderr_path: &Path) -> (Server, String) {
    let stderr = OpenOptions::new().create(true).append(true).open(stderr_path).expect("stderr");
    let mut child = Command::new(bin)
        .args(["serve", "--config"])
        .arg(config)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/tmp")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("spawn");
    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let _ = reader.read_line(&mut line);
        let _ = tx.send(line);
        let mut sink = Vec::new();
        let _ = std::io::Read::read_to_end(&mut reader, &mut sink);
    });
    let server = Server { child };
    let line = rx.recv_timeout(Duration::from_secs(30)).unwrap_or_default();
    (server, line)
}

fn install_cli(bin: &Path, config: &Path) {
    let output = Command::new(bin)
        .args(["install", "--config"])
        .arg(config)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/tmp")
        .output()
        .expect("install");
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stdout));
    let body: Value = serde_json::from_slice(&output.stdout).expect("install json");
    assert!(body["disposition"] == "installed" || body["disposition"] == "unchanged");
    assert_eq!(body["parent_complete"], false);
}

struct Env {
    state: PathBuf,
    bin: PathBuf,
    fixture_dir: PathBuf,
    https_ca: PathBuf,
    aead_key: PathBuf,
    proof: PathBuf,
    pg_host: String,
    pg_port: u16,
    pg_password: PathBuf,
    pg_ca: PathBuf,
    tls_cert: PathBuf,
    tls_key: PathBuf,
    export_bin: PathBuf,
}

impl Env {
    fn load() -> Self {
        Self {
            state: path_env("OO_A398_STATE"),
            bin: path_env("OO_A398_BIN"),
            fixture_dir: path_env("OO_A398_FIXTURE_DIR"),
            https_ca: path_env("OO_A398_HTTPS_CA"),
            aead_key: path_env("OO_A398_AEAD_KEY"),
            proof: path_env("OO_A398_PROOF"),
            pg_host: std::env::var("OO_A398_PG_HOST").expect("pg host"),
            pg_port: std::env::var("OO_A398_PG_PORT").expect("pg port").parse().expect("pg port"),
            pg_password: path_env("OO_A398_PG_PASSWORD_FILE"),
            pg_ca: path_env("OO_A398_PG_CA"),
            tls_cert: path_env("OO_A398_TLS_CERT"),
            tls_key: path_env("OO_A398_TLS_KEY"),
            export_bin: path_env("OO_A398_EXPORT_BIN"),
        }
    }
}

struct Dirs {
    config: PathBuf,
    scope: PathBuf,
    mac: PathBuf,
    objects: PathBuf,
    intake: PathBuf,
}

impl Dirs {
    fn from_state(state: &Path) -> Self {
        Self {
            config: state.join("config.json"),
            scope: state.join("scope.json"),
            mac: state.join("scope.mac"),
            objects: state.join("objects"),
            intake: state.join("intake"),
        }
    }
}

fn prepare_dirs(env: &Env) -> Dirs {
    let dirs = Dirs::from_state(&env.state);
    private_dir(&dirs.objects);
    private_dir(&dirs.intake);
    dirs
}

fn write_config(env: &Env, dirs: &Dirs) {
    let body = json!({
        "version": CONTRACT,
        "deployment": "local",
        "listen_host": "0.0.0.0",
        "listen_port": 8443,
        "https_origin": ORIGIN,
        "pg_host": env.pg_host,
        "pg_port": env.pg_port,
        "pg_user": "oo_artifact_admin",
        "pg_database": "artifact",
        "pg_password_file": env.pg_password,
        "pg_ca_file": env.pg_ca,
        "tls_cert_file": env.tls_cert,
        "tls_key_file": env.tls_key,
        "aead_key_file": env.aead_key,
        "scope_file": dirs.scope,
        "scope_mac_file": dirs.mac,
        "object_dir": dirs.objects,
        "intake_root": dirs.intake,
        "export_bin": env.export_bin,
        "workspace_enabled": false
    });
    fs::write(&dirs.config, serde_json::to_vec_pretty(&body).unwrap()).unwrap();
}

fn scope_of(principal: &str, session: &str, role: &str, policy: &str, knowledge: &str, digest: &str, end: i64) -> Scope {
    Scope {
        principal_id: principal.to_owned(),
        project_id: "shop".to_owned(),
        session_id: session.to_owned(),
        role: role.to_owned(),
        policy_id: policy.to_owned(),
        knowledge_id: knowledge.to_owned(),
        knowledge_digest: digest.to_owned(),
        absolute_end_unix: end,
        revision: 1,
    }
}

fn columns() -> Vec<String> {
    ["order_id", "customer_id", "memo", "posted", "amount", "clerk_note"].into_iter().map(str::to_owned).collect()
}

fn poison_columns() -> Vec<String> {
    ["order_id", "customer_id", "memo", "posted", "amount", "-----BEGIN PRIVATE KEY-----"].into_iter().map(str::to_owned).collect()
}

fn accept_body(source: &str, columns: &[String], key: &str, workspace: Option<bool>) -> String {
    let mut body = json!({
        "version": CONTRACT,
        "source_file": source,
        "columns": columns,
        "idempotency_key": key
    });
    if let Some(value) = workspace {
        body["workspace"] = json!(value);
    }
    body.to_string()
}

struct Http {
    client: reqwest::Client,
    cookie: String,
    csrf: String,
}

struct Reply {
    status: u16,
    headers: reqwest::header::HeaderMap,
    bytes: Vec<u8>,
    body: Value,
}

impl Reply {
    fn code(&self) -> String {
        self.body.get("code").and_then(Value::as_str).unwrap_or("").to_owned()
    }

    fn header(&self, name: &str) -> String {
        self.headers.get(name).and_then(|value| value.to_str().ok()).unwrap_or("").to_owned()
    }
}

impl Http {
    fn open(ca_path: &Path) -> Self {
        let ca = fs::read(ca_path).expect("ca");
        let certs = reqwest::Certificate::from_pem_bundle(&ca).expect("ca pem");
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .https_only(true)
            .http1_only()
            .tls_certs_only(certs)
            .timeout(Duration::from_secs(20))
            .build()
            .expect("client");
        Self { client, cookie: String::new(), csrf: String::new() }
    }

    async fn consume(&mut self) {
        let reply = self.send("POST", "/control/v1/session/consume", None).await;
        assert_eq!(reply.status, 200, "{}", reply.code());
        let cookie = reply.header("set-cookie");
        let token = cookie.split(';').next().unwrap_or("").strip_prefix("oo-artifact=").unwrap_or("");
        assert!(token_ok(token));
        self.cookie = token.to_owned();
        self.csrf = reply.body["csrf"].as_str().expect("csrf").to_owned();
        assert!(token_ok(&self.csrf));
        assert_eq!(reply.body["principal_id"], "clerk");
        assert!(reply.body["signed_url"].is_null());
    }

    async fn get(&self, path: &str) -> Reply {
        self.send("GET", path, None).await
    }

    async fn post(&self, path: &str, body: &str) -> Reply {
        self.send("POST", path, Some(body)).await
    }

    async fn send(&self, method: &str, path: &str, body: Option<&str>) -> Reply {
        let url = format!("{ORIGIN}{path}");
        let mut request = self.client.request(method.parse().expect("method"), &url);
        if !self.cookie.is_empty() {
            request = request.header("cookie", format!("oo-artifact={}", self.cookie));
        }
        if method != "GET" {
            request = request.header("origin", ORIGIN);
        }
        if method != "GET" && !self.csrf.is_empty() {
            request = request.header("x-oo-csrf", &self.csrf);
        }
        if let Some(body) = body {
            request = request.header("content-type", "application/json").body(body.to_owned());
        }
        let response = request.send().await.unwrap_or_else(|error| {
            let mut parts = Vec::new();
            let mut current = std::error::Error::source(&error);
            while let Some(inner) = current {
                parts.push(inner.to_string());
                current = std::error::Error::source(inner);
            }
            let mut detail = error.without_url().to_string();
            for part in parts {
                detail.push_str(" | ");
                detail.push_str(&part);
            }
            panic!("{method} {path} {detail}");
        });
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let bytes = response.bytes().await.unwrap_or_default().to_vec();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Reply { status, headers, bytes, body }
    }
}

async fn timed_get(http: &Http, path: &str) -> (u16, String, u64) {
    let start = Instant::now();
    let reply = http.get(path).await;
    let ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    (reply.status, reply.code(), ms)
}

fn private_dir(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn path_env(name: &str) -> PathBuf {
    PathBuf::from(std::env::var(name).unwrap_or_else(|_| panic!("{name}")))
}

fn unix_now() -> i64 {
    i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()).unwrap()
}
