//! Pure guard coverage; live pg_catalog and PostgreSQL results have an owned oracle.

use oo_gateway::policy::catalog::{
    ADDITIONAL_AGGREGATE_SIGNATURES, ADDITIONAL_SCALAR_SIGNATURES, ARITHMETIC_OPERATORS,
    ARITHMETIC_SIGNATURES, COMPARISON_OPERATORS, COMPARISON_SIGNATURES, DAY_FUNCTION_SIGNATURES,
    FUNCTION_CATALOG_LIMIT, UNARY_AGGREGATE_SIGNATURES,
};
use oo_gateway::{
    LiteralTypes, PolicyCatalog, PolicyIdentity, ResolvedFunction, ResolvedOperator, VisibleColumn,
    VisibleRelation, guard_select,
};
use postgres_types::Type;

fn oid(name: &str) -> u32 {
    [
        Type::INT2,
        Type::INT4,
        Type::INT8,
        Type::TEXT,
        Type::VARCHAR,
        Type::BPCHAR,
        Type::NUMERIC,
        Type::FLOAT8,
        Type::BOOL,
        Type::DATE,
        Type::TIME,
        Type::TIMESTAMP,
        Type::TIMESTAMPTZ,
        Type::INTERVAL,
    ]
    .into_iter()
    .find(|kind| kind.name() == name)
    .unwrap()
    .oid()
}

fn catalog() -> PolicyCatalog {
    let mut functions = vec![
        ResolvedFunction {
            oid: 200,
            schema: "pg_catalog".into(),
            name: "count".into(),
            argument_type_oids: vec![],
            aggregate_star: true,
        },
        ResolvedFunction {
            oid: 201,
            schema: "pg_catalog".into(),
            name: "count".into(),
            argument_type_oids: vec![Type::ANY.oid()],
            aggregate_star: false,
        },
    ];
    for (name, argument, _) in UNARY_AGGREGATE_SIGNATURES {
        functions.push(ResolvedFunction {
            oid: 200 + functions.len() as u32,
            schema: "pg_catalog".into(),
            name: name.to_string(),
            argument_type_oids: vec![oid(argument)],
            aggregate_star: false,
        });
    }
    for (name, arguments, _) in DAY_FUNCTION_SIGNATURES {
        functions.push(ResolvedFunction {
            oid: 200 + functions.len() as u32,
            schema: "pg_catalog".into(),
            name: name.to_string(),
            argument_type_oids: arguments.iter().map(|name| oid(name)).collect(),
            aggregate_star: false,
        });
    }
    for (name, argument, _) in ADDITIONAL_AGGREGATE_SIGNATURES {
        functions.push(ResolvedFunction {
            oid: 200 + functions.len() as u32,
            schema: "pg_catalog".into(),
            name: name.to_string(),
            argument_type_oids: vec![oid(argument)],
            aggregate_star: false,
        });
    }
    for (name, arguments, _) in ADDITIONAL_SCALAR_SIGNATURES {
        functions.push(ResolvedFunction {
            oid: 200 + functions.len() as u32,
            schema: "pg_catalog".into(),
            name: name.to_string(),
            argument_type_oids: arguments.iter().map(|name| oid(name)).collect(),
            aggregate_star: false,
        });
    }
    assert_eq!(functions.len(), FUNCTION_CATALOG_LIMIT as usize);
    let mut operators = Vec::new();
    for (left, right) in COMPARISON_SIGNATURES {
        for name in COMPARISON_OPERATORS {
            operators.push(ResolvedOperator {
                oid: 1000 + operators.len() as u32,
                procedure_oid: 2000 + operators.len() as u32,
                schema: "pg_catalog".into(),
                name: name.to_string(),
                left_type_oid: oid(left),
                right_type_oid: oid(right),
                result_type_oid: Type::BOOL.oid(),
            });
        }
    }
    for (left, right, result) in ARITHMETIC_SIGNATURES {
        for name in ARITHMETIC_OPERATORS {
            operators.push(ResolvedOperator {
                oid: 1000 + operators.len() as u32,
                procedure_oid: 2000 + operators.len() as u32,
                schema: "pg_catalog".into(),
                name: name.to_string(),
                left_type_oid: oid(left),
                right_type_oid: oid(right),
                result_type_oid: oid(result),
            });
        }
    }
    PolicyCatalog {
        identity: PolicyIdentity {
            project: "fixture".into(),
            role: "reader".into(),
            policy_id: "v1".into(),
        },
        relations: vec![VisibleRelation {
            schema: "logistics".into(),
            name: "orders".into(),
            physical_schema: "_oo".into(),
            physical_name: "pt_orders".into(),
            columns: [
                ("id", "int4"),
                ("stamp", "timestamp"),
                ("zoned", "timestamptz"),
                ("day", "date"),
                ("code", "varchar"),
                ("amount", "numeric"),
                ("flag", "bool"),
            ]
            .into_iter()
            .map(|(name, kind)| VisibleColumn {
                name: name.into(),
                physical_name: format!("pc_{name}"),
                type_oid: oid(kind),
            })
            .collect(),
            pending_unclassified: 1,
            estimated_rows: 4,
        }],
        functions,
        operators,
        literal_types: LiteralTypes {
            integer: Some(oid("int4")),
            text: Some(oid("text")),
            boolean: Some(oid("bool")),
        },
        casts: Vec::new(),
        null_test_allowed: true,
        large_table_row_threshold: 100,
    }
}

#[test]
fn temporal_comparisons_bind_only_iso_literals_to_verified_operators() {
    let cat = catalog();
    for (column, literal, oid) in [
        ("day", "'2024-02-29'", Type::DATE.oid()),
        ("stamp", "'2024-02-29'", Type::TIMESTAMP.oid()),
        (
            "stamp",
            "'2024-02-29 23:59:59.123456'",
            Type::TIMESTAMP.oid(),
        ),
        ("zoned", "'2024-02-29T23:59:59Z'", Type::TIMESTAMPTZ.oid()),
        (
            "zoned",
            "'2024-02-29 23:59:59+08:00'",
            Type::TIMESTAMPTZ.oid(),
        ),
    ] {
        for operator in COMPARISON_OPERATORS {
            for predicate in [
                format!("{column} {operator} {literal}"),
                format!("{literal} {operator} {column}"),
            ] {
                let sql = format!("SELECT id FROM logistics.orders WHERE {predicate}");
                let plan = guard_select(&cat.identity, &cat, &sql)
                    .unwrap_or_else(|error| panic!("{sql}: {error:?}"));
                assert!(plan.executed_sql.contains(&format!("pc_{column}")));
                assert_eq!(plan.bindings.operators.len(), 1);
                let binding = &plan.bindings.operators[0];
                assert!(cat.operators.iter().any(|op| op.oid == binding.oid
                    && op.left_type_oid == oid
                    && op.right_type_oid == oid));
                let mut missing = cat.clone();
                missing.operators.retain(|op| op.oid != binding.oid);
                assert!(guard_select(&missing.identity, &missing, &sql).is_err());
            }
        }
    }
    for predicate in [
        "day = 'today'",
        "day = '2026-02-29'",
        "day = '02/03/2026'",
        "stamp = 'infinity'",
        "stamp = '2026-01-01 25:00:00'",
        "stamp = '2026-01-01 00:00:00Z'",
        "zoned = '2026-01-01'",
        "zoned = '2026-01-01 00:00:00'",
        "zoned = '2026-01-01 00:00:00 America/New_York'",
        "day = code",
        "code = day",
        "secret = '2026-01-01'",
        "secret::date = '2026-01-01'",
        "day = set_config('TimeZone','UTC',false)",
    ] {
        let sql = format!("SELECT id FROM logistics.orders WHERE {predicate}");
        assert!(guard_select(&cat.identity, &cat, &sql).is_err(), "{sql}");
    }
}

#[test]
fn between_and_temporal_in_keep_bounds_nulls_and_catalog_authority() {
    let cat = catalog();
    for predicate in [
        "day BETWEEN '2026-01-01' AND '2026-01-31'",
        "day NOT BETWEEN DATE '2026-01-01' AND DATE '2026-01-31'",
        "stamp BETWEEN '2026-01-01' AND TIMESTAMP '2026-02-01'",
        "zoned BETWEEN '2026-01-01 00:00:00Z' AND '2026-02-01 00:00:00+00'",
        "id BETWEEN 1 AND 3",
        "amount NOT BETWEEN 1 AND 3",
        "code BETWEEN 'a' AND 'z'",
        "day IN ('2026-01-01', DATE '2026-01-02', NULL)",
        "day NOT IN (NULL, '2026-01-01'::date)",
        "stamp IN ('2026-01-01', TIMESTAMP '2026-01-02')",
        "zoned NOT IN ('2026-01-01 00:00:00Z', NULL)",
    ] {
        let sql = format!("SELECT id FROM logistics.orders WHERE {predicate}");
        let plan = guard_select(&cat.identity, &cat, &sql)
            .unwrap_or_else(|error| panic!("{sql}: {error:?}"));
        assert_eq!(
            plan.executed_sql.contains("NULL"),
            predicate.contains("NULL")
        );
        assert_eq!(plan.executed_sql.contains("NOT"), predicate.contains("NOT"));
        assert_eq!(
            plan.executed_sql.contains("BETWEEN"),
            predicate.contains("BETWEEN")
        );
        for binding in &plan.bindings.operators {
            let mut missing = cat.clone();
            missing.operators.retain(|op| op.oid != binding.oid);
            assert!(
                guard_select(&missing.identity, &missing, &sql).is_err(),
                "{sql}"
            );
        }
    }
    for predicate in [
        "day IN ('today')",
        "day IN ('2026-02-30')",
        "day IN (secret::date)",
        "day IN (code)",
        "day IN (set_config('DateStyle','ISO',false))",
        "day BETWEEN 'today' AND 'tomorrow'",
        "day BETWEEN secret::date AND '2026-01-01'",
        "secret BETWEEN 'a' AND 'z'",
        "'a' BETWEEN 'a' AND 'z'",
    ] {
        let sql = format!("SELECT id FROM logistics.orders WHERE {predicate}");
        assert!(guard_select(&cat.identity, &cat, &sql).is_err(), "{sql}");
    }
}

#[test]
fn analytical_predicates_do_not_open_write_catalog_or_hidden_column_paths() {
    use oo_core::api::ErrorCode;
    let mut cat = catalog();
    for sql in [
        "DELETE FROM logistics.orders WHERE day='2026-01-01'",
        "UPDATE logistics.orders SET code='x'",
        "CREATE TABLE logistics.copy AS SELECT * FROM logistics.orders",
        "COPY logistics.orders TO STDOUT",
        "SELECT * FROM pg_catalog.pg_class",
        "SELECT set_config('role','owner',false) FROM logistics.orders",
        "SELECT dblink('remote','SELECT 1') FROM logistics.orders",
        "SELECT lo_get(1) FROM logistics.orders",
        "SELECT lo_export(1,'output') FROM logistics.orders",
        "SELECT secret::date FROM logistics.orders WHERE day='2026-01-01'",
        "SELECT (SELECT secret FROM logistics.orders)",
        "WITH x AS (SELECT secret FROM logistics.orders) SELECT * FROM x",
        "SELECT id FROM logistics.orders WHERE EXISTS (SELECT secret FROM logistics.orders)",
    ] {
        let error = guard_select(&cat.identity, &cat, sql).expect_err(sql);
        assert!(!error.to_string().contains("secret"), "{sql}: {error}");
        assert_ne!(error.code(), ErrorCode::Internal);
    }
    let mut other_role = cat.identity.clone();
    other_role.role = "owner".into();
    assert!(guard_select(&other_role, &cat, "SELECT id FROM logistics.orders").is_err());
    cat.relations[0].estimated_rows = cat.large_table_row_threshold + 1;
    let error = guard_select(
        &cat.identity,
        &cat,
        "SELECT * FROM logistics.orders a CROSS JOIN logistics.orders b",
    )
    .unwrap_err();
    assert_eq!(error.code(), ErrorCode::CostTooHigh);
}

#[test]
fn timestamp_day_transforms_are_typed_and_keep_visible_names() {
    let cat = catalog();
    for sql in [
        "SELECT date_trunc('day',stamp) FROM logistics.orders",
        "SELECT pg_catalog.date_trunc('day',stamp) AS day_start FROM logistics.orders",
        "SELECT stamp::date FROM logistics.orders",
        "SELECT CAST(stamp AS pg_catalog.date) FROM logistics.orders",
        "SELECT pg_catalog.date(stamp) FROM logistics.orders",
        "SELECT date_trunc('day',stamp)::date FROM logistics.orders",
        "SELECT MIN(stamp::date),MAX(date_trunc('day',stamp)) FROM logistics.orders",
        "SELECT MIN(stamp)::date FROM logistics.orders",
        "SELECT id FROM logistics.orders WHERE stamp::date = DATE '2026-10-28'",
        "SELECT id FROM logistics.orders WHERE date_trunc('day',stamp) >= TIMESTAMP '2026-10-28 00:00:00'",
        "SELECT stamp::date AS d FROM logistics.orders ORDER BY d",
        "SELECT date_trunc('day',stamp),COUNT(*) FROM logistics.orders GROUP BY date_trunc('day',stamp)",
    ] {
        let plan = guard_select(&cat.identity, &cat, sql)
            .unwrap_or_else(|error| panic!("{sql}: {error:?}"));
        assert!(!plan.bindings.functions.is_empty());
        assert!(plan.executed_sql.contains("pc_stamp"));
        assert!(
            plan.bindings
                .functions
                .iter()
                .all(|function| function.schema == "pg_catalog")
        );
    }
    let cast = guard_select(
        &cat.identity,
        &cat,
        "SELECT stamp::date FROM logistics.orders",
    )
    .unwrap();
    assert!(
        cast.executed_sql.contains(" AS stamp"),
        "{}",
        cast.executed_sql
    );
}

#[test]
fn ordinals_resolve_after_visible_star_expansion_and_never_grant_columns() {
    let cat = catalog();
    for sql in [
        "SELECT id,code FROM logistics.orders ORDER BY 2 DESC NULLS LAST,1",
        "SELECT id AS code,code AS id FROM logistics.orders ORDER BY 1,2",
        "SELECT * FROM logistics.orders ORDER BY 7",
        "SELECT code,COUNT(*) AS n FROM logistics.orders GROUP BY 1 ORDER BY 2 DESC,1",
        "SELECT stamp::date AS d,COUNT(*) AS n FROM logistics.orders GROUP BY 1 ORDER BY 1",
        "SELECT date_trunc('day',stamp) AS d,COUNT(*) AS n FROM logistics.orders GROUP BY 1 ORDER BY 2 DESC,1",
        "SELECT amount-1 AS adjusted,COUNT(*) FROM logistics.orders GROUP BY 1 ORDER BY 1",
        "SELECT code,COUNT(*) AS n FROM logistics.orders GROUP BY code ORDER BY n DESC",
        "SELECT 2 FROM logistics.orders ORDER BY 1",
    ] {
        guard_select(&cat.identity, &cat, sql).unwrap_or_else(|error| panic!("{sql}: {error:?}"));
    }
    for sql in [
        "SELECT id FROM logistics.orders ORDER BY 0",
        "SELECT id FROM logistics.orders ORDER BY -1",
        "SELECT id FROM logistics.orders ORDER BY 2",
        "SELECT id FROM logistics.orders ORDER BY 1.5",
        "SELECT id FROM logistics.orders ORDER BY '1'",
        "SELECT * FROM logistics.orders ORDER BY 8",
        "SELECT id FROM logistics.orders ORDER BY id USING >",
        "SELECT secret FROM logistics.orders ORDER BY 1",
        "SELECT code,COUNT(*) FROM logistics.orders GROUP BY 0",
        "SELECT code,COUNT(*) FROM logistics.orders GROUP BY -1",
        "SELECT code,COUNT(*) FROM logistics.orders GROUP BY 2",
        "SELECT code,COUNT(*) FROM logistics.orders GROUP BY 3",
        "SELECT code,COUNT(*) FROM logistics.orders GROUP BY '1'",
        "SELECT code,COUNT(*) FROM logistics.orders GROUP BY COUNT(*)",
        "SELECT code,SUM(amount)+1 FROM logistics.orders GROUP BY 2",
    ] {
        assert!(guard_select(&cat.identity, &cat, sql).is_err(), "{sql}");
    }
}

#[test]
fn having_uses_only_exact_aggregate_results_and_grouped_columns() {
    let cat = catalog();
    for sql in [
        "SELECT code,COUNT(*) FROM logistics.orders GROUP BY 1 HAVING COUNT(*)>1 ORDER BY 2 DESC",
        "SELECT code,SUM(amount) FROM logistics.orders GROUP BY 1 HAVING SUM(amount-0)>0",
        "SELECT code,COUNT(*) FROM logistics.orders GROUP BY 1 HAVING code<>'' AND COUNT(*)>=1",
        "SELECT code,COUNT(*) FROM logistics.orders GROUP BY 1 HAVING code IN ('a','b')",
        "SELECT code,COUNT(*) FROM logistics.orders GROUP BY 1 HAVING code NOT IN ('a',NULL)",
        "SELECT COUNT(*) FROM logistics.orders HAVING COUNT(*)>0",
        "SELECT COUNT(DISTINCT id) FROM logistics.orders HAVING COUNT(DISTINCT id)>0",
        "SELECT 1 FROM logistics.orders HAVING COUNT(*)>0",
        "SELECT SUM(amount) FROM logistics.orders HAVING SUM(amount) IS NOT NULL",
        "SELECT stamp::date,COUNT(*) FROM logistics.orders GROUP BY 1 HAVING stamp::date>=DATE '2026-10-28'",
        "SELECT code,MIN(stamp) FROM logistics.orders GROUP BY 1 HAVING MIN(stamp)>=TIMESTAMP '2026-10-28 00:00:00'",
        "SELECT date_trunc('day',stamp),COUNT(*) FROM logistics.orders GROUP BY stamp HAVING COUNT(*)>0",
        "SELECT code,COUNT(*) FROM logistics.orders GROUP BY 1 HAVING COUNT(*)+COUNT(code)>1",
    ] {
        guard_select(&cat.identity, &cat, sql).unwrap_or_else(|error| panic!("{sql}: {error:?}"));
    }
    for sql in [
        "SELECT code,COUNT(*) FROM logistics.orders HAVING COUNT(*)>0",
        "SELECT code,COUNT(*) FROM logistics.orders GROUP BY 1 HAVING amount>0",
        "SELECT code,COUNT(*) AS n FROM logistics.orders GROUP BY 1 HAVING n>0",
        "SELECT COUNT(*) FROM logistics.orders HAVING COUNT(*)",
        "SELECT COUNT(*) FROM logistics.orders HAVING SUM(COUNT(*))>0",
        "SELECT COUNT(*) FROM logistics.orders WHERE COUNT(*)>0",
        "SELECT COUNT(*) FROM logistics.orders a JOIN logistics.orders b ON COUNT(*)>0",
        "SELECT COUNT(*) FROM logistics.orders HAVING SUM(secret)>0",
        "SELECT COUNT(*) FROM logistics.orders HAVING pg_sleep(1)=0",
        "SELECT COUNT(*) FROM logistics.orders HAVING (SELECT 1)>0",
        "SELECT MIN(stamp) FROM logistics.orders HAVING MIN(stamp)>DATE '2026-10-28'",
        "SELECT code,COUNT(*) FROM logistics.orders GROUP BY 1 ORDER BY amount",
        "SELECT stamp::date,COUNT(*) FROM logistics.orders GROUP BY 1 HAVING stamp>=TIMESTAMP '2026-10-28 00:00:00'",
    ] {
        assert!(guard_select(&cat.identity, &cat, sql).is_err(), "{sql}");
    }
}

#[test]
fn day_operations_and_having_fail_closed_without_catalog_authority() {
    let cat = catalog();
    for sql in [
        "SELECT date_trunc('invalid',stamp) FROM logistics.orders",
        "SELECT date_trunc(code,stamp) FROM logistics.orders",
        "SELECT date_trunc('day',zoned) FROM logistics.orders",
        "SELECT date_trunc('day',zoned,'UTC') FROM logistics.orders",
        "SELECT zoned::date FROM logistics.orders",
        "SELECT code::date FROM logistics.orders",
        "SELECT stamp::public.date FROM logistics.orders",
        "SELECT stamp::date[] FROM logistics.orders",
        "SELECT stamp::timestamp::date FROM logistics.orders",
        "SELECT public.date_trunc('day',stamp) FROM logistics.orders",
        "SELECT date_trunc('day',secret) FROM logistics.orders",
        "SELECT date_trunc('day',stamp) OVER () FROM logistics.orders",
        "SELECT date_trunc('day',stamp) FILTER (WHERE true) FROM logistics.orders",
    ] {
        assert!(guard_select(&cat.identity, &cat, sql).is_err(), "{sql}");
    }
    for (function, sql) in [
        (
            "date_trunc",
            "SELECT date_trunc('day',stamp) FROM logistics.orders",
        ),
        ("date", "SELECT stamp::date FROM logistics.orders"),
        (
            "sum",
            "SELECT COUNT(*) FROM logistics.orders HAVING SUM(amount)>0",
        ),
    ] {
        let mut missing = cat.clone();
        missing.functions.retain(|f| f.name != function);
        assert!(
            guard_select(&missing.identity, &missing, sql).is_err(),
            "{sql}"
        );
    }
    let sql = "SELECT COUNT(*) FROM logistics.orders HAVING SUM(amount)>0";
    let mut missing = cat.clone();
    missing
        .operators
        .retain(|op| !(op.name == ">" && op.left_type_oid == Type::NUMERIC.oid()));
    assert!(guard_select(&missing.identity, &missing, sql).is_err());
}

#[test]
fn temporal_grouping_and_having_survive_the_actual_parser_worker() {
    use oo_gateway::parser_worker::{
        AdmitRequest, KIND_PLAN, KIND_REFUSAL, KIND_REQUEST, decode_frame, decode_plan,
        decode_refusal, decode_request, encode_frame, encode_request,
    };
    use std::io::Write;
    use std::process::{Command, Stdio};

    let cat = catalog();
    for sql in [
        "SELECT date_trunc('day',stamp) AS day_start,COUNT(*) AS n FROM logistics.orders GROUP BY 1 HAVING COUNT(*)>1 ORDER BY 2 DESC,1",
        "SELECT stamp::date,MIN(stamp) FROM logistics.orders GROUP BY 1 HAVING MIN(stamp)>=TIMESTAMP '2026-10-28 00:00:00' ORDER BY 1",
        "SELECT code,SUM(amount) FROM logistics.orders GROUP BY 1 HAVING SUM(amount)>0 ORDER BY 2",
        "SELECT id FROM logistics.orders WHERE stamp>=TIMESTAMP '2026-10-28' AND stamp<TIMESTAMP '2026-10-29'",
        "SELECT id FROM logistics.orders WHERE day BETWEEN '2026-01-01' AND '2026-01-31'",
        "SELECT id FROM logistics.orders WHERE day NOT IN (DATE '2026-01-01', NULL)",
        "SELECT id FROM logistics.orders WHERE stamp>='2026-01-01'",
        "SELECT code,COUNT(*) AS n,SUM(CASE WHEN stamp>=TIMESTAMP '2026-10-29' THEN 1 ELSE 0 END) FROM logistics.orders GROUP BY code",
        "SELECT code,SUM(amount) FROM logistics.orders GROUP BY code HAVING SUM(abs(amount))+SUM(abs(amount))>0",
        "SELECT zoned::date FROM logistics.orders",
        "SELECT code,COUNT(*) FROM logistics.orders GROUP BY 2",
        "SELECT COUNT(*) FROM logistics.orders HAVING SUM(secret)>0",
    ] {
        let request = AdmitRequest {
            identity: cat.identity.clone(),
            catalog: cat.clone(),
            sql: sql.into(),
        };
        let body = encode_request(&request).unwrap();
        assert_eq!(decode_request(&body).unwrap(), request);
        let frame = encode_frame(KIND_REQUEST, &body).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_oo-sql-parser-worker"))
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&frame).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{sql}: {:?}", output.stderr);
        let response = decode_frame(&output.stdout).unwrap();
        match guard_select(&cat.identity, &cat, sql) {
            Ok(expected) => {
                assert_eq!(response.kind, KIND_PLAN);
                assert_eq!(decode_plan(&response.body).unwrap(), expected);
            }
            Err(expected) => {
                assert_eq!(response.kind, KIND_REFUSAL);
                assert_eq!(decode_refusal(&response.body).unwrap(), expected);
            }
        }
    }
}

#[test]
fn date_only_timestamp_constants_normalize_to_midnight_without_widening_casts() {
    let cat = catalog();
    for date in ["2026-10-28", "2024-02-29", "0001-01-01", "9999-12-31"] {
        for (left, right) in [
            ("stamp".to_string(), format!("TIMESTAMP '{date}'")),
            (format!("TIMESTAMP '{date}'"), "stamp".to_string()),
            ("stamp".to_string(), format!("'{date}'")),
            (format!("'{date}'"), "stamp".to_string()),
        ] {
            let sql = format!("SELECT id FROM logistics.orders WHERE {left}>={right}");
            let plan = guard_select(&cat.identity, &cat, &sql).unwrap();
            assert_eq!(plan.original_sql, sql);
            assert!(
                plan.executed_sql.contains(&format!("{date} 00:00:00")),
                "{}",
                plan.executed_sql
            );
            assert_eq!(plan.bindings.operators.len(), 1);
        }
    }
    for sql in [
        "SELECT id FROM logistics.orders WHERE stamp>=TIMESTAMP '2026-02-29'",
        "SELECT id FROM logistics.orders WHERE stamp>=TIMESTAMP '2026-02-31'",
        "SELECT id FROM logistics.orders WHERE stamp>=TIMESTAMP '0000-01-01'",
        "SELECT id FROM logistics.orders WHERE stamp>=TIMESTAMP '2026-10-28Z'",
        "SELECT id FROM logistics.orders WHERE stamp>=TIMESTAMP 'today'",
        "SELECT id FROM logistics.orders WHERE stamp>='today'",
        "SELECT id FROM logistics.orders WHERE stamp>=code::timestamp",
        "SELECT id FROM logistics.orders WHERE stamp>=TIMESTAMP(0) '2026-10-28'",
        "SELECT id FROM logistics.orders WHERE zoned>=TIMESTAMPTZ '2026-10-28'",
    ] {
        assert!(guard_select(&cat.identity, &cat, sql).is_err(), "{sql}");
    }
    let mut missing = cat;
    missing
        .operators
        .retain(|op| op.left_type_oid != Type::TIMESTAMP.oid());
    assert!(
        guard_select(
            &missing.identity,
            &missing,
            "SELECT id FROM logistics.orders WHERE stamp>=TIMESTAMP '2026-10-28'"
        )
        .is_err()
    );
}

#[test]
fn searched_numeric_case_and_abs_use_exact_function_results() {
    let cat = catalog();
    for sql in [
        "SELECT code,COUNT(*) AS n,SUM(CASE WHEN stamp>=TIMESTAMP '2026-10-29 00:00:00' THEN 1 ELSE 0 END) AS upd_today FROM logistics.orders GROUP BY code",
        "SELECT code,SUM(CASE WHEN stamp>=TIMESTAMP '2026-10-29' THEN 1 ELSE 0 END) AS n FROM logistics.orders GROUP BY 1 HAVING SUM(CASE WHEN stamp>=TIMESTAMP '2026-10-29' THEN 1 ELSE 0 END)>0 ORDER BY 2",
        "SELECT SUM(CASE WHEN flag THEN 1 END) FROM logistics.orders",
        "SELECT SUM(CASE WHEN flag THEN NULL ELSE 1 END) FROM logistics.orders",
        "SELECT SUM(CASE WHEN flag THEN amount ELSE amount END) FROM logistics.orders",
        "SELECT CASE WHEN flag THEN 1 WHEN id>2 THEN 2 ELSE 0 END AS bucket,COUNT(*) FROM logistics.orders GROUP BY 1 ORDER BY bucket",
        "SELECT CASE WHEN COUNT(*)>0 THEN 1 ELSE 0 END FROM logistics.orders",
        "SELECT CASE WHEN flag THEN CASE WHEN id>1 THEN 1 ELSE 0 END ELSE 0 END FROM logistics.orders",
        "SELECT code,SUM(amount) FROM logistics.orders GROUP BY code HAVING SUM(abs(amount))+SUM(abs(amount))+SUM(abs(amount))>0",
        "SELECT abs(amount) FROM logistics.orders",
    ] {
        guard_select(&cat.identity, &cat, sql).unwrap_or_else(|error| panic!("{sql}: {error:?}"));
    }
    for sql in [
        "SELECT CASE id WHEN 1 THEN 1 ELSE 0 END FROM logistics.orders",
        "SELECT CASE WHEN id THEN 1 ELSE 0 END FROM logistics.orders",
        "SELECT CASE WHEN flag THEN 1 ELSE amount END FROM logistics.orders",
        "SELECT CASE WHEN flag THEN 1 ELSE '1' END FROM logistics.orders",
        "SELECT CASE WHEN flag THEN NULL ELSE NULL END FROM logistics.orders",
        "SELECT CASE WHEN flag THEN stamp ELSE stamp END FROM logistics.orders",
        "SELECT CASE WHEN false THEN secret ELSE 1 END FROM logistics.orders",
        "SELECT CASE WHEN false THEN pg_sleep(1) ELSE 1 END FROM logistics.orders",
        "SELECT CASE WHEN flag THEN (SELECT 1) ELSE 0 END FROM logistics.orders",
        "SELECT SUM(CASE WHEN flag THEN SUM(amount) ELSE amount END) FROM logistics.orders",
        "SELECT id FROM logistics.orders WHERE CASE WHEN flag THEN 1 ELSE 0 END",
        "SELECT code,CASE WHEN flag THEN 1 ELSE 0 END,COUNT(*) FROM logistics.orders GROUP BY code",
        "SELECT public.abs(amount) FROM logistics.orders",
        "SELECT abs(flag) FROM logistics.orders",
        "SELECT abs(code) FROM logistics.orders",
        "SELECT abs(amount) OVER () FROM logistics.orders",
        "SELECT abs(amount) FILTER (WHERE true) FROM logistics.orders",
    ] {
        assert!(guard_select(&cat.identity, &cat, sql).is_err(), "{sql}");
    }
    for signature in [("sum", oid("int4")), ("abs", oid("numeric"))] {
        let mut missing = cat.clone();
        missing
            .functions
            .retain(|f| !(f.name == signature.0 && f.argument_type_oids == [signature.1]));
        let sql = if signature.0 == "sum" {
            "SELECT SUM(CASE WHEN flag THEN 1 ELSE 0 END) FROM logistics.orders"
        } else {
            "SELECT COUNT(*) FROM logistics.orders HAVING SUM(abs(amount))>0"
        };
        assert!(guard_select(&missing.identity, &missing, sql).is_err());
    }
    let arms = " WHEN flag THEN 1".repeat(oo_gateway::sql_guard::CASE_ARM_LIMIT + 1);
    assert_eq!(
        guard_select(
            &cat.identity,
            &cat,
            &format!("SELECT CASE{arms} ELSE 0 END FROM logistics.orders")
        ),
        Err(oo_gateway::GuardError::Budget)
    );
}

#[test]
fn timestamp_to_char_requires_exact_builtin_and_literal_format() {
    let cat = catalog();
    for sql in [
        "SELECT to_char(stamp,'YYYY-MM-DD'),COUNT(*) FROM logistics.orders GROUP BY 1 ORDER BY 1",
        "SELECT pg_catalog.to_char(stamp,'YYYY-MM-DD HH24:MI:SS.US') FROM logistics.orders",
    ] {
        guard_select(&cat.identity, &cat, sql).unwrap_or_else(|err| panic!("{sql}: {err:?}"));
    }
    for sql in [
        "SELECT to_char(zoned,'YYYY-MM-DD') FROM logistics.orders",
        "SELECT to_char(stamp,code) FROM logistics.orders",
        "SELECT public.to_char(stamp,'YYYY-MM-DD') FROM logistics.orders",
        "SELECT to_char(secret,'YYYY-MM-DD') FROM logistics.orders",
    ] {
        assert!(guard_select(&cat.identity, &cat, sql).is_err(), "{sql}");
    }
    let mut missing = cat;
    missing.functions.retain(|f| f.name != "to_char");
    assert!(
        guard_select(
            &missing.identity,
            &missing,
            "SELECT to_char(stamp,'YYYY-MM-DD') FROM logistics.orders"
        )
        .is_err()
    );
}

#[test]
fn analytical_scalar_functions_require_exact_catalog_authority() {
    let cat = catalog();
    for expression in [
        "extract(isodow FROM day) BETWEEN 1 AND 5",
        "extract(month FROM day)",
        "extract(year FROM stamp)",
        "date_part('month',stamp)",
        "date_trunc('month',stamp)",
        "lower(code)",
        "upper(code)",
        "length(code)",
        "substring(code FROM 1 FOR 2)",
        "substring(code FROM 2)",
        "coalesce(id,0)",
        "nullif(id,0)",
        "greatest(id,1)",
        "least(id,2)",
        "coalesce(code,'')",
        "nullif(code,'')",
        "greatest(code,'a')",
        "round(amount)",
        "round(amount,2)",
        "abs(id)",
    ] {
        let sql = format!("SELECT {expression} FROM logistics.orders");
        let plan = guard_select(&cat.identity, &cat, &sql)
            .unwrap_or_else(|error| panic!("{sql}: {error:?}"));
        for function in plan.bindings.functions {
            let mut missing = cat.clone();
            missing.functions.retain(|f| f.oid != function.oid);
            assert!(
                guard_select(&missing.identity, &missing, &sql).is_err(),
                "{sql}"
            );
        }
    }
    for expression in [
        "lower(secret)",
        "extract(month FROM secret)",
        "public.lower(code)",
        "set_config('TimeZone','UTC',false)",
        "substring(code FROM '(.*)')",
        "round(flag)",
        "coalesce(id,secret)",
        "greatest(id,secret)",
        "nullif(id,secret)",
        "coalesce(id,pg_sleep(1))",
    ] {
        let sql = format!("SELECT {expression} FROM logistics.orders");
        assert!(guard_select(&cat.identity, &cat, &sql).is_err(), "{sql}");
    }
}

#[test]
fn scalar_casts_bind_arguments_and_exact_conversion_authority() {
    use oo_gateway::sql_guard::ResolvedCast;
    let mut cat = catalog();
    for (source, target) in [
        (oid("numeric"), oid("int8")),
        (0, oid("numeric")),
        (0, oid("timestamp")),
        (oid("int4"), oid("numeric")),
    ] {
        cat.casts.push(ResolvedCast {
            source_type_oid: source,
            target_type_oid: target,
            functions: vec![ResolvedFunction {
                oid: 9000 + cat.casts.len() as u32,
                schema: "pg_catalog".into(),
                name: if target == oid("int8") {
                    "int8"
                } else {
                    "numeric"
                }
                .into(),
                argument_type_oids: vec![source],
                aggregate_star: false,
            }],
        });
    }
    for sql in [
        "SELECT CAST(SUM(amount) AS BIGINT) FROM logistics.orders",
        "SELECT amount::bigint FROM logistics.orders",
        "SELECT '123.45'::numeric,1::numeric,'2026-01-01'::timestamp",
    ] {
        let plan =
            guard_select(&cat.identity, &cat, sql).unwrap_or_else(|e| panic!("{sql}: {e:?}"));
        assert!(!plan.bindings.functions.is_empty());
        let mut missing = cat.clone();
        missing.casts.clear();
        assert!(
            guard_select(&missing.identity, &missing, sql).is_err(),
            "{sql}"
        );
    }
    for sql in [
        "SELECT secret::bigint FROM logistics.orders",
        "SELECT pg_sleep(1)::bigint",
        "SELECT 'pg_class'::regclass",
        "SELECT '1'::public.numeric",
        "SELECT 'now'::timestamp",
        "SELECT '1'::numeric(10,2)",
        "SELECT ARRAY[1]::integer[]",
    ] {
        assert!(guard_select(&cat.identity, &cat, sql).is_err(), "{sql}");
    }
    cat.casts.push(cat.casts[0].clone());
    assert!(guard_select(&cat.identity, &cat, "SELECT 1").is_err());
}
