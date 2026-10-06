//! Operator provision of mapping read for one explicit login.
//!
//! Offline cases refuse a bad document before a connection. The policy objects
//! are the reviewed-policy DTO. The live case runs when `OO_A249_LIVE=1`.
//! Public `init-pg` and `import-pg` install the target. `install_column_acl_set`
//! installs two roles. The login cannot load the catalog until this command.
//! Passwords stay in files.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use oo_gateway::policy::catalog::{load_policy_catalog, CatalogError, CatalogLimits, UnknownRows};
use oo_gateway::policy::{
    compile_column_acl, connect_require_tls, install_column_acl_set, rustls_connector, ClassStatus,
    ColumnAclPolicy, ColumnSpec, InstallEffect, LogicalTable, PiiClass, RowFilter, TlsEndpoint,
};
use oo_gateway::query_executor::{
    AuditAdmission, Cell, CompletionRecord, CompletionStatus, DedicatedSession, ExecError,
    SelectRequest, SelectStream,
};
use oo_gateway::{PolicyIdentity, TlsConnection};
use serde_json::{json, Value};
use tokio_postgres::Client;

const PROTOCOL: &str = "oo.gateway.provision-mapping-reader-cli/v1";
const HOST: &str = "oo-a249-pg";
const SOURCE_DB: &str = "oo_a249_src";
const TARGET_DB: &str = "oo_a249_dst";
const ADMIN: &str = "oo_fixture_admin";
const READER: &str = "oo_a249_reader";
const WRITER: &str = "oo_a249_writer";
const RUNTIME: &str = "oo_a249_runtime";
const LOGIN: &str = "oo_a249_login";
const EXTRA: &str = "oo_a249_extra";
const MISSING_LOGIN: &str = "oo_a249_missing";
const SECRET_DIR: &str = "/secrets";
const LIVE_DIR: &str = "/evidence/live";
const SOURCE_ID: &str = "shop";
const GENERATION: i64 = 7;
const STALE_GENERATION: i64 = 3;
const OWNER: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a249";
const SHARD: &str = "shard_a249";

const PRODUCER_POLICY: &str = r#"{"project":"shop","config_digest":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","generation":4,"role":"clerk","login":"clerk_login","table":{"namespace":"public","name":"customer","columns":[{"name":"id","status":"confirmed","semantic_type":"identifier","pii":{"class":"none"}},{"name":"email","status":"confirmed","semantic_type":"email","pii":{"class":"declared","explicit_allow":true}}]},"row_filter":null}"#;

#[test]
fn usage_exits_2() {
    let output = run_cli_blocking(b"{}", &["secret-arg-token"]);
    assert_line(&output, 2, "provision-mapping-reader: usage");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("secret-arg-token"));
}

#[test]
fn malformed_json_is_scan() {
    let output = run_cli_blocking(b"{", &[]);
    assert_line(&output, 4, "provision-mapping-reader: scan");
}

#[test]
fn overlong_stdin_is_budget() {
    let bytes = vec![b'a'; 1_048_577];
    let output = run_cli_blocking(&bytes, &[]);
    assert_line(&output, 3, "provision-mapping-reader: budget");
}

#[test]
fn wrong_protocol_is_request() {
    let mut body = base_doc();
    body["protocol"] = json!("oo.gateway.reviewed-policy-cli/v1");
    body["policies"] = json!([sample_policy(Value::Null)]);
    let output = run_cli_blocking(&serde_json::to_vec(&body).expect("json"), &[]);
    assert_line(&output, 5, "provision-mapping-reader: request");
}

#[test]
fn unknown_field_is_request() {
    let mut body = base_doc();
    body["policies"] = json!([sample_policy(Value::Null)]);
    body["audit_file"] = json!("/tmp/audit.jsonl");
    let output = run_cli_blocking(&serde_json::to_vec(&body).expect("json"), &[]);
    assert_line(&output, 5, "provision-mapping-reader: request");
}

#[test]
fn row_filter_is_policy_filter() {
    let mut body = base_doc();
    body["policies"] = json!([sample_policy(json!({
        "kind": "claims",
        "column": "id",
        "claim": "party"
    }))]);
    let output = run_cli_blocking(&serde_json::to_vec(&body).expect("json"), &[]);
    assert_line(&output, 7, "provision-mapping-reader: policy filter");
}

#[test]
fn empty_set_is_policy_empty() {
    let output = run_cli_blocking(&serde_json::to_vec(&base_doc()).expect("json"), &[]);
    assert_line(&output, 7, "provision-mapping-reader: policy empty");
}

#[test]
fn missing_password_file_is_request() {
    let mut body = base_doc();
    body["policies"] = json!([sample_policy(Value::Null)]);
    body["password_file"] = json!("/no/such/provision-mapping-reader-password");
    let output = run_cli_blocking(&serde_json::to_vec(&body).expect("json"), &[]);
    assert_line(&output, 5, "provision-mapping-reader: request");
}

#[test]
fn password_inside_document_is_request() {
    let dir = TempDir::new("password-echo");
    let password = dir.path.join("password");
    fs::write(&password, "shop\n").expect("password");
    let ca = dir.path.join("ca.pem");
    fs::write(&ca, b"not-a-key").expect("ca");
    let mut body = base_doc();
    body["policies"] = json!([sample_policy(Value::Null)]);
    body["password_file"] = json!(password.display().to_string());
    body["ca_file"] = json!(ca.display().to_string());
    let output = run_cli_blocking(&serde_json::to_vec(&body).expect("json"), &[]);
    assert_line(&output, 5, "provision-mapping-reader: request");
}

#[test]
fn private_key_ca_is_tls() {
    let dir = TempDir::new("private-key");
    let password = dir.path.join("password");
    fs::write(&password, "Z9\n").expect("password");
    let ca = dir.path.join("ca.pem");
    fs::write(&ca, b"-----BEGIN PRIVATE KEY-----\n").expect("ca");
    let mut body = base_doc();
    body["policies"] = json!([sample_policy(Value::Null)]);
    body["password_file"] = json!(password.display().to_string());
    body["ca_file"] = json!(ca.display().to_string());
    let text = serde_json::to_vec(&body).expect("json");
    assert!(!text.windows(2).any(|window| window == b"Z9"));
    let output = run_cli_blocking(&text, &[]);
    assert_line(&output, 6, "provision-mapping-reader: tls");
}

#[test]
fn symlink_password_is_request() {
    let dir = TempDir::new("symlink");
    let real = dir.path.join("real");
    fs::write(&real, "Z9\n").expect("real");
    let link = dir.path.join("link");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    let mut body = base_doc();
    body["policies"] = json!([sample_policy(Value::Null)]);
    body["password_file"] = json!(link.display().to_string());
    let output = run_cli_blocking(&serde_json::to_vec(&body).expect("json"), &[]);
    assert_line(&output, 5, "provision-mapping-reader: request");
}

#[test]
fn login_mismatch_is_policy_binding() {
    let mut body = base_doc();
    body["login"] = json!("other_login");
    body["policies"] = json!([serde_json::from_str::<Value>(PRODUCER_POLICY).expect("producer")]);
    body["password_file"] = json!("/no/such/provision-mapping-reader-password");
    let output = run_cli_blocking(&serde_json::to_vec(&body).expect("json"), &[]);
    assert_line(&output, 7, "provision-mapping-reader: policy binding");
}

#[test]
fn producer_policy_object_reaches_tls() {
    let dir = TempDir::new("producer");
    let password = dir.path.join("password");
    fs::write(&password, "Z9\n").expect("password");
    let ca = dir.path.join("ca.pem");
    fs::write(&ca, b"-----BEGIN PRIVATE KEY-----\n").expect("ca");
    let text = format!(
        r#"{{"protocol":"{PROTOCOL}","policies":[{PRODUCER_POLICY}],"login":"clerk_login","endpoint":{{"host":"localhost","port":1,"database":"db","user":"admin_user"}},"password_file":"{}","ca_file":"{}"}}"#,
        password.display(),
        ca.display()
    );
    assert!(text.contains(PRODUCER_POLICY));
    assert!(!text.contains("Z9"));
    let output = run_cli_blocking(text.as_bytes(), &[]);
    assert_line(&output, 6, "provision-mapping-reader: tls");
}

#[tokio::test]
async fn private_tls_two_roles_mapping_then_catalog_query() {
    if env::var("OO_A249_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    let init_bin = env::var("OO_A249_INIT_PG").expect("OO_A249_INIT_PG");
    let pipeline_bin = env::var("OO_A249_PIPELINE").expect("OO_A249_PIPELINE");
    let admin_pass = read_secret("admin.pass");
    let reader_pass = read_secret("reader.pass");
    let writer_pass = read_secret("writer.pass");
    let runtime_pass = read_secret("runtime.pass");
    let login_pass = read_secret("login.pass");
    let extra_pass = read_secret("extra.pass");
    let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca.crt");
    let mut oracle = Oracle::new(&[
        &admin_pass,
        &reader_pass,
        &writer_pass,
        &runtime_pass,
        &login_pass,
        &extra_pass,
    ]);
    fs::create_dir_all(format!("{LIVE_DIR}/steps")).expect("live steps");

    let admin = connect_db("postgres", ADMIN, &admin_pass, &ca).await;
    reset_cluster(&admin).await;
    create_login(&admin, READER, &reader_pass, "INHERIT").await;
    create_login(&admin, WRITER, &writer_pass, "INHERIT").await;
    create_login(&admin, RUNTIME, &runtime_pass, "INHERIT").await;
    create_login(&admin, LOGIN, &login_pass, "NOINHERIT").await;
    create_database(&admin, SOURCE_DB).await;
    create_database(&admin, TARGET_DB).await;
    exec(
        &admin,
        &format!("GRANT CONNECT ON DATABASE {SOURCE_DB} TO {READER}, {WRITER}"),
    )
    .await;
    exec(
        &admin,
        &format!("GRANT CONNECT ON DATABASE {TARGET_DB} TO {RUNTIME}, {LOGIN}"),
    )
    .await;
    let server = server_facts(&admin).await;
    assert!(server.starts_with("ssl=true\n") || server.starts_with("ssl=t\n"), "{server}");
    fs::write(format!("{LIVE_DIR}/server.txt"), &server).expect("server");
    oracle.fact("server", server.trim());

    let source = connect_db(SOURCE_DB, ADMIN, &admin_pass, &ca).await;
    exec(&source, SOURCE_SQL).await;
    drop(source);

    let init = run_bin(Path::new(&init_bin), &[], &init_request());
    save_step("init-pg", &init, oracle.bans());
    assert_eq!(init.status.code(), Some(0), "init-pg");
    let init_doc: Value = serde_json::from_slice(&init.stdout).expect("init json");
    assert_eq!(init_doc["protocol"], "oo.pipeline.init-pg-result/v1");
    assert_eq!(init_doc["install"], "bootstrapped");
    assert_eq!(init_doc["full_import"], "installed");
    assert_eq!(init_doc["historical_v2"], false);
    oracle.fact("init", "bootstrapped");

    exec(
        &admin,
        &format!("GRANT CONNECT ON DATABASE {TARGET_DB} TO {RUNTIME}, {LOGIN}"),
    )
    .await;
    exec(&admin, &format!("GRANT oo_pipeline_runtime TO {RUNTIME}")).await;
    let imported = run_bin(
        Path::new(&pipeline_bin),
        &["import-pg"],
        &import_request(),
    );
    save_step("import-pg", &imported, oracle.bans());
    assert_eq!(imported.status.code(), Some(0), "import-pg");
    let import_doc: Value = serde_json::from_slice(&imported.stdout).expect("import json");
    assert_eq!(import_doc["protocol"], "oo.pipeline.import-pg-result/v1");
    assert_eq!(import_doc["state"], "awaiting_publish");
    assert_eq!(import_doc["table_count"], 2);
    assert_eq!(import_doc["source_generation"], GENERATION);
    oracle.fact("import_state", "awaiting_publish");
    oracle.fact("import_tables", "2");

    let mut target = connect_db(TARGET_DB, ADMIN, &admin_pass, &ca).await;
    let (digest, generation) = mapping_identity(&target).await;
    assert_eq!(generation, GENERATION);
    assert_eq!(digest.len(), 64);
    oracle.fact("mapping_generation", &generation.to_string());

    let clerk = role_set("clerk", LOGIN, &digest, &["customer_id", "display_name", "active"], &[
        "order_id",
        "customer_id",
        "memo",
        "posted",
        "amount",
        "clerk_note",
    ]);
    let peek = role_set("peek", LOGIN, &digest, &["customer_id", "display_name"], &[
        "order_id",
        "customer_id",
        "amount",
    ]);
    let clerk_installed = install_column_acl_set(&mut target, &clerk).await.expect("clerk install");
    assert_eq!(clerk_installed.effect, InstallEffect::Installed);
    assert_eq!(clerk_installed.role, "oo_r_clerk");
    let peek_installed = install_column_acl_set(&mut target, &peek).await.expect("peek install");
    assert_eq!(peek_installed.effect, InstallEffect::Installed);
    assert_eq!(peek_installed.role, "oo_r_peek");
    oracle.fact("column_acl", "installed");
    assert!(mapping_absent(&target, LOGIN).await);
    oracle.fact("mapping_after_column_acl", "absent");

    let mut login_client = connect_db(TARGET_DB, LOGIN, &login_pass, &ca).await;
    let catalog_before = load_policy_catalog(
        &mut login_client,
        &identity("oo_r_clerk", &digest),
        &compiled(&clerk),
        &limits(),
    )
    .await;
    assert_eq!(
        catalog_before,
        Err(CatalogError::Database {
            sqlstate: "42501".to_string(),
        })
    );
    drop(login_client);
    oracle.fact("catalog_before", "42501");

    let bans = oracle.bans().to_vec();
    let mut stale = clerk.clone();
    for policy in &mut stale {
        policy.generation = STALE_GENERATION;
    }
    let stale_out = run_cli(request_bytes(LOGIN, &stale)).await;
    save_step("cli-stale", &stale_out, &bans);
    assert_line(&stale_out, 7, "provision-mapping-reader: policy stale");
    assert!(mapping_absent(&target, LOGIN).await);
    oracle.fact("stale", "policy stale");

    let mut unknown = clerk.clone();
    for policy in &mut unknown {
        policy.login = MISSING_LOGIN.to_string();
    }
    let unknown_out = run_cli(request_bytes(MISSING_LOGIN, &unknown)).await;
    save_step("cli-unknown", &unknown_out, &bans);
    assert_line(&unknown_out, 7, "provision-mapping-reader: policy login");
    assert!(mapping_absent(&target, LOGIN).await);
    oracle.fact("unknown_login", "policy login");

    create_login(&admin, EXTRA, &extra_pass, "NOINHERIT").await;
    exec(
        &admin,
        &format!("GRANT CONNECT ON DATABASE {TARGET_DB} TO {EXTRA}"),
    )
    .await;
    let extra_set = role_set(
        "extra",
        EXTRA,
        &digest,
        &["customer_id", "display_name", "active"],
        &["order_id", "customer_id", "amount"],
    );
    let extra_installed = install_column_acl_set(&mut target, &extra_set)
        .await
        .expect("extra install");
    assert_eq!(extra_installed.effect, InstallEffect::Installed);
    exec(
        &target,
        &format!("GRANT USAGE ON SCHEMA \"_oo\" TO \"{EXTRA}\""),
    )
    .await;
    let partial = run_cli(request_bytes(EXTRA, &extra_set)).await;
    save_step("cli-partial", &partial, &bans);
    assert_line(&partial, 7, "provision-mapping-reader: policy privilege");
    let extra_flags = mapping_flags(&target, EXTRA).await;
    assert!(extra_flags.usage);
    assert!(!extra_flags.select);
    assert!(!extra_flags.lookup);
    assert!(mapping_absent(&target, LOGIN).await);
    oracle.fact("partial_privilege", "usage_only_refused");

    let clerk_grants_before = grant_text(&target, "oo_r_clerk").await;
    let peek_grants_before = grant_text(&target, "oo_r_peek").await;
    let clerk_bytes = request_bytes(LOGIN, &clerk);
    let clerk_text = String::from_utf8(clerk_bytes.clone()).expect("utf8");
    assert!(clerk_text.contains("\"status\":\"confirmed\""));
    assert!(clerk_text.contains("\"pii\":{\"class\":\"none\"}"));
    assert!(clerk_text.contains("\"row_filter\":null"));
    assert!(!clerk_text.contains("explicit_allow"));
    let installed = run_cli(clerk_bytes).await;
    save_step("cli-installed", &installed, &bans);
    assert_receipt(&installed, "installed", LOGIN, "oo_r_clerk");
    assert_quiet(&installed.stdout, &bans, "installed stdout");
    assert!(mapping_exact(&target, LOGIN).await);
    assert_eq!(grant_text(&target, "oo_r_clerk").await, clerk_grants_before);
    assert_eq!(grant_text(&target, "oo_r_peek").await, peek_grants_before);
    assert_eq!(login_shadow_column_grants(&target, LOGIN).await, 0);
    assert_eq!(
        login_mapping_column_grants(&target, LOGIN).await,
        mapping_column_count(&target).await
    );
    assert!(!public_mapping(&target).await);
    oracle.fact("provision", "installed");
    oracle.fact("mapping_exposure", "database_wide_catalog_listing");

    let replay = run_cli(request_bytes(LOGIN, &clerk)).await;
    save_step("cli-replay", &replay, &bans);
    assert_receipt(&replay, "replay", LOGIN, "oo_r_clerk");
    let peek_replay = run_cli(request_bytes(LOGIN, &peek)).await;
    save_step("cli-peek-replay", &peek_replay, &bans);
    assert_receipt(&peek_replay, "replay", LOGIN, "oo_r_peek");
    assert!(mapping_exact(&target, LOGIN).await);
    oracle.fact("replay", "replay");
    oracle.fact("peek_replay", "replay");

    let clerk_catalog = load_one(&login_pass, &ca, "oo_r_clerk", &digest, &clerk).await;
    let peek_catalog = load_one(&login_pass, &ca, "oo_r_peek", &digest, &peek).await;
    assert_eq!(
        relation_names(&clerk_catalog),
        vec!["customer".to_string(), "order_doc".to_string()]
    );
    assert_eq!(
        relation_names(&peek_catalog),
        vec!["customer".to_string(), "order_doc".to_string()]
    );
    assert_eq!(
        column_names(&clerk_catalog, "customer"),
        vec!["active".to_string(), "customer_id".to_string(), "display_name".to_string()]
    );
    oracle.fact("catalog_tables", "2");
    oracle.fact("catalog_roles", "2");

    let clerk_rows = compiled_query(
        &login_pass,
        &ca,
        "oo_r_clerk",
        &digest,
        &clerk_catalog,
        "SELECT customer_id, display_name, active FROM shop.customer ORDER BY customer_id",
    )
    .await;
    assert_eq!(
        clerk_rows,
        vec![
            vec![Cell::Int("41".to_string()), Cell::Text("丙号".to_string()), Cell::Bool(true)],
            vec![Cell::Int("42".to_string()), Cell::Null, Cell::Bool(false)],
            vec![Cell::Int("43".to_string()), Cell::Text("丁铺".to_string()), Cell::Bool(true)],
        ]
    );
    let peek_rows = compiled_query(
        &login_pass,
        &ca,
        "oo_r_peek",
        &digest,
        &peek_catalog,
        "SELECT order_id, amount FROM shop.order_doc ORDER BY order_id",
    )
    .await;
    assert_eq!(
        peek_rows,
        vec![
            vec![Cell::Int("701".to_string()), Cell::Int("-9223372036854775808".to_string())],
            vec![Cell::Int("702".to_string()), Cell::Int("9223372036854775807".to_string())],
            vec![Cell::Int("703".to_string()), Cell::Int("0".to_string())],
        ]
    );
    oracle.fact("compiled_query", "clerk_customer_peek_order");

    assert!(!bool_sql(
        &target,
        &format!("SELECT pg_catalog.pg_has_role('{LOGIN}', 'oo_target_owner', 'member')")
    )
    .await);
    assert!(!bool_sql(
        &target,
        "SELECT pg_catalog.pg_has_role('oo_pipeline_runtime', 'oo_target_owner', 'member')"
    )
    .await);
    assert!(!public_mapping(&target).await);
    assert_eq!(source_customer_count(&admin_pass, &ca).await, 3);
    let reader = connect_db(SOURCE_DB, READER, &reader_pass, &ca).await;
    assert_eq!(
        sqlstate_of(
            &reader,
            "INSERT INTO shop.customer (customer_id, display_name, active) VALUES (9, 'no', true)"
        )
        .await,
        "42501"
    );
    oracle.fact("public", "false");
    oracle.fact("owner_membership", "false");
    oracle.fact("source_reader_insert", "42501");

    let outcome = json!({
        "protocol": "oo.gateway.provision-mapping-reader-cli-result/v1",
        "parent_acceptance": false,
        "release": false,
        "effect": "installed",
        "replay": "replay",
        "catalog_before": "42501",
        "mapping_exposure": "database_wide",
        "public": false,
        "owner_membership": false,
        "source_grant": false
    });
    let outcome_text = serde_json::to_string_pretty(&outcome).expect("outcome");
    for ban in &bans {
        assert!(!outcome_text.contains(ban), "outcome contains a secret");
    }
    fs::write(format!("{LIVE_DIR}/outcome.json"), outcome_text).expect("outcome");
    fs::write(format!("{LIVE_DIR}/outcome.txt"), "provision_mapping_reader_cli_recorded\n")
        .expect("outcome text");
    oracle.fact("outcome", "provision_mapping_reader_cli_recorded");
}

fn role_set(
    role: &str,
    login: &str,
    digest: &str,
    customer: &[&str],
    order: &[&str],
) -> Vec<ColumnAclPolicy> {
    vec![
        table_policy(role, login, digest, "customer", customer),
        table_policy(role, login, digest, "order_doc", order),
    ]
}

fn table_policy(role: &str, login: &str, digest: &str, table: &str, columns: &[&str]) -> ColumnAclPolicy {
    ColumnAclPolicy {
        project: SOURCE_ID.to_string(),
        config_digest: digest.to_string(),
        generation: GENERATION,
        role: role.to_string(),
        login: login.to_string(),
        table: LogicalTable {
            namespace: "shop".to_string(),
            name: table.to_string(),
            columns: columns.iter().copied().map(column_spec).collect(),
        },
        row_filter: None,
    }
}

fn column_spec(name: &str) -> ColumnSpec {
    ColumnSpec {
        name: name.to_string(),
        status: ClassStatus::Confirmed,
        semantic_type: "label".to_string(),
        pii: PiiClass::None,
    }
}

fn request_bytes(login: &str, policies: &[ColumnAclPolicy]) -> Vec<u8> {
    let body = json!({
        "protocol": PROTOCOL,
        "policies": policies.iter().map(policy_value).collect::<Vec<_>>(),
        "login": login,
        "endpoint": {
            "host": HOST,
            "port": 5432,
            "database": TARGET_DB,
            "user": ADMIN
        },
        "password_file": format!("{SECRET_DIR}/admin.pass"),
        "ca_file": format!("{SECRET_DIR}/ca.crt")
    });
    serde_json::to_vec(&body).expect("request")
}

fn policy_value(policy: &ColumnAclPolicy) -> Value {
    let columns: Vec<Value> = policy
        .table
        .columns
        .iter()
        .map(|column| {
            let status = match column.status {
                ClassStatus::Confirmed => "confirmed",
                ClassStatus::AutoAccepted => "auto_accepted",
                ClassStatus::Unclassified => "unclassified",
            };
            let pii = match column.pii {
                PiiClass::None => json!({"class": "none"}),
                PiiClass::Unknown => json!({"class": "unknown"}),
                PiiClass::Declared { explicit_allow } => {
                    json!({"class": "declared", "explicit_allow": explicit_allow})
                }
            };
            json!({
                "name": column.name,
                "status": status,
                "semantic_type": column.semantic_type,
                "pii": pii
            })
        })
        .collect();
    let row_filter = match &policy.row_filter {
        None => Value::Null,
        Some(RowFilter::Claims { column, claim }) => {
            json!({"kind": "claims", "column": column, "claim": claim})
        }
    };
    json!({
        "project": policy.project,
        "config_digest": policy.config_digest,
        "generation": policy.generation,
        "role": policy.role,
        "login": policy.login,
        "table": {
            "namespace": policy.table.namespace,
            "name": policy.table.name,
            "columns": columns
        },
        "row_filter": row_filter
    })
}

fn compiled(policies: &[ColumnAclPolicy]) -> Vec<oo_gateway::policy::CompiledPolicy> {
    policies
        .iter()
        .map(|policy| compile_column_acl(policy).expect("compile"))
        .collect()
}

fn identity(role: &str, digest: &str) -> PolicyIdentity {
    PolicyIdentity {
        project: SOURCE_ID.to_string(),
        role: role.to_string(),
        policy_id: digest.to_string(),
    }
}

fn limits() -> CatalogLimits {
    CatalogLimits {
        max_tables: 4,
        max_columns: 64,
        max_functions: 1,
        max_operators: 64,
        large_table_row_threshold: 1_000_000,
        unknown_rows: UnknownRows::Zero,
    }
}

fn relation_names(catalog: &oo_gateway::PolicyCatalog) -> Vec<String> {
    let mut names: Vec<String> = catalog.relations.iter().map(|relation| relation.name.clone()).collect();
    names.sort();
    names
}

fn column_names(catalog: &oo_gateway::PolicyCatalog, table: &str) -> Vec<String> {
    let mut names: Vec<String> = catalog
        .relations
        .iter()
        .find(|relation| relation.name == table)
        .expect("relation")
        .columns
        .iter()
        .map(|column| column.name.clone())
        .collect();
    names.sort();
    names
}

async fn load_one(
    login_pass: &str,
    ca: &[u8],
    role: &str,
    digest: &str,
    policies: &[ColumnAclPolicy],
) -> oo_gateway::PolicyCatalog {
    let mut client = connect_db(TARGET_DB, LOGIN, login_pass, ca).await;
    load_policy_catalog(&mut client, &identity(role, digest), &compiled(policies), &limits())
        .await
        .expect("catalog")
}

async fn compiled_query(
    login_pass: &str,
    ca: &[u8],
    role: &str,
    digest: &str,
    catalog: &oo_gateway::PolicyCatalog,
    sql: &str,
) -> Vec<Vec<Cell>> {
    let (client, connection) = connect_pair(TARGET_DB, LOGIN, login_pass, ca).await;
    let tls = rustls_connector(ca).expect("connector");
    let mut session = DedicatedSession::bind(client, connection, tls)
        .await
        .expect("bind");
    let ident = identity(role, digest);
    let stream = session
        .execute_trusted_select(
            SelectRequest {
                identity: &ident,
                catalog,
                original_sql: sql,
                role,
                max_rows: 10,
                max_result_bytes: 65_536,
                wall_clock: Duration::from_millis(15_000),
            },
            QuietAudit,
        )
        .await
        .expect("query");
    finish_ok(&mut session, stream).await
}

async fn finish_ok(session: &mut DedicatedSession, mut stream: SelectStream) -> Vec<Vec<Cell>> {
    let mut rows = Vec::new();
    loop {
        match stream.next_row().await {
            Ok(Some(row)) => rows.push(row),
            Ok(None) => break,
            Err(err) => panic!("row error: {err}"),
        }
    }
    drop(stream);
    session.wait_idle().await.expect("idle");
    let summary = session.summary().expect("summary");
    assert_eq!(summary.status(), CompletionStatus::Succeeded);
    assert!(!summary.truncated());
    assert!(summary.rollback_confirmed());
    assert!(!summary.socket_dropped());
    assert!(summary.end_audit_ok());
    rows
}

struct QuietAudit;

impl AuditAdmission for QuietAudit {
    async fn admit(&mut self, _admission: &oo_gateway::query_executor::Admission<'_>) -> Result<(), ExecError> {
        Ok(())
    }

    async fn complete(&mut self, _completion: &CompletionRecord) -> Result<(), ExecError> {
        Ok(())
    }
}

struct MappingFlags {
    usage: bool,
    select: bool,
    lookup: bool,
}

async fn mapping_flags(client: &Client, login: &str) -> MappingFlags {
    let row = client
        .query_one(
            "SELECT \
                pg_catalog.has_schema_privilege(($1::text)::name, '_oo'::name, 'USAGE'), \
                pg_catalog.has_table_privilege(($1::text)::name, '_oo.physical_mappings'::text, 'SELECT'), \
                pg_catalog.has_function_privilege(($1::text)::name, \
                    '_oo.lookup_physical_mapping(text, text, bigint, text, text, text, text)'::text, 'EXECUTE')",
            &[&login],
        )
        .await
        .expect("flags");
    MappingFlags {
        usage: row.get(0),
        select: row.get(1),
        lookup: row.get(2),
    }
}

async fn mapping_absent(client: &Client, login: &str) -> bool {
    let flags = mapping_flags(client, login).await;
    !flags.usage && !flags.select && !flags.lookup
}

async fn mapping_exact(client: &Client, login: &str) -> bool {
    let row = client
        .query_one(
            "SELECT \
                pg_catalog.has_schema_privilege(($1::text)::name, '_oo'::name, 'USAGE'), \
                pg_catalog.has_schema_privilege(($1::text)::name, '_oo'::name, 'CREATE'), \
                pg_catalog.has_table_privilege(($1::text)::name, '_oo.physical_mappings'::text, 'SELECT'), \
                pg_catalog.has_table_privilege(($1::text)::name, '_oo.physical_mappings'::text, 'INSERT'), \
                pg_catalog.has_function_privilege(($1::text)::name, \
                    '_oo.lookup_physical_mapping(text, text, bigint, text, text, text, text)'::text, 'EXECUTE'), \
                pg_catalog.has_function_privilege(($1::text)::name, \
                    '_oo.register_physical_mapping(text, text, bigint, text, text, text, text, bytea)'::text, 'EXECUTE')",
            &[&login],
        )
        .await
        .expect("exact");
    let usage: bool = row.get(0);
    let create: bool = row.get(1);
    let select: bool = row.get(2);
    let insert: bool = row.get(3);
    let lookup: bool = row.get(4);
    let register: bool = row.get(5);
    usage && !create && select && !insert && lookup && !register
}

async fn public_mapping(client: &Client) -> bool {
    let row = client
        .query_one(
            "SELECT \
                pg_catalog.has_schema_privilege('public'::name, '_oo'::name, 'USAGE') \
                OR pg_catalog.has_table_privilege('public'::name, '_oo.physical_mappings'::text, 'SELECT') \
                OR pg_catalog.has_function_privilege('public'::name, \
                    '_oo.lookup_physical_mapping(text, text, bigint, text, text, text, text)'::text, 'EXECUTE')",
            &[],
        )
        .await
        .expect("public");
    row.get(0)
}

async fn login_shadow_column_grants(client: &Client, login: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.column_privileges \
             WHERE grantee = $1 AND NOT (table_schema = '_oo' AND table_name = 'physical_mappings')",
            &[&login],
        )
        .await
        .expect("shadow columns")
        .get(0)
}

async fn login_mapping_column_grants(client: &Client, login: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.column_privileges \
             WHERE grantee = $1 AND table_schema = '_oo' AND table_name = 'physical_mappings' \
               AND privilege_type = 'SELECT'",
            &[&login],
        )
        .await
        .expect("mapping columns")
        .get(0)
}

async fn mapping_column_count(client: &Client) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.columns \
             WHERE table_schema = '_oo' AND table_name = 'physical_mappings'",
            &[],
        )
        .await
        .expect("mapping column count")
        .get(0)
}

async fn grant_text(client: &Client, role: &str) -> String {
    let rows = client
        .query(
            "SELECT table_name::text || '.' || column_name::text || ':' || privilege_type::text \
             FROM information_schema.column_privileges WHERE grantee = $1 ORDER BY 1",
            &[&role],
        )
        .await
        .expect("grants");
    rows.into_iter()
        .map(|row| row.get::<_, String>(0))
        .collect::<Vec<_>>()
        .join(",")
}

async fn mapping_identity(client: &Client) -> (String, i64) {
    let row = client
        .query_one(
            "SELECT config_digest::text, max(run_generation) \
             FROM _oo.physical_mappings WHERE source = $1 GROUP BY config_digest",
            &[&SOURCE_ID],
        )
        .await
        .expect("identity");
    (row.get(0), row.get(1))
}

async fn source_customer_count(admin_pass: &str, ca: &[u8]) -> i64 {
    let source = connect_db(SOURCE_DB, ADMIN, admin_pass, ca).await;
    source
        .query_one("SELECT count(*)::int8 FROM shop.customer", &[])
        .await
        .expect("count")
        .get(0)
}

async fn bool_sql(client: &Client, sql: &str) -> bool {
    client.query_one(sql, &[]).await.expect("bool").get(0)
}

async fn sqlstate_of(client: &Client, sql: &str) -> String {
    match client.query(sql, &[]).await {
        Ok(_) => "ok".to_string(),
        Err(err) => err
            .code()
            .map(|code| code.code().to_string())
            .unwrap_or_else(|| "none".to_string()),
    }
}

async fn server_facts(client: &Client) -> String {
    let ssl: bool = client
        .query_one(
            "SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()",
            &[],
        )
        .await
        .expect("ssl")
        .get(0);
    let version: String = client
        .query_one("SHOW server_version", &[])
        .await
        .expect("version")
        .get(0);
    format!("ssl={ssl}\nserver_version={version}\n")
}

async fn reset_cluster(admin: &Client) {
    for database in [SOURCE_DB, TARGET_DB] {
        exec(admin, &format!("DROP DATABASE IF EXISTS {database} WITH (FORCE)")).await;
    }
    for role in [
        READER,
        WRITER,
        RUNTIME,
        LOGIN,
        EXTRA,
        "oo_r_clerk",
        "oo_r_peek",
        "oo_r_extra",
        "oo_pipeline_runtime",
        "oo_target_owner",
    ] {
        let _ = admin
            .batch_execute(&format!(
                "DO $reset$ DECLARE member_name text; BEGIN \
                   IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = '{role}') THEN \
                     FOR member_name IN \
                       SELECT member_role.rolname FROM pg_catalog.pg_auth_members AS membership \
                       JOIN pg_catalog.pg_roles AS granted ON granted.oid = membership.roleid \
                       JOIN pg_catalog.pg_roles AS member_role ON member_role.oid = membership.member \
                       WHERE granted.rolname = '{role}' \
                     LOOP \
                       EXECUTE format('REVOKE %I FROM %I', '{role}', member_name); \
                     END LOOP; \
                     EXECUTE 'DROP OWNED BY {role}'; \
                     EXECUTE 'DROP ROLE {role}'; \
                   END IF; \
                 END $reset$;"
            ))
            .await;
    }
}

async fn create_login(admin: &Client, name: &str, password: &str, inherit: &str) {
    assert!(password.chars().all(|ch| ch.is_ascii_hexdigit()));
    assert!(name.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_'));
    exec(
        admin,
        &format!(
            "CREATE ROLE {name} LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION {inherit}"
        ),
    )
    .await;
}

async fn create_database(admin: &Client, name: &str) {
    exec(
        admin,
        &format!(
            "CREATE DATABASE {name} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'"
        ),
    )
    .await;
}

async fn exec(client: &Client, sql: &str) {
    client.batch_execute(sql).await.expect("exec");
}

async fn connect_db(database: &str, user: &str, password: &str, ca: &[u8]) -> Client {
    let (client, connection) = connect_pair(database, user, password, ca).await;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn connect_pair(database: &str, user: &str, password: &str, ca: &[u8]) -> (Client, TlsConnection) {
    let endpoint = TlsEndpoint {
        host: HOST.to_string(),
        port: 5432,
        database: database.to_string(),
        user: user.to_string(),
    };
    let mut last = String::new();
    for _ in 0..20 {
        match connect_require_tls(&endpoint, password, ca).await {
            Ok(pair) => return pair,
            Err(err) => {
                last = err.to_string();
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    panic!("tls connect to {database} as {user} failed: {last}");
}

fn init_request() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "protocol": "oo.pipeline.init-pg/v1",
        "connect_ms": 30000,
        "target": {
            "host": HOST,
            "port": 5432,
            "database": TARGET_DB,
            "administrator": ADMIN,
            "password_file": format!("{SECRET_DIR}/admin.pass"),
            "ca_file": format!("{SECRET_DIR}/ca.crt")
        }
    }))
    .expect("init request")
}

fn import_request() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "protocol": "oo.pipeline.import-pg/v1",
        "source": {
            "source_id": SOURCE_ID,
            "host": HOST,
            "port": 5432,
            "database": SOURCE_DB,
            "username": READER,
            "secret_ref": "secret://pg/a249/reader",
            "password_file": format!("{SECRET_DIR}/reader.pass"),
            "ca_file": format!("{SECRET_DIR}/ca.crt"),
            "tables": [
                {"schema": "shop", "table": "customer"},
                {"schema": "shop", "table": "order_doc"}
            ]
        },
        "target": {
            "host": HOST,
            "port": 5432,
            "database": TARGET_DB,
            "runtime_principal": RUNTIME,
            "password_file": format!("{SECRET_DIR}/runtime.pass"),
            "ca_file": format!("{SECRET_DIR}/ca.crt")
        },
        "shard": SHARD,
        "owner": OWNER,
        "source_generation": GENERATION
    }))
    .expect("import request")
}

fn run_bin(bin: &Path, args: &[&str], stdin_bytes: &[u8]) -> Output {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("PGPASSWORD")
        .env_remove("PGPASSFILE")
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin_bytes)
        .expect("write stdin");
    child.wait_with_output().expect("wait")
}

fn save_step(name: &str, output: &Output, bans: &[String]) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for ban in bans {
        assert!(!stdout.contains(ban), "{name} stdout contains a secret");
        assert!(!stderr.contains(ban), "{name} stderr contains a secret");
    }
    let dir = PathBuf::from(LIVE_DIR).join("steps");
    fs::write(dir.join(format!("{name}.stdout")), &output.stdout).expect("stdout");
    fs::write(dir.join(format!("{name}.stderr")), &output.stderr).expect("stderr");
    let code = output.status.code().unwrap_or(99);
    fs::write(dir.join(format!("{name}.exit")), format!("{code}\n")).expect("exit");
}

fn read_secret(name: &str) -> String {
    let text = fs::read_to_string(Path::new(SECRET_DIR).join(name)).expect(name);
    let trimmed = text.trim_end_matches(['\n', '\r']);
    assert!(!trimmed.is_empty());
    assert!(!trimmed.contains('\0'));
    trimmed.to_string()
}

fn bin_path() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_provision-mapping-reader") {
        return PathBuf::from(path);
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_provision_mapping_reader") {
        return PathBuf::from(path);
    }
    panic!("provision-mapping-reader binary path is missing");
}

fn run_cli_blocking(bytes: &[u8], args: &[&str]) -> Output {
    let mut command = Command::new(bin_path());
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("PGPASSWORD")
        .env_remove("PGPASSFILE");
    let mut child = command.spawn().expect("spawn provision-mapping-reader");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        let _ = stdin.write_all(bytes);
    }
    child.wait_with_output().expect("wait provision-mapping-reader")
}

async fn run_cli(bytes: Vec<u8>) -> Output {
    tokio::task::spawn_blocking(move || run_cli_blocking(&bytes, &[]))
        .await
        .expect("cli join")
}

fn assert_line(output: &Output, code: i32, line: &str) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty(), "stdout was not empty");
    assert_eq!(output.stderr, format!("{line}\n").as_bytes());
}

fn assert_receipt(output: &Output, effect: &str, login: &str, role: &str) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout.clone()).expect("utf8");
    assert!(!text.ends_with('\n'));
    let expected = format!(
        r#"{{"protocol":"{PROTOCOL}","effect":"{effect}","login":"{login}","role":"{role}"}}"#
    );
    assert_eq!(text, expected);
}

fn assert_quiet(bytes: &[u8], bans: &[String], label: &str) {
    let text = String::from_utf8_lossy(bytes);
    for ban in bans {
        assert!(!text.contains(ban.as_str()), "{label} contained a secret");
    }
}

fn base_doc() -> Value {
    json!({
        "protocol": PROTOCOL,
        "policies": [],
        "login": "clerk_login",
        "endpoint": {
            "host": "db",
            "port": 5432,
            "database": "oo_a249",
            "user": "oo_fixture_admin"
        },
        "password_file": "pw",
        "ca_file": "ca"
    })
}

fn sample_policy(row_filter: Value) -> Value {
    json!({
        "project": "shop",
        "config_digest": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "generation": 1,
        "role": "clerk",
        "login": "clerk_login",
        "table": {
            "namespace": "public",
            "name": "customer",
            "columns": [{
                "name": "id",
                "status": "confirmed",
                "semantic_type": "identifier",
                "pii": {"class": "none"}
            }]
        },
        "row_filter": row_filter
    })
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = env::temp_dir().join(format!("oo-a249-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp dir");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct Oracle {
    lines: Vec<String>,
    bans: Vec<String>,
}

impl Oracle {
    fn new(bans: &[&str]) -> Oracle {
        Oracle {
            lines: Vec::new(),
            bans: bans.iter().map(|ban| (*ban).to_string()).collect(),
        }
    }

    fn bans(&self) -> &[String] {
        &self.bans
    }

    fn fact(&mut self, key: &str, value: &str) {
        for ban in &self.bans {
            assert!(!key.contains(ban), "oracle key contains a secret");
            assert!(!value.contains(ban), "oracle fact contains a secret");
        }
        self.lines.push(format!("{key}={value}"));
        self.flush();
    }

    fn flush(&self) {
        let mut text = self.lines.join("\n");
        text.push('\n');
        for ban in &self.bans {
            assert!(!text.contains(ban), "oracle contains a secret");
        }
        fs::write(format!("{LIVE_DIR}/oracle.txt"), text).expect("oracle");
    }
}

const SOURCE_SQL: &str = "\
CREATE SCHEMA shop;\n\
CREATE TABLE shop.customer (\n\
  customer_id int8 PRIMARY KEY,\n\
  display_name text,\n\
  active bool NOT NULL,\n\
  private_note text,\n\
  ratio float8\n\
);\n\
CREATE TABLE shop.order_doc (\n\
  order_id int8 PRIMARY KEY,\n\
  customer_id int8 NOT NULL,\n\
  memo text,\n\
  posted bool NOT NULL,\n\
  amount int8 NOT NULL,\n\
  clerk_note text,\n\
  CONSTRAINT order_customer_fk FOREIGN KEY (customer_id) REFERENCES shop.customer (customer_id)\n\
);\n\
COMMENT ON TABLE shop.customer IS '甲注评客户名录';\n\
COMMENT ON COLUMN shop.customer.display_name IS '甲注评名称';\n\
CREATE TABLE shop.write_audit (who text NOT NULL);\n\
CREATE FUNCTION shop.note_write() RETURNS trigger LANGUAGE plpgsql AS $audit$\n\
BEGIN\n\
  INSERT INTO shop.write_audit (who) VALUES (current_user);\n\
  RETURN NEW;\n\
END;\n\
$audit$;\n\
CREATE TRIGGER customer_audit BEFORE INSERT OR UPDATE OR DELETE ON shop.customer\n\
  FOR EACH ROW EXECUTE FUNCTION shop.note_write();\n\
CREATE TRIGGER order_audit BEFORE INSERT OR UPDATE OR DELETE ON shop.order_doc\n\
  FOR EACH ROW EXECUTE FUNCTION shop.note_write();\n\
INSERT INTO shop.customer (customer_id, display_name, active, private_note) VALUES\n\
  (41, '丙号', true, '内部甲'),\n\
  (42, NULL, false, NULL),\n\
  (43, '丁铺', true, '内部乙');\n\
INSERT INTO shop.order_doc (order_id, customer_id, memo, posted, amount, clerk_note) VALUES\n\
  (701, 41, '首单', true, -9223372036854775808, '柜员甲'),\n\
  (702, 41, NULL, false, 9223372036854775807, NULL),\n\
  (703, 43, '零单', true, 0, '柜员乙');\n\
REVOKE ALL ON SCHEMA shop FROM PUBLIC;\n\
REVOKE ALL ON ALL TABLES IN SCHEMA shop FROM PUBLIC;\n\
GRANT USAGE ON SCHEMA shop TO oo_a249_reader;\n\
GRANT SELECT ON shop.customer, shop.order_doc TO oo_a249_reader;\n\
GRANT INSERT ON shop.customer, shop.order_doc, shop.write_audit TO oo_a249_writer;\n\
";
