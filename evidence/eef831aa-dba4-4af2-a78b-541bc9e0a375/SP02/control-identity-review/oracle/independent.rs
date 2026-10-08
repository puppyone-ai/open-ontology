#![allow(clippy::too_many_lines, clippy::too_many_arguments)]
#![forbid(unsafe_code)]

//! Independent oracle for E54. It drives the archived binary, a real RSA test issuer, and private PostgreSQL.
//! It does not change product bytes.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_postgres::config::SslMode;
use tokio_rustls::TlsAcceptor;

use control_identity::{
    authorize, canonical_commit, capabilities_for, load_config, pkcs8_rsa_public, pg_tls, server_config, sha256_hex, Binding, Config,
    Role, Store, TlsListener, VerifiedSession,
};

const CLIENT: &str = "rp-client";
const CONTRACT: &str = "oo.control-identity/v1";

struct Env {
    dir: PathBuf,
    exe: String,
    pg_host: String,
    pg_port: u16,
    pg_database: String,
    pg_password_file: String,
    pg_ca: String,
    proof: String,
    responses: String,
    truncate_note: String,
    child_lock: String,
    root_lock: String,
    scan_dir: String,
}

struct Report {
    cases: Vec<Value>,
    failed: bool,
    forbidden: Vec<String>,
}

struct Material {
    ca: Vec<u8>,
    cert: Vec<u8>,
    key: Vec<u8>,
    ders: Vec<Vec<u8>>,
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
    cookies_ok: bool,
}

struct Server {
    child: Option<std::process::Child>,
    stdout_thread: Option<std::thread::JoinHandle<()>>,
    stderr_thread: Option<std::thread::JoinHandle<()>>,
    stderr: Arc<Mutex<Vec<u8>>>,
}

#[derive(Clone)]
enum Aud {
    One(String),
    Multi(Vec<String>),
}

#[derive(Clone)]
struct Mint {
    aud: Aud,
    azp: Option<String>,
    iss: Option<String>,
    nonce: Option<String>,
    exp_offset: i64,
    iat_offset: i64,
    nbf_offset: i64,
    key_index: usize,
    kid_override: Option<String>,
    alg: String,
    bad_sig: bool,
    extra_claims: bool,
    idp_error: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JwksMode {
    Normal,
    Ambiguous,
    Unknown,
    Rotate,
}

struct Issued {
    challenge: String,
    nonce: String,
    used: bool,
}

struct SigningKey {
    kid: &'static str,
    pair: RsaKeyPair,
    n: String,
    e: String,
}

struct Inner {
    issuer: String,
    client_id: String,
    client_secret: String,
    keys: Vec<SigningKey>,
    rotate_floor: AtomicU64,
    codes: Mutex<HashMap<String, Issued>>,
    mint: Mutex<Mint>,
    subject: Mutex<String>,
    jwks_mode: Mutex<JwksMode>,
    jwks_hits: AtomicU64,
}

#[derive(Clone)]
struct Idp {
    inner: Arc<Inner>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_identity_oracle() {
    let env = env();
    let mut report = Report { cases: Vec::new(), failed: false, forbidden: Vec::new() };
    let password = read_secret(&env.pg_password_file);
    report.forbidden.push(password.clone());
    let _ = fs::remove_dir_all(&env.dir);
    fs::create_dir_all(&env.dir).expect("dir");
    let material = certs(&env.dir);
    let secret = openssl_hex(&env.dir);
    report.forbidden.push(secret.clone());
    write_secret(&env.dir.join("client.secret"), &secret);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("idp");
    let idp_port = listener.local_addr().expect("idp addr").port();
    let issuer = format!("https://127.0.0.1:{idp_port}");
    let idp = Idp::start(listener, &issuer, &secret, &material).await;
    wait_discovery(&material.ca, &issuer).await;
    let pg = pg_client(&env, &password).await;
    drop_schema(&pg).await;
    check_locks(&env, &mut report);
    let probe = audience_probe(&env, &material, &idp, &issuer, &pg, &mut report).await;
    drop_schema(&pg).await;
    let port = free_port();
    let origin = format!("https://127.0.0.1:{port}");
    let callback = format!("{origin}/control/v1/auth/callback");
    let config_path = env.dir.join("bound.json");
    write_config(&config_path, &env, &material, &origin, &issuer, &callback, port, CLIENT);
    let config = load_config(&config_path).expect("bound config");
    let install = cli(&env.exe, &["install", "--config", config_path.to_str().expect("path")]);
    report.check("install", 0, install.body["disposition"].as_str().unwrap_or(""), install.status == 0 && install.body["disposition"] == "committed");
    let mut server = Server::spawn(&env.exe, &config_path).await;
    let mut http = Http::new(&origin, &material.ca).await;
    let store = Store::connect(&config).await.expect("store");
    session_matrix(&mut http, &idp, &pg, &store, &config, &env, &config_path, &mut report).await;
    negatives(&mut http, &idp, &pg, &mut server, &env.exe, &config_path, &mut report).await;
    caller_json(&mut http, &env, &mut report).await;
    truncate_note(&pg, &env, &mut report).await;
    let stderr = server.stderr_text();
    server.kill();
    if report.forbidden.iter().any(|secret| !secret.is_empty() && stderr.contains(secret)) {
        report.failed = true;
        report.cases.push(json!({"name": "stderr_secret", "ok": false}));
    }
    if scan_secrets(&env.scan_dir, &report.forbidden) {
        report.failed = true;
        report.cases.push(json!({"name": "evidence_secret_scan", "ok": false}));
    } else {
        report.cases.push(json!({"name": "evidence_secret_scan", "ok": true}));
    }
    let binding_holds = probe["binding_holds"].as_bool() == Some(true);
    let verdict = if report.failed || !binding_holds { "WITHHOLD" } else { "APPROVE_LIMITED" };
    let body = json!({
        "contract": CONTRACT,
        "customer_integration": "not_claimed",
        "data_grants": "none",
        "verdict": verdict,
        "audience_probe": probe,
        "cases": report.cases,
    });
    let mut text = serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".to_string());
    let mut redacted = false;
    for secret in &report.forbidden {
        if !secret.is_empty() && text.contains(secret) {
            text = text.replace(secret, "REDACTED");
            redacted = true;
        }
    }
    if redacted {
        report.failed = true;
    }
    fs::write(&env.proof, &text).expect("proof");
    fs::write(Path::new(&env.proof).with_extension("verdict"), verdict).expect("verdict");
    let _ = fs::remove_dir_all(&env.dir);
    assert!(!report.failed, "independent assertions failed");
    assert!(verdict == "WITHHOLD" || verdict == "APPROVE_LIMITED");
}

fn env() -> Env {
    assert_eq!(std::env::var("OO_A375_ORACLE").ok().as_deref(), Some("1"), "OO_A375_ORACLE=1 is required");
    Env {
        dir: std::env::temp_dir().join(format!("oo-a375-oracle-{}", std::process::id())),
        exe: env!("CARGO_BIN_EXE_control-identity").to_string(),
        pg_host: std::env::var("OO_A375_PG_HOST").expect("pg host"),
        pg_port: std::env::var("OO_A375_PG_PORT").expect("pg port").parse().expect("pg port"),
        pg_database: std::env::var("OO_A375_PG_DATABASE").expect("pg database"),
        pg_password_file: std::env::var("OO_A375_PG_PASSWORD_FILE").expect("pg password file"),
        pg_ca: std::env::var("OO_A375_PG_CA").expect("pg ca"),
        proof: std::env::var("OO_A375_PROOF").expect("proof"),
        responses: std::env::var("OO_A375_RESPONSES").expect("responses"),
        truncate_note: std::env::var("OO_A375_TRUNCATE").expect("truncate"),
        child_lock: std::env::var("OO_A375_CHILD_LOCK").expect("child lock"),
        root_lock: std::env::var("OO_A375_ROOT_LOCK").expect("root lock"),
        scan_dir: std::env::var("OO_A375_SCAN_DIR").expect("scan"),
    }
}

fn check_locks(env: &Env, report: &mut Report) {
    let child = registry_packages(&fs::read_to_string(&env.child_lock).expect("child lock"));
    let root = registry_packages(&fs::read_to_string(&env.root_lock).expect("root lock"));
    let root_set: std::collections::BTreeSet<_> = root.into_iter().collect();
    let missing: Vec<_> = child.iter().filter(|item| !root_set.contains(item)).cloned().collect();
    let ok = missing.is_empty() && child.len() == 225;
    report.cases.push(json!({
        "name": "root_registry_subset",
        "count": child.len(),
        "missing": missing.len(),
        "ok": ok
    }));
    if !ok {
        report.failed = true;
    }
}

fn registry_packages(text: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut name = String::new();
    let mut version = String::new();
    let mut registry = false;
    let mut checksum = String::new();
    let flush = |name: &mut String, version: &mut String, registry: &mut bool, checksum: &mut String, out: &mut Vec<(String, String, String)>| {
        if *registry && !checksum.is_empty() {
            out.push((name.clone(), version.clone(), checksum.clone()));
        }
        name.clear();
        version.clear();
        checksum.clear();
        *registry = false;
    };
    for line in text.lines() {
        if line == "[[package]]" {
            flush(&mut name, &mut version, &mut registry, &mut checksum, &mut out);
            continue;
        }
        if let Some(value) = line.strip_prefix("name = ") {
            name = unquote(value);
        } else if let Some(value) = line.strip_prefix("version = ") {
            version = unquote(value);
        } else if let Some(value) = line.strip_prefix("source = ") {
            registry = unquote(value).starts_with("registry+");
        } else if let Some(value) = line.strip_prefix("checksum = ") {
            checksum = unquote(value);
        }
    }
    flush(&mut name, &mut version, &mut registry, &mut checksum, &mut out);
    out
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches('"').to_string()
}

async fn audience_probe(env: &Env, material: &Material, idp: &Idp, issuer: &str, pg: &tokio_postgres::Client, report: &mut Report) -> Value {
    let port = free_port();
    let origin = format!("https://127.0.0.1:{port}");
    let callback = format!("{origin}/control/v1/auth/callback");
    let path = env.dir.join("split.json");
    write_config(&path, env, material, &origin, issuer, &callback, port, "configured-aud");
    let install = cli(&env.exe, &["install", "--config", path.to_str().expect("path")]);
    report.check("split_install", install.status.into(), install.body["disposition"].as_str().unwrap_or(""), install.status == 0);
    let mut server = Server::spawn(&env.exe, &path).await;
    let mut http = Http::new(&origin, &material.ca).await;
    idp.set_subject("fixture-admin");
    idp.set_jwks(JwksMode::Normal);
    idp.set_mint(Mint::one("other-aud", Some(CLIENT.to_string())));
    let other = login_follow(&mut http, "/control/v1/auth/login?return_to=%2Fapp").await;
    idp.set_mint(Mint::one(CLIENT, Some(CLIENT.to_string())));
    let client_aud = login_follow(&mut http, "/control/v1/auth/login?return_to=%2Fapp").await;
    idp.set_mint(Mint::one("configured-aud", Some(CLIENT.to_string())));
    let configured = login_follow(&mut http, "/control/v1/auth/login?return_to=%2Fapp").await;
    let configured_session = configured.status == 302;
    if configured_session {
        let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
        report.check(
            "split_configured_aud_current",
            current.status,
            "ok",
            current.status == 200 && current.body["subject"] == "fixture-admin" && envelope_ok(&current.body),
        );
    }
    let other_rejected = other.status == 401 && error_code(&other.body) == "audience.rejected";
    let client_rejected = client_aud.status == 401 && error_code(&client_aud.body) == "audience.rejected";
    let binding_holds = other_rejected && client_aud.status == 302 && !configured_session;
    let client_id_aud_code = if client_aud.status == 302 { "ok".to_string() } else { error_code(&client_aud.body) };
    report.cases.push(json!({
        "name": "audience_client_id_binding",
        "other_aud_http": other.status,
        "other_aud_code": error_code(&other.body),
        "client_id_aud_http": client_aud.status,
        "client_id_aud_code": client_id_aud_code,
        "configured_aud_http": configured.status,
        "configured_session": configured_session,
        "binding_holds": binding_holds,
        "ok": other_rejected
    }));
    if !other_rejected {
        report.failed = true;
    }
    server.kill();
    drop_schema(pg).await;
    json!({
        "other_rejected": other_rejected,
        "client_id_rejected_when_audience_differs": client_rejected,
        "configured_audience_not_client_id_accepted": configured_session,
        "binding_holds": binding_holds
    })
}

async fn session_matrix(
    http: &mut Http,
    idp: &Idp,
    pg: &tokio_postgres::Client,
    store: &Store,
    config: &Config,
    env: &Env,
    config_path: &Path,
    report: &mut Report,
) {
    idp.set_subject("fixture-reviewer");
    idp.set_jwks(JwksMode::Normal);
    idp.set_mint(Mint::bound());
    let reviewer = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    report.check("reviewer_login_before_bootstrap", reviewer.status, "ok", reviewer.status == 302 && http.cookies_ok);
    let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    archive(env, "current-uninitialized.json", &current.body);
    let csrf = current.body["csrf_token"].as_str().unwrap_or("").to_string();
    report.check(
        "current_uninitialized",
        current.status,
        "ok",
        current.status == 200 && current.body["install_state"] == "uninitialized" && current.body["projects"].as_array().is_some_and(Vec::is_empty) && current_schema(&current.body),
    );
    let token_a = issue(env, config_path, &env.dir.join("boot-a"), report);
    let token_b = issue(env, config_path, &env.dir.join("boot-b"), report);
    let mismatch = post_bootstrap(http, &token_b, &csrf).await;
    report.check("bootstrap_subject_mismatch", mismatch.status, &error_code(&mismatch.body), mismatch.status == 403 && error_code(&mismatch.body) == "bootstrap.subject_mismatch");
    let state = token_state(pg, &token_b).await;
    report.check("bootstrap_mismatch_does_not_burn", 0, &state, state == "issued");
    let superseded = post_bootstrap(http, &token_a, &csrf).await;
    report.check("bootstrap_superseded", superseded.status, &error_code(&superseded.body), error_code(&superseded.body) == "bootstrap.rejected");
    pg.execute("UPDATE oo_identity_control.bootstrap_token SET expires_at = pg_catalog.now() - interval '1 second' WHERE state = 'issued'", &[]).await.expect("expire boot");
    idp.set_subject("fixture-admin");
    let admin = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    report.check("admin_login", admin.status, "ok", admin.status == 302);
    let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    let csrf = current.body["csrf_token"].as_str().unwrap_or("").to_string();
    let expired = post_bootstrap(http, &token_b, &csrf).await;
    report.check("bootstrap_expired", expired.status, &error_code(&expired.body), error_code(&expired.body) == "bootstrap.expired");
    let bare = http.call("POST", "/control/v1/auth/bootstrap/consume", Some(&format!("{{\"token\":\"{token_b}\"}}")), false, None).await;
    report.check("bootstrap_missing_csrf", bare.status, &error_code(&bare.body), error_code(&bare.body) == "csrf.rejected");
    let token_c = issue(env, config_path, &env.dir.join("boot-c"), report);
    let started = http.call("GET", &format!("/control/v1/auth/login?return_to=%2Fapp&bootstrap_token={token_c}"), None, false, None).await;
    report.check(
        "bootstrap_query_not_in_idp_redirect",
        started.status,
        "ok",
        started.status == 302 && !started.location.contains(&token_c) && !started.location.contains("bootstrap"),
    );
    let at_idp = http.get_external(&started.location).await;
    let done = http.call("GET", &at_idp.location, None, false, None).await;
    report.check(
        "bootstrap_query_consume",
        done.status,
        "ok",
        done.status == 302 && done.location.ends_with("/app") && !done.location.contains('?') && !done.location.contains(&token_c),
    );
    let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    archive(env, "current-admin.json", &current.body);
    report.check(
        "current_admin",
        current.status,
        "ok",
        current.status == 200 && current.body["install_state"] == "initialized" && current.body["projects"][0]["role"] == "admin" && current.body["projects"][0]["project_id"] == "alpha" && current_schema(&current.body),
    );
    let csrf = current.body["csrf_token"].as_str().unwrap_or("").to_string();
    let replay = post_bootstrap(http, &token_c, &csrf).await;
    archive(env, "bootstrap-replay.json", &replay.body);
    report.check("bootstrap_replay", replay.status, &error_code(&replay.body), replay.status == 409 && error_code(&replay.body) == "bootstrap.replay");
    let again = cli(&env.exe, &["bootstrap-issue", "--config", config_path.to_str().expect("path"), "--out", env.dir.join("boot-d").to_str().expect("out")]);
    report.check("bootstrap_unavailable", again.status.into(), &error_code(&again.body), again.status == 2 && error_code(&again.body) == "bootstrap.unavailable");
    let cookie = http.session.clone().expect("session");
    let session = store.authenticate(&cookie).await.expect("authenticate");
    let allowed = authorize(&session, config, "alpha", "project.admin", config.revision);
    let cross = authorize(&session, config, "beta", "project.admin", config.revision);
    let stale = authorize(&session, config, "alpha", "project.admin", config.revision + 1);
    report.check("project_and_revision_guard", 0, "ok", allowed.is_ok() && cross.err().map(|fault| fault.code) == Some("forbidden") && stale.err().map(|fault| fault.code) == Some("revision.conflict"));
    role_logins(http, idp, store, config, env, report).await;
    let missing = http.call("POST", "/control/v1/auth/logout", None, true, None).await;
    report.check("csrf_missing_does_not_revoke", missing.status, &error_code(&missing.body), error_code(&missing.body) == "csrf.rejected");
    let still = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    report.check("session_survives_bad_csrf", still.status, "ok", still.status == 200);
    let csrf = still.body["csrf_token"].as_str().unwrap_or("").to_string();
    let bad_origin = http.call_origin("POST", "/control/v1/auth/logout", None, "https://evil.example", Some(&csrf)).await;
    report.check("origin_rejected", bad_origin.status, &error_code(&bad_origin.body), error_code(&bad_origin.body) == "origin.rejected");
    let still = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    report.check("session_survives_bad_origin", still.status, "ok", still.status == 200);
    pg.execute("UPDATE oo_identity_control.session SET revision = revision + 1 WHERE revoked_at IS NULL", &[]).await.expect("bump");
    let conflict = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    archive(env, "revision-conflict.json", &conflict.body);
    report.check("revision_conflict", conflict.status, &error_code(&conflict.body), conflict.status == 409 && error_code(&conflict.body) == "revision.conflict" && envelope_ok(&conflict.body));
    pg.execute("UPDATE oo_identity_control.session SET revision = $1 WHERE revoked_at IS NULL", &[&config.revision]).await.expect("restore");
    let restored = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    report.check("current_after_revision_restore", restored.status, "ok", restored.status == 200 && restored.body["revision"] == config.revision);
    let csrf = restored.body["csrf_token"].as_str().unwrap_or("").to_string();
    let logout = http.call("POST", "/control/v1/auth/logout", None, true, Some(&csrf)).await;
    archive(env, "logout.json", &logout.body);
    report.check("logout", logout.status, &error_code(&logout.body), logout.status == 200 && logout.body["outcome"] == "revoked" && logout_schema(&logout.body) && http.session.is_none());
    let after = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    report.check("current_after_logout", after.status, &error_code(&after.body), after.status == 401 && error_code(&after.body) == "unauthenticated");
}

async fn role_logins(http: &mut Http, idp: &Idp, store: &Store, config: &Config, env: &Env, report: &mut Report) {
    let roles = [
        ("fixture-admin", "alpha", "admin", "project.admin"),
        ("fixture-reviewer", "beta", "reviewer", "review.decide"),
        ("fixture-steward", "gamma", "steward", "ontology.draft"),
        ("fixture-auditor", "delta", "auditor", "audit.read"),
        ("fixture-owner", "epsilon", "business_owner", "business.record"),
    ];
    for (subject, project, role, capability) in roles {
        idp.set_subject(subject);
        let mint = if role == "auditor" { Mint::claims() } else { Mint::bound() };
        idp.set_mint(mint);
        let reply = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
        let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
        let projects = &current.body["projects"];
        let role_ok = projects[0]["role"] == role && projects[0]["project_id"] == project;
        let caps = projects[0]["capabilities"].as_array().map(|items| items.iter().filter_map(|item| item.as_str()).collect::<Vec<_>>());
        let expected = capabilities_for(role).unwrap_or(&[]);
        let caps_ok = caps.as_ref().is_some_and(|got| got == expected);
        let text = current.body.to_string();
        let no_data = !text.contains("oo_r_") && current.body["data_grants"] == "none" && current.body["owner_certification"] == "not_issued";
        report.check(role, reply.status, capability, reply.status == 302 && current.status == 200 && role_ok && caps_ok && no_data && envelope_ok(&current.body));
        if role == "auditor" {
            archive(env, "current-auditor-role-claims.json", &current.body);
            let cookie = http.session.clone().expect("cookie");
            let session = store.authenticate(&cookie).await.expect("session");
            let upgrade = authorize(&session, config, project, "project.admin", config.revision);
            report.check("role_claims_do_not_grant_admin", 0, "forbidden", upgrade.err().map(|fault| fault.code) == Some("forbidden"));
        }
    }
    idp.set_subject("unbound-subject");
    idp.set_mint(Mint::claims());
    let reply = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    report.check(
        "unbound_role_claims",
        current.status,
        "none",
        reply.status == 302 && current.status == 200 && current.body["projects"].as_array().is_some_and(Vec::is_empty) && current.body["data_grants"] == "none" && current.body["agent_session"] == "not_issued" && current.body["harness_session"] == "not_issued",
    );
}

async fn negatives(http: &mut Http, idp: &Idp, pg: &tokio_postgres::Client, server: &mut Server, exe: &str, config: &Path, report: &mut Report) {
    idp.set_subject("fixture-admin");
    idp.set_jwks(JwksMode::Normal);
    tamper_pkce(http, idp, report).await;
    tamper_state(http, idp, report).await;
    oauth_replay(http, idp, report).await;
    let foreign = http.call("GET", "/control/v1/auth/login?redirect_uri=https://evil.example", None, false, None).await;
    report.check("foreign_redirect_query", foreign.status, &error_code(&foreign.body), error_code(&foreign.body) == "malformed");
    let offsite = http.call("GET", "/control/v1/auth/login?return_to=%2F%2Fevil", None, false, None).await;
    report.check("redirect_rejected", offsite.status, &error_code(&offsite.body), error_code(&offsite.body) == "redirect.rejected");
    expect_mode(http, idp, Mint::alg_none(), 401, "alg.rejected", "alg_none", report).await;
    expect_mode(http, idp, Mint::bad_sig(), 401, "token.rejected", "bad_signature", report).await;
    expect_mode(http, idp, Mint::iss("https://evil.example"), 401, "issuer.rejected", "issuer", report).await;
    expect_mode(http, idp, Mint::one("other-aud", Some(CLIENT.to_string())), 401, "audience.rejected", "bound_arbitrary_aud", report).await;
    expect_mode(http, idp, Mint::one(CLIENT, Some("other-client".to_string())), 401, "audience.rejected", "bad_azp", report).await;
    expect_mode(http, idp, Mint::multi(), 401, "audience.rejected", "multi_aud_without_azp", report).await;
    expect_mode(http, idp, Mint::one(CLIENT, None), 302, "ok", "single_aud_client_id_without_azp", report).await;
    expect_mode(http, idp, Mint::nonce("wrong-nonce-value"), 400, "nonce.rejected", "bad_nonce", report).await;
    expect_mode(http, idp, Mint::expired(), 401, "token.expired", "token_expired", report).await;
    expect_mode(http, idp, Mint::future_iat(), 401, "token.rejected", "future_iat", report).await;
    expect_mode(http, idp, Mint::idp_error(), 401, "token.rejected", "idp_error", report).await;
    restart(server, exe, config).await;
    idp.set_mint(Mint::bound());
    idp.set_jwks(JwksMode::Ambiguous);
    let before = idp.hits();
    let reply = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    report.check("jwks_ambiguous_one_fetch", reply.status, &error_code(&reply.body), error_code(&reply.body) == "kid.ambiguous" && idp.hits() - before == 1);
    restart(server, exe, config).await;
    idp.set_jwks(JwksMode::Unknown);
    idp.set_mint(Mint::unknown_kid());
    let before = idp.hits();
    let reply = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    report.check("jwks_unknown_kid_two_fetches", reply.status, &error_code(&reply.body), error_code(&reply.body) == "kid.unknown" && idp.hits() - before == 2);
    restart(server, exe, config).await;
    idp.set_jwks(JwksMode::Rotate);
    idp.set_mint(Mint::rotate());
    let before = idp.hits();
    let reply = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    report.check("jwks_rotation_bounded", reply.status, "ok", reply.status == 302 && idp.hits() - before == 2);
    idp.set_jwks(JwksMode::Normal);
    idp.set_mint(Mint::bound());
    let live = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    report.check("login_before_restart", live.status, "ok", live.status == 302);
    let saved = http.session.clone();
    restart(server, exe, config).await;
    http.session = saved;
    let survived = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    report.check("restart_session_survives", survived.status, "ok", survived.status == 200);
    let csrf = survived.body["csrf_token"].as_str().unwrap_or("").to_string();
    let logout = http.call("POST", "/control/v1/auth/logout", None, true, Some(&csrf)).await;
    report.check("logout_before_restart", logout.status, "ok", logout.status == 200);
    let saved = http.session.clone();
    restart(server, exe, config).await;
    http.session = saved;
    let revoked = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    report.check("restart_revocation_holds", revoked.status, &error_code(&revoked.body), revoked.status == 401 && error_code(&revoked.body) == "unauthenticated");
    idp.set_mint(Mint::bound());
    let again = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    report.check("login_after_revocation", again.status, "ok", again.status == 302);
    pg.execute("UPDATE oo_identity_control.session SET idle_expires_at = pg_catalog.now() - interval '1 second' WHERE revoked_at IS NULL", &[]).await.expect("idle");
    let idle = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    report.check("idle_expiry", idle.status, &error_code(&idle.body), idle.status == 401);
    let started = http.call("GET", "/control/v1/auth/login?return_to=%2Fapp", None, false, None).await;
    pg.execute("UPDATE oo_identity_control.oauth_transaction SET expires_at = pg_catalog.now() - interval '1 second' WHERE consumed_at IS NULL", &[]).await.expect("oauth");
    let at_idp = http.get_external(&started.location).await;
    let continued = http.call("GET", &at_idp.location, None, false, None).await;
    report.check("oauth_expiry", continued.status, &error_code(&continued.body), error_code(&continued.body) == "token.expired");
}

async fn caller_json(http: &mut Http, env: &Env, report: &mut Report) {
    http.session = None;
    http.login = None;
    let posted = http.call("POST", "/control/v1/identity/current", Some("{\"authorized\":true,\"role\":\"admin\"}"), true, None).await;
    archive(env, "caller-json-current.json", &posted.body);
    report.check(
        "caller_json_post_current",
        posted.status,
        &error_code(&posted.body),
        posted.status == 405 && error_code(&posted.body) == "method.not_allowed" && envelope_ok(&posted.body) && posted.body.get("projects").is_none() && http.session.is_none(),
    );
    let logout = http.call("POST", "/control/v1/auth/logout", Some("{\"authorized\":true}"), true, None).await;
    archive(env, "caller-json-logout.json", &logout.body);
    report.check(
        "caller_json_logout",
        logout.status,
        &error_code(&logout.body),
        logout.status == 400 && error_code(&logout.body) == "malformed" && envelope_ok(&logout.body) && logout.body.get("projects").is_none() && http.session.is_none(),
    );
    let library = VerifiedSession::try_from_caller_json(&json!({"authorized": true, "role": "admin", "data_grants": "oo_r_all"}));
    report.check("caller_json_library", 401, "unauthenticated", library.err().map(|fault| fault.code) == Some("unauthenticated"));
}

async fn truncate_note(pg: &tokio_postgres::Client, env: &Env, report: &mut Report) {
    let rows = pg
        .query(
            "SELECT pg_get_userbyid(c.relowner), COALESCE(pg_get_triggerdef(t.oid, true), '') \
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             LEFT JOIN pg_trigger t ON t.tgrelid = c.oid AND NOT t.tgisinternal \
             WHERE n.nspname = 'oo_identity_control' AND c.relname = 'audit_event'",
            &[],
        )
        .await
        .expect("trigger");
    let owner: String = rows.first().map(|row| row.get(0)).unwrap_or_default();
    let def: String = rows.first().map(|row| row.get(1)).unwrap_or_default();
    let note = format!(
        "audit_event owner={owner}\ntrigger={def}\nThe append-only trigger is BEFORE UPDATE OR DELETE. It does not cover TRUNCATE. The table owner can TRUNCATE. This review retains that note and does not run TRUNCATE or an unrelated database campaign. The service does not issue TRUNCATE.\n"
    );
    fs::write(&env.truncate_note, &note).expect("note");
    let retained = owner == "oo_identity_admin" && def.contains("UPDATE") && def.contains("DELETE") && !def.contains("TRUNCATE");
    report.check("table_owner_truncate_note", 0, "retained", retained);
}

async fn tamper_pkce(http: &mut Http, idp: &Idp, report: &mut Report) {
    idp.set_mint(Mint::bound());
    let started = http.call("GET", "/control/v1/auth/login?return_to=%2Fapp", None, false, None).await;
    let cookie = http.login.clone().unwrap_or_default();
    http.login = Some(tamper_verifier(&cookie));
    let at_idp = http.get_external(&started.location).await;
    let reply = http.call("GET", &at_idp.location, None, false, None).await;
    report.check("pkce_rejected", reply.status, &error_code(&reply.body), error_code(&reply.body) == "pkce.rejected");
}

async fn tamper_state(http: &mut Http, idp: &Idp, report: &mut Report) {
    idp.set_mint(Mint::bound());
    let started = http.call("GET", "/control/v1/auth/login?return_to=%2Fapp", None, false, None).await;
    let at_idp = http.get_external(&started.location).await;
    let location = replace_query(&at_idp.location, "state", "not-the-issued-state-value");
    let reply = http.call("GET", &location, None, false, None).await;
    report.check("state_rejected", reply.status, &error_code(&reply.body), error_code(&reply.body) == "state.rejected");
}

async fn oauth_replay(http: &mut Http, idp: &Idp, report: &mut Report) {
    idp.set_mint(Mint::bound());
    let started = http.call("GET", "/control/v1/auth/login?return_to=%2Fapp", None, false, None).await;
    let saved_login = http.login.clone();
    let at_idp = http.get_external(&started.location).await;
    let callback = at_idp.location.clone();
    let first = http.call("GET", &callback, None, false, None).await;
    http.session = None;
    http.login = saved_login;
    let second = http.call("GET", &callback, None, false, None).await;
    report.check("oauth_callback_replay", second.status, &error_code(&second.body), first.status == 302 && error_code(&second.body) == "state.rejected");
}

async fn expect_mode(http: &mut Http, idp: &Idp, mint: Mint, status: u16, code: &str, name: &str, report: &mut Report) {
    idp.set_mint(mint);
    let reply = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    let got = if reply.status == 302 { "ok".to_string() } else { error_code(&reply.body) };
    report.check(name, reply.status, &got, reply.status == status && got == code);
}

async fn login_follow(http: &mut Http, path: &str) -> Reply {
    let started = http.call("GET", path, None, false, None).await;
    if started.status != 302 {
        return started;
    }
    let at_idp = http.get_external(&started.location).await;
    if at_idp.status != 302 {
        return at_idp;
    }
    http.call("GET", &at_idp.location, None, false, None).await
}

async fn post_bootstrap(http: &mut Http, token: &str, csrf: &str) -> Reply {
    let body = format!("{{\"token\":\"{token}\"}}");
    http.call("POST", "/control/v1/auth/bootstrap/consume", Some(&body), true, Some(csrf)).await
}

async fn token_state(pg: &tokio_postgres::Client, token: &str) -> String {
    let hash = sha256_hex(token.as_bytes());
    let rows = pg.query("SELECT state FROM oo_identity_control.bootstrap_token WHERE token_hash = $1", &[&hash]).await.expect("state");
    rows.first().map(|row| row.get(0)).unwrap_or_default()
}

async fn drop_schema(pg: &tokio_postgres::Client) {
    pg.batch_execute("DROP SCHEMA IF EXISTS oo_identity_control CASCADE").await.expect("drop");
}

fn issue(env: &Env, config: &Path, out: &Path, report: &mut Report) -> String {
    let result = cli(&env.exe, &["bootstrap-issue", "--config", config.to_str().expect("path"), "--out", out.to_str().expect("out")]);
    let mode = fs::metadata(out).map(|meta| meta.permissions().mode() & 0o777).unwrap_or(0);
    let text = fs::read_to_string(out).unwrap_or_default();
    let token = text.trim_end_matches(['\n', '\r']).to_string();
    let stdout = serde_json::to_string(&result.body).unwrap_or_default();
    let ok = result.status == 0 && result.body["fixture_class"] == "explicit_test_fixture" && result.body["customer_owner"] == false && mode == 0o600 && token.len() == 43 && !stdout.contains(&token) && result.stderr.is_empty();
    report.check("bootstrap_issue", result.status.into(), "issued", ok);
    report.forbidden.push(token.clone());
    token
}

impl Report {
    fn check(&mut self, name: &str, http: u16, code: &str, ok: bool) {
        self.cases.push(json!({"name": name, "http": http, "code": code, "ok": ok}));
        if !ok {
            self.failed = true;
        }
    }
}

impl Mint {
    fn bound() -> Self {
        Self::one(CLIENT, Some(CLIENT.to_string()))
    }

    fn one(aud: &str, azp: Option<String>) -> Self {
        Self {
            aud: Aud::One(aud.to_string()),
            azp,
            iss: None,
            nonce: None,
            exp_offset: 300,
            iat_offset: 0,
            nbf_offset: 0,
            key_index: 0,
            kid_override: None,
            alg: "RS256".to_string(),
            bad_sig: false,
            extra_claims: false,
            idp_error: false,
        }
    }

    fn claims() -> Self {
        let mut mint = Self::bound();
        mint.extra_claims = true;
        mint
    }

    fn multi() -> Self {
        let mut mint = Self::bound();
        mint.aud = Aud::Multi(vec![CLIENT.to_string(), "extra".to_string()]);
        mint.azp = None;
        mint
    }

    fn iss(value: &str) -> Self {
        let mut mint = Self::bound();
        mint.iss = Some(value.to_string());
        mint
    }

    fn nonce(value: &str) -> Self {
        let mut mint = Self::bound();
        mint.nonce = Some(value.to_string());
        mint
    }

    fn expired() -> Self {
        let mut mint = Self::bound();
        mint.exp_offset = -120;
        mint.iat_offset = -400;
        mint.nbf_offset = -400;
        mint
    }

    fn future_iat() -> Self {
        let mut mint = Self::bound();
        mint.exp_offset = 4000;
        mint.iat_offset = 3600;
        mint.nbf_offset = 3600;
        mint
    }

    fn bad_sig() -> Self {
        let mut mint = Self::bound();
        mint.bad_sig = true;
        mint
    }

    fn alg_none() -> Self {
        let mut mint = Self::bound();
        mint.alg = "none".to_string();
        mint
    }

    fn idp_error() -> Self {
        let mut mint = Self::bound();
        mint.idp_error = true;
        mint
    }

    fn unknown_kid() -> Self {
        let mut mint = Self::bound();
        mint.kid_override = Some("k-missing".to_string());
        mint
    }

    fn rotate() -> Self {
        let mut mint = Self::bound();
        mint.key_index = 1;
        mint
    }
}

impl Idp {
    async fn start(listener: TcpListener, issuer: &str, secret: &str, material: &Material) -> Self {
        let keys = vec![signing_key("k1", &material.ders[0]), signing_key("k2", &material.ders[1])];
        let inner = Arc::new(Inner {
            issuer: issuer.to_string(),
            client_id: CLIENT.to_string(),
            client_secret: secret.to_string(),
            keys,
            rotate_floor: AtomicU64::new(0),
            codes: Mutex::new(HashMap::new()),
            mint: Mutex::new(Mint::bound()),
            subject: Mutex::new("fixture-admin".to_string()),
            jwks_mode: Mutex::new(JwksMode::Normal),
            jwks_hits: AtomicU64::new(0),
        });
        let state = Arc::clone(&inner);
        let tls = server_config(&material.cert, &material.key).expect("idp tls");
        let incoming = TlsListener { tcp: listener, acceptor: TlsAcceptor::from(Arc::new(tls)) };
        let router = axum::Router::new()
            .route("/.well-known/openid-configuration", axum::routing::get(discovery))
            .route("/authorize", axum::routing::get(idp_authorize))
            .route("/token", axum::routing::post(token))
            .route("/jwks", axum::routing::get(jwks))
            .with_state(state);
        tokio::spawn(async move {
            let _ = axum::serve(incoming, router).await;
        });
        Self { inner }
    }

    fn set_subject(&self, subject: &str) {
        *self.inner.subject.lock().expect("subject") = subject.to_string();
    }

    fn set_mint(&self, mint: Mint) {
        *self.inner.mint.lock().expect("mint") = mint;
    }

    fn set_jwks(&self, mode: JwksMode) {
        if mode == JwksMode::Rotate {
            self.inner.rotate_floor.store(self.hits(), Ordering::SeqCst);
        }
        *self.inner.jwks_mode.lock().expect("jwks") = mode;
    }

    fn hits(&self) -> u64 {
        self.inner.jwks_hits.load(Ordering::SeqCst)
    }
}

async fn discovery(axum::extract::State(state): axum::extract::State<Arc<Inner>>) -> axum::response::Response {
    json_response(json!({
        "issuer": state.issuer,
        "authorization_endpoint": format!("{}/authorize", state.issuer),
        "token_endpoint": format!("{}/token", state.issuer),
        "jwks_uri": format!("{}/jwks", state.issuer),
        "response_types_supported": ["code"],
        "id_token_signing_alg_values_supported": ["RS256"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["client_secret_post"]
    }))
}

async fn idp_authorize(axum::extract::State(state): axum::extract::State<Arc<Inner>>, request: axum::extract::Request) -> axum::response::Response {
    let query = request.uri().query().unwrap_or("");
    let map = form(query);
    let required = ["response_type", "client_id", "redirect_uri", "scope", "state", "nonce", "code_challenge", "code_challenge_method"];
    if required.iter().any(|key| !map.contains_key(*key)) || map["response_type"] != "code" || map["client_id"] != state.client_id || map["code_challenge_method"] != "S256" || map["scope"] != "openid" {
        return text(400);
    }
    if map["redirect_uri"] != format!("{}/control/v1/auth/callback", origin_of(&map["redirect_uri"])) && !map["redirect_uri"].ends_with("/control/v1/auth/callback") {
        return text(400);
    }
    let code = random_token();
    state.codes.lock().expect("codes").insert(code.clone(), Issued { challenge: map["code_challenge"].clone(), nonce: map["nonce"].clone(), used: false });
    let mint = state.mint.lock().expect("mint").clone();
    if mint.idp_error {
        return redirect(&format!("{}?error=access_denied&state={}", map["redirect_uri"], map["state"]));
    }
    redirect(&format!("{}?code={}&state={}", map["redirect_uri"], code, map["state"]))
}

async fn token(axum::extract::State(state): axum::extract::State<Arc<Inner>>, request: axum::extract::Request) -> axum::response::Response {
    let bytes = axum::body::to_bytes(request.into_body(), 65_536).await.unwrap_or_default();
    let Ok(form_text) = String::from_utf8(bytes.to_vec()) else {
        return text(400);
    };
    let fields = form(&form_text);
    let required = ["grant_type", "code", "redirect_uri", "client_id", "code_verifier", "client_secret"];
    if required.iter().any(|key| !fields.contains_key(*key)) || fields["grant_type"] != "authorization_code" || fields["client_id"] != state.client_id || fields["client_secret"] != state.client_secret {
        return text(400);
    }
    let mut codes = state.codes.lock().expect("codes");
    let Some(issued) = codes.get_mut(fields["code"].as_str()) else {
        return text(400);
    };
    if issued.used || sha256_b64(&fields["code_verifier"]) != issued.challenge {
        return text(400);
    }
    issued.used = true;
    let nonce = issued.nonce.clone();
    drop(codes);
    let mint = state.mint.lock().expect("mint").clone();
    let subject = state.subject.lock().expect("subject").clone();
    let id_token = mint_token(&state, &mint, &subject, &nonce);
    json_response(json!({"id_token": id_token, "token_type": "Bearer", "expires_in": 300}))
}

async fn jwks(axum::extract::State(state): axum::extract::State<Arc<Inner>>) -> axum::response::Response {
    let hit = state.jwks_hits.fetch_add(1, Ordering::SeqCst);
    let mode = *state.jwks_mode.lock().expect("mode");
    let keys = match mode {
        JwksMode::Ambiguous => vec![jwk(&state.keys[0], state.keys[0].kid), jwk(&state.keys[1], state.keys[0].kid)],
        JwksMode::Unknown => vec![jwk(&state.keys[0], state.keys[0].kid)],
        JwksMode::Rotate if hit == state.rotate_floor.load(Ordering::SeqCst) => vec![jwk(&state.keys[0], state.keys[0].kid)],
        JwksMode::Rotate => vec![jwk(&state.keys[0], state.keys[0].kid), jwk(&state.keys[1], state.keys[1].kid)],
        JwksMode::Normal => vec![jwk(&state.keys[0], state.keys[0].kid)],
    };
    json_response(json!({"keys": keys}))
}

fn mint_token(state: &Inner, mint: &Mint, subject: &str, nonce: &str) -> String {
    let now = unix_now();
    let aud = match &mint.aud {
        Aud::One(value) => json!(value),
        Aud::Multi(values) => json!(values),
    };
    let mut payload = json!({
        "iss": mint.iss.as_deref().unwrap_or(state.issuer.as_str()),
        "aud": aud,
        "exp": now + mint.exp_offset,
        "nbf": now + mint.nbf_offset,
        "iat": now + mint.iat_offset,
        "nonce": mint.nonce.as_deref().unwrap_or(nonce),
        "sub": subject
    });
    if let Some(azp) = &mint.azp {
        payload["azp"] = json!(azp);
    }
    if mint.extra_claims {
        payload["role"] = json!("admin");
        payload["roles"] = json!(["admin", "owner"]);
        payload["groups"] = json!(["agent", "harness"]);
        payload["data_grants"] = json!("oo_r_all");
        payload["authorized"] = json!(true);
    }
    let body = serde_json::to_string(&payload).expect("payload");
    let key = &state.keys[mint.key_index];
    let kid = mint.kid_override.as_deref().unwrap_or(key.kid);
    if mint.alg != "RS256" {
        let header = b64_str(&format!("{{\"alg\":\"{}\",\"kid\":\"{kid}\"}}", mint.alg));
        let encoded = b64_str(&body);
        return format!("{header}.{encoded}.x");
    }
    let header = b64_str(&format!("{{\"alg\":\"RS256\",\"kid\":\"{kid}\",\"typ\":\"JWT\"}}"));
    let encoded = b64_str(&body);
    let input = format!("{header}.{encoded}");
    let mut signature = vec![0_u8; key.pair.public().modulus_len()];
    key.pair.sign(&RSA_PKCS1_SHA256, &SystemRandom::new(), input.as_bytes(), &mut signature).expect("sign");
    let mut token = format!("{input}.{}", b64(&signature));
    if mint.bad_sig {
        let last = token.pop().unwrap_or('A');
        token.push(if last == 'A' { 'B' } else { 'A' });
    }
    token
}

fn jwk(key: &SigningKey, kid: &str) -> Value {
    json!({"kty": "RSA", "kid": kid, "use": "sig", "alg": "RS256", "n": key.n, "e": key.e})
}

fn signing_key(kid: &'static str, der: &[u8]) -> SigningKey {
    let pair = RsaKeyPair::from_pkcs8(der).expect("pair");
    let (n, e) = pkcs8_rsa_public(der).expect("public");
    SigningKey { kid, pair, n: b64(&n), e: b64(&e) }
}

fn json_response(value: Value) -> axum::response::Response {
    let bytes = serde_json::to_vec(&value).unwrap_or_default();
    axum::response::Response::builder().status(200).header("content-type", "application/json").body(axum::body::Body::from(bytes)).expect("response")
}

fn text(status: u16) -> axum::response::Response {
    axum::response::Response::builder().status(status).body(axum::body::Body::from("bad")).expect("response")
}

fn redirect(location: &str) -> axum::response::Response {
    axum::response::Response::builder().status(302).header("location", location).body(axum::body::Body::empty()).expect("response")
}

fn origin_of(redirect: &str) -> String {
    redirect.trim_end_matches("/control/v1/auth/callback").to_string()
}

impl Http {
    async fn new(origin: &str, ca: &[u8]) -> Self {
        let certs = reqwest::Certificate::from_pem_bundle(ca).expect("ca");
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .https_only(true)
            .http1_only()
            .referer(false)
            .connection_verbose(false)
            .tls_certs_only(certs)
            .timeout(Duration::from_secs(10))
            .build()
            .expect("client");
        Self { client, origin: origin.to_string(), login: None, session: None, cookies_ok: true }
    }

    async fn call(&mut self, method: &str, path: &str, body: Option<&str>, origin: bool, csrf: Option<&str>) -> Reply {
        let origin_value = self.origin.clone();
        self.call_origin(method, path, body, if origin { origin_value.as_str() } else { "" }, csrf).await
    }

    async fn call_origin(&mut self, method: &str, path: &str, body: Option<&str>, origin: &str, csrf: Option<&str>) -> Reply {
        let url = if path.starts_with("https://") { path.to_string() } else { format!("{}{path}", self.origin) };
        let mut request = self.client.request(method.parse().expect("method"), &url);
        if let Some(cookie) = self.cookie_header() {
            request = request.header("cookie", cookie);
        }
        if !origin.is_empty() {
            request = request.header("origin", origin);
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
            let Ok(text) = value.to_str() else {
                self.cookies_ok = false;
                continue;
            };
            if !host_cookie(text) {
                self.cookies_ok = false;
            }
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

fn host_cookie(header: &str) -> bool {
    let mut secure = false;
    let mut http_only = false;
    let mut same_site = false;
    let mut path = false;
    let mut domain = false;
    let mut host_name = false;
    if let Some((pair, _)) = header.split_once(';') {
        host_name = pair.starts_with("__Host-oo-login=") || pair.starts_with("__Host-oo-session=");
    }
    for part in header.split(';').skip(1) {
        match part.trim() {
            "Secure" => secure = true,
            "HttpOnly" => http_only = true,
            "SameSite=Lax" => same_site = true,
            "Path=/" => path = true,
            other if other.to_ascii_lowercase().starts_with("domain") => domain = true,
            _ => {}
        }
    }
    host_name && secure && http_only && same_site && path && !domain
}

async fn reply_from(response: reqwest::Response) -> Reply {
    let status = response.status().as_u16();
    let location = response.headers().get(reqwest::header::LOCATION).and_then(|value| value.to_str().ok()).unwrap_or("").to_string();
    let bytes = response.bytes().await.unwrap_or_default();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Reply { status, location, body }
}

fn error_code(body: &Value) -> String {
    if body.get("outcome").and_then(Value::as_str) == Some("consumed") || body.get("outcome").and_then(Value::as_str) == Some("revoked") {
        return "ok".to_string();
    }
    body.get("error").and_then(|error| error.get("code")).and_then(Value::as_str).unwrap_or("").to_string()
}

fn envelope_ok(value: &Value) -> bool {
    value.get("contract").and_then(Value::as_str) == Some(CONTRACT)
        && value.get("authority").and_then(Value::as_str) == Some("management_session")
        && value.get("data_grants").and_then(Value::as_str) == Some("none")
        && value.get("owner_certification").and_then(Value::as_str) == Some("not_issued")
        && value.get("agent_session").and_then(Value::as_str) == Some("not_issued")
        && value.get("harness_session").and_then(Value::as_str) == Some("not_issued")
        && value.get("gateway_token").and_then(Value::as_str) == Some("not_issued")
        && value.get("customer_integration").and_then(Value::as_str) == Some("not_claimed")
}

fn current_schema(value: &Value) -> bool {
    keys_exact(value, &["contract", "operation", "subject", "issuer", "config_commit", "revision", "install_state", "projects", "csrf_token", "session", "authority", "session_class", "data_grants", "owner_certification", "agent_session", "harness_session", "gateway_token", "customer_integration"])
        && value.get("operation").and_then(Value::as_str) == Some("getIdentityCurrent")
        && value.get("session_class").and_then(Value::as_str) == Some("management_cookie")
        && value.get("install_state").and_then(Value::as_str).is_some_and(|state| state == "uninitialized" || state == "initialized")
        && value.get("csrf_token").and_then(Value::as_str).is_some_and(token_shape)
        && value.get("config_commit").and_then(Value::as_str).is_some_and(|text| text.len() == 64 && text.chars().all(|ch| ch.is_ascii_hexdigit()))
        && envelope_ok(value)
        && session_schema(value.get("session"))
}

fn session_schema(value: Option<&Value>) -> bool {
    let Some(value) = value else { return false };
    keys_exact(value, &["idle_expires_at", "absolute_expires_at", "revoked"]) && value.get("revoked") == Some(&json!(false)) && value.get("idle_expires_at").and_then(Value::as_str).is_some_and(is_time) && value.get("absolute_expires_at").and_then(Value::as_str).is_some_and(is_time)
}

fn logout_schema(value: &Value) -> bool {
    keys_exact(value, &["contract", "operation", "outcome", "authority", "data_grants", "owner_certification", "agent_session", "harness_session", "gateway_token", "customer_integration"])
        && value.get("operation").and_then(Value::as_str) == Some("logoutManagementSession")
        && value.get("outcome").and_then(Value::as_str) == Some("revoked")
        && envelope_ok(value)
}

fn keys_exact(value: &Value, expected: &[&str]) -> bool {
    let Some(object) = value.as_object() else { return false };
    object.len() == expected.len() && expected.iter().all(|key| object.contains_key(*key))
}

fn token_shape(value: &str) -> bool {
    value.len() == 43 && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn is_time(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() > 20 && bytes[4] == b'-' && bytes[10] == b'T' && value.ends_with('Z')
}

fn archive(env: &Env, name: &str, body: &Value) {
    let mut copy = body.clone();
    if let Some(object) = copy.as_object_mut()
        && object.contains_key("csrf_token")
    {
        object.insert("csrf_token".to_string(), json!("REDACTED"));
    }
    fs::create_dir_all(&env.responses).expect("responses");
    fs::write(Path::new(&env.responses).join(name), serde_json::to_string_pretty(&copy).unwrap_or_default()).expect("archive");
}

fn tamper_verifier(cookie: &str) -> String {
    let mut raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(cookie).unwrap_or_else(|_| vec![0; 64]);
    if raw.is_empty() {
        return cookie.to_string();
    }
    let last = raw.len() - 1;
    raw[last] ^= 0x1f;
    b64(&raw)
}

fn replace_query(url: &str, key: &str, value: &str) -> String {
    let Some((base, query)) = url.split_once('?') else { return url.to_string() };
    let mut parts = Vec::new();
    for pair in query.split('&') {
        if let Some((got, _)) = pair.split_once('=')
            && got == key
        {
            parts.push(format!("{key}={value}"));
            continue;
        }
        parts.push(pair.to_string());
    }
    format!("{base}?{}", parts.join("&"))
}

fn form(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for pair in text.split('&') {
        let Some((key, value)) = pair.split_once('=') else { continue };
        out.insert(percent_decode(key), percent_decode(value));
    }
    out
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = hex(bytes[index + 1]);
            let low = hex(bytes[index + 2]);
            out.push((high << 4) | low);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

fn hex(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => 0,
    }
}

fn sha256_b64(value: &str) -> String {
    b64(ring::digest::digest(&ring::digest::SHA256, value.as_bytes()).as_ref())
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn b64_str(value: &str) -> String {
    b64(value.as_bytes())
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    SystemRandom::new().fill(&mut bytes).expect("random");
    b64(&bytes)
}

fn unix_now() -> i64 {
    i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)).unwrap_or(0)
}

impl Server {
    async fn spawn(exe: &str, config: &Path) -> Self {
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
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let stderr_buffer = Arc::clone(&buffer);
        let stderr_thread = std::thread::spawn(move || {
            let mut stderr = stderr;
            let mut local = Vec::new();
            let _ = stderr.read_to_end(&mut local);
            *stderr_buffer.lock().expect("stderr") = local;
        });
        let line = tokio::task::spawn_blocking(move || receiver.recv_timeout(Duration::from_secs(30))).await.expect("join").expect("listening");
        let text = String::from_utf8(line).expect("utf8");
        let value: Value = serde_json::from_str(text.trim()).expect("listen json");
        assert_eq!(value["outcome"], "listening");
        Self { child: Some(child), stdout_thread: Some(stdout_thread), stderr_thread: Some(stderr_thread), stderr: buffer }
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

    fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr.lock().expect("stderr")).to_string()
    }
}

async fn restart(server: &mut Server, exe: &str, config: &Path) {
    server.kill();
    *server = Server::spawn(exe, config).await;
}

impl Drop for Server {
    fn drop(&mut self) {
        self.kill();
    }
}

struct Cli {
    status: u8,
    body: Value,
    stderr: Vec<u8>,
}

fn cli(exe: &str, args: &[&str]) -> Cli {
    let output = Command::new(exe).args(args).env_clear().env("HOME", "/tmp").env("PATH", "/usr/bin:/bin").output().expect("cli");
    let status = output.status.code().unwrap_or(1) as u8;
    let body = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    Cli { status, body, stderr: output.stderr }
}

fn write_config(path: &Path, env: &Env, material: &Material, origin: &str, issuer: &str, callback: &str, port: u16, audience: &str) {
    let bindings = [
        Binding { subject: "fixture-admin".into(), project_id: "alpha".into(), role: Role::Admin },
        Binding { subject: "fixture-reviewer".into(), project_id: "beta".into(), role: Role::Reviewer },
        Binding { subject: "fixture-steward".into(), project_id: "gamma".into(), role: Role::Steward },
        Binding { subject: "fixture-auditor".into(), project_id: "delta".into(), role: Role::Auditor },
        Binding { subject: "fixture-owner".into(), project_id: "epsilon".into(), role: Role::BusinessOwner },
    ];
    let redirects = vec![callback.to_string()];
    let commit = canonical_commit(issuer, audience, CLIENT, &redirects, &bindings, &bindings[0], 7).expect("commit");
    let body = json!({
        "https_origin": origin,
        "listen_host": "127.0.0.1",
        "listen_port": port,
        "tls_cert_file": env.dir.join("server.crt").to_string_lossy(),
        "tls_key_file": env.dir.join("server.pkcs8.pem").to_string_lossy(),
        "issuer": issuer,
        "audience": audience,
        "client_id": CLIENT,
        "client_secret_file": env.dir.join("client.secret").to_string_lossy(),
        "redirect_uris": [callback],
        "config_commit": commit,
        "revision": 7,
        "idp_ca_file": env.dir.join("ca.crt").to_string_lossy(),
        "pg": {
            "host": env.pg_host,
            "port": env.pg_port,
            "database": env.pg_database,
            "user": "oo_identity_admin",
            "password_file": env.pg_password_file,
            "ca_file": env.pg_ca
        },
        "bindings": bindings.iter().map(|item| json!({"subject": item.subject, "project_id": item.project_id, "role": item.role.as_str()})).collect::<Vec<_>>(),
        "bootstrap": {"subject": "fixture-admin", "project_id": "alpha", "role": "admin"},
        "idle_seconds": 600,
        "absolute_seconds": 3600,
        "oauth_ttl_seconds": 300,
        "bootstrap_ttl_seconds": 300,
        "clock_skew_seconds": 30
    });
    fs::write(path, serde_json::to_vec_pretty(&body).expect("json")).expect("config");
    let _ = material;
}

fn certs(dir: &Path) -> Material {
    openssl(dir, &["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", "ca.key", "-out", "ca.crt", "-days", "7", "-subj", "/CN=oo-a375-oracle"]);
    openssl(dir, &["req", "-newkey", "rsa:2048", "-nodes", "-keyout", "server.key", "-out", "server.csr", "-subj", "/CN=127.0.0.1"]);
    fs::write(dir.join("san.cnf"), "basicConstraints=CA:FALSE\nsubjectAltName=IP:127.0.0.1\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n").expect("san");
    openssl(dir, &["x509", "-req", "-in", "server.csr", "-CA", "ca.crt", "-CAkey", "ca.key", "-CAcreateserial", "-out", "server.crt", "-days", "7", "-extfile", "san.cnf"]);
    openssl(dir, &["pkcs8", "-topk8", "-nocrypt", "-in", "server.key", "-out", "server.pkcs8.pem"]);
    openssl(dir, &["genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", "k1.pem"]);
    openssl(dir, &["pkcs8", "-topk8", "-nocrypt", "-in", "k1.pem", "-outform", "DER", "-out", "k1.der"]);
    openssl(dir, &["genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", "k2.pem"]);
    openssl(dir, &["pkcs8", "-topk8", "-nocrypt", "-in", "k2.pem", "-outform", "DER", "-out", "k2.der"]);
    Material {
        ca: fs::read(dir.join("ca.crt")).expect("ca"),
        cert: fs::read(dir.join("server.crt")).expect("cert"),
        key: fs::read(dir.join("server.pkcs8.pem")).expect("key"),
        ders: vec![fs::read(dir.join("k1.der")).expect("k1"), fs::read(dir.join("k2.der")).expect("k2")],
    }
}

fn openssl(dir: &Path, args: &[&str]) {
    let status = Command::new("openssl").args(args).current_dir(dir).status().expect("openssl");
    assert!(status.success(), "openssl");
}

fn openssl_hex(dir: &Path) -> String {
    let output = Command::new("openssl").args(["rand", "-hex", "32"]).current_dir(dir).output().expect("rand");
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
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    port
}

async fn pg_client(env: &Env, password: &str) -> tokio_postgres::Client {
    let mut config = tokio_postgres::Config::new();
    config.user("oo_identity_admin");
    config.password(password);
    config.host(&env.pg_host);
    config.port(env.pg_port);
    config.dbname(&env.pg_database);
    config.ssl_mode(SslMode::Require);
    let ca = fs::read(&env.pg_ca).expect("pg ca");
    let tls = pg_tls(&ca).expect("pg tls");
    let (client, connection) = config.connect(tls).await.expect("pg");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn wait_discovery(ca: &[u8], issuer: &str) {
    let certs = reqwest::Certificate::from_pem_bundle(ca).expect("ca");
    let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).https_only(true).tls_certs_only(certs).timeout(Duration::from_secs(2)).build().expect("client");
    let url = format!("{issuer}/.well-known/openid-configuration");
    for _ in 0..50 {
        if client.get(&url).send().await.ok().is_some_and(|response| response.status().is_success()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("test issuer did not become ready");
}

fn scan_secrets(dir: &str, secrets: &[String]) -> bool {
    let mut found = false;
    let mut stack = vec![PathBuf::from(dir)];
    while let Some(path) = stack.pop() {
        let Ok(entries) = fs::read_dir(&path) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(bytes) = fs::read(&path) else { continue };
            let text = String::from_utf8_lossy(&bytes);
            if secrets.iter().any(|secret| !secret.is_empty() && secret.len() >= 32 && text.contains(secret)) {
                found = true;
            }
        }
    }
    found
}
