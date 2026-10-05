//! Independent admission checks for the fixed `03512f91` guard.
//!
//! The harness supplies the snapshot. It does not resolve OIDs inside
//! `guard_select`, and it does not treat a successful parse as authentication.
//! The PostgreSQL section runs only when `OO_A179_LIVE=1`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use oo_core::api::ErrorCode;
use oo_gateway::{
    connect_require_tls, guard_select, GuardError, GuardedPlan, LiteralTypes, PolicyCatalog,
    PolicyIdentity, ResolvedFunction, ResolvedOperator, TlsEndpoint, VisibleColumn, VisibleRelation,
    PINNED_PARSER_VERSION,
};
use sha2::{Digest, Sha256};
use tokio_postgres::Client;

const INT_OID: u32 = 2301;
const TEXT_OID: u32 = 2501;
const BOOL_OID: u32 = 1601;
const LOWER_OID: u32 = 7001;
const COUNT_OID: u32 = 7002;
const EQ_INT_OID: u32 = 8001;
const EQ_INT_PROC: u32 = 8101;
const EQ_TEXT_OID: u32 = 8002;
const EQ_TEXT_PROC: u32 = 8102;
const HIDDEN: &str = "badge_secret_z9";
const HIDDEN_PHYSICAL: &str = "pc_badge";
const HOST: &str = "oo-a179-pg";
const DATABASE: &str = "oo_a179";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a179_login";
const OWNER: &str = "oo_a179_owner";
const SHADOW: &str = "oo_a179_shadow";
const SECRET_DIR: &str = "/run/oo-a179/fixture";
const ORACLE_PATH: &str = "/evidence/live/oracle.txt";

fn identity() -> PolicyIdentity {
    PolicyIdentity {
        project: "a179-review".to_string(),
        role: "yard_reader".to_string(),
        policy_id: "a179-policy".to_string(),
    }
}

fn column(name: &str, physical: &str, type_oid: u32) -> VisibleColumn {
    VisibleColumn {
        name: name.to_string(),
        physical_name: physical.to_string(),
        type_oid,
    }
}

fn yard(rows: u64) -> VisibleRelation {
    VisibleRelation {
        schema: "logistics".to_string(),
        name: "cargo_bay".to_string(),
        physical_schema: "ship_store".to_string(),
        physical_name: "pt_ship".to_string(),
        columns: vec![
            column("order_id", "pc_order", INT_OID),
            column("region", "pc_region", TEXT_OID),
        ],
        pending_unclassified: 1,
        estimated_rows: rows,
    }
}

fn desk() -> VisibleRelation {
    VisibleRelation {
        schema: "logistics".to_string(),
        name: "front_desk".to_string(),
        physical_schema: "ship_store".to_string(),
        physical_name: "pt_counter".to_string(),
        columns: vec![
            column("order_id", "pc_order", INT_OID),
            column("desk_name", "pc_name", TEXT_OID),
        ],
        pending_unclassified: 0,
        estimated_rows: 4,
    }
}

fn catalog(rows: u64) -> PolicyCatalog {
    PolicyCatalog {
        identity: identity(),
        relations: vec![yard(rows), desk()],
        functions: vec![
            ResolvedFunction {
                oid: LOWER_OID,
                schema: "pg_catalog".to_string(),
                name: "lower".to_string(),
                argument_type_oids: vec![TEXT_OID],
                aggregate_star: false,
            },
            ResolvedFunction {
                oid: COUNT_OID,
                schema: "pg_catalog".to_string(),
                name: "count".to_string(),
                argument_type_oids: Vec::new(),
                aggregate_star: true,
            },
        ],
        operators: vec![
            ResolvedOperator {
                oid: EQ_INT_OID,
                procedure_oid: EQ_INT_PROC,
                schema: "pg_catalog".to_string(),
                name: "=".to_string(),
                left_type_oid: INT_OID,
                right_type_oid: INT_OID,
            },
            ResolvedOperator {
                oid: EQ_TEXT_OID,
                procedure_oid: EQ_TEXT_PROC,
                schema: "pg_catalog".to_string(),
                name: "=".to_string(),
                left_type_oid: TEXT_OID,
                right_type_oid: TEXT_OID,
            },
        ],
        literal_types: LiteralTypes {
            integer: Some(INT_OID),
            text: Some(TEXT_OID),
            boolean: Some(BOOL_OID),
        },
        null_test_allowed: true,
        large_table_row_threshold: 1_000_000,
    }
}

fn physical_catalog(catalog: &PolicyCatalog) -> PolicyCatalog {
    let mut next = catalog.clone();
    for relation in &mut next.relations {
        relation.schema.clone_from(&relation.physical_schema);
        relation.name.clone_from(&relation.physical_name);
        for column in &mut relation.columns {
            column.name.clone_from(&column.physical_name);
        }
    }
    next
}

fn admit(sql: &str) -> Result<GuardedPlan, GuardError> {
    guard_select(&identity(), &catalog(10), sql)
}

fn sha256_hex(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        hex.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    hex
}

fn assert_silent(err: &GuardError, hidden: &[&str]) {
    let text = err.to_string();
    for name in hidden {
        assert!(!text.contains(name), "{text}");
    }
}

fn assert_rewritten(plan: &GuardedPlan, hidden: &[&str]) {
    assert_ne!(plan.executed_sql, plan.original_sql, "raw SQL was returned");
    assert_eq!(plan.parser_version, PINNED_PARSER_VERSION);
    assert_eq!(plan.original_sha256, sha256_hex(&plan.original_sql));
    assert_eq!(plan.executed_sha256, sha256_hex(&plan.executed_sql));
    assert!(!plan.fingerprint_hex.is_empty());
    for name in hidden {
        assert!(
            !plan.executed_sql.contains(name),
            "executed SQL contains {name}: {}",
            plan.executed_sql
        );
    }
    let parsed = pg_query::parse(&plan.executed_sql).expect("independent reparse");
    assert_eq!(parsed.protobuf.version, PINNED_PARSER_VERSION);
    let again = pg_query::deparse(&parsed.protobuf).expect("independent second deparse");
    assert_eq!(again, plan.executed_sql);
    let fingerprint = pg_query::fingerprint(&plan.executed_sql).expect("independent fingerprint");
    assert_eq!(fingerprint.hex, plan.fingerprint_hex);
    let physical = physical_catalog(&catalog(10));
    let second = guard_select(&identity(), &physical, &plan.executed_sql).expect("physical recheck");
    assert_eq!(second.executed_sql, plan.executed_sql);
    assert!(second.star_expansions.is_empty());
}

#[test]
fn alias_join_and_star_omit_the_hidden_column() {
    let sql = "SELECT s.*, d.desk_name \
               FROM logistics.cargo_bay AS s \
               INNER JOIN logistics.front_desk AS d ON s.order_id = d.order_id \
               WHERE s.region = 'west' AND s.order_id = 42";
    let plan = admit(sql).expect("join");
    assert_rewritten(
        &plan,
        &[HIDDEN, HIDDEN_PHYSICAL, "logistics", "cargo_bay", "front_desk"],
    );
    assert!(plan.executed_sql.contains("ship_store.pt_ship"));
    assert!(plan.executed_sql.contains("ship_store.pt_counter"));
    assert!(plan.executed_sql.contains("pc_region"));
    assert!(plan.executed_sql.contains("pc_name"));
    assert!(!plan.executed_sql.contains('*'));
    assert_eq!(plan.star_expansions.len(), 1);
    assert_eq!(plan.star_expansions[0].qualifier, "s");
    assert_eq!(plan.star_expansions[0].schema, "logistics");
    assert_eq!(plan.star_expansions[0].name, "cargo_bay");
    assert_eq!(
        plan.star_expansions[0].columns,
        vec!["order_id".to_string(), "region".to_string()]
    );
    assert_eq!(plan.bindings.relations.len(), 2);
    assert_eq!(plan.bindings.relations[0].alias.as_deref(), Some("s"));
    assert_eq!(plan.bindings.relations[0].physical_name, "pt_ship");
    assert_eq!(plan.bindings.relations[1].alias.as_deref(), Some("d"));
    assert_eq!(plan.bindings.operators.len(), 3);
    assert_eq!(
        plan.bindings
            .operators
            .iter()
            .filter(|op| op.oid == EQ_INT_OID)
            .count(),
        2
    );
    assert_eq!(
        plan.bindings
            .operators
            .iter()
            .filter(|op| op.oid == EQ_TEXT_OID)
            .count(),
        1
    );

    let bare = admit("SELECT * FROM logistics.cargo_bay AS s").expect("aliased star");
    assert_eq!(bare.star_expansions[0].columns.len(), 2);
    assert!(!bare.executed_sql.contains(HIDDEN));
    assert!(!bare.executed_sql.contains(HIDDEN_PHYSICAL));
}

#[test]
fn snapshot_oids_bind_one_operator_and_function_signature() {
    let lowered = admit("SELECT lower(s.region) FROM logistics.cargo_bay AS s").expect("lower");
    assert_rewritten(&lowered, &[HIDDEN, HIDDEN_PHYSICAL, "cargo_bay"]);
    assert_eq!(lowered.bindings.functions.len(), 1);
    assert_eq!(lowered.bindings.functions[0].oid, LOWER_OID);
    assert!(!lowered.bindings.functions[0].aggregate_star);
    assert!(lowered.executed_sql.contains("lower"));
    assert!(lowered.executed_sql.contains("pc_region"));

    let counted = admit("SELECT count(*) FROM logistics.cargo_bay").expect("count");
    assert!(counted.star_expansions.is_empty());
    assert_eq!(counted.bindings.functions[0].oid, COUNT_OID);
    assert!(counted.bindings.functions[0].aggregate_star);
    assert!(counted.executed_sql.contains("count(*)"));
    assert!(!counted.executed_sql.contains(HIDDEN_PHYSICAL));
    assert_ne!(counted.executed_sql, counted.original_sql);

    let wrong_arg = admit("SELECT lower(s.order_id) FROM logistics.cargo_bay AS s")
        .expect_err("lower(int)");
    assert!(matches!(wrong_arg, GuardError::FunctionNotAllowed));
    assert_eq!(wrong_arg.code(), ErrorCode::FunctionNotAllowed);
    assert_silent(&wrong_arg, &[HIDDEN, "lower"]);

    let wrong_pair = admit("SELECT region FROM logistics.cargo_bay WHERE region = 42")
        .expect_err("text = int");
    assert!(matches!(wrong_pair, GuardError::FunctionNotAllowed));

    let counted_column = admit("SELECT count(order_id) FROM logistics.cargo_bay").expect_err("count(column)");
    assert!(matches!(counted_column, GuardError::FunctionNotAllowed));
}

#[test]
fn missing_and_hidden_diagnostics_share_one_error() {
    let hidden_column = admit(&format!("SELECT {HIDDEN} FROM logistics.cargo_bay")).expect_err("hidden column");
    let missing_column = admit("SELECT ghost_col_z9 FROM logistics.cargo_bay").expect_err("missing column");
    assert_eq!(hidden_column.to_string(), missing_column.to_string());
    let text = hidden_column.to_string();
    assert!(text.contains("column is not visible"));
    assert!(text.contains("logistics.cargo_bay.order_id"));
    assert!(text.contains("logistics.cargo_bay.region"));
    assert!(text.contains("1 columns are pending classification"));
    assert!(!text.contains("42424242"));
    assert_silent(&hidden_column, &[HIDDEN, HIDDEN_PHYSICAL, "ghost_col_z9"]);
    assert_eq!(hidden_column.code(), ErrorCode::ColumnNotAllowed);

    let hidden_relation =
        admit("SELECT order_id FROM logistics.hidden_bay_z9").expect_err("hidden relation");
    let missing_relation =
        admit("SELECT order_id FROM logistics.missing_bay_z9").expect_err("missing relation");
    assert_eq!(hidden_relation.to_string(), missing_relation.to_string());
    assert_eq!(hidden_relation.to_string(), "relation is not visible");
    assert!(hidden_relation.hint().is_none());
    assert_silent(
        &hidden_relation,
        &["hidden_bay_z9", "missing_bay_z9", HIDDEN, "cargo_bay"],
    );
    assert_eq!(hidden_relation.code(), ErrorCode::RelationNotFound);

    let large = catalog(42_424_242);
    let open = guard_select(&identity(), &large, "SELECT * FROM logistics.cargo_bay").expect_err("open star");
    assert!(matches!(open, GuardError::CostTooHigh { .. }));
    assert_eq!(open.code(), ErrorCode::CostTooHigh);
    let open_text = open.to_string();
    assert!(open_text.contains("logistics.cargo_bay"));
    assert!(!open_text.contains("42424242"));
    assert_eq!(open.hint().as_deref(), Some("add a filter or use export"));
    let folded = guard_select(
        &identity(),
        &large,
        "SELECT * FROM logistics.cargo_bay WHERE 'yes'::bool",
    )
    .expect_err("true cast");
    assert!(matches!(folded, GuardError::CostTooHigh { .. }));
    let filtered = guard_select(
        &identity(),
        &large,
        "SELECT * FROM logistics.cargo_bay WHERE 'no'::bool",
    )
    .expect("false cast is a filter");
    assert!(filtered.star_expansions[0].columns.iter().all(|name| name != HIDDEN));
}

#[test]
fn unknown_ast_fails_closed_without_echoing_the_input() {
    let cases = [
        (
            "WITH x AS (DELETE FROM logistics.cargo_bay RETURNING order_id) SELECT order_id FROM x",
            "write",
        ),
        (
            "EXPLAIN INSERT INTO logistics.cargo_bay (order_id) VALUES (1)",
            "write",
        ),
        (
            "SELECT order_id FROM (SELECT order_id FROM logistics.cargo_bay) q",
            "subquery",
        ),
        (
            "SELECT order_id FROM logistics.cargo_bay WHERE order_id = $1",
            "parameter",
        ),
        (
            "SELECT order_id FROM logistics.cargo_bay WHERE order_id IN (1)",
            "in",
        ),
        ("SELECT order_id::int FROM logistics.cargo_bay", "cast"),
        ("SELECT relname FROM pg_catalog.pg_class", "system"),
        ("SELEKT badge_secret_z9", "parse"),
    ];
    for (sql, kind) in cases {
        let err = admit(sql).expect_err(kind);
        assert_silent(&err, &[HIDDEN, HIDDEN_PHYSICAL, "cargo_bay", "pg_class", "pg_catalog"]);
        match kind {
            "write" => assert!(matches!(err, GuardError::Write), "{sql} -> {err}"),
            "subquery" => assert!(
                matches!(err, GuardError::Unsupported { construct: "subquery" }),
                "{err}"
            ),
            "parameter" => assert!(
                matches!(err, GuardError::Unsupported { construct: "parameter" }),
                "{err}"
            ),
            "in" => assert!(
                matches!(err, GuardError::Unsupported { construct: "IN predicate" }),
                "{err}"
            ),
            "cast" => assert!(
                matches!(err, GuardError::Unsupported { construct: "cast" }),
                "{err}"
            ),
            "system" => {
                assert!(matches!(err, GuardError::SystemSchema));
                assert_eq!(err.to_string(), "system schema is not allowed");
            }
            "parse" => {
                assert!(matches!(err, GuardError::Parse));
                assert_eq!(err.to_string(), "SQL parse failed");
            }
            _ => unreachable!(),
        }
    }
}

fn oracle_fact(key: &str, value: &str) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(ORACLE_PATH)
        .expect("oracle");
    writeln!(file, "{key}={value}").expect("oracle write");
}

fn read_secret(name: &str) -> String {
    let raw = fs::read_to_string(Path::new(SECRET_DIR).join(name)).expect("secret file");
    let value = raw.trim();
    assert!(
        (16..=128).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "{name} is not lowercase hex"
    );
    value.to_string()
}

async fn connect_db(database: &str, user: &str, password: &str, ca: &[u8]) -> Client {
    let endpoint = TlsEndpoint {
        host: HOST.to_string(),
        port: 5432,
        database: database.to_string(),
        user: user.to_string(),
    };
    let mut last = String::from("unavailable");
    for _ in 0..20 {
        match connect_require_tls(&endpoint, password, ca).await {
            Ok((client, connection)) => {
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                return client;
            }
            Err(err) => {
                last = err.to_string();
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
    panic!("tls connect failed: {last}");
}

async fn exec(client: &Client, sql: &str) {
    client.batch_execute(sql).await.expect("fixture statement");
}

async fn sqlstate(client: &Client, sql: &str) -> String {
    match client.query(sql, &[]).await {
        Ok(_) => "ok".to_string(),
        Err(err) => err
            .as_db_error()
            .map(|db| db.code().code().to_string())
            .unwrap_or_else(|| "none".to_string()),
    }
}

async fn type_oid(client: &Client, name: &str) -> u32 {
    let oid: i32 = client
        .query_one(
            "SELECT oid::int4 FROM pg_catalog.pg_type \
             WHERE typname = $1 AND typnamespace = 'pg_catalog'::regnamespace",
            &[&name],
        )
        .await
        .expect("type oid")
        .get(0);
    u32::try_from(oid).expect("type oid range")
}

async fn operator_oid(client: &Client, left: u32, right: u32) -> (u32, u32) {
    let left_i = i32::try_from(left).expect("left");
    let right_i = i32::try_from(right).expect("right");
    let row = client
        .query_one(
            "SELECT o.oid::int4, o.oprcode::oid::int4 \
             FROM pg_catalog.pg_operator o \
             JOIN pg_catalog.pg_namespace n ON n.oid = o.oprnamespace \
             WHERE n.nspname = 'pg_catalog' AND o.oprname = '=' \
               AND o.oprleft = ($1::int4)::oid AND o.oprright = ($2::int4)::oid",
            &[&left_i, &right_i],
        )
        .await
        .expect("operator oid")
        .get::<_, i32>(0);
    let proc: i32 = client
        .query_one(
            "SELECT o.oprcode::oid::int4 \
             FROM pg_catalog.pg_operator o \
             JOIN pg_catalog.pg_namespace n ON n.oid = o.oprnamespace \
             WHERE n.nspname = 'pg_catalog' AND o.oprname = '=' \
               AND o.oprleft = ($1::int4)::oid AND o.oprright = ($2::int4)::oid",
            &[&left_i, &right_i],
        )
        .await
        .expect("operator proc")
        .get(0);
    (
        u32::try_from(row).expect("operator oid range"),
        u32::try_from(proc).expect("operator proc range"),
    )
}

async fn count_oid(client: &Client) -> u32 {
    let oid: i32 = client
        .query_one(
            "SELECT p.oid::int4 FROM pg_catalog.pg_proc p \
             JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = 'pg_catalog' AND p.proname = 'count' AND p.pronargs = 0",
            &[],
        )
        .await
        .expect("count oid")
        .get(0);
    u32::try_from(oid).expect("count oid range")
}

async fn lower_oid(client: &Client, text_oid: u32) -> u32 {
    let text_i = i32::try_from(text_oid).expect("text oid");
    let oid: i32 = client
        .query_one(
            "SELECT p.oid::int4 FROM pg_catalog.pg_proc p \
             JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = 'pg_catalog' AND p.proname = 'lower' \
               AND p.pronargs = 1 AND p.proargtypes[0] = ($1::int4)::oid",
            &[&text_i],
        )
        .await
        .expect("lower oid")
        .get(0);
    u32::try_from(oid).expect("lower oid range")
}

#[tokio::test]
async fn live_shadow_role_executes_guarded_sql_and_database_denies_the_rest() {
    if std::env::var("OO_A179_LIVE").ok().as_deref() != Some("1") {
        eprintln!("OO_A179_LIVE is unset; the PostgreSQL comparison was not run");
        return;
    }
    let _ = fs::remove_file(ORACLE_PATH);
    let admin_pass = read_secret("admin.pass");
    let login_pass = read_secret("login.pass");
    let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca");
    let admin = connect_db("postgres", ADMIN, &admin_pass, &ca).await;
    exec(
        &admin,
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
         WHERE datname = 'oo_a179' AND pid <> pg_backend_pid()",
    )
    .await;
    exec(&admin, "DROP DATABASE IF EXISTS oo_a179").await;
    exec(
        &admin,
        &format!("DROP ROLE IF EXISTS {LOGIN}; DROP ROLE IF EXISTS {SHADOW}; DROP ROLE IF EXISTS {OWNER}"),
    )
    .await;
    exec(&admin, "CREATE DATABASE oo_a179").await;
    let db = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
    let admin_ssl: bool = db
        .query_one(
            "SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = pg_backend_pid()",
            &[],
        )
        .await
        .expect("admin ssl")
        .get(0);
    oracle_fact("admin_ssl", if admin_ssl { "on" } else { "off" });
    assert!(admin_ssl);
    exec(
        &db,
        &format!(
            "CREATE ROLE {OWNER} NOLOGIN; \
             CREATE ROLE {SHADOW} NOLOGIN; \
             CREATE ROLE {LOGIN} LOGIN; \
             GRANT {SHADOW} TO {LOGIN}"
        ),
    )
    .await;
    if let Err(err) = db
        .batch_execute(&format!("ALTER ROLE {LOGIN} PASSWORD '{login_pass}'"))
        .await
    {
        let state = err
            .as_db_error()
            .map(|db| db.code().code().to_string())
            .unwrap_or_else(|| "none".to_string());
        panic!("set login password failed: {state}");
    }
    exec(
        &db,
        &format!(
            "CREATE SCHEMA _oo AUTHORIZATION {OWNER}; \
             CREATE SCHEMA logistics AUTHORIZATION {OWNER}; \
             REVOKE ALL ON SCHEMA _oo FROM PUBLIC; \
             REVOKE ALL ON SCHEMA logistics FROM PUBLIC; \
             GRANT USAGE ON SCHEMA _oo TO {SHADOW}; \
             GRANT USAGE ON SCHEMA logistics TO {SHADOW}; \
             CREATE TABLE _oo.pt_ship (pc_order integer, pc_region text, pc_badge text); \
             CREATE TABLE _oo.pt_counter (pc_order integer, pc_name text); \
             ALTER TABLE _oo.pt_ship OWNER TO {OWNER}; \
             ALTER TABLE _oo.pt_counter OWNER TO {OWNER}; \
             REVOKE ALL ON TABLE _oo.pt_ship FROM PUBLIC; \
             REVOKE ALL ON TABLE _oo.pt_counter FROM PUBLIC; \
             GRANT SELECT (pc_order, pc_region) ON TABLE _oo.pt_ship TO {SHADOW}; \
             GRANT SELECT (pc_order, pc_name) ON TABLE _oo.pt_counter TO {SHADOW}; \
             INSERT INTO _oo.pt_ship (pc_order, pc_region, pc_badge) \
               VALUES (42, 'west', '{HIDDEN}'); \
             INSERT INTO _oo.pt_counter (pc_order, pc_name) VALUES (42, 'ada')"
        ),
    )
    .await;
    let owner: String = db
        .query_one(
            "SELECT tableowner FROM pg_catalog.pg_tables \
             WHERE schemaname = '_oo' AND tablename = 'pt_ship'",
            &[],
        )
        .await
        .expect("owner")
        .get(0);
    oracle_fact("tableowner", &owner);
    assert_eq!(owner, OWNER);
    let int_oid = type_oid(&db, "int4").await;
    let text_oid = type_oid(&db, "text").await;
    let bool_oid = type_oid(&db, "bool").await;
    let eq_int = operator_oid(&db, int_oid, int_oid).await;
    let eq_text = operator_oid(&db, text_oid, text_oid).await;
    let lower_oid = lower_oid(&db, text_oid).await;
    let count_oid = count_oid(&db).await;
    oracle_fact("oid_source", "pg_catalog_query_by_harness");
    let id = identity();
    let mut snap = catalog(10);
    snap.identity = id.clone();
    for relation in &mut snap.relations {
        relation.physical_schema = "_oo".to_string();
        for column in &mut relation.columns {
            column.type_oid = if column.name == "order_id" {
                int_oid
            } else {
                text_oid
            };
        }
    }
    snap.functions[0].oid = lower_oid;
    snap.functions[0].argument_type_oids = vec![text_oid];
    snap.functions[1].oid = count_oid;
    snap.operators[0].oid = eq_int.0;
    snap.operators[0].procedure_oid = eq_int.1;
    snap.operators[0].left_type_oid = int_oid;
    snap.operators[0].right_type_oid = int_oid;
    snap.operators[1].oid = eq_text.0;
    snap.operators[1].procedure_oid = eq_text.1;
    snap.operators[1].left_type_oid = text_oid;
    snap.operators[1].right_type_oid = text_oid;
    snap.literal_types = LiteralTypes {
        integer: Some(int_oid),
        text: Some(text_oid),
        boolean: Some(bool_oid),
    };
    let join_sql = "SELECT s.region, d.desk_name \
                    FROM logistics.cargo_bay AS s \
                    INNER JOIN logistics.front_desk AS d ON s.order_id = d.order_id \
                    WHERE s.order_id = 42";
    let guarded = guard_select(&id, &snap, join_sql).expect("guard join");
    assert_ne!(guarded.executed_sql, join_sql);
    assert!(!guarded.executed_sql.contains(HIDDEN));
    assert!(!guarded.executed_sql.contains("logistics"));
    assert!(guarded.executed_sql.contains("_oo.pt_ship"));
    let parsed = pg_query::parse(&guarded.executed_sql).expect("live reparse");
    let again = pg_query::deparse(&parsed.protobuf).expect("live deparse");
    assert_eq!(again, guarded.executed_sql);
    let admin_row = db
        .query_one(
            "SELECT y.pc_region, d.pc_name \
             FROM _oo.pt_ship AS y \
             JOIN _oo.pt_counter AS d ON y.pc_order = d.pc_order \
             WHERE y.pc_order = 42",
            &[],
        )
        .await
        .expect("admin compare");
    let admin_region: String = admin_row.get(0);
    let admin_name: String = admin_row.get(1);

    let reader = connect_db(DATABASE, LOGIN, &login_pass, &ca).await;
    let reader_ssl: bool = reader
        .query_one(
            "SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = pg_backend_pid()",
            &[],
        )
        .await
        .expect("reader ssl")
        .get(0);
    oracle_fact("reader_ssl", if reader_ssl { "on" } else { "off" });
    assert!(reader_ssl);
    reader
        .batch_execute("SET search_path TO ''")
        .await
        .expect("search_path");
    reader
        .batch_execute(&format!("SET ROLE {SHADOW}"))
        .await
        .expect("set role");
    let who = reader
        .query_one("SELECT session_user, current_user", &[])
        .await
        .expect("who");
    let session_user: String = who.get(0);
    let current_user: String = who.get(1);
    oracle_fact("session_user", &session_user);
    oracle_fact("current_user", &current_user);
    assert_eq!(session_user, LOGIN);
    assert_eq!(current_user, SHADOW);
    assert_ne!(current_user, owner);

    let rows = reader
        .query(&guarded.executed_sql, &[])
        .await
        .expect("execute guarded join");
    assert_eq!(rows.len(), 1);
    let region: String = rows[0].get(0);
    let name: String = rows[0].get(1);
    assert_eq!(region, admin_region);
    assert_eq!(name, admin_name);
    oracle_fact("join_region", &region);
    oracle_fact("join_name", &name);
    oracle_fact("join_compare", "equal");

    let star = guard_select(&id, &snap, "SELECT s.* FROM logistics.cargo_bay AS s").expect("guard star");
    assert_eq!(star.star_expansions[0].columns, vec!["order_id".to_string(), "region".to_string()]);
    assert!(!star.executed_sql.contains(HIDDEN_PHYSICAL));
    let star_rows = reader
        .query(&star.executed_sql, &[])
        .await
        .expect("execute guarded star");
    assert_eq!(star_rows.len(), 1);
    assert_eq!(star_rows[0].len(), 2);
    let star_id: i32 = star_rows[0].get(0);
    let star_region: String = star_rows[0].get(1);
    assert_eq!(star_id, 42);
    assert_eq!(star_region, "west");
    oracle_fact("star_order_id", &star_id.to_string());
    oracle_fact("star_region", &star_region);
    oracle_fact("star_columns", "2");

    let hidden_state = sqlstate(&reader, "SELECT pc_badge FROM _oo.pt_ship").await;
    let write_state = sqlstate(
        &reader,
        "INSERT INTO _oo.pt_ship (pc_order, pc_region) VALUES (7, 'east')",
    )
    .await;
    let logical_state = sqlstate(&reader, "SELECT order_id FROM logistics.cargo_bay").await;
    oracle_fact("hidden_sqlstate", &hidden_state);
    oracle_fact("write_sqlstate", &write_state);
    oracle_fact("logical_sqlstate", &logical_state);
    assert_eq!(hidden_state, "42501");
    assert_eq!(write_state, "42501");
    assert_eq!(logical_state, "42P01");

    let counted = guard_select(&id, &snap, "SELECT count(*) FROM logistics.cargo_bay").expect("guard count");
    assert!(counted.bindings.functions[0].aggregate_star);
    assert_eq!(counted.bindings.functions[0].oid, count_oid);
    match reader.query(&counted.executed_sql, &[]).await {
        Ok(rows) => oracle_fact("count_sqlstate", &format!("ok:{}", rows.len())),
        Err(err) => oracle_fact(
            "count_sqlstate",
            err.as_db_error()
                .map(|db| db.code().code())
                .unwrap_or("none"),
        ),
    }

    let oracle = fs::read_to_string(ORACLE_PATH).expect("oracle readback");
    assert!(!oracle.contains(&admin_pass));
    assert!(!oracle.contains(&login_pass));
    assert!(!oracle.contains(HIDDEN));
    oracle_fact("live_result", "compared");
}
