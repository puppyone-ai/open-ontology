//! Real target fence for one non-production shop candidate.
//! Returns immediately unless the harness sets OO_W73_PG_PORT or OO_W73_REQUIRED=1.
//! OO_W73_KILL_STEP runs only the re-execed step so a child does not start another proof.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use postgres_types::ToSql;
use serde_json::{Value, json};
use tokio_postgres::Client;
use tokio_postgres::config::SslMode;

use crate::sql::{self, INSTALL_LOCK};
use crate::store::{
    Endpoint, GateRequest, InstallRequest, PlanRequest, RecoverRequest, StageRequest, activate,
    gate_record, install, recover, stage,
};
use crate::target::{self, FenceRef, TargetConn};
use crate::tls::rustls_connector;
use crate::{
    ControlConn, GATE_IDS, MaintainRequest, StoreError, dispatch, maintain_activate,
    maintain_install, maintain_recover,
};

const PROJECT: &str = "shop";
const POLICY_ID: &str = "pol-shop";
const POLICY_VERSION: &str = "1";
const KNOWLEDGE_ID: &str = "know-shop";
const KNOWLEDGE_VERSION: &str = "1";
const CATALOG_VERSION: &str = "cat-1";
const SCHEMA: &str = "public";
const RELATION: &str = "customer";
const ROLE: &str = "oo_r_clerk";
const FAKE_SOURCE: &str = "abababababababababababababababababababababababababababababababab";

#[derive(Clone)]
struct Env {
    control: Endpoint,
    target: Endpoint,
    admin_password: String,
    reader_password: String,
    target_password: String,
    clerk_password: String,
    probe_password: String,
    super_password: String,
    ca: Vec<u8>,
}

struct Sealed {
    id: String,
    version: String,
    source: String,
    pack: String,
}

fn required() -> bool {
    env::var("OO_W73_REQUIRED").ok().as_deref() == Some("1") || env::var("OO_W73_PG_PORT").is_ok()
}

fn secret(name: &str) -> String {
    let path = env::var(name).unwrap_or_else(|_| panic!("{name} is required"));
    let mut text = fs::read_to_string(&path).unwrap_or_else(|_| panic!("secret {path}"));
    if text.ends_with('\n') {
        text.pop();
    }
    assert!(!text.is_empty(), "{name}");
    text
}

fn load_env() -> Env {
    let port = env::var("OO_W73_PG_PORT")
        .expect("OO_W73_PG_PORT")
        .parse()
        .expect("port");
    let host = env::var("OO_W73_PG_HOST").expect("OO_W73_PG_HOST");
    let ca_file = env::var("OO_W73_CA_FILE").expect("OO_W73_CA_FILE");
    Env {
        control: Endpoint {
            host: host.clone(),
            port,
            database: env::var("OO_W73_CONTROL_DATABASE").expect("control database"),
        },
        target: Endpoint {
            host,
            port,
            database: env::var("OO_W73_TARGET_DATABASE").expect("target database"),
        },
        admin_password: secret("OO_W73_ADMIN_PASSWORD_FILE"),
        reader_password: secret("OO_W73_READER_PASSWORD_FILE"),
        target_password: secret("OO_W73_TARGET_PASSWORD_FILE"),
        clerk_password: secret("OO_W73_ROLE_PASSWORD_FILE"),
        probe_password: secret("OO_W73_PROBE_PASSWORD_FILE"),
        super_password: secret("OO_W73_SUPER_PASSWORD_FILE"),
        ca: fs::read(&ca_file).unwrap_or_else(|_| panic!("ca {ca_file}")),
    }
}

async fn connect(endpoint: &Endpoint, user: &str, password: &str, ca: &[u8], app: &str) -> Client {
    let tls = rustls_connector(ca).expect("tls");
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(password);
    config.host(&endpoint.host);
    config.port(endpoint.port);
    config.dbname(&endpoint.database);
    config.ssl_mode(SslMode::Require);
    config.application_name(app);
    config.connect_timeout(Duration::from_secs(10));
    let (client, connection) = config
        .connect(tls)
        .await
        .unwrap_or_else(|_| panic!("connect {user} {}", endpoint.database));
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

fn control<'a>(env: &'a Env) -> ControlConn<'a> {
    ControlConn {
        endpoint: &env.control,
        user: "oo_release_admin",
        password: &env.admin_password,
        ca_pem: &env.ca,
    }
}

fn target<'a>(env: &'a Env) -> TargetConn<'a> {
    TargetConn {
        endpoint: &env.target,
        user: "oo_target_admin",
        password: &env.target_password,
        ca_pem: &env.ca,
    }
}

fn fence() -> FenceRef<'static> {
    FenceRef {
        schema_name: SCHEMA,
        relation_name: RELATION,
        role_name: ROLE,
    }
}

fn maintain<'a>(
    env: &'a Env,
    sealed: &'a Sealed,
    expected: Option<&'a str>,
) -> MaintainRequest<'a> {
    MaintainRequest {
        replica: None,
        native_mapping: None,
        control: control(env),
        target: target(env),
        reader_user: ROLE,
        reader_password: &env.clerk_password,
        project_id: PROJECT,
        candidate_id: &sealed.id,
        source_hash: &sealed.source,
        expected_version: expected,
        schema_name: SCHEMA,
        relation_name: RELATION,
        role_name: ROLE,
    }
}

fn flags(body: &Value) {
    for key in [
        "full_sp08_accepted",
        "public_deployment_claimed",
        "customer_ownership_claimed",
        "standby_configured",
        "production_ready_certificate",
        "owner_authenticated",
        "credential_verified",
        "integrity_verified",
        "customer_gate_verified",
        "runsc_verified",
    ] {
        assert_eq!(body.get(key), Some(&Value::Bool(false)), "{key} in {body}");
    }
}

fn spawn_step(step: &str, extra: &[(&str, &str)]) -> (std::process::Child, std::path::PathBuf) {
    let exe = env::current_exe().expect("current exe");
    let log_path = env::temp_dir().join(format!("oo-w73-step-{}-{}", step, std::process::id()));
    let marker = log_path.with_extension("mark");
    let log = fs::File::create(&log_path).expect("step log");
    let err = log.try_clone().expect("step log");
    let mut cmd = Command::new(exe);
    cmd.args([
        "--exact",
        "w73::kill_step_driver",
        "--test-threads",
        "1",
        "--nocapture",
    ])
    .env("OO_W73_KILL_STEP", step)
    .env("OO_W73_KILL_LOG", &marker)
    .stdin(Stdio::null())
    .stdout(Stdio::from(log))
    .stderr(Stdio::from(err));
    for (key, value) in extra {
        cmd.env(key, value);
    }
    let child = cmd.spawn().expect("spawn step");
    (child, log_path)
}

async fn wait_lock(
    client: &Client,
    database: &str,
    child: &mut std::process::Child,
    log_path: &std::path::Path,
) -> i32 {
    for _ in 0..160 {
        if let Some(status) = child.try_wait().expect("child status") {
            let logged = fs::read_to_string(log_path).unwrap_or_default();
            let marker = fs::read_to_string(log_path.with_extension("mark")).unwrap_or_default();
            panic!("child exited before the lock: {status}; marker {marker}; child {logged}");
        }
        let row = client
            .query_opt(
                "SELECT pid FROM pg_catalog.pg_stat_activity \
                 WHERE application_name = 'release-coordinator' AND datname = $1 AND wait_event_type = 'Lock' \
                 LIMIT 1",
                &[&database],
            )
            .await
            .expect("stat");
        if let Some(row) = row {
            return row.get(0);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let activity: String = client
        .query(
            "SELECT coalesce(usename, ''), coalesce(datname, ''), coalesce(application_name, ''), \
                    coalesce(state, ''), coalesce(wait_event_type, ''), left(coalesce(query, ''), 180) \
             FROM pg_catalog.pg_stat_activity ORDER BY pid",
            &[],
        )
        .await
        .expect("activity")
        .iter()
        .map(|row| {
            let user: String = row.get(0);
            let db: String = row.get(1);
            let app: String = row.get(2);
            let state: String = row.get(3);
            let wait: String = row.get(4);
            let query: String = row.get(5);
            format!("{user} {db} {app} {state} {wait} {query}")
        })
        .collect::<Vec<_>>()
        .join(" | ");
    let logged = fs::read_to_string(log_path).unwrap_or_default();
    let marker = fs::read_to_string(log_path.with_extension("mark")).unwrap_or_default();
    panic!(
        "child never blocked on {database}; activity {activity}; marker {marker}; child {logged}"
    );
}

async fn terminate_app(client: &Client, database: &str) {
    client
        .execute(
            "SELECT pg_catalog.pg_terminate_backend(pid) FROM pg_catalog.pg_stat_activity \
             WHERE application_name = 'release-coordinator' AND datname = $1 AND pid <> pg_catalog.pg_backend_pid()",
            &[&database],
        )
        .await
        .expect("terminate");
}

async fn text_one(client: &Client, sql: &str) -> Option<String> {
    let row = client.query_one(sql, &[]).await.expect(sql);
    row.get(0)
}

async fn bool_one(client: &Client, sql: &str) -> bool {
    let row = client.query_one(sql, &[]).await.expect(sql);
    row.get(0)
}

async fn privilege(client: &Client, column: &str) -> bool {
    let row = client
        .query_one(
            "SELECT has_column_privilege(current_user, 'public.customer'::regclass, $1, 'SELECT')",
            &[&column],
        )
        .await
        .expect("privilege");
    row.get(0)
}

async fn query_state(client: &Client, sql: &str) -> Result<(), String> {
    match client.query_one(sql, &[]).await {
        Ok(_) => Ok(()),
        Err(err) => Err(err
            .code()
            .map(|code| code.code().to_owned())
            .unwrap_or_else(|| "query".to_owned())),
    }
}

async fn fence_state(env: &Env) -> String {
    let admin = connect(
        &env.target,
        "oo_target_admin",
        &env.target_password,
        &env.ca,
        "w73-holder",
    )
    .await;
    text_one(
        &admin,
        "SELECT state FROM oo_publish_target.fence WHERE singleton",
    )
    .await
    .expect("fence state")
}

async fn pointer_version(env: &Env) -> Option<String> {
    let reader = connect(
        &env.control,
        "oo_release_reader_login",
        &env.reader_password,
        &env.ca,
        "w73-reader",
    )
    .await;
    text_one(
        &reader,
        "SELECT version FROM oo_release_control.publication_status",
    )
    .await
}

async fn stage_one(
    env: &Env,
    id: &str,
    version: &str,
    base: Option<&str>,
    effective: &Value,
    visibility: &Value,
) -> Sealed {
    let receipt = stage(&StageRequest {
        endpoint: &env.control,
        user: "oo_release_admin",
        password: &env.admin_password,
        ca_pem: &env.ca,
        project_id: PROJECT,
        candidate_id: id,
        version,
        base_version: base,
        effective,
        visibility,
        policy_id: POLICY_ID,
        policy_version: POLICY_VERSION,
        knowledge_id: KNOWLEDGE_ID,
        knowledge_version: KNOWLEDGE_VERSION,
        catalog_version: CATALOG_VERSION,
    })
    .await
    .expect("stage");
    assert_eq!(receipt.code, 0, "{:?}", receipt.body);
    assert_eq!(receipt.body["outcome"], "staged", "{:?}", receipt.body);
    assert_eq!(receipt.body["compiler"], "oo-builder/role-pack/v1");
    Sealed {
        id: id.to_owned(),
        version: version.to_owned(),
        source: receipt.body["source_hash"]
            .as_str()
            .expect("source")
            .to_owned(),
        pack: receipt.body["pack_digest"]
            .as_str()
            .expect("pack")
            .to_owned(),
    }
}

async fn kill_table(env: &Env, eye: &Client, step: &str) {
    let mut holder = connect(
        &env.target,
        "oo_target_admin",
        &env.target_password,
        &env.ca,
        "w73-holder",
    )
    .await;
    let tx = holder.transaction().await.expect("holder");
    tx.batch_execute("LOCK TABLE public.customer IN ACCESS EXCLUSIVE MODE")
        .await
        .expect("lock table");
    let (mut child, log_path) = spawn_step(step, &[]);
    let _pid = wait_lock(eye, &env.target.database, &mut child, &log_path).await;
    let _ = child.kill();
    let _ = child.wait();
    terminate_app(eye, &env.target.database).await;
    tx.rollback().await.expect("unlock table");
}

async fn kill_enter(env: &Env, eye: &Client) {
    let holder = connect(
        &env.control,
        "oo_release_admin",
        &env.admin_password,
        &env.ca,
        "w73-holder",
    )
    .await;
    holder
        .query_one(
            "SELECT pg_catalog.pg_advisory_lock($1::bigint)",
            &[&INSTALL_LOCK],
        )
        .await
        .expect("advisory");
    let (mut child, log_path) = spawn_step("enter", &[]);
    let _pid = wait_lock(eye, &env.control.database, &mut child, &log_path).await;
    let _ = child.kill();
    let _ = child.wait();
    terminate_app(eye, &env.control.database).await;
    holder
        .query_one(
            "SELECT pg_catalog.pg_advisory_unlock($1::bigint)",
            &[&INSTALL_LOCK],
        )
        .await
        .expect("unlock");
}

async fn kill_commit(env: &Env, eye: &Client) {
    let mut holder = connect(
        &env.control,
        "oo_release_admin",
        &env.admin_password,
        &env.ca,
        "w73-holder",
    )
    .await;
    let tx = holder.transaction().await.expect("pointer tx");
    tx.query_one(sql::LOCK_POINTER, &[])
        .await
        .expect("pointer lock");
    let (mut child, log_path) = spawn_step("commit", &[]);
    let _pid = wait_lock(eye, &env.control.database, &mut child, &log_path).await;
    let _ = child.kill();
    let _ = child.wait();
    terminate_app(eye, &env.control.database).await;
    tx.rollback().await.expect("unlock pointer");
}

fn activated(code: u8, body: &Value) -> bool {
    code == 0 && body.get("outcome").and_then(Value::as_str) == Some("activated")
}

fn assert_winner(body: &Value) {
    flags(body);
    assert_eq!(body["compiled_pack"], "not_published", "{body}");
    assert_eq!(body["knowledge_published"], true, "{body}");
    assert_eq!(
        body["certificate_class"], "test_scope_not_production",
        "{body}"
    );
    assert_eq!(body["production_blocked"], true, "{body}");
    assert_eq!(body["sessions_terminated_by_tool"], false, "{body}");
    assert_ne!(
        body.get("sessions_terminated"),
        Some(&Value::Bool(true)),
        "{body}"
    );
    let streams = body["streams_stopped"].as_i64().unwrap_or(0);
    assert!(streams >= 1, "streams_stopped {body}");
    for gate in ["G5a", "G5b", "G6", "G10", "G11", "G12"] {
        assert_gate(body, gate, "not_applicable", false);
    }
    assert_gate(body, "G1", "admissible", true);
    assert_gate(body, "G3", "not_run", false);
    assert_gate(body, "G4", "not_run", false);
    assert_gate(body, "G9", "admissible", true);
    assert_gate(body, "G7", "admissible", true);
}

fn assert_gate(body: &Value, gate: &str, disposition: &str, admissible: bool) {
    let gates = body["gates"]
        .as_array()
        .unwrap_or_else(|| panic!("gates {body}"));
    let row = gates
        .iter()
        .find(|item| item["gate_id"] == gate)
        .unwrap_or_else(|| panic!("{gate} missing in {body}"));
    assert_eq!(row["disposition"], disposition, "{gate} {row}");
    assert_eq!(row["test_admissible"], admissible, "{gate} {row}");
    assert_eq!(row["production_admissible"], false, "{gate} {row}");
}

fn raw(label: &str, code: u8, body: &Value) {
    println!("W73_RAW {label} code={code} body={body}");
}

#[tokio::test]
async fn kill_step_driver() {
    let Ok(step) = env::var("OO_W73_KILL_STEP") else {
        return;
    };
    if let Ok(path) = env::var("OO_W73_KILL_LOG") {
        let _ = fs::write(&path, format!("entered {step}\n"));
    }
    let env = load_env();
    let receipt = match step.as_str() {
        "close" => target::close_target(&target(&env), &fence()).await,
        "grant" => {
            target::grant_target(
                &target(&env),
                &fence(),
                &["contact".to_owned(), "id".to_owned()],
            )
            .await
        }
        "observe" => {
            target::observe_target(
                &target(&env),
                &fence(),
                &["contact".to_owned(), "id".to_owned()],
                ROLE,
                &env.clerk_password,
            )
            .await
        }
        "open" => {
            target::open_target(
                &target(&env),
                &fence(),
                &["contact".to_owned(), "id".to_owned()],
                FAKE_SOURCE,
                "ver-kill",
                "cand-kill",
            )
            .await
        }
        "enter" => {
            crate::enter_applying(&PlanRequest {
                endpoint: &env.control,
                user: "oo_release_admin",
                password: &env.admin_password,
                ca_pem: &env.ca,
                project_id: PROJECT,
                candidate_id: "cand-kill",
                source_hash: FAKE_SOURCE,
                scope: "test_scope",
                expected_version: None,
            })
            .await
        }
        "commit" => {
            crate::commit_activation(&PlanRequest {
                endpoint: &env.control,
                user: "oo_release_admin",
                password: &env.admin_password,
                ca_pem: &env.ca,
                project_id: PROJECT,
                candidate_id: "cand-kill",
                source_hash: FAKE_SOURCE,
                scope: "test_scope",
                expected_version: None,
            })
            .await
        }
        "contend" => {
            let id = env::var("OO_W73_KILL_CANDIDATE").expect("candidate");
            let source = env::var("OO_W73_KILL_SOURCE").expect("source");
            let expected = env::var("OO_W73_KILL_EXPECTED")
                .ok()
                .filter(|value| !value.is_empty());
            let sealed = Sealed {
                id,
                version: "ver-child".to_owned(),
                source,
                pack: String::new(),
            };
            let request = MaintainRequest {
                replica: None,
                native_mapping: None,
                control: control(&env),
                target: target(&env),
                reader_user: ROLE,
                reader_password: &env.clerk_password,
                project_id: PROJECT,
                candidate_id: &sealed.id,
                source_hash: &sealed.source,
                expected_version: expected.as_deref(),
                schema_name: SCHEMA,
                relation_name: RELATION,
                role_name: ROLE,
            };
            maintain_activate(&request).await
        }
        other => panic!("unknown kill step {other}"),
    };
    if let Ok(path) = env::var("OO_W73_KILL_LOG") {
        let line = match &receipt {
            Ok(receipt) => format!("result code={} body={}\n", receipt.code, receipt.body),
            Err(err) => format!("result err={err}\n"),
        };
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = file.write_all(line.as_bytes());
        }
    }
    if step == "contend" {
        let path = env::var("OO_W73_RESULT").expect("result path");
        let body = match receipt {
            Ok(receipt) => json!({"code": receipt.code, "body": receipt.body}),
            Err(err) => json!({"code": 1, "error": err.to_string()}),
        };
        fs::write(&path, serde_json::to_vec(&body).expect("json")).expect("result write");
    }
}

async fn counterexamples(env: &Env, effective: &Value, visibility: &Value, authority: &str) {
    let failed = stage_one(env, "cand-wide", "ver-wide", None, effective, visibility).await;
    let g5a = gate_record(&GateRequest {
        endpoint: &env.control,
        user: "oo_release_admin",
        password: &env.admin_password,
        ca_pem: &env.ca,
        project_id: PROJECT,
        candidate_id: &failed.id,
        source_hash: &failed.source,
        gate_id: "G5a",
        issuer_class: "test_issuer",
        status: "fail",
        blocking: true,
        report_type: "test_issuer",
        authority_statement_sha256: None,
    })
    .await
    .expect("g5a record");
    raw("g5a-record", g5a.code, &g5a.body);
    assert_eq!(g5a.code, 0, "{:?}", g5a.body);
    assert_eq!(g5a.body["disposition"], "failed");
    assert_eq!(g5a.body["test_admissible"], false);
    let refused = maintain_activate(&maintain(env, &failed, None))
        .await
        .expect("g5a activate");
    raw("g5a-activate", refused.code, &refused.body);
    assert_eq!(refused.code, 2, "{:?}", refused.body);
    assert_eq!(refused.body["outcome"], "refused");
    assert_eq!(refused.body["reason"], "gate_failed");
    assert_eq!(refused.body["fence_touched"], false);
    assert_eq!(refused.body["knowledge_published"], false);
    assert_eq!(refused.body["compiled_pack"], "not_published");
    assert_gate(&refused.body, "G5a", "failed", false);
    for gate in ["G5b", "G6", "G10", "G11", "G12"] {
        assert_gate(&refused.body, gate, "not_applicable", false);
    }
    assert_eq!(fence_state(env).await, "closed");
    assert!(pointer_version(env).await.is_none());

    let mut blocked_effective = effective.clone();
    blocked_effective["metrics"] = json!([{"metric": "orders"}]);
    blocked_effective["examples"] = json!([{"id": "ex-1"}]);
    blocked_effective["deleted_names"] = json!(["old_note"]);
    let ontology = blocked_effective
        .as_object_mut()
        .expect("effective")
        .entry("ontology")
        .or_insert(json!({}));
    let publish = ontology
        .as_object_mut()
        .expect("ontology")
        .entry("publish")
        .or_insert(json!({}));
    let _ = publish
        .as_object_mut()
        .expect("publish")
        .insert("require_eval".to_owned(), json!(["customer_holdout"]));
    let blocked = stage_one(
        env,
        "cand-blocked",
        "ver-blocked",
        None,
        effective,
        visibility,
    )
    .await;
    let admin = connect(
        &env.control,
        "oo_release_admin",
        &env.admin_password,
        &env.ca,
        "w73-holder",
    )
    .await;
    let planted = blocked_effective.to_string();
    let updated = admin
        .execute(
            "UPDATE oo_release_control.candidate SET effective_oos = $2 WHERE candidate_id = $1 AND state = 'candidate_built'",
            &[&blocked.id, &planted],
        )
        .await
        .expect("plant blocked inputs");
    assert_eq!(updated, 1);
    drop(admin);
    let blocked_activate = maintain_activate(&maintain(env, &blocked, None))
        .await
        .expect("blocked activate");
    raw(
        "blocked-activate",
        blocked_activate.code,
        &blocked_activate.body,
    );
    assert_eq!(blocked_activate.code, 2, "{:?}", blocked_activate.body);
    assert_eq!(blocked_activate.body["outcome"], "refused");
    assert_eq!(blocked_activate.body["reason"], "blocked");
    assert_eq!(blocked_activate.body["fence_touched"], false);
    for gate in ["G5a", "G5b", "G6", "G10", "G11", "G12"] {
        assert_gate(&blocked_activate.body, gate, "blocked", false);
    }
    let admin = connect(
        &env.control,
        "oo_release_admin",
        &env.admin_password,
        &env.ca,
        "w73-holder",
    )
    .await;
    for gate in ["G5a", "G5b", "G10"] {
        let detail: String = admin
            .query_one(
                "SELECT detail FROM oo_release_control.maintenance_gate WHERE candidate_id = $1 AND gate_id = $2",
                &[&blocked.id, &gate],
            )
            .await
            .unwrap_or_else(|_| panic!("blocked detail {gate}"))
            .get(0);
        println!("W73_RAW blocked-detail gate={gate} detail={detail}");
        match gate {
            "G5a" => assert_eq!(detail, "certified_metric_without_independent_control_total"),
            "G5b" => assert_eq!(detail, "control_total_not_supplied"),
            "G10" => assert_eq!(detail, "customer_eval_not_arrived"),
            _ => {}
        }
        assert_ne!(detail, "control_total_arrived");
    }
    let arrived: i64 = admin
        .query_one("SELECT count(*) FROM oo_release_control.maintenance_gate WHERE detail = 'control_total_arrived'", &[])
        .await
        .expect("control total")
        .get(0);
    assert_eq!(arrived, 0);
    drop(admin);
    assert_eq!(fence_state(env).await, "closed");
    assert!(pointer_version(env).await.is_none());

    let caller = stage_one(
        env,
        "cand-caller",
        "ver-caller",
        None,
        effective,
        visibility,
    )
    .await;
    let forged = gate_record(&GateRequest {
        endpoint: &env.control,
        user: "oo_release_admin",
        password: &env.admin_password,
        ca_pem: &env.ca,
        project_id: PROJECT,
        candidate_id: &caller.id,
        source_hash: &caller.source,
        gate_id: "G1",
        issuer_class: "fixture_authority",
        status: "pass",
        blocking: false,
        report_type: "fixture_certificate",
        authority_statement_sha256: Some(authority),
    })
    .await
    .expect("caller gate");
    raw("cand-caller", forged.code, &forged.body);
    assert_eq!(forged.code, 0, "{:?}", forged.body);
    assert_eq!(forged.body["disposition"], "foreign_receipt");
    assert_eq!(forged.body["test_admissible"], false);
    assert_eq!(forged.body["detail"], "caller_receipt");

    let other_project = maintain_install(&control(env), "other")
        .await
        .expect("other project");
    raw("second-project", other_project.code, &other_project.body);
    assert_eq!(other_project.code, 2, "{:?}", other_project.body);
    assert_eq!(other_project.body["reason"], "project_binding");

    let superuser = connect(
        &env.target,
        "oo_fixture_admin",
        &env.super_password,
        &env.ca,
        "w73-eye",
    )
    .await;
    superuser
        .batch_execute("DO $$ BEGIN CREATE ROLE oo_r_other NOLOGIN; EXCEPTION WHEN duplicate_object THEN NULL; END $$;")
        .await
        .expect("second role");
    drop(superuser);
    let other_fence = FenceRef {
        schema_name: SCHEMA,
        relation_name: RELATION,
        role_name: "oo_r_other",
    };
    let other_role = target::install_target(&target(env), &other_fence).await;
    match other_role {
        Err(StoreError::Input) => {
            println!("W73_RAW second-role error=input unsupported_binding_not_isolation")
        }
        Err(err) => panic!("second role must be an input error, not an isolation proof: {err}"),
        Ok(receipt) => panic!(
            "second role returned a receipt, which is not an isolation proof: code={}",
            receipt.code
        ),
    }
    assert_eq!(fence_state(env).await, "closed");

    let fault = stage_one(env, "cand-fault", "ver-fault", None, effective, visibility).await;
    let result_path = env::temp_dir().join(format!("oo-w73-fault-{}", std::process::id()));
    let result_text = result_path.display().to_string();
    let (mut child, _log_path) = spawn_step(
        "contend",
        &[
            ("OO_W73_OPEN_FAULT", "1"),
            ("OO_W73_KILL_CANDIDATE", &fault.id),
            ("OO_W73_KILL_SOURCE", &fault.source),
            ("OO_W73_KILL_EXPECTED", ""),
            ("OO_W73_RESULT", &result_text),
        ],
    );
    let waited = tokio::task::spawn_blocking(move || child.wait())
        .await
        .expect("join")
        .expect("fault child");
    assert!(waited.success(), "open-fault child {waited}");
    let fault_value: Value =
        serde_json::from_slice(&fs::read(&result_path).expect("fault result")).expect("fault json");
    let _ = fs::remove_file(&result_path);
    println!("W73_RAW open-fault {fault_value}");
    assert_eq!(fault_value["code"], json!(2), "{fault_value}");
    assert_eq!(
        fault_value["body"]["reason"], "open_failed",
        "{fault_value}"
    );
    assert_eq!(
        fault_value["body"]["open_sqlstate"], "22012",
        "{fault_value}"
    );
    assert_eq!(
        fault_value["body"]["rollback_outcome"], "rolled_back",
        "{fault_value}"
    );
    assert_eq!(
        fault_value["body"]["fence_state"], "closed",
        "{fault_value}"
    );
    assert_eq!(
        fault_value["body"]["knowledge_published"], false,
        "{fault_value}"
    );
    assert_eq!(
        fault_value["body"]["compiled_pack"], "not_published",
        "{fault_value}"
    );
    assert!(
        fault_value["body"]["pointer_version"].is_null(),
        "{fault_value}"
    );
    assert_eq!(fence_state(env).await, "closed");
    assert!(pointer_version(env).await.is_none());
    let admin = connect(
        &env.target,
        "oo_target_admin",
        &env.target_password,
        &env.ca,
        "w73-holder",
    )
    .await;
    admin
        .batch_execute(
            "ALTER TABLE public.customer NO FORCE ROW LEVEL SECURITY; \
             ALTER TABLE public.customer DISABLE ROW LEVEL SECURITY; \
             DROP POLICY IF EXISTS oo_fence ON public.customer;",
        )
        .await
        .expect("restore closed table");
    assert!(!bool_one(&admin, "SELECT c.relrowsecurity FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'public' AND c.relname = 'customer'").await);
    drop(admin);
}

#[tokio::test]
async fn publish_maintenance_fence() {
    if env::var("OO_W73_KILL_STEP").is_ok() || !required() {
        return;
    }
    let env = load_env();
    let (wide_effective, wide_visibility) = crate::live::shop(&["contact", "id"]);
    let (_same, narrow_visibility) = crate::live::shop(&["id"]);
    let covers: Vec<String> = GATE_IDS.iter().map(|gate| (*gate).to_owned()).collect();
    let installed = install(&InstallRequest {
        endpoint: &env.control,
        user: "oo_release_admin",
        password: &env.admin_password,
        ca_pem: &env.ca,
        project_id: PROJECT,
        authority_id: "w73-shop",
        covers: &covers,
    })
    .await
    .expect("install");
    assert_eq!(installed.code, 0, "{:?}", installed.body);
    assert!(
        matches!(
            installed.body["outcome"].as_str(),
            Some("installed" | "replay")
        ),
        "{:?}",
        installed.body
    );
    let maintained = maintain_install(&control(&env), PROJECT)
        .await
        .expect("maintain install");
    assert_eq!(maintained.code, 0, "{:?}", maintained.body);
    let again = maintain_install(&control(&env), PROJECT)
        .await
        .expect("maintain replay");
    assert_eq!(again.body["outcome"], "replay");
    assert_eq!(again.body["sql_sha256"], maintained.body["sql_sha256"]);
    let target_installed = target::install_target(&target(&env), &fence())
        .await
        .expect("target install");
    assert_eq!(target_installed.code, 0, "{:?}", target_installed.body);

    let admin = connect(
        &env.target,
        "oo_target_admin",
        &env.target_password,
        &env.ca,
        "w73-holder",
    )
    .await;
    admin
        .execute(
            "INSERT INTO public.customer (id, contact, note) VALUES (1, 'ada', 'hidden')",
            &[],
        )
        .await
        .expect("insert");
    assert!(!bool_one(&admin, "SELECT c.relrowsecurity FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'public' AND c.relname = 'customer'").await);
    drop(admin);
    let authority = installed.body["authority_statement_sha256"]
        .as_str()
        .expect("authority")
        .to_owned();
    counterexamples(&env, &wide_effective, &wide_visibility, &authority).await;

    let wide_a = stage_one(
        &env,
        "cand-w",
        "ver-w",
        None,
        &wide_effective,
        &wide_visibility,
    )
    .await;
    let wide_b = stage_one(
        &env,
        "cand-x",
        "ver-x",
        None,
        &wide_effective,
        &wide_visibility,
    )
    .await;
    assert_ne!(wide_a.source, wide_b.source);
    assert!(
        pointer_version(&env).await.is_none(),
        "candidate is public before activation"
    );

    let eye = connect(
        &Endpoint {
            host: env.control.host.clone(),
            port: env.control.port,
            database: "postgres".to_owned(),
        },
        "oo_fixture_admin",
        &env.super_password,
        &env.ca,
        "w73-eye",
    )
    .await;
    for step in ["close", "grant", "open"] {
        kill_table(&env, &eye, step).await;
        assert_eq!(fence_state(&env).await, "closed");
        assert!(pointer_version(&env).await.is_none());
    }
    let admin = connect(
        &env.target,
        "oo_target_admin",
        &env.target_password,
        &env.ca,
        "w73-holder",
    )
    .await;
    assert!(!bool_one(&admin, "SELECT c.relrowsecurity FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'public' AND c.relname = 'customer'").await);
    drop(admin);
    kill_enter(&env, &eye).await;
    kill_commit(&env, &eye).await;
    assert!(pointer_version(&env).await.is_none());

    let closed = target::close_target(&target(&env), &fence())
        .await
        .expect("close");
    assert_eq!(closed.code, 0, "{:?}", closed.body);
    assert_eq!(closed.body["fence_state"], "closed");
    kill_table(&env, &eye, "observe").await;
    assert_eq!(fence_state(&env).await, "closed");
    assert!(pointer_version(&env).await.is_none());

    let granted = target::grant_target(
        &target(&env),
        &fence(),
        &["contact".to_owned(), "id".to_owned()],
    )
    .await
    .expect("grant");
    assert_eq!(granted.code, 0, "{:?}", granted.body);
    assert_eq!(granted.body["fence_state"], "granted");
    let cas = maintain_activate(&MaintainRequest {
        replica: None,
        native_mapping: None,
        control: control(&env),
        target: target(&env),
        reader_user: ROLE,
        reader_password: &env.clerk_password,
        project_id: PROJECT,
        candidate_id: "cand-cas",
        source_hash: FAKE_SOURCE,
        expected_version: Some("nope"),
        schema_name: SCHEMA,
        relation_name: RELATION,
        role_name: ROLE,
    })
    .await
    .expect("cas");
    assert_eq!(cas.code, 2, "{:?}", cas.body);
    assert_eq!(cas.body["reason"], "cas_conflict");
    assert_eq!(cas.body["fence_touched"], false);
    assert_eq!(fence_state(&env).await, "granted");
    let clerk = connect(&env.target, ROLE, &env.clerk_password, &env.ca, "w73-clerk").await;
    assert!(privilege(&clerk, "id").await);
    assert!(!privilege(&clerk, "note").await);
    let count: i64 = clerk
        .query_one("SELECT count(*) FROM public.customer", &[])
        .await
        .expect("count")
        .get(0);
    assert_eq!(
        count, 0,
        "policy false keeps the granted role from seeing the row"
    );
    drop(clerk);
    let revoked = target::revoke_target(&target(&env), &fence())
        .await
        .expect("revoke");
    assert_eq!(revoked.code, 0, "{:?}", revoked.body);
    assert_eq!(fence_state(&env).await, "closed");
    let clerk = connect(&env.target, ROLE, &env.clerk_password, &env.ca, "w73-clerk").await;
    assert_eq!(
        query_state(&clerk, "SELECT id FROM public.customer").await,
        Err("42501".to_owned())
    );
    assert!(!privilege(&clerk, "note").await);
    drop(clerk);
    let probe = connect(
        &env.target,
        "oo_probe",
        &env.probe_password,
        &env.ca,
        "w73-probe",
    )
    .await;
    assert_eq!(
        query_state(&probe, "SELECT id FROM public.customer").await,
        Err("42501".to_owned())
    );
    drop(probe);

    let sleeper = connect(&env.target, ROLE, &env.clerk_password, &env.ca, "w73-clerk").await;
    tokio::spawn(async move {
        let _ = sleeper
            .query_one("SELECT pg_catalog.pg_sleep(180)", &[])
            .await;
    });
    let mut seen = false;
    for _ in 0..40 {
        let n: i64 = eye
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_stat_activity WHERE usename = 'oo_r_clerk' AND datname = $1 AND state = 'active'",
                &[&env.target.database],
            )
            .await
            .expect("clerk stat")
            .get(0);
        if n >= 1 {
            seen = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(seen, "idle clerk was not visible");

    let result_path = env::temp_dir().join(format!("oo-w73-contend-{}", std::process::id()));
    let result_text = result_path.display().to_string();
    let (mut child, _log_path) = spawn_step(
        "contend",
        &[
            ("OO_W73_KILL_CANDIDATE", &wide_b.id),
            ("OO_W73_KILL_SOURCE", &wide_b.source),
            ("OO_W73_KILL_EXPECTED", ""),
            ("OO_W73_RESULT", &result_text),
        ],
    );
    let parent = maintain_activate(&maintain(&env, &wide_a, None))
        .await
        .expect("race parent");
    let waited = tokio::task::spawn_blocking(move || child.wait())
        .await
        .expect("join")
        .expect("child wait");
    assert!(waited.success(), "contend child status {waited}");
    let child_value: Value =
        serde_json::from_slice(&fs::read(&result_path).expect("result")).expect("result json");
    let _ = fs::remove_file(&result_path);
    let child_code = u8::try_from(child_value["code"].as_u64().unwrap_or(9)).unwrap_or(9);
    let child_body = child_value.get("body").cloned().unwrap_or(Value::Null);
    let parent_won = activated(parent.code, &parent.body);
    let child_won = activated(child_code, &child_body);
    assert!(
        parent_won ^ child_won,
        "parent {:?} child {}",
        parent.body,
        child_value
    );
    let winner_body = if parent_won {
        parent.body.clone()
    } else {
        child_body.clone()
    };
    let loser_body = if parent_won {
        child_body
    } else {
        parent.body.clone()
    };
    raw("race-winner", 0, &winner_body);
    raw(
        "race-loser",
        if parent_won { child_code } else { parent.code },
        &loser_body,
    );
    let (winner, loser) = if parent_won {
        (&wide_a, &wide_b)
    } else {
        (&wide_b, &wide_a)
    };
    print_measured_gates(&env, &winner.id).await;
    assert_winner(&winner_body);
    assert_eq!(loser_body["reason"], "cas_conflict", "{loser_body}");
    assert_eq!(loser_body["fence_touched"], false, "{loser_body}");
    assert_eq!(
        pointer_version(&env).await.as_deref(),
        Some(winner.version.as_str())
    );
    assert_eq!(fence_state(&env).await, "open");
    let clerk = connect(&env.target, ROLE, &env.clerk_password, &env.ca, "w73-clerk").await;
    assert!(privilege(&clerk, "id").await);
    assert!(privilege(&clerk, "contact").await);
    assert!(!privilege(&clerk, "note").await);
    assert!(
        query_state(&clerk, "SELECT id, contact FROM public.customer")
            .await
            .is_ok()
    );
    drop(clerk);
    let fence_admin = connect(
        &env.target,
        "oo_target_admin",
        &env.target_password,
        &env.ca,
        "w73-holder",
    )
    .await;
    let policy: String = fence_admin
        .query_one("SELECT pg_catalog.pg_get_expr(polqual, polrelid) FROM pg_catalog.pg_policy WHERE polname = 'oo_fence'", &[])
        .await
        .expect("policy")
        .get(0);
    let opened_columns: Vec<String> = fence_admin
        .query_one(
            "SELECT opened_columns FROM oo_publish_target.fence WHERE singleton",
            &[],
        )
        .await
        .expect("opened")
        .get(0);
    println!("W73_RAW open-policy {policy} opened={opened_columns:?}");
    assert!(matches!(policy.as_str(), "true" | "(true)"), "{policy}");
    assert_eq!(opened_columns, vec!["contact".to_owned(), "id".to_owned()]);
    drop(fence_admin);
    let reader = connect(
        &env.control,
        "oo_release_reader_login",
        &env.reader_password,
        &env.ca,
        "w73-reader",
    )
    .await;
    let shown: String = reader
        .query_one(
            "SELECT candidate_id FROM oo_release_control.publication_status",
            &[],
        )
        .await
        .expect("view")
        .get(0);
    assert_eq!(shown, winner.id);
    assert_ne!(shown, loser.id);
    drop(reader);

    let control_admin = connect(
        &env.control,
        "oo_release_admin",
        &env.admin_password,
        &env.ca,
        "w73-holder",
    )
    .await;
    for gate in ["G3", "G4"] {
        let row = control_admin
            .query_one(
                "SELECT status, disposition, report_type FROM oo_release_control.gate_report WHERE candidate_id = $1 AND gate_id = $2 AND issuer_class = 'fixture_authority'",
                &[&winner.id, &gate],
            )
            .await
            .unwrap_or_else(|_| panic!("{gate}"));
        let status: String = row.get(0);
        let disposition: String = row.get(1);
        let report_type: String = row.get(2);
        assert_eq!(status, "warn", "{gate}");
        assert_eq!(disposition, "not_run", "{gate}");
        assert_eq!(report_type, "target_measurement", "{gate}");
    }
    let g7 = control_admin
        .query_one(
            "SELECT status, disposition, report_type, detail FROM oo_release_control.gate_report WHERE candidate_id = $1 AND gate_id = 'G7'",
            &[&winner.id],
        )
        .await
        .expect("g7 row");
    let g7_status: String = g7.get(0);
    let g7_disposition: String = g7.get(1);
    let g7_report: String = g7.get(2);
    let g7_detail: String = g7.get(3);
    println!(
        "W73_RAW g7-after-open status={g7_status} disposition={g7_disposition} report_type={g7_report} detail={g7_detail}"
    );
    assert_eq!(g7_status, "pass");
    assert_eq!(g7_disposition, "admissible");
    assert_eq!(g7_report, "target_measurement");
    assert_eq!(g7_detail, "open_grant_matches_pointer_columns");
    let replica = control_admin
        .query_one(
            "SELECT barrier, sessions_terminated, disposition FROM oo_release_control.checkpoint WHERE candidate_id = $1 AND kind = 'replica'",
            &[&winner.id],
        )
        .await
        .expect("replica checkpoint");
    let barrier: String = replica.get(0);
    let sessions_terminated: bool = replica.get(1);
    let replica_disposition: String = replica.get(2);
    println!(
        "W73_RAW replica barrier={barrier} sessions_terminated={sessions_terminated} disposition={replica_disposition}"
    );
    assert_eq!(barrier, "missing");
    assert!(!sessions_terminated);
    assert_eq!(replica_disposition, "not_fresh");
    let replayed: i64 = control_admin
        .query_one(
            "SELECT count(*) FROM oo_release_control.checkpoint WHERE barrier = 'replayed'",
            &[],
        )
        .await
        .expect("replayed")
        .get(0);
    assert_eq!(replayed, 0);
    let passes: i64 = control_admin
        .query_one(
            "SELECT count(*) FROM oo_release_control.gate_report \
             WHERE candidate_id = $1 AND gate_id IN ('G5a', 'G5b', 'G6', 'G10', 'G11', 'G12') \
               AND issuer_class = 'fixture_authority' AND status = 'pass'",
            &[&winner.id],
        )
        .await
        .expect("fixture passes")
        .get(0);
    assert_eq!(passes, 0);
    for gate in ["G5a", "G5b", "G6", "G10", "G11", "G12"] {
        let disposition: String = control_admin
            .query_one(
                "SELECT disposition FROM oo_release_control.maintenance_gate WHERE candidate_id = $1 AND gate_id = $2",
                &[&winner.id, &gate],
            )
            .await
            .unwrap_or_else(|_| panic!("maintenance {gate}"))
            .get(0);
        assert_eq!(disposition, "not_applicable", "{gate}");
    }
    drop(control_admin);

    let mut gateway = GatewayTools::load();
    gateway_publication(
        &env,
        &mut gateway,
        "wide",
        &winner.version,
        &winner.pack,
        &wide_effective,
        &wide_visibility,
        &["contact", "id"],
        &[
            Probe {
                name: "wide-read",
                sql: "SELECT id, contact FROM public.customer WHERE id = 1",
                ok: true,
                needle: "ada",
                absent: "hidden",
            },
            Probe {
                name: "wide-note",
                sql: "SELECT note FROM public.customer WHERE id = 1",
                ok: false,
                needle: "",
                absent: "hidden",
            },
        ],
    )
    .await;

    let produced = activate(&PlanRequest {
        endpoint: &env.control,
        user: "oo_release_admin",
        password: &env.admin_password,
        ca_pem: &env.ca,
        project_id: PROJECT,
        candidate_id: &winner.id,
        source_hash: &winner.source,
        scope: "production",
        expected_version: None,
    })
    .await
    .expect("production");
    assert_eq!(produced.code, 2, "{:?}", produced.body);
    assert_eq!(produced.body["reason"], "production_blocked");
    flags(&produced.body);
    assert_eq!(fence_state(&env).await, "open");
    assert_eq!(
        pointer_version(&env).await.as_deref(),
        Some(winner.version.as_str())
    );

    let narrow = stage_one(
        &env,
        "cand-n",
        "ver-n",
        Some(&winner.version),
        &wide_effective,
        &narrow_visibility,
    )
    .await;
    let shrunk = maintain_activate(&maintain(&env, &narrow, Some(&winner.version)))
        .await
        .expect("shrink");
    assert_eq!(shrunk.code, 0, "{:?}", shrunk.body);
    assert_eq!(shrunk.body["outcome"], "activated");
    assert_eq!(shrunk.body["compiled_pack"], "not_published");
    assert_eq!(shrunk.body["knowledge_published"], true);
    assert_eq!(
        shrunk.body["certificate_class"],
        "test_scope_not_production"
    );
    let clerk = connect(&env.target, ROLE, &env.clerk_password, &env.ca, "w73-clerk").await;
    assert!(privilege(&clerk, "id").await);
    assert!(
        !privilege(&clerk, "contact").await,
        "shrink revokes contact"
    );
    assert!(!privilege(&clerk, "note").await);
    assert!(
        query_state(&clerk, "SELECT id FROM public.customer")
            .await
            .is_ok()
    );
    assert_eq!(
        query_state(&clerk, "SELECT contact FROM public.customer").await,
        Err("42501".to_owned())
    );
    drop(clerk);
    gateway_publication(
        &env,
        &mut gateway,
        "narrow",
        &narrow.version,
        &narrow.pack,
        &wide_effective,
        &narrow_visibility,
        &["id"],
        &[
            Probe {
                name: "narrow-read",
                sql: "SELECT id FROM public.customer WHERE id = 1",
                ok: true,
                needle: "\"id\"",
                absent: "ada",
            },
            Probe {
                name: "narrow-contact",
                sql: "SELECT contact FROM public.customer WHERE id = 1",
                ok: false,
                needle: "",
                absent: "ada",
            },
        ],
    )
    .await;
    drop(gateway);
    let rolled = recover(&RecoverRequest {
        endpoint: &env.control,
        user: "oo_release_admin",
        password: &env.admin_password,
        ca_pem: &env.ca,
        action: "rollback",
        project_id: PROJECT,
        candidate_id: &narrow.id,
        expected_version: Some(&narrow.version),
    })
    .await
    .expect("rollback");
    assert_eq!(rolled.code, 0, "{:?}", rolled.body);
    assert_eq!(rolled.body["outcome"], "rolled_back");
    let recovered = maintain_recover(&maintain(&env, &narrow, Some(&narrow.version)))
        .await
        .expect("maintain recover");
    raw("maintain-recover", recovered.code, &recovered.body);
    assert_eq!(recovered.code, 0, "{:?}", recovered.body);
    assert_eq!(
        recovered.body["knowledge_published"], false,
        "{:?}",
        recovered.body
    );
    assert_eq!(
        recovered.body["compiled_pack"], "not_published",
        "{:?}",
        recovered.body
    );
    assert_eq!(
        recovered.body["certificate_class"], "not_issued",
        "{:?}",
        recovered.body
    );
    let clerk = connect(&env.target, ROLE, &env.clerk_password, &env.ca, "w73-clerk").await;
    assert!(privilege(&clerk, "id").await);
    assert!(
        !privilege(&clerk, "contact").await,
        "rollback must not grant contact again"
    );
    assert!(!privilege(&clerk, "note").await);
    let contact_state = query_state(&clerk, "SELECT contact FROM public.customer").await;
    println!("W73_RAW recover-contact {contact_state:?}");
    assert_eq!(contact_state, Err("42501".to_owned()));
    drop(clerk);
    assert_eq!(
        pointer_version(&env).await.as_deref(),
        Some(winner.version.as_str())
    );
    multi_dataset_publication(&env, &winner.version).await;
    let control_admin = connect(
        &env.control,
        "oo_release_admin",
        &env.admin_password,
        &env.ca,
        "w73-holder",
    )
    .await;
    let stored_pack: String = control_admin
        .query_one(
            "SELECT pack_digest FROM oo_release_control.candidate WHERE candidate_id = $1",
            &[&winner.id],
        )
        .await
        .expect("old pack")
        .get(0);
    let stored_source: String = control_admin
        .query_one(
            "SELECT source_hash FROM oo_release_control.candidate WHERE candidate_id = $1",
            &[&winner.id],
        )
        .await
        .expect("old source")
        .get(0);
    assert_eq!(stored_pack, winner.pack);
    assert_eq!(stored_source, winner.source);
    drop(control_admin);

    let args = vec![
        "maintain-activate".to_owned(),
        "--config".to_owned(),
        "unused.json".to_owned(),
    ];
    let force = dispatch(
        &args,
        br#"{"protocol":"oo.release-coordinator/maintain-activate/v1","force":true}"#,
    )
    .await;
    assert_eq!(force.code, 1);
    assert!(force.stdout.is_empty());
}

async fn print_measured_gates(env: &Env, candidate_id: &str) {
    let admin = connect(
        &env.control,
        "oo_release_admin",
        &env.admin_password,
        &env.ca,
        "w73-holder",
    )
    .await;
    for gate in ["G1", "G9"] {
        let row = admin
            .query_one(
                "SELECT status, disposition, report_type, detail FROM oo_release_control.gate_report \
                 WHERE candidate_id = $1 AND gate_id = $2 AND issuer_class = 'fixture_authority' AND report_type = 'target_measurement'",
                &[&candidate_id, &gate],
            )
            .await
            .unwrap_or_else(|_| panic!("{gate} measurement"));
        let status: String = row.get(0);
        let disposition: String = row.get(1);
        let report_type: String = row.get(2);
        let detail: String = row.get(3);
        println!(
            "W73_RAW measured gate={gate} status={status} disposition={disposition} report_type={report_type} detail={detail}"
        );
        assert_eq!(status, "pass", "{gate} {detail}");
        assert_eq!(disposition, "admissible", "{gate} {detail}");
        assert_eq!(report_type, "target_measurement");
        if gate == "G1" {
            assert!(
                detail.contains("json_schema_applied;oos-v1.json;sha256="),
                "{detail}"
            );
            assert!(!detail.contains("json_schema_rejected"), "{detail}");
            assert!(!detail.contains("json_schema_file_not_applied"), "{detail}");
        } else {
            assert!(detail.contains("token_budget_measured;"), "{detail}");
            assert!(detail.contains("estimator=unicode_word"), "{detail}");
            assert!(!detail.contains("token_budget_not_measured"), "{detail}");
        }
    }
}

struct Probe {
    name: &'static str,
    sql: &'static str,
    ok: bool,
    needle: &'static str,
    absent: &'static str,
}

struct GatewayTools {
    oo: String,
    gateway: String,
    issue: String,
    root: PathBuf,
    token: PathBuf,
    token_admin_password: PathBuf,
    runtime_password: String,
    issued: bool,
}

struct GatewayStop {
    child: Option<std::process::Child>,
}

impl Drop for GatewayStop {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for GatewayTools {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.token);
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl GatewayTools {
    fn load() -> Self {
        let oo = env::var("OO_W73_OO_BIN").unwrap_or_default();
        let gateway = env::var("OO_W73_GATEWAY_BIN").unwrap_or_default();
        let issue = env::var("OO_W73_ISSUE_BIN").unwrap_or_default();
        let token_admin = env::var("OO_W73_TOKEN_ADMIN_PASSWORD_FILE").unwrap_or_default();
        assert!(
            !oo.is_empty() && !gateway.is_empty() && !issue.is_empty(),
            "fixed gateway bins are required"
        );
        assert!(
            !token_admin.is_empty(),
            "token admin password file is required"
        );
        let id = std::process::id();
        Self {
            oo,
            gateway,
            issue,
            root: env::temp_dir().join(format!("oo-w73-gw-{id}")),
            token: env::temp_dir().join(format!("oo-w73-client-token-{id}")),
            token_admin_password: PathBuf::from(token_admin),
            runtime_password: secret("OO_W73_TOKEN_RUNTIME_PASSWORD_FILE"),
            issued: false,
        }
    }
}

async fn set_clerk_login(env: &Env, login: bool) {
    let eye = connect(
        &env.target,
        "oo_fixture_admin",
        &env.super_password,
        &env.ca,
        "w73-eye",
    )
    .await;
    let sql = if login {
        "ALTER ROLE oo_r_clerk LOGIN"
    } else {
        "ALTER ROLE oo_r_clerk NOLOGIN"
    };
    eye.batch_execute(sql).await.expect("clerk login flag");
}

#[allow(clippy::too_many_arguments)]
async fn gateway_publication(
    env: &Env,
    tools: &mut GatewayTools,
    label: &'static str,
    version: &str,
    pack_digest: &str,
    effective: &Value,
    visibility: &Value,
    columns: &'static [&'static str],
    probes: &'static [Probe],
) {
    let task_env = env.clone();
    let bits = tools.prepare(env.ca.clone(), &env.target_password, &env.target);
    let version = version.to_owned();
    let pack_digest = pack_digest.to_owned();
    let effective = effective.clone();
    let visibility = visibility.clone();
    set_clerk_login(env, false).await;
    let joined = tokio::spawn(async move {
        run_gateway_probes(
            task_env,
            bits,
            label,
            &version,
            &pack_digest,
            effective,
            visibility,
            columns,
            probes,
        )
        .await;
    })
    .await;
    set_clerk_login(env, true).await;
    joined.expect("gateway probe");
    tools.issued = true;
}

struct GatewayBits {
    oo: String,
    gateway: String,
    issue: String,
    root: PathBuf,
    token: PathBuf,
    token_admin_password: PathBuf,
    ca: Vec<u8>,
    target_password: String,
    runtime_password: String,
    host: String,
    port: u16,
    issued: bool,
}

impl GatewayTools {
    fn prepare(&mut self, ca: Vec<u8>, target_password: &str, target: &Endpoint) -> GatewayBits {
        GatewayBits {
            oo: self.oo.clone(),
            gateway: self.gateway.clone(),
            issue: self.issue.clone(),
            root: self.root.clone(),
            token: self.token.clone(),
            token_admin_password: self.token_admin_password.clone(),
            ca,
            target_password: target_password.to_owned(),
            runtime_password: self.runtime_password.clone(),
            host: target.host.clone(),
            port: target.port,
            issued: self.issued,
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_gateway_probes(
    env: Env,
    bits: GatewayBits,
    label: &str,
    version: &str,
    pack_digest: &str,
    effective: Value,
    visibility: Value,
    columns: &'static [&'static str],
    probes: &'static [Probe],
) {
    install_physical_adapter(&env, columns).await;
    let pack = crate::seal::compiled_pack(&effective, &visibility).expect("compiled pack");
    let artifact: Value = serde_json::from_str(&pack.artifact).expect("artifact json");
    let digest = artifact["pack_digest"].as_str().unwrap_or("").to_owned();
    assert_eq!(digest, pack_digest, "gateway pack is not this publication");
    write_gateway_dir(&bits, &visibility, columns, &pack.artifact);
    if !bits.issued {
        issue_client_token(&bits);
    }
    let token = fs::read_to_string(&bits.token).unwrap_or_else(|_| panic!("token file"));
    let token = token.trim().to_owned();
    assert!(!token.is_empty(), "token file was empty");
    let secrets = [
        token.clone(),
        bits.runtime_password.clone(),
        bits.target_password.clone(),
    ];
    let stderr_path = bits.root.join("gateway.stderr");
    let stderr = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&stderr_path)
        .expect("gateway stderr");
    let mut stop = GatewayStop {
        child: Some(
            Command::new(&bits.gateway)
                .arg(&bits.root)
                .stdout(Stdio::null())
                .stderr(Stdio::from(stderr))
                .spawn()
                .expect("spawn gateway"),
        ),
    };
    let bound = wait_listen(
        &bits.root.join("listen.addr"),
        stop.child.as_mut().expect("child"),
        &stderr_path,
        &secrets,
    )
    .await;
    println!(
        "W73_RAW gateway-context label={label} version={version} role={ROLE} columns={columns:?} pack={digest} listen={bound}"
    );
    let origin = format!("http://{bound}");
    let mut failures = Vec::new();
    for probe in probes {
        let output = run_query(&bits.oo, &bits.token, &origin, probe.sql);
        let code = output.status.code().unwrap_or(9);
        let stdout = scrub(&String::from_utf8_lossy(&output.stdout), &secrets);
        let stderr_text = scrub(&String::from_utf8_lossy(&output.stderr), &secrets);
        println!("W73_RAW gateway-{label}-{} code={code}", probe.name);
        println!("W73_RAW gateway-{label}-{}-stdout {stdout}", probe.name);
        println!(
            "W73_RAW gateway-{label}-{}-stderr {stderr_text}",
            probe.name
        );
        if (code == 0) != probe.ok {
            failures.push(format!("{} code={code}", probe.name));
        }
        if !probe.needle.is_empty() && !stdout.contains(probe.needle) {
            failures.push(format!("{} missing {}", probe.name, probe.needle));
        }
        if !probe.absent.is_empty()
            && (stdout.contains(probe.absent) || stderr_text.contains(probe.absent))
        {
            failures.push(format!("{} leaked {}", probe.name, probe.absent));
        }
    }
    drop(stop);
    let server = fs::read_to_string(&stderr_path).unwrap_or_default();
    println!(
        "W73_RAW gateway-{label}-server {}",
        scrub(&server, &secrets)
    );
    assert!(
        failures.is_empty(),
        "gateway {label} {}",
        failures.join("; ")
    );
}

fn issue_client_token(bits: &GatewayBits) {
    let endpoint = bits.root.join("token-endpoint.json");
    fs::write(
        &endpoint,
        format!(
            "{{\"host\":\"{}\",\"port\":{},\"database\":\"oo_w73_token\"}}\n",
            bits.host, bits.port
        ),
    )
    .expect("token endpoint");
    let ca_path = bits.root.join("ca.crt");
    let install = Command::new(&bits.issue)
        .args([
            "install",
            endpoint.to_str().expect("endpoint"),
            bits.token_admin_password.to_str().expect("password"),
            ca_path.to_str().expect("ca"),
        ])
        .output()
        .expect("issue-token install");
    assert!(
        install.status.success(),
        "issue-token install stderr={}",
        String::from_utf8_lossy(&install.stderr)
    );
    let grant = bits.root.join("grant.json");
    fs::write(
        &grant,
        r#"{"kind":"client","project_id":"shop","principal_id":"ada","role":"oo_r_clerk","policy_version":"1","knowledge_version":"1","audience_url":"https://127.0.0.1:9/","claims":[],"client_ttl_ms":600000}"#,
    )
    .expect("grant");
    let _ = fs::remove_file(&bits.token);
    let issue = Command::new(&bits.issue)
        .args([
            "issue",
            endpoint.to_str().expect("endpoint"),
            bits.token_admin_password.to_str().expect("password"),
            ca_path.to_str().expect("ca"),
            grant.to_str().expect("grant"),
            bits.token.to_str().expect("token"),
        ])
        .output()
        .expect("issue-token issue");
    let hash_line = String::from_utf8_lossy(&issue.stdout);
    println!("W73_RAW gateway-token {}", hash_line.trim());
    assert!(
        issue.status.success(),
        "issue-token issue stderr={}",
        String::from_utf8_lossy(&issue.stderr)
    );
}

fn write_gateway_dir(bits: &GatewayBits, visibility: &Value, columns: &[&str], artifact: &str) {
    let _ = fs::remove_dir_all(&bits.root);
    fs::create_dir_all(bits.root.join("packs")).expect("packs");
    fs::create_dir_all(bits.root.join("reviewed/clerk")).expect("reviewed");
    fs::create_dir_all(bits.root.join("wal")).expect("wal");
    write_mode(&bits.root.join("ca.crt"), &bits.ca);
    write_mode(
        &bits.root.join("runtime.pass"),
        bits.runtime_password.as_bytes(),
    );
    write_mode(
        &bits.root.join("data.pass"),
        bits.target_password.as_bytes(),
    );
    let endpoint = format!(
        "{{\"host\":\"{}\",\"port\":{},\"database\":\"oo_w73_token\"}}\n",
        bits.host, bits.port
    );
    let data_endpoint = format!(
        "{{\"host\":\"{}\",\"port\":{},\"database\":\"oo_w73_target\"}}\n",
        bits.host, bits.port
    );
    fs::write(bits.root.join("endpoint.json"), endpoint).expect("endpoint");
    fs::write(bits.root.join("data-endpoint.json"), data_endpoint).expect("data endpoint");
    let listed = visibility["datasets"][0]["columns"]
        .as_array()
        .expect("visibility columns");
    let names: Vec<&str> = listed
        .iter()
        .map(|item| item.as_str().expect("column"))
        .collect();
    assert_eq!(
        names, columns,
        "policy columns must equal the opened visibility"
    );
    let digest =
        crate::sha256_hex(format!("shop\npublic.customer\n{}", columns.join("\n")).as_bytes());
    let mut policy_columns = Vec::new();
    for name in columns {
        policy_columns.push(json!({
            "name": name,
            "pii": {"class": "none"},
            "semantic_type": "identifier",
            "status": "confirmed"
        }));
    }
    let policy = json!({
        "protocol": "oo.operator-preview.policy/v1",
        "project": PROJECT,
        "role": "clerk",
        "login": "oo_target_admin",
        "generation": 1,
        "config_digest": digest,
        "table": {"namespace": "public", "name": "customer", "columns": policy_columns}
    });
    fs::write(
        bits.root.join("reviewed/clerk/policy.json"),
        serde_json::to_vec(&policy).expect("policy"),
    )
    .expect("policy file");
    fs::write(
        bits.root.join("reviewed/clerk/visibility.json"),
        serde_json::to_vec(visibility).expect("visibility"),
    )
    .expect("visibility file");
    fs::write(bits.root.join("packs/clerk.json"), artifact).expect("pack");
    let config = json!({
        "listen": "127.0.0.1:0",
        "audience": "https://127.0.0.1:9/",
        "runtime_user": "oo_token_runtime",
        "password_file": "runtime.pass",
        "ca_file": "ca.crt",
        "endpoint_file": "endpoint.json",
        "pack_dir": "packs",
        "body_limit": 1048576,
        "request_timeout_ms": 15000,
        "global_concurrency": 4,
        "data_endpoint_file": "data-endpoint.json",
        "data_password_file": "data.pass",
        "data_ca_file": "ca.crt",
        "data_user": "oo_target_admin",
        "wal_dir": "wal",
        "reviewed_dir": "reviewed"
    });
    fs::write(
        bits.root.join("config.json"),
        serde_json::to_vec(&config).expect("config"),
    )
    .expect("config file");
}

async fn install_physical_adapter(env: &Env, columns: &[&str]) {
    let table = physical_ident("pt_", "w73-table");
    let id_col = physical_ident("pc_", "w73-id");
    let contact_col = physical_ident("pc_", "w73-contact");
    let digest =
        crate::sha256_hex(format!("shop\npublic.customer\n{}", columns.join("\n")).as_bytes());
    let admin = connect(
        &env.target,
        "oo_target_admin",
        &env.target_password,
        &env.ca,
        "w73-holder",
    )
    .await;
    let ddl = format!(
        "CREATE SCHEMA IF NOT EXISTS _oo; \
         CREATE TABLE IF NOT EXISTS _oo.physical_mappings ( \
           source text NOT NULL, \
           config_digest text NOT NULL, \
           run_generation bigint NOT NULL, \
           level text NOT NULL, \
           logical_namespace text NOT NULL, \
           logical_table text NOT NULL, \
           logical_column text NOT NULL DEFAULT '', \
           physical_name text NOT NULL \
         ); \
         CREATE TABLE IF NOT EXISTS _oo.\"{table}\" (\"{id_col}\" int4 NOT NULL, \"{contact_col}\" text NOT NULL); \
         REVOKE ALL ON SCHEMA _oo FROM PUBLIC; \
         REVOKE ALL ON TABLE _oo.\"{table}\" FROM PUBLIC; \
         CREATE OR REPLACE FUNCTION _oo.lookup_physical_mapping(source text, config_digest text, run_generation bigint, level_word text, logical_namespace text, logical_table text, logical_column text) \
         RETURNS text LANGUAGE sql STABLE SET search_path TO pg_catalog AS $fn$ \
           SELECT mapping.physical_name FROM _oo.physical_mappings AS mapping \
           WHERE mapping.source = lookup_physical_mapping.source \
             AND mapping.config_digest = lookup_physical_mapping.config_digest \
             AND mapping.run_generation = lookup_physical_mapping.run_generation \
             AND mapping.level = CASE lookup_physical_mapping.level_word WHEN 'namespace' THEN 'n' WHEN 'table' THEN 't' WHEN 'column' THEN 'c' ELSE NULL END \
             AND mapping.logical_namespace = lookup_physical_mapping.logical_namespace \
             AND mapping.logical_table = lookup_physical_mapping.logical_table \
             AND mapping.logical_column = lookup_physical_mapping.logical_column \
         $fn$; \
         REVOKE ALL ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) FROM PUBLIC; \
         GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO oo_target_admin; \
         GRANT USAGE ON SCHEMA _oo TO oo_r_clerk; \
         REVOKE ALL ON TABLE _oo.\"{table}\" FROM oo_r_clerk;"
    );
    admin.batch_execute(&ddl).await.expect("physical ddl");
    admin
        .batch_execute("DELETE FROM _oo.physical_mappings WHERE source = 'shop'")
        .await
        .expect("clear mappings");
    let source = "shop".to_owned();
    let generation = 1_i64;
    let empty = String::new();
    admin
        .execute(
            "INSERT INTO _oo.physical_mappings (source, config_digest, run_generation, level, logical_namespace, logical_table, logical_column, physical_name) \
             VALUES ($1, $2, $3, 't', 'public', 'customer', $4, $5)",
            &[&source as &(dyn ToSql + Sync), &digest, &generation, &empty, &table],
        )
        .await
        .expect("table mapping");
    for name in columns {
        let column = (*name).to_owned();
        let physical = match *name {
            "id" => id_col.clone(),
            "contact" => contact_col.clone(),
            _ => panic!("unexpected column {name}"),
        };
        admin
            .execute(
                "INSERT INTO _oo.physical_mappings (source, config_digest, run_generation, level, logical_namespace, logical_table, logical_column, physical_name) \
                 VALUES ($1, $2, $3, 'c', 'public', 'customer', $4, $5)",
                &[&source as &(dyn ToSql + Sync), &digest, &generation, &column, &physical],
            )
            .await
            .expect("column mapping");
    }
    let grant_cols = columns
        .iter()
        .map(|name| match *name {
            "id" => format!("\"{id_col}\""),
            "contact" => format!("\"{contact_col}\""),
            _ => panic!("unexpected column {name}"),
        })
        .collect::<Vec<_>>()
        .join(", ");
    admin
        .batch_execute(&format!(
            "GRANT SELECT ({grant_cols}) ON TABLE _oo.\"{table}\" TO oo_r_clerk"
        ))
        .await
        .expect("column grant");
    admin
        .batch_execute(&format!("TRUNCATE _oo.\"{table}\"; INSERT INTO _oo.\"{table}\" (\"{id_col}\", \"{contact_col}\") SELECT id, contact FROM public.customer"))
        .await
        .expect("copy opened row");
}

fn physical_ident(prefix: &str, label: &str) -> String {
    let digest = crate::sha256_hex(label.as_bytes());
    let name = format!("{prefix}{}", &digest[..60]);
    assert_eq!(name.len(), 63, "{name}");
    assert!(
        name.bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
        "{name}"
    );
    name
}

fn write_mode(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap_or_else(|_| panic!("write {}", path.display()));
    file.write_all(bytes).expect("write");
}

fn run_query(oo: &str, token: &Path, origin: &str, sql: &str) -> std::process::Output {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("publication-read.sh");
    Command::new("bash")
        .arg(&script)
        .env("OO_BIN", oo)
        .env("TOKEN_FILE", token)
        .env("GATEWAY_ORIGIN", origin)
        .env("SQL", sql)
        .output()
        .expect("publication read")
}

async fn wait_listen(
    path: &Path,
    child: &mut std::process::Child,
    stderr_path: &Path,
    secrets: &[String],
) -> String {
    for _ in 0..60 {
        if let Ok(text) = fs::read_to_string(path) {
            let line = text.trim();
            if !line.is_empty() {
                return line.to_owned();
            }
        }
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let stderr = fs::read_to_string(stderr_path).unwrap_or_default();
    panic!("gateway did not bind: {}", scrub(&stderr, secrets));
}

fn scrub(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_owned();
    for secret in secrets {
        if secret.len() >= 8 {
            out = out.replace(secret, "[redacted]");
        }
    }
    out
}

async fn multi_dataset_publication(env: &Env, base: &str) {
    let admin = connect(
        &env.target,
        "oo_target_admin",
        &env.target_password,
        &env.ca,
        "w73-multi-admin",
    )
    .await;
    admin.batch_execute("CREATE TABLE public.customer_extra(id int NOT NULL PRIMARY KEY,contact text NOT NULL,note text); INSERT INTO public.customer_extra VALUES(2,'grace','hidden');").await.expect("owned second table");
    let (effective, wide) =
        crate::live::shop_tables(&["contact", "id"], &["customer", "customer_extra"]);
    let (_, narrow) = crate::live::shop_tables(&["id"], &["customer", "customer_extra"]);
    // customer contact was permanently revoked by the preceding single-table case.
    // Preserve that boundary and test the new table's independent same-named column.
    let mut wide = wide;
    wide["datasets"][0]["columns"] = json!(["id"]);
    let w = stage_one(
        env,
        "cand-multi-w",
        "ver-multi-w",
        Some(base),
        &effective,
        &wide,
    )
    .await;
    let receipt = maintain_activate(&maintain(env, &w, Some(base)))
        .await
        .expect("multi activate");
    raw("multi-wide", receipt.code, &receipt.body);
    assert_eq!(receipt.code, 0, "{}", receipt.body);
    assert_eq!(receipt.body["knowledge_published"], true);
    assert_eq!(receipt.body["datasets"].as_array().unwrap().len(), 2);
    let reader = connect(
        &env.target,
        ROLE,
        &env.clerk_password,
        &env.ca,
        "w73-multi-reader",
    )
    .await;
    let rows = reader
        .query("SELECT id,contact FROM public.customer_extra", &[])
        .await
        .expect("second dataset visible");
    assert_eq!(rows[0].get::<_, i32>(0), 2);
    assert_eq!(rows[0].get::<_, String>(1), "grace");
    assert_eq!(
        query_state(&reader, "SELECT contact FROM public.customer").await,
        Err("42501".to_owned())
    );
    assert_eq!(
        query_state(&reader, "SELECT note FROM public.customer_extra").await,
        Err("42501".to_owned())
    );
    let n = stage_one(
        env,
        "cand-multi-n",
        "ver-multi-n",
        Some(&w.version),
        &effective,
        &narrow,
    )
    .await;
    let shrunk = maintain_activate(&maintain(env, &n, Some(&w.version)))
        .await
        .expect("multi narrow");
    raw("multi-narrow", shrunk.code, &shrunk.body);
    assert_eq!(shrunk.code, 0, "{}", shrunk.body);
    drop(reader);
    let reader = connect(
        &env.target,
        ROLE,
        &env.clerk_password,
        &env.ca,
        "w73-multi-narrow-reader",
    )
    .await;
    assert_eq!(
        query_state(&reader, "SELECT contact FROM public.customer_extra").await,
        Err("42501".to_owned())
    );
    let rolled = recover(&RecoverRequest {
        endpoint: &env.control,
        user: "oo_release_admin",
        password: &env.admin_password,
        ca_pem: &env.ca,
        action: "rollback",
        project_id: PROJECT,
        candidate_id: &n.id,
        expected_version: Some(&n.version),
    })
    .await
    .expect("multi rollback");
    assert_eq!(rolled.code, 0);
    let recovered = maintain_recover(&maintain(env, &n, Some(&n.version)))
        .await
        .expect("multi recover");
    raw("multi-recover", recovered.code, &recovered.body);
    assert_eq!(recovered.body["knowledge_published"], false);
    assert_eq!(recovered.body["certificate_class"], "not_issued");
    assert_eq!(recovered.code, 0, "{}", recovered.body);
    let reader = connect(
        &env.target,
        ROLE,
        &env.clerk_password,
        &env.ca,
        "w73-multi-recovered",
    )
    .await;
    assert_eq!(
        query_state(&reader, "SELECT contact FROM public.customer_extra").await,
        Err("42501".to_owned())
    );
    assert!(
        query_state(&reader, "SELECT id FROM public.customer_extra")
            .await
            .is_ok()
    );
    native_mapped_fixture(env, &w.version, &effective, &wide, &narrow).await;
    println!(
        "W73_RAW multi-dataset typed_rows=1 hidden_note_denied=true shrink_contact_denied=true rollback_contact_denied=true"
    );
}

async fn native_mapped_fixture(
    env: &Env,
    base: &str,
    effective: &Value,
    wide: &Value,
    narrow: &Value,
) {
    // Synthetic target mapping fixture only: never a native source admission/lease proof.
    let admin = connect(
        &env.target,
        "oo_target_admin",
        &env.target_password,
        &env.ca,
        "w73-native-map-fixture-admin",
    )
    .await;
    let namespace = physical_ident("pn_", "w73-native-namespace");
    let first_table = physical_ident("pt_", "w73-table");
    admin.batch_execute(&format!("CREATE SCHEMA \"{namespace}\"; ALTER TABLE _oo.\"{first_table}\" SET SCHEMA \"{namespace}\";")).await.expect("actual native namespace");
    for (source, digest, generation) in [("shop", crate::sha256_hex(b"shop\npublic.customer\nid"), 1_i64), ("shop-extra", "ab".repeat(32), 2_i64)] {
        admin.execute("INSERT INTO _oo.physical_mappings(source,config_digest,run_generation,level,logical_namespace,logical_table,logical_column,physical_name) VALUES($1,$2,$3,'n','public','','',$4)", &[&source as &(dyn ToSql+Sync), &digest, &generation, &namespace]).await.expect("namespace mapping");
    }
    let table = physical_ident("pt_", "w73-native-second-table");
    let id = physical_ident("pc_", "w73-native-second-id");
    let contact = physical_ident("pc_", "w73-native-second-contact");
    admin.batch_execute(&format!("CREATE TABLE \"{namespace}\".\"{table}\"(\"{id}\" int4 NOT NULL,\"{contact}\" text NOT NULL);INSERT INTO \"{namespace}\".\"{table}\" VALUES(2,'grace');")).await.expect("synthetic shadow");
    let digest = crate::sha256_hex(b"shop\npublic.customer\nid");
    let extra_digest = "ab".repeat(32);
    for (level, column, physical) in [
        ("t", "", table.as_str()),
        ("c", "id", id.as_str()),
        ("c", "contact", contact.as_str()),
    ] {
        admin.execute("INSERT INTO _oo.physical_mappings(source,config_digest,run_generation,level,logical_namespace,logical_table,logical_column,physical_name) VALUES('shop-extra',$1,2,$2,'public','customer_extra',$3,$4)",&[&extra_digest as &(dyn ToSql+Sync),&level,&column,&physical]).await.expect("synthetic mapping");
    }
    let context = json!({"datasets":[{"dataset":"public.customer","source":"shop","config_digest":digest,"generation":1},{"dataset":"public.customer_extra","source":"shop-extra","config_digest":extra_digest,"generation":2}]});
    let w = stage_one(
        env,
        "cand-native-map-w",
        "ver-native-map-w",
        Some(base),
        effective,
        wide,
    )
    .await;
    let wrong = json!({"source":"shop","config_digest":digest,"generation":99});
    let mut request = maintain(env, &w, Some(base));
    request.native_mapping = Some(&wrong);
    assert!(
        maintain_activate(&request).await.is_err(),
        "nonexistent generation must not create grants"
    );
    assert_eq!(pointer_version(env).await.as_deref(), Some(base));
    request.native_mapping = Some(&context);
    let opened = maintain_activate(&request)
        .await
        .expect("native map activate");
    raw("native-mapping-wide", opened.code, &opened.body);
    assert_eq!(opened.code, 0, "{}", opened.body);
    assert_eq!(opened.body["knowledge_published"], true);
    request.native_mapping = Some(&wrong);
    let refused = maintain_activate(&request)
        .await
        .expect("changed native binding rejected");
    assert_eq!(refused.code, 2);
    assert_eq!(refused.body["reason"], "target_binding_changed");
    assert_eq!(refused.body["fence_touched"], false);
    let reader = connect(
        &env.target,
        ROLE,
        &env.clerk_password,
        &env.ca,
        "w73-native-reader",
    )
    .await;
    let rows = reader
        .query(
            &format!("SELECT \"{id}\",\"{contact}\" FROM \"{namespace}\".\"{table}\""),
            &[],
        )
        .await
        .expect("actual mapped read");
    assert_eq!(rows[0].get::<_, i32>(0), 2);
    assert_eq!(rows[0].get::<_, String>(1), "grace");
    drop(reader);
    let n = stage_one(
        env,
        "cand-native-map-n",
        "ver-native-map-n",
        Some(&w.version),
        effective,
        narrow,
    )
    .await;
    let mut request = maintain(env, &n, Some(&w.version));
    request.native_mapping = Some(&context);
    let shrunk = maintain_activate(&request).await.expect("native shrink");
    raw("native-mapping-narrow", shrunk.code, &shrunk.body);
    assert_eq!(shrunk.code, 0, "{}", shrunk.body);
    let reader = connect(
        &env.target,
        ROLE,
        &env.clerk_password,
        &env.ca,
        "w73-native-narrow-reader",
    )
    .await;
    assert_eq!(
        query_state(
            &reader,
            &format!("SELECT \"{contact}\" FROM \"{namespace}\".\"{table}\"")
        )
        .await,
        Err("42501".to_owned())
    );
    drop(reader);
    let rolled = recover(&RecoverRequest {
        endpoint: &env.control,
        user: "oo_release_admin",
        password: &env.admin_password,
        ca_pem: &env.ca,
        action: "rollback",
        project_id: PROJECT,
        candidate_id: &n.id,
        expected_version: Some(&n.version),
    })
    .await
    .expect("native rollback");
    assert_eq!(rolled.code, 0);
    request.native_mapping = None; // persisted candidate/pointer mappings are authoritative
    let recovered = maintain_recover(&request).await.expect("native recover");
    raw("native-mapping-recover", recovered.code, &recovered.body);
    assert_eq!(recovered.code, 0, "{}", recovered.body);
    assert_eq!(recovered.body["knowledge_published"], false);
    let reader = connect(
        &env.target,
        ROLE,
        &env.clerk_password,
        &env.ca,
        "w73-native-recovered-reader",
    )
    .await;
    assert_eq!(
        query_state(
            &reader,
            &format!("SELECT \"{contact}\" FROM \"{namespace}\".\"{table}\"")
        )
        .await,
        Err("42501".to_owned())
    );
    assert!(
        query_state(&reader, &format!("SELECT \"{id}\" FROM \"{namespace}\".\"{table}\""))
            .await
            .is_ok()
    );
    println!(
        "W73_RAW native-mapping-fixture typed_row=[2,grace] wrong_generation_refused=true shrink_denied=true rollback_revocation_preserved=true native_source_accepted=false"
    );
}
