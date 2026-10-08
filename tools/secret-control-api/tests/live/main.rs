#![forbid(unsafe_code)]

mod idp;

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{json, Value};
use tokio_postgres::config::SslMode;

use secret_control_api::{
    canonical, listen, load_config, open, pg_tls, sha256_hex, spawn_server, Scope, ScriptedNonce,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_secret_loop() {
    if std::env::var("OO_A384_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    match std::env::var("OO_A384_PHASE").ok().as_deref() {
        Some("write") => write_phase().await,
        Some("restart") => restart_phase().await,
        _ => panic!("OO_A384_PHASE must be write or restart"),
    }
}

struct World {
    env: Env,
    state: PathBuf,
    material: Material,
    http: Http,
    idp: idp::Idp,
    identity: ServerProc,
    csrf: String,
    revision: i64,
    secret: String,
    rotated: String,
    agent_secret: String,
    issuer: String,
}

async fn write_phase() {
    let mut world = boot_world().await;
    let secret_config = install_secret(&world);
    half_write(&world.env, &world.state, &world.material, &mut world.http, world.revision).await;
    let mut secret_server = ServerProc::spawn(&secret_bin(), &secret_config);
    let listening = secret_server.listening().await;
    assert_eq!(listening["customer_kms"], "external_gate");
    assert_eq!(listening["mtls_ca"], "not_issued");
    let management = format!("https://{}", listening["management_addr"].as_str().expect("mgmt"));
    let component = format!("https://{}", listening["component_addr"].as_str().expect("comp"));
    let mut cases = Vec::new();
    run_matrix(&mut world, &management, &component, &mut cases).await;
    prove_ciphertext(&mut world, &secret_config, &mut secret_server, &mut cases).await;
    finish_files(&world, &cases);
}

async fn boot_world() -> World {
    let env = Env::load();
    let state = PathBuf::from(&env.state);
    let _ = fs::remove_file(state.join("phase.txt"));
    let _ = fs::remove_file(state.join("run.json"));
    let _ = fs::remove_file(state.join("witness"));
    let material = certs(&state);
    chmod_private(&state);
    let secret = openssl_hex();
    let rotated = openssl_hex();
    let agent_secret = openssl_hex();
    assert!(secret.len() >= 32 && rotated.len() >= 32 && agent_secret.len() >= 32, "entropy");
    assert_ne!(secret, rotated, "rotation changes the value");
    let client_secret = openssl_hex();
    write_secret(&state.join("client.secret"), &client_secret);
    let identity_port = free_port();
    let idp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("idp");
    let idp_port = idp_listener.local_addr().expect("idp addr").port();
    let origin = format!("https://127.0.0.1:{identity_port}");
    let issuer = format!("https://127.0.0.1:{idp_port}");
    let callback = format!("{origin}/control/v1/auth/callback");
    write_identity_config(&state, &env, &origin, &issuer, &callback, identity_port);
    let idp = start_idp(&state, &material, idp_listener, &issuer, &client_secret, &callback).await;
    let identity_config = state.join("identity.json");
    let installed = cli(&env.identity_bin, &["install", "--config", identity_config.to_str().expect("path")]);
    assert_eq!(installed.status, 0, "identity install");
    let token_path = state.join("bootstrap.token");
    let _ = fs::remove_file(&token_path);
    let issued = cli(&env.identity_bin, &["bootstrap-issue", "--config", identity_config.to_str().expect("path"), "--out", token_path.to_str().expect("out")]);
    assert_eq!(issued.status, 0, "bootstrap issue");
    let token = fs::read_to_string(&token_path).unwrap_or_default();
    assert!(!issued.raw.contains(token.trim()), "token stdout");
    let bootstrap = read_trimmed(&token_path);
    let mut identity = ServerProc::spawn(&env.identity_bin, &identity_config);
    let identity_listen = identity.listening().await;
    assert_eq!(identity_listen["outcome"], "listening", "identity serve");
    let mut http = Http::new(&origin, &material.ca);
    let logged_in = login(&mut http, &issuer, &format!("/control/v1/auth/login?return_to=%2Fapp&bootstrap_token={bootstrap}")).await;
    assert_eq!(logged_in.status, 302, "admin login");
    let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(current.status, 200, "current");
    let caps = current.body["projects"][0]["capabilities"].as_array().expect("caps");
    assert!(caps.iter().any(|item| item.as_str() == Some("project.admin")), "project.admin");
    let caller = secret_control_api::try_from_caller_json(current.raw.as_bytes()).expect_err("caller");
    assert_eq!(caller.code, "unauthenticated");
    World {
        csrf: current.body["csrf_token"].as_str().expect("csrf").to_string(),
        revision: current.body["revision"].as_i64().expect("revision"),
        env,
        state,
        material,
        http,
        idp,
        identity,
        secret,
        rotated,
        agent_secret,
        issuer,
    }
}

async fn start_idp(state: &Path, material: &Material, listener: tokio::net::TcpListener, issuer: &str, client_secret: &str, callback: &str) -> idp::Idp {
    idp::start(
        listener,
        idp::IdpParts {
            cert_pem: &material.cert,
            key_pem: &material.key,
            issuer,
            client_id: "oo-control-client",
            client_secret,
            redirect_uri: callback,
            der_key: &fs::read(state.join("k1.der")).expect("der"),
        },
    )
    .await
}

fn install_secret(world: &World) -> PathBuf {
    let secret_config = world.state.join("secret.json");
    write_secret_config(&secret_config, &world.env, &world.state, &world.material, 0, 0);
    let secret_bin = secret_bin();
    let secret_install = cli(&secret_bin, &["install", "--config", secret_config.to_str().expect("path")]);
    assert_eq!(secret_install.status, 0, "secret install");
    assert_eq!(secret_install.body["disposition"], "committed");
    assert_eq!(secret_install.body["customer_kms"], "external_gate");
    assert_eq!(secret_install.body["mtls_ca"], "not_issued");
    let replay = cli(&secret_bin, &["install", "--config", secret_config.to_str().expect("path")]);
    assert_eq!(replay.body["disposition"], "replayed");
    secret_config
}

async fn run_matrix(world: &mut World, management: &str, component: &str, cases: &mut Vec<Value>) {
    prove_db(world, management, cases).await;
    prove_open(world, management, component, cases).await;
    prove_controls(world, management, cases).await;
    prove_race(world, management, cases).await;
    prove_revoke(world, management, component, cases).await;
    role_denials(&mut world.http, &world.idp, &world.issuer, management, cases).await;
}

async fn prove_ciphertext(world: &mut World, secret_config: &Path, secret_server: &mut ServerProc, cases: &mut Vec<Value>) {
    let pg_password = read_trimmed(Path::new(&world.env.password_file));
    let ciphertext = read_ciphertext(&world.env, &pg_password, "db", 2).await;
    assert!(!ciphertext.windows(world.rotated.len()).any(|window| window == world.rotated.as_bytes()), "stored bytes hide the value");
    let config = load_config(secret_config).expect("config");
    let scope = Scope { deployment: "lab".into(), project: "alpha".into(), namespace: "builder".into(), secret_id: "db".into() };
    let mut nonce = [0_u8; 12];
    nonce.copy_from_slice(&read_nonce(&world.env, &pg_password, "db", 2).await);
    assert!(open(config.master_key.as_slice(), &nonce, &canonical(&scope, 2), &ciphertext).is_ok(), "library opens the row");
    let mut other = scope.clone();
    other.namespace = "agent".into();
    assert!(open(config.master_key.as_slice(), &nonce, &canonical(&other, 2), &ciphertext).is_err(), "wrong aad");
    let mut wrong = config.master_key.to_vec();
    wrong[0] ^= 0x5a;
    assert!(open(&wrong, &nonce, &canonical(&scope, 2), &ciphertext).is_err(), "wrong key");
    cases.push(json!({"name": "aad_and_key", "http": 403, "code": "aead.rejected"}));
    let stderr = secret_server.stderr_text();
    let identity_stderr = world.identity.stderr_text();
    assert!(!stderr.contains(&world.secret) && !stderr.contains(&world.rotated) && !stderr.contains(&world.agent_secret), "secret stderr");
    assert!(!identity_stderr.contains(&world.secret) && !identity_stderr.contains(&world.rotated), "identity stderr");
    assert!(!secret_server.stdout_text().contains(&world.rotated), "secret stdout");
    secret_server.kill();
    world.identity.kill();
}

fn finish_files(world: &World, cases: &[Value]) {
    write_private(&world.state.join("witness"), world.rotated.as_bytes());
    fs::write(
        world.state.join("run.json"),
        serde_json::to_vec(&json!({
            "secret_ref": "secret://lab/alpha/builder/db",
            "version": 2,
            "previous_version": 1,
            "project_id": "alpha",
            "namespace": "builder",
            "workload": "urn:oo:workload:builder.one",
            "deployment": "lab"
        }))
        .expect("run"),
    )
    .expect("run write");
    if let Ok(path) = std::env::var("OO_A384_PROOF") {
        let proof = json!({"cases": cases, "customer_kms": "external_gate", "mtls_ca": "not_issued"});
        fs::write(path, serde_json::to_vec(&proof).expect("proof")).expect("proof");
    }
    mark(&world.state, "live_secret=write_done");
}

async fn prove_db(world: &mut World, management: &str, cases: &mut Vec<Value>) {
    let revision = world.revision;
    let secret = world.secret.clone();
    let rotated = world.rotated.clone();
    let http = &mut world.http;
    let db = format!("{management}/control/v1/projects/alpha/namespaces/builder/secrets/db");
    let created = mutate(http, "PUT", &db, &write_body(revision, "db-create-key-0001", "urn:oo:workload:builder.one", &secret), None).await;
    assert_eq!(created.status, 201, "create");
    assert_metadata(&created.body, "putSecret", 1, "active");
    absent(&created.body, &secret);
    cases.push(json!({"name": "create", "http": 201, "code": "ok"}));
    let replay = mutate(http, "PUT", &db, &write_body(revision, "db-create-key-0001", "urn:oo:workload:builder.one", &secret), None).await;
    assert_eq!(replay.status, 200, "replay");
    assert_eq!(replay.body["version"], 1);
    absent(&replay.body, &secret);
    cases.push(json!({"name": "idempotent_replay", "http": 200, "code": "ok"}));
    let clash = mutate(http, "PUT", &db, &write_body(revision, "db-create-key-0001", "urn:oo:workload:builder.one", &rotated), None).await;
    assert_eq!(code(&clash.body), "idempotency.conflict");
    absent(&clash.body, &rotated);
    cases.push(json!({"name": "idempotency_conflict", "http": clash.status, "code": "idempotency.conflict"}));
    let again = mutate(http, "PUT", &db, &write_body(revision, "db-create-key-0002", "urn:oo:workload:builder.one", &secret), None).await;
    assert_eq!(code(&again.body), "version.conflict");
    cases.push(json!({"name": "write_existing", "http": again.status, "code": "version.conflict"}));
    let status = http.call("GET", &db, None, false, None).await;
    assert_eq!(status.status, 200, "status");
    assert_eq!(status.body["operation"], "getSecretStatus");
    assert_eq!(status.body["subject"], "fixture-admin");
    assert!(status.body.get("value").is_none(), "status has no value");
    absent(&status.body, &secret);
    cases.push(json!({"name": "status", "http": 200, "code": "ok"}));
    let rotated_body = rotate_body(revision, "db-rotate-key-0001", "urn:oo:workload:builder.one", 1, &rotated);
    let turned = mutate(http, "POST", &format!("{db}/rotate"), &rotated_body, None).await;
    assert_eq!(turned.status, 200, "rotate");
    assert_eq!(turned.body["version"], 2);
    absent(&turned.body, &rotated);
    cases.push(json!({"name": "rotate", "http": 200, "code": "ok"}));
    let stale = mutate(http, "POST", &format!("{db}/rotate"), &rotate_body(revision, "db-rotate-key-0002", "urn:oo:workload:builder.one", 1, &secret), None).await;
    assert_eq!(code(&stale.body), "version.conflict");
    cases.push(json!({"name": "stale_rotate", "http": stale.status, "code": "version.conflict"}));
}

async fn prove_open(world: &mut World, management: &str, component: &str, cases: &mut Vec<Value>) {
    let revision = world.revision;
    let secret = world.secret.clone();
    let rotated = world.rotated.clone();
    let agent_secret = world.agent_secret.clone();
    let state = world.state.clone();
    let ca = world.material.ca.clone();
    let session = world.http.session.clone().unwrap_or_default();
    let builder = component_client(&ca, &fs::read(state.join("builder.crt")).expect("cert"), &fs::read(state.join("builder.pkcs8")).expect("key"));
    let agent = component_client(&ca, &fs::read(state.join("agent.crt")).expect("cert"), &fs::read(state.join("agent.pkcs8")).expect("key"));
    let opened = resolve(&builder, component, "secret://lab/alpha/builder/db", "alpha", "builder", None, Some(session.as_str())).await;
    assert_eq!(opened.status, 200, "resolve");
    assert_eq!(opened.body["value"].as_str().unwrap_or(""), rotated.as_str(), "opened current");
    assert_eq!(opened.body["customer_kms"], "external_gate");
    assert_eq!(opened.body["mtls_ca"], "not_issued");
    cases.push(json!({"name": "resolve", "http": 200, "code": "ok"}));
    let retired = resolve(&builder, component, "secret://lab/alpha/builder/db", "alpha", "builder", Some(1), None).await;
    assert_eq!(code(&retired.body), "version.retired");
    absent(&retired.body, &secret);
    absent(&retired.body, &rotated);
    cases.push(json!({"name": "retired", "http": retired.status, "code": "version.retired"}));
    let agent_path = format!("{management}/control/v1/projects/alpha/namespaces/agent/secrets/token");
    let agent_write = mutate(&mut world.http, "PUT", &agent_path, &write_body(revision, "agent-create-key-01", "urn:oo:workload:agent.one", &agent_secret), None).await;
    assert_eq!(agent_write.status, 201, "agent create");
    absent(&agent_write.body, &agent_secret);
    let cross = resolve(&builder, component, "secret://lab/alpha/agent/token", "alpha", "agent", None, None).await;
    assert_eq!(code(&cross.body), "workload.rejected");
    absent(&cross.body, &agent_secret);
    let cross_back = resolve(&agent, component, "secret://lab/alpha/builder/db", "alpha", "builder", None, None).await;
    assert_eq!(code(&cross_back.body), "workload.rejected");
    absent(&cross_back.body, &rotated);
    cases.push(json!({"name": "cross_namespace", "http": cross.status, "code": "workload.rejected"}));
    let project_mismatch = resolve(&builder, component, "secret://lab/alpha/builder/db", "beta", "builder", None, None).await;
    assert_eq!(code(&project_mismatch.body), "forbidden");
    let namespace_mismatch = resolve(&builder, component, "secret://lab/alpha/builder/db", "alpha", "agent", None, None).await;
    assert_eq!(code(&namespace_mismatch.body), "namespace.rejected");
    let other_deployment = resolve(&builder, component, "secret://other/alpha/builder/db", "alpha", "builder", None, None).await;
    assert_eq!(code(&other_deployment.body), "not_found");
    cases.push(json!({"name": "scope_mismatch", "http": 403, "code": "forbidden"}));
}

async fn prove_controls(world: &mut World, management: &str, cases: &mut Vec<Value>) {
    let csrf = world.csrf.clone();
    let revision = world.revision;
    let secret = world.secret.clone();
    let http = &mut world.http;
    let db = format!("{management}/control/v1/projects/alpha/namespaces/builder/secrets/db");
    let unauthenticated = http_raw("PUT", &db, &write_body(revision, "db-create-key-0003", "urn:oo:workload:builder.one", &secret), None, Some(&csrf), Some(&http.origin)).await;
    assert_eq!(unauthenticated.status, 401, "no cookie");
    cases.push(json!({"name": "unauthenticated", "http": 401, "code": code(&unauthenticated.body)}));
    let bad_csrf = mutate(http, "POST", &format!("{db}/rotate"), &rotate_body(revision, "db-rotate-key-0003", "urn:oo:workload:builder.one", 2, &secret), Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")).await;
    assert_eq!(code(&bad_csrf.body), "csrf.rejected");
    cases.push(json!({"name": "csrf", "http": bad_csrf.status, "code": "csrf.rejected"}));
    let bad_origin = http_raw("POST", &format!("{db}/rotate"), &rotate_body(revision, "db-rotate-key-0004", "urn:oo:workload:builder.one", 2, &secret), http.session.as_deref(), Some(&csrf), Some("https://evil.example")).await;
    assert_eq!(code(&bad_origin.body), "origin.rejected");
    cases.push(json!({"name": "origin", "http": bad_origin.status, "code": "origin.rejected"}));
    let bad_revision = mutate(http, "POST", &format!("{db}/rotate"), &rotate_body(revision + 1, "db-rotate-key-0005", "urn:oo:workload:builder.one", 2, &secret), None).await;
    assert_eq!(code(&bad_revision.body), "revision.conflict");
    cases.push(json!({"name": "revision", "http": bad_revision.status, "code": "revision.conflict"}));
    let beta = mutate(http, "PUT", &format!("{management}/control/v1/projects/beta/namespaces/builder/secrets/db"), &write_body(revision, "beta-create-key-01", "urn:oo:workload:builder.one", &secret), None).await;
    assert_eq!(code(&beta.body), "forbidden");
    cases.push(json!({"name": "cross_project", "http": beta.status, "code": "forbidden"}));
    let query = http.call("GET", &format!("{db}?x=1"), None, false, None).await;
    assert_eq!(query.status, 400, "query");
    cases.push(json!({"name": "query", "http": 400, "code": code(&query.body)}));
}

async fn prove_race(world: &mut World, management: &str, cases: &mut Vec<Value>) {
    let revision = world.revision;
    let path = format!("{management}/control/v1/projects/alpha/namespaces/builder/secrets/race");
    let created = mutate(&mut world.http, "PUT", &path, &write_body(revision, "race-create-key-01", "urn:oo:workload:builder.one", "dddddddddddddddddddddddddddddddd"), None).await;
    assert_eq!(created.status, 201, "race create");
    let mismatch = mutate(&mut world.http, "POST", &format!("{path}/rotate"), &rotate_body(revision, "race-mismatch-key1", "urn:oo:workload:agent.one", 1, "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"), None).await;
    assert_eq!(code(&mismatch.body), "workload.rejected");
    cases.push(json!({"name": "rotate_workload", "http": mismatch.status, "code": "workload.rejected"}));
    let password = read_trimmed(Path::new(&world.env.password_file));
    assert_eq!(version_count(&world.env, &password, "race").await, (1, 1), "mismatch did not write");
    let origin = world.http.origin.clone();
    let session = world.http.session.clone().expect("session");
    let csrf = fresh_csrf(&mut world.http).await;
    let rotate_url = format!("{path}/rotate");
    let left_body = rotate_body(revision, "race-rotate-key-0a", "urn:oo:workload:builder.one", 1, "ffffffffffffffffffffffffffffffff");
    let right_body = rotate_body(revision, "race-rotate-key-0b", "urn:oo:workload:builder.one", 1, "gggggggggggggggggggggggggggggggg");
    let left = http_raw("POST", &rotate_url, &left_body, Some(session.as_str()), Some(csrf.as_str()), Some(origin.as_str()));
    let right = http_raw("POST", &rotate_url, &right_body, Some(session.as_str()), Some(csrf.as_str()), Some(origin.as_str()));
    let (left, right) = tokio::join!(left, right);
    let won = [left.status, right.status].into_iter().filter(|status| *status == 200).count();
    let lost = [code(&left.body), code(&right.body)].into_iter().filter(|item| item == "csrf.rejected").count();
    assert_eq!((won, lost), (1, 1), "peer csrf rejected");
    assert_eq!(version_count(&world.env, &password, "race").await, (2, 2), "loser did not insert");
    cases.push(json!({"name": "parallel_rotate", "http": 200, "code": "csrf.rejected"}));
}

async fn prove_revoke(world: &mut World, management: &str, component: &str, cases: &mut Vec<Value>) {
    let revision = world.revision;
    let secret = world.secret.clone();
    let state = world.state.clone();
    let ca = world.material.ca.clone();
    let builder = component_client(&ca, &fs::read(state.join("builder.crt")).expect("cert"), &fs::read(state.join("builder.pkcs8")).expect("key"));
    let gone = format!("{management}/control/v1/projects/alpha/namespaces/builder/secrets/gone");
    let gone_write = mutate(&mut world.http, "PUT", &gone, &write_body(revision, "gone-create-key-01", "urn:oo:workload:builder.one", &secret), None).await;
    assert_eq!(gone_write.status, 201, "gone create");
    let revoked = mutate(&mut world.http, "POST", &format!("{gone}/revoke"), &json!({"identity_revision": revision, "idempotency_key": "gone-revoke-key-01"}).to_string(), None).await;
    assert_eq!(revoked.status, 200, "revoke");
    assert_eq!(revoked.body["state"], "revoked");
    absent(&revoked.body, &secret);
    let refused = resolve(&builder, component, "secret://lab/alpha/builder/gone", "alpha", "builder", None, None).await;
    assert_eq!(code(&refused.body), "revoked");
    absent(&refused.body, &secret);
    cases.push(json!({"name": "revoke", "http": refused.status, "code": "revoked"}));
}

async fn role_denials(http: &mut Http, idp: &idp::Idp, issuer: &str, management: &str, cases: &mut Vec<Value>) {
    for (subject, project) in [("fixture-reviewer", "beta"), ("fixture-owner", "epsilon")] {
        idp.set_subject(subject);
        let callback = login(http, issuer, "/control/v1/auth/login?return_to=%2Fapp").await;
        assert_eq!(callback.status, 302, "role login");
        let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
        assert_eq!(current.status, 200, "role current");
        let role_revision = current.body["revision"].as_i64().expect("revision");
        let path = format!("{management}/control/v1/projects/{project}/namespaces/builder/secrets/db");
        let denied = mutate(http, "PUT", &path, &write_body(role_revision, "role-deny-key-0001", "urn:oo:workload:builder.one", "0123456789abcdef0123456789abcdef"), None).await;
        assert_eq!(code(&denied.body), "forbidden");
        cases.push(json!({"name": subject, "http": denied.status, "code": "forbidden"}));
    }
}

async fn half_write(env: &Env, state: &Path, material: &Material, http: &mut Http, revision: i64) {
    let config_path = state.join("scripted.json");
    write_secret_config(&config_path, env, state, material, 0, 0);
    let config = Arc::new(load_config(&config_path).expect("scripted config"));
    let (management, component) = listen(&config).await.expect("bind");
    let management_addr = management.local_addr().expect("mgmt");
    let nonces = Arc::new(ScriptedNonce::new());
    nonces.push([1_u8; 12]).expect("nonce");
    nonces.push([1_u8; 12]).expect("nonce");
    nonces.push([2_u8; 12]).expect("nonce");
    let mut server = spawn_server(Arc::clone(&config), management, component, nonces);
    server.ready().await.expect("ready");
    let management_origin = format!("https://{management_addr}");
    let path = format!("{management_origin}/control/v1/projects/alpha/namespaces/builder/secrets/half");
    let session = http.session.clone().expect("session");
    let caller_origin = http.origin.clone();
    let csrf = fresh_csrf(http).await;
    let created = http_raw("PUT", &path, &write_body(revision, "half-create-key-01", "urn:oo:workload:builder.one", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), Some(&session), Some(&csrf), Some(&caller_origin)).await;
    assert_eq!(created.status, 201, "half create");
    let csrf = fresh_csrf(http).await;
    let collided = http_raw(
        "POST",
        &format!("{path}/rotate"),
        &rotate_body(revision, "half-rotate-key-01", "urn:oo:workload:builder.one", 1, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        Some(&session),
        Some(&csrf),
        Some(&caller_origin),
    )
    .await;
    assert_eq!(code(&collided.body), "version.conflict");
    let password = read_trimmed(Path::new(&env.password_file));
    let (version, count) = version_count(env, &password, "half").await;
    assert_eq!(version, 1, "collision did not activate");
    assert_eq!(count, 1, "collision did not insert");
    let csrf = fresh_csrf(http).await;
    let turned = http_raw(
        "POST",
        &format!("{path}/rotate"),
        &rotate_body(revision, "half-rotate-key-02", "urn:oo:workload:builder.one", 1, "cccccccccccccccccccccccccccccccc"),
        Some(&session),
        Some(&csrf),
        Some(&caller_origin),
    )
    .await;
    assert_eq!(turned.status, 200, "scripted rotate");
    assert_eq!(turned.body["version"], 2);
    let (version, count) = version_count(env, &password, "half").await;
    assert_eq!((version, count), (2, 2), "one new version");
    server.stop().await.expect("stop");
}

async fn restart_phase() {
    let env = Env::load();
    let state = PathBuf::from(&env.state);
    let run: Value = serde_json::from_slice(&fs::read(state.join("run.json")).expect("run")).expect("json");
    let witness = fs::read(state.join("witness")).expect("witness");
    let material = Material {
        ca: fs::read(state.join("ca.pem")).expect("ca"),
        cert: fs::read(state.join("srv.pem")).expect("cert"),
        key: fs::read(state.join("srv.key")).expect("key"),
    };
    let secret_config = state.join("secret.json");
    write_secret_config(&secret_config, &env, &state, &material, 0, 0);
    let secret_bin = secret_bin();
    let mut secret_server = ServerProc::spawn(&secret_bin, &secret_config);
    let listening = secret_server.listening().await;
    let component = format!("https://{}", listening["component_addr"].as_str().expect("comp"));
    let builder = component_client(&material.ca, &fs::read(state.join("builder.crt")).expect("cert"), &fs::read(state.join("builder.pkcs8")).expect("key"));
    let reference = run["secret_ref"].as_str().expect("ref");
    let project = run["project_id"].as_str().expect("project");
    let namespace = run["namespace"].as_str().expect("namespace");
    let previous = run["previous_version"].as_i64().expect("previous");
    let opened = resolve(&builder, &component, reference, project, namespace, None, None).await;
    assert_eq!(opened.status, 200, "restart resolve");
    assert_eq!(opened.body["value"].as_str().unwrap_or("").as_bytes(), witness.as_slice(), "restart opened current");
    assert_eq!(opened.body["version"], run["version"]);
    let retired = resolve(&builder, &component, reference, project, namespace, Some(previous), None).await;
    assert_eq!(code(&retired.body), "version.retired");
    let witness_text = String::from_utf8_lossy(&witness);
    absent(&retired.body, &witness_text);
    let stderr = secret_server.stderr_text();
    assert!(!stderr.contains(witness_text.as_ref()), "restart stderr");
    secret_server.kill();
    mark(&state, "live_secret=done");
}

struct Env {
    host: String,
    port: u16,
    password_file: String,
    ca_file: String,
    state: String,
    master_key: String,
    identity_bin: String,
}

impl Env {
    fn load() -> Self {
        Self {
            host: env_var("OO_A384_PG_HOST"),
            port: env_var("OO_A384_PG_PORT").parse().expect("port"),
            password_file: env_var("OO_A384_PG_PASSWORD_FILE"),
            ca_file: env_var("OO_A384_PG_CA"),
            state: env_var("OO_A384_STATE"),
            master_key: env_var("OO_A384_MASTER_KEY"),
            identity_bin: env_var("OO_A384_IDENTITY_BIN"),
        }
    }
}

struct Material {
    ca: Vec<u8>,
    cert: Vec<u8>,
    key: Vec<u8>,
}

struct Reply {
    status: u16,
    location: String,
    body: Value,
    raw: String,
}

struct Http {
    client: reqwest::Client,
    origin: String,
    login: Option<String>,
    session: Option<String>,
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
        let response = request.send().await.unwrap_or_else(|error| panic!("request failed: {}", error.without_url()));
        self.absorb(&response);
        reply_from(response).await
    }

    async fn get_external(&mut self, url: &str) -> Reply {
        let response = self.client.get(url).send().await.unwrap_or_else(|error| panic!("external failed: {}", error.without_url()));
        self.absorb(&response);
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

struct ServerProc {
    child: Option<std::process::Child>,
    stdout: Arc<Mutex<Vec<u8>>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<()>>,
}

impl ServerProc {
    fn spawn(exe: &str, config: &Path) -> Self {
        let mut child = Command::new(exe)
            .args(["serve", "--config", config.to_str().expect("path")])
            .env_clear()
            .env("HOME", "/tmp")
            .env("PATH", "/usr/bin:/bin:/usr/local/bin")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn");
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let stdout_thread = pump(child.stdout.take().expect("stdout"), Arc::clone(&stdout));
        let stderr_thread = pump(child.stderr.take().expect("stderr"), Arc::clone(&stderr));
        Self { child: Some(child), stdout, stderr, stdout_thread: Some(stdout_thread), stderr_thread: Some(stderr_thread) }
    }

    async fn listening(&mut self) -> Value {
        let stdout = Arc::clone(&self.stdout);
        let started = std::time::Instant::now();
        loop {
            let bytes = stdout.lock().expect("stdout").clone();
            if let Some(line) = bytes.split(|byte| *byte == b'\n').next()
                && !line.is_empty()
                && let Ok(value) = serde_json::from_slice::<Value>(line)
            {
                return value;
            }
            if let Some(status) = self.child.as_mut().and_then(|child| child.try_wait().ok().flatten()) {
                let err = String::from_utf8_lossy(&self.stderr.lock().expect("stderr")).into_owned();
                panic!("server exited {status} stderr={err}");
            }
            if started.elapsed() > Duration::from_secs(20) {
                let err = String::from_utf8_lossy(&self.stderr.lock().expect("stderr")).into_owned();
                panic!("server did not listen stderr={err}");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout.lock().expect("stdout")).into_owned()
    }

    fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr.lock().expect("stderr")).into_owned()
    }

    fn kill(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(thread) = self.stdout_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for ServerProc {
    fn drop(&mut self) {
        self.kill();
    }
}

fn pump(mut pipe: impl Read + Send + 'static, sink: Arc<Mutex<Vec<u8>>>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0_u8; 1024];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(count) => sink.lock().expect("pump").extend_from_slice(&buf[..count]),
            }
        }
    })
}

async fn login(http: &mut Http, issuer: &str, path: &str) -> Reply {
    let started = http.call("GET", path, None, false, None).await;
    assert_eq!(started.status, 302, "authorize");
    assert!(started.location.starts_with(issuer), "issuer");
    assert!(!started.location.contains("bootstrap"), "bootstrap stripped");
    let idp_reply = http.get_external(&started.location).await;
    assert_eq!(idp_reply.status, 302, "idp");
    let callback = idp_reply.location.trim_start_matches(&http.origin).to_string();
    http.call("GET", &callback, None, false, None).await
}

async fn fresh_csrf(http: &mut Http) -> String {
    let current = http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(current.status, 200, "csrf refresh");
    current.body["csrf_token"].as_str().expect("csrf").to_string()
}

async fn mutate(http: &mut Http, method: &str, url: &str, body: &str, csrf: Option<&str>) -> Reply {
    let token = match csrf {
        Some(value) => value.to_string(),
        None => fresh_csrf(http).await,
    };
    http.call(method, url, Some(body), true, Some(&token)).await
}

async fn http_raw(method: &str, url: &str, body: &str, session: Option<&str>, csrf: Option<&str>, origin_header: Option<&str>) -> Reply {
    let certs = reqwest::Certificate::from_pem_bundle(&fs::read(std::env::var("OO_A384_STATE").expect("state") + "/ca.pem").expect("ca")).expect("ca");
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .https_only(true)
        .http1_only()
        .tls_certs_only(certs)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("raw");
    let mut request = client.request(method.parse().expect("method"), url);
    if let Some(session) = session {
        request = request.header("cookie", format!("__Host-oo-session={session}"));
    }
    if let Some(csrf) = csrf {
        request = request.header("x-oo-csrf", csrf);
    }
    if let Some(origin_header) = origin_header {
        request = request.header("origin", origin_header);
    }
    request = request.header("content-type", "application/json").body(body.to_string());
    let response = request.send().await.unwrap_or_else(|error| panic!("raw request failed: {}", error.without_url()));
    reply_from(response).await
}

async fn resolve(client: &reqwest::Client, component: &str, secret_ref: &str, project: &str, namespace: &str, version: Option<i64>, session: Option<&str>) -> Reply {
    let mut body = json!({"secret_ref": secret_ref, "project_id": project, "namespace": namespace});
    if let Some(version) = version {
        body["version"] = json!(version);
    }
    let mut request = client.post(format!("{component}/component/v1/resolve")).header("content-type", "application/json").header("x-oo-workload", "urn:oo:workload:agent.one");
    if let Some(session) = session {
        request = request.header("cookie", format!("__Host-oo-session={session}"));
    }
    let response = request.body(body.to_string()).send().await.unwrap_or_else(|error| panic!("resolve failed: {}", error.without_url()));
    reply_from(response).await
}

fn component_client(ca: &[u8], cert: &[u8], key: &[u8]) -> reqwest::Client {
    let mut pem = cert.to_vec();
    if !pem.ends_with(b"\n") {
        pem.push(b'\n');
    }
    pem.extend_from_slice(key);
    let identity = reqwest::Identity::from_pem(&pem).expect("identity");
    let certs = reqwest::Certificate::from_pem_bundle(ca).expect("ca");
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .https_only(true)
        .http1_only()
        .tls_certs_only(certs)
        .identity(identity)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("component")
}

async fn reply_from(response: reqwest::Response) -> Reply {
    let status = response.status().as_u16();
    let location = response.headers().get(reqwest::header::LOCATION).and_then(|value| value.to_str().ok()).unwrap_or("").to_string();
    let raw = response.text().await.unwrap_or_default();
    let body = serde_json::from_str(&raw).unwrap_or(Value::Null);
    Reply { status, location, body, raw }
}

fn write_body(revision: i64, key: &str, workload: &str, value: &str) -> String {
    json!({"identity_revision": revision, "idempotency_key": key, "workload": workload, "value": value}).to_string()
}

fn rotate_body(revision: i64, key: &str, workload: &str, expected: i64, value: &str) -> String {
    json!({"identity_revision": revision, "idempotency_key": key, "workload": workload, "expected_version": expected, "value": value}).to_string()
}

fn assert_metadata(body: &Value, operation: &str, version: i64, state: &str) {
    assert_eq!(body["contract"], "oo.secret-control/v1");
    assert_eq!(body["operation"], operation);
    assert_eq!(body["version"], version);
    assert_eq!(body["state"], state);
    assert_eq!(body["customer_kms"], "external_gate");
    assert_eq!(body["mtls_ca"], "not_issued");
    assert!(body.get("value").is_none(), "no value");
}

fn absent(body: &Value, secret: &str) {
    assert!(!body.to_string().contains(secret), "response contained the secret");
}

fn code(body: &Value) -> String {
    body["error"]["code"].as_str().unwrap_or("").to_string()
}

fn identity_commit(issuer: &str, callback: &str) -> String {
    let mut rows = [
        ("alpha", "fixture-admin", "admin"),
        ("beta", "fixture-reviewer", "reviewer"),
        ("delta", "fixture-auditor", "auditor"),
        ("epsilon", "fixture-owner", "business_owner"),
        ("gamma", "fixture-steward", "steward"),
    ];
    rows.sort();
    let value = json!({
        "audience": "oo-control",
        "bindings": rows.iter().map(|(project, subject, role)| json!({"project_id": project, "role": role, "subject": subject})).collect::<Vec<_>>(),
        "bootstrap": {"customer_owner": false, "fixture_class": "explicit_test_fixture", "project_id": "alpha", "role": "admin", "subject": "fixture-admin"},
        "client_id": "oo-control-client",
        "issuer": issuer,
        "protocol": "oo.control-identity/config-commit/v1",
        "redirect_uris": [callback],
        "revision": 1
    });
    sha256_hex(serde_json::to_string(&value).expect("preimage").as_bytes())
}

fn write_identity_config(state: &Path, env: &Env, origin: &str, issuer: &str, callback: &str, port: u16) {
    let body = json!({
        "https_origin": origin,
        "listen_host": "127.0.0.1",
        "listen_port": port,
        "tls_cert_file": state.join("srv.pem"),
        "tls_key_file": state.join("srv.key"),
        "issuer": issuer,
        "audience": "oo-control",
        "client_id": "oo-control-client",
        "client_secret_file": state.join("client.secret"),
        "redirect_uris": [callback],
        "config_commit": identity_commit(issuer, callback),
        "revision": 1,
        "idp_ca_file": state.join("ca.pem"),
        "pg": {
            "host": env.host,
            "port": env.port,
            "database": "identity",
            "user": "oo_identity_admin",
            "password_file": env.password_file,
            "ca_file": env.ca_file
        },
        "bindings": [
            {"subject": "fixture-admin", "project_id": "alpha", "role": "admin"},
            {"subject": "fixture-reviewer", "project_id": "beta", "role": "reviewer"},
            {"subject": "fixture-steward", "project_id": "gamma", "role": "steward"},
            {"subject": "fixture-auditor", "project_id": "delta", "role": "auditor"},
            {"subject": "fixture-owner", "project_id": "epsilon", "role": "business_owner"}
        ],
        "bootstrap": {"subject": "fixture-admin", "project_id": "alpha", "role": "admin"},
        "idle_seconds": 300,
        "absolute_seconds": 300,
        "oauth_ttl_seconds": 60,
        "bootstrap_ttl_seconds": 120,
        "clock_skew_seconds": 0
    });
    fs::write(state.join("identity.json"), serde_json::to_vec_pretty(&body).expect("identity")).expect("write");
}

fn write_secret_config(path: &Path, env: &Env, state: &Path, _material: &Material, management: u16, component: u16) {
    let body = json!({
        "https_origin": read_origin(state),
        "listen_host": "127.0.0.1",
        "management_port": management,
        "component_port": component,
        "tls_cert_file": state.join("srv.pem"),
        "tls_key_file": state.join("srv.key"),
        "client_ca_file": state.join("ca.pem"),
        "identity_origin": read_origin(state),
        "identity_ca_file": state.join("ca.pem"),
        "deployment": "lab",
        "key_source": "file",
        "master_key_file": env.master_key,
        "pg": {
            "host": env.host,
            "port": env.port,
            "database": "secret",
            "user": "oo_secret_admin",
            "password_file": env.password_file,
            "ca_file": env.ca_file
        }
    });
    fs::write(path, serde_json::to_vec_pretty(&body).expect("secret config")).expect("write");
}

fn read_origin(state: &Path) -> String {
    let bytes = fs::read(state.join("identity.json")).unwrap_or_default();
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    value["https_origin"].as_str().unwrap_or("https://127.0.0.1:1").to_string()
}

fn certs(dir: &Path) -> Material {
    openssl(dir, &["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", "ca.key", "-out", "ca.pem", "-days", "2", "-subj", "/CN=oo-a384-test-ca"]);
    openssl(dir, &["req", "-newkey", "rsa:2048", "-nodes", "-keyout", "srv.key", "-out", "srv.csr", "-subj", "/CN=127.0.0.1"]);
    fs::write(dir.join("srv.ext"), "subjectAltName=IP:127.0.0.1\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n").expect("ext");
    openssl(dir, &["x509", "-req", "-in", "srv.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial", "-out", "srv.pem", "-days", "2", "-extfile", "srv.ext"]);
    openssl(dir, &["genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", "k1.pem"]);
    openssl(dir, &["pkcs8", "-topk8", "-nocrypt", "-in", "k1.pem", "-outform", "DER", "-out", "k1.der"]);
    sign_client(dir, "builder", "urn:oo:workload:builder.one");
    sign_client(dir, "agent", "urn:oo:workload:agent.one");
    Material { ca: fs::read(dir.join("ca.pem")).expect("ca"), cert: fs::read(dir.join("srv.pem")).expect("cert"), key: fs::read(dir.join("srv.key")).expect("key") }
}

fn sign_client(dir: &Path, name: &str, workload: &str) {
    let key = format!("{name}.key");
    let csr = format!("{name}.csr");
    let crt = format!("{name}.crt");
    let ext = format!("{name}.ext");
    openssl(dir, &["ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", &key]);
    openssl(dir, &["req", "-new", "-key", &key, "-out", &csr, "-subj", &format!("/CN=oo-a384-{name}")]);
    fs::write(dir.join(&ext), format!("subjectAltName=URI:{workload}\nextendedKeyUsage=clientAuth\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\n")).expect("ext");
    openssl(dir, &["x509", "-req", "-in", &csr, "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial", "-out", &crt, "-days", "2", "-extfile", &ext]);
    openssl(dir, &["pkcs8", "-topk8", "-nocrypt", "-in", &key, "-out", &format!("{name}.pkcs8")]);
}

fn chmod_private(dir: &Path) {
    for name in ["srv.key", "ca.key", "k1.pem", "k1.der", "builder.key", "builder.pkcs8", "agent.key", "agent.pkcs8", "client.secret"] {
        let path = dir.join(name);
        if path.exists() {
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
    }
}

fn openssl(dir: &Path, args: &[&str]) {
    let output = Command::new("openssl").args(args).current_dir(dir).output().expect("openssl");
    assert!(output.status.success(), "openssl failed");
}

fn openssl_hex() -> String {
    let output = Command::new("openssl").args(["rand", "-hex", "32"]).output().expect("rand");
    assert!(output.status.success(), "rand");
    String::from_utf8(output.stdout).expect("hex").trim().to_string()
}

fn write_secret(path: &Path, value: &str) {
    write_private(path, format!("{value}\n").as_bytes());
}

fn write_private(path: &Path, bytes: &[u8]) {
    let _ = fs::remove_file(path);
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(path).expect("private");
    file.write_all(bytes).expect("write");
    file.set_permissions(fs::Permissions::from_mode(0o600)).expect("mode");
}

fn read_trimmed(path: &Path) -> String {
    let mut text = fs::read_to_string(path).expect("read");
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    text
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("port");
    listener.local_addr().expect("addr").port()
}

fn cli(exe: &str, args: &[&str]) -> Cli {
    let output = Command::new(exe).args(args).env_clear().env("HOME", "/tmp").env("PATH", "/usr/bin:/bin:/usr/local/bin").output().expect("cli");
    let status = output.status.code().unwrap_or(1) as u8;
    let raw = String::from_utf8_lossy(&output.stdout).into_owned();
    let body = serde_json::from_str(raw.trim()).unwrap_or(Value::Null);
    assert!(output.stderr.is_empty(), "cli stderr");
    Cli { status, body, raw }
}

struct Cli {
    status: u8,
    body: Value,
    raw: String,
}

fn secret_bin() -> String {
    std::env::var("OO_A384_SECRET_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_secret-control").to_string())
}

async fn pg(env: &Env, password: &str) -> tokio_postgres::Client {
    let mut config = tokio_postgres::Config::new();
    config.user("oo_secret_admin");
    config.password(password);
    config.host(&env.host);
    config.port(env.port);
    config.dbname("secret");
    config.ssl_mode(SslMode::Require);
    let ca = fs::read(&env.ca_file).expect("pg ca");
    let tls = pg_tls(&ca).expect("tls");
    let (client, connection) = config.connect(tls).await.expect("pg");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn read_ciphertext(env: &Env, password: &str, secret_id: &str, version: i32) -> Vec<u8> {
    let client = pg(env, password).await;
    let row = client
        .query_one("SELECT ciphertext FROM oo_secret_control.secret_version WHERE secret_id = $1::text AND version = $2::int4", &[&secret_id, &version])
        .await
        .expect("ciphertext");
    row.get(0)
}

async fn read_nonce(env: &Env, password: &str, secret_id: &str, version: i32) -> Vec<u8> {
    let client = pg(env, password).await;
    let row = client
        .query_one("SELECT nonce FROM oo_secret_control.secret_version WHERE secret_id = $1::text AND version = $2::int4", &[&secret_id, &version])
        .await
        .expect("nonce");
    row.get(0)
}

async fn version_count(env: &Env, password: &str, secret_id: &str) -> (i32, i64) {
    let client = pg(env, password).await;
    let version: i32 = client
        .query_one("SELECT current_version FROM oo_secret_control.secret_head WHERE secret_id = $1::text", &[&secret_id])
        .await
        .expect("head")
        .get(0);
    let count: i64 = client
        .query_one("SELECT COUNT(*)::bigint FROM oo_secret_control.secret_version WHERE secret_id = $1::text", &[&secret_id])
        .await
        .expect("count")
        .get(0);
    (version, count)
}

fn mark(state: &Path, line: &str) {
    fs::write(state.join("phase.txt"), format!("{line}\n")).expect("phase");
    println!("{line}");
}

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("missing {name}"))
}
