#![allow(clippy::too_many_lines)]
#![forbid(unsafe_code)]

//! Changed-audience recheck for original E54. Real RS256 over HTTPS. No product edits.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
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

use control_identity::{canonical_commit, load_config, pg_tls, pkcs8_rsa_public, server_config, Binding, Role, TlsListener};

const CLIENT: &str = "rp-client";
const CONFIGURED: &str = "configured-aud";
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
    product_commit: String,
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
    codes: Mutex<HashMap<String, Issued>>,
    mint: Mutex<Mint>,
    subject: Mutex<String>,
    jwks_hits: AtomicU64,
    last_alg: Mutex<String>,
}

#[derive(Clone)]
struct Idp {
    inner: Arc<Inner>,
}

struct Leg {
    name: &'static str,
    http: u16,
    code: String,
    current_subject: String,
    body: Value,
    ok: bool,
}

struct Cli {
    status: u8,
    body: Value,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audience_recheck() {
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

    let port = free_port();
    let origin = format!("https://127.0.0.1:{port}");
    let callback = format!("{origin}/control/v1/auth/callback");
    let path = env.dir.join("split.json");
    write_config(&path, &env, &origin, &issuer, &callback, port);
    let loaded = load_config(&path).expect("config");
    let split = loaded.audience != loaded.client_id && loaded.audience == CONFIGURED && loaded.client_id == CLIENT;
    report.cases.push(json!({"name": "split_config", "ok": split}));
    if !split {
        report.failed = true;
    }
    let install = cli(&env.exe, &["install", "--config", path.to_str().expect("path")]);
    let installed = install.status == 0 && install.body["disposition"] == "committed";
    report.cases.push(json!({"name": "install", "ok": installed, "disposition": install.body["disposition"]}));
    if !installed {
        report.failed = true;
    }

    let mut server = Server::spawn(&env.exe, &path).await;
    let mut http = Http::new(&origin, &material.ca).await;
    idp.set_subject("fixture-admin");

    let other = probe(&mut http, &idp, "aud_other", Mint::one("other-aud", Some(CLIENT.to_string())), false).await;
    let client = probe(&mut http, &idp, "aud_client_id", Mint::one(CLIENT, Some(CLIENT.to_string())), true).await;
    let configured = probe(&mut http, &idp, "aud_configured_only", Mint::one(CONFIGURED, Some(CLIENT.to_string())), false).await;
    let single = probe(&mut http, &idp, "aud_client_id_no_azp", Mint::one(CLIENT, None), true).await;
    let multi_azp = probe(
        &mut http,
        &idp,
        "aud_multi_matching_azp",
        Mint { aud: Aud::Multi(vec![CLIENT.to_string(), "extra".to_string()]), azp: Some(CLIENT.to_string()) },
        true,
    )
    .await;
    let multi_missing = probe(
        &mut http,
        &idp,
        "aud_multi_without_azp",
        Mint { aud: Aud::Multi(vec![CLIENT.to_string(), "extra".to_string()]), azp: None },
        false,
    )
    .await;

    archive_leg(&env, "other-aud.json", &other);
    archive_leg(&env, "client-id-current.json", &client);
    archive_leg(&env, "configured-aud.json", &configured);
    archive_leg(&env, "single-no-azp-current.json", &single);
    archive_leg(&env, "multi-azp-current.json", &multi_azp);
    archive_leg(&env, "multi-no-azp.json", &multi_missing);
    for leg in [&other, &client, &configured, &single, &multi_azp, &multi_missing] {
        report.cases.push(json!({
            "name": leg.name,
            "http": leg.http,
            "code": leg.code,
            "current_subject": leg.current_subject,
            "ok": leg.ok
        }));
    }

    let three = other.ok && client.ok && configured.ok;
    let azp_rules = single.ok && multi_azp.ok && multi_missing.ok;
    let closed = three && azp_rules && !report.failed;
    let verdict = if closed { "FINDING_CLOSED" } else { "WITHHOLD" };
    let alg = idp.inner.last_alg.lock().expect("alg").clone();
    let fetches = idp.inner.jwks_hits.load(Ordering::SeqCst);
    if alg != "RS256" || fetches == 0 {
        report.failed = true;
    }
    let body = json!({
        "product_commit": env.product_commit,
        "reviewed_producer": "35bd6b8157cecd89f2ae085b012b4b0137562ddb",
        "customer_integration": "not_claimed",
        "e54": "not_accepted",
        "sp02_parent": "not_accepted",
        "full_goal": "not_accepted",
        "clause": "OIDC Core 1.0 §3.1.3.7",
        "original_finding": if three && !report.failed { "closed" } else { "open" },
        "azp_rules": if azp_rules { "held" } else { "failed" },
        "verdict": verdict,
        "audience": CONFIGURED,
        "client_id": CLIENT,
        "issuer_scheme": "https",
        "id_token_alg": alg,
        "jwks_fetches": fetches,
        "reused_68_rerun": false,
        "cases": report.cases
    });
    let mut text = serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".to_string());
    for secret in &report.forbidden {
        if !secret.is_empty() && text.contains(secret) {
            text = text.replace(secret, "REDACTED");
            report.failed = true;
        }
    }
    fs::write(&env.proof, &text).expect("proof");
    fs::write(Path::new(&env.proof).with_extension("verdict"), verdict).expect("verdict");
    server.kill();
    let stderr = server.stderr_text();
    if report.forbidden.iter().any(|item| !item.is_empty() && stderr.contains(item)) {
        report.failed = true;
    }
    drop_schema(&pg).await;
    let _ = fs::remove_dir_all(&env.dir);
    assert!(!report.failed, "audience recheck did not complete");
    assert!(verdict == "FINDING_CLOSED" || verdict == "WITHHOLD");
}

async fn probe(http: &mut Http, idp: &Idp, name: &'static str, mint: Mint, accept: bool) -> Leg {
    http.clear();
    idp.set_mint(mint);
    let reply = login_follow(http, "/control/v1/auth/login?return_to=%2Fapp").await;
    let code = if reply.status == 302 { "ok".to_string() } else { error_code(&reply.body) };
    let (subject, body, current_ok) = if accept && reply.status == 302 {
        let current = http.call("GET", "/control/v1/identity/current", None).await;
        let subject = current.body.get("subject").and_then(Value::as_str).unwrap_or("").to_string();
        let current_ok = current.status == 200 && subject == "fixture-admin" && envelope_ok(&current.body);
        (subject, current.body, current_ok)
    } else {
        (String::new(), reply.body.clone(), false)
    };
    let ok = if accept { reply.status == 302 && current_ok } else { reply.status == 401 && code == "audience.rejected" };
    Leg { name, http: reply.status, code, current_subject: subject, body, ok }
}

async fn login_follow(http: &mut Http, path: &str) -> Reply {
    let started = http.call("GET", path, None).await;
    if started.status != 302 {
        return started;
    }
    let at_idp = http.get_external(&started.location).await;
    if at_idp.status != 302 {
        return at_idp;
    }
    http.call("GET", &at_idp.location, None).await
}

fn env() -> Env {
    assert_eq!(std::env::var("OO_A375C_ORACLE").ok().as_deref(), Some("1"));
    Env {
        dir: std::env::temp_dir().join(format!("oo-a375c-oracle-{}", std::process::id())),
        exe: env!("CARGO_BIN_EXE_control-identity").to_string(),
        pg_host: std::env::var("OO_A375C_PG_HOST").expect("pg host"),
        pg_port: std::env::var("OO_A375C_PG_PORT").expect("pg port").parse().expect("pg port"),
        pg_database: std::env::var("OO_A375C_PG_DATABASE").expect("pg database"),
        pg_password_file: std::env::var("OO_A375C_PG_PASSWORD_FILE").expect("pg password file"),
        pg_ca: std::env::var("OO_A375C_PG_CA").expect("pg ca"),
        proof: std::env::var("OO_A375C_PROOF").expect("proof"),
        responses: std::env::var("OO_A375C_RESPONSES").expect("responses"),
        product_commit: std::env::var("OO_A375C_PRODUCT_COMMIT").expect("commit"),
    }
}

impl Mint {
    fn one(aud: &str, azp: Option<String>) -> Self {
        Self { aud: Aud::One(aud.to_string()), azp }
    }
}

impl Idp {
    async fn start(listener: TcpListener, issuer: &str, secret: &str, material: &Material) -> Self {
        let inner = Arc::new(Inner {
            issuer: issuer.to_string(),
            client_id: CLIENT.to_string(),
            client_secret: secret.to_string(),
            keys: vec![signing_key("k1", &material.ders[0])],
            codes: Mutex::new(HashMap::new()),
            mint: Mutex::new(Mint::one(CLIENT, Some(CLIENT.to_string()))),
            subject: Mutex::new("fixture-admin".to_string()),
            jwks_hits: AtomicU64::new(0),
            last_alg: Mutex::new(String::new()),
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
    let map = form(request.uri().query().unwrap_or(""));
    let required = ["response_type", "client_id", "redirect_uri", "scope", "state", "nonce", "code_challenge", "code_challenge_method"];
    if required.iter().any(|key| !map.contains_key(*key))
        || map["response_type"] != "code"
        || map["client_id"] != state.client_id
        || map["code_challenge_method"] != "S256"
        || map["scope"] != "openid"
    {
        return text(400);
    }
    let code = random_token();
    state.codes.lock().expect("codes").insert(
        code.clone(),
        Issued { challenge: map["code_challenge"].clone(), nonce: map["nonce"].clone(), used: false },
    );
    redirect(&format!("{}?code={}&state={}", map["redirect_uri"], code, map["state"]))
}

async fn token(axum::extract::State(state): axum::extract::State<Arc<Inner>>, request: axum::extract::Request) -> axum::response::Response {
    let bytes = axum::body::to_bytes(request.into_body(), 65_536).await.unwrap_or_default();
    let Ok(form_text) = String::from_utf8(bytes.to_vec()) else {
        return text(400);
    };
    let fields = form(&form_text);
    let required = ["grant_type", "code", "redirect_uri", "client_id", "code_verifier", "client_secret"];
    if required.iter().any(|key| !fields.contains_key(*key))
        || fields["grant_type"] != "authorization_code"
        || fields["client_id"] != state.client_id
        || fields["client_secret"] != state.client_secret
    {
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
    state.jwks_hits.fetch_add(1, Ordering::SeqCst);
    json_response(json!({"keys": [jwk(&state.keys[0])]}))
}

fn mint_token(state: &Inner, mint: &Mint, subject: &str, nonce: &str) -> String {
    let now = unix_now();
    let aud = match &mint.aud {
        Aud::One(value) => json!(value),
        Aud::Multi(values) => json!(values),
    };
    let mut payload = json!({
        "iss": state.issuer,
        "aud": aud,
        "exp": now + 300,
        "nbf": now,
        "iat": now,
        "nonce": nonce,
        "sub": subject
    });
    if let Some(azp) = &mint.azp {
        payload["azp"] = json!(azp);
    }
    let body = serde_json::to_string(&payload).expect("payload");
    let key = &state.keys[0];
    let header = b64_str(&format!("{{\"alg\":\"RS256\",\"kid\":\"{}\",\"typ\":\"JWT\"}}", key.kid));
    let encoded = b64_str(&body);
    let input = format!("{header}.{encoded}");
    let mut signature = vec![0_u8; key.pair.public().modulus_len()];
    key.pair.sign(&RSA_PKCS1_SHA256, &SystemRandom::new(), input.as_bytes(), &mut signature).expect("sign");
    *state.last_alg.lock().expect("alg") = "RS256".to_string();
    format!("{input}.{}", b64(&signature))
}

fn jwk(key: &SigningKey) -> Value {
    json!({"kty": "RSA", "kid": key.kid, "use": "sig", "alg": "RS256", "n": key.n, "e": key.e})
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
        Self { client, origin: origin.to_string(), login: None, session: None }
    }

    fn clear(&mut self) {
        self.login = None;
        self.session = None;
    }

    async fn call(&mut self, method: &str, path: &str, body: Option<&str>) -> Reply {
        let url = if path.starts_with("https://") { path.to_string() } else { format!("{}{path}", self.origin) };
        let mut request = self.client.request(method.parse().expect("method"), &url);
        if let Some(cookie) = self.cookie_header() {
            request = request.header("cookie", cookie);
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

fn envelope_ok(value: &Value) -> bool {
    value.get("contract").and_then(Value::as_str) == Some(CONTRACT)
        && value.get("authority").and_then(Value::as_str) == Some("management_session")
        && value.get("data_grants").and_then(Value::as_str) == Some("none")
        && value.get("session_class").and_then(Value::as_str) == Some("management_cookie")
        && value.get("subject").and_then(Value::as_str) == Some("fixture-admin")
}

fn archive_leg(env: &Env, name: &str, leg: &Leg) {
    let mut copy = leg.body.clone();
    if let Some(object) = copy.as_object_mut()
        && object.contains_key("csrf_token")
    {
        object.insert("csrf_token".to_string(), json!("REDACTED"));
    }
    fs::create_dir_all(&env.responses).expect("responses");
    fs::write(Path::new(&env.responses).join(name), serde_json::to_string_pretty(&copy).unwrap_or_default()).expect("archive");
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
            out.push((hex(bytes[index + 1]) << 4) | hex(bytes[index + 2]));
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

impl Drop for Server {
    fn drop(&mut self) {
        self.kill();
    }
}

fn cli(exe: &str, args: &[&str]) -> Cli {
    let output = Command::new(exe).args(args).env_clear().env("HOME", "/tmp").env("PATH", "/usr/bin:/bin").output().expect("cli");
    Cli { status: output.status.code().unwrap_or(1) as u8, body: serde_json::from_slice(&output.stdout).unwrap_or(Value::Null) }
}

fn write_config(path: &Path, env: &Env, origin: &str, issuer: &str, callback: &str, port: u16) {
    let bindings = [Binding { subject: "fixture-admin".into(), project_id: "alpha".into(), role: Role::Admin }];
    let redirects = vec![callback.to_string()];
    let commit = canonical_commit(issuer, CONFIGURED, CLIENT, &redirects, &bindings, &bindings[0], 7).expect("commit");
    let body = json!({
        "https_origin": origin,
        "listen_host": "127.0.0.1",
        "listen_port": port,
        "tls_cert_file": env.dir.join("server.crt").to_string_lossy(),
        "tls_key_file": env.dir.join("server.pkcs8.pem").to_string_lossy(),
        "issuer": issuer,
        "audience": CONFIGURED,
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
        "bindings": [{"subject": "fixture-admin", "project_id": "alpha", "role": "admin"}],
        "bootstrap": {"subject": "fixture-admin", "project_id": "alpha", "role": "admin"},
        "idle_seconds": 600,
        "absolute_seconds": 3600,
        "oauth_ttl_seconds": 300,
        "bootstrap_ttl_seconds": 300,
        "clock_skew_seconds": 30
    });
    fs::write(path, serde_json::to_vec_pretty(&body).expect("json")).expect("config");
}

fn certs(dir: &Path) -> Material {
    openssl(dir, &["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", "ca.key", "-out", "ca.crt", "-days", "7", "-subj", "/CN=oo-a375c-oracle"]);
    openssl(dir, &["req", "-newkey", "rsa:2048", "-nodes", "-keyout", "server.key", "-out", "server.csr", "-subj", "/CN=127.0.0.1"]);
    fs::write(dir.join("san.cnf"), "basicConstraints=CA:FALSE\nsubjectAltName=IP:127.0.0.1\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n").expect("san");
    openssl(dir, &["x509", "-req", "-in", "server.csr", "-CA", "ca.crt", "-CAkey", "ca.key", "-CAcreateserial", "-out", "server.crt", "-days", "7", "-extfile", "san.cnf"]);
    openssl(dir, &["pkcs8", "-topk8", "-nocrypt", "-in", "server.key", "-out", "server.pkcs8.pem"]);
    openssl(dir, &["genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", "k1.pem"]);
    openssl(dir, &["pkcs8", "-topk8", "-nocrypt", "-in", "k1.pem", "-outform", "DER", "-out", "k1.der"]);
    Material {
        ca: fs::read(dir.join("ca.crt")).expect("ca"),
        cert: fs::read(dir.join("server.crt")).expect("cert"),
        key: fs::read(dir.join("server.pkcs8.pem")).expect("key"),
        ders: vec![fs::read(dir.join("k1.der")).expect("k1")],
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

async fn drop_schema(pg: &tokio_postgres::Client) {
    pg.batch_execute("DROP SCHEMA IF EXISTS oo_identity_control CASCADE").await.expect("drop");
}

async fn wait_discovery(ca: &[u8], issuer: &str) {
    let certs = reqwest::Certificate::from_pem_bundle(ca).expect("ca");
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .tls_certs_only(certs)
        .timeout(Duration::from_secs(2))
        .build()
        .expect("client");
    let url = format!("{issuer}/.well-known/openid-configuration");
    for _ in 0..50 {
        if client.get(&url).send().await.ok().is_some_and(|response| response.status().is_success()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("test issuer did not become ready");
}
