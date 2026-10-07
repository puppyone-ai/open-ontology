//! Bounded literal `IN` admission, plus one private `query-pg` fixture.
//!
//! The PostgreSQL fixture runs only when `OO_A343_LIVE=1`. Without that
//! variable these tests parse and refuse shapes, and they do not claim a live run.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use oo_core::api::ErrorCode;
use oo_gateway::policy::catalog::{CatalogLimits, UnknownRows, load_policy_catalog};
use oo_gateway::sql_guard::IN_LIST_LIMIT;
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
const EQ_CROSS: u32 = 401;
const EQ_SAME: u32 = 402;
const NE_CROSS: u32 = 403;
const NE_SAME: u32 = 404;
const EQ_TEXT: u32 = 405;
const EQ_BOOL: u32 = 407;

const QTY_SQL: &str = "SELECT entry_id, qty FROM shop.entry WHERE entry_id IN (0, 1, 2) ORDER BY entry_id";

fn identity() -> PolicyIdentity {
    PolicyIdentity {
        project: "shopfix".to_string(),
        role: "oo_r_wide".to_string(),
        policy_id: "policy-a343".to_string(),
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
        procedure_oid: oid + 100,
        schema: "pg_catalog".to_string(),
        name: name.to_string(),
        left_type_oid: left,
        right_type_oid: right,
    }
}

fn catalog(rows: u64, operators: Vec<ResolvedOperator>, integer: Option<u32>) -> PolicyCatalog {
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
            oid: 201,
            schema: "pg_catalog".to_string(),
            name: "count".to_string(),
            argument_type_oids: Vec::new(),
            aggregate_star: true,
        }],
        operators,
        literal_types: LiteralTypes {
            integer,
            text: Some(TEXT_OID),
            boolean: Some(BOOL_OID),
        },
        null_test_allowed: true,
        large_table_row_threshold: 100,
    }
}

fn operators() -> Vec<ResolvedOperator> {
    vec![
        operator(EQ_CROSS, "=", INT8_OID, INT4_OID),
        operator(EQ_SAME, "=", INT8_OID, INT8_OID),
        operator(NE_CROSS, "<>", INT8_OID, INT4_OID),
        operator(NE_SAME, "<>", INT8_OID, INT8_OID),
        operator(EQ_TEXT, "=", TEXT_OID, TEXT_OID),
        operator(406, "<>", TEXT_OID, TEXT_OID),
        operator(EQ_BOOL, "=", BOOL_OID, BOOL_OID),
        operator(408, "<>", BOOL_OID, BOOL_OID),
    ]
}

fn small() -> PolicyCatalog {
    catalog(10, operators(), Some(INT4_OID))
}

fn large() -> PolicyCatalog {
    catalog(2_000_000, operators(), Some(INT4_OID))
}

fn admit(catalog: &PolicyCatalog, sql: &str) -> GuardedPlan {
    guard_select(&catalog.identity, catalog, sql).unwrap_or_else(|err| panic!("{sql}\n{err}\n{err:?}"))
}

fn refuse(catalog: &PolicyCatalog, sql: &str) -> GuardError {
    guard_select(&catalog.identity, catalog, sql).expect_err(sql)
}

fn construct(err: &GuardError) -> &'static str {
    match err {
        GuardError::Unsupported { construct } => construct,
        other => panic!("expected unsupported, got {other}"),
    }
}

fn digest(text: &str) -> String {
    let bytes = Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        hex.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    hex
}

fn assert_kept_in(plan: &GuardedPlan, marker: &str) {
    assert_eq!(plan.original_sha256, digest(&plan.original_sql));
    assert_eq!(plan.executed_sha256, digest(&plan.executed_sql));
    assert!(plan.executed_sql.contains(marker), "{}", plan.executed_sql);
    assert!(!plan.executed_sql.contains(" OR "), "{}", plan.executed_sql);
    assert!(!plan.executed_sql.contains(" AND "), "{}", plan.executed_sql);
    assert!(plan.executed_sql.contains("pt_entry"), "{}", plan.executed_sql);
    assert!(plan.executed_sql.contains("pc_entry"), "{}", plan.executed_sql);
    assert!(!plan.executed_sql.contains("shop"), "{}", plan.executed_sql);
    assert!(
        !plan.executed_sql.contains("entry_id IN") && !plan.executed_sql.contains(".entry_id"),
        "{}",
        plan.executed_sql
    );
}

#[test]
fn entry_id_in_three_literals_keeps_exact_values() {
    let plan = admit(&small(), QTY_SQL);
    assert_eq!(plan.original_sql, QTY_SQL);
    assert_kept_in(&plan, "IN (0, 1, 2)");
    assert_eq!(plan.bindings.operators.len(), 3);
    assert!(plan.bindings.operators.iter().all(|op| op.oid == EQ_CROSS && op.name == "="));
    assert_eq!(plan.bindings.relations[0].physical_schema, "_oo");
    assert_eq!(plan.bindings.relations[0].physical_name, "pt_entry");
    let negative = admit(&small(), "SELECT entry_id FROM shop.entry WHERE entry_id IN (-1)");
    assert_kept_in(&negative, "IN (-1)");
    assert_eq!(negative.bindings.operators.len(), 1);
    assert_eq!(negative.bindings.operators[0].oid, EQ_CROSS);
    match &plan.bindings.outputs[0] {
        OutputItem::Column(column) => assert_eq!(column.physical_name, "pc_entry"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn tight_in_spacing_deparses_the_same_integers() {
    let sql = "SELECT entry_id FROM shop.entry WHERE entry_id IN(0,1,2)";
    let plan = admit(&small(), sql);
    assert_eq!(plan.original_sql, sql);
    assert_kept_in(&plan, "IN (0, 1, 2)");
}

#[test]
fn null_and_not_in_stay_in_the_tree() {
    let with_null = admit(
        &small(),
        "SELECT entry_id FROM shop.entry WHERE entry_id IN (0, NULL, 2)",
    );
    assert_kept_in(&with_null, "IN (0, NULL, 2)");
    assert_eq!(
        with_null
            .bindings
            .operators
            .iter()
            .map(|op| op.oid)
            .collect::<Vec<_>>(),
        vec![EQ_CROSS, EQ_SAME, EQ_CROSS]
    );

    let not_in = admit(
        &small(),
        "SELECT entry_id FROM shop.entry WHERE entry_id NOT IN (0, NULL)",
    );
    assert_kept_in(&not_in, "NOT IN (0, NULL)");
    assert_eq!(
        not_in.bindings.operators.iter().map(|op| op.oid).collect::<Vec<_>>(),
        vec![NE_CROSS, NE_SAME]
    );

    let plain = admit(
        &small(),
        "SELECT entry_id FROM shop.entry WHERE entry_id NOT IN (0, 1)",
    );
    assert_kept_in(&plain, "NOT IN (0, 1)");
    assert!(plain.bindings.operators.iter().all(|op| op.oid == NE_CROSS));
}

#[test]
fn text_and_bool_lists_use_their_literal_types() {
    let text = admit(&small(), "SELECT label FROM shop.entry WHERE label IN ('a', 'b')");
    assert!(text.executed_sql.contains("IN ('a', 'b')"), "{}", text.executed_sql);
    assert!(text.bindings.operators.iter().all(|op| op.oid == EQ_TEXT));
    assert!(!text.executed_sql.contains(" OR "), "{}", text.executed_sql);

    let boolean = admit(
        &small(),
        "SELECT active FROM shop.entry WHERE active IN (true, false)",
    );
    assert!(
        boolean.executed_sql.contains("IN (true, false)"),
        "{}",
        boolean.executed_sql
    );
    assert!(boolean.bindings.operators.iter().all(|op| op.oid == EQ_BOOL));
}

#[test]
fn list_length_stops_at_the_explicit_bound() {
    assert_eq!(IN_LIST_LIMIT, 32);
    let at_bound = (0..IN_LIST_LIMIT)
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let plan = admit(
        &small(),
        &format!("SELECT entry_id FROM shop.entry WHERE entry_id IN ({at_bound})"),
    );
    assert_eq!(plan.bindings.operators.len(), IN_LIST_LIMIT);
    assert!(plan.executed_sql.contains("IN (0, 1, 2, "));
    let over = format!("{at_bound}, {IN_LIST_LIMIT}");
    assert_eq!(
        construct(&refuse(
            &small(),
            &format!("SELECT entry_id FROM shop.entry WHERE entry_id IN ({over})")
        )),
        "IN predicate"
    );
}

#[test]
fn unsupported_in_shapes_fail_closed() {
    let cases = [
        ("SELECT entry_id FROM shop.entry WHERE entry_id IN ()", "parse"),
        ("SELECT entry_id FROM shop.entry WHERE entry_id IN (-qty)", "unary operator"),
        ("SELECT entry_id FROM shop.entry WHERE entry_id IN (1.5)", "float literal"),
        ("SELECT entry_id FROM shop.entry WHERE entry_id IN (qty)", "IN predicate"),
        ("SELECT entry_id FROM shop.entry WHERE entry_id IN (memo)", "IN predicate"),
        (
            "SELECT entry_id FROM shop.entry WHERE entry_id IN (SELECT entry_id FROM shop.entry)",
            "subquery",
        ),
        (
            "SELECT entry_id FROM shop.entry WHERE entry_id NOT IN (SELECT entry_id FROM shop.entry)",
            "subquery",
        ),
        (
            "SELECT entry_id FROM shop.entry WHERE entry_id = ANY (ARRAY[0, 1, 2])",
            "ANY predicate",
        ),
        ("SELECT entry_id FROM shop.entry WHERE entry_id IN (abs(0))", "function"),
        ("SELECT entry_id FROM shop.entry WHERE entry_id IN (0::int8)", "cast"),
        ("SELECT active FROM shop.entry WHERE active IN ('t'::bool)", "cast"),
        ("SELECT entry_id FROM shop.entry WHERE entry_id IN ($1)", "parameter"),
        ("SELECT entry_id FROM shop.entry WHERE (entry_id, qty) IN ((0, 1))", "IN predicate"),
        ("SELECT label FROM shop.entry WHERE label LIKE 'a'", "LIKE predicate"),
        ("SELECT entry_id FROM shop.entry WHERE entry_id BETWEEN 0 AND 2", "BETWEEN predicate"),
    ];
    for (sql, expect) in cases {
        let err = refuse(&small(), sql);
        if expect == "parse" {
            assert!(matches!(err, GuardError::Parse), "{sql} {err:?}");
            assert_eq!(err.code(), ErrorCode::SqlParseError);
        } else {
            assert_eq!(construct(&err), expect, "{sql}");
        }
        assert!(!err.to_string().contains("memo"), "{sql}");
        assert!(!format!("{err:?}").contains("memo"), "{sql}");
    }
}

#[test]
fn hidden_column_is_not_granted_by_in() {
    let err = refuse(&small(), "SELECT memo FROM shop.entry WHERE memo IN (1)");
    match err {
        GuardError::ColumnNotAllowed { pending, ambiguous, .. } => {
            assert_eq!(pending, 1);
            assert!(!ambiguous);
        }
        other => panic!("{other:?}"),
    }
    let text = err_text("SELECT entry_id FROM shop.entry WHERE memo IN (0, 1)");
    assert!(!text.contains("memo"));
    assert!(text.contains("pending classification"));
}

fn err_text(sql: &str) -> String {
    let err = refuse(&small(), sql);
    assert!(matches!(err, GuardError::ColumnNotAllowed { .. }), "{err:?}");
    let text = err.to_string();
    assert!(!format!("{err:?}").contains("memo"));
    text
}

#[test]
fn missing_operator_and_literal_type_refuse() {
    let mut ops = operators();
    ops.retain(|op| op.oid != NE_CROSS && op.oid != NE_SAME);
    let no_not = catalog(10, ops, Some(INT4_OID));
    assert!(matches!(
        refuse(&no_not, "SELECT entry_id FROM shop.entry WHERE entry_id NOT IN (0, 1)"),
        GuardError::FunctionNotAllowed
    ));

    let mut ops = operators();
    ops.retain(|op| op.oid != EQ_SAME);
    let no_same = catalog(10, ops, Some(INT4_OID));
    assert!(matches!(
        refuse(&no_same, "SELECT entry_id FROM shop.entry WHERE entry_id IN (NULL)"),
        GuardError::FunctionNotAllowed
    ));
    assert!(admit(&no_same, "SELECT entry_id FROM shop.entry WHERE entry_id IN (0)").executed_sql.contains("IN (0)"));

    let unresolved = catalog(10, operators(), None);
    assert!(matches!(
        refuse(&unresolved, "SELECT entry_id FROM shop.entry WHERE entry_id IN (0, 1)"),
        GuardError::FunctionNotAllowed
    ));
    assert!(matches!(
        refuse(&small(), "SELECT entry_id FROM shop.entry WHERE entry_id IN ('no')"),
        GuardError::FunctionNotAllowed
    ));
}

#[test]
fn existing_guards_still_apply() {
    assert!(matches!(
        refuse(&small(), "INSERT INTO shop.entry (entry_id, qty) VALUES (1, 1)"),
        GuardError::Write
    ));
    assert_eq!(
        construct(&refuse(&small(), "SELECT entry_id FROM shop.entry WHERE entry_id = $1")),
        "parameter"
    );
    let compared = admit(&small(), "SELECT qty FROM shop.entry WHERE entry_id = 0");
    assert_eq!(compared.bindings.operators.len(), 1);
    assert_eq!(compared.bindings.operators[0].oid, EQ_CROSS);
    assert!(!compared.executed_sql.contains(" IN "), "{}", compared.executed_sql);

    let counted = admit(&small(), "SELECT count(*) FROM shop.entry");
    assert!(matches!(counted.bindings.outputs[0], OutputItem::Function(_)));

    match refuse(&large(), "SELECT * FROM shop.entry") {
        GuardError::CostTooHigh { relation } => {
            assert_eq!(relation, "shop.entry");
            assert!(!relation.contains("2000000"));
        }
        other => panic!("{other:?}"),
    }
    let filtered = admit(&large(), "SELECT * FROM shop.entry WHERE entry_id IN (0, 1, 2)");
    assert!(filtered.executed_sql.contains("IN (0, 1, 2)"), "{}", filtered.executed_sql);
    assert!(!filtered.executed_sql.contains('*'), "{}", filtered.executed_sql);
    assert!(matches!(
        refuse(&large(), "SELECT * FROM shop.entry WHERE true OR entry_id IN (0)"),
        GuardError::CostTooHigh { .. }
    ));
    assert!(matches!(
        refuse(
            &large(),
            "SELECT * FROM shop.entry JOIN shop.entry other ON entry.entry_id IN (0)"
        ),
        GuardError::Unsupported { construct: "duplicate range name" } | GuardError::CostTooHigh { .. }
    ));

    let mut long = String::from("SELECT entry_id FROM shop.entry WHERE entry_id IN (0)");
    long.push_str(&" ".repeat(oo_gateway::SQL_BYTE_BUDGET));
    assert!(matches!(refuse(&small(), &long), GuardError::Budget));
}

#[test]
fn alias_and_qualified_column_rewrite_inside_in() {
    let alias = admit(
        &small(),
        "SELECT e.entry_id FROM shop.entry AS e WHERE e.entry_id IN (0, 1, 2)",
    );
    assert_kept_in(&alias, "IN (0, 1, 2)");
    assert!(alias.executed_sql.contains('e'), "{}", alias.executed_sql);

    let qualified = admit(
        &small(),
        "SELECT entry_id FROM shop.entry WHERE shop.entry.entry_id IN (0, 1, 2)",
    );
    assert_kept_in(&qualified, "IN (0, 1, 2)");
}

const HOST: &str = "oo-a343-pg";
const DATABASE: &str = "oo_a343";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a343_login";
const SECRET_DIR: &str = "/run/oo-a343/fixture";
const OWNER: &str = "a3430000-0000-4000-8000-000000000343";
const SHARD: &str = "a343main";
const SOURCE: &str = "shopfix";
const CONFIG: &str = "a343a343a343a343a343a343a343a343a343a343a343a343a343a343a343a343";
const TYPE_DIGEST: &str = "b343b343b343b343b343b343b343b343b343b343b343b343b343b343b343b343";
const INPUT_DIGEST: &str = "c343c343c343c343c343c343c343c343c343c343c343c343c343c343c343c343";
const SCHEMA_DIGEST: &str = "d343d343d343d343d343d343d343d343d343d343d343d343d343d343d343d343";
const NAMESPACE: &str = "shop";
const TABLE: &str = "entry";
const GENERATION: i64 = 343;
const ADVISORY_LOCK: i64 = 64_042_026_343;
const MIN_QTY: &str = "-9223372036854775808";
const MAX_QTY: &str = "9223372036854775807";

static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(1);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_query_pg_executes_bounded_in() {
    if env::var("OO_A343_LIVE").ok().as_deref() != Some("1") {
        eprintln!("OO_A343_LIVE is unset; the private postgres comparison was not run");
        return;
    }
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
    assert_eq!(
        int_rows(&qty_turn.rows),
        vec![
            vec!["0".to_string(), MIN_QTY.to_string()],
            vec!["1".to_string(), MAX_QTY.to_string()],
            vec!["2".to_string(), "0".to_string()],
        ]
    );
    let plan = assert_plan_hashes(&qty_turn.summary, QTY_SQL, &loaded);
    assert!(plan.executed_sql.contains("IN (0, 1, 2)"), "{}", plan.executed_sql);
    assert!(!plan.executed_sql.contains(" OR "), "{}", plan.executed_sql);
    assert_eq!(admin_ints(&admin, &plan.executed_sql, 2).await, int_rows(&qty_turn.rows));
    assert_audit(&qty.audit, QTY_SQL, &loaded, &bans);

    let with_null = "SELECT entry_id FROM shop.entry WHERE entry_id IN (0, NULL, 2) ORDER BY entry_id";
    let null_run = run_query(&work, &password_file, &ca_file, with_null, 10, &bans);
    assert_eq!(null_run.code, 0);
    let null_turn = one_turn(&null_run.lines);
    assert_eq!(int_rows(&null_turn.rows), vec![vec!["0".to_string()], vec!["2".to_string()]]);
    let null_plan = assert_plan_hashes(&null_turn.summary, with_null, &loaded);
    assert!(null_plan.executed_sql.contains("IN (0, NULL, 2)"), "{}", null_plan.executed_sql);
    assert!(!null_plan.executed_sql.contains(" OR "));
    assert_eq!(admin_ints(&admin, &null_plan.executed_sql, 1).await, int_rows(&null_turn.rows));

    let not_null = "SELECT entry_id FROM shop.entry WHERE entry_id NOT IN (0, NULL) ORDER BY entry_id";
    let not_null_run = run_query(&work, &password_file, &ca_file, not_null, 10, &bans);
    assert_eq!(not_null_run.code, 0);
    let not_null_turn = one_turn(&not_null_run.lines);
    assert!(not_null_turn.rows.is_empty());
    let not_null_plan = assert_plan_hashes(&not_null_turn.summary, not_null, &loaded);
    assert!(not_null_plan.executed_sql.contains("NOT IN (0, NULL)"), "{}", not_null_plan.executed_sql);
    assert!(admin_ints(&admin, &not_null_plan.executed_sql, 1).await.is_empty());

    let not_in = "SELECT entry_id FROM shop.entry WHERE entry_id NOT IN (0, 1) ORDER BY entry_id";
    let not_in_run = run_query(&work, &password_file, &ca_file, not_in, 10, &bans);
    assert_eq!(not_in_run.code, 0);
    let not_in_turn = one_turn(&not_in_run.lines);
    assert_eq!(int_rows(&not_in_turn.rows), vec![vec!["2".to_string()], vec!["4".to_string()]]);
    let not_in_plan = assert_plan_hashes(&not_in_turn.summary, not_in, &loaded);
    assert!(not_in_plan.executed_sql.contains("NOT IN (0, 1)"));
    assert!(!not_in_plan.executed_sql.contains(" AND "));

    let equal = "SELECT entry_id, qty FROM shop.entry WHERE entry_id = 0";
    let equal_run = run_query(&work, &password_file, &ca_file, equal, 5, &bans);
    assert_eq!(equal_run.code, 0);
    assert_eq!(
        int_rows(&one_turn(&equal_run.lines).rows),
        vec![vec!["0".to_string(), MIN_QTY.to_string()]]
    );

    for sql in [
        "SELECT memo FROM shop.entry",
        "SELECT entry_id FROM shop.entry WHERE memo IN ('x')",
        "INSERT INTO shop.entry (entry_id, qty) VALUES (9, 9)",
        "SELECT entry_id FROM shop.entry WHERE entry_id IN ($1)",
        "SELECT entry_id FROM shop.entry WHERE entry_id = ANY (ARRAY[0, 1, 2])",
        "SELECT entry_id FROM shop.entry WHERE entry_id IN (0::int8)",
        "SELECT entry_id FROM shop.entry WHERE entry_id IN (abs(0))",
    ] {
        let run = run_query(&work, &password_file, &ca_file, sql, 5, &bans);
        assert_eq!(run.code, 6, "{sql}");
        assert_eq!(run.stderr, b"query-pg: guard\n");
        assert!(run.stdout.is_empty(), "{sql}");
        assert!(!run.audit.exists(), "{sql}");
    }

    let after = fetch_stored(&admin, &shadow).await;
    assert_eq!(before, after);
    write_oracle(&[
        "qty_rows=3",
        "null_in_rows=2",
        "not_in_null_rows=0",
        "not_in_rows=2",
        "guard_refusals=7",
        "rows_unchanged=yes",
    ]);
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
    let audit = dir.join(format!("audit-{}.jsonl", SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed)));
    let request = dir.join(format!("request-{}.json", SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed)));
    let columns = ["entry_id", "qty"]
        .iter()
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
            "table": {
                "namespace": NAMESPACE,
                "name": TABLE,
                "columns": columns
            }
        }],
        "catalog": {
            "max_tables": 4,
            "max_columns": 64,
            "max_functions": 1,
            "max_operators": 64,
            "large_table_row_threshold": 1_000_000,
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
    assert_quiet(&bytes, bans, "request");
    fs::write(&request, &bytes).unwrap();
    let output = run_raw(&["--request", request.to_str().unwrap()]);
    assert_quiet(&output.stdout, bans, "stdout");
    assert_quiet(&output.stderr, bans, "stderr");
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
    if let Some(path) = option_env!("CARGO_BIN_EXE_query_pg") {
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
        "oo-a343-bounded-in-{}-{}",
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
    plan
}

fn assert_audit(path: &Path, sql: &str, catalog: &PolicyCatalog, bans: &[String]) {
    let raw = fs::read(path).expect("audit");
    assert_quiet(&raw, bans, "audit");
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

fn assert_quiet(bytes: &[u8], bans: &[String], label: &str) {
    let text = String::from_utf8_lossy(bytes);
    for ban in bans {
        assert!(!text.contains(ban), "{label} contained a secret");
    }
}

fn write_oracle(lines: &[&str]) {
    let path = Path::new("/evidence/live/oracle.txt");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("oracle dir");
    }
    fs::write(path, lines.join("\n") + "\n").expect("oracle");
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
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a343_login') THEN\n\
             DROP ROLE oo_a343_login;\n\
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
    (text, sha256_hex(&bytes))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        hex.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    hex
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
        &SHARD,
        &OWNER,
        &target,
        &SOURCE,
        &GENERATION,
        &CONFIG,
        &TYPE_DIGEST,
        &INPUT_DIGEST,
        &SCHEMA_DIGEST,
        &schemas,
        &tables,
        &physical,
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
        &SHARD,
        &OWNER,
        &target,
        &run_id,
        &index,
        &SOURCE,
        &CONFIG,
        &GENERATION,
        &NAMESPACE,
        &TABLE,
        &idents,
        &oids,
        &typmods,
        &required,
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
    let rows = [
        (0_i64, i64::MIN, "1"),
        (1, i64::MAX, "2"),
        (2, 0, "3"),
        (4, 4, "4"),
    ];
    let tx = client.transaction().await.expect("insert begin");
    for (entry_id, qty, pos) in rows {
        tx.execute(&sql, &[&pos, &entry_id, &qty, &"stored"])
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
    rows.iter()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
        .collect()
}

async fn admin_ints(client: &Client, sql: &str, width: usize) -> Vec<Vec<String>> {
    let rows = client.query(sql, &[]).await.unwrap_or_else(|err| fail_db("admin-read", &err));
    rows.iter()
        .map(|row| {
            (0..width)
                .map(|index| row.get::<_, i64>(index).to_string())
                .collect()
        })
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
