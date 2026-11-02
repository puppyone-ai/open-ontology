//! Owned, opt-in local PG proof of the actual operator loader, not publication.

use super::*;
use crate::policy::{TlsEndpoint, connect_require_tls};
use crate::sql_guard::{GuardError, guard_select};
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

async fn text_rows(client: &Client, sql: &str) -> Vec<Vec<Option<String>>> {
    client
        .simple_query(sql)
        .await
        .expect("fixture SELECT")
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

#[tokio::test]
#[ignore = "owned local PG only; testing/real-db/numeric-operators/run_local.py"]
async fn owned_numeric_catalog_and_execution() {
    let config = std::env::var("OO_NUMERIC_PG_CONFIG").expect("owned fixture config required");
    let fixture: Fixture = serde_json::from_slice(&fs::read(config).unwrap()).unwrap();
    assert!(fixture.port > 1024 && fixture.port != 5432);
    assert!(fixture.database.starts_with("oo_numeric_fixture_"));
    assert_eq!(fixture.user, "oo_numeric_fixture_reader");
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
         current_setting('oo.numeric_fixture_nonce', true)",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(actual.get::<_, String>(0), fixture.database);
    assert_eq!(actual.get::<_, String>(1), fixture.user);
    assert_eq!(actual.get::<_, i32>(2), i32::from(fixture.port));
    assert_eq!(actual.get::<_, String>(5), "on");
    assert_eq!(actual.get::<_, String>(6), fixture.nonce);
    let role = client
        .query_one(
            "SELECT rolsuper, rolcreatedb, rolcreaterole, rolinherit, rolreplication, rolbypassrls \
         FROM pg_catalog.pg_roles WHERE rolname = current_user",
            &[],
        )
        .await
        .unwrap();
    for index in 0..6 {
        assert!(!role.get::<_, bool>(index));
    }
    let privileges = client
        .query_one(
            "SELECT has_schema_privilege(current_user, 'numeric_fixture', 'CREATE'), \
         has_table_privilege(current_user, 'numeric_fixture.amounts', 'INSERT'), \
         has_table_privilege(current_user, 'numeric_fixture.amounts', 'UPDATE'), \
         has_table_privilege(current_user, 'numeric_fixture.amounts', 'DELETE')",
            &[],
        )
        .await
        .unwrap();
    for index in 0..4 {
        assert!(!privileges.get::<_, bool>(index));
    }

    let limits = CatalogLimits {
        max_tables: 1,
        max_columns: 8,
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
    tx.batch_execute("SET LOCAL search_path = pg_catalog")
        .await
        .unwrap();
    let types = load_types(&tx).await.unwrap();
    let operators = load_operators(&tx, &types, &limits).await.unwrap();
    let functions = load_functions(&tx, &types, &limits).await.unwrap();
    assert_eq!(operators.len(), required_operator_count());
    assert_eq!(functions.len(), FUNCTION_CATALOG_LIMIT as usize);

    let mut signatures = Vec::new();
    for operator in &operators {
        let row = tx.query_one(
            "SELECT o.oprcode::oid::int4, o.oprleft::int4, o.oprright::int4, o.oprresult::int4, \
             p.prorettype::int4, p.provolatile::text, p.prokind::text, l.lanname, \
             p.prosecdef, p.proretset, p.provariadic::int4, p.proconfig IS NULL, \
             n.nspname, p.proargtypes[0]::int4, p.proargtypes[1]::int4, p.pronargs, \
             o.oprname, t.typname, p.proname, \
             EXISTS(SELECT 1 FROM pg_catalog.pg_depend d WHERE d.deptype='e' AND \
               ((d.classid='pg_catalog.pg_operator'::regclass AND d.objid=o.oid) OR \
                (d.classid='pg_catalog.pg_proc'::regclass AND d.objid=p.oid))) \
             FROM pg_catalog.pg_operator o JOIN pg_catalog.pg_proc p ON p.oid=o.oprcode \
             JOIN pg_catalog.pg_language l ON l.oid=p.prolang \
             JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace \
             JOIN pg_catalog.pg_type t ON t.oid=o.oprresult WHERE o.oid=($1::int4)::oid",
            &[&i32::try_from(operator.oid).unwrap()],
        ).await.unwrap();
        for (index, expected) in [
            (0, operator.procedure_oid),
            (1, operator.left_type_oid),
            (2, operator.right_type_oid),
            (3, operator.result_type_oid),
            (4, operator.result_type_oid),
            (13, operator.left_type_oid),
            (14, operator.right_type_oid),
        ] {
            assert_eq!(row.get::<_, i32>(index) as u32, expected);
        }
        for (index, expected) in [(5, "i"), (6, "f"), (7, "internal"), (12, "pg_catalog")] {
            assert_eq!(row.get::<_, String>(index), expected);
        }
        assert!(!row.get::<_, bool>(8));
        assert!(!row.get::<_, bool>(9));
        assert_eq!(row.get::<_, i32>(10), 0);
        assert!(row.get::<_, bool>(11));
        assert_eq!(row.get::<_, i16>(15), 2);
        assert_eq!(row.get::<_, String>(16), operator.name);
        assert!(!row.get::<_, bool>(19));
        signatures.push(json!({
            "name": operator.name, "oid": operator.oid, "procedure_oid": operator.procedure_oid,
            "procedure": row.get::<_, String>(18), "left_type_oid": operator.left_type_oid,
            "right_type_oid": operator.right_type_oid, "result_type_oid": operator.result_type_oid,
            "result_type": row.get::<_, String>(17),
        }));
    }
    let columns = tx.query(
        "SELECT a.attname, a.atttypid::int4, t.typname, a.atttypmod \
         FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_type t ON t.oid=a.atttypid \
         WHERE a.attrelid='numeric_fixture.amounts'::regclass AND a.attnum>0 AND NOT a.attisdropped \
         AND has_column_privilege(current_user, a.attrelid, a.attnum, 'SELECT') ORDER BY a.attnum",
        &[],
    ).await.unwrap().into_iter().map(|row| {
        assert!(grantable_base_type(&row.get::<_, String>(2), row.get(3)));
        let name: String = row.get(0);
        VisibleColumn { name: name.clone(), physical_name: name, type_oid: row.get::<_, i32>(1) as u32 }
    }).collect::<Vec<_>>();
    assert_eq!(columns.len(), 7);
    tx.commit().await.unwrap();
    let identity = PolicyIdentity {
        project: "owned_numeric_fixture".into(),
        role: fixture.user.clone(),
        policy_id: fixture.nonce.clone(),
    };
    let catalog = PolicyCatalog {
        identity: identity.clone(),
        relations: vec![VisibleRelation {
            schema: "numeric_fixture".into(),
            name: "amounts".into(),
            physical_schema: "numeric_fixture".into(),
            physical_name: "amounts".into(),
            columns,
            pending_unclassified: 0,
            estimated_rows: 6,
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
    let mut queries = Vec::new();
    for (sql, expected) in [
        (
            "SELECT SUM(amount_freight-amount_transfer-amount_dff) FROM numeric_fixture.amounts",
            vec![vec![Some("84.104996")]],
        ),
        (
            "SELECT amount_freight-amount_transfer-amount_dff FROM numeric_fixture.amounts ORDER BY id",
            vec![
                vec![Some("75.110000")],
                vec![Some("9.999996")],
                vec![None],
                vec![None],
                vec![None],
                vec![Some("-1.005000")],
            ],
        ),
        (
            "SELECT SUM(amount_freight+amount_transfer-amount_dff) FROM numeric_fixture.amounts",
            vec![vec![Some("124.125000")]],
        ),
        (
            "SELECT rounded FROM numeric_fixture.amounts ORDER BY id",
            vec![
                vec![Some("1.01")],
                vec![Some("-1.01")],
                vec![Some("0.00")],
                vec![None],
                vec![None],
                vec![Some("1.00")],
            ],
        ),
        (
            "SELECT SUM(rounded-0) FROM numeric_fixture.amounts",
            vec![vec![Some("1.00")]],
        ),
        (
            "SELECT COUNT(*), SUM(amount_freight-amount_transfer-amount_dff) FROM numeric_fixture.amounts \
          WHERE amount_freight-amount_transfer-amount_dff > 0",
            vec![vec![Some("2"), Some("85.109996")]],
        ),
        (
            "SELECT SUM(amount_freight-amount_transfer-amount_dff) FROM numeric_fixture.amounts \
          WHERE amount_transfer IS NULL",
            vec![vec![None]],
        ),
    ] {
        let plan = guard_select(&identity, &catalog, sql).unwrap();
        let original = text_rows(&client, sql).await;
        let rewritten = text_rows(&client, &plan.executed_sql).await;
        let expected: Vec<Vec<Option<String>>> = expected
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|value| value.map(str::to_owned))
                    .collect()
            })
            .collect();
        assert_eq!(original, expected, "{sql}");
        assert_eq!(rewritten, expected, "{}", plan.executed_sql);
        let statement = client.prepare(&plan.executed_sql).await.unwrap();
        queries.push(json!({
            "sql": sql, "executed_sql": plan.executed_sql, "rows": rewritten,
            "result_type_oids": statement.columns().iter().map(|c| c.type_().oid()).collect::<Vec<_>>(),
            "operator_oids": plan.bindings.operators.iter().map(|o| o.oid).collect::<Vec<_>>(),
            "procedure_oids": plan.bindings.operators.iter().map(|o| o.procedure_oid).collect::<Vec<_>>(),
        }));
    }
    let mut refusals = Vec::new();
    for sql in [
        "SELECT id FROM numeric_fixture.amounts WHERE amount_freight-amount_transfer-amount_dff",
        "SELECT id FROM numeric_fixture.amounts WHERE NOT (amount_freight+amount_transfer)",
    ] {
        assert!(matches!(
            guard_select(&identity, &catalog, sql),
            Err(GuardError::Unsupported {
                construct: "predicate"
            })
        ));
        let error = client.prepare(sql).await.unwrap_err();
        assert_eq!(error.code().unwrap().code(), "42804");
        refusals.push(
            json!({"sql": sql, "guard": "STATEMENT_NOT_ALLOWED", "postgres_sqlstate": "42804"}),
        );
    }
    for sql in [
        "SELECT label-label FROM numeric_fixture.amounts",
        "SELECT SUM(amount_freight-amount_transfer-pg_sleep(1)) FROM numeric_fixture.amounts",
        "SELECT ROUND(amount_freight,2) FROM numeric_fixture.amounts",
    ] {
        assert!(guard_select(&identity, &catalog, sql).is_err());
        refusals.push(json!({"sql": sql, "guard": "refused", "executed": false}));
    }
    let report = json!({
        "actualDB": {"host": "127.0.0.1", "port": fixture.port, "database": fixture.database,
            "user": fixture.user, "version": actual.get::<_, String>(3),
            "server_version_num": actual.get::<_, String>(4), "tls_verified": true,
            "read_only": true, "elevated_role_flags": false, "fixture_write_grants": false},
        "scope": "owned isolated local cluster; actual load_types/load_operators/load_functions and guard_select; not publication/fence/deployment acceptance",
        "operator_count": signatures.len(), "signatures": signatures, "queries": queries, "refusals": refusals,
        "rounding": "numeric(10,2) fixture storage: 1.005 -> 1.01, -1.005 -> -1.01; ROUND function remains refused",
        "business_instances_touched": false,
    });
    fs::write(
        &fixture.report_path,
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    drop(client);
    driver.await.unwrap().unwrap();
}
