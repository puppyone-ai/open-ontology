//! Isolated, opt-in proof of the live builtin loader and guarded SQL semantics.

use super::*;
use crate::policy::{TlsEndpoint, connect_require_tls};
use crate::sql_guard::guard_select;
use serde::Deserialize;
use serde_json::json;
use std::fs;
use tokio_postgres::SimpleQueryMessage;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    port: u16,
    database: String,
    user: String,
    nonce: String,
    ca_file: String,
    password_file: String,
    report_path: String,
}

async fn rows(client: &Client, sql: &str) -> Vec<Vec<Option<String>>> {
    client
        .simple_query(sql)
        .await
        .expect(sql)
        .into_iter()
        .filter_map(|message| match message {
            SimpleQueryMessage::Row(row) => Some(
                (0..row.len())
                    .map(|index| row.get(index).map(str::to_owned))
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

async fn oracle(client: &Client, catalog: &PolicyCatalog, sql: &str) -> Value {
    let plan = guard_select(&catalog.identity, catalog, sql).expect(sql);
    let original = rows(client, sql).await;
    let actual = rows(client, &plan.executed_sql).await;
    assert_eq!(actual, original, "{sql}");
    let original_statement = client.prepare(sql).await.unwrap();
    let actual_statement = client.prepare(&plan.executed_sql).await.unwrap();
    let result_types = |statement: &tokio_postgres::Statement| {
        statement
            .columns()
            .iter()
            .map(|column| column.type_().oid())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        result_types(&actual_statement),
        result_types(&original_statement)
    );
    assert_eq!(
        actual_statement
            .columns()
            .iter()
            .map(|column| column.name())
            .collect::<Vec<_>>(),
        original_statement
            .columns()
            .iter()
            .map(|column| column.name())
            .collect::<Vec<_>>(),
        "{sql}",
    );
    json!({
        "sql": sql, "executed_sql": plan.executed_sql, "rows": actual,
        "result_type_oids": result_types(&actual_statement),
        "function_oids": plan.bindings.functions.iter().map(|f| f.oid).collect::<Vec<_>>(),
        "operator_oids": plan.bindings.operators.iter().map(|o| o.oid).collect::<Vec<_>>(),
        "procedure_oids": plan.bindings.operators.iter().map(|o| o.procedure_oid).collect::<Vec<_>>(),
    })
}

#[tokio::test]
#[ignore = "owned local PG only; crates/oo-gateway/tests/sql_capabilities_owned_pg.py"]
async fn owned_sql_capabilities() {
    let path = std::env::var("OO_SQL_CAPABILITIES_PG_CONFIG").expect("owned fixture config");
    let fixture: Fixture = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(fixture.port > 1024 && fixture.port != 5432);
    assert!(fixture.database.starts_with("oo_sql_capabilities_"));
    assert_eq!(fixture.user, "oo_sql_capabilities_reader");
    let endpoint = TlsEndpoint {
        host: "127.0.0.1".into(),
        port: fixture.port,
        database: fixture.database.clone(),
        user: fixture.user.clone(),
    };
    let password = fs::read_to_string(&fixture.password_file).unwrap();
    let ca = fs::read(&fixture.ca_file).unwrap();
    let (mut client, connection) = connect_require_tls(&endpoint, password.trim(), &ca)
        .await
        .unwrap();
    let driver = tokio::spawn(connection);
    require_tls(&client).await.unwrap();
    let actual = client
        .query_one(
            "SELECT current_database(), current_user, inet_server_port(), version(), \
         current_setting('server_version_num'), current_setting('transaction_read_only'), \
         current_setting('oo.sql_capabilities_nonce', true)",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(actual.get::<_, String>(0), fixture.database);
    assert_eq!(actual.get::<_, String>(1), fixture.user);
    assert_eq!(actual.get::<_, i32>(2), i32::from(fixture.port));
    assert_eq!(actual.get::<_, String>(5), "on");
    assert_eq!(actual.get::<_, String>(6), fixture.nonce);
    let flags = client
        .query_one(
            "SELECT rolsuper,rolcreatedb,rolcreaterole,rolinherit,rolreplication,rolbypassrls \
         FROM pg_catalog.pg_roles WHERE rolname=current_user",
            &[],
        )
        .await
        .unwrap();
    for index in 0..6 {
        assert!(!flags.get::<_, bool>(index));
    }
    let privileges = client
        .query_one(
            "SELECT has_schema_privilege(current_user,'capability_fixture','CREATE'), \
         has_table_privilege(current_user,'capability_fixture.shipments','INSERT'), \
         has_table_privilege(current_user,'capability_fixture.shipments','UPDATE'), \
         has_table_privilege(current_user,'capability_fixture.shipments','DELETE'), \
         has_column_privilege(current_user,'capability_fixture.shipments','secret','SELECT')",
            &[],
        )
        .await
        .unwrap();
    for index in 0..5 {
        assert!(!privileges.get::<_, bool>(index));
    }

    let limits = CatalogLimits {
        max_tables: 1,
        max_columns: 32,
        max_functions: FUNCTION_CATALOG_LIMIT,
        max_operators: 128,
        large_table_row_threshold: 100,
        unknown_rows: UnknownRows::Refuse,
    };
    check_limits(&limits, 1).unwrap();
    let tx = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await
        .unwrap();
    tx.batch_execute("SET LOCAL search_path=pg_catalog")
        .await
        .unwrap();
    let types = load_types(&tx).await.unwrap();
    let operators = load_operators(&tx, &types, &limits).await.unwrap();
    for &(name, arguments, result) in ADDITIONAL_SCALAR_SIGNATURES {
        load_scalar_function(&tx, &types, name, arguments, result)
            .await
            .unwrap_or_else(|error| panic!("scalar {name} {arguments:?}: {error:?}"));
    }
    let functions = load_functions(&tx, &types, &limits).await.unwrap();
    assert_eq!(functions.len(), FUNCTION_CATALOG_LIMIT as usize);
    assert_eq!(operators.len(), required_operator_count());
    let mut budgets = Vec::new();
    for budget in [1, 2, 6, 8, 28, 29, 30, 31, 32, FUNCTION_CATALOG_LIMIT] {
        let bounded = load_functions(
            &tx,
            &types,
            &CatalogLimits {
                max_functions: budget,
                ..limits
            },
        )
        .await
        .unwrap();
        assert_eq!(bounded, functions[..budget as usize]);
        budgets.push(json!({"requested": budget, "loaded": bounded.len()}));
    }
    for (name, argument, result) in [
        ("min", "bool", "bool"),
        ("max", "jsonb", "jsonb"),
        ("min", "timestamp", "date"),
        ("abs", "numeric", "numeric"),
        ("sum", "timestamp", "timestamp"),
    ] {
        assert_eq!(
            load_unary_aggregates(&tx, &types, &[(name, argument, result)]).await,
            Err(CatalogError::MissingObject)
        );
    }
    for (name, arguments, result) in [
        ("date_trunc", &["text", "timestamptz"][..], "timestamptz"),
        ("date", &["timestamptz"][..], "date"),
        ("date_trunc", &["text", "timestamp"][..], "date"),
        ("lower", &["bool"][..], "text"),
    ] {
        assert_eq!(
            load_scalar_function(&tx, &types, name, arguments, result).await,
            Err(CatalogError::MissingObject)
        );
    }
    assert_eq!(
        load_unary_aggregates(
            &tx,
            &types,
            &[
                ("min", "timestamp", "timestamp"),
                ("min", "timestamp", "timestamp"),
            ]
        )
        .await,
        Err(CatalogError::AmbiguousObject)
    );
    let mut wrong_type = types.clone();
    wrong_type.insert("timestamp".into(), types["bool"]);
    assert_eq!(
        load_unary_aggregates(&tx, &wrong_type, &[("min", "timestamp", "timestamp")]).await,
        Err(CatalogError::MissingObject)
    );
    let mut signatures = Vec::new();
    for function in &functions {
        let row = tx
            .query_one(
                "SELECT n.nspname,p.proname,p.prokind::text,p.provolatile::text,p.pronargs, \
             p.prorettype::int4,p.prosecdef,p.proretset,p.provariadic::int4,p.proconfig IS NULL, \
             l.lanname,a.aggkind::text,a.aggnumdirectargs, \
             ARRAY(SELECT x::int4 FROM unnest(p.proargtypes) x), \
             EXISTS(SELECT 1 FROM pg_catalog.pg_depend d WHERE d.objid=p.oid \
               AND d.classid='pg_catalog.pg_proc'::regclass AND d.deptype='e') \
             FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace \
             JOIN pg_catalog.pg_language l ON l.oid=p.prolang \
             LEFT JOIN pg_catalog.pg_aggregate a ON a.aggfnoid=p.oid WHERE p.oid=($1::int4)::oid",
                &[&(function.oid as i32)],
            )
            .await
            .unwrap();
        assert_eq!(row.get::<_, String>(0), "pg_catalog");
        assert_eq!(row.get::<_, String>(1), function.name);
        let scalar = matches!(
            function.name.as_str(),
            "date"
                | "date_trunc"
                | "abs"
                | "to_char"
                | "extract"
                | "date_part"
                | "lower"
                | "upper"
                | "length"
                | "substring"
                | "round"
        );
        assert_eq!(row.get::<_, String>(2), if scalar { "f" } else { "a" });
        assert_eq!(
            row.get::<_, String>(3),
            if function.name == "to_char" { "s" } else { "i" }
        );
        assert_eq!(
            row.get::<_, i16>(4) as usize,
            function.argument_type_oids.len()
        );
        assert!(!row.get::<_, bool>(6));
        assert!(!row.get::<_, bool>(7));
        assert_eq!(row.get::<_, i32>(8), 0);
        assert!(row.get::<_, bool>(9));
        assert_eq!(row.get::<_, String>(10), "internal");
        assert_eq!(
            row.get::<_, Option<String>>(11),
            (!scalar).then(|| "n".into())
        );
        assert_eq!(row.get::<_, Option<i16>>(12), (!scalar).then_some(0));
        assert_eq!(
            row.get::<_, Vec<i32>>(13)
                .iter()
                .map(|oid| *oid as u32)
                .collect::<Vec<_>>(),
            function.argument_type_oids
        );
        assert!(!row.get::<_, bool>(14));
        let result_oid = if function.name == "count" {
            types["int8"]
        } else if scalar {
            let (_, _, result) = DAY_FUNCTION_SIGNATURES
                .iter()
                .chain(ADDITIONAL_SCALAR_SIGNATURES)
                .find(|(name, args, _)| {
                    *name == function.name
                        && args.iter().map(|arg| types[*arg]).collect::<Vec<_>>()
                            == function.argument_type_oids
                })
                .unwrap();
            types[*result]
        } else {
            let (_, _, result) = UNARY_AGGREGATE_SIGNATURES
                .iter()
                .chain(ADDITIONAL_AGGREGATE_SIGNATURES)
                .find(|(name, argument, _)| {
                    *name == function.name && function.argument_type_oids == [types[*argument]]
                })
                .unwrap();
            types[*result]
        };
        assert_eq!(row.get::<_, i32>(5) as u32, result_oid);
        assert_eq!(builtin_function_result_type(function), Some(result_oid));
        if function.name == "date" {
            let cast = tx
                .query_one(
                    "SELECT castsource::int4,casttarget::int4,castmethod::text,castcontext::text \
                 FROM pg_catalog.pg_cast WHERE castfunc=($1::int4)::oid",
                    &[&(function.oid as i32)],
                )
                .await
                .unwrap();
            assert_eq!(cast.get::<_, i32>(0) as u32, types["timestamp"]);
            assert_eq!(cast.get::<_, i32>(1) as u32, types["date"]);
            assert_eq!(cast.get::<_, String>(2), "f");
            assert_eq!(cast.get::<_, String>(3), "a");
        }
        signatures.push(json!({"name":function.name,"oid":function.oid,
            "argument_type_oids":function.argument_type_oids,"result_type_oid":result_oid}));
    }
    let columns = tx.query(
        "SELECT a.attname,a.atttypid::int4,t.typname,a.atttypmod \
         FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_type t ON t.oid=a.atttypid \
         WHERE a.attrelid='capability_fixture.shipments'::regclass AND a.attnum>0 \
           AND NOT a.attisdropped AND has_column_privilege(current_user,a.attrelid,a.attnum,'SELECT') \
         ORDER BY a.attnum", &[],
    ).await.unwrap().into_iter().map(|row| {
        assert!(grantable_base_type(&row.get::<_, String>(2), row.get(3)));
        let name: String = row.get(0);
        VisibleColumn { name: name.clone(), physical_name: name, type_oid: row.get::<_, i32>(1) as u32 }
    }).collect::<Vec<_>>();
    assert_eq!(columns.len(), 18);
    tx.commit().await.unwrap();
    client.batch_execute("SET search_path=''").await.unwrap();
    let identity = PolicyIdentity {
        project: "owned_sql_fixture".into(),
        role: fixture.user.clone(),
        policy_id: fixture.nonce.clone(),
    };
    let catalog = PolicyCatalog {
        identity: identity.clone(),
        relations: vec![VisibleRelation {
            schema: "logistics".into(),
            name: "shipment".into(),
            physical_schema: "capability_fixture".into(),
            physical_name: "shipments".into(),
            columns,
            pending_unclassified: 1,
            estimated_rows: 4,
        }],
        functions,
        operators,
        literal_types: LiteralTypes {
            integer: Some(types["int4"]),
            text: Some(types["text"]),
            boolean: Some(types["bool"]),
        },
        null_test_allowed: true,
        large_table_row_threshold: 100,
    };
    let request = crate::parser_worker::AdmitRequest {
        identity: identity.clone(),
        catalog: catalog.clone(),
        sql: "SELECT MIN(stamp),MAX(day) FROM logistics.shipment WHERE code <> ''".into(),
    };
    let encoded = crate::parser_worker::encode_request(&request).unwrap();
    assert_eq!(
        crate::parser_worker::decode_request(&encoded).unwrap(),
        request
    );
    let mut queries = Vec::new();
    for (column, result_type) in [
        ("id", "int4"),
        ("small", "int2"),
        ("big", "int8"),
        ("amount", "numeric"),
        ("fraction", "float8"),
        ("txt", "text"),
        ("code", "text"),
        ("fixed", "bpchar"),
        ("day", "date"),
        ("clock", "time"),
        ("stamp", "timestamp"),
        ("zoned", "timestamptz"),
        ("span", "interval"),
    ] {
        for predicate in ["", " WHERE id=4", " WHERE id=0"] {
            let sql = format!(
                "SELECT MIN({column}),pg_catalog.MAX({column}) FROM logistics.shipment{predicate}"
            );
            let proof = oracle(&client, &catalog, &sql).await;
            assert_eq!(
                proof["result_type_oids"],
                json!([types[result_type], types[result_type]])
            );
            if !predicate.is_empty() && column != "id" {
                assert_eq!(proof["rows"], json!([[null, null]]));
            }
            queries.push(proof);
        }
    }
    for operator in COMPARISON_OPERATORS {
        for (left, right) in [
            ("code", "'a'"),
            ("'a'", "code"),
            ("code", "txt"),
            ("txt", "code"),
            ("code", "code"),
        ] {
            for token in [
                operator.to_string(),
                format!("OPERATOR(pg_catalog.{operator})"),
            ] {
                let sql = format!(
                    "SELECT id FROM logistics.shipment WHERE {left} {token} {right} ORDER BY id"
                );
                let proof = oracle(&client, &catalog, &sql).await;
                let builtin = catalog
                    .operators
                    .iter()
                    .find(|op| {
                        op.name == *operator
                            && op.left_type_oid == types["text"]
                            && op.right_type_oid == types["text"]
                    })
                    .unwrap();
                assert_eq!(proof["operator_oids"], json!([builtin.oid]));
                queries.push(proof);
            }
        }
    }
    for (sql, expected) in [
        (
            "SELECT id FROM logistics.shipment WHERE code != '' ORDER BY id",
            json!([["1"], ["2"], ["3"]]),
        ),
        (
            "SELECT id FROM logistics.shipment WHERE code = 'a ' ORDER BY id",
            json!([["3"]]),
        ),
        (
            "SELECT id FROM logistics.shipment WHERE code IN ('a','z') ORDER BY id",
            json!([["1"], ["2"]]),
        ),
        (
            "SELECT id FROM logistics.shipment WHERE code NOT IN ('a','z') ORDER BY id",
            json!([["3"]]),
        ),
        (
            "SELECT COUNT(*),COUNT(code),COUNT(DISTINCT code) FROM logistics.shipment",
            json!([["4", "3", "3"]]),
        ),
        (
            "SELECT SUM(big),SUM(amount-0),MIN(big),MAX(big) FROM logistics.shipment",
            json!([["600", "6.500003", "-200", "400"]]),
        ),
        (
            "SELECT MIN(stamp),MAX(stamp) FROM logistics.shipment WHERE stamp >= TIMESTAMP '2026-10-28 00:00:00' AND stamp < TIMESTAMP '2026-10-29 00:00:00'",
            json!([["2026-10-28 00:00:00", "2026-10-28 23:59:59.999999"]]),
        ),
    ] {
        let proof = oracle(&client, &catalog, sql).await;
        assert_eq!(proof["rows"], expected, "{sql}");
        queries.push(proof);
    }
    queries.push(oracle(&client, &catalog,
        "SELECT flag,MIN(stamp) AS first_seen,MAX(day) AS last_day FROM logistics.shipment GROUP BY flag ORDER BY first_seen").await);
    let day_queries = [
        "SELECT id FROM logistics.shipment WHERE day='2026-10-28' ORDER BY id",
        "SELECT id FROM logistics.shipment WHERE '2026-10-28'<=day ORDER BY id",
        "SELECT id FROM logistics.shipment WHERE stamp>='2026-10-28' AND stamp<'2026-10-29' ORDER BY id",
        "SELECT id FROM logistics.shipment WHERE '2026-10-28'<=stamp ORDER BY id",
        "SELECT id FROM logistics.shipment WHERE zoned>='2026-10-28 00:00:00Z' ORDER BY id",
        "SELECT id FROM logistics.shipment WHERE day BETWEEN '2026-10-28' AND '2026-10-29' ORDER BY id",
        "SELECT id FROM logistics.shipment WHERE stamp NOT BETWEEN '2026-10-28' AND TIMESTAMP '2026-10-29' ORDER BY id",
        "SELECT id FROM logistics.shipment WHERE id BETWEEN 1 AND 3 ORDER BY id",
        "SELECT id FROM logistics.shipment WHERE code BETWEEN 'a' AND 'z' ORDER BY id",
        "SELECT id FROM logistics.shipment WHERE day IN ('2026-10-28', DATE '2026-10-29', NULL) ORDER BY id",
        "SELECT id FROM logistics.shipment WHERE day NOT IN (NULL, '2026-10-28'::date) ORDER BY id",
        "SELECT id FROM logistics.shipment WHERE stamp IN ('2026-10-28', TIMESTAMP '2026-10-29') ORDER BY id",
        "SELECT date_trunc('day',stamp) FROM logistics.shipment ORDER BY id",
        "SELECT extract(month FROM day),extract(year FROM stamp),date_part('month',stamp) FROM logistics.shipment ORDER BY id",
        "SELECT id FROM logistics.shipment WHERE EXTRACT(ISODOW FROM day) BETWEEN 1 AND 5 ORDER BY id",
        "SELECT coalesce(code,''),nullif(code,'a'),greatest(code,'a'),least(code,'z') FROM logistics.shipment ORDER BY id",
        "SELECT lower(code),upper(code),length(code),substring(code FROM 1 FOR 2),substring(code FROM 2) FROM logistics.shipment ORDER BY id",
        "SELECT date_trunc('month',stamp),date_trunc('hour',stamp) FROM logistics.shipment ORDER BY id",
        "SELECT pg_catalog.date_trunc('day',stamp) AS bucket,COUNT(*) FROM logistics.shipment GROUP BY 1 ORDER BY 1",
        "SELECT stamp::date,COUNT(*) FROM logistics.shipment GROUP BY 1 ORDER BY 1 NULLS LAST",
        "SELECT CAST(stamp AS pg_catalog.date) AS bucket,COUNT(*) AS n FROM logistics.shipment GROUP BY 1 ORDER BY 2 DESC,1",
        "SELECT date(stamp),COUNT(*) FROM logistics.shipment GROUP BY date(stamp) ORDER BY 1",
        "SELECT date_trunc('day',stamp)::date,COUNT(*) FROM logistics.shipment GROUP BY 1 ORDER BY 1",
        "SELECT MIN(stamp::date),MAX(date_trunc('day',stamp)) FROM logistics.shipment",
        "SELECT MIN(stamp)::date,MAX(stamp)::date FROM logistics.shipment",
        "SELECT id FROM logistics.shipment WHERE stamp::date = DATE '2026-10-28' ORDER BY 1",
        "SELECT id FROM logistics.shipment WHERE date_trunc('day',stamp)>=TIMESTAMP '2026-10-29 00:00:00' ORDER BY 1",
        "SELECT id FROM logistics.shipment WHERE stamp>=TIMESTAMP '2026-10-28' AND stamp<TIMESTAMP '2026-10-29' ORDER BY 1",
        "SELECT id FROM logistics.shipment WHERE TIMESTAMP '2026-10-28'<=stamp AND TIMESTAMP '2026-10-29'>stamp ORDER BY 1",
        "SELECT COUNT(*) FROM logistics.shipment WHERE stamp>=TIMESTAMP '2024-02-29'",
        "SELECT COUNT(*) FROM logistics.shipment HAVING MIN(stamp)>=TIMESTAMP '2026-10-28'",
        "SELECT date_trunc('day',stamp),COUNT(*) FROM logistics.shipment GROUP BY 1 HAVING COUNT(*)>1 ORDER BY 1",
        "SELECT stamp::date,COUNT(*) FROM logistics.shipment GROUP BY 1 HAVING stamp::date>=DATE '2026-10-28' ORDER BY 1",
        "SELECT stamp::date,COUNT(*) FROM logistics.shipment GROUP BY 1 HAVING COUNT(*)>0 AND stamp::date IS NOT NULL ORDER BY 1",
        "SELECT code,COUNT(*) AS n FROM logistics.shipment GROUP BY 1 HAVING code<>'' AND COUNT(*)>=1 ORDER BY 2 DESC,1",
        "SELECT code,COUNT(*) FROM logistics.shipment GROUP BY 1 HAVING code IN ('a','z') ORDER BY 1",
        "SELECT code,COUNT(*) FROM logistics.shipment GROUP BY 1 HAVING code NOT IN ('a',NULL) ORDER BY 1",
        "SELECT flag,SUM(amount) AS total FROM logistics.shipment GROUP BY 1 HAVING SUM(amount-0)>0 ORDER BY total",
        "SELECT flag,COUNT(*) FROM logistics.shipment GROUP BY 1 HAVING MIN(stamp)>=TIMESTAMP '2026-10-28 00:00:00' ORDER BY 1",
        "SELECT flag,COUNT(*) FROM logistics.shipment GROUP BY 1 HAVING NOT (COUNT(*)>1) ORDER BY 1",
        "SELECT COUNT(*) FROM logistics.shipment HAVING COUNT(*)>0",
        "SELECT COUNT(*) FROM logistics.shipment HAVING COUNT(*)=0",
        "SELECT 1 FROM logistics.shipment HAVING COUNT(*)>0",
        "SELECT COUNT(DISTINCT code) FROM logistics.shipment HAVING COUNT(DISTINCT code)>1",
        "SELECT SUM(amount) FROM logistics.shipment WHERE id=4 HAVING SUM(amount) IS NULL",
        "SELECT COUNT(*) FROM logistics.shipment WHERE id=0 HAVING COUNT(*)=0",
        "SELECT COUNT(*)+COUNT(code) AS n FROM logistics.shipment HAVING COUNT(*)+COUNT(code)>1 ORDER BY n",
        "SELECT amount-1 AS adjusted,COUNT(*) FROM logistics.shipment GROUP BY 1 ORDER BY adjusted",
        "SELECT code AS id,id AS code FROM logistics.shipment ORDER BY 1,2",
        "SELECT date_trunc('day',stamp),COUNT(*) FROM logistics.shipment GROUP BY date_trunc('day',stamp) ORDER BY 1",
        "SELECT date_trunc('day',stamp),COUNT(*) FROM logistics.shipment GROUP BY stamp ORDER BY 1,2",
    ];
    for sql in day_queries {
        queries.push(oracle(&client, &catalog, sql).await);
    }
    for sql in [
        "SELECT to_char(stamp,'YYYY-MM-DD') AS d,COUNT(*) FROM logistics.shipment GROUP BY 1 ORDER BY 1",
        "SELECT to_char(stamp,'YYYY-MM-DD HH24:MI:SS.US') FROM logistics.shipment ORDER BY id",
        "SELECT pg_catalog.to_char(stamp,'YYYY-MM'),MIN(stamp) FROM logistics.shipment GROUP BY 1 ORDER BY 1",
        "SELECT id FROM logistics.shipment WHERE to_char(stamp,'YYYY-MM-DD')='2026-10-28' ORDER BY id",
    ] {
        queries.push(oracle(&client, &catalog, sql).await);
    }
    for sql in [
        "SELECT to_char(zoned,'YYYY-MM-DD') FROM logistics.shipment",
        "SELECT to_char(stamp,code) FROM logistics.shipment",
        "SELECT public.to_char(stamp,'YYYY-MM-DD') FROM logistics.shipment",
        "SELECT to_char(secret,'YYYY-MM-DD') FROM logistics.shipment",
    ] {
        assert!(
            guard_select(&catalog.identity, &catalog, sql).is_err(),
            "{sql}"
        );
    }
    let day_counts = oracle(
        &client,
        &catalog,
        "SELECT stamp::date,COUNT(*) FROM logistics.shipment GROUP BY 1 ORDER BY 1",
    )
    .await;
    assert_eq!(
        day_counts["rows"],
        json!([["2026-10-28", "2"], ["2026-10-29", "1"], [null, "1"]])
    );
    for timezone in ["UTC", "Asia/Shanghai", "America/New_York"] {
        client
            .batch_execute(&format!("SET TIME ZONE '{timezone}'"))
            .await
            .unwrap();
        let mut proof = oracle(
            &client,
            &catalog,
            "SELECT date_trunc('day',stamp),stamp::date FROM logistics.shipment ORDER BY id",
        )
        .await;
        assert_eq!(
            proof["rows"],
            json!([
                ["2026-10-28 00:00:00", "2026-10-28"],
                ["2026-10-28 00:00:00", "2026-10-28"],
                ["2026-10-29 00:00:00", "2026-10-29"],
                [null, null]
            ])
        );
        proof["session_timezone"] = json!(timezone);
        queries.push(proof);
        let mut midnight = oracle(&client, &catalog,
            "SELECT id FROM logistics.shipment WHERE stamp>=TIMESTAMP '2026-10-28' AND stamp<TIMESTAMP '2026-10-29' ORDER BY 1").await;
        assert_eq!(midnight["rows"], json!([["1"], ["2"]]));
        assert!(
            midnight["executed_sql"]
                .as_str()
                .unwrap()
                .contains("2026-10-28 00:00:00")
        );
        midnight["session_timezone"] = json!(timezone);
        queries.push(midnight);
    }
    client.batch_execute("SET TIME ZONE 'UTC'").await.unwrap();
    let mut refusals = Vec::new();
    let mut daily_catalog = catalog.clone();
    daily_catalog.relations = vec![VisibleRelation {
        schema: "logistics".into(),
        name: "daily_orders".into(),
        physical_schema: "capability_fixture".into(),
        physical_name: "shipments".into(),
        columns: vec![
            VisibleColumn {
                name: "order_status".into(),
                physical_name: "flag".into(),
                type_oid: types["bool"],
            },
            VisibleColumn {
                name: "upd_time".into(),
                physical_name: "stamp".into(),
                type_oid: types["timestamp"],
            },
        ],
        pending_unclassified: 1,
        estimated_rows: 4,
    }];
    let q5 = "SELECT order_status,count(*) AS n,sum(CASE WHEN upd_time >= TIMESTAMP '2026-10-29 00:00:00' THEN 1 ELSE 0 END) AS upd_today FROM logistics.daily_orders GROUP BY order_status ORDER BY order_status";
    let proof = oracle(&client, &daily_catalog, q5).await;
    assert_eq!(
        proof["result_type_oids"],
        json!([types["bool"], types["int8"], types["int8"]])
    );
    assert_eq!(
        proof["rows"],
        json!([["f", "1", "0"], ["t", "2", "1"], [null, "1", "0"]])
    );
    queries.push(proof);
    let balance_columns = client.prepare("SELECT balance_time,total_amount,paid_amount,today_remain_amount FROM logistics.balances")
        .await.unwrap().columns().iter().map(|column| VisibleColumn {
            name: column.name().into(), physical_name: column.name().into(), type_oid: column.type_().oid(),
        }).collect();
    let mut balance_catalog = catalog.clone();
    balance_catalog.relations = vec![VisibleRelation {
        schema: "logistics".into(),
        name: "balances".into(),
        physical_schema: "capability_fixture".into(),
        physical_name: "balances".into(),
        columns: balance_columns,
        pending_unclassified: 0,
        estimated_rows: 4,
    }];
    let q3 = "SELECT balance_time,count(*) AS n,sum(total_amount) AS due,sum(paid_amount) AS paid FROM logistics.balances GROUP BY balance_time HAVING sum(abs(total_amount))+sum(abs(paid_amount))+sum(abs(today_remain_amount)) > 0 ORDER BY balance_time";
    let proof = oracle(&client, &balance_catalog, q3).await;
    assert_eq!(
        proof["rows"],
        json!([["2026-10-29 00:00:00", "2", "6.000000", "-2.000000"]])
    );
    queries.push(proof);
    for (sql, expected, expected_oid) in [
        (
            "SELECT SUM(CASE WHEN stamp>=TIMESTAMP '2026-10-29' THEN 1 ELSE 0 END) FROM logistics.shipment",
            json!([["1"]]),
            types["int8"],
        ),
        (
            "SELECT SUM(CASE WHEN flag THEN 1 END) FROM logistics.shipment",
            json!([["2"]]),
            types["int8"],
        ),
        (
            "SELECT SUM(CASE WHEN flag THEN NULL ELSE 1 END) FROM logistics.shipment",
            json!([["2"]]),
            types["int8"],
        ),
        (
            "SELECT SUM(CASE WHEN flag THEN 1 END) FROM logistics.shipment WHERE id=0",
            json!([[null]]),
            types["int8"],
        ),
        (
            "SELECT SUM(CASE WHEN flag THEN amount ELSE amount END) FROM logistics.shipment",
            json!([["6.500003"]]),
            types["numeric"],
        ),
        (
            "SELECT SUM(abs(amount)) FROM logistics.shipment",
            json!([["11.500005"]]),
            types["numeric"],
        ),
        (
            "SELECT SUM(abs(amount)) FROM logistics.shipment WHERE id=4",
            json!([[null]]),
            types["numeric"],
        ),
        (
            "SELECT SUM(abs(amount)) FROM logistics.shipment WHERE id=0",
            json!([[null]]),
            types["numeric"],
        ),
    ] {
        let proof = oracle(&client, &catalog, sql).await;
        assert_eq!(proof["rows"], expected, "{sql}");
        assert_eq!(proof["result_type_oids"], json!([expected_oid]), "{sql}");
        queries.push(proof);
    }
    for sql in [
        "SELECT flag,SUM(CASE WHEN stamp>=TIMESTAMP '2026-10-29' THEN 1 ELSE 0 END) AS n FROM logistics.shipment GROUP BY 1 HAVING SUM(CASE WHEN stamp>=TIMESTAMP '2026-10-29' THEN 1 ELSE 0 END)>0 ORDER BY 2",
        "SELECT CASE WHEN flag THEN 1 WHEN id>2 THEN 2 ELSE 0 END AS bucket,COUNT(*) FROM logistics.shipment GROUP BY 1 ORDER BY bucket",
        "SELECT CASE WHEN COUNT(*)>0 THEN 1 ELSE 0 END FROM logistics.shipment",
        "SELECT CASE WHEN flag THEN CASE WHEN id>1 THEN 1 ELSE 0 END ELSE 0 END FROM logistics.shipment ORDER BY id",
    ] {
        queries.push(oracle(&client, &catalog, sql).await);
    }
    for sql in [
        "SELECT CASE id WHEN 1 THEN 1 ELSE 0 END FROM logistics.shipment",
        "SELECT CASE WHEN id THEN 1 ELSE 0 END FROM logistics.shipment",
        "SELECT CASE WHEN flag THEN 1 ELSE amount END FROM logistics.shipment",
        "SELECT CASE WHEN flag THEN 1 ELSE '1' END FROM logistics.shipment",
        "SELECT CASE WHEN flag THEN NULL ELSE NULL END FROM logistics.shipment",
        "SELECT CASE WHEN flag THEN stamp ELSE stamp END FROM logistics.shipment",
        "SELECT CASE WHEN false THEN secret ELSE 1 END FROM logistics.shipment",
        "SELECT CASE WHEN false THEN pg_sleep(1) ELSE 1 END FROM logistics.shipment",
        "SELECT CASE WHEN flag THEN (SELECT 1) ELSE 0 END FROM logistics.shipment",
        "SELECT SUM(CASE WHEN flag THEN SUM(amount) ELSE amount END) FROM logistics.shipment",
        "SELECT id FROM logistics.shipment WHERE CASE WHEN flag THEN 1 ELSE 0 END",
        "SELECT code,CASE WHEN flag THEN 1 ELSE 0 END,COUNT(*) FROM logistics.shipment GROUP BY code",
        "SELECT logistics.abs(amount) FROM logistics.shipment",
        "SELECT abs(flag) FROM logistics.shipment",
        "SELECT abs(code) FROM logistics.shipment",
        "SELECT abs(amount) OVER () FROM logistics.shipment",
        "SELECT abs(amount) FILTER (WHERE true) FROM logistics.shipment",
    ] {
        assert!(guard_select(&identity, &catalog, sql).is_err(), "{sql}");
        refusals.push(
            json!({"sql":sql,"guard":"searched numeric CASE / ABS boundary","executed":false}),
        );
    }
    for (name, argument, sql) in [
        (
            "sum",
            types["int4"],
            "SELECT SUM(CASE WHEN flag THEN 1 ELSE 0 END) FROM logistics.shipment",
        ),
        (
            "abs",
            types["numeric"],
            "SELECT SUM(abs(amount)) FROM logistics.shipment",
        ),
    ] {
        let mut missing = catalog.clone();
        missing
            .functions
            .retain(|f| !(f.name == name && f.argument_type_oids == [argument]));
        assert!(guard_select(&identity, &missing, sql).is_err());
        refusals.push(
            json!({"sql":sql,"guard":"required sum(int4) / abs(numeric) removed","executed":false}),
        );
    }
    let pattern_columns = client
        .prepare("SELECT id,label,code FROM logistics.pattern_lookup")
        .await
        .unwrap()
        .columns()
        .iter()
        .map(|column| VisibleColumn {
            name: column.name().into(),
            physical_name: column.name().into(),
            type_oid: column.type_().oid(),
        })
        .collect();
    let mut pattern_catalog = catalog.clone();
    pattern_catalog.relations = vec![VisibleRelation {
        schema: "logistics".into(),
        name: "pattern_lookup".into(),
        physical_schema: "capability_fixture".into(),
        physical_name: "patterns".into(),
        columns: pattern_columns,
        pending_unclassified: 1,
        estimated_rows: 8,
    }];
    for (keyword, token) in [
        ("LIKE", "~~"),
        ("NOT LIKE", "!~~"),
        ("ILIKE", "~~*"),
        ("NOT ILIKE", "!~~*"),
    ] {
        let operator = pattern_catalog
            .operators
            .iter()
            .find(|op| op.name == token)
            .unwrap();
        assert!(pattern_signature_allowed(operator));
        for column in ["label", "code"] {
            for syntax in [keyword.to_string(), format!("OPERATOR(pg_catalog.{token})")] {
                for pattern in [
                    "A%",
                    "%a%",
                    "A_B",
                    r"A\_B",
                    r"A\%B",
                    "",
                    "%",
                    "\u{4e2d}\u{6587}%",
                    r"back\\slash",
                    "_",
                ] {
                    let sql = format!(
                        "SELECT id FROM logistics.pattern_lookup WHERE {column} {syntax} '{pattern}' ORDER BY 1"
                    );
                    let proof = oracle(&client, &pattern_catalog, &sql).await;
                    assert_eq!(proof["operator_oids"], json!([operator.oid]));
                    queries.push(proof);
                }
            }
        }
        let sql = format!("SELECT id FROM logistics.pattern_lookup WHERE code {keyword} '%'");
        let mut missing = pattern_catalog.clone();
        missing.operators.retain(|op| op.name != token);
        assert!(guard_select(&identity, &missing, &sql).is_err());
        refusals
            .push(json!({"sql":sql,"guard":"required pattern operator removed","executed":false}));
    }
    for (predicate, expected) in [
        ("code LIKE 'A%'", json!([["1"], ["3"], ["4"]])),
        ("code ILIKE 'a%'", json!([["1"], ["2"], ["3"], ["4"]])),
        (r"code LIKE 'A\_B'", json!([["3"]])),
        (r"code LIKE 'A\%B'", json!([["4"]])),
        ("code LIKE ''", json!([["7"]])),
        ("code NOT LIKE '%'", json!([])),
    ] {
        let sql = format!("SELECT id FROM logistics.pattern_lookup WHERE {predicate} ORDER BY 1");
        let proof = oracle(&client, &pattern_catalog, &sql).await;
        assert_eq!(proof["rows"], expected);
        queries.push(proof);
    }
    for sql in [
        "SELECT id FROM logistics.pattern_lookup WHERE code LIKE label",
        "SELECT id FROM logistics.pattern_lookup WHERE code LIKE '%' ESCAPE ''",
        "SELECT id FROM logistics.pattern_lookup WHERE code ILIKE '%' ESCAPE '!'",
        "SELECT id FROM logistics.pattern_lookup WHERE code LIKE lower(label)",
        "SELECT id FROM logistics.pattern_lookup WHERE id LIKE '%'",
        "SELECT id FROM logistics.pattern_lookup WHERE secret LIKE '%'",
        "SELECT id FROM logistics.pattern_lookup WHERE code OPERATOR(logistics.~~) '%'",
        "SELECT id FROM logistics.pattern_lookup WHERE code ~ '.*'",
        "SELECT id FROM logistics.pattern_lookup WHERE code SIMILAR TO '%a%'",
        "SELECT id FROM logistics.pattern_lookup WHERE code LIKE ANY (ARRAY['a'])",
        r"SELECT id FROM logistics.pattern_lookup WHERE code LIKE 'a\'",
    ] {
        assert!(
            guard_select(&identity, &pattern_catalog, sql).is_err(),
            "{sql}"
        );
        refusals.push(json!({"sql":sql,"guard":"unsupported pattern shape","executed":false}));
    }
    for pattern in [
        "a".repeat(crate::sql_guard::LIKE_PATTERN_BYTE_LIMIT + 1),
        "_".repeat(crate::sql_guard::LIKE_WILDCARD_LIMIT + 1),
    ] {
        let sql = format!(
            "SELECT id FROM logistics.pattern_lookup WHERE code OPERATOR(pg_catalog.~~) '{pattern}'"
        );
        assert_eq!(
            guard_select(&identity, &pattern_catalog, &sql),
            Err(crate::sql_guard::GuardError::Budget)
        );
        refusals
            .push(json!({"pattern_bytes":pattern.len(),"guard":"pattern budget","executed":false}));
    }
    for sql in [
        "SELECT MIN(secret) FROM logistics.shipment",
        "SELECT logistics.min(stamp) FROM logistics.shipment",
        "SELECT MIN(zoned::date) FROM logistics.shipment",
        "SELECT MIN(pg_sleep(1)) FROM logistics.shipment",
        "SELECT MIN(MAX(stamp)) FROM logistics.shipment",
        "SELECT MIN(stamp) OVER () FROM logistics.shipment",
        "SELECT MIN(stamp) FILTER (WHERE true) FROM logistics.shipment",
        "SELECT MIN(DISTINCT stamp) FROM logistics.shipment",
        "SELECT MIN(flag),MAX(doc) FROM logistics.shipment",
        "SELECT MIN(binary_doc) FROM logistics.shipment",
        "SELECT MIN(token) FROM logistics.shipment",
        "SELECT MIN(payload) FROM logistics.shipment",
        "SELECT id FROM logistics.shipment WHERE code <> 1",
        "SELECT id FROM logistics.shipment WHERE code <> fixed",
        "SELECT id FROM logistics.shipment WHERE code::text <> ''",
        "SELECT id FROM logistics.shipment WHERE code OPERATOR(logistics.<>) ''",
        "SELECT id FROM logistics.shipment WHERE lower(secret) <> ''",
        "SELECT id FROM logistics.shipment WHERE code LIKE txt",
        "SELECT MIN(stamp) FROM private.shipment",
        "SELECT MIN(stamp) FROM capability_fixture.shipments",
        "SELECT MIN(stamp) FROM logistics.shipment HAVING id>0",
        "SELECT flag,MIN(stamp) FROM logistics.shipment GROUP BY 2",
        "SELECT MIN(stamp) FROM logistics.shipment ORDER BY 0",
        "SELECT date_trunc('day',zoned) FROM logistics.shipment",
        "SELECT id FROM logistics.shipment WHERE stamp>=TIMESTAMP '2026-02-29'",
        "SELECT id FROM logistics.shipment WHERE stamp>=TIMESTAMP '2026-02-31'",
        "SELECT id FROM logistics.shipment WHERE stamp>=TIMESTAMP '0000-01-01'",
        "SELECT id FROM logistics.shipment WHERE stamp>=TIMESTAMP '2026-10-28Z'",
        "SELECT id FROM logistics.shipment WHERE stamp>=TIMESTAMP 'today'",
        "SELECT id FROM logistics.shipment WHERE stamp>='today'",
        "SELECT id FROM logistics.shipment WHERE stamp>=code::timestamp",
        "SELECT id FROM logistics.shipment WHERE stamp>=TIMESTAMP(0) '2026-10-28'",
        "SELECT id FROM logistics.shipment WHERE zoned>=TIMESTAMPTZ '2026-10-28'",
        "SELECT date_trunc('day',zoned,'Asia/Shanghai') FROM logistics.shipment",
        "SELECT date_trunc('invalid',stamp) FROM logistics.shipment",
        "SELECT date_trunc(code,stamp) FROM logistics.shipment",
        "SELECT date_trunc('day',secret) FROM logistics.shipment",
        "SELECT logistics.date_trunc('day',stamp) FROM logistics.shipment",
        "SELECT logistics.date(stamp) FROM logistics.shipment",
        "SELECT stamp::logistics.date FROM logistics.shipment",
        "SELECT stamp::date[] FROM logistics.shipment",
        "SELECT stamp::timestamp::date FROM logistics.shipment",
        "SELECT code::date FROM logistics.shipment",
        "SELECT date_trunc('day',stamp) OVER () FROM logistics.shipment",
        "SELECT date_trunc('day',stamp) FILTER (WHERE true) FROM logistics.shipment",
        "SELECT id FROM logistics.shipment ORDER BY 2",
        "SELECT id FROM logistics.shipment ORDER BY -1",
        "SELECT id FROM logistics.shipment ORDER BY 1.5",
        "SELECT id FROM logistics.shipment ORDER BY '1'",
        "SELECT id FROM logistics.shipment ORDER BY id USING >",
        "SELECT id,COUNT(*) FROM logistics.shipment GROUP BY 3",
        "SELECT id,COUNT(*) FROM logistics.shipment GROUP BY 0",
        "SELECT id,COUNT(*) FROM logistics.shipment GROUP BY -1",
        "SELECT id,COUNT(*) FROM logistics.shipment GROUP BY '1'",
        "SELECT id,COUNT(*) FROM logistics.shipment GROUP BY COUNT(*)",
        "SELECT id,SUM(amount)+1 FROM logistics.shipment GROUP BY 2",
        "SELECT COUNT(*) FROM logistics.shipment HAVING COUNT(*)",
        "SELECT COUNT(*) FROM logistics.shipment HAVING COUNT(*)>pg_sleep(1)",
        "SELECT COUNT(*) FROM logistics.shipment HAVING SUM(secret)>0",
        "SELECT COUNT(*) FROM logistics.shipment HAVING SUM(COUNT(*))>0",
        "SELECT COUNT(*) FROM logistics.shipment HAVING (SELECT 1)>0",
        "SELECT COUNT(*) AS n FROM logistics.shipment HAVING n>0",
        "SELECT COUNT(*) FROM logistics.shipment WHERE COUNT(*)>0",
        "SELECT COUNT(*) FROM logistics.shipment a JOIN logistics.shipment b ON COUNT(*)>0",
        "SELECT id,COUNT(*) FROM logistics.shipment HAVING COUNT(*)>0",
        "SELECT code,COUNT(*) FROM logistics.shipment GROUP BY 1 ORDER BY amount",
        "SELECT stamp::date,COUNT(*) FROM logistics.shipment GROUP BY 1 HAVING stamp>=TIMESTAMP '2026-10-28 00:00:00'",
        "DELETE FROM logistics.shipment",
    ] {
        let error = guard_select(&identity, &catalog, sql).expect_err(sql);
        refusals.push(json!({"sql":sql,"guard":error.to_string(),"executed":false}));
    }
    for (function, sql) in [
        ("date", "SELECT stamp::date FROM logistics.shipment"),
        (
            "date_trunc",
            "SELECT date_trunc('day',stamp) FROM logistics.shipment",
        ),
        (
            "sum",
            "SELECT COUNT(*) FROM logistics.shipment HAVING SUM(amount)>0",
        ),
    ] {
        let mut missing = catalog.clone();
        missing.functions.retain(|f| f.name != function);
        assert!(guard_select(&identity, &missing, sql).is_err(), "{sql}");
        refusals.push(json!({"sql":sql,"guard":"required builtin removed","executed":false}));
    }
    for operator in COMPARISON_OPERATORS {
        let mut missing = catalog.clone();
        missing
            .operators
            .retain(|op| op.name != *operator || op.left_type_oid != types["text"]);
        let sql = format!("SELECT code FROM logistics.shipment WHERE code {operator} ''");
        assert!(guard_select(&identity, &missing, &sql).is_err(), "{sql}");
        refusals.push(json!({"sql":sql,"guard":"required text operator removed","executed":false}));
    }
    for name in ["min", "max"] {
        for (column, kind) in [
            ("stamp", "timestamp"),
            ("day", "date"),
            ("zoned", "timestamptz"),
            ("code", "text"),
        ] {
            let mut missing = catalog.clone();
            missing
                .functions
                .retain(|f| !(f.name == name && f.argument_type_oids == [types[kind]]));
            let sql = format!("SELECT {name}({column}) FROM logistics.shipment");
            assert!(guard_select(&identity, &missing, &sql).is_err(), "{sql}");
            refusals.push(json!({"sql":sql,"guard":"required aggregate removed","executed":false}));
        }
    }
    for (sql, code) in [
        ("SELECT secret FROM capability_fixture.shipments", "42501"),
        ("DELETE FROM capability_fixture.shipments", "25006"),
    ] {
        let error = client.simple_query(sql).await.expect_err(sql);
        assert_eq!(error.code().unwrap().code(), code);
        refusals
            .push(json!({"sql":sql,"postgres_sqlstate":code,"guard":"direct fixture ACL oracle"}));
    }
    for (sql, code) in [
        ("SELECT id FROM logistics.shipment ORDER BY 0", "42P10"),
        (
            "SELECT flag,COUNT(*) FROM logistics.shipment GROUP BY 2",
            "42803",
        ),
        (
            "SELECT COUNT(*) FROM logistics.shipment HAVING id>0",
            "42803",
        ),
        (
            "SELECT COUNT(*) FROM logistics.shipment HAVING SUM(COUNT(*))>0",
            "42803",
        ),
        (
            "SELECT COUNT(*) FROM logistics.shipment WHERE COUNT(*)>0",
            "42803",
        ),
    ] {
        assert!(guard_select(&identity, &catalog, sql).is_err(), "{sql}");
        let error = client.prepare(sql).await.expect_err(sql);
        assert_eq!(error.code().unwrap().code(), code, "{sql}");
        refusals
            .push(json!({"sql":sql,"postgres_sqlstate":code,"guard":"refused; PostgreSQL agrees"}));
    }
    let report = json!({
        "actualDB": {"host":"127.0.0.1","port":fixture.port,"database":fixture.database,
            "user":fixture.user,"version":actual.get::<_,String>(3),
            "server_version_num":actual.get::<_,String>(4),"tls_verified":true,
            "read_only":true,"elevated_role_flags":false,"fixture_write_grants":false},
        "scope":"live load_types/load_functions/load_operators + guard_select + rewritten SQL execution; not publication/HTTP/deployment acceptance",
        "function_count":catalog.functions.len(),"operator_count":catalog.operators.len(),
        "parser_catalog_wire_roundtrip":true,
        "signatures":signatures,"function_budgets":budgets,"queries":queries,"refusals":refusals,
        "business_instances_touched":false,
    });
    fs::write(
        &fixture.report_path,
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    drop(client);
    driver.await.unwrap().unwrap();
}

#[tokio::test]
#[ignore = "only the owned runner mutates its fresh private pg_catalog fixture"]
async fn owned_day_catalog_refuses_modified_builtin() {
    let path = std::env::var("OO_SQL_CAPABILITIES_PG_CONFIG").expect("owned fixture config");
    let fixture: Fixture = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(fixture.database.starts_with("oo_sql_capabilities_"));
    assert_eq!(fixture.user, "oo_sql_capabilities_reader");
    assert!(fixture.port > 1024 && fixture.port != 5432);
    let endpoint = TlsEndpoint {
        host: "127.0.0.1".into(),
        port: fixture.port,
        database: fixture.database.clone(),
        user: fixture.user.clone(),
    };
    let password = fs::read_to_string(&fixture.password_file).unwrap();
    let ca = fs::read(&fixture.ca_file).unwrap();
    let (mut client, connection) = connect_require_tls(&endpoint, password.trim(), &ca)
        .await
        .unwrap();
    let driver = tokio::spawn(connection);
    require_tls(&client).await.unwrap();
    let actual = client.query_one(
        "SELECT current_database(),current_user,inet_server_port(), \
         current_setting('oo.sql_capabilities_nonce',true),current_setting('transaction_read_only')",
        &[],
    ).await.unwrap();
    assert_eq!(actual.get::<_, String>(0), fixture.database);
    assert_eq!(actual.get::<_, String>(1), fixture.user);
    assert_eq!(actual.get::<_, i32>(2), i32::from(fixture.port));
    assert_eq!(actual.get::<_, String>(3), fixture.nonce);
    assert_eq!(actual.get::<_, String>(4), "on");
    let tx = client
        .build_transaction()
        .read_only(true)
        .isolation_level(IsolationLevel::RepeatableRead)
        .start()
        .await
        .unwrap();
    let types = load_types(&tx).await.unwrap();
    let limits = CatalogLimits {
        max_tables: 1,
        max_columns: 32,
        max_functions: FUNCTION_CATALOG_LIMIT,
        max_operators: 128,
        large_table_row_threshold: 100,
        unknown_rows: UnknownRows::Refuse,
    };
    assert_eq!(
        load_functions(&tx, &types, &limits).await,
        Err(CatalogError::MissingObject)
    );
    tx.rollback().await.unwrap();
    drop(client);
    driver.await.unwrap().unwrap();
}
