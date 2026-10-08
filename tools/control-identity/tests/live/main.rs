#![forbid(unsafe_code)]

mod idp;

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use base64::Engine;

use serde_json::{json, Value};
use tokio_postgres::config::SslMode;

use control_identity::{
    authorize, canonical_commit, capabilities_for, load_config, migration_sha256, pg_tls, Binding, Config, Role, Store, VerifiedSession,
};
use idp::{Idp, IdpParts, Mode};

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
        let text = serde_json::json!({
            "contract": "oo.control-identity/v1",
            "customer_integration": "not_claimed",
            "data_grants": "none",
            "cases": self.cases,
        })
        .to_string();
        let leaked = self.forbidden.iter().any(|secret| !secret.is_empty() && text.contains(secret));
        let body = if leaked {
            r#"{"contract":"oo.control-identity/v1","customer_integration":"not_claimed","proof":"redacted"}"#.to_string()
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
    stderr: Arc<Mutex<Vec<u8>>>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<()>>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_identity_boundaries() {
    let env = live_env();
    let mut proof = Proof { path: env.proof.clone(), cases: Vec::new(), forbidden: Vec::new() };
    let password = read_secret(&env.password_file);
    proof.forbidden.push(password.clone());
    let dir = std::env::temp_dir().join(format!("oo-a370-live-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    let material = certs(&dir);
    let secret = openssl_hex();
    proof.forbidden.push(secret.clone());
    write_secret(&dir.join("client.secret"), &secret);
    let product_port = free_port();
    let idp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("idp bind");
    let idp_port = idp_listener.local_addr().expect("idp addr").port();
    let origin = format!("https://127.0.0.1:{product_port}");
    let issuer = format!("https://127.0.0.1:{idp_port}");
    let callback = format!("{origin}/control/v1/auth/callback");
    let config_path = dir.join("identity.json");
    write_config(
        &config_path,
        &Endpoints { env: &env, dir: &dir, material: &material, origin: &origin, issuer: &issuer, callback: &callback, port: product_port },
    );
    let config = load_config(&config_path).expect("config");
    let der_keys = vec![fs::read(dir.join("k1.der")).expect("k1"), fs::read(dir.join("k2.der")).expect("k2")];
    let idp = idp::start(
        idp_listener,
        IdpParts {
            cert_pem: &material.cert,
            key_pem: &material.key,
            issuer: &issuer,
            client_id: "oo-control-client",
            client_secret: &secret,
            redirect_uri: &callback,
            der_keys: &der_keys,
        },
    )
    .await;
    let pg = pg_client(&env, &password).await;
    let exe = env!("CARGO_BIN_EXE_control-identity").to_string();
    let install = cli(&exe, &["install", "--config", config_path.to_str().expect("path")]);
    assert_eq!(install.status, 0, "install exit");
    assert_eq!(install.body["disposition"], "committed");
    assert_eq!(install.body["sql_sha256"], migration_sha256());
    proof.record("install", 0, "committed");
    let replay = cli(&exe, &["install", "--config", config_path.to_str().expect("path")]);
    assert_eq!(replay.body["disposition"], "replayed");
    proof.record("install_replay", 0, "replayed");
    let token = issue_token(&exe, &config_path, &dir.join("bootstrap.token"), &mut proof);
    let mut http = Http::new(&origin, &material.ca);
    let mut server = ServerProc::spawn(&exe, &config_path);
    phase_positive(&mut http, &idp, &token, &mut proof, &mut server, &exe, &config_path).await;
    phase_roles(&mut http, &idp, &config, &mut proof).await;
    phase_negatives(&mut http, &idp, &pg, &mut proof, &mut server, &exe, &config_path).await;
    server.kill();
    pg.batch_execute("DROP SCHEMA IF EXISTS oo_identity_control CASCADE").await.expect("drop");
    let reinstall = cli(&exe, &["install", "--config", config_path.to_str().expect("path")]);
    assert_eq!(reinstall.body["disposition"], "committed");
    let mut server = ServerProc::spawn(&exe, &config_path);
    phase_bootstrap_post(&mut http, &idp, &exe, &config_path, &dir, &pg, &mut proof).await;
    let stderr = server.stderr_text();
    server.kill();
    assert!(!proof.forbidden.iter().any(|secret| !secret.is_empty() && stderr.contains(secret)), "stderr contained a secret");
    proof.record("complete", 0, "ok");
    let _ = fs::remove_dir_all(&dir);
}

async fn phase_positive(http: &mut Http, idp: &Idp, token: &str, proof: &mut Proof, server: &mut ServerProc, exe: &str, config: &Path) {
    let missing = http.call("GET", "/control/v1/no-such-route", None, false, None).await;
    assert_eq!(missing.status, 404);
    assert!(!missing.body.to_string().contains("no-such-route"));
    proof.record("not_found", missing.status, error_code(&missing.body));
    let wrong = http.call("POST", "/control/v1/auth/login", None, false, None).await;
    assert_eq!(wrong.status, 405);
    proof.record("method", wrong.status, error_code(&wrong.body));
    let bad_return = http.call("GET", "/control/v1/auth/login?return_to=//evil", None, false, None).await;
    assert_eq!(bad_return.status, 400);
    assert_eq!(error_code(&bad_return.body), "redirect.rejected");
    proof.record("redirect", bad_return.status, "redirect.rejected");
    let callback = login_follow(http, &format!("/control/v1/auth/login?return_to=%2Fapp&bootstrap_token={token}")).await;
    assert_eq!(callback.status, 302, "bootstrap callback");
    assert!(callback.location.ends_with("/app") && !callback.location.contains('?') && !callback.location.contains("bootstrap"), "redirect stripped");
    assert!(http.login.is_none() && http.session.is_some(), "session cookie replaced the login cookie");
    proof.record("bootstrap_login", callback.status, "redirect");
    let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(current.status, 200);
    assert_eq!(current.body["install_state"], "initialized");
    assert_eq!(current.body["authority"], "management_session");
    assert_eq!(current.body["data_grants"], "none");
    assert_eq!(current.body["customer_integration"], "not_claimed");
    assert_eq!(current.body["projects"][0]["role"], "admin");
    assert_eq!(current.body["projects"][0]["capabilities"], json!(capabilities_for("admin").expect("admin")));
    let csrf = current.body["csrf_token"].as_str().expect("csrf").to_string();
    assert_eq!(csrf.len(), 43);
    proof.record("current", 200, "ok");
    let bad_csrf = http.call("POST", "/control/v1/auth/logout", None, true, Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")).await;
    assert_eq!(bad_csrf.status, 403);
    assert_eq!(error_code(&bad_csrf.body), "csrf.rejected");
    proof.record("csrf", bad_csrf.status, "csrf.rejected");
    let bad_origin = http.call("POST", "/control/v1/auth/logout", None, false, Some(&csrf)).await;
    assert_eq!(bad_origin.status, 403);
    assert_eq!(error_code(&bad_origin.body), "origin.rejected");
    proof.record("origin", bad_origin.status, "origin.rejected");
    let still = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(still.status, 200);
    let csrf = still.body["csrf_token"].as_str().expect("csrf").to_string();
    let logout = http.call("POST", "/control/v1/auth/logout", None, true, Some(&csrf)).await;
    assert_eq!(logout.status, 200);
    assert_eq!(logout.body["outcome"], "revoked");
    proof.record("logout", 200, "revoked");
    let gone = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(gone.status, 401);
    proof.record("logged_out", 401, error_code(&gone.body));
    restart(server, exe, config);
    let still_gone = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(still_gone.status, 401);
    proof.record("revocation_survives_restart", 401, error_code(&still_gone.body));
    idp.set_subject("fixture-admin");
    let again = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    assert_eq!(again.status, 302, "second login");
    restart(server, exe, config);
    let kept = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(kept.status, 200);
    proof.record("session_survives_restart", 200, "ok");
}

async fn phase_roles(http: &mut Http, idp: &Idp, config: &Config, proof: &mut Proof) {
    let checks = [
        ("fixture-admin", "alpha", "admin", "project.admin"),
        ("fixture-reviewer", "beta", "reviewer", "review.decide"),
        ("fixture-steward", "gamma", "steward", "ontology.draft"),
        ("fixture-auditor", "delta", "auditor", "audit.read"),
        ("fixture-owner", "epsilon", "business_owner", "business.record"),
    ];
    for (subject, project, role, capability) in checks {
        idp.set_subject(subject);
        idp.set_mode(Mode::Ok);
        let callback = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
        assert_eq!(callback.status, 302, "role login");
        let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
        assert_eq!(current.status, 200);
        assert_eq!(current.body["projects"][0]["project_id"], project);
        assert_eq!(current.body["projects"][0]["role"], role);
        assert_eq!(current.body["projects"][0]["capabilities"], json!(capabilities_for(role).expect("role")));
        let store = Store::connect(config).await.expect("store");
        let session_cookie = http.session.clone().expect("session");
        let session = store.authenticate(&session_cookie).await.expect("session");
        authorize(&session, config, project, capability, config.revision).expect("allow");
        assert_eq!(authorize(&session, config, "other", capability, config.revision).expect_err("project").code, "forbidden");
        let review = authorize(&session, config, project, "review.decide", config.revision);
        if capability == "review.decide" {
            review.expect("reviewer can decide");
        } else {
            assert_eq!(review.expect_err("role cannot decide").code, "forbidden");
        }
        assert_eq!(authorize(&session, config, project, capability, config.revision + 1).expect_err("revision").code, "revision.conflict");
        proof.record(role, 200, capability);
    }
    assert!(VerifiedSession::try_from_caller_json(&json!({"authorized": true})).is_err());
    proof.record("caller_json", 401, "unauthenticated");
}

async fn phase_negatives(http: &mut Http, idp: &Idp, pg: &tokio_postgres::Client, proof: &mut Proof, server: &mut ServerProc, exe: &str, config: &Path) {
    idp.set_subject("fixture-admin");
    idp.set_mode(Mode::Ok);
    let foreign = http.call("GET", "/control/v1/auth/login?redirect_uri=https://evil.example", None, false, None).await;
    assert_eq!(error_code(&foreign.body), "malformed");
    proof.record("foreign_redirect", foreign.status, "malformed");
    tamper_pkce(http, idp, proof).await;
    tamper_state(http, idp, proof).await;
    for (mode, status, code) in [
        (Mode::AlgNone, 401, "alg.rejected"),
        (Mode::Hs256, 401, "alg.rejected"),
        (Mode::WrongIssuer, 401, "issuer.rejected"),
        (Mode::WrongAudience, 401, "audience.rejected"),
        (Mode::WrongAzp, 401, "audience.rejected"),
        (Mode::MultiAud, 401, "audience.rejected"),
        (Mode::BadNonce, 400, "nonce.rejected"),
        (Mode::Expired, 401, "token.expired"),
        (Mode::FutureIat, 401, "token.rejected"),
        (Mode::IdpError, 401, "token.rejected"),
    ] {
        idp.set_mode(mode);
        let reply = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
        assert_eq!(reply.status, status, "{code}");
        assert_eq!(error_code(&reply.body), code);
        proof.record(code, reply.status, code);
    }
    restart(server, exe, config);
    idp.set_mode(Mode::Ambiguous);
    let before = idp.jwks_hits();
    let reply = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    assert_eq!(error_code(&reply.body), "kid.ambiguous");
    assert_eq!(idp.jwks_hits() - before, 1);
    proof.record("kid.ambiguous", reply.status, "kid.ambiguous");
    restart(server, exe, config);
    idp.set_mode(Mode::UnknownKid);
    let before = idp.jwks_hits();
    let reply = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    assert_eq!(error_code(&reply.body), "kid.unknown");
    assert_eq!(idp.jwks_hits() - before, 2);
    proof.record("kid.unknown", reply.status, "kid.unknown");
    restart(server, exe, config);
    idp.set_mode(Mode::RotateKid);
    idp.arm_rotation();
    let before = idp.jwks_hits();
    let reply = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    assert_eq!(reply.status, 302, "rotated kid");
    assert_eq!(idp.jwks_hits() - before, 2);
    proof.record("kid.rotation", reply.status, "ok");
    idp.set_mode(Mode::Ok);
    let _ = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    pg.execute("UPDATE oo_identity_control.session SET idle_expires_at = pg_catalog.now() - interval '1 second' WHERE revoked_at IS NULL", &[]).await.expect("expire session");
    let expired = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(expired.status, 401);
    proof.record("session_expired", expired.status, error_code(&expired.body));
    let started = http.call("GET", "/control/v1/auth/login?return_to=%2Fapp", None, false, None).await;
    assert_eq!(started.status, 302, "oauth start");
    pg.execute("UPDATE oo_identity_control.oauth_transaction SET expires_at = pg_catalog.now() - interval '1 second' WHERE consumed_at IS NULL", &[]).await.expect("expire oauth");
    let continued = continue_login(http, &started.location).await;
    assert_eq!(error_code(&continued.body), "token.expired");
    proof.record("oauth_expired", continued.status, "token.expired");
}

async fn phase_bootstrap_post(http: &mut Http, idp: &Idp, exe: &str, config: &Path, dir: &Path, pg: &tokio_postgres::Client, proof: &mut Proof) {
    let first = issue_token(exe, config, &dir.join("boot-a.token"), proof);
    let hash_rows = pg.query("SELECT state FROM oo_identity_control.bootstrap_token", &[]).await.expect("rows");
    assert_eq!(hash_rows.len(), 1);
    idp.set_mode(Mode::Ok);
    idp.set_subject("fixture-reviewer");
    let reviewer = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    assert_eq!(reviewer.status, 302, "reviewer login");
    let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(current.body["install_state"], "uninitialized");
    assert_eq!(current.body["projects"].as_array().expect("projects").len(), 0);
    let csrf = current.body["csrf_token"].as_str().expect("csrf").to_string();
    let mismatch = post_token(http, &first, &csrf).await;
    assert_eq!(mismatch.status, 403);
    assert_eq!(error_code(&mismatch.body), "bootstrap.subject_mismatch");
    let states = pg.query("SELECT state FROM oo_identity_control.bootstrap_token", &[]).await.expect("states");
    assert_eq!(states[0].get::<_, String>(0), "issued");
    proof.record("bootstrap.subject_mismatch", mismatch.status, "bootstrap.subject_mismatch");
    let second = issue_token(exe, config, &dir.join("boot-b.token"), proof);
    let rejected = post_token(http, &first, &csrf).await;
    assert_eq!(error_code(&rejected.body), "bootstrap.rejected");
    proof.record("bootstrap.superseded", rejected.status, "bootstrap.rejected");
    idp.set_subject("fixture-admin");
    let admin = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    assert_eq!(admin.status, 302, "admin login");
    let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    let csrf = current.body["csrf_token"].as_str().expect("csrf").to_string();
    let left = post_token(http, &second, &csrf);
    let right = post_token(http, &second, &csrf);
    let (left, right) = tokio::join!(left, right);
    let mut codes = [error_code(&left.body), error_code(&right.body)];
    codes.sort();
    assert_eq!(codes, ["bootstrap.replay", "ok"]);
    assert!(left.status == 200 || right.status == 200);
    proof.record("bootstrap.replay", 409, "bootstrap.replay");
    let third = post_token(http, &second, &csrf).await;
    assert_eq!(error_code(&third.body), "bootstrap.replay");
    let again = issue_token_status(exe, config, &dir.join("boot-c.token"));
    assert_eq!(again.status, 2);
    assert_eq!(error_code(&again.body), "bootstrap.unavailable");
    proof.record("bootstrap.unavailable", 403, "bootstrap.unavailable");
    pg.execute("UPDATE oo_identity_control.session SET revision = revision + 1 WHERE revoked_at IS NULL", &[]).await.expect("revision");
    let conflict = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(conflict.status, 409);
    assert_eq!(error_code(&conflict.body), "revision.conflict");
    proof.record("revision.conflict", conflict.status, "revision.conflict");
}

async fn tamper_pkce(http: &mut Http, idp: &Idp, proof: &mut Proof) {
    idp.set_mode(Mode::Ok);
    let before = idp.token_hits();
    let started = http.call("GET", "/control/v1/auth/login?return_to=%2Fapp", None, false, None).await;
    assert_eq!(started.status, 302, "pkce start");
    flip_login_verifier(http);
    let reply = continue_login(http, &started.location).await;
    assert_eq!(error_code(&reply.body), "pkce.rejected");
    assert_eq!(idp.token_hits(), before);
    proof.record("pkce.rejected", reply.status, "pkce.rejected");
}

async fn tamper_state(http: &mut Http, idp: &Idp, proof: &mut Proof) {
    idp.set_mode(Mode::Ok);
    let started = http.call("GET", "/control/v1/auth/login?return_to=%2Fapp", None, false, None).await;
    let idp_reply = http.get_external(&started.location).await;
    assert_eq!(idp_reply.status, 302, "state idp");
    let mut path = idp_reply.location.trim_start_matches(&http.origin).to_string();
    if let Some(index) = path.rfind('=') {
        let byte = path.pop().unwrap_or('x');
        path.push(if byte == 'A' { 'B' } else { 'A' });
        let _ = index;
    }
    let reply = http.call("GET", &path, None, false, None).await;
    assert_eq!(error_code(&reply.body), "state.rejected");
    proof.record("state.rejected", reply.status, "state.rejected");
}

async fn login_follow(http: &mut Http, path: &str) -> Reply {
    let started = http.call("GET", path, None, false, None).await;
    if started.status != 302 {
        return started;
    }
    assert_login_location(&started.location);
    continue_login(http, &started.location).await
}

async fn continue_login(http: &mut Http, location: &str) -> Reply {
    let idp_reply = http.get_external(location).await;
    if idp_reply.status != 302 {
        return idp_reply;
    }
    let path = idp_reply.location.trim_start_matches(&http.origin).to_string();
    http.call("GET", &path, None, false, None).await
}

async fn post_token(http: &Http, token: &str, csrf: &str) -> Reply {
    let mut clone = http.clone_jar();
    let body = json!({ "token": token }).to_string();
    clone.call("POST", "/control/v1/auth/bootstrap/consume", Some(&body), true, Some(csrf)).await
}

fn flip_login_verifier(http: &mut Http) {
    let Some(value) = http.login.clone() else { panic!("missing login cookie"); };
    let mut bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&value).expect("cookie");
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    http.login = Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes));
}

fn assert_login_location(location: &str) {
    let ok = location.contains("response_type=code")
        && location.contains("code_challenge_method=S256")
        && location.contains("scope=openid")
        && !location.contains("bootstrap")
        && !location.contains("client_secret")
        && !location.contains("code_verifier");
    assert!(ok, "authorization redirect was rejected");
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
            .timeout(Duration::from_secs(10))
            .build()
            .expect("client");
        Self { client, origin: origin.to_string(), login: None, session: None }
    }

    fn clone_jar(&self) -> Self {
        Self { client: self.client.clone(), origin: self.origin.clone(), login: self.login.clone(), session: self.session.clone() }
    }

    async fn call(&mut self, method: &str, path: &str, body: Option<&str>, origin: bool, csrf: Option<&str>) -> Reply {
        let url = if path.starts_with("https://") { path.to_string() } else { format!("{}{path}", self.origin) };
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
            assert!(host_cookie(text), "cookie attributes");
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
    secure && http_only && same_site && path && !domain
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

fn restart(server: &mut ServerProc, exe: &str, config: &Path) {
    server.kill();
    *server = ServerProc::spawn(exe, config);
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
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let stderr_buffer = Arc::clone(&buffer);
        let stderr_thread = std::thread::spawn(move || {
            let mut stderr = stderr;
            let mut local = Vec::new();
            let _ = stderr.read_to_end(&mut local);
            *stderr_buffer.lock().expect("stderr") = local;
        });
        let line = receiver.recv_timeout(Duration::from_secs(30)).expect("listening");
        let text = String::from_utf8(line).expect("utf8");
        let value: Value = serde_json::from_str(text.trim()).expect("listen json");
        assert_eq!(value["outcome"], "listening");
        Self { child: Some(child), stderr: buffer, stdout_thread: Some(stdout_thread), stderr_thread: Some(stderr_thread) }
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
    assert!(output.stderr.is_empty(), "cli stderr");
    let status = output.status.code().unwrap_or(1) as u8;
    let body = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    Cli { status, body }
}

fn issue_token(exe: &str, config: &Path, out: &Path, proof: &mut Proof) -> String {
    let result = issue_token_status(exe, config, out);
    assert_eq!(result.status, 0, "issue");
    assert_eq!(result.body["fixture_class"], "explicit_test_fixture");
    assert_eq!(result.body["customer_owner"], false);
    let mode = fs::metadata(out).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let text = fs::read_to_string(out).expect("token file");
    assert!(text.ends_with('\n'));
    let token = text.trim_end_matches(['\n', '\r']).to_string();
    assert_eq!(token.len(), 43);
    assert!(!String::from_utf8_lossy(&serde_json::to_vec(&result.body).unwrap_or_default()).contains(&token), "issue stdout contained the token");
    proof.forbidden.push(token.clone());
    proof.record("bootstrap_issue", 0, "issued");
    token
}

fn issue_token_status(exe: &str, config: &Path, out: &Path) -> Cli {
    cli(exe, &["bootstrap-issue", "--config", config.to_str().expect("path"), "--out", out.to_str().expect("out")])
}

struct Endpoints<'a> {
    env: &'a LiveEnv,
    dir: &'a Path,
    material: &'a Material,
    origin: &'a str,
    issuer: &'a str,
    callback: &'a str,
    port: u16,
}

fn write_config(path: &Path, endpoints: &Endpoints<'_>) {
    let Endpoints { env, dir, material, origin, issuer, callback, port } = endpoints;
    let bindings = vec![
        Binding { subject: "fixture-admin".into(), project_id: "alpha".into(), role: Role::Admin },
        Binding { subject: "fixture-reviewer".into(), project_id: "beta".into(), role: Role::Reviewer },
        Binding { subject: "fixture-steward".into(), project_id: "gamma".into(), role: Role::Steward },
        Binding { subject: "fixture-auditor".into(), project_id: "delta".into(), role: Role::Auditor },
        Binding { subject: "fixture-owner".into(), project_id: "epsilon".into(), role: Role::BusinessOwner },
    ];
    let bootstrap = bindings[0].clone();
    let redirects = vec![callback.to_string()];
    let commit = canonical_commit(issuer, "oo-control", "oo-control-client", &redirects, &bindings, &bootstrap, 1).expect("commit");
    let body = json!({
        "https_origin": origin,
        "listen_host": "127.0.0.1",
        "listen_port": port,
        "tls_cert_file": dir.join("srv.pem"),
        "tls_key_file": dir.join("srv.key"),
        "issuer": issuer,
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
        "clock_skew_seconds": 0
    });
    fs::write(path, serde_json::to_vec_pretty(&body).expect("config")).expect("write config");
    let _ = material;
}

struct Material {
    ca: Vec<u8>,
    cert: Vec<u8>,
    key: Vec<u8>,
}

fn certs(dir: &Path) -> Material {
    openssl(dir, &["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", "ca.key", "-out", "ca.pem", "-days", "2", "-subj", "/CN=oo-a370-test-ca", "-addext", "basicConstraints=CA:TRUE"]);
    openssl(dir, &["req", "-newkey", "rsa:2048", "-nodes", "-keyout", "srv.key", "-out", "srv.csr", "-subj", "/CN=127.0.0.1"]);
    fs::write(dir.join("ext.cnf"), "subjectAltName=IP:127.0.0.1\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n").expect("ext");
    openssl(dir, &["x509", "-req", "-in", "srv.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial", "-out", "srv.pem", "-days", "2", "-extfile", "ext.cnf"]);
    openssl(dir, &["genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", "k1.pem"]);
    openssl(dir, &["pkcs8", "-topk8", "-nocrypt", "-in", "k1.pem", "-outform", "DER", "-out", "k1.der"]);
    openssl(dir, &["genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", "k2.pem"]);
    openssl(dir, &["pkcs8", "-topk8", "-nocrypt", "-in", "k2.pem", "-outform", "DER", "-out", "k2.der"]);
    Material {
        ca: fs::read(dir.join("ca.pem")).expect("ca"),
        cert: fs::read(dir.join("srv.pem")).expect("cert"),
        key: fs::read(dir.join("srv.key")).expect("key"),
    }
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

fn live_env() -> LiveEnv {
    if std::env::var("OO_A370_LIVE").ok().as_deref() != Some("1") {
        panic!("OO_A370_LIVE=1 is required");
    }
    LiveEnv {
        host: env_var("OO_A370_PG_HOST"),
        port: env_var("OO_A370_PG_PORT").parse().expect("pg port"),
        database: env_var("OO_A370_PG_DATABASE"),
        password_file: env_var("OO_A370_PG_PASSWORD_FILE"),
        ca_file: env_var("OO_A370_PG_CA"),
        proof: env_var("OO_A370_PROOF"),
    }
}

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("missing {name}"))
}
