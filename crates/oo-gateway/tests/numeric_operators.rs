//! Offline admission and worker regression. No database or service is started.

use std::io::Write;
use std::process::{Command, Stdio};

use oo_core::api::ErrorCode;
use oo_gateway::parser_worker::{
    AdmitRequest, CodecError, KIND_PLAN, KIND_REQUEST, PROTOCOL_VERSION, decode_frame, decode_plan,
    decode_request, encode_frame, encode_request,
};
use oo_gateway::policy::catalog::{ARITHMETIC_OPERATORS, ARITHMETIC_SIGNATURES};
use oo_gateway::{
    GuardError, GuardedPlan, LiteralTypes, PolicyCatalog, PolicyIdentity, ResolvedFunction,
    ResolvedOperator, VisibleColumn, VisibleRelation, guard_select,
};
use postgres_types::Type;

const FREIGHT: &str =
    "SELECT SUM(amount_freight-amount_transfer-amount_dff) FROM logistics.shipment";

fn column(name: &str, type_oid: u32) -> VisibleColumn {
    VisibleColumn {
        name: name.into(),
        physical_name: format!("pc_{name}"),
        type_oid,
    }
}

fn operator(
    oid: u32,
    procedure_oid: u32,
    name: &str,
    left: u32,
    right: u32,
    result: u32,
) -> ResolvedOperator {
    ResolvedOperator {
        oid,
        procedure_oid,
        schema: "pg_catalog".into(),
        name: name.into(),
        left_type_oid: left,
        right_type_oid: right,
        result_type_oid: result,
    }
}

fn catalog() -> PolicyCatalog {
    // PostgreSQL 17 pg_operator.dat / pg_proc.dat fixtures, not runtime OID resolution.
    PolicyCatalog {
        identity: PolicyIdentity {
            project: "fixture".into(),
            role: "oo_r_reader".into(),
            policy_id: "numeric-v1".into(),
        },
        relations: vec![VisibleRelation {
            schema: "logistics".into(),
            name: "shipment".into(),
            physical_schema: "_oo".into(),
            physical_name: "pt_shipment".into(),
            columns: vec![
                column("amount_freight", 1700),
                column("amount_transfer", 1700),
                column("amount_dff", 1700),
                column("label", 25),
                column("integer_col", 23),
                column("active", 16),
            ],
            pending_unclassified: 1,
            estimated_rows: 10,
        }],
        functions: vec![ResolvedFunction {
            oid: 2114,
            schema: "pg_catalog".into(),
            name: "sum".into(),
            argument_type_oids: vec![1700],
            aggregate_star: false,
        }],
        operators: vec![
            operator(1758, 1724, "+", 1700, 1700, 1700),
            operator(1759, 1725, "-", 1700, 1700, 1700),
            operator(1760, 1726, "*", 1700, 1700, 1700),
            operator(1761, 1727, "/", 1700, 1700, 1700),
            operator(1754, 1720, ">", 1700, 1700, 16),
            operator(1752, 1718, "=", 1700, 1700, 16),
        ],
        literal_types: LiteralTypes {
            integer: Some(23),
            text: Some(25),
            boolean: Some(16),
        },
        null_test_allowed: true,
        large_table_row_threshold: 100,
    }
}

fn admit(catalog: &PolicyCatalog, sql: &str) -> GuardedPlan {
    guard_select(&catalog.identity, catalog, sql).unwrap_or_else(|error| panic!("{sql}: {error:?}"))
}

#[test]
fn freight_sum_propagates_numeric_results_and_keeps_the_actual_bindings() {
    let catalog = catalog();
    let plan = admit(&catalog, FREIGHT);
    assert_eq!(plan.bindings.functions[0].oid, 2114);
    assert_eq!(plan.bindings.operators.len(), 2);
    assert!(
        plan.bindings
            .operators
            .iter()
            .all(|op| { op.oid == 1759 && op.procedure_oid == 1725 && op.schema == "pg_catalog" })
    );
    assert!(plan.executed_sql.contains("pc_amount_freight -"));
    assert!(plan.executed_sql.contains("pc_amount_transfer"));
    assert!(plan.executed_sql.contains("pc_amount_dff"));
    assert!(plan.executed_sql.contains("pt_shipment"));

    for expression in [
        "amount_freight + amount_transfer - amount_dff",
        "amount_freight - (amount_transfer - amount_dff)",
        "(amount_freight - amount_transfer) * amount_dff / amount_freight",
        "amount_freight OPERATOR(pg_catalog.-) amount_transfer",
        "amount_freight - 1 - 2",
        "1 + amount_freight - -2",
        "(amount_freight + 1) / 2",
    ] {
        let sql = format!("SELECT SUM({expression}) FROM logistics.shipment");
        admit(&catalog, &sql);
    }
    // The result must not accidentally bind to an aggregate expecting bool.
    let mut wrong = catalog.clone();
    wrong.functions[0].argument_type_oids = vec![16];
    assert!(matches!(
        guard_select(&wrong.identity, &wrong, FREIGHT),
        Err(GuardError::SignatureNotAllowed { operator: false, argument_type_oids, .. })
            if argument_type_oids == [1700]
    ));
}

#[test]
fn arithmetic_is_not_a_predicate_but_comparison_results_remain_bool() {
    let catalog = catalog();
    for condition in [
        "amount_freight - amount_transfer > 0",
        "0 > amount_freight - amount_transfer",
        "active AND NOT (amount_freight - amount_transfer > 0)",
        "amount_freight = amount_transfer OR active",
        "amount_freight IN (0, 1, NULL)",
    ] {
        admit(&catalog, &format!("{FREIGHT} WHERE {condition}"));
    }
    for condition in [
        "amount_freight - amount_transfer",
        "NOT (amount_freight + 1)",
        "(amount_freight * amount_transfer) AND active",
        "active OR (amount_freight / amount_dff)",
    ] {
        assert!(
            matches!(
                guard_select(
                    &catalog.identity,
                    &catalog,
                    &format!("{FREIGHT} WHERE {condition}")
                ),
                Err(GuardError::Unsupported {
                    construct: "predicate"
                })
            ),
            "{condition}"
        );
    }
    let join = "SELECT s.amount_freight FROM logistics.shipment s \
                JOIN logistics.shipment t ON s.amount_freight - t.amount_freight";
    assert!(matches!(
        guard_select(&catalog.identity, &catalog, join),
        Err(GuardError::Unsupported {
            construct: "predicate"
        })
    ));
    // Comparisons are bool, not arithmetic input or numeric SUM input.
    for expression in [
        "(amount_freight > 0) - amount_dff",
        "SUM(amount_freight > 0)",
    ] {
        let sql = format!("SELECT {expression} FROM logistics.shipment");
        assert_eq!(
            guard_select(&catalog.identity, &catalog, &sql)
                .unwrap_err()
                .code(),
            ErrorCode::FunctionNotAllowed
        );
    }
}

#[test]
fn arithmetic_stays_closed_for_strings_unsafe_calls_and_unresolved_signatures() {
    let catalog = catalog();
    for expression in [
        "label + label",
        "amount_freight - '1'",
        "'1' + amount_freight",
        "amount_freight - integer_col",
        "active + active",
        "amount_freight || amount_transfer",
        "amount_freight % amount_transfer",
        "amount_freight ^ amount_transfer",
        "amount_freight - 1.5",
        "-amount_freight",
        "amount_freight::text - label",
        "amount_freight + pg_sleep(1)",
        "amount_freight + nextval('sequence')",
        "amount_freight OPERATOR(public.-) amount_transfer",
        "amount_freight - (SELECT 1)",
    ] {
        let sql = format!("SELECT {expression} FROM logistics.shipment");
        assert!(
            guard_select(&catalog.identity, &catalog, &sql).is_err(),
            "{sql}"
        );
    }
    let hidden = FREIGHT.replace("amount_dff", "hidden");
    assert!(matches!(
        guard_select(&catalog.identity, &catalog, &hidden),
        Err(GuardError::ColumnNotAllowed { .. })
    ));
    let mut missing = catalog.clone();
    missing.operators.retain(|op| op.name != "-");
    assert_eq!(
        guard_select(&missing.identity, &missing, FREIGHT)
            .unwrap_err()
            .code(),
        ErrorCode::FunctionNotAllowed
    );
}

#[test]
fn exact_supported_numeric_signatures_propagate_their_declared_result() {
    let oid = |name| match name {
        "int2" => Type::INT2.oid(),
        "int4" => Type::INT4.oid(),
        "int8" => Type::INT8.oid(),
        "numeric" => Type::NUMERIC.oid(),
        "float8" => Type::FLOAT8.oid(),
        _ => panic!("type"),
    };
    for &(left, right, result) in ARITHMETIC_SIGNATURES {
        for name in ARITHMETIC_OPERATORS {
            let mut catalog = catalog();
            catalog.relations[0].columns = vec![column("a", oid(left)), column("b", oid(right))];
            catalog.operators = vec![operator(
                8001,
                9001,
                name,
                oid(left),
                oid(right),
                oid(result),
            )];
            // Type-checking oracle only: this aggregate fixture does not assert
            // that every numeric SUM overload is published by the live loader.
            catalog.functions[0].argument_type_oids = vec![oid(result)];
            let sql = format!("SELECT SUM(a {name} b) FROM logistics.shipment");
            let plan = admit(&catalog, &sql);
            assert_eq!(plan.bindings.operators[0].oid, 8001);
            assert_eq!(plan.bindings.operators[0].procedure_oid, 9001);
        }
    }
}

#[test]
fn malformed_arithmetic_catalogs_fail_closed() {
    for field in [
        "oid",
        "procedure",
        "left",
        "right",
        "result",
        "schema",
        "text",
        "float4",
        "mixed",
        "wrong_result",
    ] {
        let mut catalog = catalog();
        let op = &mut catalog.operators[1];
        match field {
            "oid" => op.oid = 0,
            "procedure" => op.procedure_oid = 0,
            "left" => op.left_type_oid = 0,
            "right" => op.right_type_oid = 0,
            "result" => op.result_type_oid = 0,
            "schema" => op.schema = "public".into(),
            "text" => {
                op.left_type_oid = 25;
                op.right_type_oid = 25;
                op.result_type_oid = 25;
            }
            "float4" => {
                op.left_type_oid = 700;
                op.right_type_oid = 700;
                op.result_type_oid = 700;
            }
            "mixed" => op.right_type_oid = 23,
            "wrong_result" => op.result_type_oid = 16,
            _ => unreachable!(),
        }
        assert!(
            matches!(
                guard_select(&catalog.identity, &catalog, FREIGHT),
                Err(GuardError::CatalogInvalid { .. })
            ),
            "{field}"
        );
    }
    let mut catalog = catalog();
    catalog.operators[4].result_type_oid = 1700;
    assert!(matches!(
        guard_select(&catalog.identity, &catalog, FREIGHT),
        Err(GuardError::CatalogInvalid {
            reason: "operator signature"
        })
    ));
    catalog = self::catalog();
    catalog.operators.push(catalog.operators[1].clone());
    assert!(matches!(
        guard_select(&catalog.identity, &catalog, FREIGHT),
        Err(GuardError::CatalogInvalid {
            reason: "duplicate operator"
        })
    ));
}

#[test]
fn numeric_worker_roundtrip_and_binary_preserve_types_and_reject_v1() {
    let catalog = catalog();
    let request = AdmitRequest {
        identity: catalog.identity.clone(),
        catalog,
        sql: FREIGHT.into(),
    };
    let body = encode_request(&request).unwrap();
    let decoded = decode_request(&body).unwrap();
    assert_eq!(decoded, request);
    assert_eq!(decoded.catalog.operators[1].result_type_oid, 1700);
    assert_eq!(decoded.catalog.operators[4].result_type_oid, 16);
    let frame = encode_frame(KIND_REQUEST, &body).unwrap();
    let run = |frame: &[u8]| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_oo-sql-parser-worker"))
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(frame).unwrap();
        child.wait_with_output().unwrap()
    };
    let output = run(&frame);
    assert!(output.status.success(), "{:?}", output.stderr);
    let response = decode_frame(&output.stdout).unwrap();
    assert_eq!(response.version, PROTOCOL_VERSION);
    assert_eq!(response.kind, KIND_PLAN);
    assert_eq!(
        decode_plan(&response.body).unwrap(),
        admit(&request.catalog, FREIGHT)
    );
    let mut old = frame;
    old[4..6].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(decode_frame(&old), Err(CodecError::Version));
    let output = run(&old);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
