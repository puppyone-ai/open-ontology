//! Independent card probe. It is compiled onto a private copy of the frozen archive.
//! It does not change product source. The test issuer is a fixture, not a customer IdP.

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

use secret_control_api::{canonical, listen, load_config, open, pg_tls, sha256_hex, spawn_server, Scope, ScriptedNonce};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_secret_card() {
    match std::env::var("OO_A400_PHASE").ok().as_deref() {
        Some("write") => write_phase().await,
        Some("restart") => restart_phase().await,
        _ => panic!("OO_A400_PHASE must be write or restart"),
    }
}

async fn write_phase() {
    let mut world = boot().await;
    kms_refuses(&world);
    let secret_config = install_secret(&world);
    half_write(&mut world).await;
    let mut secret_server = ServerProc::spawn(&secret_bin(), &secret_config);
    let listening = secret_server.listening().await;
    assert_eq!(listening["customer_kms"], "external_gate", "listening gate");
    assert_eq!(listening["mtls_ca"], "not_issued", "listening ca");
    let management = format!("https://{}", listening["management_addr"].as_str().expect("mgmt"));
    let component = format!("https://{}", listening["component_addr"].as_str().expect("comp"));
    let mut cases = Vec::new();
    prove_writes(&mut world, &management, &mut cases).await;
    prove_resolve_identity(&mut world, &management, &component, &mut cases).await;
    prove_rotation(&mut world, &management, &component, &mut cases).await;
    prove_ciphertext(&world, &secret_config, &mut cases).await;
    prove_revoke(&mut world, &management, &component, &mut cases).await;
    prove_reviewer(&mut world, &management, &mut cases).await;
    finish_write(&world, &cases, &mut secret_server);
}

async fn restart_phase() {
    let env = Env::load();
    let state = PathBuf::from(&env.state);
    let run: Value = serde_json::from_slice(&fs::read(state.join("run.json")).expect("run")).expect("json");
    let witness = fs::read(state.join("witness")).expect("witness");
    let material = Material::read(&state);
    let secret_config = state.join("secret.json");
    write_secret_config(&secret_config, &env, &state);
    let mut secret_server = ServerProc::spawn(&secret_bin(), &secret_config);
    let listening = secret_server.listening().await;
    assert_eq!(listening["mtls_ca"], "not_issued");
    let component = format!("https://{}", listening["component_addr"].as_str().expect("comp"));
    let builder = component_client(&material.ca, &fs::read(state.join("builder.crt")).expect("crt"), &fs::read(state.join("builder.pkcs8")).expect("key"));
    let reference = run["secret_ref"].as_str().expect("ref");
    let project = run["project_id"].as_str().expect("project");
    let namespace = run["namespace"].as_str().expect("namespace");
    let previous = run["previous_version"].as_i64().expect("previous");
    let opened = resolve_ok(&builder, &component, reference, project, namespace, None).await;
    let got = opened.body["value"].as_str().unwrap_or("");
    assert!(got.as_bytes() == witness.as_slice(), "restart opened the current ciphertext");
    assert_eq!(opened.body["version"], run["version"]);
    assert_eq!(opened.body["workload"], "urn:oo:workload:builder.one", "restart cert grant");
    assert_eq!(opened.body["customer_kms"], "external_gate");
    let retired = resolve_ok(&builder, &component, reference, project, namespace, Some(previous)).await;
    assert_eq!(code(&retired.body), "version.retired");
    assert!(retired.body.get("value").is_none(), "retired has no value");
    assert!(!retired.raw.contains(std::str::from_utf8(&witness).expect("utf8")), "retired hides the value");
    let revoked_ref = run["revoked_ref"].as_str().expect("revoked");
    let revoked = resolve_ok(&builder, &component, revoked_ref, project, namespace, None).await;
    assert_eq!(code(&revoked.body), "revoked");
    assert!(revoked.body.get("value").is_none(), "revoked has no value");
    let proof = json!({
        "current_http": opened.status,
        "current_version": opened.body["version"],
        "retired": "version.retired",
        "revoked": "revoked",
        "value_matched_witness": true,
        "spoof_header_ignored": opened.body["workload"] == "urn:oo:workload:builder.one",
        "customer_kms": "external_gate",
        "mtls_ca": "not_issued",
        "foreign_pool_refresh": "not_claimed"
    });
    fs::write(env_proof("OO_A400_RESTART_PROOF"), serde_json::to_vec_pretty(&proof).expect("proof")).expect("write");
    assert!(!secret_server.stderr_text().contains(std::str::from_utf8(&witness).expect("utf8")), "restart stderr");
    secret_server.kill();
    mark(&state, "live_secret=done");
}

struct World {
    env: Env,
    state: PathBuf,
    material: Material,
    http: Http,
    idp: idp::Idp,
    identity: ServerProc,
    revision: i64,
    issuer: String,
    current_value: String,
    rotated_value: String,
    agent_value: String,
}

async fn boot() -> World {
    let env = Env::load();
    let state = PathBuf::from(&env.state);
    let _ = fs::remove_file(state.join("phase.txt"));
    let _ = fs::remove_file(state.join("run.json"));
    let _ = fs::remove_file(state.join("witness"));
    let material = certs(&state);
    chmod_private(&state);
    let current_value = openssl_hex();
    let rotated_value = openssl_hex();
    let agent_value = openssl_hex();
    assert!(current_value.len() >= 32 && rotated_value.len() >= 32 && agent_value.len() >= 32, "entropy");
    assert_ne!(current_value, rotated_value, "rotation changes the value");
    assert_ne!(current_value, agent_value, "namespaces differ");
    let client_secret = openssl_hex();
    write_secret(&state.join("client.secret"), &client_secret);
    let identity_port = free_port();
    let idp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("idp");
    let idp_port = idp_listener.local_addr().expect("idp addr").port();
    let origin = format!("https://127.0.0.1:{identity_port}");
    let issuer = format!("https://127.0.0.1:{idp_port}");
    let callback = format!("{origin}/control/v1/auth/callback");
    write_identity_config(&state, &env, &origin, &issuer, &callback, identity_port);
    let idp = idp::start(
        idp_listener,
        idp::IdpParts {
            cert_pem: &material.cert,
            key_pem: &material.key,
            issuer: &issuer,
            client_id: "oo-control-client",
            client_secret: &client_secret,
            redirect_uri: &callback,
            der_key: &fs::read(state.join("k1.der")).expect("der"),
        },
    )
    .await;
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
    assert_eq!(current.body["operation"], "getIdentityCurrent");
    let caps = current.body["projects"][0]["capabilities"].as_array().expect("caps");
    assert!(caps.iter().any(|item| item.as_str() == Some("project.admin")), "project.admin");
    let caller = secret_control_api::try_from_caller_json(current.raw.as_bytes()).expect_err("caller json");
    assert_eq!(caller.code, "unauthenticated");
    World {
        revision: current.body["revision"].as_i64().expect("revision"),
        env,
        state,
        material,
        http,
        idp,
        identity,
        issuer,
        current_value,
        rotated_value,
        agent_value,
    }
}

fn kms_refuses(world: &World) {
    let missing = world.state.join("kms-not-a-key");
    let _ = fs::remove_file(&missing);
    let config = world.state.join("kms.json");
    let body = json!({
        "https_origin": "https://127.0.0.1:1",
        "listen_host": "127.0.0.1",
        "management_port": 0,
        "component_port": 0,
        "tls_cert_file": world.state.join("srv.pem"),
        "tls_key_file": world.state.join("srv.key"),
        "client_ca_file": world.state.join("ca.pem"),
        "identity_origin": "https://127.0.0.1:1",
        "identity_ca_file": world.state.join("ca.pem"),
        "deployment": "lab",
        "key_source": "kms",
        "master_key_file": missing,
        "pg": {
            "host": world.env.host,
            "port": world.env.port,
            "database": "secret",
            "user": "oo_secret_admin",
            "password_file": world.env.password_file,
            "ca_file": world.env.ca_file
        }
    });
    fs::write(&config, serde_json::to_vec_pretty(&body).expect("kms")).expect("write");
    let output = Command::new(secret_bin())
        .args(["install", "--config", config.to_str().expect("path")])
        .env_clear()
        .env("HOME", "/tmp")
        .env("PATH", "/usr/bin:/bin:/usr/local/bin")
        .output()
        .expect("kms cli");
    assert_eq!(output.status.code(), Some(2), "kms exit");
    assert!(output.stderr.is_empty(), "kms stderr");
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("kms json");
    assert_eq!(parsed["error"]["code"], "kms.external");
    assert_eq!(parsed["customer_kms"], "external_gate");
    assert_eq!(parsed["mtls_ca"], "not_issued");
    assert!(!missing.exists(), "kms did not create a key file");
}

fn install_secret(world: &World) -> PathBuf {
    let secret_config = world.state.join("secret.json");
    write_secret_config(&secret_config, &world.env, &world.state);
    let secret_bin = secret_bin();
    let installed = cli(&secret_bin, &["install", "--config", secret_config.to_str().expect("path")]);
    assert_eq!(installed.status, 0, "secret install");
    assert_eq!(installed.body["disposition"], "committed");
    assert_eq!(installed.body["customer_kms"], "external_gate");
    assert_eq!(installed.body["mtls_ca"], "not_issued");
    let replay = cli(&secret_bin, &["install", "--config", secret_config.to_str().expect("path")]);
    assert_eq!(replay.body["disposition"], "replayed");
    let mode = fs::metadata(&world.env.master_key).expect("key").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "master key mode");
    secret_config
}

async fn half_write(world: &mut World) {
    let config_path = world.state.join("scripted.json");
    write_secret_config(&config_path, &world.env, &world.state);
    let config = Arc::new(load_config(&config_path).expect("scripted config"));
    let (management, component) = listen(&config).await.expect("bind");
    let management_addr = management.local_addr().expect("mgmt");
    let nonces = Arc::new(ScriptedNonce::new());
    nonces.push([1_u8; 12]).expect("nonce");
    nonces.push([1_u8; 12]).expect("nonce");
    nonces.push([2_u8; 12]).expect("nonce");
    let mut server = spawn_server(Arc::clone(&config), management, component, nonces);
    server.ready().await.expect("ready");
    let origin = format!("https://{management_addr}");
    let path = format!("{origin}/control/v1/projects/alpha/namespaces/builder/secrets/half");
    let created = mutate(&mut world.http, "PUT", &path, &write_body(world.revision, "half-create-key-01", "urn:oo:workload:builder.one", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), None).await;
    assert_eq!(created.status, 201, "half create");
    assert!(created.body.get("value").is_none(), "half metadata");
    let collided = mutate(
        &mut world.http,
        "POST",
        &format!("{path}/rotate"),
        &rotate_body(world.revision, "half-rotate-key-01", "urn:oo:workload:builder.one", 1, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        None,
    )
    .await;
    assert_eq!(code(&collided.body), "version.conflict");
    let password = read_trimmed(Path::new(&world.env.password_file));
    assert_eq!(version_count(&world.env, &password, "builder", "half").await, (1, 1), "collision did not activate");
    let turned = mutate(
        &mut world.http,
        "POST",
        &format!("{path}/rotate"),
        &rotate_body(world.revision, "half-rotate-key-02", "urn:oo:workload:builder.one", 1, "cccccccccccccccccccccccccccccccc"),
        None,
    )
    .await;
    assert_eq!(turned.status, 200, "scripted rotate");
    assert_eq!(turned.body["version"], 2);
    assert_eq!(version_count(&world.env, &password, "builder", "half").await, (2, 2), "one new version");
    server.stop().await.expect("stop");
}

async fn prove_writes(world: &mut World, management: &str, cases: &mut Vec<Value>) {
    let revision = world.revision;
    let db = format!("{management}/control/v1/projects/alpha/namespaces/builder/secrets/db");
    let created = mutate(&mut world.http, "PUT", &db, &write_body(revision, "db-create-key-0001", "urn:oo:workload:builder.one", &world.current_value), None).await;
    assert_eq!(created.status, 201, "create");
    assert_metadata(&created.body, "putSecret", 1, "active");
    assert!(!created.raw.contains(&world.current_value), "create hides the value");
    push(cases, "create", created.status, "ok");
    let replay = mutate(&mut world.http, "PUT", &db, &write_body(revision, "db-create-key-0001", "urn:oo:workload:builder.one", &world.current_value), None).await;
    assert_eq!(replay.status, 200, "replay");
    assert!(replay.body.get("value").is_none(), "replay metadata");
    push(cases, "idempotent_replay", 200, "ok");
    let conflict = mutate(&mut world.http, "PUT", &db, &write_body(revision, "db-create-key-0001", "urn:oo:workload:builder.one", &world.rotated_value), None).await;
    assert_eq!(code(&conflict.body), "idempotency.conflict");
    push(cases, "idempotency_conflict", conflict.status, "idempotency.conflict");
    let again = mutate(&mut world.http, "PUT", &db, &write_body(revision, "db-create-key-0002", "urn:oo:workload:builder.one", &world.rotated_value), None).await;
    assert_eq!(code(&again.body), "version.conflict");
    push(cases, "write_existing", again.status, "version.conflict");
    let status = world.http.call("GET", &db, None, false, None).await;
    assert_eq!(status.status, 200, "status");
    assert_metadata(&status.body, "getSecretStatus", 1, "active");
    assert_eq!(status.body["subject"], "fixture-admin");
    push(cases, "status", 200, "ok");
    let missing = http_raw("PUT", &db, &write_body(revision, "db-create-key-0003", "urn:oo:workload:builder.one", &world.current_value), None, Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), Some(&world.http.origin)).await;
    assert_eq!(missing.status, 401, "no cookie");
    push(cases, "unauthenticated", 401, code(&missing.body));
    let bad_csrf = mutate(&mut world.http, "POST", &format!("{db}/rotate"), &rotate_body(revision, "db-rotate-key-0003", "urn:oo:workload:builder.one", 1, &world.rotated_value), Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")).await;
    assert_eq!(code(&bad_csrf.body), "csrf.rejected");
    push(cases, "csrf", bad_csrf.status, "csrf.rejected");
    let csrf = fresh_csrf(&mut world.http).await;
    let bad_origin = http_raw("POST", &format!("{db}/rotate"), &rotate_body(revision, "db-rotate-key-0004", "urn:oo:workload:builder.one", 1, &world.rotated_value), world.http.session.as_deref(), Some(&csrf), Some("https://evil.example")).await;
    assert_eq!(code(&bad_origin.body), "origin.rejected");
    push(cases, "origin", bad_origin.status, "origin.rejected");
    let beta = mutate(&mut world.http, "PUT", &format!("{management}/control/v1/projects/beta/namespaces/builder/secrets/db"), &write_body(revision, "beta-create-key-01", "urn:oo:workload:builder.one", &world.current_value), None).await;
    assert_eq!(code(&beta.body), "forbidden");
    push(cases, "cross_project", beta.status, "forbidden");
    let query = world.http.call("GET", &format!("{db}?x=1"), None, false, None).await;
    assert_eq!(query.status, 400, "query");
    push(cases, "query", 400, code(&query.body));
    let password = read_trimmed(Path::new(&world.env.password_file));
    assert_eq!(version_count(&world.env, &password, "builder", "db").await, (1, 1), "negatives wrote nothing");
}

async fn prove_resolve_identity(world: &mut World, management: &str, component: &str, cases: &mut Vec<Value>) {
    let state = world.state.clone();
    let ca = world.material.ca.clone();
    let builder = component_client(&ca, &fs::read(state.join("builder.crt")).expect("crt"), &fs::read(state.join("builder.pkcs8")).expect("key"));
    let agent = component_client(&ca, &fs::read(state.join("agent.crt")).expect("crt"), &fs::read(state.join("agent.pkcs8")).expect("key"));
    let opened = resolve_ok(&builder, component, "secret://lab/alpha/builder/db", "alpha", "builder", None).await;
    assert_eq!(opened.status, 200, "resolve");
    let got = opened.body["value"].as_str().unwrap_or("");
    assert!(got.as_bytes() == world.current_value.as_bytes(), "cert opened the granted value");
    assert_eq!(opened.body["workload"], "urn:oo:workload:builder.one", "response grant is the cert subject");
    assert_eq!(opened.body["customer_kms"], "external_gate");
    assert_eq!(opened.body["mtls_ca"], "not_issued");
    push(cases, "resolve_cert_ignores_caller_header", 200, "ok");
    let spoof_body = json!({"secret_ref": "secret://lab/alpha/builder/db", "project_id": "alpha", "namespace": "builder", "workload": "urn:oo:workload:agent.one"}).to_string();
    let spoof = post_resolve(&builder, component, &spoof_body).await.expect("http");
    assert_eq!(code(&spoof.body), "malformed");
    assert!(!spoof.raw.contains(&world.current_value), "malformed hides the value");
    push(cases, "resolve_caller_json_workload", spoof.status, "malformed");
    let envelope = world.http.call("GET", "/control/v1/identity/current", None, false, None).await;
    let posted = post_resolve(&builder, component, &envelope.raw).await.expect("http");
    assert_eq!(code(&posted.body), "malformed", "current envelope is not a resolve authority");
    push(cases, "resolve_caller_json_envelope", posted.status, "malformed");
    let wrong_cert = resolve_ok(&agent, component, "secret://lab/alpha/builder/db", "alpha", "builder", None).await;
    assert_eq!(code(&wrong_cert.body), "workload.rejected");
    assert!(!wrong_cert.raw.contains(&world.current_value), "wrong cert hides the value");
    push(cases, "resolve_other_cert", wrong_cert.status, "workload.rejected");
    let foreign = foreign_client(&state, &ca);
    assert!(resolve_transport(&foreign, component, "secret://lab/alpha/builder/db", "alpha", "builder", None).await.is_err(), "foreign ca rejected");
    push(cases, "resolve_foreign_ca", 0, "transport.rejected");
    let bare = server_only_client(&ca);
    assert!(resolve_transport(&bare, component, "secret://lab/alpha/builder/db", "alpha", "builder", None).await.is_err(), "missing client cert");
    push(cases, "resolve_no_client_cert", 0, "transport.rejected");
    let crossed = resolve_ok(&builder, component, "secret://lab/alpha/builder/db", "alpha", "agent", None).await;
    assert_eq!(code(&crossed.body), "namespace.rejected");
    push(cases, "namespace_mismatch", crossed.status, "namespace.rejected");
    let agent_url = format!("{management}/control/v1/projects/alpha/namespaces/agent/secrets/db");
    let agent_write = mutate(&mut world.http, "PUT", &agent_url, &write_body(world.revision, "agent-create-key01", "urn:oo:workload:agent.one", &world.agent_value), None).await;
    assert_eq!(agent_write.status, 201, "agent create");
    assert!(agent_write.body.get("value").is_none(), "agent metadata");
    let builder_on_agent = resolve_ok(&builder, component, "secret://lab/alpha/agent/db", "alpha", "agent", None).await;
    assert_eq!(code(&builder_on_agent.body), "workload.rejected");
    assert!(!builder_on_agent.raw.contains(&world.agent_value), "builder cert hides the agent value");
    let agent_on_builder = resolve_ok(&agent, component, "secret://lab/alpha/builder/db", "alpha", "builder", None).await;
    assert_eq!(code(&agent_on_builder.body), "workload.rejected");
    let agent_ok = resolve_ok(&agent, component, "secret://lab/alpha/agent/db", "alpha", "agent", None).await;
    assert_eq!(agent_ok.status, 200, "agent resolve");
    assert!(agent_ok.body["value"].as_str().unwrap_or("").as_bytes() == world.agent_value.as_bytes(), "agent cert opened its namespace");
    assert!(agent_ok.body["value"].as_str().unwrap_or("").as_bytes() != world.current_value.as_bytes(), "agent cert did not open builder");
    push(cases, "cross_namespace", 403, "workload.rejected");
    let session = world.http.session.clone();
    let management_resolve = world.http.call("POST", &format!("{management}/component/v1/resolve"), Some(r#"{"secret_ref":"secret://lab/alpha/builder/db","project_id":"alpha","namespace":"builder"}"#), false, None).await;
    assert_eq!(management_resolve.status, 404, "management has no resolve route");
    assert!(!management_resolve.raw.contains(&world.current_value), "management hides the value");
    assert!(session.is_some(), "session kept");
    push(cases, "management_cannot_resolve", 404, code(&management_resolve.body));
}

async fn prove_rotation(world: &mut World, management: &str, component: &str, cases: &mut Vec<Value>) {
    let revision = world.revision;
    let db = format!("{management}/control/v1/projects/alpha/namespaces/builder/secrets/db");
    let password = read_trimmed(Path::new(&world.env.password_file));
    let stale = mutate(&mut world.http, "POST", &format!("{db}/rotate"), &rotate_body(revision, "db-rotate-key-stale", "urn:oo:workload:builder.one", 9, &world.rotated_value), None).await;
    assert_eq!(code(&stale.body), "version.conflict");
    assert_eq!(version_count(&world.env, &password, "builder", "db").await, (1, 1), "stale did not activate");
    push(cases, "stale_rotate", stale.status, "version.conflict");
    let mismatch = mutate(&mut world.http, "POST", &format!("{db}/rotate"), &rotate_body(revision, "db-rotate-key-work", "urn:oo:workload:agent.one", 1, &world.rotated_value), None).await;
    assert_eq!(code(&mismatch.body), "workload.rejected");
    assert_eq!(version_count(&world.env, &password, "builder", "db").await, (1, 1), "wrong workload did not activate");
    push(cases, "rotate_workload", mismatch.status, "workload.rejected");
    let rotated = mutate(&mut world.http, "POST", &format!("{db}/rotate"), &rotate_body(revision, "db-rotate-key-0002", "urn:oo:workload:builder.one", 1, &world.rotated_value), None).await;
    assert_eq!(rotated.status, 200, "rotate");
    assert_metadata(&rotated.body, "rotateSecret", 2, "active");
    assert!(!rotated.raw.contains(&world.rotated_value), "rotate hides the value");
    assert_eq!(version_count(&world.env, &password, "builder", "db").await, (2, 2), "rotate inserted one version");
    push(cases, "rotate", 200, "ok");
    let again = mutate(&mut world.http, "POST", &format!("{db}/rotate"), &rotate_body(revision, "db-rotate-key-oldv", "urn:oo:workload:builder.one", 1, &world.current_value), None).await;
    assert_eq!(code(&again.body), "version.conflict");
    assert_eq!(version_count(&world.env, &password, "builder", "db").await, (2, 2), "old version did not activate");
    push(cases, "stale_after_rotate", again.status, "version.conflict");
    let builder = component_client(&world.material.ca, &fs::read(world.state.join("builder.crt")).expect("crt"), &fs::read(world.state.join("builder.pkcs8")).expect("key"));
    let retired = resolve_ok(&builder, component, "secret://lab/alpha/builder/db", "alpha", "builder", Some(1)).await;
    assert_eq!(code(&retired.body), "version.retired");
    assert!(retired.body.get("value").is_none(), "retired has no value");
    assert!(!retired.raw.contains(&world.current_value), "retired hides version 1");
    assert!(!retired.raw.contains(&world.rotated_value), "retired hides version 2");
    push(cases, "retired", retired.status, "version.retired");
    let current = resolve_ok(&builder, component, "secret://lab/alpha/builder/db", "alpha", "builder", None).await;
    assert!(current.body["value"].as_str().unwrap_or("").as_bytes() == world.rotated_value.as_bytes(), "current is version 2");
    assert_eq!(current.body["version"], 2);
}

async fn prove_ciphertext(world: &World, secret_config: &Path, cases: &mut Vec<Value>) {
    let password = read_trimmed(Path::new(&world.env.password_file));
    let row = read_row(&world.env, &password, "builder", "db", 2).await;
    assert_eq!(row.cipher, "ring-aes-256-gcm");
    assert_eq!(row.nonce.len(), 12, "nonce length");
    assert!(!row.ciphertext.windows(world.rotated_value.len()).any(|window| window == world.rotated_value.as_bytes()), "row hides the value");
    let previous = read_row(&world.env, &password, "builder", "db", 1).await;
    assert_ne!(row.nonce, previous.nonce, "nonces differ");
    let config = load_config(secret_config).expect("config");
    let scope = Scope { deployment: "lab".into(), project: "alpha".into(), namespace: "builder".into(), secret_id: "db".into() };
    let mut nonce = [0_u8; 12];
    nonce.copy_from_slice(&row.nonce);
    assert!(open(config.master_key.as_slice(), &nonce, &canonical(&scope, 2), &row.ciphertext).is_ok(), "bound open");
    let mut other = scope.clone();
    other.namespace = "agent".into();
    assert!(open(config.master_key.as_slice(), &nonce, &canonical(&other, 2), &row.ciphertext).is_err(), "wrong namespace aad");
    assert!(open(config.master_key.as_slice(), &nonce, &canonical(&scope, 1), &row.ciphertext).is_err(), "wrong version aad");
    let mut wrong = config.master_key.to_vec();
    wrong[0] ^= 0x5a;
    assert!(open(&wrong, &nonce, &canonical(&scope, 2), &row.ciphertext).is_err(), "wrong key");
    push(cases, "aad_and_key", 0, "aead.rejected");
}

async fn prove_revoke(world: &mut World, management: &str, component: &str, cases: &mut Vec<Value>) {
    let gone = format!("{management}/control/v1/projects/alpha/namespaces/builder/secrets/gone");
    let created = mutate(&mut world.http, "PUT", &gone, &write_body(world.revision, "gone-create-key-01", "urn:oo:workload:builder.one", &world.current_value), None).await;
    assert_eq!(created.status, 201, "gone create");
    let revoked = mutate(&mut world.http, "POST", &format!("{gone}/revoke"), &json!({"identity_revision": world.revision, "idempotency_key": "gone-revoke-key-01"}).to_string(), None).await;
    assert_eq!(revoked.status, 200, "revoke");
    assert_eq!(revoked.body["state"], "revoked");
    assert!(revoked.body.get("value").is_none(), "revoke metadata");
    let builder = component_client(&world.material.ca, &fs::read(world.state.join("builder.crt")).expect("crt"), &fs::read(world.state.join("builder.pkcs8")).expect("key"));
    let refused = resolve_ok(&builder, component, "secret://lab/alpha/builder/gone", "alpha", "builder", None).await;
    assert_eq!(code(&refused.body), "revoked");
    assert!(!refused.raw.contains(&world.current_value), "revoked hides the value");
    push(cases, "revoke", refused.status, "revoked");
}

async fn prove_reviewer(world: &mut World, management: &str, cases: &mut Vec<Value>) {
    world.idp.set_subject("fixture-reviewer");
    let callback = login(&mut world.http, &world.issuer, "/control/v1/auth/login?return_to=%2Fapp").await;
    assert_eq!(callback.status, 302, "reviewer login");
    let current = world.http.call("GET", "/control/v1/identity/current", None, false, None).await;
    assert_eq!(current.status, 200, "reviewer current");
    assert_eq!(current.body["subject"], "fixture-reviewer");
    let caller = secret_control_api::try_from_caller_json(current.raw.as_bytes()).expect_err("reviewer json");
    assert_eq!(caller.code, "unauthenticated");
    let revision = current.body["revision"].as_i64().expect("revision");
    let path = format!("{management}/control/v1/projects/beta/namespaces/builder/secrets/db");
    let denied = mutate(&mut world.http, "PUT", &path, &write_body(revision, "role-deny-key-0001", "urn:oo:workload:builder.one", "0123456789abcdef0123456789abcdef"), None).await;
    assert_eq!(code(&denied.body), "forbidden");
    push(cases, "fixture-reviewer", denied.status, "forbidden");
}

fn finish_write(world: &World, cases: &[Value], secret_server: &mut ServerProc) {
    assert!(!secret_server.stderr_text().contains(&world.rotated_value), "secret stderr");
    assert!(!world.identity.stderr_text().contains(&world.rotated_value), "identity stderr");
    assert!(!secret_server.stdout_text().contains(&world.rotated_value), "secret stdout");
    secret_server.kill();
    let proof = json!({
        "fixed_commit": std::env::var("OO_A400_FIXED_COMMIT").expect("commit"),
        "cases": cases,
        "customer_kms": "external_gate",
        "mtls_ca": "not_issued",
        "customer_idp": "not_claimed",
        "mtls_ca_issued_by_service": false,
        "foreign_pool_refresh": "not_claimed",
        "resolve_authority": "verified_client_certificate"
    });
    fs::write(env_proof("OO_A400_PROOF"), serde_json::to_vec_pretty(&proof).expect("proof")).expect("write");
    assert!(!fs::read_to_string(env_proof("OO_A400_PROOF")).unwrap_or_default().contains(&world.rotated_value), "proof hides the value");
    write_private(&world.state.join("witness"), world.rotated_value.as_bytes());
    fs::write(
        world.state.join("run.json"),
        serde_json::to_vec(&json!({
            "secret_ref": "secret://lab/alpha/builder/db",
            "version": 2,
            "previous_version": 1,
            "revoked_ref": "secret://lab/alpha/builder/gone",
            "project_id": "alpha",
            "namespace": "builder",
            "workload": "urn:oo:workload:builder.one",
            "deployment": "lab",
            "resolve_authority": "verified_client_certificate"
        }))
        .expect("run"),
    )
    .expect("run");
    mark(&world.state, "live_secret=write_done");
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
            host: env_var("OO_A400_PG_HOST"),
            port: env_var("OO_A400_PG_PORT").parse().expect("port"),
            password_file: env_var("OO_A400_PG_PASSWORD_FILE"),
            ca_file: env_var("OO_A400_PG_CA"),
            state: env_var("OO_A400_STATE"),
            master_key: env_var("OO_A400_MASTER_KEY"),
            identity_bin: env_var("OO_A400_IDENTITY_BIN"),
        }
    }
}

struct Material {
    ca: Vec<u8>,
    cert: Vec<u8>,
    key: Vec<u8>,
}

impl Material {
    fn read(state: &Path) -> Self {
        Self {
            ca: fs::read(state.join("ca.pem")).expect("ca"),
            cert: fs::read(state.join("srv.pem")).expect("cert"),
            key: fs::read(state.join("srv.key")).expect("key"),
        }
    }
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
        Self { client: tls_client(ca_pem, None), origin: origin.to_string(), login: None, session: None }
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
    let certs = reqwest::Certificate::from_pem_bundle(&fs::read(env_var("OO_A400_STATE") + "/ca.pem").expect("ca")).expect("ca");
    let client = tls_client_certs(certs);
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

async fn resolve_ok(client: &reqwest::Client, component: &str, secret_ref: &str, project: &str, namespace: &str, version: Option<i64>) -> Reply {
    resolve_transport(client, component, secret_ref, project, namespace, version).await.expect("resolve transport")
}

async fn resolve_transport(client: &reqwest::Client, component: &str, secret_ref: &str, project: &str, namespace: &str, version: Option<i64>) -> Result<Reply, String> {
    let mut body = json!({"secret_ref": secret_ref, "project_id": project, "namespace": namespace});
    if let Some(version) = version {
        body["version"] = json!(version);
    }
    post_resolve(client, component, &body.to_string()).await
}

async fn post_resolve(client: &reqwest::Client, component: &str, body: &str) -> Result<Reply, String> {
    let request = client
        .post(format!("{component}/component/v1/resolve"))
        .header("content-type", "application/json")
        .header("x-oo-workload", "urn:oo:workload:agent.one")
        .body(body.to_string());
    match request.send().await {
        Ok(response) => Ok(reply_from(response).await),
        Err(error) => Err(error.without_url().to_string()),
    }
}

fn tls_client(ca_pem: &[u8], identity: Option<reqwest::Identity>) -> reqwest::Client {
    let certs = reqwest::Certificate::from_pem_bundle(ca_pem).expect("ca");
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .https_only(true)
        .http1_only()
        .referer(false)
        .connection_verbose(false)
        .tls_certs_only(certs)
        .timeout(Duration::from_secs(10));
    if let Some(identity) = identity {
        builder = builder.identity(identity);
    }
    builder.build().expect("client")
}

fn tls_client_certs(certs: Vec<reqwest::Certificate>) -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .https_only(true)
        .http1_only()
        .tls_certs_only(certs)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("raw")
}

fn component_client(ca: &[u8], cert: &[u8], key: &[u8]) -> reqwest::Client {
    tls_client(ca, Some(identity_pem(cert, key)))
}

fn server_only_client(ca: &[u8]) -> reqwest::Client {
    tls_client(ca, None)
}

fn foreign_client(state: &Path, server_ca: &[u8]) -> reqwest::Client {
    openssl(state, &["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", "foreign-ca.key", "-out", "foreign-ca.pem", "-days", "2", "-subj", "/CN=oo-a400-foreign-ca"]);
    openssl(state, &["ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", "foreign.key"]);
    openssl(state, &["req", "-new", "-key", "foreign.key", "-out", "foreign.csr", "-subj", "/CN=oo-a400-foreign"]);
    fs::write(state.join("foreign.ext"), "subjectAltName=URI:urn:oo:workload:builder.one\nextendedKeyUsage=clientAuth\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\n").expect("ext");
    openssl(state, &["x509", "-req", "-in", "foreign.csr", "-CA", "foreign-ca.pem", "-CAkey", "foreign-ca.key", "-CAcreateserial", "-out", "foreign.crt", "-days", "2", "-extfile", "foreign.ext"]);
    openssl(state, &["pkcs8", "-topk8", "-nocrypt", "-in", "foreign.key", "-out", "foreign.pkcs8"]);
    let cert = fs::read(state.join("foreign.crt")).expect("crt");
    let key = fs::read(state.join("foreign.pkcs8")).expect("key");
    tls_client(server_ca, Some(identity_pem(&cert, &key)))
}

fn identity_pem(cert: &[u8], key: &[u8]) -> reqwest::Identity {
    let mut pem = cert.to_vec();
    if !pem.ends_with(b"\n") {
        pem.push(b'\n');
    }
    pem.extend_from_slice(key);
    reqwest::Identity::from_pem(&pem).expect("identity")
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

fn code(body: &Value) -> String {
    body["error"]["code"].as_str().unwrap_or("").to_string()
}

fn push(cases: &mut Vec<Value>, name: &str, http: u16, code_name: impl AsRef<str>) {
    cases.push(json!({"name": name, "http": http, "code": code_name.as_ref()}));
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

fn write_secret_config(path: &Path, env: &Env, state: &Path) {
    let origin = read_origin(state);
    let body = json!({
        "https_origin": origin,
        "listen_host": "127.0.0.1",
        "management_port": 0,
        "component_port": 0,
        "tls_cert_file": state.join("srv.pem"),
        "tls_key_file": state.join("srv.key"),
        "client_ca_file": state.join("ca.pem"),
        "identity_origin": origin,
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
    openssl(dir, &["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", "ca.key", "-out", "ca.pem", "-days", "2", "-subj", "/CN=oo-a400-test-ca"]);
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
    openssl(dir, &["req", "-new", "-key", &key, "-out", &csr, "-subj", &format!("/CN=oo-a400-{name}")]);
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

struct Cli {
    status: u8,
    body: Value,
    raw: String,
}

fn cli(exe: &str, args: &[&str]) -> Cli {
    let output = Command::new(exe).args(args).env_clear().env("HOME", "/tmp").env("PATH", "/usr/bin:/bin:/usr/local/bin").output().expect("cli");
    let status = output.status.code().unwrap_or(1) as u8;
    let raw = String::from_utf8_lossy(&output.stdout).into_owned();
    let body = serde_json::from_str(raw.trim()).unwrap_or(Value::Null);
    assert!(output.stderr.is_empty(), "cli stderr");
    Cli { status, body, raw }
}

fn secret_bin() -> String {
    std::env::var("OO_A400_SECRET_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_secret-control").to_string())
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

struct CipherRow {
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
    cipher: String,
}

async fn read_row(env: &Env, password: &str, namespace: &str, secret_id: &str, version: i32) -> CipherRow {
    let client = pg(env, password).await;
    let row = client
        .query_one(
            "SELECT nonce, ciphertext, cipher FROM oo_secret_control.secret_version WHERE namespace = $1::text AND secret_id = $2::text AND version = $3::int4",
            &[&namespace, &secret_id, &version],
        )
        .await
        .expect("row");
    CipherRow { nonce: row.get(0), ciphertext: row.get(1), cipher: row.get(2) }
}

async fn version_count(env: &Env, password: &str, namespace: &str, secret_id: &str) -> (i32, i64) {
    let client = pg(env, password).await;
    let version: i32 = client
        .query_one("SELECT current_version FROM oo_secret_control.secret_head WHERE namespace = $1::text AND secret_id = $2::text", &[&namespace, &secret_id])
        .await
        .expect("head")
        .get(0);
    let count: i64 = client
        .query_one("SELECT COUNT(*)::bigint FROM oo_secret_control.secret_version WHERE namespace = $1::text AND secret_id = $2::text", &[&namespace, &secret_id])
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

fn env_proof(name: &str) -> String {
    env_var(name)
}
