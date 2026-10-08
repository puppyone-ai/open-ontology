#![forbid(unsafe_code)]

// Same fixture as the live suite. This loop only drives Mode::Ok.
#[allow(dead_code)]
#[path = "live/idp.rs"]
mod idp;

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{json, Value};
use tokio_postgres::config::SslMode;

use control_identity::{canonical_commit, capabilities_for, load_config, management_document, pg_tls, Binding, Role};
use idp::{Idp, IdpParts, Mode};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn management_binding_read_write_loop() {
    if std::env::var("OO_A402_LIVE").ok().as_deref() != Some("1") {
        panic!("OO_A402_LIVE=1 is required");
    }
    let env = LiveEnv {
        host: env_var("OO_A402_PG_HOST"),
        port: env_var("OO_A402_PG_PORT").parse().expect("pg port"),
        database: env_var("OO_A402_PG_DATABASE"),
        password_file: env_var("OO_A402_PG_PASSWORD_FILE"),
        ca_file: env_var("OO_A402_PG_CA"),
        proof: env_var("OO_A402_PROOF"),
    };
    let binding_bin = env_var("OO_A402_BINDING_BIN");
    let mut proof = Proof { path: env.proof.clone(), cases: Vec::new(), forbidden: Vec::new() };
    let password = read_secret(&env.password_file);
    proof.forbidden.push(password.clone());
    let dir = std::env::temp_dir().join(format!("oo-a402-loop-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    let repo = fs::canonicalize(dir.join("repo")).or_else(|_| {
        fs::create_dir_all(dir.join("repo")).expect("repo");
        fs::canonicalize(dir.join("repo"))
    }).expect("canonical repo");
    let material = certs(&dir);
    let secret = openssl_hex();
    proof.forbidden.push(secret.clone());
    write_secret(&dir.join("client.secret"), &secret);
    let product_port = free_port();
    let binding_port = free_port();
    let idp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("idp bind");
    let idp_port = idp_listener.local_addr().expect("idp addr").port();
    let origin = format!("https://127.0.0.1:{product_port}");
    let binding_origin = format!("https://127.0.0.1:{binding_port}");
    let issuer = format!("https://127.0.0.1:{idp_port}");
    let callback = format!("{origin}/control/v1/auth/callback");
    let identity_config = dir.join("identity.json");
    let bindings = sample_bindings();
    write_identity_config(
        &identity_config,
        &env,
        &dir,
        Listen { origin: &origin, issuer: &issuer, callback: &callback, port: product_port },
        &bindings,
        &repo,
    );
    let config = load_config(&identity_config).expect("config");
    let document = management_document(&config.bindings).expect("document");
    fs::write(dir.join("document.json"), document.as_bytes()).expect("document");
    let pg = pg_client(&env, &password).await;
    pg.batch_execute("DROP SCHEMA IF EXISTS oo_identity_control CASCADE").await.expect("drop");
    let exe = env!("CARGO_BIN_EXE_control-identity");
    let install = cli(exe, &["install", "--config", identity_config.to_str().expect("path")]);
    assert_eq!(install.status, 0, "install");
    assert_eq!(install.body["disposition"], "committed");
    proof.record("install", 0, "committed");
    let token = issue_token(exe, &identity_config, &dir.join("bootstrap.token"));
    proof.forbidden.push(token.clone());
    let der_keys = vec![fs::read(dir.join("k1.der")).expect("k1"), fs::read(dir.join("k2.der")).expect("k2")];
    let idp = idp::start(
        idp_listener,
        IdpParts {
            cert_pem: &material.cert,
            key_pem: &material.key,
            issuer: &issuer,
            client_id: &config.client_id,
            configured_audience: &config.audience,
            client_secret: &secret,
            redirect_uri: &callback,
            der_keys: &der_keys,
        },
    )
    .await;
    write_binding_config(&dir.join("binding.json"), &dir, &binding_origin, &origin, binding_port, &repo);
    let seeded = cli(&binding_bin, &["seed", "--config", dir.join("binding.json").to_str().expect("cfg"), "--document", dir.join("document.json").to_str().expect("doc")]);
    assert_eq!(seeded.status, 0, "seed {}", seeded.body);
    let seed_commit = seeded.body["git_commit"].as_str().expect("seed").to_string();
    assert_eq!(git_head(&repo), seed_commit);
    let mut identity = ServerProc::spawn(exe, &identity_config);
    let mut binding = ServerProc::spawn(&binding_bin, &dir.join("binding.json"));
    let mut http = Http::new(&origin, &material.ca);
    idp.set_subject("fixture-admin");
    idp.set_mode(Mode::Ok);
    let callback = login_follow(&mut http, &format!("/control/v1/auth/login?return_to=%2Fapp&bootstrap_token={token}")).await;
    assert_eq!(callback.status, 302, "admin login");
    let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(current.status, 200);
    assert_eq!(current.body["projects"][0]["role"], "admin");
    assert_eq!(current.body["data_grants"], "none");
    assert_eq!(current.body["gateway_token"], "not_issued");
    let csrf = current.body["csrf_token"].as_str().expect("csrf").to_string();
    let revision = current.body["revision"].as_i64().expect("revision");
    let config_commit = current.body["config_commit"].as_str().expect("commit").to_string();
    assert_eq!(revision, 1);
    let audits = audit_count(&pg).await;
    let preview = bind_call(&http, &binding_origin, "/binding/v1/preview", &grant_body("alpha", "person-reviewer", "reviewer", revision, &config_commit, &seed_commit, "grant-reviewer"), &csrf, true).await;
    assert_eq!(preview.status, 200, "preview {}", preview.body);
    assert_eq!(preview.body["outcome"], "planned");
    assert_eq!(preview.body["contract"], "oo.identity-binding/v1");
    assert_eq!(git_head(&repo), seed_commit);
    assert_eq!(audit_count(&pg).await, audits);
    let head_row = pg.query_one("SELECT revision FROM oo_identity_control.binding_head", &[]).await.expect("head");
    let head_revision: i64 = head_row.get(0);
    assert_eq!(head_revision, 1);
    proof.record("preview_no_write", 200, "planned");
    let approved = bind_call(&http, &binding_origin, "/binding/v1/approve", &grant_body("alpha", "person-reviewer", "reviewer", revision, &config_commit, &seed_commit, "grant-reviewer"), &csrf, true).await;
    assert_eq!(approved.status, 200, "approve {}", approved.body);
    assert_eq!(approved.body["disposition"], "committed");
    assert_eq!(approved.body["outcome"], "applied");
    assert_eq!(approved.body["data_grants"], "none");
    assert_eq!(approved.body["gateway_token"], "not_issued");
    assert_eq!(approved.body["customer_integration"], "not_claimed");
    let new_head = git_head(&repo);
    assert_ne!(new_head, seed_commit);
    assert_eq!(approved.body["git_commit"], new_head);
    assert_eq!(audit_count(&pg).await, audits + 1);
    proof.record("approve_reviewer", 200, "committed");
    let replay = bind_call(&http, &binding_origin, "/binding/v1/approve", &grant_body("alpha", "person-reviewer", "reviewer", revision, &config_commit, &seed_commit, "grant-reviewer"), &csrf, true).await;
    assert_eq!(replay.status, 200, "replay {}", replay.body);
    assert_eq!(replay.body["disposition"], "replayed");
    assert_eq!(replay.body["receipt_sha256"], approved.body["receipt_sha256"]);
    assert_eq!(audit_count(&pg).await, audits + 1);
    assert_eq!(git_head(&repo), new_head);
    proof.record("idempotent_replay", 200, "replayed");
    let clash = bind_call(&http, &binding_origin, "/binding/v1/approve", &grant_body("alpha", "person-other", "auditor", revision, &config_commit, &seed_commit, "grant-reviewer"), &csrf, true).await;
    assert_eq!(clash.status, 409, "clash {}", clash.body);
    assert_eq!(error_code(&clash.body), "idempotency.conflict");
    assert_eq!(audit_count(&pg).await, audits + 1);
    proof.record("idempotency_conflict", 409, "idempotency.conflict");
    let stale_current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(stale_current.status, 409);
    assert_eq!(error_code(&stale_current.body), "revision.conflict");
    proof.record("stale_session_current", 409, "revision.conflict");
    let logout = http.call("POST", "/control/v1/auth/logout", None, true, Some(&csrf)).await;
    assert_eq!(logout.status, 200, "logout {}", logout.body);
    proof.record("logout_after_apply", 200, "revoked");
    idp.set_subject("person-reviewer");
    let reviewer_login = login_follow(&mut http, "/control/v1/auth/login?return_to=%2Fapp").await;
    assert_eq!(reviewer_login.status, 302, "reviewer login");
    let reviewer = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(reviewer.status, 200, "reviewer current {}", reviewer.body);
    assert_eq!(reviewer.body["projects"][0]["role"], "reviewer");
    assert_eq!(reviewer.body["projects"][0]["project_id"], "alpha");
    assert_eq!(reviewer.body["projects"][0]["capabilities"], json!(capabilities_for("reviewer").expect("caps")));
    assert_eq!(reviewer.body["owner_certification"], "not_issued");
    assert_eq!(reviewer.body["harness_session"], "not_issued");
    assert_eq!(reviewer.body["agent_session"], "not_issued");
    proof.record("reviewer_current", 200, "reviewer");
    let reviewer_csrf = reviewer.body["csrf_token"].as_str().expect("csrf").to_string();
    let reviewer_revision = reviewer.body["revision"].as_i64().expect("rev");
    let reviewer_commit = reviewer.body["config_commit"].as_str().expect("commit").to_string();
    let denied = bind_call(&http, &binding_origin, "/binding/v1/approve", &grant_body("alpha", "person-auditor", "auditor", reviewer_revision, &reviewer_commit, &new_head, "reviewer-cannot"), &reviewer_csrf, true).await;
    assert_eq!(denied.status, 403, "reviewer approve {}", denied.body);
    assert_eq!(error_code(&denied.body), "forbidden");
    assert_eq!(git_head(&repo), new_head);
    proof.record("reviewer_forbidden", 403, "forbidden");
    idp.set_subject("fixture-admin");
    assert_eq!(login_follow(&mut http, "/control/v1/auth/login?return_to=%2Fapp").await.status, 302);
    let admin = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(admin.status, 200);
    let csrf = admin.body["csrf_token"].as_str().expect("csrf").to_string();
    let revision = admin.body["revision"].as_i64().expect("revision");
    let config_commit = admin.body["config_commit"].as_str().expect("commit").to_string();
    let same = bind_call(&http, &binding_origin, "/binding/v1/approve", &grant_body("alpha", "person-reviewer", "reviewer", revision, &config_commit, &new_head, "grant-reviewer-again"), &csrf, true).await;
    assert_eq!(same.status, 200, "unchanged {}", same.body);
    assert_eq!(same.body["disposition"], "unchanged");
    assert_eq!(same.body["outcome"], "unchanged");
    assert_eq!(git_head(&repo), new_head);
    assert_eq!(audit_count(&pg).await, audits + 1);
    proof.record("unchanged", 200, "unchanged");
    let elevated = bind_call(&http, &binding_origin, "/binding/v1/approve", &grant_body("alpha", "fixture-admin", "admin", revision, &config_commit, &new_head, "self-elevate"), &csrf, true).await;
    assert_eq!(elevated.status, 403, "self {}", elevated.body);
    assert_eq!(error_code(&elevated.body), "binding.elevation");
    proof.record("self_elevation", 403, "binding.elevation");
    let self_revoke = bind_call(&http, &binding_origin, "/binding/v1/approve", &revoke_body("alpha", "fixture-admin", revision, &config_commit, &new_head, "self-revoke"), &csrf, true).await;
    assert_eq!(self_revoke.status, 403, "self revoke {}", self_revoke.body);
    assert_eq!(error_code(&self_revoke.body), "binding.elevation");
    proof.record("self_revoke", 403, "binding.elevation");
    let cross = bind_call(&http, &binding_origin, "/binding/v1/approve", &grant_body("beta", "person-beta", "admin", revision, &config_commit, &new_head, "cross-project"), &csrf, true).await;
    assert_eq!(cross.status, 403, "cross {}", cross.body);
    assert_eq!(error_code(&cross.body), "forbidden");
    proof.record("cross_project", 403, "forbidden");
    let bad_csrf = bind_call(&http, &binding_origin, "/binding/v1/approve", &grant_body("alpha", "person-auditor", "auditor", revision, &config_commit, &new_head, "bad-csrf"), "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", true).await;
    assert_eq!(bad_csrf.status, 403);
    assert_eq!(error_code(&bad_csrf.body), "csrf.rejected");
    proof.record("bad_csrf", 403, "csrf.rejected");
    let stale = bind_call(&http, &binding_origin, "/binding/v1/approve", &grant_body("alpha", "person-auditor", "auditor", 1, &config_commit, &seed_commit, "stale-revision"), &csrf, true).await;
    assert_eq!(stale.status, 409, "stale {}", stale.body);
    assert_eq!(error_code(&stale.body), "revision.conflict");
    proof.record("stale_revision", 409, "revision.conflict");
    let mut caller_json: Value = serde_json::from_str(&grant_body("alpha", "person-auditor", "auditor", revision, &config_commit, &new_head, "caller-json")).expect("json");
    caller_json["authorized"] = json!(true);
    let forged = bind_call(&http, &binding_origin, "/binding/v1/approve", &caller_json.to_string(), &csrf, true).await;
    assert_eq!(forged.status, 400, "forged {}", forged.body);
    assert_eq!(error_code(&forged.body), "malformed");
    assert_eq!(git_head(&repo), new_head);
    proof.record("caller_json", 400, "malformed");
    assert_bootstrap_frozen(&mut http, &idp, &binding_origin, &repo).await;
    proof.record("bootstrap_frozen", 403, "binding.frozen");
    let granted_auditor = grant_role(&mut http, &idp, &binding_origin, &repo, "person-auditor", "auditor", "grant-auditor").await;
    proof.record("approve_auditor", granted_auditor.status, "applied");
    let granted_owner = grant_role(&mut http, &idp, &binding_origin, &repo, "person-owner", "business_owner", "grant-owner").await;
    proof.record("approve_owner", granted_owner.status, "applied");
    assert_role(&mut http, &idp, "person-auditor", "auditor").await;
    assert_role(&mut http, &idp, "person-owner", "business_owner").await;
    proof.record("roles_visible", 200, "ok");
    identity.kill();
    identity = ServerProc::spawn(exe, &identity_config);
    assert_role(&mut http, &idp, "person-reviewer", "reviewer").await;
    proof.record("restart_retains", 200, "reviewer");
    idp.set_subject("fixture-admin");
    assert_eq!(login_follow(&mut http, "/control/v1/auth/login?return_to=%2Fapp").await.status, 302);
    let admin = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    let csrf = admin.body["csrf_token"].as_str().expect("csrf").to_string();
    let revision = admin.body["revision"].as_i64().expect("revision");
    let config_commit = admin.body["config_commit"].as_str().expect("commit").to_string();
    let head = git_head(&repo);
    let before_audit = audit_count(&pg).await;
    let left = grant_body("alpha", "person-race-a", "steward", revision, &config_commit, &head, "race-a");
    let right = grant_body("alpha", "person-race-b", "steward", revision, &config_commit, &head, "race-b");
    let (one, two) = tokio::join!(
        bind_call(&http, &binding_origin, "/binding/v1/approve", &left, &csrf, true),
        bind_call(&http, &binding_origin, "/binding/v1/approve", &right, &csrf, true)
    );
    let winners = [one.status, two.status].iter().filter(|status| **status == 200).count();
    assert_eq!(winners, 1, "one writer wins {} {}", one.body, two.body);
    assert_eq!(audit_count(&pg).await, before_audit + 1);
    let after = pg.query_one("SELECT revision FROM oo_identity_control.binding_head", &[]).await.expect("head");
    let after_revision: i64 = after.get(0);
    assert_eq!(after_revision, revision + 1);
    let present = pg.query_one("SELECT COUNT(*)::bigint FROM oo_identity_control.management_binding WHERE subject IN ('person-race-a', 'person-race-b')", &[]).await.expect("rows");
    let present_count: i64 = present.get(0);
    assert_eq!(present_count, 1);
    proof.record("two_writer_cas", 200, "one");
    let roles = pg.query_one("SELECT COUNT(*)::bigint FROM pg_catalog.pg_roles WHERE rolname ~ '^oo_r_'", &[]).await.expect("roles");
    let role_count: i64 = roles.get(0);
    assert_eq!(role_count, 0);
    let schemas = pg.query_one("SELECT COUNT(*)::bigint FROM pg_catalog.pg_namespace WHERE nspname = 'oo_identity_control'", &[]).await.expect("schema");
    let schema_count: i64 = schemas.get(0);
    assert_eq!(schema_count, 1);
    proof.record("no_data_roles", 0, "none");
    identity.kill();
    binding.kill();
    proof.record("complete", 0, "ok");
    let _ = fs::remove_dir_all(&dir);
}

async fn assert_bootstrap_frozen(http: &mut Http, idp: &Idp, binding_origin: &str, repo: &Path) {
    let granted = grant_role(http, idp, binding_origin, repo, "person-deputy", "admin", "grant-deputy").await;
    assert_eq!(granted.status, 200, "deputy {}", granted.body);
    idp.set_subject("person-deputy");
    idp.set_mode(Mode::Ok);
    assert_eq!(login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await.status, 302, "deputy login");
    let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(current.status, 200, "deputy current {}", current.body);
    assert_eq!(current.body["projects"][0]["role"], "admin");
    let csrf = current.body["csrf_token"].as_str().expect("csrf").to_string();
    let revision = current.body["revision"].as_i64().expect("revision");
    let config_commit = current.body["config_commit"].as_str().expect("commit").to_string();
    let head = git_head(repo);
    let frozen = bind_call(http, binding_origin, "/binding/v1/approve", &revoke_body("alpha", "fixture-admin", revision, &config_commit, &head, "deputy-revoke-bootstrap"), &csrf, true).await;
    assert_eq!(frozen.status, 403, "frozen {}", frozen.body);
    assert_eq!(error_code(&frozen.body), "binding.frozen");
    assert_eq!(git_head(repo), head);
}

async fn grant_role(http: &mut Http, idp: &Idp, binding_origin: &str, repo: &Path, subject: &str, role: &str, key: &str) -> Reply {
    idp.set_subject("fixture-admin");
    idp.set_mode(Mode::Ok);
    assert_eq!(login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await.status, 302, "relogin {subject}");
    let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(current.status, 200, "admin current before {role}");
    let csrf = current.body["csrf_token"].as_str().expect("csrf").to_string();
    let revision = current.body["revision"].as_i64().expect("revision");
    let config_commit = current.body["config_commit"].as_str().expect("commit").to_string();
    let head = git_head(repo);
    let reply = bind_call(http, binding_origin, "/binding/v1/approve", &grant_body("alpha", subject, role, revision, &config_commit, &head, key), &csrf, true).await;
    assert_eq!(reply.status, 200, "{role} grant {}", reply.body);
    assert_eq!(reply.body["outcome"], "applied");
    reply
}

async fn assert_role(http: &mut Http, idp: &Idp, subject: &str, role: &str) {
    idp.set_subject(subject);
    idp.set_mode(Mode::Ok);
    assert_eq!(login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await.status, 302, "login {subject}");
    let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(current.status, 200, "{subject} current {}", current.body);
    assert_eq!(current.body["projects"][0]["role"], role);
    assert_eq!(current.body["projects"][0]["capabilities"], json!(capabilities_for(role).expect("caps")));
    assert_eq!(current.body["data_grants"], "none");
    assert_eq!(current.body["gateway_token"], "not_issued");
}

fn sample_bindings() -> Vec<Binding> {
    vec![
        Binding { subject: "fixture-admin".into(), project_id: "alpha".into(), role: Role::Admin },
        Binding { subject: "fixture-reviewer".into(), project_id: "beta".into(), role: Role::Reviewer },
        Binding { subject: "fixture-steward".into(), project_id: "gamma".into(), role: Role::Steward },
        Binding { subject: "fixture-auditor".into(), project_id: "delta".into(), role: Role::Auditor },
        Binding { subject: "fixture-owner".into(), project_id: "epsilon".into(), role: Role::BusinessOwner },
    ]
}

struct ChangeFields<'a> {
    effect: &'a str,
    project: &'a str,
    subject: &'a str,
    role: Option<&'a str>,
    revision: i64,
    config_commit: &'a str,
    git_head: &'a str,
    key: &'a str,
}

fn change_body(fields: ChangeFields<'_>) -> String {
    let mut body = json!({
        "effect": fields.effect,
        "project_id": fields.project,
        "subject": fields.subject,
        "expected_revision": fields.revision,
        "expected_config_commit": fields.config_commit,
        "expected_git_head": fields.git_head,
        "idempotency_key": fields.key
    });
    if let Some(role) = fields.role {
        body["role"] = json!(role);
    }
    body.to_string()
}

fn grant_body(project: &str, subject: &str, role: &str, revision: i64, config_commit: &str, git_head: &str, key: &str) -> String {
    change_body(ChangeFields { effect: "grant", project, subject, role: Some(role), revision, config_commit, git_head, key })
}

fn revoke_body(project: &str, subject: &str, revision: i64, config_commit: &str, git_head: &str, key: &str) -> String {
    change_body(ChangeFields { effect: "revoke", project, subject, role: None, revision, config_commit, git_head, key })
}

fn git_head(root: &Path) -> String {
    fs::read_to_string(root.join(".git/refs/heads/management")).expect("ref").trim().to_string()
}

async fn audit_count(pg: &tokio_postgres::Client) -> i64 {
    let row = pg.query_one("SELECT COUNT(*)::bigint FROM oo_identity_control.audit_event WHERE action = 'binding_applied'", &[]).await.expect("audit");
    row.get(0)
}

struct LiveEnv {
    host: String,
    port: u16,
    database: String,
    password_file: String,
    ca_file: String,
    proof: String,
}

struct Proof {
    path: String,
    cases: Vec<Value>,
    forbidden: Vec<String>,
}

impl Proof {
    fn record(&mut self, name: &str, http: u16, code: impl AsRef<str>) {
        self.cases.push(json!({ "name": name, "http": http, "code": code.as_ref() }));
    }
}

impl Drop for Proof {
    fn drop(&mut self) {
        let text = json!({
            "contract": "oo.identity-binding/v1",
            "customer_integration": "not_claimed",
            "data_grants": "none",
            "cases": self.cases
        })
        .to_string();
        let leaked = self.forbidden.iter().any(|secret| !secret.is_empty() && text.contains(secret));
        let body = if leaked {
            r#"{"contract":"oo.identity-binding/v1","customer_integration":"not_claimed","proof":"redacted"}"#.to_string()
        } else {
            text
        };
        let _ = fs::write(&self.path, body);
    }
}

struct Reply {
    status: u16,
    location: String,
    body: Value,
}

struct Http {
    client: reqwest::Client,
    origin: String,
    login: Option<String>,
    session: Option<String>,
}

struct ServerProc {
    child: Option<std::process::Child>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<()>>,
}

async fn bind_call(http: &Http, origin: &str, path: &str, body: &str, csrf: &str, origin_ok: bool) -> Reply {
    let url = format!("{origin}{path}");
    let mut request = http.client.post(url).header("content-type", "application/json").header("x-oo-csrf", csrf).body(body.to_string());
    if origin_ok {
        request = request.header("origin", origin);
    }
    if let Some(session) = &http.session {
        request = request.header("cookie", format!("__Host-oo-session={session}"));
    }
    let response = request.send().await.expect("binding");
    reply_from(response).await
}

async fn login_follow(http: &mut Http, path: &str) -> Reply {
    let started = http.call("GET", path, None, false, None).await;
    if started.status != 302 {
        return started;
    }
    let idp_reply = http.get_external(&started.location).await;
    if idp_reply.status != 302 {
        return idp_reply;
    }
    let path = idp_reply.location.trim_start_matches(&http.origin).to_string();
    http.call("GET", &path, None, false, None).await
}

impl Http {
    fn new(origin: &str, ca_pem: &[u8]) -> Self {
        let certs = reqwest::Certificate::from_pem_bundle(ca_pem).expect("ca");
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .https_only(true)
            .http1_only()
            .referer(false)
            .connection_verbose(false)
            .tls_certs_only(certs)
            .timeout(Duration::from_secs(20))
            .build()
            .expect("client");
        Self { client, origin: origin.to_string(), login: None, session: None }
    }

    async fn call(&mut self, method: &str, path: &str, body: Option<&str>, origin: bool, csrf: Option<&str>) -> Reply {
        let url = format!("{}{path}", self.origin);
        let mut request = self.client.request(method.parse().expect("method"), &url);
        if let Some(cookie) = self.cookie_header() {
            request = request.header("cookie", cookie);
        }
        if origin {
            request = request.header("origin", &self.origin);
        }
        if let Some(csrf) = csrf {
            request = request.header("x-oo-csrf", csrf);
        }
        if let Some(body) = body {
            request = request.header("content-type", "application/json").body(body.to_string());
        }
        let response = request.send().await.expect("request");
        self.absorb(&response);
        reply_from(response).await
    }

    async fn get_external(&self, url: &str) -> Reply {
        let response = self.client.get(url).send().await.expect("idp");
        reply_from(response).await
    }

    fn cookie_header(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(value) = &self.login {
            parts.push(format!("__Host-oo-login={value}"));
        }
        if let Some(value) = &self.session {
            parts.push(format!("__Host-oo-session={value}"));
        }
        if parts.is_empty() { None } else { Some(parts.join("; ")) }
    }

    fn absorb(&mut self, response: &reqwest::Response) {
        for value in response.headers().get_all(reqwest::header::SET_COOKIE) {
            let Ok(text) = value.to_str() else { continue };
            let (pair, _) = text.split_once(';').unwrap_or((text, ""));
            let Some((name, cookie)) = pair.split_once('=') else { continue };
            let stored = if cookie.is_empty() { None } else { Some(cookie.to_string()) };
            if name == "__Host-oo-login" {
                self.login = stored;
            } else if name == "__Host-oo-session" {
                self.session = stored;
            }
        }
    }
}

async fn reply_from(response: reqwest::Response) -> Reply {
    let status = response.status().as_u16();
    let location = response.headers().get(reqwest::header::LOCATION).and_then(|value| value.to_str().ok()).unwrap_or("").to_string();
    let bytes = response.bytes().await.unwrap_or_default();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Reply { status, location, body }
}

fn error_code(body: &Value) -> String {
    body.get("error").and_then(|error| error.get("code")).and_then(Value::as_str).unwrap_or("").to_string()
}

impl ServerProc {
    fn spawn(exe: &str, config: &Path) -> Self {
        let mut child = Command::new(exe)
            .args(["serve", "--config"])
            .arg(config)
            .env_clear()
            .env("HOME", "/tmp")
            .env("PATH", "/usr/bin:/bin")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("serve");
        let stdout = child.stdout.take().expect("stdout");
        let stderr = child.stderr.take().expect("stderr");
        let (sender, receiver) = std::sync::mpsc::channel();
        let stdout_thread = std::thread::spawn(move || {
            let mut stdout = stdout;
            let mut line = Vec::new();
            let mut byte = [0_u8; 1];
            loop {
                if stdout.read(&mut byte).ok() != Some(1) {
                    break;
                }
                line.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
            }
            let _ = sender.send(line);
            let mut rest = Vec::new();
            let _ = stdout.read_to_end(&mut rest);
        });
        let stderr_thread = std::thread::spawn(move || {
            let mut stderr = stderr;
            let mut local = Vec::new();
            let _ = stderr.read_to_end(&mut local);
        });
        let line = receiver.recv_timeout(Duration::from_secs(30)).expect("listening");
        let text = String::from_utf8(line).expect("utf8");
        let value: Value = serde_json::from_str(text.trim()).expect("listen json");
        assert_eq!(value["outcome"], "listening", "{text}");
        Self { child: Some(child), stdout_thread: Some(stdout_thread), stderr_thread: Some(stderr_thread) }
    }

    fn kill(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
        if let Some(handle) = self.stdout_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ServerProc {
    fn drop(&mut self) {
        self.kill();
    }
}

struct Cli {
    status: u8,
    body: Value,
}

fn cli(exe: &str, args: &[&str]) -> Cli {
    let output = Command::new(exe).args(args).env_clear().env("HOME", "/tmp").env("PATH", "/usr/bin:/bin").output().expect("cli");
    let status = output.status.code().unwrap_or(1) as u8;
    let body = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    if status != 0 {
        let _ = std::io::stderr().write_all(&output.stderr);
    }
    Cli { status, body }
}

fn issue_token(exe: &str, config: &Path, out: &Path) -> String {
    let result = cli(exe, &["bootstrap-issue", "--config", config.to_str().expect("path"), "--out", out.to_str().expect("out")]);
    assert_eq!(result.status, 0, "issue {}", result.body);
    fs::read_to_string(out).expect("token").trim().to_string()
}

struct Listen<'a> {
    origin: &'a str,
    issuer: &'a str,
    callback: &'a str,
    port: u16,
}

fn write_identity_config(path: &Path, env: &LiveEnv, dir: &Path, listen: Listen<'_>, bindings: &[Binding], repo: &Path) {
    let bootstrap = bindings[0].clone();
    let redirects = vec![listen.callback.to_string()];
    let commit = canonical_commit(listen.issuer, "oo-control", "oo-control-client", &redirects, bindings, &bootstrap, 1).expect("commit");
    let body = json!({
        "https_origin": listen.origin,
        "listen_host": "127.0.0.1",
        "listen_port": listen.port,
        "tls_cert_file": dir.join("srv.pem"),
        "tls_key_file": dir.join("srv.key"),
        "issuer": listen.issuer,
        "audience": "oo-control",
        "client_id": "oo-control-client",
        "client_secret_file": dir.join("client.secret"),
        "redirect_uris": redirects,
        "config_commit": commit,
        "revision": 1,
        "idp_ca_file": dir.join("ca.pem"),
        "pg": {
            "host": env.host,
            "port": env.port,
            "database": env.database,
            "user": "oo_identity_admin",
            "password_file": env.password_file,
            "ca_file": env.ca_file
        },
        "bindings": bindings.iter().map(|item| json!({"subject": item.subject, "project_id": item.project_id, "role": item.role.as_str()})).collect::<Vec<_>>(),
        "bootstrap": {"subject": bootstrap.subject, "project_id": bootstrap.project_id, "role": bootstrap.role.as_str()},
        "idle_seconds": 300,
        "absolute_seconds": 300,
        "oauth_ttl_seconds": 60,
        "bootstrap_ttl_seconds": 120,
        "clock_skew_seconds": 0,
        "binding_git": {
            "binary": "/usr/bin/git",
            "root": repo,
            "reference": "refs/heads/management"
        }
    });
    fs::write(path, serde_json::to_vec_pretty(&body).expect("config")).expect("write");
}

fn write_binding_config(path: &Path, dir: &Path, origin: &str, identity_origin: &str, port: u16, repo: &Path) {
    let body = json!({
        "https_origin": origin,
        "listen_host": "127.0.0.1",
        "listen_port": port,
        "tls_cert_file": dir.join("srv.pem"),
        "tls_key_file": dir.join("srv.key"),
        "identity_origin": identity_origin,
        "identity_ca_file": dir.join("ca.pem"),
        "git": {
            "binary": "/usr/bin/git",
            "root": repo,
            "reference": "refs/heads/management"
        }
    });
    fs::write(path, serde_json::to_vec_pretty(&body).expect("binding config")).expect("write");
}

struct Material {
    ca: Vec<u8>,
    cert: Vec<u8>,
    key: Vec<u8>,
}

fn certs(dir: &Path) -> Material {
    openssl(dir, &["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", "ca.key", "-out", "ca.pem", "-days", "2", "-subj", "/CN=oo-a402-test-ca", "-addext", "basicConstraints=CA:TRUE"]);
    openssl(dir, &["req", "-newkey", "rsa:2048", "-nodes", "-keyout", "srv.key", "-out", "srv.csr", "-subj", "/CN=127.0.0.1"]);
    fs::write(dir.join("ext.cnf"), "subjectAltName=IP:127.0.0.1\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n").expect("ext");
    openssl(dir, &["x509", "-req", "-in", "srv.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial", "-out", "srv.pem", "-days", "2", "-extfile", "ext.cnf"]);
    openssl(dir, &["genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", "k1.pem"]);
    openssl(dir, &["pkcs8", "-topk8", "-nocrypt", "-in", "k1.pem", "-outform", "DER", "-out", "k1.der"]);
    openssl(dir, &["genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", "k2.pem"]);
    openssl(dir, &["pkcs8", "-topk8", "-nocrypt", "-in", "k2.pem", "-outform", "DER", "-out", "k2.der"]);
    Material { ca: fs::read(dir.join("ca.pem")).expect("ca"), cert: fs::read(dir.join("srv.pem")).expect("cert"), key: fs::read(dir.join("srv.key")).expect("key") }
}

fn openssl(dir: &Path, args: &[&str]) {
    let status = Command::new("openssl").args(args).current_dir(dir).status().expect("openssl");
    assert!(status.success(), "openssl");
}

fn openssl_hex() -> String {
    let output = Command::new("openssl").args(["rand", "-hex", "32"]).output().expect("rand");
    assert!(output.status.success());
    String::from_utf8(output.stdout).expect("hex").trim().to_string()
}

fn write_secret(path: &Path, value: &str) {
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(path).expect("secret");
    writeln!(file, "{value}").expect("write");
}

fn read_secret(path: &str) -> String {
    let mut text = fs::read_to_string(path).expect("secret file");
    if text.ends_with('\n') {
        text.pop();
    }
    text
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("port");
    listener.local_addr().expect("addr").port()
}

async fn pg_client(env: &LiveEnv, password: &str) -> tokio_postgres::Client {
    let mut config = tokio_postgres::Config::new();
    config.user("oo_identity_admin");
    config.password(password);
    config.host(&env.host);
    config.port(env.port);
    config.dbname(&env.database);
    config.ssl_mode(SslMode::Require);
    let ca = fs::read(&env.ca_file).expect("pg ca");
    let tls = pg_tls(&ca).expect("pg tls");
    let (client, connection) = config.connect(tls).await.expect("pg");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("missing {name}"))
}
