//! Independent HTTP check of a6f92 current rotation and consume side effects.
//! Tokens, cookies, and passwords are not written to the output file.

use std::env;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use control_identity::{canonical_commit, listen, load_config, pg_tls, serve, sha256_hex, Binding, Role, Store};
use serde_json::{json, Value};
use tokio_postgres::config::SslMode;
use tokio_postgres::Client;

const UNKNOWN: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn csrf_rotation_and_consume_side_effect() {
    let env = ProbeEnv::load(false);
    let dir = env::temp_dir().join(format!("oo-a404-csrf-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("work dir");
    let secret_path = dir.join("client.secret");
    write_secret(&secret_path);

    let port = free_port();
    let origin = format!("https://127.0.0.1:{port}");
    let issuer = "https://127.0.0.1:1".to_string();
    let callback = format!("{origin}/control/v1/auth/callback");
    let config_path = dir.join("identity.json");
    write_config(&config_path, &env, &secret_path, &origin, &issuer, &callback, port);
    let config = Arc::new(load_config(&config_path).expect("identity config"));
    Store::install(&config).await.expect("install");
    let token_path = dir.join("bootstrap.token");
    Store::issue_bootstrap(&config, &token_path).await.expect("issue");
    let token = fs::read_to_string(&token_path).expect("token file");
    let token = token.trim_end_matches(['\n', '\r']).to_string();
    assert_eq!(token.len(), 43, "issued token length");
    let token_hash = sha256_hex(token.as_bytes());
    drop(token);
    let _ = fs::remove_file(&token_path);
    let store = Store::connect(&config).await.expect("connect");
    let cookie = store.finish_login(&config, "fixture-admin", Some(&token_hash)).await.expect("finish login");

    let listener = listen(&config).await.expect("listen");
    let served = Arc::clone(&config);
    let _join = tokio::spawn(async move {
        let _ = serve(served, listener).await;
    });

    let http = Http::new(&origin, &fs::read(env.cert_dir.join("ca.pem")).expect("https ca"));
    let first = http.current(&cookie).await;
    assert_eq!(first.status, 200, "first current");
    let second = http.current(&cookie).await;
    assert_eq!(second.status, 200, "second current");
    let rotated = first.csrf_sha != second.csrf_sha && !first.csrf_sha.is_empty();
    let hash_before = csrf_hash(&env).await;

    let before = audit_count(&env, "bootstrap_rejected").await;
    let stale = http.consume(&cookie, &first.token, &origin).await;
    let after_stale = audit_count(&env, "bootstrap_rejected").await;
    let hash_after_stale = csrf_hash(&env).await;

    let wrong_origin = http.consume(&cookie, &second.token, "https://127.0.0.1:9").await;
    let after_origin = audit_count(&env, "bootstrap_rejected").await;
    let hash_after_origin = csrf_hash(&env).await;

    let fresh = http.consume(&cookie, &second.token, &origin).await;
    let after_fresh = audit_count(&env, "bootstrap_rejected").await;
    let hash_after_fresh = csrf_hash(&env).await;
    let presented_matches = hash_after_fresh == second.csrf_sha;

    let third = http.current(&cookie).await;
    assert_eq!(third.status, 200, "current after consume");
    let hash_after_third = csrf_hash(&env).await;
    let burned = http.consume(&cookie, &second.token, &origin).await;
    let after_burned = audit_count(&env, "bootstrap_rejected").await;

    let states = bootstrap_states(&env).await;
    let groups = audit_groups(&env).await;
    let body = json!({
        "pinned_identity": "a6f92da8a0bf872f8a30efb116c2c7b14463505b",
        "unknown_token": "43-A",
        "current_rotated": rotated,
        "stored_hash_is_second_token": hash_before == second.csrf_sha,
        "stale_status": stale.status,
        "stale_code": stale.code,
        "stale_audit_delta": after_stale - before,
        "stored_hash_unchanged_on_stale": hash_after_stale == hash_before,
        "origin_status": wrong_origin.status,
        "origin_code": wrong_origin.code,
        "origin_audit_delta": after_origin - after_stale,
        "stored_hash_unchanged_on_origin": hash_after_origin == second.csrf_sha,
        "consume_status": fresh.status,
        "consume_code": fresh.code,
        "consume_audit_delta": after_fresh - after_origin,
        "stored_hash_unchanged_on_consume": hash_after_fresh == second.csrf_sha,
        "stored_hash_matches_presented": presented_matches,
        "current_after_consume_differs": third.csrf_sha != second.csrf_sha,
        "stored_hash_matches_rotated": hash_after_third == third.csrf_sha,
        "burned_status": burned.status,
        "burned_code": burned.code,
        "burned_audit_delta": after_burned - after_fresh,
        "bootstrap_states": states,
        "audit": groups,
        "session_still_current": third.status == 200
    });
    fs::write(&env.out, serde_json::to_vec_pretty(&body).expect("json")).expect("write probe");
    let _ = fs::remove_dir_all(&dir);
    assert!(rotated, "current rotated csrf");
    assert_eq!(hash_before, second.csrf_sha, "stored hash is the second token");
    assert_eq!(hash_after_stale, hash_before, "stale request left the hash");
    assert_eq!(stale.status, 403, "stale csrf status");
    assert_eq!(stale.code, "csrf.rejected", "stale csrf code");
    assert_eq!(after_stale, before, "stale csrf wrote no bootstrap audit");
    assert_eq!(wrong_origin.status, 403, "origin status");
    assert_eq!(wrong_origin.code, "origin.rejected", "origin code");
    assert_eq!(after_origin, after_stale, "origin wrote no bootstrap audit");
    assert_eq!(fresh.status, 403, "unknown consume status");
    assert_eq!(fresh.code, "bootstrap.rejected", "unknown consume code");
    assert_eq!(after_fresh, after_origin + 1, "unknown consume inserted one audit");
    assert!(presented_matches, "consume left the stored csrf hash");
    assert_ne!(third.csrf_sha, second.csrf_sha, "current after the probe rotated again");
    assert_eq!(burned.code, "csrf.rejected", "rotated token is dead");
    assert_eq!(after_burned, after_fresh, "dead token wrote no second audit");
    assert_eq!(states, vec!["consumed".to_string()], "unknown token did not consume a row");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn observe_product_database() {
    let env = ProbeEnv::load(true);
    let identity = connect(&env, "oo_identity_admin", &env.identity_password).await;
    let groups = group_rows(&identity).await;
    let producer = connect(&env, "oo_release_admin", &env.producer_password).await;
    let view = producer
        .query_one("SELECT pg_catalog.pg_get_viewdef('oo_release_control.publication_status'::regclass, true)", &[])
        .await
        .expect("viewdef")
        .get::<_, String>(0);
    let columns = producer
        .query(
            "SELECT attname FROM pg_catalog.pg_attribute \
             WHERE attrelid = 'oo_release_control.publication_status'::regclass AND attnum > 0 AND NOT attisdropped \
             ORDER BY attnum",
            &[],
        )
        .await
        .expect("columns");
    let mut names = Vec::new();
    for row in &columns {
        names.push(row.get::<_, String>(0));
    }
    let policies = producer
        .query_one(
            "SELECT count(*) FROM pg_catalog.pg_policy WHERE polrelid = 'oo_release_control.candidate_file'::regclass",
            &[],
        )
        .await
        .expect("policies")
        .get::<_, i64>(0);
    let files = producer
        .query_one("SELECT count(*) FROM oo_release_control.candidate_file", &[])
        .await
        .expect("files")
        .get::<_, i64>(0);
    let rls = producer
        .query_one(
            "SELECT relrowsecurity FROM pg_catalog.pg_class WHERE oid = 'oo_release_control.candidate_file'::regclass",
            &[],
        )
        .await
        .expect("rls")
        .get::<_, bool>(0);
    let reader = connect(&env, "oo_release_reader", &env.reader_password).await;
    let pointer = reader
        .query_one(
            "SELECT version, production_ready_certificate, full_sp08_accepted FROM oo_release_control.publication_status",
            &[],
        )
        .await
        .expect("pointer");
    let body = json!({
        "identity_audit": groups,
        "publication_view": view,
        "publication_columns": names,
        "publication_has_project_id": names.iter().any(|name| name == "project_id"),
        "candidate_file_policies": policies,
        "candidate_file_rls": rls,
        "candidate_file_count": files,
        "reader_version": pointer.get::<_, Option<String>>(0),
        "reader_production_ready_certificate": pointer.get::<_, bool>(1),
        "reader_full_sp08_accepted": pointer.get::<_, bool>(2)
    });
    fs::write(&env.out, serde_json::to_vec_pretty(&body).expect("json")).expect("write observe");
    assert!(!names.iter().any(|name| name == "project_id"), "view exposes project_id");
    assert!(view.to_lowercase().contains("singleton"), "view predicate");
    assert!(!view.to_lowercase().contains("project_id"), "view predicate omits project");
    assert_eq!(policies, 0, "candidate_file policy count");
    assert!(!rls, "candidate_file rls");
    assert!(!pointer.get::<_, bool>(1), "production certificate");
    assert!(!pointer.get::<_, bool>(2), "sp08 accepted");
}

struct ProbeEnv {
    host: String,
    port: u16,
    database: String,
    cert_dir: PathBuf,
    ca: Vec<u8>,
    identity_password: PathBuf,
    producer_password: PathBuf,
    reader_password: PathBuf,
    out: PathBuf,
}

impl ProbeEnv {
    fn load(observe: bool) -> Self {
        let database = if observe {
            env::var("OO_A404_OBSERVE_DATABASE").expect("observe database")
        } else {
            env::var("OO_A404_PG_DATABASE").expect("csrf database")
        };
        let out = if observe {
            env::var("OO_A404_OBSERVE_OUT").expect("observe out")
        } else {
            env::var("OO_A404_CSRF_OUT").expect("csrf out")
        };
        let ca_path = PathBuf::from(env::var("OO_A404_PG_CA").expect("ca"));
        Self {
            host: env::var("OO_A404_PG_HOST").expect("host"),
            port: env::var("OO_A404_PG_PORT").expect("port").parse().expect("port number"),
            database,
            cert_dir: PathBuf::from(env::var("OO_A404_CERT_DIR").expect("certs")),
            ca: fs::read(&ca_path).expect("ca bytes"),
            identity_password: PathBuf::from(env::var("OO_A404_IDENTITY_PASSWORD_FILE").expect("identity pass")),
            producer_password: PathBuf::from(env::var("OO_A404_PRODUCER_PASSWORD_FILE").unwrap_or_default()),
            reader_password: PathBuf::from(env::var("OO_A404_READER_PASSWORD_FILE").unwrap_or_default()),
            out: PathBuf::from(out),
        }
    }
}

struct Http {
    client: reqwest::Client,
    origin: String,
}

struct Reply {
    status: u16,
    code: String,
    csrf_sha: String,
    token: String,
}

impl Http {
    fn new(origin: &str, ca_pem: &[u8]) -> Self {
        let certs = reqwest::Certificate::from_pem_bundle(ca_pem).expect("ca bundle");
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
        Self { client, origin: origin.to_string() }
    }

    async fn current(&self, cookie: &str) -> Reply {
        self.send("GET", "/control/v1/identity/current", None, cookie, None, None).await
    }

    async fn consume(&self, cookie: &str, csrf: &str, origin: &str) -> Reply {
        let body = format!("{{\"token\":\"{UNKNOWN}\"}}");
        self.send("POST", "/control/v1/auth/bootstrap/consume", Some(body), cookie, Some(csrf), Some(origin)).await
    }

    async fn send(&self, method: &str, path: &str, body: Option<String>, cookie: &str, csrf: Option<&str>, origin: Option<&str>) -> Reply {
        let url = format!("{}{path}", self.origin);
        let mut request = self.client.request(method.parse().expect("method"), url);
        request = request.header("cookie", format!("__Host-oo-session={cookie}"));
        request = request.header("accept", "application/json");
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        if let Some(csrf) = csrf {
            request = request.header("x-oo-csrf", csrf);
        }
        if let Some(body) = body {
            request = request.header("content-type", "application/json").body(body);
        }
        let response = request.send().await.expect("identity http");
        let status = response.status().as_u16();
        let bytes = response.bytes().await.expect("body");
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        let code = value
            .get("error")
            .and_then(|item| item.get("code"))
            .and_then(Value::as_str)
            .unwrap_or(if status == 200 { "ok" } else { "" })
            .to_string();
        let token = value.get("csrf_token").and_then(Value::as_str).unwrap_or("").to_string();
        let csrf_sha = if token.is_empty() { String::new() } else { sha256_hex(token.as_bytes()) };
        drop(value);
        drop(bytes);
        Reply { status, code, csrf_sha, token }
    }
}

fn write_secret(path: &Path) {
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(path).expect("secret");
    use std::io::Write;
    file.write_all(b"fixture-client-secret-a404\n").expect("secret bytes");
}

fn write_config(path: &Path, env: &ProbeEnv, secret: &Path, origin: &str, issuer: &str, callback: &str, port: u16) {
    let bindings = vec![Binding { subject: "fixture-admin".into(), project_id: "shop".into(), role: Role::Admin }];
    let bootstrap = bindings[0].clone();
    let redirects = vec![callback.to_string()];
    let commit = canonical_commit(issuer, "oo-control", "oo-control-client", &redirects, &bindings, &bootstrap, 1).expect("commit");
    let body = json!({
        "https_origin": origin,
        "listen_host": "127.0.0.1",
        "listen_port": port,
        "tls_cert_file": env.cert_dir.join("srv.pem"),
        "tls_key_file": env.cert_dir.join("srv.key"),
        "issuer": issuer,
        "audience": "oo-control",
        "client_id": "oo-control-client",
        "client_secret_file": secret,
        "redirect_uris": redirects,
        "config_commit": commit,
        "revision": 1,
        "idp_ca_file": env.cert_dir.join("ca.pem"),
        "pg": {
            "host": env.host,
            "port": env.port,
            "database": env.database,
            "user": "oo_identity_admin",
            "password_file": env.identity_password,
            "ca_file": env::var("OO_A404_PG_CA").expect("ca path")
        },
        "bindings": [{"subject": "fixture-admin", "project_id": "shop", "role": "admin"}],
        "bootstrap": {"subject": "fixture-admin", "project_id": "shop", "role": "admin"},
        "idle_seconds": 300,
        "absolute_seconds": 300,
        "oauth_ttl_seconds": 60,
        "bootstrap_ttl_seconds": 120,
        "clock_skew_seconds": 0
    });
    fs::write(path, serde_json::to_vec(&body).expect("config json")).expect("write config");
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("port");
    listener.local_addr().expect("addr").port()
}

async fn connect(env: &ProbeEnv, user: &str, password_file: &Path) -> Client {
    let mut password = fs::read_to_string(password_file).expect("password file");
    if password.ends_with('\n') {
        password.pop();
    }
    let tls = pg_tls(&env.ca).expect("pg tls");
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(&password);
    config.host(&env.host);
    config.port(env.port);
    config.dbname(&env.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("oo-a404-review");
    let (client, connection) = config.connect(tls).await.expect("pg connect");
    drop(password);
    let _join = tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn audit_count(env: &ProbeEnv, action: &str) -> i64 {
    let client = connect(env, "oo_identity_admin", &env.identity_password).await;
    let row = client
        .query_one("SELECT count(*) FROM oo_identity_control.audit_event WHERE action = $1", &[&action])
        .await
        .expect("audit count");
    row.get(0)
}

async fn csrf_hash(env: &ProbeEnv) -> String {
    let client = connect(env, "oo_identity_admin", &env.identity_password).await;
    let row = client.query_one("SELECT csrf_hash FROM oo_identity_control.session", &[]).await.expect("csrf hash");
    row.get(0)
}

async fn bootstrap_states(env: &ProbeEnv) -> Vec<String> {
    let client = connect(env, "oo_identity_admin", &env.identity_password).await;
    let rows = client.query("SELECT state FROM oo_identity_control.bootstrap_token ORDER BY state", &[]).await.expect("states");
    rows.iter().map(|row| row.get(0)).collect()
}

async fn audit_groups(env: &ProbeEnv) -> Vec<Value> {
    let client = connect(env, "oo_identity_admin", &env.identity_password).await;
    group_rows(&client).await
}

async fn group_rows(client: &Client) -> Vec<Value> {
    let rows = client
        .query(
            "SELECT action, code, count(*) FROM oo_identity_control.audit_event GROUP BY action, code ORDER BY action, code",
            &[],
        )
        .await
        .expect("groups");
    rows.iter()
        .map(|row| json!({"action": row.get::<_, String>(0), "code": row.get::<_, String>(1), "n": row.get::<_, i64>(2)}))
        .collect()
}
