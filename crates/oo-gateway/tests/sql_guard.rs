//! SQL admission parsed by pg_query.
//!
//! The PostgreSQL comparison runs only when `OO_A175_LIVE=1`. Without that
//! variable this file still parses fixtures, and it does not claim a live run.

use std::fs;
use std::path::Path;

use oo_core::api::ErrorCode;
use oo_gateway::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, GuardError, GuardedPlan, InstallEffect, LiteralKind,
    LiteralTypes, LogicalTable, OutputItem, PINNED_PARSER_VERSION, PiiClass, PolicyCatalog,
    PolicyIdentity, ResolvedFunction, ResolvedOperator, SQL_BYTE_BUDGET, TlsEndpoint,
    VisibleColumn, VisibleRelation, connect_require_tls, guard_select, install_column_acl,
};
use sha2::{Digest, Sha256};
use tokio_postgres::Client;
use tokio_postgres::types::ToSql;

const INT_OID: u32 = 11;
const TEXT_OID: u32 = 13;
const BOOL_OID: u32 = 17;

fn identity() -> PolicyIdentity {
    PolicyIdentity {
        project: "orders".to_string(),
        role: "oo_r_dispatcher".to_string(),
        policy_id: "policy-a175".to_string(),
    }
}

fn col(name: &str, physical: &str, type_oid: u32) -> VisibleColumn {
    VisibleColumn {
        name: name.to_string(),
        physical_name: physical.to_string(),
        type_oid,
    }
}

fn relation(
    schema: &str,
    name: &str,
    physical: &str,
    columns: Vec<VisibleColumn>,
    pending: u32,
    rows: u64,
) -> VisibleRelation {
    VisibleRelation {
        schema: schema.to_string(),
        name: name.to_string(),
        physical_schema: "_oo".to_string(),
        physical_name: physical.to_string(),
        columns,
        pending_unclassified: pending,
        estimated_rows: rows,
    }
}

fn shipment(rows: u64) -> VisibleRelation {
    relation(
        "logistics",
        "shipment",
        "pt_ship",
        vec![
            col("order_id", "pc_order", INT_OID),
            col("region", "pc_region", TEXT_OID),
        ],
        1,
        rows,
    )
}

fn customer(rows: u64) -> VisibleRelation {
    relation(
        "logistics",
        "customer",
        "pt_cust",
        vec![
            col("customer_id", "pc_cust", INT_OID),
            col("region", "pc_cregion", TEXT_OID),
        ],
        0,
        rows,
    )
}

fn catalog(relations: Vec<VisibleRelation>) -> PolicyCatalog {
    PolicyCatalog {
        identity: identity(),
        relations,
        functions: vec![ResolvedFunction {
            oid: 201,
            schema: "pg_catalog".to_string(),
            name: "count".to_string(),
            argument_type_oids: Vec::new(),
            aggregate_star: true,
        }],
        operators: vec![
            ResolvedOperator {
                oid: 301,
                procedure_oid: 302,
                schema: "pg_catalog".to_string(),
                name: "=".to_string(),
                left_type_oid: INT_OID,
                right_type_oid: INT_OID,
            },
            ResolvedOperator {
                oid: 303,
                procedure_oid: 304,
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
        large_table_row_threshold: 100,
    }
}

fn small() -> PolicyCatalog {
    catalog(vec![shipment(10)])
}

fn large() -> PolicyCatalog {
    catalog(vec![shipment(424_242)])
}

fn joined() -> PolicyCatalog {
    catalog(vec![customer(10), shipment(424_242)])
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

fn admit(catalog: &PolicyCatalog, sql: &str) -> GuardedPlan {
    guard_select(&catalog.identity, catalog, sql)
        .unwrap_or_else(|err| panic!("{sql}\n{err}\n{err:?}"))
}

fn refuse(catalog: &PolicyCatalog, sql: &str) -> GuardError {
    guard_select(&catalog.identity, catalog, sql).expect_err(sql)
}

fn assert_silent(err: &GuardError, tokens: &[&str]) {
    let display = err.to_string();
    let debug = format!("{err:?}");
    for token in tokens {
        assert!(
            !display.contains(token),
            "display leaked {token}: {display}"
        );
        assert!(!debug.contains(token), "debug leaked {token}: {debug}");
    }
}

fn assert_physical(plan: &GuardedPlan, sql: &str) {
    assert!(
        plan.executed_sql.contains("_oo"),
        "{sql}\n{}",
        plan.executed_sql
    );
    assert!(
        plan.executed_sql.contains("pt_ship"),
        "{sql}\n{}",
        plan.executed_sql
    );
    assert!(
        !plan.executed_sql.contains("secret"),
        "{sql}\n{}",
        plan.executed_sql
    );
}

#[test]
fn integer_literal_uses_the_numeric_operator() {
    const NUM_OID: u32 = 1700;
    const NUM_GT: u32 = 901;
    let mut relations = vec![shipment(10)];
    relations[0]
        .columns
        .push(col("amount", "pc_amount", NUM_OID));
    let mut catalog = catalog(relations);
    catalog.operators.push(ResolvedOperator {
        oid: 907,
        procedure_oid: 908,
        schema: "pg_catalog".to_string(),
        name: "=".to_string(),
        left_type_oid: 1082,
        right_type_oid: 1082,
    });
    catalog.operators.push(ResolvedOperator {
        oid: 905,
        procedure_oid: 906,
        schema: "pg_catalog".to_string(),
        name: "<".to_string(),
        left_type_oid: NUM_OID,
        right_type_oid: NUM_OID,
    });
    catalog.operators.push(ResolvedOperator {
        oid: NUM_GT,
        procedure_oid: 902,
        schema: "pg_catalog".to_string(),
        name: ">".to_string(),
        left_type_oid: NUM_OID,
        right_type_oid: NUM_OID,
    });
    let plan = guard_select(
        &identity(),
        &catalog,
        "SELECT order_id FROM logistics.shipment WHERE amount > 10",
    )
    .expect("numeric comparison");
    assert!(plan.bindings.operators.iter().any(|op| op.oid == NUM_GT));
    assert!(plan.executed_sql.contains("pc_amount"));
    assert!(plan.executed_sql.contains('>'));
    for comparison in ["amount > -10", "amount > 0", "amount < 0", "0 < amount"] {
        admit(&catalog, &format!("SELECT order_id FROM logistics.shipment WHERE {comparison}"));
    }
    let floated = guard_select(
        &identity(),
        &catalog,
        "SELECT order_id FROM logistics.shipment WHERE amount > 10.5",
    );
    assert!(matches!(
        floated,
        Err(GuardError::Unsupported {
            construct: "float literal"
        })
    ));
    let int_column = guard_select(
        &identity(),
        &catalog,
        "SELECT order_id FROM logistics.shipment WHERE order_id > 10",
    );
    assert!(matches!(int_column, Err(GuardError::FunctionNotAllowed)));
    catalog.operators.pop();
    let denied = guard_select(
        &identity(),
        &catalog,
        "SELECT order_id FROM logistics.shipment WHERE amount > 10",
    );
    assert!(matches!(denied, Err(GuardError::FunctionNotAllowed)));
}

#[test]
fn comment_select_rewrites_through_the_parser() {
    let sql = "SELECT/*x*/ order_id, region FROM logistics.shipment";
    let plan = admit(&small(), sql);
    assert_eq!(plan.original_sql, sql);
    assert_ne!(plan.executed_sql, sql);
    assert_eq!(plan.original_sha256, digest(sql));
    assert_eq!(plan.executed_sha256, digest(&plan.executed_sql));
    assert_eq!(plan.parser_version, PINNED_PARSER_VERSION);
    assert!(!plan.fingerprint_hex.is_empty());
    assert!(
        plan.fingerprint_hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    );
    assert_eq!(plan.identity, identity());
    assert!(plan.star_expansions.is_empty());
    assert_eq!(plan.bindings.relations.len(), 1);
    assert!(plan.bindings.relations[0].alias.is_none());
    assert_eq!(plan.bindings.relations[0].physical_schema, "_oo");
    assert_eq!(plan.bindings.relations[0].physical_name, "pt_ship");
    match &plan.bindings.outputs[0] {
        OutputItem::Column(column) => {
            assert_eq!(column.name, "order_id");
            assert_eq!(column.physical_name, "pc_order");
            assert_eq!(column.output_name, "order_id");
            assert_eq!(column.type_oid, INT_OID);
        }
        other => panic!("{other:?}"),
    }
    assert_physical(&plan, sql);
    assert!(
        plan.executed_sql.contains("pc_order"),
        "{}",
        plan.executed_sql
    );
    assert!(
        plan.executed_sql.contains("pc_region"),
        "{}",
        plan.executed_sql
    );
}

#[test]
fn quoted_native_column_keeps_its_case_through_physical_recheck() {
    let catalog = catalog(vec![relation(
        "oo_w141",
        "shipments",
        "pt_ship",
        vec![col("whCode", "pc_whcode", TEXT_OID)],
        0,
        14,
    )]);
    let plan = admit(&catalog, "SELECT \"whCode\" FROM oo_w141.shipments LIMIT 1");
    match &plan.bindings.outputs[0] {
        OutputItem::Column(column) => {
            assert_eq!(column.name, "whCode");
            assert_eq!(column.output_name, "whCode");
            assert_eq!(column.physical_name, "pc_whcode");
        }
        other => panic!("{other:?}"),
    }
    assert!(plan.executed_sql.contains("pc_whcode"));
    assert!(matches!(
        refuse(&catalog, "SELECT whCode FROM oo_w141.shipments LIMIT 1"),
        GuardError::ColumnNotAllowed { .. }
    ));
}

#[test]
fn varchar_equality_uses_only_the_catalogued_text_operator() {
    let catalog = catalog(vec![relation(
        "oo_w141",
        "shipments",
        "pt_ship",
        vec![col("code", "pc_code", 1043)],
        0,
        14,
    )]);
    let plan = admit(&catalog, "SELECT COUNT(*) FROM oo_w141.shipments WHERE code = 'S41-001-U'");
    assert_eq!(plan.bindings.operators.len(), 1);
    assert_eq!(plan.bindings.operators[0].name, "=");
    assert!(matches!(
        refuse(&catalog, "SELECT COUNT(*) FROM oo_w141.shipments WHERE code = 1"),
        GuardError::FunctionNotAllowed
    ));
    assert!(matches!(
        refuse(&catalog, "SELECT COUNT(*) FROM oo_w141.shipments WHERE code::text = 'S41-001-U'"),
        GuardError::Unsupported { construct: "cast" }
    ));
    let function = refuse(&catalog, "SELECT COUNT(*) FROM oo_w141.shipments WHERE lower(code) = 's41-001-u'");
    assert!(matches!(function, GuardError::Unsupported { .. } | GuardError::FunctionNotAllowed), "{function:?}");
}

#[test]
fn date_literal_binds_only_the_catalogued_date_comparison() {
    const DATE_OID: u32 = 1082;
    const DATE_EQ: u32 = 701;
    const DATE_EQ_PROC: u32 = 702;
    const DATE_LT: u32 = 703;
    const DATE_LT_PROC: u32 = 704;
    let mut relations = vec![shipment(10)];
    relations[0]
        .columns
        .push(col("bucket", "pc_bucket", DATE_OID));
    let mut catalog = catalog(relations);
    catalog.operators.push(ResolvedOperator {
        oid: DATE_EQ,
        procedure_oid: DATE_EQ_PROC,
        schema: "pg_catalog".to_string(),
        name: "=".to_string(),
        left_type_oid: DATE_OID,
        right_type_oid: DATE_OID,
    });
    catalog.operators.push(ResolvedOperator {
        oid: DATE_LT,
        procedure_oid: DATE_LT_PROC,
        schema: "pg_catalog".to_string(),
        name: "<".to_string(),
        left_type_oid: DATE_OID,
        right_type_oid: DATE_OID,
    });
    catalog.operators.push(ResolvedOperator {
        oid: 1701,
        procedure_oid: 1702,
        schema: "pg_catalog".to_string(),
        name: "=".to_string(),
        left_type_oid: 1700,
        right_type_oid: 1700,
    });
    let plan = admit(
        &catalog,
        "SELECT COUNT(*) FROM logistics.shipment WHERE bucket = DATE '2026-05-06'",
    );
    assert_eq!(plan.bindings.operators.len(), 1);
    assert_eq!(plan.bindings.operators[0].oid, DATE_EQ);
    assert_eq!(plan.bindings.operators[0].procedure_oid, DATE_EQ_PROC);
    assert_ne!(DATE_EQ, DATE_EQ_PROC);
    assert!(plan.executed_sql.contains("pc_bucket"), "{}", plan.executed_sql);
    assert!(plan.executed_sql.contains("2026-05-06"), "{}", plan.executed_sql);
    assert!(!plan.executed_sql.contains('$'), "{}", plan.executed_sql);
    let earlier = admit(
        &catalog,
        "SELECT COUNT(*) FROM logistics.shipment WHERE bucket < DATE '2024-02-29'",
    );
    assert_eq!(earlier.bindings.operators[0].oid, DATE_LT);
    assert_eq!(earlier.bindings.operators[0].procedure_oid, DATE_LT_PROC);
    let cast_form = admit(
        &catalog,
        "SELECT COUNT(*) FROM logistics.shipment WHERE bucket = '2026-05-01'::pg_catalog.date",
    );
    assert_eq!(cast_form.bindings.operators[0].oid, DATE_EQ);
    assert_eq!(
        admit(&catalog, "SELECT DATE '2026-05-01'").bindings.outputs,
        vec![OutputItem::Literal(LiteralKind::Date)]
    );
    for sql in [
        "SELECT COUNT(*) FROM logistics.shipment WHERE bucket = DATE '2026-02-31'",
        "SELECT COUNT(*) FROM logistics.shipment WHERE bucket = DATE '2026-02-29'",
        "SELECT COUNT(*) FROM logistics.shipment WHERE bucket = DATE '0000-01-01'",
        "SELECT COUNT(*) FROM logistics.shipment WHERE bucket = DATE '2026-05-06 00:00:00'",
        "SELECT COUNT(*) FROM logistics.shipment WHERE bucket = '2026-05-06'::timestamp",
        "SELECT COUNT(*) FROM logistics.shipment WHERE bucket::date = DATE '2026-05-06'",
        "SELECT COUNT(*) FROM logistics.shipment WHERE CAST(lower(bucket) AS date) = DATE '2026-05-06'",
    ] {
        assert_eq!(
            refuse(&catalog, sql),
            GuardError::Unsupported { construct: "cast" },
            "{sql}"
        );
    }
    assert!(matches!(
        refuse(
            &catalog,
            "SELECT COUNT(*) FROM logistics.shipment WHERE bucket = '2026-05-06'"
        ),
        GuardError::FunctionNotAllowed
    ));
    assert!(matches!(
        refuse(
            &catalog,
            "SELECT COUNT(*) FROM logistics.shipment WHERE bucket = 1"
        ),
        GuardError::FunctionNotAllowed
    ));
    assert_eq!(
        refuse(&catalog, "SELECT lower(bucket) FROM logistics.shipment"),
        GuardError::FunctionNotAllowed
    );
    assert!(matches!(
        refuse(&catalog, "SELECT hidden_balance FROM logistics.shipment"),
        GuardError::ColumnNotAllowed { .. }
    ));
    assert!(matches!(
        refuse(
            &catalog,
            "SELECT COUNT(*) FROM logistics.shipment WHERE bucket IN (DATE '2026-05-06')"
        ),
        GuardError::Unsupported { construct: "cast" }
    ));
}

#[test]
fn alias_hides_the_table_name() {
    let plan = admit(&small(), "SELECT s.order_id FROM logistics.shipment AS s");
    assert_eq!(plan.bindings.relations[0].alias.as_deref(), Some("s"));
    assert!(
        plan.executed_sql.contains("pc_order"),
        "{}",
        plan.executed_sql
    );
    let err = refuse(
        &small(),
        "SELECT shipment.order_id FROM logistics.shipment AS s",
    );
    assert!(matches!(
        err,
        GuardError::ColumnNotAllowed {
            ambiguous: false,
            ..
        }
    ));
    assert_silent(&err, &["shipment"]);
    let renamed = admit(&small(), "SELECT order_id AS id FROM logistics.shipment");
    match &renamed.bindings.outputs[0] {
        OutputItem::Column(column) => assert_eq!(column.output_name, "id"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        refuse(
            &small(),
            "SELECT order_id AS \"Bad\" FROM logistics.shipment"
        ),
        GuardError::Unsupported {
            construct: "output alias"
        }
    );
}

#[test]
fn visible_star_expands_in_catalog_order() {
    let plan = admit(&small(), "SELECT * FROM logistics.shipment");
    assert_eq!(plan.star_expansions.len(), 1);
    assert_eq!(plan.star_expansions[0].qualifier, "");
    assert_eq!(plan.star_expansions[0].columns, ["order_id", "region"]);
    assert_physical(&plan, "star");
    assert!(
        plan.executed_sql.contains("pc_order"),
        "{}",
        plan.executed_sql
    );
    assert!(
        plan.executed_sql.contains("pc_region"),
        "{}",
        plan.executed_sql
    );
    let qualified = admit(
        &small(),
        "SELECT logistics.shipment.* FROM logistics.shipment",
    );
    assert_eq!(qualified.star_expansions[0].qualifier, "logistics.shipment");
    let aliased = admit(&small(), "SELECT s.* FROM logistics.shipment AS s");
    assert_eq!(aliased.star_expansions[0].qualifier, "s");
    let missed = refuse(
        &small(),
        "SELECT logistics.shipment.* FROM logistics.shipment AS s",
    );
    assert!(matches!(
        missed,
        GuardError::ColumnNotAllowed {
            ambiguous: false,
            ..
        }
    ));
    assert!(!missed.to_string().contains("logistics"));
    assert!(!format!("{missed:?}").contains("logistics"));
}

#[test]
fn hidden_and_missing_columns_share_one_error() {
    let cat = small();
    let hidden = refuse(&cat, "SELECT secret_zz FROM logistics.shipment");
    let missing = refuse(&cat, "SELECT missing_zz FROM logistics.shipment");
    assert_eq!(hidden, missing);
    assert_eq!(
        hidden.to_string(),
        "column is not visible: logistics.shipment.order_id, logistics.shipment.region; 1 columns are pending classification"
    );
    assert_silent(&hidden, &["secret_zz", "missing_zz"]);
    assert_eq!(hidden.code(), ErrorCode::ColumnNotAllowed);
    assert_eq!(hidden.code().http_status(), 400);
}

#[test]
fn hidden_and_missing_relations_share_one_error() {
    let cat = small();
    let hidden = refuse(&cat, "SELECT order_id FROM logistics.vault_zz");
    let missing = refuse(&cat, "SELECT order_id FROM other_zz.shipment");
    assert_eq!(hidden, GuardError::RelationNotFound);
    assert_eq!(missing, GuardError::RelationNotFound);
    assert_eq!(hidden.to_string(), "relation is not visible");
    assert_silent(&hidden, &["vault_zz"]);
    assert_silent(&missing, &["other_zz"]);
    assert_eq!(hidden.code(), ErrorCode::RelationNotFound);
}

#[test]
fn unknown_qualifier_is_not_echoed() {
    let err = refuse(&small(), "SELECT nope_zz.order_id FROM logistics.shipment");
    assert!(matches!(
        err,
        GuardError::ColumnNotAllowed {
            ambiguous: false,
            ..
        }
    ));
    assert_silent(&err, &["nope_zz"]);
}

#[test]
fn unqualified_relation_suggests_only_a_unique_visible_name() {
    let err = refuse(&small(), "SELECT order_id FROM shipment");
    assert_eq!(
        err,
        GuardError::Unqualified {
            suggestion: Some("logistics.shipment".to_string())
        }
    );
    assert_eq!(err.hint().as_deref(), Some("use logistics.shipment"));
    assert_eq!(err.code(), ErrorCode::UnqualifiedRelation);
    let both = catalog(vec![
        shipment(10),
        relation(
            "billing",
            "shipment",
            "pt_bill",
            vec![col("order_id", "pc_bill", INT_OID)],
            0,
            1,
        ),
    ]);
    let ambiguous = refuse(&both, "SELECT order_id FROM shipment");
    assert_eq!(ambiguous, GuardError::Unqualified { suggestion: None });
    assert_eq!(ambiguous.hint().as_deref(), Some("use schema.name"));
    assert_eq!(ambiguous.to_string(), "relation must be schema-qualified");
}

#[test]
fn utilities_are_rejected() {
    let cat = small();
    for sql in [
        "VACUUM",
        "SHOW search_path",
        "SET search_path TO ''",
        "EXPLAIN SELECT 1",
        "COPY logistics.shipment TO STDOUT",
        "SELECT 1; SELECT 2",
    ] {
        let err = refuse(&cat, sql);
        assert_eq!(err, GuardError::Statement, "{sql} -> {err}");
        assert_eq!(err.code(), ErrorCode::StatementNotAllowed);
    }
}

#[test]
fn writes_are_rejected_including_inside_a_cte() {
    let cat = small();
    for sql in [
        "INSERT INTO logistics.shipment (order_id) VALUES (1)",
        "UPDATE logistics.shipment SET region = 'west'",
        "DELETE FROM logistics.shipment",
        "WITH x AS (DELETE FROM logistics.shipment RETURNING order_id) SELECT order_id FROM x",
        "EXPLAIN INSERT INTO logistics.shipment (order_id) VALUES (1)",
        "SELECT 1; DELETE FROM logistics.shipment",
    ] {
        assert_eq!(refuse(&cat, sql), GuardError::Write, "{sql}");
    }
}

#[test]
fn read_cte_and_subquery_are_unsupported() {
    let cat = small();
    assert_eq!(
        refuse(
            &cat,
            "WITH t AS (SELECT 1 AS order_id) SELECT order_id FROM t"
        ),
        GuardError::Unsupported { construct: "CTE" }
    );
    assert_eq!(
        refuse(&cat, "SELECT * FROM (SELECT 1) AS s"),
        GuardError::Unsupported {
            construct: "subquery"
        }
    );
}

#[test]
fn casts_other_than_a_bool_literal_are_refused() {
    assert_eq!(
        refuse(&small(), "SELECT order_id::text FROM logistics.shipment"),
        GuardError::Unsupported { construct: "cast" }
    );
    let plan = admit(&small(), "SELECT 'true'::bool");
    assert_eq!(
        plan.bindings.outputs,
        vec![OutputItem::Literal(LiteralKind::Boolean)]
    );
}

#[test]
fn functions_and_operators_need_one_oid_signature() {
    let cat = small();
    assert_eq!(
        refuse(&cat, "SELECT lower(region) FROM logistics.shipment"),
        GuardError::FunctionNotAllowed
    );
    assert_eq!(
        refuse(&cat, "SELECT count(order_id) FROM logistics.shipment"),
        GuardError::FunctionNotAllowed
    );
    assert_eq!(
        refuse(
            &cat,
            "SELECT order_id FROM logistics.shipment WHERE order_id = 'west'"
        ),
        GuardError::FunctionNotAllowed
    );
    let text = admit(
        &cat,
        "SELECT region FROM logistics.shipment WHERE region = 'west'",
    );
    assert_eq!(text.bindings.operators[0].oid, 303);
    let integer = admit(
        &cat,
        "SELECT order_id FROM logistics.shipment WHERE order_id = 1",
    );
    assert_eq!(integer.bindings.operators[0].oid, 301);
    assert_eq!(integer.bindings.operators[0].procedure_oid, 302);
}

#[test]
fn count_star_is_not_rewritten() {
    let plan = admit(&small(), "SELECT count(*) FROM logistics.shipment");
    assert!(plan.star_expansions.is_empty());
    let sql = plan.executed_sql.to_ascii_lowercase();
    assert!(sql.contains("count"), "{sql}");
    assert!(sql.contains('*'), "{sql}");
    assert!(sql.contains("pt_ship"), "{sql}");
    assert!(!sql.contains("pc_order"), "{sql}");
    assert!(!sql.contains("pc_region"), "{sql}");
    assert_eq!(plan.bindings.functions.len(), 1);
    assert!(plan.bindings.functions[0].aggregate_star);
    assert_eq!(plan.bindings.functions[0].oid, 201);
}

#[test]
fn parser_text_is_dropped() {
    let err = refuse(&small(), "SELEKT hidden_name_zz");
    assert_eq!(err, GuardError::Parse);
    assert_eq!(err.to_string(), "SQL parse failed");
    assert_eq!(err.code(), ErrorCode::SqlParseError);
    assert_silent(&err, &["hidden_name_zz", "SELEKT"]);
}

#[test]
fn budgets_fail_closed() {
    let mut at_limit = String::from("SELECT 1");
    at_limit.push_str(&" ".repeat(SQL_BYTE_BUDGET - at_limit.len()));
    assert_eq!(at_limit.len(), SQL_BYTE_BUDGET);
    let plan = admit(&small(), &at_limit);
    assert!(plan.executed_sql.contains('1'), "{}", plan.executed_sql);
    let mut over = at_limit;
    over.push(' ');
    assert_eq!(refuse(&small(), &over), GuardError::Budget);

    // Forty nested NOTs decode and stay under the depth budget.
    // Forty-eight hits prost's decode recursion limit inside pg_query and
    // fails closed as a parse error. A decoded tree never reaches depth 128,
    // because that decoder stops first. The walker still enforces the budget.
    let nested = format!("SELECT {}true", "NOT ".repeat(40));
    let nested_plan = admit(&small(), &nested);
    assert!(matches!(
        nested_plan.bindings.outputs[0],
        OutputItem::Compound
    ));
    let too_nested = format!("SELECT {}true", "NOT ".repeat(48));
    let nested_err = refuse(&small(), &too_nested);
    assert_eq!(nested_err, GuardError::Parse);
    assert_silent(&nested_err, &["NOT", "true"]);
    let shallow = admit(&small(), "SELECT NOT true");
    assert!(matches!(shallow.bindings.outputs[0], OutputItem::Compound));

    let mut wide = String::from("SELECT 1 WHERE 1=1");
    for _ in 0..2799 {
        wide.push_str(" AND 1=1");
    }
    assert!(wide.len() <= SQL_BYTE_BUDGET);
    assert_eq!(refuse(&small(), &wide), GuardError::Budget);
    let narrow = admit(&small(), "SELECT 1 WHERE 1=1 AND 1=1");
    assert_eq!(narrow.bindings.operators.len(), 2);
}

#[test]
fn large_star_without_a_real_filter_is_refused() {
    let err = refuse(&large(), "SELECT * FROM logistics.shipment");
    assert_eq!(
        err,
        GuardError::CostTooHigh {
            relation: "logistics.shipment".to_string()
        }
    );
    assert_eq!(err.hint().as_deref(), Some("add a filter or use export"));
    assert_eq!(err.code(), ErrorCode::CostTooHigh);
    assert_eq!(err.code().http_status(), 422);
    assert_silent(&err, &["424242", "secret"]);
    for sql in [
        "SELECT * FROM logistics.shipment WHERE true",
        "SELECT * FROM logistics.shipment LIMIT 10",
    ] {
        assert_eq!(
            refuse(&large(), sql),
            GuardError::CostTooHigh {
                relation: "logistics.shipment".to_string()
            },
            "{sql}"
        );
    }
}

#[test]
fn a_real_filter_or_an_explicit_column_allows_a_large_table() {
    let filtered = admit(
        &large(),
        "SELECT * FROM logistics.shipment WHERE order_id = 1",
    );
    assert!(
        filtered.executed_sql.contains("pc_order"),
        "{}",
        filtered.executed_sql
    );
    assert!(!filtered.star_expansions.is_empty());
    let compared = admit(&large(), "SELECT * FROM logistics.shipment WHERE 1 = 1");
    assert!(!compared.star_expansions.is_empty());
    let false_where = admit(&large(), "SELECT * FROM logistics.shipment WHERE false");
    assert!(!false_where.star_expansions.is_empty());
    let explicit = admit(&large(), "SELECT order_id FROM logistics.shipment");
    assert!(explicit.star_expansions.is_empty());
    assert!(
        explicit.executed_sql.contains("pc_order"),
        "{}",
        explicit.executed_sql
    );
}

#[test]
fn join_star_charges_the_starred_relation() {
    let small_star = admit(
        &joined(),
        "SELECT c.* FROM logistics.customer AS c \
         INNER JOIN logistics.shipment AS s ON c.region = s.region",
    );
    assert_eq!(small_star.star_expansions.len(), 1);
    assert_eq!(small_star.star_expansions[0].name, "customer");
    assert!(
        small_star.executed_sql.contains("pc_cust"),
        "{}",
        small_star.executed_sql
    );
    assert!(
        !small_star.executed_sql.contains("pc_order"),
        "{}",
        small_star.executed_sql
    );
    let err = refuse(
        &joined(),
        "SELECT * FROM logistics.customer AS c \
         INNER JOIN logistics.shipment AS s ON c.region = s.region",
    );
    assert_eq!(
        err,
        GuardError::CostTooHigh {
            relation: "logistics.shipment".to_string()
        }
    );
}

#[test]
fn explicit_joins_of_base_relations_are_admitted() {
    let cat = joined();
    for sql in [
        "SELECT c.customer_id, s.order_id FROM logistics.customer AS c \
         INNER JOIN logistics.shipment AS s ON c.region = s.region",
        "SELECT c.customer_id FROM logistics.customer AS c \
         LEFT JOIN logistics.shipment AS s ON c.region = s.region",
        "SELECT c.customer_id FROM logistics.customer AS c \
         RIGHT JOIN logistics.shipment AS s ON c.region = s.region",
        "SELECT c.customer_id FROM logistics.customer AS c \
         FULL JOIN logistics.shipment AS s ON c.region = s.region",
        "SELECT c.customer_id, s.order_id FROM logistics.customer AS c \
         CROSS JOIN logistics.shipment AS s",
        "SELECT c.customer_id, s.order_id FROM logistics.customer AS c, logistics.shipment AS s",
    ] {
        let plan = admit(&cat, sql);
        assert!(
            plan.executed_sql.contains("pt_cust"),
            "{sql}\n{}",
            plan.executed_sql
        );
        assert!(
            plan.executed_sql.contains("pt_ship"),
            "{sql}\n{}",
            plan.executed_sql
        );
        assert_eq!(plan.bindings.relations.len(), 2, "{sql}");
    }
    assert_eq!(
        refuse(
            &cat,
            "SELECT c.customer_id FROM logistics.customer AS c \
             NATURAL JOIN logistics.shipment AS s"
        ),
        GuardError::Unsupported {
            construct: "NATURAL JOIN"
        }
    );
    assert_eq!(
        refuse(
            &cat,
            "SELECT c.customer_id FROM logistics.customer AS c \
             INNER JOIN logistics.shipment AS s USING (region)"
        ),
        GuardError::Unsupported {
            construct: "JOIN USING"
        }
    );
}

#[test]
fn system_schemas_are_not_echoed() {
    let cat = small();
    let catalog_err = refuse(&cat, "SELECT 1 FROM pg_catalog.pg_class");
    assert_eq!(catalog_err, GuardError::SystemSchema);
    assert_eq!(catalog_err.to_string(), "system schema is not allowed");
    assert_silent(&catalog_err, &["pg_catalog", "pg_class"]);
    let internal = refuse(&cat, "SELECT 1 FROM _oo.pt_hidenzz");
    assert_eq!(internal, GuardError::SystemSchema);
    assert_silent(&internal, &["_oo", "pt_hidenzz"]);
    assert_eq!(catalog_err.code(), ErrorCode::StatementNotAllowed);
}

#[test]
fn identity_and_catalog_shape_are_required() {
    let mut other = identity();
    other.policy_id = "other-policy".to_string();
    let mismatch = guard_select(&other, &small(), "SELECT 1").expect_err("mismatch");
    assert_eq!(mismatch, GuardError::IdentityMismatch);
    assert_eq!(mismatch.code(), ErrorCode::SchemaInvalid);
    let mut broken = shipment(10);
    broken.columns.clear();
    let empty = refuse(&catalog(vec![broken]), "SELECT 1");
    assert_eq!(
        empty,
        GuardError::CatalogInvalid {
            reason: "empty column list"
        }
    );
    let mut zero = shipment(10);
    zero.columns[0].type_oid = 0;
    assert_eq!(
        refuse(&catalog(vec![zero]), "SELECT 1"),
        GuardError::CatalogInvalid { reason: "type oid" }
    );
}

#[test]
fn order_null_and_shape_limits() {
    let cat = small();
    let ordered = admit(
        &cat,
        "SELECT order_id FROM logistics.shipment ORDER BY order_id",
    );
    assert!(
        ordered.executed_sql.contains("pc_order"),
        "{}",
        ordered.executed_sql
    );
    assert_eq!(
        refuse(&cat, "SELECT order_id FROM logistics.shipment ORDER BY 1"),
        GuardError::Unsupported {
            construct: "order key"
        }
    );
    let null_test = admit(
        &cat,
        "SELECT order_id FROM logistics.shipment WHERE order_id IS NULL",
    );
    assert_eq!(null_test.bindings.null_tests, 1);
    let mut blocked = small();
    blocked.null_test_allowed = false;
    assert_eq!(
        refuse(
            &blocked,
            "SELECT order_id FROM logistics.shipment WHERE order_id IS NULL"
        ),
        GuardError::FunctionNotAllowed
    );
    assert_eq!(
        refuse(&cat, "SELECT DISTINCT order_id FROM logistics.shipment"),
        GuardError::Unsupported {
            construct: "DISTINCT"
        }
    );
    let grouped = admit(
        &cat,
        "SELECT order_id FROM logistics.shipment GROUP BY order_id",
    );
    assert!(grouped.executed_sql.contains("GROUP BY"));
    assert_eq!(
        refuse(
            &cat,
            "SELECT order_id FROM logistics.shipment GROUP BY secret_zz"
        ),
        refuse(&cat, "SELECT secret_zz FROM logistics.shipment")
    );
    assert_eq!(
        refuse(
            &cat,
            "SELECT order_id FROM logistics.shipment \
             UNION SELECT order_id FROM logistics.shipment"
        ),
        GuardError::Unsupported {
            construct: "set operation"
        }
    );
    assert_eq!(
        refuse(&cat, "SELECT order_id INTO result FROM logistics.shipment"),
        GuardError::Into
    );
    assert_eq!(
        refuse(&cat, "SELECT order_id FROM logistics.shipment FOR UPDATE"),
        GuardError::Locking
    );
    assert_eq!(
        refuse(&cat, "SELECT order_id FROM ONLY logistics.shipment"),
        GuardError::Unsupported { construct: "ONLY" }
    );
    assert_eq!(
        refuse(&cat, "SELECT 1.5"),
        GuardError::Unsupported {
            construct: "float literal"
        }
    );
    assert_eq!(
        refuse(&cat, "SELECT $1"),
        GuardError::Unsupported {
            construct: "parameter"
        }
    );
    assert_eq!(
        refuse(
            &cat,
            "SELECT region FROM logistics.shipment WHERE region LIKE 'w%'"
        ),
        GuardError::Unsupported {
            construct: "LIKE predicate"
        }
    );
    let constant = admit(&cat, "SELECT 1");
    assert!(constant.bindings.relations.is_empty());
    assert_eq!(
        constant.bindings.outputs,
        vec![OutputItem::Literal(LiteralKind::Integer)]
    );
    assert_eq!(
        refuse(&cat, "SELECT *"),
        GuardError::Unsupported { construct: "star" }
    );
}

#[test]
fn ambiguous_column_lists_visible_labels_only() {
    let err = refuse(
        &joined(),
        "SELECT region FROM logistics.customer AS c \
         INNER JOIN logistics.shipment AS s ON c.region = s.region",
    );
    let text = err.to_string();
    assert!(text.contains("column reference is ambiguous"), "{text}");
    assert!(text.contains("c.region"), "{text}");
    assert!(text.contains("s.region"), "{text}");
    assert!(
        text.contains("1 columns are pending classification"),
        "{text}"
    );
    assert_silent(&err, &["secret", "pc_"]);
    assert_eq!(err.code(), ErrorCode::ColumnNotAllowed);
}

#[tokio::test]
async fn live_column_role_executes_guarded_select() {
    if std::env::var("OO_A175_LIVE").ok().as_deref() != Some("1") {
        eprintln!("OO_A175_LIVE is unset; the PostgreSQL comparison was not run");
        return;
    }
    run_live().await;
}

const HOST: &str = "oo-a175-pg";
const DATABASE: &str = "oo_a175";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a175_login";
const SECRET_DIR: &str = "/run/oo-a175/fixture";
const ORACLE_PATH: &str = "/evidence/live/oracle.txt";
const OWNER: &str = "a1750000-0000-4000-8000-000000000175";
const SHARD: &str = "shard_a175";
const SOURCE: &str = "orders";
const CONFIG: &str = "a175a175a175a175a175a175a175a175a175a175a175a175a175a175a175a175";
const TYPE_DIGEST: &str = "b175b175b175b175b175b175b175b175b175b175b175b175b175b175b175b175";
const INPUT_DIGEST: &str = "c175c175c175c175c175c175c175c175c175c175c175c175c175c175c175c175";
const SCHEMA_DIGEST: &str = "d175d175d175d175d175d175d175d175d175d175d175d175d175d175d175d175";
const NAMESPACE: &str = "logistics";
const TABLE: &str = "shipment";
const STALE_GENERATION: i64 = 3;
const CURRENT_GENERATION: i64 = 7;

struct Shadow {
    table: String,
    order_id: String,
    region: String,
    secret: String,
}

struct Oracle {
    lines: Vec<String>,
    secrets: Vec<String>,
}

impl Oracle {
    fn new(admin_pass: &str, login_pass: &str) -> Self {
        Self {
            lines: Vec::new(),
            secrets: vec![admin_pass.to_string(), login_pass.to_string()],
        }
    }

    fn fact(&mut self, key: &str, value: &str) {
        assert!(!key.contains("pass"), "oracle key");
        for secret in &self.secrets {
            assert!(!value.contains(secret), "oracle value contained a password");
        }
        self.lines.push(format!("{key}={value}"));
        self.flush();
    }

    fn flush(&self) {
        let path = Path::new(ORACLE_PATH);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("oracle dir");
        }
        fs::write(path, self.lines.join("\n") + "\n").expect("oracle write");
    }
}

async fn run_live() {
    let admin_pass = read_hex("admin.pass");
    let login_pass = read_hex("login.pass");
    let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca.crt");
    let mut oracle = Oracle::new(&admin_pass, &login_pass);
    oracle.fact("live", "ran");

    let maintenance = connect_db("postgres", ADMIN, &admin_pass, &ca).await;
    reset_database(&maintenance).await;
    drop(maintenance);

    let mut admin = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
    apply_frozen_schema(&mut admin).await;
    create_login(&admin, &login_pass).await;
    let shadow = prepare_shadow(&mut admin).await;
    let installed = install_column_acl(&mut admin, &dispatcher_policy())
        .await
        .expect("install dispatcher");
    assert_eq!(installed.effect, InstallEffect::Installed);
    assert_eq!(installed.role, "oo_r_dispatcher");
    oracle.fact("physical_table", &shadow.table);
    oracle.fact("order_column", &shadow.order_id);
    oracle.fact("region_column", &shadow.region);
    oracle.fact("dispatcher_role", &installed.role);

    let catalog = live_catalog(&admin, &shadow).await;
    oracle.fact(
        "int_oid",
        &catalog.literal_types.integer.unwrap_or(0).to_string(),
    );
    oracle.fact(
        "text_oid",
        &catalog.literal_types.text.unwrap_or(0).to_string(),
    );
    oracle.fact("count_oid", &catalog.functions[0].oid.to_string());

    let star = guard_select(
        &catalog.identity,
        &catalog,
        "SELECT * FROM logistics.shipment",
    )
    .expect("guard star");
    assert!(
        star.executed_sql.contains(&shadow.order_id),
        "{}",
        star.executed_sql
    );
    assert!(
        star.executed_sql.contains(&shadow.region),
        "{}",
        star.executed_sql
    );
    assert!(
        !star.executed_sql.contains(&shadow.secret),
        "{}",
        star.executed_sql
    );
    assert!(
        !star.executed_sql.contains("secret"),
        "{}",
        star.executed_sql
    );
    assert_eq!(star.parser_version, PINNED_PARSER_VERSION);
    oracle.fact("parser_version", &star.parser_version.to_string());
    oracle.fact("executed_sha256", &star.executed_sha256);
    oracle.fact("executed_sql", &star.executed_sql);

    let reader = connect_db(DATABASE, LOGIN, &login_pass, &ca).await;
    reader
        .batch_execute("SET search_path TO ''")
        .await
        .expect("search_path");
    reader
        .batch_execute("SET ROLE oo_r_dispatcher")
        .await
        .expect("set role");
    let rows = reader
        .query(&star.executed_sql, &[])
        .await
        .expect("execute guarded star");
    assert_eq!(rows.len(), 1);
    let order_id: i32 = rows[0].get(0);
    let region: String = rows[0].get(1);
    assert_eq!(order_id, 42);
    assert_eq!(region, "west");
    oracle.fact("star_order_id", &order_id.to_string());
    oracle.fact("star_region", &region);

    let original = reader.query("SELECT * FROM logistics.shipment", &[]).await;
    let state = match original {
        Ok(_) => panic!("unguarded star succeeded under the column role"),
        Err(err) => sqlstate(&err),
    };
    oracle.fact("original_star_sqlstate", &state);

    let counted = guard_select(
        &catalog.identity,
        &catalog,
        "SELECT count(*) FROM logistics.shipment",
    )
    .expect("guard count");
    assert!(counted.star_expansions.is_empty());
    assert!(!counted.executed_sql.contains(&shadow.secret));
    match reader.query(&counted.executed_sql, &[]).await {
        Ok(rows) => oracle.fact("count_sqlstate", &format!("ok:{}", rows.len())),
        Err(err) => oracle.fact("count_sqlstate", &sqlstate(&err)),
    }

    let filtered = guard_select(
        &catalog.identity,
        &catalog,
        "SELECT order_id, region FROM logistics.shipment WHERE order_id = 42",
    )
    .expect("guard filter");
    let filtered_rows = reader
        .query(&filtered.executed_sql, &[])
        .await
        .expect("execute filter");
    assert_eq!(filtered_rows.len(), 1);
    let filtered_id: i32 = filtered_rows[0].get(0);
    assert_eq!(filtered_id, 42);
    oracle.fact("filter_order_id", &filtered_id.to_string());
    oracle.fact("live_result", "compared");
}

fn sqlstate(err: &tokio_postgres::Error) -> String {
    err.as_db_error()
        .map(|db| db.code().code().to_string())
        .unwrap_or_else(|| "none".to_string())
}

async fn live_catalog(client: &Client, shadow: &Shadow) -> PolicyCatalog {
    let int_oid = type_oid(client, "int4").await;
    let text_oid = type_oid(client, "text").await;
    let bool_oid = type_oid(client, "bool").await;
    let order_ty = column_type(client, &shadow.table, &shadow.order_id).await;
    let region_ty = column_type(client, &shadow.table, &shadow.region).await;
    assert_eq!(order_ty, int_oid);
    assert_eq!(region_ty, text_oid);
    let (eq_int, eq_int_proc) = operator_oid(client, int_oid, int_oid).await;
    let (eq_text, eq_text_proc) = operator_oid(client, text_oid, text_oid).await;
    let count_oid = count_oid(client).await;
    PolicyCatalog {
        identity: PolicyIdentity {
            project: SOURCE.to_string(),
            role: "oo_r_dispatcher".to_string(),
            policy_id: CONFIG.to_string(),
        },
        relations: vec![VisibleRelation {
            schema: NAMESPACE.to_string(),
            name: TABLE.to_string(),
            physical_schema: "_oo".to_string(),
            physical_name: shadow.table.clone(),
            columns: vec![
                VisibleColumn {
                    name: "order_id".to_string(),
                    physical_name: shadow.order_id.clone(),
                    type_oid: order_ty,
                },
                VisibleColumn {
                    name: "region".to_string(),
                    physical_name: shadow.region.clone(),
                    type_oid: region_ty,
                },
            ],
            pending_unclassified: 1,
            estimated_rows: 10,
        }],
        functions: vec![ResolvedFunction {
            oid: count_oid,
            schema: "pg_catalog".to_string(),
            name: "count".to_string(),
            argument_type_oids: Vec::new(),
            aggregate_star: true,
        }],
        operators: vec![
            ResolvedOperator {
                oid: eq_int,
                procedure_oid: eq_int_proc,
                schema: "pg_catalog".to_string(),
                name: "=".to_string(),
                left_type_oid: int_oid,
                right_type_oid: int_oid,
            },
            ResolvedOperator {
                oid: eq_text,
                procedure_oid: eq_text_proc,
                schema: "pg_catalog".to_string(),
                name: "=".to_string(),
                left_type_oid: text_oid,
                right_type_oid: text_oid,
            },
        ],
        literal_types: LiteralTypes {
            integer: Some(int_oid),
            text: Some(text_oid),
            boolean: Some(bool_oid),
        },
        null_test_allowed: true,
        large_table_row_threshold: 1_000_000,
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

async fn column_type(client: &Client, table: &str, column: &str) -> u32 {
    let rel = format!("_oo.{table}");
    let oid: i32 = client
        .query_one(
            "SELECT atttypid::int4 FROM pg_catalog.pg_attribute \
             WHERE attrelid = ($1::text)::regclass AND attname = $2 AND NOT attisdropped",
            &[&rel, &column],
        )
        .await
        .expect("column type")
        .get(0);
    u32::try_from(oid).expect("column type range")
}

async fn operator_oid(client: &Client, left: u32, right: u32) -> (u32, u32) {
    let left_i = i32::try_from(left).expect("left oid");
    let right_i = i32::try_from(right).expect("right oid");
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
        .expect("operator");
    let oid: i32 = row.get(0);
    let proc: i32 = row.get(1);
    (
        u32::try_from(oid).expect("operator oid"),
        u32::try_from(proc).expect("operator proc"),
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

fn dispatcher_policy() -> ColumnAclPolicy {
    ColumnAclPolicy {
        project: SOURCE.to_string(),
        config_digest: CONFIG.to_string(),
        generation: CURRENT_GENERATION,
        role: "dispatcher".to_string(),
        login: LOGIN.to_string(),
        table: LogicalTable {
            namespace: NAMESPACE.to_string(),
            name: TABLE.to_string(),
            columns: vec![
                ColumnSpec {
                    name: "order_id".to_string(),
                    status: ClassStatus::Confirmed,
                    semantic_type: "identifier".to_string(),
                    pii: PiiClass::None,
                },
                ColumnSpec {
                    name: "region".to_string(),
                    status: ClassStatus::Confirmed,
                    semantic_type: "identifier".to_string(),
                    pii: PiiClass::None,
                },
            ],
        },
        row_filter: None,
    }
}

fn read_hex(name: &str) -> String {
    let raw = fs::read_to_string(Path::new(SECRET_DIR).join(name)).expect("secret file");
    let value = raw.trim();
    let hex = (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    assert!(hex, "{name} is not lowercase hex");
    value.to_string()
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
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
    panic!("tls connect to {database} as {user} failed: {last}");
}

async fn reset_database(client: &Client) {
    expect_sql(
        client,
        "terminate database sessions",
        &format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{DATABASE}' AND pid <> pg_backend_pid()"
        ),
    )
    .await;
    expect_sql(
        client,
        "drop database",
        &format!("DROP DATABASE IF EXISTS \"{DATABASE}\""),
    )
    .await;
    expect_sql(
        client,
        "drop fixture roles",
        "DO $$\n\
         DECLARE role_name text;\n\
         BEGIN\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a175_login') THEN\n\
             DROP ROLE oo_a175_login;\n\
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
    expect_sql(
        client,
        "create database",
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
    exec_named(&tx, "0000_bootstrap_roles.sql", &bootstrap.0).await;
    exec_named(&tx, "set-owner", "SET LOCAL ROLE oo_target_owner").await;
    exec_named(&tx, "0001_target_fence_expand.sql", &fence.0).await;
    exec_named(
        &tx,
        "row-lock",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE",
    )
    .await;
    exec_named(
        &tx,
        "0003_target_segment_preflight_expand.sql",
        &preflight.0,
    )
    .await;
    exec_named(&tx, "0004_physical_mapping.sql", &mapping.0).await;
    exec_named(
        &tx,
        "markers-target",
        &marker_sql(&[
            (
                "target-bootstrap/1",
                &bootstrap.1,
                "0000_bootstrap_roles.sql",
            ),
            (
                "target-fence-receipts/1",
                &fence.1,
                "0001_target_fence_expand.sql",
            ),
            (
                "target-fence-receipts/3",
                &preflight.1,
                "0003_target_segment_preflight_expand.sql",
            ),
            (
                "physical-mapping/v1",
                &mapping.1,
                "0004_physical_mapping.sql",
            ),
        ]),
    )
    .await;
    tx.commit().await.expect("commit target schema");

    let tx = begin_locked(client).await;
    exec_named(
        &tx,
        "row-lock-import",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE",
    )
    .await;
    exec_named(
        &tx,
        "stats-grant",
        "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE",
    )
    .await;
    exec_named(&tx, "set-owner-import", "SET LOCAL ROLE oo_target_owner").await;
    exec_named(&tx, "0005_full_import_expand.sql", &full_import.0).await;
    exec_named(
        &tx,
        "marker-import",
        &marker_sql(&[(
            "full-import/v1",
            &full_import.1,
            "0005_full_import_expand.sql",
        )]),
    )
    .await;
    tx.commit().await.expect("commit full import");
}

fn migration(name: &str) -> (String, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oo-pipeline/migrations")
        .join(name);
    let bytes = fs::read(&path).unwrap_or_else(|err| panic!("read {name}: {err}"));
    let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| panic!("{name} is not utf-8"));
    (text, sha256_bytes(&bytes))
}

fn sha256_bytes(bytes: &[u8]) -> String {
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
        if index > 0 {
            values.push_str(",\n");
        }
        values.push_str(&format!(
            "  ('{version}', '{checksum}', 'expand', '{filename}')"
        ));
    }
    format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values}"
    )
}

async fn begin_locked(client: &mut Client) -> tokio_postgres::Transaction<'_> {
    let tx = client.transaction().await.expect("begin");
    exec_named(&tx, "lock-timeout", "SET LOCAL lock_timeout = '15s'").await;
    exec_named(
        &tx,
        "statement-timeout",
        "SET LOCAL statement_timeout = '60s'",
    )
    .await;
    exec_named(
        &tx,
        "advisory-lock",
        "SELECT pg_catalog.pg_advisory_xact_lock(6404202617)",
    )
    .await;
    tx
}

async fn expect_sql(client: &Client, name: &str, sql: &str) {
    client
        .batch_execute(sql)
        .await
        .unwrap_or_else(|err| panic!("{name} failed: {}", db_detail(&err)));
}

fn db_detail(err: &tokio_postgres::Error) -> String {
    err.as_db_error()
        .map(|db| db.to_string())
        .unwrap_or_else(|| err.to_string())
}

async fn exec_named(tx: &tokio_postgres::Transaction<'_>, name: &str, sql: &str) {
    tx.batch_execute(sql)
        .await
        .unwrap_or_else(|err| panic!("{name} failed: {}", db_detail(&err)));
}

async fn create_login(client: &Client, password: &str) {
    assert!(password.bytes().all(|byte| byte.is_ascii_hexdigit()));
    client
        .batch_execute(&format!(
            "CREATE ROLE \"{LOGIN}\" LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOBYPASSRLS NOREPLICATION PASSWORD '{password}';\n\
             GRANT CONNECT ON DATABASE \"{DATABASE}\" TO \"{LOGIN}\""
        ))
        .await
        .expect("create login");
}

async fn prepare_shadow(client: &mut Client) -> Shadow {
    let target: i64 = client
        .query_one(
            "SELECT _oo.adopt_target_generation($1, ($2::text)::uuid)",
            &[&SHARD, &OWNER],
        )
        .await
        .expect("adopt")
        .get(0);
    client
        .execute(
            "SELECT _oo.full_import_bind_session($1, ($2::text)::uuid, $3)",
            &[&SHARD, &OWNER, &target],
        )
        .await
        .expect("bind");
    for generation in [STALE_GENERATION, CURRENT_GENERATION] {
        register(client, generation, "namespace", "n", NAMESPACE, "", "").await;
        register(client, generation, "table", "t", NAMESPACE, TABLE, "").await;
        for column in ["order_id", "region", "secret"] {
            register(client, generation, "column", "c", NAMESPACE, TABLE, column).await;
        }
    }
    let table = lookup(client, "table", NAMESPACE, TABLE, "").await;
    let order_id = lookup(client, "column", NAMESPACE, TABLE, "order_id").await;
    let region = lookup(client, "column", NAMESPACE, TABLE, "region").await;
    let secret = lookup(client, "column", NAMESPACE, TABLE, "secret").await;
    let quoted_table = quote_physical(&table, "pt_");
    let schemas = vec![NAMESPACE.to_string()];
    let tables = vec![TABLE.to_string()];
    let physical = vec![table.clone()];
    let open_params: [&(dyn ToSql + Sync); 12] = [
        &SHARD,
        &OWNER,
        &target,
        &SOURCE,
        &CURRENT_GENERATION,
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
        .expect("open run")
        .get(0);
    let idents = vec![
        "order_id".to_string(),
        "region".to_string(),
        "secret".to_string(),
    ];
    let oids = vec![23_i64, 25, 25];
    let typmods = vec![-1_i32, -1, -1];
    let required = vec![true, true, true];
    let index = 0_i32;
    let prepare_params: [&(dyn ToSql + Sync); 14] = [
        &SHARD,
        &OWNER,
        &target,
        &run_id,
        &index,
        &SOURCE,
        &CONFIG,
        &CURRENT_GENERATION,
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
        .expect("prepare shadow")
        .get(0);
    assert_eq!(prepared, table);
    let order_sql = quote_physical(&order_id, "pc_");
    let region_sql = quote_physical(&region, "pc_");
    let secret_sql = quote_physical(&secret, "pc_");
    let insert = format!(
        "INSERT INTO \"_oo\".{quoted_table} (\"_oo_pos\", {order_sql}, {region_sql}, {secret_sql}) \
         VALUES (($1::text)::numeric, $2, $3, $4)"
    );
    let tx = client.transaction().await.expect("insert begin");
    let params: [&(dyn ToSql + Sync); 4] = [&"1", &42_i32, &"west", &"hidden-value"];
    tx.execute(&insert, &params)
        .await
        .expect("insert shadow row");
    tx.commit().await.expect("insert commit");
    Shadow {
        table,
        order_id,
        region,
        secret,
    }
}

async fn register(
    client: &Client,
    generation: i64,
    level: &str,
    letter: &str,
    namespace: &str,
    table: &str,
    column: &str,
) {
    let payload: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &CONFIG,
        &generation,
        &letter,
        &namespace,
        &table,
        &column,
    ];
    let digest: Vec<u8> = client
        .query_one(
            "SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, $3, $4, $5, $6, $7))",
            &payload,
        )
        .await
        .expect("digest")
        .get(0);
    let bind: [&(dyn ToSql + Sync); 8] = [
        &SOURCE,
        &CONFIG,
        &generation,
        &level,
        &namespace,
        &table,
        &column,
        &digest,
    ];
    client
        .execute(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &bind,
        )
        .await
        .expect("register");
}

async fn lookup(
    client: &Client,
    level: &str,
    namespace: &str,
    table: &str,
    column: &str,
) -> String {
    let params: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &CONFIG,
        &CURRENT_GENERATION,
        &level,
        &namespace,
        &table,
        &column,
    ];
    client
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &params,
        )
        .await
        .expect("lookup")
        .get(0)
}

fn quote_physical(name: &str, prefix: &str) -> String {
    let rest = name.strip_prefix(prefix).unwrap_or("");
    assert_eq!(name.len(), 63, "physical length");
    assert_eq!(rest.len(), 60, "physical body");
    assert!(
        rest.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "physical hex"
    );
    format!("\"{name}\"")
}
