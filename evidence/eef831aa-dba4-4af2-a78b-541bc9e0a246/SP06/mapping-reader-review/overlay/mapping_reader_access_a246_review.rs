//! Independent review of `provision_mapping_reader` on one fresh private TLS cluster.
//!
//! Runs only when `OO_A246_LIVE=1`. Public `init-pg` and `import-pg` install the
//! target. `install_column_acl_set` installs the aggregate column set. This file
//! is not in the fixed commit. Passwords stay in files.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use oo_gateway::mapping_reader_access::{
    provision_mapping_reader, MappingReaderEffect, MappingReaderError, MappingReaderOutcome,
};
use oo_gateway::policy::catalog::{load_policy_catalog, CatalogError, CatalogLimits, UnknownRows};
use oo_gateway::policy::{
    compile_column_acl, connect_require_tls, install_column_acl_set, rustls_connector, ClassStatus,
    ColumnAclPolicy, ColumnSpec, InstallEffect, LogicalTable, PiiClass, TlsEndpoint,
};
use oo_gateway::query_executor::{
    AuditAdmission, Cell, CompletionRecord, CompletionStatus, DedicatedSession, ExecError, SelectRequest,
    SelectStream,
};
use oo_gateway::{PolicyIdentity, TlsConnection};
use serde_json::{json, Value};
use tokio_postgres::Client;

const HOST: &str = "oo-a246-pg";
const SOURCE_DB: &str = "oo_a246_src";
const TARGET_DB: &str = "oo_a246_dst";
const ADMIN: &str = "oo_fixture_admin";
const READER: &str = "oo_a246_reader";
const RUNTIME: &str = "oo_a246_runtime";
const LOGIN: &str = "oo_a246_login";
const EXTRA: &str = "oo_a246_extra";
const MISSING_LOGIN: &str = "oo_a246_missing";
const SECRET_DIR: &str = "/secrets";
const LIVE_DIR: &str = "/evidence/live";
const SOURCE_ID: &str = "shop";
const GENERATION: i64 = 7;
const STALE_GENERATION: i64 = 3;
const OWNER: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a246";
const SHARD: &str = "shard_a246";

#[tokio::test]
async fn review_mapping_reader_on_fresh_tls() {
    if env::var("OO_A246_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    let init_bin = env::var("OO_A246_INIT_PG").expect("OO_A246_INIT_PG");
    let pipeline_bin = env::var("OO_A246_PIPELINE").expect("OO_A246_PIPELINE");
    let admin_pass = read_secret("admin.pass");
    let reader_pass = read_secret("reader.pass");
    let runtime_pass = read_secret("runtime.pass");
    let login_pass = read_secret("login.pass");
    let extra_pass = read_secret("extra.pass");
    let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca.crt");
    let mut oracle = Oracle::new(&[
        &admin_pass,
        &reader_pass,
        &runtime_pass,
        &login_pass,
        &extra_pass,
    ]);
    fs::create_dir_all(format!("{LIVE_DIR}/steps")).expect("live steps");
    assert!(
        !Path::new(SECRET_DIR).join("server.key").exists(),
        "review fixture must not contain a server key"
    );
    assert!(!Path::new(SECRET_DIR).join("ca.key").exists());

    let admin = connect_db("postgres", ADMIN, &admin_pass, &ca).await;
    reset_cluster(&admin).await;
    create_login(&admin, READER, &reader_pass, "INHERIT").await;
    create_login(&admin, RUNTIME, &runtime_pass, "INHERIT").await;
    create_login(&admin, LOGIN, &login_pass, "NOINHERIT").await;
    create_login(&admin, EXTRA, &extra_pass, "NOINHERIT").await;
    create_database(&admin, SOURCE_DB).await;
    create_database(&admin, TARGET_DB).await;
    exec(
        &admin,
        &format!("GRANT CONNECT ON DATABASE {SOURCE_DB} TO {READER}"),
    )
    .await;
    exec(
        &admin,
        &format!("GRANT CONNECT ON DATABASE {TARGET_DB} TO {RUNTIME}, {LOGIN}, {EXTRA}"),
    )
    .await;
    let server = server_facts(&admin).await;
    assert!(
        server.starts_with("ssl=true\n") || server.starts_with("ssl=t\n"),
        "{server}"
    );
    assert!(server.contains("server_version=17.10"), "{server}");
    fs::write(format!("{LIVE_DIR}/server.txt"), &server).expect("server");
    oracle.fact("server", "tls_17.10");

    let source = connect_db(SOURCE_DB, ADMIN, &admin_pass, &ca).await;
    exec(&source, SOURCE_SQL).await;
    drop(source);
    let reader = connect_db(SOURCE_DB, READER, &reader_pass, &ca).await;
    assert_eq!(
        sqlstate_of(
            &reader,
            "INSERT INTO shop.customer (customer_id, display_name, active) VALUES (9, 'no', true)"
        )
        .await,
        "42501"
    );
    drop(reader);
    oracle.fact("source_reader_insert_before", "42501");
    assert_eq!(source_count(&admin_pass, &ca, "customer").await, 3);
    assert_eq!(source_count(&admin_pass, &ca, "order_doc").await, 3);

    let early = run_bin(
        Path::new(&pipeline_bin),
        &["import-pg"],
        &import_request(),
    );
    save_step("import-pg-not-ready", &early, oracle.bans());
    assert_eq!(early.status.code(), Some(1));
    assert!(early.stdout.is_empty());
    let early_err = String::from_utf8_lossy(&early.stderr);
    assert!(
        early_err.contains("target schema is not ready"),
        "early import stderr"
    );
    oracle.fact("import_before_init", "target schema is not ready");

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
        &format!("GRANT CONNECT ON DATABASE {TARGET_DB} TO {RUNTIME}, {LOGIN}, {EXTRA}"),
    )
    .await;
    exec(&admin, &format!("GRANT oo_pipeline_runtime TO {RUNTIME}")).await;
    let imported = run_bin(Path::new(&pipeline_bin), &["import-pg"], &import_request());
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
    oracle.fact("mapping_generation", "7");
    let names = physical_names(&target, &digest).await;
    assert!(names.contains_key("customer.private_note"));
    assert!(names.contains_key("customer.ratio"));
    assert!(names.contains_key("customer"));

    let publications = count_sql(&target, "SELECT count(*)::int8 FROM pg_catalog.pg_publication").await;
    assert_eq!(publications, 0);
    assert!(!runtime_owner(&target, "oo_pipeline_runtime").await);
    assert!(!runtime_owner(&target, RUNTIME).await);
    oracle.fact("publication", "0");
    oracle.fact("runtime_owner", "false");

    let clerk = role_set(
        "clerk",
        LOGIN,
        &digest,
        &["customer_id", "display_name", "active"],
        &["order_id", "customer_id", "memo", "posted", "amount", "clerk_note"],
    );
    let outside = clerk_columns(&clerk);
    assert!(!outside.iter().any(|name| *name == "private_note" || *name == "ratio"));
    let installed = install_column_acl_set(&mut target, &clerk)
        .await
        .expect("aggregate acl");
    assert_eq!(installed.effect, InstallEffect::Installed);
    assert_eq!(installed.role, "oo_r_clerk");
    assert_eq!(installed.tables.len(), 2);
    assert!(mapping_absent(&target, LOGIN).await);
    oracle.fact("column_acl", "installed");
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
    exec(&login_client, "SET search_path TO ''").await;
    assert_eq!(
        sqlstate_of(&login_client, "SELECT count(*) FROM \"_oo\".physical_mappings").await,
        "42501"
    );
    drop(login_client);
    oracle.fact("catalog_before", "42501");
    oracle.fact("mapping_select_before", "42501");

    assert_eq!(
        provision_mapping_reader(&mut target, "oo_a246_other", &clerk).await,
        Err(MappingReaderError::LoginMismatch)
    );
    assert!(mapping_absent(&target, LOGIN).await);
    oracle.fact("login_mismatch", "LoginMismatch");

    let mut stale = clerk.clone();
    for policy in &mut stale {
        policy.generation = STALE_GENERATION;
    }
    assert_eq!(
        provision_mapping_reader(&mut target, LOGIN, &stale).await,
        Err(MappingReaderError::StaleGeneration {
            policy: STALE_GENERATION,
            current: Some(GENERATION),
        })
    );
    assert!(mapping_absent(&target, LOGIN).await);
    oracle.fact("stale", "StaleGeneration");

    let mut missing_table = clerk.clone();
    missing_table[1].table.name = "missing".to_string();
    assert_eq!(
        provision_mapping_reader(&mut target, LOGIN, &missing_table).await,
        Err(MappingReaderError::MappingAbsent)
    );
    assert!(mapping_absent(&target, LOGIN).await);
    oracle.fact("mapping_absent", "MappingAbsent");

    let mut unknown = clerk.clone();
    for policy in &mut unknown {
        policy.login = MISSING_LOGIN.to_string();
    }
    assert_eq!(
        provision_mapping_reader(&mut target, MISSING_LOGIN, &unknown).await,
        Err(MappingReaderError::LoginAbsent)
    );
    assert!(mapping_absent(&target, LOGIN).await);
    oracle.fact("login_absent", "LoginAbsent");

    let extra_set = role_set(
        "extra",
        EXTRA,
        &digest,
        &["customer_id", "display_name", "active"],
        &["order_id", "customer_id", "amount"],
    );
    let extra_installed = install_column_acl_set(&mut target, &extra_set)
        .await
        .expect("extra role");
    assert_eq!(extra_installed.effect, InstallEffect::Installed);
    exec(
        &target,
        &format!("GRANT USAGE ON SCHEMA \"_oo\" TO \"{EXTRA}\""),
    )
    .await;
    assert_eq!(
        provision_mapping_reader(&mut target, EXTRA, &extra_set).await,
        Err(MappingReaderError::UnexpectedPrivilege)
    );
    let extra_flags = mapping_flags(&target, EXTRA).await;
    assert!(extra_flags.usage);
    assert!(!extra_flags.select);
    assert!(!extra_flags.lookup);
    assert!(mapping_absent(&target, LOGIN).await);
    oracle.fact("partial_privilege", "usage_only_refused");

    let grants_before = grant_text(&target, "oo_r_clerk").await;
    assert!(!grants_before.is_empty());
    let provisioned = provision_mapping_reader(&mut target, LOGIN, &clerk)
        .await
        .expect("provision");
    assert_eq!(
        provisioned,
        MappingReaderOutcome {
            effect: MappingReaderEffect::Installed,
            login: LOGIN.to_string(),
            role: "oo_r_clerk".to_string(),
        }
    );
    assert!(mapping_exact(&target, LOGIN).await);
    assert_eq!(grant_text(&target, "oo_r_clerk").await, grants_before);
    assert_eq!(login_shadow_column_grants(&target, LOGIN).await, 0);
    assert_eq!(
        login_mapping_column_grants(&target, LOGIN).await,
        mapping_column_count(&target).await
    );
    assert!(!public_mapping(&target).await);
    assert!(!runtime_owner(&target, LOGIN).await);
    oracle.fact("provision", "Installed");
    oracle.fact("business_grants", "unchanged");
    oracle.fact("shadow_columns", "0");
    oracle.fact("public", "absent");

    let replay = provision_mapping_reader(&mut target, LOGIN, &clerk)
        .await
        .expect("replay");
    assert_eq!(replay.effect, MappingReaderEffect::Replay);
    assert_eq!(replay.role, "oo_r_clerk");
    assert!(mapping_exact(&target, LOGIN).await);
    oracle.fact("replay", "Replay");

    let catalog = load_one(&login_pass, &ca, "oo_r_clerk", &digest, &clerk).await;
    assert_eq!(
        relation_names(&catalog),
        vec!["customer".to_string(), "order_doc".to_string()]
    );
    assert_eq!(
        column_names(&catalog, "customer"),
        vec![
            "customer_id".to_string(),
            "display_name".to_string(),
            "active".to_string(),
        ]
    );
    assert!(!column_names(&catalog, "customer").iter().any(|name| name == "private_note"));
    let rows = compiled_query(
        &login_pass,
        &ca,
        "oo_r_clerk",
        &digest,
        &catalog,
        "SELECT customer_id, display_name, active FROM shop.customer ORDER BY customer_id",
    )
    .await;
    assert_eq!(
        rows,
        vec![
            vec![
                Cell::Int("41".to_string()),
                Cell::Text("丙号".to_string()),
                Cell::Bool(true),
            ],
            vec![Cell::Int("42".to_string()), Cell::Null, Cell::Bool(false)],
            vec![
                Cell::Int("43".to_string()),
                Cell::Text("丁铺".to_string()),
                Cell::Bool(true),
            ],
        ]
    );
    oracle.fact("catalog_tables", "2");
    oracle.fact("typed_query", "clerk_customer_3");

    let direct = connect_db(TARGET_DB, LOGIN, &login_pass, &ca).await;
    exec(&direct, "SET search_path TO ''").await;
    hidden_denied(&direct, "oo_r_clerk", &names, "customer", "private_note").await;
    exec(&direct, "SET ROLE \"oo_r_clerk\"").await;
    let star = format!(
        "SELECT * FROM \"_oo\".{}",
        quote_physical(names.get("customer").expect("customer table"))
    );
    assert_eq!(sqlstate_of(&direct, &star).await, "42501");
    exec(&direct, "RESET ROLE").await;
    oracle.fact("hidden", "42501");

    let admin_rows = mapping_rows(&target).await;
    let login_rows = mapping_rows(&direct).await;
    assert_eq!(login_rows, admin_rows);
    assert!(admin_rows.len() > 9, "mapping row count {}", admin_rows.len());
    assert!(admin_rows.iter().any(|row| row == "c|customer|private_note"));
    assert!(admin_rows.iter().any(|row| row == "c|customer|ratio"));
    oracle.fact("mapping_rows", "all");
    oracle.fact("mapping_row_count", &admin_rows.len().to_string());
    oracle.fact("outside_policy", "private_note,ratio");
    oracle.fact("tenant_isolation", "not_provided");
    drop(direct);

    assert_eq!(source_count(&admin_pass, &ca, "customer").await, 3);
    assert_eq!(source_count(&admin_pass, &ca, "order_doc").await, 3);
    let audit = source_audit(&admin_pass, &ca).await;
    assert_eq!(audit.get(READER).copied().unwrap_or(0), 0);
    assert_eq!(audit.get(LOGIN).copied().unwrap_or(0), 0);
    assert_eq!(audit.get(RUNTIME).copied().unwrap_or(0), 0);
    assert_eq!(audit.get("oo_pipeline_runtime").copied().unwrap_or(0), 0);
    assert!(audit.get(ADMIN).copied().unwrap_or(0) >= 6);
    let reader_again = connect_db(SOURCE_DB, READER, &reader_pass, &ca).await;
    assert_eq!(
        sqlstate_of(
            &reader_again,
            "INSERT INTO shop.customer (customer_id, display_name, active) VALUES (9, 'no', true)"
        )
        .await,
        "42501"
    );
    assert!(!runtime_owner(&target, "oo_pipeline_runtime").await);
    assert!(!runtime_owner(&target, RUNTIME).await);
    assert!(!runtime_owner(&target, LOGIN).await);
    assert_eq!(
        count_sql(&target, "SELECT count(*)::int8 FROM pg_catalog.pg_publication").await,
        0
    );
    oracle.fact("source_writes", "reader_runtime_login_zero");
    oracle.fact("runtime_owner_after", "false");

    let outcome = json!({
        "protocol": "oo.gateway.mapping-reader-access-review/v1",
        "parent_acceptance": false,
        "release": false,
        "effect": "Installed",
        "replay": "Replay",
        "catalog_before": "42501",
        "hidden": "42501",
        "mapping_rows": "all",
        "tenant_isolation": "not_provided",
        "source_reader_writes": 0,
        "runtime_owner": false
    });
    let outcome_text = serde_json::to_string_pretty(&outcome).expect("outcome");
    for ban in oracle.bans() {
        assert!(!outcome_text.contains(ban), "outcome contains a secret");
    }
    fs::write(format!("{LIVE_DIR}/outcome.json"), outcome_text).expect("outcome");
    fs::write(format!("{LIVE_DIR}/outcome.txt"), "mapping_reader_review_recorded\n").expect("outcome text");
    oracle.fact("outcome", "mapping_reader_review_recorded");
}

fn clerk_columns(policies: &[ColumnAclPolicy]) -> Vec<&str> {
    policies
        .iter()
        .flat_map(|policy| policy.table.columns.iter().map(|column| column.name.as_str()))
        .collect()
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
    catalog
        .relations
        .iter()
        .find(|relation| relation.name == table)
        .expect("relation")
        .columns
        .iter()
        .map(|column| column.name.clone())
        .collect()
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

async fn hidden_denied(
    client: &Client,
    role: &str,
    names: &BTreeMap<String, String>,
    table: &str,
    column: &str,
) {
    exec(client, &format!("SET ROLE \"{role}\"")).await;
    let sql = format!(
        "SELECT {} FROM \"_oo\".{}",
        quote_physical(names.get(&format!("{table}.{column}")).expect("column")),
        quote_physical(names.get(table).expect("table"))
    );
    assert_eq!(sqlstate_of(client, &sql).await, "42501", "{role} {column}");
    exec(client, "RESET ROLE").await;
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

async fn physical_names(client: &Client, digest: &str) -> BTreeMap<String, String> {
    let rows = client
        .query(
            "SELECT level::text, logical_table::text, logical_column::text, physical_name::text \
             FROM _oo.physical_mappings \
             WHERE source = $1 AND config_digest = $2 AND run_generation = $3",
            &[&SOURCE_ID, &digest, &GENERATION],
        )
        .await
        .expect("names");
    let mut names = BTreeMap::new();
    for row in rows {
        let level: String = row.get(0);
        let table: String = row.get(1);
        let column: String = row.get(2);
        let physical: String = row.get(3);
        if level == "t" {
            names.insert(table, physical);
        } else if level == "c" {
            names.insert(format!("{table}.{column}"), physical);
        }
    }
    names
}

async fn mapping_rows(client: &Client) -> Vec<String> {
    let rows = client
        .query(
            "SELECT level::text || '|' || logical_table::text || '|' || logical_column::text \
             FROM \"_oo\".physical_mappings ORDER BY 1",
            &[],
        )
        .await
        .expect("mapping rows");
    rows.into_iter().map(|row| row.get(0)).collect()
}

async fn runtime_owner(client: &Client, role: &str) -> bool {
    let row = client
        .query_one(
            "SELECT pg_catalog.pg_has_role($1::name, 'oo_target_owner'::name, 'member')",
            &[&role],
        )
        .await
        .expect("owner");
    row.get(0)
}

fn quote_physical(name: &str) -> String {
    let rest = name
        .strip_prefix("pt_")
        .or_else(|| name.strip_prefix("pc_"))
        .expect("prefix");
    assert_eq!(name.len(), 63);
    assert_eq!(rest.len(), 60);
    assert!(rest
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    format!("\"{name}\"")
}

async fn source_count(admin_pass: &str, ca: &[u8], table: &str) -> i64 {
    assert!(table == "customer" || table == "order_doc");
    let source = connect_db(SOURCE_DB, ADMIN, admin_pass, ca).await;
    count_sql(&source, &format!("SELECT count(*)::int8 FROM shop.{table}")).await
}

async fn source_audit(admin_pass: &str, ca: &[u8]) -> BTreeMap<String, i64> {
    let source = connect_db(SOURCE_DB, ADMIN, admin_pass, ca).await;
    let rows = source
        .query(
            "SELECT who::text, count(*)::int8 FROM shop.write_audit GROUP BY who",
            &[],
        )
        .await
        .expect("audit");
    rows.into_iter().map(|row| (row.get(0), row.get(1))).collect()
}

async fn count_sql(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.expect("count").get(0)
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
        exec(
            admin,
            &format!("DROP DATABASE IF EXISTS {database} WITH (FORCE)"),
        )
        .await;
    }
    for role in [
        READER,
        RUNTIME,
        LOGIN,
        EXTRA,
        "oo_r_clerk",
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
    assert!(inherit == "INHERIT" || inherit == "NOINHERIT");
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
            "secret_ref": "secret://pg/a246/reader",
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
GRANT USAGE ON SCHEMA shop TO oo_a246_reader;\n\
GRANT SELECT ON shop.customer, shop.order_doc TO oo_a246_reader;\n\
";
