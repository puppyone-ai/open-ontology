//! Independent review of bounded literal `IN` on archive 6f87942.
//!
//! The live PostgreSQL comparison runs only when `OO_A347_LIVE=1`.
//! Expected integers are `i64::MIN`, `i64::MAX`, and zero. This file does
//! not read an author oracle.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use oo_core::api::ErrorCode;
use oo_gateway::policy::catalog::{CatalogLimits, UnknownRows, load_policy_catalog};
use oo_gateway::sql_guard::{IN_LIST_LIMIT, PINNED_PARSER_VERSION, SQL_BYTE_BUDGET};
use oo_gateway::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, GuardError, GuardedPlan, InstallEffect,
    LiteralTypes, LogicalTable, OutputItem, PiiClass, PolicyCatalog, PolicyIdentity,
    ResolvedFunction, ResolvedOperator, TlsEndpoint, VisibleColumn, VisibleRelation,
    compile_column_acl, connect_require_tls, guard_select, install_column_acl,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_postgres::Client;
use tokio_postgres::types::ToSql;

const INT8_OID: u32 = 20;
const INT4_OID: u32 = 23;
const TEXT_OID: u32 = 25;
const BOOL_OID: u32 = 16;
const EQ_CROSS: u32 = 701;
const EQ_SAME: u32 = 702;
const NE_CROSS: u32 = 703;
const NE_SAME: u32 = 704;
const EQ_TEXT: u32 = 705;
const NE_TEXT: u32 = 706;
const EQ_BOOL: u32 = 707;
const NE_BOOL: u32 = 708;

const QTY_SQL: &str =
    "SELECT entry_id, qty FROM shop.entry WHERE entry_id IN (0, 1, 2) ORDER BY entry_id";

const HOST: &str = "oo-a347-pg";
const DATABASE: &str = "oo_a347";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a347_login";
const SECRET_DIR: &str = "/run/oo-a347/fixture";
const SOURCE: &str = "shopfix";
const CONFIG: &str = "a347a347a347a347a347a347a347a347a347a347a347a347a347a347a347a347";
const TYPE_DIGEST: &str = "b347b347b347b347b347b347b347b347b347b347b347b347b347b347b347b347";
const INPUT_DIGEST: &str = "c347c347c347c347c347c347c347c347c347c347c347c347c347c347c347c347";
const SCHEMA_DIGEST: &str = "d347d347d347d347d347d347d347d347d347d347d347d347d347d347d347d347";
const NAMESPACE: &str = "shop";
const TABLE: &str = "entry";
const GENERATION: i64 = 347;
const ADVISORY_LOCK: i64 = 64_042_026_347;
const OWNER: &str = "a3470000-0000-4000-8000-000000000347";
const SHARD: &str = "a347main";

static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(1);

fn identity() -> PolicyIdentity {
    PolicyIdentity {
        project: "shopfix".to_string(),
        role: "oo_r_wide".to_string(),
        policy_id: "policy-a347".to_string(),
    }
}

fn col(name: &str, physical: &str, type_oid: u32) -> VisibleColumn {
    VisibleColumn {
        name: name.to_string(),
        physical_name: physical.to_string(),
        type_oid,
    }
}

fn operator(oid: u32, name: &str, left: u32, right: u32) -> ResolvedOperator {
    ResolvedOperator {
        oid,
        procedure_oid: oid + 50,
        schema: "pg_catalog".to_string(),
        name: name.to_string(),
        left_type_oid: left,
        right_type_oid: right,
    }
}

fn operators() -> Vec<ResolvedOperator> {
    vec![
        operator(EQ_CROSS, "=", INT8_OID, INT4_OID),
        operator(EQ_SAME, "=", INT8_OID, INT8_OID),
        operator(NE_CROSS, "<>", INT8_OID, INT4_OID),
        operator(NE_SAME, "<>", INT8_OID, INT8_OID),
        operator(EQ_TEXT, "=", TEXT_OID, TEXT_OID),
        operator(NE_TEXT, "<>", TEXT_OID, TEXT_OID),
        operator(EQ_BOOL, "=", BOOL_OID, BOOL_OID),
        operator(NE_BOOL, "<>", BOOL_OID, BOOL_OID),
    ]
}

fn catalog(rows: u64, ops: Vec<ResolvedOperator>, integer: Option<u32>) -> PolicyCatalog {
    PolicyCatalog {
        identity: identity(),
        relations: vec![VisibleRelation {
            schema: "shop".to_string(),
            name: "entry".to_string(),
            physical_schema: "_oo".to_string(),
            physical_name: "pt_entry".to_string(),
            columns: vec![
                col("entry_id", "pc_entry", INT8_OID),
                col("qty", "pc_qty", INT8_OID),
                col("label", "pc_label", TEXT_OID),
                col("active", "pc_active", BOOL_OID),
            ],
            pending_unclassified: 1,
            estimated_rows: rows,
        }],
        functions: vec![ResolvedFunction {
            oid: 801,
            schema: "pg_catalog".to_string(),
            name: "count".to_string(),
            argument_type_oids: Vec::new(),
            aggregate_star: true,
        }],
        operators: ops,
        literal_types: LiteralTypes {
            integer,
            text: Some(TEXT_OID),
            boolean: Some(BOOL_OID),
        },
        null_test_allowed: true,
        large_table_row_threshold: 100,
    }
}

fn small() -> PolicyCatalog {
    catalog(10, operators(), Some(INT4_OID))
}

fn large() -> PolicyCatalog {
    catalog(2_000_000, operators(), Some(INT4_OID))
}

fn admit(catalog: &PolicyCatalog, sql: &str) -> GuardedPlan {
    guard_select(&catalog.identity, catalog, sql).unwrap_or_else(|err| panic!("{sql}\n{err}"))
}

fn refuse(catalog: &PolicyCatalog, sql: &str) -> GuardError {
    guard_select(&catalog.identity, catalog, sql).expect_err(sql)
}

fn digest(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

fn construct(err: &GuardError) -> &'static str {
    match err {
        GuardError::Unsupported { construct } => construct,
        other => panic!("expected unsupported, got {other}"),
    }
}

fn assert_kept(plan: &GuardedPlan, marker: &str) {
    assert_eq!(plan.parser_version, PINNED_PARSER_VERSION);
    assert_eq!(plan.original_sha256, digest(&plan.original_sql));
    assert_eq!(plan.executed_sha256, digest(&plan.executed_sql));
    assert!(!plan.fingerprint_hex.is_empty());
    assert!(plan.executed_sql.contains(marker), "{}", plan.executed_sql);
    assert!(plan.executed_sql.contains("pt_entry"), "{}", plan.executed_sql);
    assert!(plan.executed_sql.contains("pc_entry"), "{}", plan.executed_sql);
    assert!(!plan.executed_sql.contains("shop"), "{}", plan.executed_sql);
    assert!(!plan.executed_sql.contains("::"), "{}", plan.executed_sql);
    assert!(
        !plan.executed_sql.contains("= 0") && !plan.executed_sql.contains("= 1"),
        "{}",
        plan.executed_sql
    );
}

fn oids(plan: &GuardedPlan) -> Vec<u32> {
    plan.bindings.operators.iter().map(|op| op.oid).collect()
}

#[test]
fn literal_in_keeps_the_list_and_the_cross_type_operator() {
    assert_eq!(IN_LIST_LIMIT, 32);
    let plan = admit(&small(), QTY_SQL);
    assert_eq!(plan.original_sql, QTY_SQL);
    assert_kept(&plan, "IN (0, 1, 2)");
    assert!(!plan.executed_sql.contains(" OR "), "{}", plan.executed_sql);
    assert!(!plan.executed_sql.contains(" AND "), "{}", plan.executed_sql);
    assert_eq!(oids(&plan), vec![EQ_CROSS, EQ_CROSS, EQ_CROSS]);
    assert!(plan.bindings.operators.iter().all(|op| op.name == "="));
    assert_eq!(plan.bindings.relations[0].physical_schema, "_oo");
    assert_eq!(plan.bindings.relations[0].physical_name, "pt_entry");
    match &plan.bindings.outputs[0] {
        OutputItem::Column(column) => {
            assert_eq!(column.name, "entry_id");
            assert_eq!(column.output_name, "entry_id");
            assert_eq!(column.physical_name, "pc_entry");
        }
        other => panic!("{other:?}"),
    }
    match &plan.bindings.outputs[1] {
        OutputItem::Column(column) => {
            assert_eq!(column.output_name, "qty");
            assert_eq!(column.physical_name, "pc_qty");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn not_in_keeps_the_node_and_uses_inequality() {
    let sql = "SELECT entry_id FROM shop.entry WHERE entry_id NOT IN (0, 1)";
    let plan = admit(&small(), sql);
    assert_kept(&plan, "NOT IN (0, 1)");
    assert!(!plan.executed_sql.contains(" AND "), "{}", plan.executed_sql);
    assert!(!plan.executed_sql.contains(" OR "), "{}", plan.executed_sql);
    assert_eq!(oids(&plan), vec![NE_CROSS, NE_CROSS]);
    assert!(plan.bindings.operators.iter().all(|op| op.name == "<>"));
}

#[test]
fn null_stays_in_the_list_and_uses_the_same_type_operator() {
    let sql = "SELECT entry_id FROM shop.entry WHERE entry_id IN (0, NULL, 2)";
    let plan = admit(&small(), sql);
    assert_kept(&plan, "IN (0, NULL, 2)");
    assert!(!plan.executed_sql.contains(" OR "), "{}", plan.executed_sql);
    assert_eq!(oids(&plan), vec![EQ_CROSS, EQ_SAME, EQ_CROSS]);
    let not_in = admit(
        &small(),
        "SELECT entry_id FROM shop.entry WHERE entry_id NOT IN (0, NULL)",
    );
    assert_kept(&not_in, "NOT IN (0, NULL)");
    assert!(!not_in.executed_sql.contains(" AND "), "{}", not_in.executed_sql);
    assert_eq!(oids(&not_in), vec![NE_CROSS, NE_SAME]);
}

#[test]
fn a_signed_integer_constant_is_a_literal() {
    let plan = admit(
        &small(),
        "SELECT entry_id FROM shop.entry WHERE entry_id IN (-1)",
    );
    assert_kept(&plan, "IN (-1)");
    assert_eq!(oids(&plan), vec![EQ_CROSS]);
    let err = refuse(
        &small(),
        "SELECT entry_id FROM shop.entry WHERE entry_id IN (-qty)",
    );
    assert_eq!(construct(&err), "unary operator");
}

#[test]
fn text_and_bool_literals_resolve_their_own_operators() {
    let text = admit(
        &small(),
        "SELECT label FROM shop.entry WHERE label IN ('a', 'b')",
    );
    assert!(text.executed_sql.contains("IN ('a', 'b')"), "{}", text.executed_sql);
    assert!(!text.executed_sql.contains(" OR "), "{}", text.executed_sql);
    assert_eq!(oids(&text), vec![EQ_TEXT, EQ_TEXT]);
    let boolean = admit(
        &small(),
        "SELECT active FROM shop.entry WHERE active IN (true, false)",
    );
    assert!(
        boolean.executed_sql.contains("IN (true, false)"),
        "{}",
        boolean.executed_sql
    );
    assert_eq!(oids(&boolean), vec![EQ_BOOL, EQ_BOOL]);
    let mismatch = refuse(
        &small(),
        "SELECT entry_id FROM shop.entry WHERE entry_id IN ('a')",
    );
    assert_eq!(mismatch, GuardError::FunctionNotAllowed);
}

#[test]
fn the_list_bound_is_one_through_thirty_two() {
    let mut body = String::new();
    for index in 0..IN_LIST_LIMIT {
        if index > 0 {
            body.push_str(", ");
        }
        body.push_str(&index.to_string());
    }
    let full = format!("SELECT entry_id FROM shop.entry WHERE entry_id IN ({body})");
    let plan = admit(&small(), &full);
    assert_eq!(plan.bindings.operators.len(), IN_LIST_LIMIT);
    assert!(plan.executed_sql.contains("IN (0, 1, 2, "), "{}", plan.executed_sql);
    assert!(!plan.executed_sql.contains(" OR "), "{}", plan.executed_sql);
    body.push_str(", 32");
    let over = format!("SELECT entry_id FROM shop.entry WHERE entry_id IN ({body})");
    assert_eq!(construct(&refuse(&small(), &over)), "IN predicate");
    let empty = refuse(&small(), "SELECT entry_id FROM shop.entry WHERE entry_id IN ()");
    assert!(
        matches!(empty, GuardError::Parse)
            || construct(&empty) == "IN predicate"
    );
}

#[test]
fn a_hidden_column_is_not_granted_by_the_list() {
    let left = refuse(&small(), "SELECT entry_id FROM shop.entry WHERE memo IN (0, 1)");
    assert!(matches!(left, GuardError::ColumnNotAllowed { .. }));
    assert_eq!(left.code(), ErrorCode::ColumnNotAllowed);
    let shown = left.to_string();
    assert!(!shown.contains("memo"), "{shown}");
    assert!(!shown.contains("pc_"), "{shown}");
    let element = refuse(
        &small(),
        "SELECT entry_id FROM shop.entry WHERE entry_id IN (memo)",
    );
    assert_eq!(construct(&element), "IN predicate");
    assert!(!element.to_string().contains("memo"));
}

#[test]
fn subquery_cast_function_parameter_float_and_any_fail_closed() {
    let cases = [
        (
            "SELECT entry_id FROM shop.entry WHERE entry_id IN (SELECT entry_id FROM shop.entry)",
            "subquery",
        ),
        (
            "SELECT entry_id FROM shop.entry WHERE entry_id IN (0::int8)",
            "cast",
        ),
        (
            "SELECT entry_id FROM shop.entry WHERE entry_id IN (abs(0))",
            "function",
        ),
        (
            "SELECT entry_id FROM shop.entry WHERE entry_id IN ($1)",
            "parameter",
        ),
        (
            "SELECT entry_id FROM shop.entry WHERE entry_id IN (1.5)",
            "float literal",
        ),
        (
            "SELECT entry_id FROM shop.entry WHERE entry_id = ANY (ARRAY[0, 1, 2])",
            "ANY predicate",
        ),
        (
            "SELECT entry_id FROM shop.entry WHERE EXISTS (SELECT 1)",
            "subquery",
        ),
    ];
    for (sql, label) in cases {
        let err = refuse(&small(), sql);
        assert_eq!(construct(&err), label, "{sql}");
        assert_eq!(err.code(), ErrorCode::StatementNotAllowed);
    }
}

#[test]
fn a_missing_operator_refuses_without_inserting_a_cast() {
    let mut ops = operators();
    ops.retain(|op| op.oid != EQ_CROSS);
    let no_cross = catalog(10, ops, Some(INT4_OID));
    assert_eq!(
        refuse(&no_cross, "SELECT entry_id FROM shop.entry WHERE entry_id IN (0, 1)"),
        GuardError::FunctionNotAllowed
    );
    let mut same_only = operators();
    same_only.retain(|op| op.oid != EQ_SAME);
    let no_same = catalog(10, same_only, Some(INT4_OID));
    let kept = admit(&no_same, "SELECT entry_id FROM shop.entry WHERE entry_id IN (0)");
    assert!(kept.executed_sql.contains("IN (0)"), "{}", kept.executed_sql);
    assert!(!kept.executed_sql.contains("::"), "{}", kept.executed_sql);
    assert_eq!(
        refuse(&no_same, "SELECT entry_id FROM shop.entry WHERE entry_id IN (NULL)"),
        GuardError::FunctionNotAllowed
    );
    let unresolved = catalog(10, operators(), None);
    assert_eq!(
        refuse(&unresolved, "SELECT entry_id FROM shop.entry WHERE entry_id IN (0)"),
        GuardError::FunctionNotAllowed
    );
}

#[test]
fn an_admitted_in_is_a_real_r9_filter() {
    let open = refuse(&large(), "SELECT * FROM shop.entry");
    match &open {
        GuardError::CostTooHigh { relation } => assert_eq!(relation, "shop.entry"),
        other => panic!("{other}"),
    }
    assert_eq!(open.code(), ErrorCode::CostTooHigh);
    assert_eq!(open.hint().as_deref(), Some("add a filter or use export"));
    let filtered = admit(&large(), "SELECT * FROM shop.entry WHERE entry_id IN (0, 1, 2)");
    assert_kept(&filtered, "IN (0, 1, 2)");
    assert!(!filtered.executed_sql.contains('*'), "{}", filtered.executed_sql);
    let folded = refuse(
        &large(),
        "SELECT * FROM shop.entry WHERE true OR entry_id IN (0, 1, 2)",
    );
    match folded {
        GuardError::CostTooHigh { relation } => assert_eq!(relation, "shop.entry"),
        other => panic!("{other}"),
    }
    let false_or = admit(
        &large(),
        "SELECT * FROM shop.entry WHERE false OR entry_id IN (0, 1, 2)",
    );
    assert!(false_or.executed_sql.contains("IN (0, 1, 2)"), "{}", false_or.executed_sql);
    assert!(false_or.executed_sql.contains(" OR "), "{}", false_or.executed_sql);
    assert!(!false_or.executed_sql.contains("= 0"), "{}", false_or.executed_sql);
    let on_only = refuse(
        &large(),
        "SELECT * FROM shop.entry JOIN shop.entry AS other ON entry.entry_id IN (0)",
    );
    match on_only {
        GuardError::CostTooHigh { relation } => assert_eq!(relation, "shop.entry"),
        other => panic!("{other}"),
    }
    let explicit = admit(
        &small(),
        "SELECT entry_id FROM shop.entry WHERE true OR entry_id IN (0, 1, 2)",
    );
    assert!(explicit.executed_sql.contains("IN (0, 1, 2)"), "{}", explicit.executed_sql);
    assert!(explicit.executed_sql.contains(" OR "), "{}", explicit.executed_sql);
    assert!(!explicit.executed_sql.contains("= 0"), "{}", explicit.executed_sql);
}

#[test]
fn the_byte_budget_still_applies_to_an_in_predicate() {
    let base = "SELECT entry_id FROM shop.entry WHERE entry_id IN (0)";
    let mut sql = base.to_string();
    sql.push_str(&" ".repeat(SQL_BYTE_BUDGET + 1 - base.len()));
    assert!(sql.len() > SQL_BYTE_BUDGET);
    assert_eq!(refuse(&small(), &sql), GuardError::Budget);
    assert_eq!(refuse(&small(), &sql).code(), ErrorCode::StatementNotAllowed);
}

#[test]
fn older_shapes_stay_where_they_were() {
    let like = refuse(&small(), "SELECT label FROM shop.entry WHERE label LIKE 'a'");
    assert_eq!(construct(&like), "LIKE predicate");
    let between = refuse(
        &small(),
        "SELECT entry_id FROM shop.entry WHERE entry_id BETWEEN 0 AND 2",
    );
    assert_eq!(construct(&between), "BETWEEN predicate");
    let cast = admit(&small(), "SELECT active FROM shop.entry WHERE 'true'::bool");
    assert!(!cast.executed_sql.contains(" IN "), "{}", cast.executed_sql);
    let equality = admit(&small(), "SELECT entry_id FROM shop.entry WHERE entry_id = 0");
    assert!(!equality.executed_sql.contains(" IN "), "{}", equality.executed_sql);
    assert_eq!(oids(&equality), vec![EQ_CROSS]);
    let alias = admit(
        &small(),
        "SELECT e.entry_id FROM shop.entry AS e WHERE e.entry_id IN (0, 1, 2)",
    );
    assert_kept(&alias, "IN (0, 1, 2)");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_query_pg_returns_int8_extrema_for_the_exact_in_list() {
    if env::var("OO_A347_LIVE").ok().as_deref() != Some("1") {
        eprintln!("OO_A347_LIVE is unset; the private postgres comparison was not run");
        return;
    }
    let min_qty = i64::MIN.to_string();
    let max_qty = i64::MAX.to_string();
    let admin_pass = read_secret("admin.pass");
    let login_pass = read_secret("login.pass");
    let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca");
    let bans = [admin_pass.clone(), login_pass.clone()];
    reset_database(&admin_pass, &ca).await;
    let mut admin = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
    apply_frozen_schema(&mut admin).await;
    create_login(&admin, &login_pass).await;
    grant_mapping_read(&admin).await;
    let shadow = prepare_entry(&mut admin).await;
    analyze_shadow(&admin, &shadow.table).await;
    let policy = acl(&["entry_id", "qty"]);
    let installed = install_column_acl(&mut admin, &policy).await.expect("install");
    assert_eq!(installed.effect, InstallEffect::Installed);
    assert_eq!(installed.role, "oo_r_wide");
    let compiled = compile_column_acl(&policy).expect("compile");
    let mut login = connect_db(DATABASE, LOGIN, &login_pass, &ca).await;
    let loaded = load_policy_catalog(&mut login, &identity_live(), &[compiled], &limits())
        .await
        .expect("catalog");
    assert!(loaded.relations[0].estimated_rows >= 1);
    assert!(!loaded.relations[0].columns.iter().any(|column| column.name == "memo"));
    let before = fetch_stored(&admin, &shadow).await;
    let work = scratch();
    let password_file = Path::new(SECRET_DIR).join("login.pass");
    let ca_file = Path::new(SECRET_DIR).join("ca.crt");

    let qty = run_query(&work, &password_file, &ca_file, QTY_SQL, 3, &bans);
    assert_eq!(qty.code, 0, "stderr {}", String::from_utf8_lossy(&qty.stderr));
    let qty_turn = one_turn(&qty.lines);
    let expected = vec![
        vec!["0".to_string(), min_qty.clone()],
        vec!["1".to_string(), max_qty.clone()],
        vec!["2".to_string(), "0".to_string()],
    ];
    assert_eq!(int_rows(&qty_turn.rows), expected);
    let plan = assert_plan_hashes(&qty_turn.summary, QTY_SQL, &loaded);
    assert!(plan.executed_sql.contains("IN (0, 1, 2)"), "{}", plan.executed_sql);
    assert!(!plan.executed_sql.contains(" OR "), "{}", plan.executed_sql);
    assert!(!plan.executed_sql.contains("::"), "{}", plan.executed_sql);
    assert!(!plan.executed_sql.contains("shop"), "{}", plan.executed_sql);
    assert_eq!(plan.bindings.operators.len(), 3);
    assert_resolved(&plan, &loaded, "=", 3);
    assert_eq!(admin_ints(&admin, &plan.executed_sql, 2).await, expected);
    assert_audit(&qty.audit, QTY_SQL, &loaded, &bans);
    fs::write("/evidence/live/executed-qty.sql", &plan.executed_sql).expect("executed sql");
    fs::write("/evidence/live/qty.stdout.jsonl", &qty.stdout).expect("stdout");

    let with_null = "SELECT entry_id FROM shop.entry WHERE entry_id IN (0, NULL, 2) ORDER BY entry_id";
    let null_run = run_query(&work, &password_file, &ca_file, with_null, 10, &bans);
    assert_eq!(null_run.code, 0, "{}", String::from_utf8_lossy(&null_run.stderr));
    let null_turn = one_turn(&null_run.lines);
    assert_eq!(int_rows(&null_turn.rows), vec![vec!["0".to_string()], vec!["2".to_string()]]);
    let null_plan = assert_plan_hashes(&null_turn.summary, with_null, &loaded);
    assert!(null_plan.executed_sql.contains("IN (0, NULL, 2)"), "{}", null_plan.executed_sql);
    assert!(!null_plan.executed_sql.contains(" OR "), "{}", null_plan.executed_sql);
    assert_eq!(admin_ints(&admin, &null_plan.executed_sql, 1).await, int_rows(&null_turn.rows));

    let not_null = "SELECT entry_id FROM shop.entry WHERE entry_id NOT IN (0, NULL) ORDER BY entry_id";
    let not_null_run = run_query(&work, &password_file, &ca_file, not_null, 10, &bans);
    assert_eq!(not_null_run.code, 0, "{}", String::from_utf8_lossy(&not_null_run.stderr));
    let not_null_turn = one_turn(&not_null_run.lines);
    assert!(not_null_turn.rows.is_empty());
    let not_null_plan = assert_plan_hashes(&not_null_turn.summary, not_null, &loaded);
    assert!(not_null_plan.executed_sql.contains("NOT IN (0, NULL)"), "{}", not_null_plan.executed_sql);
    assert!(admin_ints(&admin, &not_null_plan.executed_sql, 1).await.is_empty());

    let not_in = "SELECT entry_id FROM shop.entry WHERE entry_id NOT IN (0, 1) ORDER BY entry_id";
    let not_in_run = run_query(&work, &password_file, &ca_file, not_in, 10, &bans);
    assert_eq!(not_in_run.code, 0, "{}", String::from_utf8_lossy(&not_in_run.stderr));
    let not_in_turn = one_turn(&not_in_run.lines);
    assert_eq!(int_rows(&not_in_turn.rows), vec![vec!["2".to_string()], vec!["7".to_string()]]);
    let not_in_plan = assert_plan_hashes(&not_in_turn.summary, not_in, &loaded);
    assert!(not_in_plan.executed_sql.contains("NOT IN (0, 1)"), "{}", not_in_plan.executed_sql);
    assert!(!not_in_plan.executed_sql.contains(" AND "), "{}", not_in_plan.executed_sql);

    let equal = "SELECT qty FROM shop.entry WHERE entry_id = 0";
    let equal_run = run_query(&work, &password_file, &ca_file, equal, 5, &bans);
    assert_eq!(equal_run.code, 0, "{}", String::from_utf8_lossy(&equal_run.stderr));
    assert_eq!(int_rows(&one_turn(&equal_run.lines).rows), vec![vec![min_qty.clone()]]);

    let refusals = [
        "SELECT memo FROM shop.entry",
        "SELECT entry_id FROM shop.entry WHERE memo IN ('x')",
        "SELECT entry_id FROM shop.entry WHERE entry_id IN (SELECT 0)",
        "SELECT entry_id FROM shop.entry WHERE entry_id IN (0::int8)",
        "INSERT INTO shop.entry (entry_id, qty) VALUES (9, 9)",
        "SELECT entry_id FROM shop.entry WHERE entry_id IN ($1)",
        "SELECT entry_id FROM shop.entry WHERE entry_id = ANY (ARRAY[0, 1, 2])",
        "SELECT entry_id FROM shop.entry WHERE entry_id IN (abs(0))",
    ];
    for sql in refusals {
        let run = run_query(&work, &password_file, &ca_file, sql, 5, &bans);
        assert_eq!(run.code, 6, "{sql} stderr {}", String::from_utf8_lossy(&run.stderr));
        assert_eq!(run.stderr, b"query-pg: guard\n");
        assert!(run.stdout.is_empty(), "{sql}");
        assert!(!run.audit.exists(), "{sql}");
    }

    let after = fetch_stored(&admin, &shadow).await;
    assert_eq!(before, after);
    let gold = format!(
        "qty_0={min_qty}\nqty_1={max_qty}\nqty_2=0\nnull_in=0,2\nnot_in_null=\nnot_in=2,7\nequal_0={min_qty}\nguard_refusals=8\nrows_unchanged=yes\nexecuted_contains_or=no\n"
    );
    fs::create_dir_all("/evidence/live").expect("live dir");
    fs::write("/evidence/live/gold.txt", gold).expect("gold");
}

fn assert_resolved(plan: &GuardedPlan, catalog: &PolicyCatalog, name: &str, count: usize) {
    let column = catalog.relations[0]
        .columns
        .iter()
        .find(|column| column.name == "entry_id")
        .expect("entry_id");
    let literal = catalog.literal_types.integer.expect("integer literal oid");
    let want = catalog
        .operators
        .iter()
        .filter(|op| {
            op.name == name && op.left_type_oid == column.type_oid && op.right_type_oid == literal
        })
        .map(|op| op.oid)
        .collect::<Vec<_>>();
    assert_eq!(want.len(), 1, "cross-type operator was not unique");
    assert_eq!(plan.bindings.operators.len(), count);
    assert!(plan.bindings.operators.iter().all(|op| op.oid == want[0] && op.name == name));
}

fn identity_live() -> PolicyIdentity {
    PolicyIdentity {
        project: SOURCE.to_string(),
        role: "oo_r_wide".to_string(),
        policy_id: CONFIG.to_string(),
    }
}

fn limits() -> CatalogLimits {
    CatalogLimits {
        max_tables: 4,
        max_columns: 64,
        max_functions: 1,
        max_operators: 64,
        large_table_row_threshold: 1_000_000,
        unknown_rows: UnknownRows::Refuse,
    }
}

fn acl(columns: &[&str]) -> ColumnAclPolicy {
    ColumnAclPolicy {
        project: SOURCE.to_string(),
        config_digest: CONFIG.to_string(),
        generation: GENERATION,
        role: "wide".to_string(),
        login: LOGIN.to_string(),
        table: LogicalTable {
            namespace: NAMESPACE.to_string(),
            name: TABLE.to_string(),
            columns: columns.iter().copied().map(column_spec).collect(),
        },
        row_filter: None,
    }
}

fn column_spec(name: &str) -> ColumnSpec {
    ColumnSpec {
        name: name.to_string(),
        status: ClassStatus::Confirmed,
        semantic_type: "value".to_string(),
        pii: PiiClass::None,
    }
}

struct Shadow {
    table: String,
    entry_id: String,
    qty: String,
    memo: String,
}

struct Run {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    lines: Vec<Value>,
    audit: PathBuf,
}

struct Turn {
    rows: Vec<Vec<Value>>,
    summary: Value,
}

fn run_query(dir: &Path, password_file: &Path, ca_file: &Path, sql: &str, max_rows: u64, bans: &[String]) -> Run {
    let stamp = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let audit = dir.join(format!("audit-{stamp}.jsonl"));
    let request = dir.join(format!("request-{stamp}.json"));
    let columns = ["entry_id", "qty"]
        .into_iter()
        .map(|name| {
            json!({
                "name": name,
                "status": "confirmed",
                "semantic_type": "value",
                "pii": {"class": "none"}
            })
        })
        .collect::<Vec<_>>();
    let doc = json!({
        "version": "oo.gateway.query-pg/v1",
        "endpoint": {"host": HOST, "port": 5432, "database": DATABASE, "user": LOGIN},
        "password_file": password_file.display().to_string(),
        "ca_file": ca_file.display().to_string(),
        "audit_file": audit.display().to_string(),
        "identity": {"project": SOURCE, "role": "oo_r_wide", "policy_id": CONFIG},
        "policies": [{
            "project": SOURCE,
            "config_digest": CONFIG,
            "generation": GENERATION,
            "role": "wide",
            "login": LOGIN,
            "table": {"namespace": NAMESPACE, "name": TABLE, "columns": columns}
        }],
        "catalog": {
            "max_tables": 4,
            "max_columns": 64,
            "max_functions": 1,
            "max_operators": 64,
            "large_table_row_threshold": 1000000,
            "unknown_rows": "refuse"
        },
        "statement": {
            "source": "request",
            "sql": sql,
            "max_rows": max_rows,
            "max_result_bytes": 8192,
            "wall_clock_ms": 30000
        }
    });
    let bytes = serde_json::to_vec(&doc).unwrap();
    assert_quiet(&bytes, bans);
    fs::write(&request, &bytes).unwrap();
    let output = run_raw(&["--request", request.to_str().unwrap()]);
    assert_quiet(&output.stdout, bans);
    assert_quiet(&output.stderr, bans);
    let lines = if output.stdout.is_empty() {
        Vec::new()
    } else {
        json_lines(&output.stdout)
    };
    Run {
        code: output.status.code().expect("exit"),
        stdout: output.stdout,
        stderr: output.stderr,
        lines,
        audit,
    }
}

fn run_raw(args: &[&str]) -> Output {
    let mut command = Command::new(query_pg_exe());
    command.args(args);
    command.env_remove("PGPASSWORD");
    command.stdin(Stdio::null());
    command.output().expect("run query-pg")
}

fn query_pg_exe() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_query-pg") {
        return PathBuf::from(path);
    }
    let mut path = env::current_exe().expect("current_exe");
    path.pop();
    path.pop();
    path.push("query-pg");
    path
}

fn scratch() -> PathBuf {
    let path = env::temp_dir().join(format!(
        "oo-a347-bounded-in-{}-{}",
        std::process::id(),
        SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn one_turn(lines: &[Value]) -> Turn {
    let mut rows = Vec::new();
    let mut summary = None;
    for line in lines {
        match line["record"].as_str() {
            Some("row") => rows.push(line["cells"].as_array().expect("cells").clone()),
            Some("summary") => {
                assert!(summary.is_none());
                summary = Some(line.clone());
            }
            other => panic!("unexpected record {other:?}"),
        }
    }
    Turn {
        rows,
        summary: summary.expect("summary"),
    }
}

fn int_rows(rows: &[Vec<Value>]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|cell| {
                    assert_eq!(cell["kind"], "int");
                    cell["value"].as_str().expect("int text").to_string()
                })
                .collect()
        })
        .collect()
}

fn assert_plan_hashes(summary: &Value, sql: &str, catalog: &PolicyCatalog) -> GuardedPlan {
    let plan = guard_select(&identity_live(), catalog, sql).expect("guard");
    assert_eq!(summary["original_sha256"].as_str().unwrap(), digest(sql));
    assert_eq!(summary["executed_sha256"].as_str().unwrap(), plan.executed_sha256);
    assert_eq!(summary["status"], "succeeded");
    assert_eq!(summary["truncated"], false);
    assert_eq!(summary["end_audit_ok"], true);
    plan
}

fn assert_audit(path: &Path, sql: &str, catalog: &PolicyCatalog, bans: &[String]) {
    let raw = fs::read(path).expect("audit");
    assert_quiet(&raw, bans);
    assert!(!raw.windows(6).any(|window| window == b"SELECT"));
    let values = json_lines(&raw);
    assert_eq!(values.len(), 2);
    assert_eq!(values[0]["record"], "admit");
    assert_eq!(values[1]["record"], "complete");
    let plan = guard_select(&identity_live(), catalog, sql).expect("audit guard");
    assert_eq!(values[0]["original_sha256"].as_str().unwrap(), digest(sql));
    assert_eq!(values[0]["executed_sha256"].as_str().unwrap(), plan.executed_sha256);
    assert_eq!(values[1]["executed_sha256"], values[0]["executed_sha256"]);
    assert_eq!(values[0]["fingerprint_hex"].as_str().unwrap(), plan.fingerprint_hex);
}

fn json_lines(bytes: &[u8]) -> Vec<Value> {
    let text = std::str::from_utf8(bytes).expect("utf8");
    text.split('\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).expect("json"))
        .collect()
}

fn assert_quiet(bytes: &[u8], bans: &[String]) {
    let text = String::from_utf8_lossy(bytes);
    for ban in bans {
        assert!(!text.contains(ban), "output contained a secret");
    }
}

fn read_secret(name: &str) -> String {
    let raw = fs::read_to_string(Path::new(SECRET_DIR).join(name)).expect("secret");
    let value = raw.trim().to_string();
    assert!(
        (16..=128).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{name} is not lowercase hex"
    );
    value
}

async fn connect_db(database: &str, user: &str, password: &str, ca: &[u8]) -> Client {
    let endpoint = TlsEndpoint {
        host: HOST.to_string(),
        port: 5432,
        database: database.to_string(),
        user: user.to_string(),
    };
    let mut last = String::new();
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
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    assert!(!last.contains(password), "connect error contained a password");
    panic!("tls connect failed");
}

async fn reset_database(admin_pass: &str, ca: &[u8]) {
    let maintenance = connect_db("postgres", ADMIN, admin_pass, ca).await;
    exec_hidden(
        &maintenance,
        "terminate",
        &format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{DATABASE}' AND pid <> pg_backend_pid()"
        ),
    )
    .await;
    exec_hidden(&maintenance, "drop-database", &format!("DROP DATABASE IF EXISTS \"{DATABASE}\"")).await;
    exec_hidden(
        &maintenance,
        "drop-roles",
        "DO $$\n\
         DECLARE role_name text;\n\
         BEGIN\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a347_login') THEN\n\
             DROP ROLE oo_a347_login;\n\
           END IF;\n\
           FOR role_name IN\n\
             SELECT rolname FROM pg_catalog.pg_roles\n\
             WHERE rolname LIKE 'oo\\_r\\_%' ESCAPE '\\'\n\
           LOOP\n\
             EXECUTE format('DROP ROLE %I', role_name);\n\
           END LOOP;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_pipeline_runtime') THEN\n\
             DROP ROLE oo_pipeline_runtime;\n\
           END IF;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_target_owner') THEN\n\
             DROP ROLE oo_target_owner;\n\
           END IF;\n\
         END\n\
         $$",
    )
    .await;
    exec_hidden(
        &maintenance,
        "create-database",
        &format!("CREATE DATABASE \"{DATABASE}\" TEMPLATE template0 ENCODING 'UTF8'"),
    )
    .await;
}

async fn apply_frozen_schema(client: &mut Client) {
    let bootstrap = migration("0000_bootstrap_roles.sql");
    let fence = migration("0001_target_fence_expand.sql");
    let preflight = migration("0003_target_segment_preflight_expand.sql");
    let mapping = migration("0004_physical_mapping.sql");
    let full_import = migration("0005_full_import_expand.sql");
    let tx = begin_locked(client).await;
    exec_tx(&tx, "0000_bootstrap_roles.sql", &bootstrap.0).await;
    exec_tx(&tx, "set-owner", "SET LOCAL ROLE oo_target_owner").await;
    exec_tx(&tx, "0001_target_fence_expand.sql", &fence.0).await;
    exec_tx(&tx, "row-lock", "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE").await;
    exec_tx(&tx, "0003_target_segment_preflight_expand.sql", &preflight.0).await;
    exec_tx(&tx, "0004_physical_mapping.sql", &mapping.0).await;
    exec_tx(
        &tx,
        "markers-target",
        &marker_sql(&[
            ("target-bootstrap/1", &bootstrap.1, "0000_bootstrap_roles.sql"),
            ("target-fence-receipts/1", &fence.1, "0001_target_fence_expand.sql"),
            ("target-fence-receipts/3", &preflight.1, "0003_target_segment_preflight_expand.sql"),
            ("physical-mapping/v1", &mapping.1, "0004_physical_mapping.sql"),
        ]),
    )
    .await;
    tx.commit().await.expect("commit target schema");
    let tx = begin_locked(client).await;
    exec_tx(&tx, "row-lock-import", "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE").await;
    exec_tx(&tx, "stats-grant", "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE").await;
    exec_tx(&tx, "set-owner-import", "SET LOCAL ROLE oo_target_owner").await;
    exec_tx(&tx, "0005_full_import_expand.sql", &full_import.0).await;
    exec_tx(
        &tx,
        "marker-import",
        &marker_sql(&[("full-import/v1", &full_import.1, "0005_full_import_expand.sql")]),
    )
    .await;
    tx.commit().await.expect("commit full import");
}

fn migration(name: &str) -> (String, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oo-pipeline/migrations")
        .join(name);
    let bytes = fs::read(&path).unwrap_or_else(|_| panic!("read {name}"));
    let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| panic!("{name} utf8"));
    (text, hex(&Sha256::digest(&bytes)))
}

fn marker_sql(rows: &[(&str, &str, &str)]) -> String {
    let mut values = String::new();
    for (index, (version, checksum, filename)) in rows.iter().enumerate() {
        assert_eq!(checksum.len(), 64);
        if index > 0 {
            values.push_str(",\n");
        }
        values.push_str(&format!("  ('{version}', '{checksum}', 'expand', '{filename}')"));
    }
    format!("INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values}")
}

async fn begin_locked(client: &mut Client) -> tokio_postgres::Transaction<'_> {
    let tx = client.transaction().await.expect("begin");
    exec_tx(&tx, "lock-timeout", "SET LOCAL lock_timeout = '15s'").await;
    exec_tx(&tx, "statement-timeout", "SET LOCAL statement_timeout = '60s'").await;
    exec_tx(&tx, "advisory-lock", &format!("SELECT pg_catalog.pg_advisory_xact_lock({ADVISORY_LOCK})")).await;
    tx
}

async fn create_login(client: &Client, password: &str) {
    assert!(password.bytes().all(|byte| byte.is_ascii_hexdigit()));
    exec_hidden(
        client,
        "create-login",
        &format!(
            "CREATE ROLE \"{LOGIN}\" LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOBYPASSRLS NOREPLICATION PASSWORD '{password}';\n\
             GRANT CONNECT ON DATABASE \"{DATABASE}\" TO \"{LOGIN}\""
        ),
    )
    .await;
}

async fn grant_mapping_read(client: &Client) {
    exec_hidden(
        client,
        "grant-mapping",
        &format!(
            "GRANT USAGE ON SCHEMA _oo TO \"{LOGIN}\";\n\
             GRANT SELECT ON TABLE _oo.physical_mappings TO \"{LOGIN}\";\n\
             GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO \"{LOGIN}\""
        ),
    )
    .await;
}

async fn prepare_entry(client: &mut Client) -> Shadow {
    let target: i64 = client
        .query_one("SELECT _oo.adopt_target_generation($1, ($2::text)::uuid)", &[&SHARD, &OWNER])
        .await
        .unwrap_or_else(|err| fail_db("adopt", &err))
        .get(0);
    client
        .execute(
            "SELECT _oo.full_import_bind_session($1, ($2::text)::uuid, $3)",
            &[&SHARD, &OWNER, &target],
        )
        .await
        .unwrap_or_else(|err| fail_db("bind", &err));
    let columns = ["entry_id", "qty", "memo"];
    register(client, "namespace", "n", NAMESPACE, "", "").await;
    register(client, "table", "t", NAMESPACE, TABLE, "").await;
    for column in columns {
        register(client, "column", "c", NAMESPACE, TABLE, column).await;
    }
    let table = lookup(client, "table", NAMESPACE, TABLE, "").await;
    let entry_id = lookup(client, "column", NAMESPACE, TABLE, "entry_id").await;
    let qty = lookup(client, "column", NAMESPACE, TABLE, "qty").await;
    let memo = lookup(client, "column", NAMESPACE, TABLE, "memo").await;
    let oids = vec![
        type_oid(client, "int8").await,
        type_oid(client, "int8").await,
        type_oid(client, "text").await,
    ];
    let schemas = vec![NAMESPACE.to_string()];
    let tables = vec![TABLE.to_string()];
    let physical = vec![table.clone()];
    let open_params: [&(dyn ToSql + Sync); 12] = [
        &SHARD, &OWNER, &target, &SOURCE, &GENERATION, &CONFIG, &TYPE_DIGEST, &INPUT_DIGEST,
        &SCHEMA_DIGEST, &schemas, &tables, &physical,
    ];
    let run_id: String = client
        .query_one(
            "SELECT _oo.full_import_open_run($1, ($2::text)::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
            &open_params,
        )
        .await
        .unwrap_or_else(|err| fail_db("open-run", &err))
        .get(0);
    let idents: Vec<String> = columns.into_iter().map(str::to_string).collect();
    let typmods = vec![-1_i32; 3];
    let required = vec![true, true, false];
    let index = 0_i32;
    let prepare_params: [&(dyn ToSql + Sync); 14] = [
        &SHARD, &OWNER, &target, &run_id, &index, &SOURCE, &CONFIG, &GENERATION, &NAMESPACE,
        &TABLE, &idents, &oids, &typmods, &required,
    ];
    let prepared: String = client
        .query_one(
            "SELECT _oo.full_import_prepare_shadow($1, ($2::text)::uuid, $3, ($4::text)::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
            &prepare_params,
        )
        .await
        .unwrap_or_else(|err| fail_db("prepare", &err))
        .get(0);
    assert_eq!(prepared, table);
    let shadow = Shadow { table, entry_id, qty, memo };
    insert_rows(client, &shadow).await;
    shadow
}

async fn type_oid(client: &Client, name: &str) -> i64 {
    let oid: i32 = client
        .query_one(
            "SELECT type_row.oid::int4 \
             FROM pg_catalog.pg_type AS type_row \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type_row.typnamespace \
             WHERE namespace.nspname = 'pg_catalog' AND type_row.typname = $1 AND type_row.typtype = 'b'",
            &[&name],
        )
        .await
        .unwrap_or_else(|err| fail_db("type-oid", &err))
        .get(0);
    i64::from(oid)
}

async fn register(client: &Client, level: &str, letter: &str, namespace: &str, table: &str, column: &str) {
    let payload: [&(dyn ToSql + Sync); 7] = [&SOURCE, &CONFIG, &GENERATION, &letter, &namespace, &table, &column];
    let digest: Vec<u8> = client
        .query_one("SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, $3, $4, $5, $6, $7))", &payload)
        .await
        .unwrap_or_else(|err| fail_db("digest", &err))
        .get(0);
    let bind: [&(dyn ToSql + Sync); 8] = [&SOURCE, &CONFIG, &GENERATION, &level, &namespace, &table, &column, &digest];
    client
        .execute("SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)", &bind)
        .await
        .unwrap_or_else(|err| fail_db("register", &err));
}

async fn lookup(client: &Client, level: &str, namespace: &str, table: &str, column: &str) -> String {
    let params: [&(dyn ToSql + Sync); 7] = [&SOURCE, &CONFIG, &GENERATION, &level, &namespace, &table, &column];
    client
        .query_one("SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)", &params)
        .await
        .unwrap_or_else(|err| fail_db("lookup", &err))
        .get(0)
}

async fn insert_rows(client: &mut Client, shadow: &Shadow) {
    let sql = format!(
        "INSERT INTO \"_oo\".{table} (\"_oo_pos\", {entry_id}, {qty}, {memo}) \
         VALUES (($1::text)::numeric, $2, $3, $4)",
        table = quote_physical(&shadow.table, "pt_"),
        entry_id = quote_physical(&shadow.entry_id, "pc_"),
        qty = quote_physical(&shadow.qty, "pc_"),
        memo = quote_physical(&shadow.memo, "pc_"),
    );
    let rows = [(0_i64, i64::MIN, "1"), (1, i64::MAX, "2"), (2, 0, "3"), (7, 11, "4")];
    let tx = client.transaction().await.expect("insert begin");
    for (entry_id, qty, pos) in rows {
        tx.execute(&sql, &[&pos, &entry_id, &qty, &"review"])
            .await
            .unwrap_or_else(|err| fail_db("insert", &err));
    }
    tx.commit().await.expect("insert commit");
}

fn quote_physical(name: &str, prefix: &str) -> String {
    let rest = name.strip_prefix(prefix).unwrap_or("");
    assert_eq!(name.len(), 63, "physical length");
    assert_eq!(rest.len(), 60, "physical body");
    assert!(rest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    format!("\"{name}\"")
}

async fn analyze_shadow(client: &Client, table: &str) {
    exec_hidden(client, "analyze", &format!("ANALYZE \"_oo\".{}", quote_physical(table, "pt_"))).await;
}

async fn fetch_stored(client: &Client, shadow: &Shadow) -> Vec<(i64, i64, String)> {
    let sql = format!(
        "SELECT {entry_id}, {qty}, {memo} FROM \"_oo\".{table} ORDER BY {entry_id}",
        table = quote_physical(&shadow.table, "pt_"),
        entry_id = quote_physical(&shadow.entry_id, "pc_"),
        qty = quote_physical(&shadow.qty, "pc_"),
        memo = quote_physical(&shadow.memo, "pc_"),
    );
    let rows = client.query(&sql, &[]).await.unwrap_or_else(|err| fail_db("stored", &err));
    rows.iter().map(|row| (row.get(0), row.get(1), row.get(2))).collect()
}

async fn admin_ints(client: &Client, sql: &str, width: usize) -> Vec<Vec<String>> {
    let rows = client.query(sql, &[]).await.unwrap_or_else(|err| fail_db("admin-read", &err));
    rows.iter()
        .map(|row| (0..width).map(|index| row.get::<_, i64>(index).to_string()).collect())
        .collect()
}

async fn exec_hidden(client: &Client, name: &str, sql: &str) {
    client.batch_execute(sql).await.unwrap_or_else(|err| fail_db(name, &err));
}

async fn exec_tx(tx: &tokio_postgres::Transaction<'_>, name: &str, sql: &str) {
    tx.batch_execute(sql).await.unwrap_or_else(|err| fail_db(name, &err));
}

fn fail_db(name: &str, err: &tokio_postgres::Error) -> ! {
    let state = err.code().map(|code| code.code().to_string()).unwrap_or_default();
    panic!("{name} sqlstate={state}");
}
