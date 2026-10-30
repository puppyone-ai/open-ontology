//! Pure guard coverage; live pg_catalog and PostgreSQL results have an owned oracle.

use oo_gateway::policy::catalog::{
    ARITHMETIC_OPERATORS, ARITHMETIC_SIGNATURES, COMPARISON_OPERATORS, COMPARISON_SIGNATURES,
    DAY_FUNCTION_SIGNATURES, FUNCTION_CATALOG_LIMIT, UNARY_AGGREGATE_SIGNATURES,
    ADDITIONAL_AGGREGATE_SIGNATURES, ADDITIONAL_SCALAR_SIGNATURES,
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
            oid: 200 + functions.len() as u32, schema: "pg_catalog".into(), name: name.to_string(),
            argument_type_oids: vec![oid(argument)], aggregate_star: false,
        });
    }
    for (name, arguments, _) in ADDITIONAL_SCALAR_SIGNATURES {
        functions.push(ResolvedFunction {
            oid: 200 + functions.len() as u32, schema: "pg_catalog".into(), name: name.to_string(),
            argument_type_oids: arguments.iter().map(|name| oid(name)).collect(), aggregate_star: false,
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
        null_test_allowed: true,
        large_table_row_threshold: 100,
    }
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
        "SELECT date_trunc('hour',stamp) FROM logistics.orders",
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
        ] {
            let sql = format!("SELECT id FROM logistics.orders WHERE {left}>={right}");
            let plan = guard_select(&cat.identity, &cat, &sql).unwrap();
            assert_eq!(plan.original_sql, sql);
            assert!(plan.executed_sql.contains(&format!("{date} 00:00:00")), "{}", plan.executed_sql);
            assert_eq!(plan.bindings.operators.len(), 1);
        }
    }
    for sql in [
        "SELECT id FROM logistics.orders WHERE stamp>=TIMESTAMP '2026-02-29'",
        "SELECT id FROM logistics.orders WHERE stamp>=TIMESTAMP '2026-02-31'",
        "SELECT id FROM logistics.orders WHERE stamp>=TIMESTAMP '0000-01-01'",
        "SELECT id FROM logistics.orders WHERE stamp>=TIMESTAMP '2026-10-28Z'",
        "SELECT id FROM logistics.orders WHERE stamp>=TIMESTAMP 'today'",
        "SELECT id FROM logistics.orders WHERE stamp>='2026-10-28'",
        "SELECT id FROM logistics.orders WHERE stamp>=code::timestamp",
        "SELECT id FROM logistics.orders WHERE stamp>=TIMESTAMP(0) '2026-10-28'",
        "SELECT id FROM logistics.orders WHERE zoned>=TIMESTAMPTZ '2026-10-28'",
    ] {
        assert!(guard_select(&cat.identity, &cat, sql).is_err(), "{sql}");
    }
    let mut missing = cat;
    missing.operators.retain(|op| op.left_type_oid != Type::TIMESTAMP.oid());
    assert!(guard_select(&missing.identity, &missing,
        "SELECT id FROM logistics.orders WHERE stamp>=TIMESTAMP '2026-10-28'").is_err());
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
        "SELECT abs(id) FROM logistics.orders",
        "SELECT abs(code) FROM logistics.orders",
        "SELECT abs(amount) OVER () FROM logistics.orders",
        "SELECT abs(amount) FILTER (WHERE true) FROM logistics.orders",
    ] {
        assert!(guard_select(&cat.identity, &cat, sql).is_err(), "{sql}");
    }
    for signature in [("sum", oid("int4")), ("abs", oid("numeric"))] {
        let mut missing = cat.clone();
        missing.functions.retain(|f| !(f.name == signature.0 && f.argument_type_oids == [signature.1]));
        let sql = if signature.0 == "sum" {
            "SELECT SUM(CASE WHEN flag THEN 1 ELSE 0 END) FROM logistics.orders"
        } else {
            "SELECT COUNT(*) FROM logistics.orders HAVING SUM(abs(amount))>0"
        };
        assert!(guard_select(&missing.identity, &missing, sql).is_err());
    }
    let arms = " WHEN flag THEN 1".repeat(oo_gateway::sql_guard::CASE_ARM_LIMIT + 1);
    assert_eq!(guard_select(&cat.identity, &cat, &format!("SELECT CASE{arms} ELSE 0 END FROM logistics.orders")),
        Err(oo_gateway::GuardError::Budget));
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
    assert!(guard_select(&missing.identity, &missing,
        "SELECT to_char(stamp,'YYYY-MM-DD') FROM logistics.orders").is_err());
}
