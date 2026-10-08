//! Private PostgreSQL proof for publish, refusal, and rollback.
//!
//! The identity service and the release coordinator run as the fixed libraries.
//! A missing database host is a failure, not a skip.

mod idp;
mod shop;

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use control_identity::{canonical_commit, listen as identity_listen, load_config as load_identity, serve as identity_serve, Binding, Role, Store};
use release_control_api::sha256_hex;
use release_coordinator::{rustls_connector, GATE_IDS};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_postgres::config::SslMode;
use tokio_postgres::Client;

use idp::{Idp, IdpParts};
use shop::{pack_digest, shop, CATALOG_VERSION, KNOWLEDGE_ID, KNOWLEDGE_VERSION, POLICY_ID, POLICY_VERSION};

const PROJECT: &str = "shop";
const ROLE: &str = "oo_r_clerk";
const AUDIT_ROLE: &str = "oo_r_audit";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_postgres_publish_and_rollback() {
    let env = LiveEnv::load();
    let mut proof = Proof::default();
    let dir = env::temp_dir().join(format!("oo-a392-live-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("work dir");
    let pack_dir = dir.join("packs");
    fs::create_dir_all(&pack_dir).expect("packs");
    let secret_path = dir.join("client.secret");
    write_secret(&secret_path, "fixture-client-secret-a392\n");
    proof.forbid("fixture-client-secret-a392");

    let (effective, narrow_vis) = shop(&["contact"], ROLE);
    let (same, wide_vis) = shop(&["contact", "id"], ROLE);
    assert_eq!(effective, same, "visibility does not change the reviewed effective");
    let (_, audit_narrow) = shop(&["contact"], AUDIT_ROLE);
    let (_, audit_wide) = shop(&["contact", "id"], AUDIT_ROLE);
    let narrow_digest = pack_digest(&effective, &narrow_vis);
    let wide_digest = pack_digest(&effective, &wide_vis);
    let audit_narrow_digest = pack_digest(&effective, &audit_narrow);
    let audit_wide_digest = pack_digest(&effective, &audit_wide);
    assert_ne!(narrow_digest, wide_digest, "column visibility changes the pack digest");

    let narrow_commit = sha256_hex(b"a392-narrow");
    let wide_commit = sha256_hex(b"a392-wide");
    let omit_commit = sha256_hex(b"a392-omit-g7");
    let prod_commit = sha256_hex(b"a392-production");
    let fake_commit = sha256_hex(b"a392-fabricated");
    write_pack(&pack_dir, &narrow_commit, &effective, &narrow_vis, &audit_narrow, &narrow_digest, &audit_narrow_digest);
    write_pack(&pack_dir, &wide_commit, &effective, &wide_vis, &audit_wide, &wide_digest, &audit_wide_digest);
    write_pack(&pack_dir, &omit_commit, &effective, &narrow_vis, &audit_narrow, &narrow_digest, &audit_narrow_digest);
    write_pack(&pack_dir, &prod_commit, &effective, &narrow_vis, &audit_narrow, &narrow_digest, &audit_narrow_digest);
    write_pack(&pack_dir, &fake_commit, &effective, &narrow_vis, &audit_narrow, &narrow_digest, &audit_narrow_digest);
    write_intake(&pack_dir, &narrow_commit, &[], false);
    write_intake(&pack_dir, &wide_commit, &[], false);
    write_intake(&pack_dir, &omit_commit, &["G7"], false);
    write_intake(&pack_dir, &prod_commit, &[], false);
    write_intake(&pack_dir, &fake_commit, &[], true);
    let narrow_file_hash = sha256_hex(&fs::read(pack_dir.join(format!("{narrow_commit}.json"))).expect("narrow file"));
    let narrow_columns = columns_text(&narrow_vis);

    let idp_listener = TcpListener::bind("127.0.0.1:0").await.expect("idp bind");
    let idp_port = idp_listener.local_addr().expect("idp port").port();
    let identity_port = probe_port().await;
    let issuer = format!("https://127.0.0.1:{idp_port}");
    let identity_origin = format!("https://127.0.0.1:{identity_port}");
    let callback = format!("{identity_origin}/control/v1/auth/callback");
    let identity_config = dir.join("identity.json");
    write_identity_config(&identity_config, &env, &secret_path, &identity_origin, &issuer, &callback, identity_port);
    let identity = Arc::new(load_identity(&identity_config).expect("identity config"));
    Store::install(&identity).await.expect("identity install");
    let token_path = dir.join("bootstrap.token");
    Store::issue_bootstrap(&identity, &token_path).await.expect("bootstrap");
    let token = fs::read_to_string(&token_path).expect("token file");
    let token = token.trim_end_matches(['\n', '\r']).to_string();
    assert_eq!(token.len(), 43);
    proof.forbid(&token);
    let _ = fs::remove_file(&token_path);

    let der_keys = vec![fs::read(env.cert_dir.join("k1.der")).expect("k1"), fs::read(env.cert_dir.join("k2.der")).expect("k2")];
    let secret = fs::read_to_string(&secret_path).expect("secret");
    let secret = secret.trim_end_matches(['\n', '\r']).to_string();
    let idp = idp::start(idp_listener, IdpParts {
        cert_pem: &env.cert,
        key_pem: &env.key,
        issuer: &issuer,
        client_id: "oo-control-client",
        client_secret: &secret,
        redirect_uri: &callback,
        der_keys: &der_keys,
    })
    .await;
    let identity_listener = identity_listen(&identity).await.expect("identity listen");
    let identity_task = tokio::spawn(async move {
        let _ = identity_serve(identity, identity_listener).await;
    });
    let _identity_guard = TaskGuard(identity_task);

    let mut identity_http = Http::new(&identity_origin, &env.https_ca);
    let admin = login_session(&mut identity_http, &idp, Some(&token), "fixture-admin").await;
    proof.forbid(&admin.cookie);
    proof.forbid(&admin.csrf);
    assert_eq!(admin.subject, "fixture-admin");
    assert_eq!(admin.revision, 1);

    let api_port = probe_port().await;
    let api_origin = format!("https://127.0.0.1:{api_port}");
    let api_config = dir.join("api.json");
    write_api_config(&api_config, &env, &pack_dir, &api_origin, &identity_origin, api_port);
    let install = Command::new(env.exe.as_os_str())
        .args(["install", "--config"])
        .arg(&api_config)
        .env_clear()
        .env("HOME", "/tmp")
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("install");
    assert!(install.stderr.is_empty(), "install stderr");
    let install_body: Value = serde_json::from_slice(&install.stdout).expect("install json");
    assert_eq!(install.status.code(), Some(0), "{install_body}");
    assert_eq!(install_body["production_ready"], false);
    assert_eq!(install_body["full_sp08_accepted"], false);
    proof.step("install", 0, install_body["outcome"].as_str().unwrap_or(""), "", Value::Null);

    let mut server = ServerProc::spawn(&env.exe, &api_config);
    let mut api = Http::new(&api_origin, &env.https_ca);
    api.session = Some(admin.cookie.clone());
    api.csrf = Some(admin.csrf.clone());

    let (reviewer, auditor, owner, other) = {
        let mut sessions = Sessions { identity_origin: identity_origin.clone(), cert: env.https_ca.clone(), idp: &idp, proof: &mut proof };
        let reviewer = sessions.open("fixture-reviewer").await;
        let auditor = sessions.open("fixture-auditor").await;
        let owner = sessions.open("fixture-owner").await;
        let other = sessions.open("fixture-other").await;
        (reviewer, auditor, owner, other)
    };

    proof.step("active-empty", 0, "", "", active_version(&api).await);
    assert!(active_version(&api).await.is_null(), "pointer starts empty");
    let empty_pack = api.call("GET", &format!("/control/v1/releases/active/pack?project_id={PROJECT}&role={ROLE}"), None, false).await;
    assert_eq!(empty_pack.status, 409);
    assert_eq!(empty_pack.body["detail"], "inactive");
    proof.step("pack-inactive", empty_pack.status, empty_pack.body["code"].as_str().unwrap_or(""), "inactive", Value::Null);

    let before = operation_count(&env).await;
    let mut bare = api.clone_client();
    bare.session = None;
    bare.csrf = None;
    bare.login = None;
    let no_cookie = bare.call("POST", "/control/v1/releases", Some("{"), false).await;
    assert_eq!(no_cookie.status, 401, "auth runs before the body parser");
    assert_eq!(operation_count(&env).await, before);
    proof.step("no-cookie", no_cookie.status, no_cookie.body["code"].as_str().unwrap_or(""), no_cookie.body["detail"].as_str().unwrap_or(""), Value::Null);

    let duplicate = api.call("POST", "/control/v1/releases", Some(r#"{"project_id":"shop","project_id":"shop"}"#), true).await;
    assert_eq!(duplicate.status, 400);
    assert_eq!(operation_count(&env).await, before);
    proof.step("duplicate-key", duplicate.status, "malformed", duplicate.body["detail"].as_str().unwrap_or(""), Value::Null);

    let waiver = api.call("POST", "/control/v1/releases", Some(r#"{"force":false}"#), true).await;
    assert_eq!(waiver.status, 400);
    assert_eq!(operation_count(&env).await, before);
    proof.step("waiver", waiver.status, "malformed", waiver.body["detail"].as_str().unwrap_or(""), Value::Null);

    let bad_origin = api.call_origin("POST", "/control/v1/releases", Some(&create_json(&omit_commit, "cand-origin", "ver-origin", None, "test_scope", "key-origin-00000001")), "https://127.0.0.1:9").await;
    assert_eq!(bad_origin.status, 403);
    assert_eq!(bad_origin.body["code"], "origin.rejected");
    assert_eq!(operation_count(&env).await, before);
    proof.step("origin", bad_origin.status, "origin.rejected", "origin", Value::Null);

    let bad_csrf = api.call_csrf("POST", "/control/v1/releases", Some(&create_json(&omit_commit, "cand-csrf", "ver-csrf", None, "test_scope", "key-csrf-0000000001")), "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").await;
    assert_eq!(bad_csrf.status, 403);
    assert_eq!(bad_csrf.body["code"], "csrf.rejected");
    assert_eq!(operation_count(&env).await, before);
    proof.step("csrf", bad_csrf.status, "csrf.rejected", "csrf", Value::Null);

    refresh_csrf(&mut identity_http, &mut api).await;
    let bad_revision = api.call("POST", "/control/v1/releases", Some(&create_json_revision(&omit_commit, "cand-rev", "ver-rev", None, "test_scope", "key-revision-000001", 2)), true).await;
    assert_eq!(bad_revision.status, 409);
    assert_eq!(bad_revision.body["code"], "revision.conflict");
    assert_eq!(operation_count(&env).await, before);
    proof.step("revision", bad_revision.status, "revision.conflict", "revision", Value::Null);

    let missing = api.call("GET", "/control/v1/releases/op_0123456789abcdef0123456789abcdef", None, false).await;
    assert_eq!(missing.status, 403);
    assert_eq!(missing.body["detail"], "scope");
    proof.step("missing-op", missing.status, "forbidden", "scope", Value::Null);

    let unknown = api.call("GET", "/control/v1/no-such", None, false).await;
    assert_eq!(unknown.status, 404);
    proof.step("unknown-route", unknown.status, unknown.body["code"].as_str().unwrap_or(""), "route", Value::Null);
    let method = api.call("POST", "/control/v1/releases/active", Some("{}"), true).await;
    assert_eq!(method.status, 405);
    proof.step("method", method.status, method.body["code"].as_str().unwrap_or(""), "method", Value::Null);

    let mut reviewer_http = api.with_session(&reviewer);
    refresh_session(&identity_origin, &env.https_ca, &mut reviewer_http).await;
    let reviewer_rollback = reviewer_http.call("POST", "/control/v1/releases/op_0123456789abcdef0123456789abcdef/rollback", Some(&rollback_json("ver-missing", "key-reviewer-rollback")), true).await;
    assert_eq!(reviewer_rollback.status, 403);
    assert_eq!(reviewer_rollback.body["detail"], "capability");
    assert_eq!(operation_count(&env).await, before);
    proof.step("reviewer-rollback", reviewer_rollback.status, "forbidden", "capability", Value::Null);

    let mut other_http = api.with_session(&other);
    let other_shop = other_http.call("GET", &format!("/control/v1/releases?project_id={PROJECT}"), None, false).await;
    assert_eq!(other_shop.status, 403);
    assert_eq!(other_shop.body["detail"], "scope");
    let other_project = other_http.call("GET", "/control/v1/releases?project_id=otherproj", None, false).await;
    assert_eq!(other_project.status, 403);
    assert_eq!(other_project.body["detail"], "scope");
    proof.step("cross-project", other_shop.status, "forbidden", "scope", Value::Null);

    let mut owner_http = api.with_session(&owner);
    refresh_session(&identity_origin, &env.https_ca, &mut owner_http).await;
    let owner_publish = owner_http.call("POST", "/control/v1/releases", Some(&create_json(&narrow_commit, "cand-owner", "ver-owner", None, "test_scope", "key-owner-publish01")), true).await;
    assert_eq!(owner_publish.status, 403);
    assert_eq!(owner_publish.body["detail"], "capability");
    assert_eq!(operation_count(&env).await, before);
    proof.step("owner-publish", owner_publish.status, "forbidden", "capability", Value::Null);

    let omit = publish_and_wait(&mut identity_http, &mut api, &create_json(&omit_commit, "cand-omit", "ver-omit", None, "test_scope", "key-omit-g7-000001")).await;
    assert_eq!(omit["status"], "refused", "{omit}");
    assert_eq!(omit["certificate_class"], "not_issued");
    assert!(gate_has(&omit, "G7_missing"), "{omit}");
    assert!(active_version(&api).await.is_null());
    assert_flags(&omit);
    proof.step("omit-g7", 200, omit["status"].as_str().unwrap_or(""), omit["producer_reason"].as_str().unwrap_or(""), Value::Null);

    let fake = publish_and_wait(&mut identity_http, &mut api, &create_json(&fake_commit, "cand-fake", "ver-fake", None, "test_scope", "key-fake-fence-0001")).await;
    assert_eq!(fake["status"], "refused", "{fake}");
    assert!(gate_has(&fake, "stale_fence"), "{fake}");
    assert!(active_version(&api).await.is_null());
    proof.step("fabricated-fence", 200, "refused", fake["producer_reason"].as_str().unwrap_or(""), Value::Null);

    let prod0 = publish_and_wait(&mut identity_http, &mut api, &create_json(&prod_commit, "cand-prod0", "ver-prod0", None, "production", "key-prod-empty-0001")).await;
    assert_eq!(prod0["status"], "refused", "{prod0}");
    assert_eq!(prod0["certificate_class"], "not_issued");
    assert_eq!(prod0["producer_reason"], "production_blocked");
    for token in ["authenticated_owner_missing", "runsc_missing", "customer_not_production_admissible", "replica_not_production_admissible", "fence_not_production_admissible"] {
        assert!(gate_has(&prod0, token), "{token} missing from {prod0}");
    }
    assert_flags(&prod0);
    assert!(active_version(&api).await.is_null());
    proof.step("production-blocked-empty", 200, "refused", "production_blocked", Value::Null);

    let narrow_body = create_json(&narrow_commit, "cand-n", "ver-n", None, "test_scope", "key-narrow-v1-00001");
    let narrow = publish_and_wait(&mut identity_http, &mut api, &narrow_body).await;
    assert_eq!(narrow["status"], "succeeded", "{narrow}");
    assert_eq!(narrow["certificate_class"], "test_scope_not_production");
    assert_eq!(narrow["producer_outcome"], "activated");
    assert_flags(&narrow);
    assert_eq!(active_version(&api).await, json!("ver-n"));
    let narrow_id = narrow["operation_id"].as_str().expect("id").to_string();
    proof.step("publish-narrow", 200, "succeeded", "test_scope_not_production", json!("ver-n"));

    refresh_csrf(&mut identity_http, &mut api).await;
    let replay = api.call("POST", "/control/v1/releases", Some(&narrow_body), true).await;
    assert_eq!(replay.status, 202);
    assert_eq!(replay.body["replay"], true);
    assert_eq!(replay.body["operation_id"], narrow_id);
    assert_eq!(replay.body["status"], "succeeded");
    proof.step("idempotency-replay", replay.status, "succeeded", "replay", json!("ver-n"));
    let conflict_body = create_json(&narrow_commit, "cand-other", "ver-other", None, "test_scope", "key-narrow-v1-00001");
    refresh_csrf(&mut identity_http, &mut api).await;
    let conflict = api.call("POST", "/control/v1/releases", Some(&conflict_body), true).await;
    assert_eq!(conflict.status, 409);
    assert_eq!(conflict.body["code"], "idempotency.conflict");
    proof.step("idempotency-conflict", conflict.status, "idempotency.conflict", "idempotency", json!("ver-n"));

    let count_at_narrow = operation_count(&env).await;
    refresh_csrf(&mut identity_http, &mut api).await;
    let mismatch = api.call("POST", "/control/v1/releases", Some(&create_json(&wide_commit, "cand-mismatch", "ver-mismatch", None, "test_scope", "key-base-mismatch01")), true).await;
    assert_eq!(mismatch.status, 409);
    assert_eq!(mismatch.body["detail"], "pointer");
    assert_eq!(operation_count(&env).await, count_at_narrow);
    assert_eq!(active_version(&api).await, json!("ver-n"));
    proof.step("base-mismatch", mismatch.status, "conflict", "pointer", json!("ver-n"));

    let prod1 = publish_and_wait(&mut identity_http, &mut api, &create_json(&prod_commit, "cand-prod1", "ver-prod1", Some("ver-n"), "production", "key-prod-active-0001")).await;
    assert_eq!(prod1["status"], "refused", "{prod1}");
    assert_eq!(prod1["certificate_class"], "not_issued");
    assert_eq!(prod1["customer_gate_verified"], false);
    assert!(gate_has(&prod1, "authenticated_owner_missing"));
    assert!(gate_has(&prod1, "runsc_missing"));
    assert_eq!(active_version(&api).await, json!("ver-n"));
    proof.step("production-blocked-active", 200, "refused", "production_blocked", json!("ver-n"));

    let wide = publish_and_wait(&mut identity_http, &mut api, &create_json(&wide_commit, "cand-w", "ver-w", Some("ver-n"), "test_scope", "key-wide-v2-0000001")).await;
    assert_eq!(wide["status"], "succeeded", "{wide}");
    assert_eq!(wide["certificate_class"], "test_scope_not_production");
    assert_eq!(active_version(&api).await, json!("ver-w"));
    let wide_id = wide["operation_id"].as_str().expect("wide").to_string();
    proof.step("publish-wide", 200, "succeeded", "test_scope_not_production", json!("ver-w"));

    assert_reader(&env, Some("ver-w")).await;
    assert_admin_cannot_read_view(&env).await;
    let mut auditor_http = api.with_session(&auditor);
    let auditor_pack = auditor_http.call("GET", &format!("/control/v1/releases/active/pack?project_id={PROJECT}&role={ROLE}"), None, false).await;
    assert_eq!(auditor_pack.status, 200, "{}", auditor_pack.body);
    assert_eq!(auditor_pack.body["certificate_class"], "test_scope_not_production");
    assert_eq!(auditor_pack.body["replica_production_admissible"], false);
    assert_eq!(auditor_pack.body["production_ready"], false);
    let files = auditor_pack.body["files"].as_array().expect("files");
    assert!(!files.is_empty());
    for file in files {
        let path = file["path"].as_str().expect("path");
        let sha = file["sha256"].as_str().expect("sha");
        assert!(!path.contains("..") && !path.starts_with('/'));
        assert_eq!(sha.len(), 64);
        assert!(file["bytes"].as_i64().expect("bytes") >= 0);
    }
    proof.step("auditor-pack", 200, "getActiveRolePack", "test_scope_not_production", json!("ver-w"));
    let reviewer_pack = reviewer_http.call("GET", &format!("/control/v1/releases/active/pack?project_id={PROJECT}&role={ROLE}"), None, false).await;
    assert_eq!(reviewer_pack.status, 403);
    assert_eq!(reviewer_pack.body["detail"], "capability");
    proof.step("reviewer-pack", reviewer_pack.status, "forbidden", "capability", json!("ver-w"));
    let wrong_role = api.call("GET", &format!("/control/v1/releases/active/pack?project_id={PROJECT}&role={AUDIT_ROLE}"), None, false).await;
    assert_eq!(wrong_role.status, 403);
    assert_eq!(wrong_role.body["detail"], "role");
    proof.step("role-mismatch", wrong_role.status, "forbidden", "role", json!("ver-w"));

    let rolled = rollback_and_wait(&mut identity_http, &mut api, &wide_id, "ver-w", "key-rollback-wide-01").await;
    assert_eq!(rolled["status"], "succeeded", "{rolled}");
    assert_eq!(rolled["kind"], "rollback");
    assert_eq!(rolled["certificate_class"], "not_issued");
    assert_eq!(rolled["producer_outcome"], "rolled_back");
    assert_eq!(active_version(&api).await, json!("ver-n"));
    let still = api.call("GET", &format!("/control/v1/releases/{narrow_id}"), None, false).await;
    assert_eq!(still.body["status"], "succeeded");
    let wide_still = api.call("GET", &format!("/control/v1/releases/{wide_id}"), None, false).await;
    assert_eq!(wide_still.body["status"], "succeeded");
    assert_producer_state(&env, "cand-n", "active").await;
    assert_producer_state(&env, "cand-w", "inactive").await;
    assert_producer_audit(&env, "cand-w", "rolled_back").await;
    assert_reader(&env, Some("ver-n")).await;
    proof.step("rollback-wide", 200, "rolled_back", "not_issued", json!("ver-n"));

    let wide2 = publish_and_wait(&mut identity_http, &mut api, &create_json(&wide_commit, "cand-w2", "ver-w2", Some("ver-n"), "test_scope", "key-wide-again-0001")).await;
    assert_eq!(wide2["status"], "succeeded", "{wide2}");
    let narrow2 = publish_and_wait(&mut identity_http, &mut api, &create_json(&narrow_commit, "cand-n2", "ver-n2", Some("ver-w2"), "test_scope", "key-narrow-again-01")).await;
    assert_eq!(narrow2["status"], "succeeded", "{narrow2}");
    assert_eq!(active_version(&api).await, json!("ver-n2"));
    let narrow2_id = narrow2["operation_id"].as_str().expect("n2").to_string();
    let count_before_compat = operation_count(&env).await;
    refresh_csrf(&mut identity_http, &mut api).await;
    let compat = api.call("POST", &format!("/control/v1/releases/{narrow2_id}/rollback"), Some(&rollback_json("ver-n2", "key-compat-refuse01")), true).await;
    assert_eq!(compat.status, 409, "{}", compat.body);
    assert_eq!(compat.body["code"], "compat.refused");
    assert_eq!(operation_count(&env).await, count_before_compat);
    assert_eq!(active_version(&api).await, json!("ver-n2"));
    proof.step("rollback-widen-refused", compat.status, "compat.refused", "permission", json!("ver-n2"));

    server.kill();
    let running_id = format!("op_{}", &sha256_hex(b"a392-running-row")[..32]);
    insert_running(&env, &running_id).await;
    server = ServerProc::spawn(&env.exe, &api_config);
    let resumed = api.call("GET", &format!("/control/v1/releases/{running_id}"), None, false).await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    assert_eq!(resumed.body["status"], "resume_uncertain");
    assert_eq!(active_version(&api).await, json!("ver-n2"));
    proof.step("restart-uncertain", resumed.status, "resume_uncertain", "", json!("ver-n2"));

    server.kill();
    let restart_id = format!("op_{}", &sha256_hex(b"a392-queued-restart")[..32]);
    insert_queued(&env, &restart_id, &narrow_commit, &narrow_file_hash, &narrow_columns).await;
    server = ServerProc::spawn(&env.exe, &api_config);
    let restarted = wait_id(&api, &restart_id).await;
    assert_eq!(restarted["status"], "succeeded", "{restarted}");
    assert_eq!(restarted["certificate_class"], "test_scope_not_production");
    assert_eq!(active_version(&api).await, json!("ver-restart"));
    assert_reader(&env, Some("ver-restart")).await;
    assert_audit_immutable(&env).await;
    proof.step("restart-queued", 200, "succeeded", "test_scope_not_production", json!("ver-restart"));

    drop(server);
    proof.finish(&env);
    let _ = fs::remove_dir_all(&dir);
}

struct LiveEnv {
    host: String,
    port: u16,
    database: String,
    exe: PathBuf,
    cert_dir: PathBuf,
    cert: Vec<u8>,
    key: Vec<u8>,
    https_ca: Vec<u8>,
    ca: Vec<u8>,
    api_password_file: PathBuf,
    producer_password_file: PathBuf,
    reader_password_file: PathBuf,
    identity_password_file: PathBuf,
    pg_ca_file: PathBuf,
    proof_path: Option<PathBuf>,
}

impl LiveEnv {
    fn load() -> Self {
        let host = env::var("OO_A392_PG_HOST").unwrap_or_else(|_| panic!("OO_A392_PG_HOST is required for the private postgres test"));
        let port = env::var("OO_A392_PG_PORT").unwrap_or_else(|_| panic!("OO_A392_PG_PORT is required")).parse().expect("port");
        let database = env::var("OO_A392_PG_DATABASE").unwrap_or_else(|_| panic!("OO_A392_PG_DATABASE is required"));
        let cert_dir = PathBuf::from(env::var("OO_A392_CERT_DIR").unwrap_or_else(|_| panic!("OO_A392_CERT_DIR is required")));
        let pg_ca_file = PathBuf::from(env::var("OO_A392_PG_CA").unwrap_or_else(|_| panic!("OO_A392_PG_CA is required")));
        Self {
            host,
            port,
            database,
            exe: PathBuf::from(env!("CARGO_BIN_EXE_release-control-api")),
            cert: fs::read(cert_dir.join("srv.pem")).expect("srv.pem"),
            key: fs::read(cert_dir.join("srv.key")).expect("srv.key"),
            https_ca: fs::read(cert_dir.join("ca.pem")).expect("https ca"),
            ca: fs::read(&pg_ca_file).expect("pg ca"),
            api_password_file: PathBuf::from(env::var("OO_A392_API_PASSWORD_FILE").unwrap_or_else(|_| panic!("OO_A392_API_PASSWORD_FILE is required"))),
            producer_password_file: PathBuf::from(env::var("OO_A392_PRODUCER_PASSWORD_FILE").unwrap_or_else(|_| panic!("OO_A392_PRODUCER_PASSWORD_FILE is required"))),
            reader_password_file: PathBuf::from(env::var("OO_A392_READER_PASSWORD_FILE").unwrap_or_else(|_| panic!("OO_A392_READER_PASSWORD_FILE is required"))),
            identity_password_file: PathBuf::from(env::var("OO_A392_IDENTITY_PASSWORD_FILE").unwrap_or_else(|_| panic!("OO_A392_IDENTITY_PASSWORD_FILE is required"))),
            pg_ca_file,
            cert_dir,
            proof_path: env::var("OO_A392_PROOF").ok().map(PathBuf::from),
        }
    }
}

struct Session {
    cookie: String,
    csrf: String,
    subject: String,
    revision: i64,
}

struct Sessions<'a> {
    identity_origin: String,
    cert: Vec<u8>,
    idp: &'a Idp,
    proof: &'a mut Proof,
}

impl Sessions<'_> {
    async fn open(&mut self, subject: &str) -> Session {
        let mut http = Http::new(&self.identity_origin, &self.cert);
        let session = login_session(&mut http, self.idp, None, subject).await;
        self.proof.forbid(&session.cookie);
        self.proof.forbid(&session.csrf);
        assert_eq!(session.subject, subject);
        session
    }
}

struct Http {
    client: reqwest::Client,
    origin: String,
    login: Option<String>,
    session: Option<String>,
    csrf: Option<String>,
}

struct Reply {
    status: u16,
    location: String,
    body: Value,
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
        Self { client, origin: origin.to_string(), login: None, session: None, csrf: None }
    }

    fn with_session(&self, session: &Session) -> Self {
        Self { client: self.client.clone(), origin: self.origin.clone(), login: None, session: Some(session.cookie.clone()), csrf: Some(session.csrf.clone()) }
    }

    async fn call(&mut self, method: &str, path: &str, body: Option<&str>, mutate: bool) -> Reply {
        if mutate {
            let origin = self.origin.clone();
            let csrf = self.csrf.clone().unwrap_or_default();
            self.send(method, path, body, Some(&origin), Some(&csrf), true).await
        } else {
            self.send(method, path, body, None, None, true).await
        }
    }

    async fn call_csrf(&mut self, method: &str, path: &str, body: Option<&str>, csrf: &str) -> Reply {
        let origin = self.origin.clone();
        self.send(method, path, body, Some(&origin), Some(csrf), true).await
    }

    async fn call_origin(&mut self, method: &str, path: &str, body: Option<&str>, origin: &str) -> Reply {
        let csrf = self.csrf.clone().unwrap_or_default();
        self.send(method, path, body, Some(origin), Some(&csrf), true).await
    }

    async fn send(&mut self, method: &str, path: &str, body: Option<&str>, origin: Option<&str>, csrf: Option<&str>, send_cookie: bool) -> Reply {
        let url = if path.starts_with("https://") { path.to_string() } else { format!("{}{path}", self.origin) };
        let mut request = self.client.request(method.parse().expect("method"), &url);
        if send_cookie && let Some(cookie) = self.cookie_header() {
            request = request.header("cookie", cookie);
        }
        if let Some(origin) = origin {
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

async fn login_session(http: &mut Http, idp: &Idp, bootstrap: Option<&str>, subject: &str) -> Session {
    idp.set_subject(subject);
    let path = match bootstrap {
        Some(token) => format!("/control/v1/auth/login?return_to=%2Fapp&bootstrap_token={token}"),
        None => "/control/v1/auth/login?return_to=%2Fapp".to_string(),
    };
    let done = follow_login(http, &path).await;
    assert_eq!(done.status, 302, "login status {}", done.status);
    assert!(done.location.starts_with(&format!("{}/app", http.origin)) || done.location.contains("/app"), "return {}", done.location);
    let current = http.call("GET", "/control/v1/identity/current", None, false).await;
    assert_eq!(current.status, 200, "current {}", current.status);
    Session {
        cookie: http.session.clone().expect("session"),
        csrf: current.body["csrf_token"].as_str().expect("csrf").to_string(),
        subject: current.body["subject"].as_str().expect("subject").to_string(),
        revision: current.body["revision"].as_i64().expect("revision"),
    }
}

async fn follow_login(http: &mut Http, path: &str) -> Reply {
    let mut reply = http.call("GET", path, None, false).await;
    for _ in 0..6 {
        if reply.status != 302 || reply.location.is_empty() {
            return reply;
        }
        let app = format!("{}/app", http.origin);
        if reply.location == app || reply.location.starts_with(&format!("{app}?")) {
            return reply;
        }
        if reply.location.starts_with(&http.origin) {
            let next = reply.location.trim_start_matches(&http.origin).to_string();
            reply = http.call("GET", &next, None, false).await;
        } else {
            let external = http.get_external(&reply.location).await;
            if external.status != 302 {
                return external;
            }
            let next = external.location.trim_start_matches(&http.origin).to_string();
            reply = http.call("GET", &next, None, false).await;
        }
    }
    reply
}

async fn refresh_csrf(identity: &mut Http, api: &mut Http) {
    let current = identity.call("GET", "/control/v1/identity/current", None, false).await;
    assert_eq!(current.status, 200, "csrf refresh {}", current.body);
    api.csrf = Some(current.body["csrf_token"].as_str().expect("csrf").to_string());
}

async fn refresh_session(identity_origin: &str, ca: &[u8], api: &mut Http) {
    let mut identity = Http::new(identity_origin, ca);
    identity.session = api.session.clone();
    refresh_csrf(&mut identity, api).await;
}

async fn publish_and_wait(identity: &mut Http, api: &mut Http, body: &str) -> Value {
    refresh_csrf(identity, api).await;
    let mut http = api.clone_client();
    let created = http.call("POST", "/control/v1/releases", Some(body), true).await;
    assert_eq!(created.status, 202, "{}", created.body);
    assert_eq!(created.body["replay"], false);
    let id = created.body["operation_id"].as_str().expect("operation").to_string();
    wait_id(&http, &id).await
}

async fn rollback_and_wait(identity: &mut Http, api: &mut Http, operation_id: &str, version: &str, key: &str) -> Value {
    refresh_csrf(identity, api).await;
    let mut http = api.clone_client();
    let created = http.call("POST", &format!("/control/v1/releases/{operation_id}/rollback"), Some(&rollback_json(version, key)), true).await;
    assert_eq!(created.status, 202, "{}", created.body);
    let id = created.body["operation_id"].as_str().expect("rollback id").to_string();
    wait_id(&http, &id).await
}

async fn wait_id(api: &Http, operation_id: &str) -> Value {
    let mut http = api.clone_client();
    for _ in 0..300 {
        let reply = http.call("GET", &format!("/control/v1/releases/{operation_id}"), None, false).await;
        assert_eq!(reply.status, 200, "{}", reply.body);
        let status = reply.body["status"].as_str().unwrap_or("");
        if status != "queued" && status != "running" {
            return reply.body;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("operation {operation_id} stayed queued");
}

async fn active_version(api: &Http) -> Value {
    let mut http = api.clone_client();
    let reply = http.call("GET", &format!("/control/v1/releases/active?project_id={PROJECT}"), None, false).await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(reply.body["reader"], "oo_release_reader");
    assert_eq!(reply.body["production_ready"], false);
    assert_eq!(reply.body["customer_gate_verified"], false);
    assert_eq!(reply.body["owner_authenticated"], false);
    assert_eq!(reply.body["runsc_verified"], false);
    assert_eq!(reply.body["full_sp08_accepted"], false);
    assert_eq!(reply.body["half_published"], false);
    reply.body["version"].clone()
}

impl Http {
    fn clone_client(&self) -> Self {
        Self {
            client: self.client.clone(),
            origin: self.origin.clone(),
            login: self.login.clone(),
            session: self.session.clone(),
            csrf: self.csrf.clone(),
        }
    }
}

fn create_json(commit: &str, candidate: &str, version: &str, base: Option<&str>, scope: &str, key: &str) -> String {
    create_json_revision(commit, candidate, version, base, scope, key, 1)
}

fn create_json_revision(commit: &str, candidate: &str, version: &str, base: Option<&str>, scope: &str, key: &str, revision: i64) -> String {
    json!({
        "base_version": base,
        "candidate_id": candidate,
        "config_commit": commit,
        "expected_version": base,
        "idempotency_key": key,
        "identity_revision": revision,
        "project_id": PROJECT,
        "role": ROLE,
        "scope": scope,
        "version": version
    })
    .to_string()
}

fn rollback_json(version: &str, key: &str) -> String {
    json!({
        "expected_version": version,
        "idempotency_key": key,
        "identity_revision": 1,
        "project_id": PROJECT
    })
    .to_string()
}

fn assert_flags(body: &Value) {
    for key in ["production_ready", "customer_gate_verified", "owner_authenticated", "runsc_verified", "full_sp08_accepted", "half_published"] {
        assert_eq!(body[key], false, "{key}");
    }
}

fn gate_has(body: &Value, token: &str) -> bool {
    body["gates"].as_array().is_some_and(|gates| gates.iter().any(|gate| gate["gate_id"] == token))
}

fn write_pack(dir: &Path, commit: &str, effective: &Value, clerk: &Value, audit: &Value, clerk_digest: &str, audit_digest: &str) {
    let producer = json!({
        "authenticated": false,
        "outcome": "packed",
        "parent_auth": "required",
        "product": {
            "effective": {"effective_oos": effective},
            "packs": [
                {"pack_digest": clerk_digest, "role": ROLE},
                {"pack_digest": audit_digest, "role": AUDIT_ROLE}
            ]
        },
        "protocol": "oo.pg-builder.pack-combined/v1",
        "statements": {"model_publication": false}
    });
    let output = sha256_hex(&serde_json::to_vec(&producer).expect("producer"));
    let wrapper = json!({
        "config_commit": commit,
        "envelope": {
            "active": false,
            "compiled_pack": "not_published",
            "contract": "oo.selfbuild-control-api/v1",
            "operation": "getBuildPack",
            "output_sha256": output,
            "producer": producer,
            "producer_authenticated": false,
            "published": false
        },
        "protocol": "oo.release-control.reviewed-pack/v1",
        "roles": [clerk, audit]
    });
    fs::write(dir.join(format!("{commit}.json")), serde_json::to_vec(&wrapper).expect("wrapper")).expect("write pack");
}

fn write_intake(dir: &Path, commit: &str, skip: &[&str], fabricated: bool) {
    let mut gates = Vec::new();
    for gate in GATE_IDS {
        if skip.contains(gate) {
            continue;
        }
        if *gate == "G8" {
            gates.push(json!({"blocking": false, "gate_id": gate, "issuer_class": "component_authority", "report_type": "component_recompile", "status": "pass"}));
        } else {
            gates.push(json!({"blocking": false, "gate_id": gate, "issuer_class": "fixture_authority", "report_type": "fixture_certificate", "status": "pass"}));
        }
    }
    let external = ["credential", "integrity", "customer"].iter().map(|gate| {
        json!({"blocking": false, "gate_id": gate, "issuer_class": "test_issuer", "report_type": "test_issuer", "status": "pass"})
    }).collect::<Vec<_>>();
    let fence_terminated = fabricated;
    let intake = json!({
        "checkpoints": [
            {"barrier": "closed", "epoch": "1", "issuer_class": "test_issuer", "kind": "fence", "sessions_terminated": fence_terminated},
            {"barrier": "replayed", "epoch": "1", "issuer_class": "test_issuer", "kind": "replica", "sessions_terminated": false}
        ],
        "config_commit": commit,
        "external": external,
        "gates": gates,
        "protocol": "oo.release-control.gate-intake/v1",
        "skip": skip
    });
    fs::write(dir.join(format!("{commit}.gates.json")), serde_json::to_vec(&intake).expect("intake")).expect("write intake");
}

fn columns_text(visibility: &Value) -> String {
    let mut columns = std::collections::BTreeSet::new();
    for dataset in visibility["datasets"].as_array().expect("datasets") {
        let name = dataset["dataset"].as_str().expect("dataset");
        for column in dataset["columns"].as_array().expect("columns") {
            columns.insert(format!("{name}.{}", column.as_str().expect("column")));
        }
    }
    columns.into_iter().collect::<Vec<_>>().join("\n")
}

fn write_secret(path: &Path, text: &str) {
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(path).expect("secret");
    file.write_all(text.as_bytes()).expect("secret write");
}

fn write_identity_config(path: &Path, env: &LiveEnv, secret: &Path, origin: &str, issuer: &str, callback: &str, port: u16) {
    let bindings = vec![
        Binding { subject: "fixture-admin".into(), project_id: "shop".into(), role: Role::Admin },
        Binding { subject: "fixture-reviewer".into(), project_id: "shop".into(), role: Role::Reviewer },
        Binding { subject: "fixture-auditor".into(), project_id: "shop".into(), role: Role::Auditor },
        Binding { subject: "fixture-owner".into(), project_id: "shop".into(), role: Role::BusinessOwner },
        Binding { subject: "fixture-other".into(), project_id: "otherproj".into(), role: Role::Admin },
    ];
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
            "password_file": env.identity_password_file,
            "ca_file": env.pg_ca_file
        },
        "bindings": bindings.iter().map(|item| json!({"subject": item.subject, "project_id": item.project_id, "role": item.role.as_str()})).collect::<Vec<_>>(),
        "bootstrap": {"subject": bootstrap.subject, "project_id": bootstrap.project_id, "role": bootstrap.role.as_str()},
        "idle_seconds": 300,
        "absolute_seconds": 300,
        "oauth_ttl_seconds": 60,
        "bootstrap_ttl_seconds": 120,
        "clock_skew_seconds": 0
    });
    fs::write(path, serde_json::to_vec(&body).expect("identity config")).expect("write identity");
}

fn write_api_config(path: &Path, env: &LiveEnv, pack_dir: &Path, origin: &str, identity_origin: &str, port: u16) {
    let pg = |user: &str, password: &Path| {
        json!({
            "ca_file": env.pg_ca_file,
            "database": env.database,
            "host": env.host,
            "password_file": password,
            "port": env.port,
            "user": user
        })
    };
    let body = json!({
        "api_pg": pg("oo_release_api_admin", &env.api_password_file),
        "authority_id": "shop-authority",
        "deployment_origins": [origin],
        "https_origin": origin,
        "identity_ca_file": env.cert_dir.join("ca.pem"),
        "identity_origin": identity_origin,
        "listen_host": "127.0.0.1",
        "listen_port": port,
        "pack_dir": pack_dir,
        "producer_pg": pg("oo_release_admin", &env.producer_password_file),
        "project_id": PROJECT,
        "reader_pg": pg("oo_release_reader", &env.reader_password_file),
        "tls_cert_file": env.cert_dir.join("srv.pem"),
        "tls_key_file": env.cert_dir.join("srv.key")
    });
    fs::write(path, serde_json::to_vec(&body).expect("api config")).expect("write api");
}

async fn probe_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("probe");
    listener.local_addr().expect("probe port").port()
}

async fn connect_as(env: &LiveEnv, user: &str, password_file: &Path) -> Client {
    let password = read_secret(password_file);
    let tls = rustls_connector(&env.ca).expect("tls");
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(&password);
    config.host(&env.host);
    config.port(env.port);
    config.dbname(&env.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("oo-release-control-api-proof");
    let (client, connection) = config.connect(tls).await.expect("pg");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

fn read_secret(path: &Path) -> String {
    let mut text = fs::read_to_string(path).unwrap_or_else(|_| panic!("secret file {}", path.display()));
    if text.ends_with('\n') {
        text.pop();
    }
    text
}

async fn operation_count(env: &LiveEnv) -> i64 {
    let client = connect_as(env, "oo_release_api_admin", &env.api_password_file).await;
    let row = client.query_one("SELECT count(*) FROM oo_release_api.operation", &[]).await.expect("count");
    row.get(0)
}

async fn assert_reader(env: &LiveEnv, version: Option<&str>) {
    let client = connect_as(env, "oo_release_reader", &env.reader_password_file).await;
    let row = client
        .query_one(
            "SELECT version, production_ready_certificate, full_sp08_accepted, public_deployment_claimed, customer_ownership_claimed, standby_configured FROM oo_release_control.publication_status",
            &[],
        )
        .await
        .expect("reader");
    let found: Option<String> = row.get(0);
    assert_eq!(found.as_deref(), version);
    for index in 1..6 {
        assert!(!row.get::<_, bool>(index));
    }
}

async fn assert_admin_cannot_read_view(env: &LiveEnv) {
    let client = connect_as(env, "oo_release_api_admin", &env.api_password_file).await;
    let err = client.query_one("SELECT version FROM oo_release_control.publication_status", &[]).await.expect_err("admin view");
    assert_eq!(err.code().map(|code| code.code()), Some("42501"));
}

async fn assert_producer_state(env: &LiveEnv, candidate: &str, state: &str) {
    let client = connect_as(env, "oo_release_admin", &env.producer_password_file).await;
    let row = client.query_one("SELECT state FROM oo_release_control.candidate WHERE candidate_id = $1", &[&candidate]).await.expect("candidate");
    let found: String = row.get(0);
    assert_eq!(found, state);
}

async fn assert_producer_audit(env: &LiveEnv, candidate: &str, event: &str) {
    let client = connect_as(env, "oo_release_admin", &env.producer_password_file).await;
    let row = client
        .query_one("SELECT count(*) FROM oo_release_control.audit_event WHERE candidate_id = $1 AND event = $2", &[&candidate, &event])
        .await
        .expect("audit");
    let count: i64 = row.get(0);
    assert!(count >= 1, "{event} for {candidate}");
}

async fn assert_audit_immutable(env: &LiveEnv) {
    let client = connect_as(env, "oo_release_api_admin", &env.api_password_file).await;
    let err = client.execute("UPDATE oo_release_api.audit_event SET detail = 'tamper'", &[]).await.expect_err("audit update");
    assert_eq!(err.code().map(|code| code.code()), Some("42501"));
}

async fn insert_running(env: &LiveEnv, operation_id: &str) {
    let client = connect_as(env, "oo_release_api_admin", &env.api_password_file).await;
    let body = sha256_hex(b"a392-running-body");
    client
        .execute(
            "INSERT INTO oo_release_api.operation ( \
                operation_id, project_id, idempotency_key, body_sha256, kind, status, \
                columns_text, review_admitted, certificate_class, production_ready, \
                customer_gate_verified, owner_authenticated, runsc_verified, gates_json, subject_name \
             ) VALUES ($1, 'shop', 'key-running-uncertain', $2, 'publish', 'running', \
                '', FALSE, 'not_issued', FALSE, FALSE, FALSE, FALSE, '[]', 'fixture-admin')",
            &[&operation_id, &body],
        )
        .await
        .expect("insert running");
}

async fn insert_queued(env: &LiveEnv, operation_id: &str, commit: &str, file_hash: &str, columns: &str) {
    let client = connect_as(env, "oo_release_api_admin", &env.api_password_file).await;
    let body = sha256_hex(b"a392-queued-restart");
    let audit_id = sha256_hex(format!("{operation_id}:pack.file").as_bytes());
    client
        .execute(
            "INSERT INTO oo_release_api.operation ( \
                operation_id, project_id, idempotency_key, body_sha256, kind, status, \
                candidate_id, version, base_version, expected_version, role_name, scope_name, \
                config_commit, columns_text, policy_id, policy_version, knowledge_id, knowledge_version, \
                catalog_version, review_admitted, certificate_class, production_ready, \
                customer_gate_verified, owner_authenticated, runsc_verified, gates_json, subject_name \
             ) VALUES ($1, 'shop', 'key-restart-queued1', $2, 'publish', 'queued', \
                'cand-restart', 'ver-restart', 'ver-n2', 'ver-n2', $3, 'test_scope', \
                $4, $5, $6, $7, $8, $9, \
                $10, TRUE, 'not_issued', FALSE, FALSE, FALSE, FALSE, '[]', 'fixture-admin')",
            &[&operation_id, &body, &ROLE, &commit, &columns, &POLICY_ID, &POLICY_VERSION, &KNOWLEDGE_ID, &KNOWLEDGE_VERSION, &CATALOG_VERSION],
        )
        .await
        .expect("insert queued");
    client
        .execute(
            "INSERT INTO oo_release_api.audit_event (audit_id, operation_id, project_id, event_name, detail) VALUES ($1, $2, 'shop', 'pack.file', $3)",
            &[&audit_id, &operation_id, &file_hash],
        )
        .await
        .expect("pack.file");
}

#[derive(Default)]
struct Proof {
    steps: Vec<Value>,
    forbidden: Vec<String>,
}

impl Proof {
    fn forbid(&mut self, value: &str) {
        if !value.is_empty() {
            self.forbidden.push(value.to_string());
        }
    }

    fn step(&mut self, name: &str, status: u16, code: &str, detail: &str, version: Value) {
        self.steps.push(json!({"name": name, "status": status, "code": code, "detail": detail, "version": version}));
    }

    fn finish(&self, env: &LiveEnv) {
        let body = json!({
            "contract": "oo.release-control-api/v1",
            "production_ready": false,
            "full_sp08_accepted": false,
            "customer_approval": "not_issued",
            "test_scope": "not_production",
            "steps": self.steps
        });
        let text = serde_json::to_string_pretty(&body).expect("proof");
        for secret in &self.forbidden {
            assert!(!text.contains(secret), "proof contains a secret");
        }
        if let Some(path) = &env.proof_path {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("proof dir");
            }
            fs::write(path, &text).expect("proof write");
        }
    }
}

struct ServerProc {
    child: Option<std::process::Child>,
    stderr: Arc<Mutex<Vec<u8>>>,
}

impl ServerProc {
    fn spawn(exe: &Path, config: &Path) -> Self {
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
        std::thread::spawn(move || {
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
        std::thread::spawn(move || {
            let mut stderr = stderr;
            let mut local = Vec::new();
            let _ = stderr.read_to_end(&mut local);
            *stderr_buffer.lock().expect("stderr") = local;
        });
        let line = receiver.recv_timeout(Duration::from_secs(30)).expect("listening");
        let text = String::from_utf8(line).expect("utf8");
        let value: Value = serde_json::from_str(text.trim()).unwrap_or(Value::Null);
        assert_eq!(value["outcome"], "listening", "{text}");
        assert_eq!(value["production_ready"], false);
        Self { child: Some(child), stderr: buffer }
    }

    fn kill(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
    }
}

impl Drop for ServerProc {
    fn drop(&mut self) {
        let stderr = String::from_utf8_lossy(&self.stderr.lock().expect("stderr")).to_string();
        self.kill();
        if !stderr.is_empty() {
            eprintln!("api stderr bytes {}", stderr.len());
        }
    }
}

struct TaskGuard(tokio::task::JoinHandle<()>);

impl Drop for TaskGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

