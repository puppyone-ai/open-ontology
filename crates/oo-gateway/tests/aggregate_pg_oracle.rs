//! An opt-in PostgreSQL oracle with duplicates, NULL IDs and empty destinations.
//! Uses only a fresh, network-isolated container, never the user's databases.
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use oo_gateway::{
    GuardError, LiteralTypes, PolicyCatalog, PolicyIdentity, ResolvedFunction, ResolvedOperator,
    VisibleColumn, VisibleRelation, guard_select,
};

struct Fixture(String);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = Command::new("docker").args(["rm", "-f", &self.0]).output();
    }
}

impl Fixture {
    fn sql(&self, sql: &str) -> std::process::Output {
        Command::new("docker")
            .args([
                "exec",
                &self.0,
                "psql",
                "-X",
                "-qAt",
                "-v",
                "ON_ERROR_STOP=1",
                "-h",
                "127.0.0.1",
                "-U",
                "postgres",
                "-c",
                sql,
            ])
            .output()
            .expect("docker psql")
    }

    fn text(&self, sql: &str) -> String {
        let result = self.sql(sql);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap().trim().to_owned()
    }
}

#[test]
#[ignore = "requires Docker and an already-cached postgres:17 image"]
fn guarded_business_aggregates_match_postgres_without_expanding_permissions() {
    let started = Command::new("docker")
        .args([
            "run",
            "--pull=never",
            "--rm",
            "-d",
            "--network",
            "none",
            "-e",
            "POSTGRES_HOST_AUTH_METHOD=trust",
            "postgres:17",
        ])
        .output()
        .expect("docker run");
    assert!(
        started.status.success(),
        "{}",
        String::from_utf8_lossy(&started.stderr)
    );
    let id = String::from_utf8(started.stdout).unwrap().trim().to_owned();
    assert!(id.len() == 64 && id.bytes().all(|c| c.is_ascii_hexdigit()));
    let fixture = Fixture(id);
    let mut ready = false;
    for _ in 0..100 {
        // TCP excludes the entrypoint's temporary initialization server.
        if Command::new("docker")
            .args([
                "exec",
                &fixture.0,
                "pg_isready",
                "-h",
                "127.0.0.1",
                "-U",
                "postgres",
            ])
            .output()
            .unwrap()
            .status
            .success()
        {
            ready = true;
            break;
        }
        sleep(Duration::from_millis(100));
    }
    assert!(ready, "private fixture did not start");
    fixture.text(
        "CREATE SCHEMA _oo; CREATE SCHEMA logistics;
         CREATE TABLE _oo.pt_ship(pc_order integer, pc_region text, secret text);
         INSERT INTO _oo.pt_ship VALUES
           (1,'north','private'),(1,'north','private'),(2,'north','private'),
           (3,'south','private'),(NULL,'south','private'),(4,'','private');
         CREATE VIEW logistics.shipment AS
           SELECT pc_order AS order_id, pc_region AS region, secret FROM _oo.pt_ship;
         CREATE ROLE fixture_reader;
         GRANT USAGE ON SCHEMA _oo TO fixture_reader;
         GRANT SELECT(pc_order,pc_region) ON _oo.pt_ship TO fixture_reader;",
    );
    let count_oid = |arity| {
        fixture
            .text(&format!(
                "SELECT oid FROM pg_proc WHERE pronamespace='pg_catalog'::regnamespace
         AND proname='count' AND pronargs={arity}"
            ))
            .parse::<u32>()
            .unwrap()
    };
    let identity = PolicyIdentity {
        project: "fixture".into(),
        role: "fixture_reader".into(),
        policy_id: "p1".into(),
    };
    let mut catalog = PolicyCatalog {
        identity: identity.clone(),
        relations: vec![VisibleRelation {
            schema: "logistics".into(),
            name: "shipment".into(),
            physical_schema: "_oo".into(),
            physical_name: "pt_ship".into(),
            columns: vec![
                VisibleColumn {
                    name: "order_id".into(),
                    physical_name: "pc_order".into(),
                    type_oid: 23,
                },
                VisibleColumn {
                    name: "region".into(),
                    physical_name: "pc_region".into(),
                    type_oid: 25,
                },
            ],
            pending_unclassified: 1,
            estimated_rows: 6,
        }],
        functions: vec![
            ResolvedFunction {
                oid: count_oid(0),
                schema: "pg_catalog".into(),
                name: "count".into(),
                argument_type_oids: vec![],
                aggregate_star: true,
            },
            ResolvedFunction {
                oid: count_oid(1),
                schema: "pg_catalog".into(),
                name: "count".into(),
                argument_type_oids: vec![2276],
                aggregate_star: false,
            },
        ],
        operators: vec![],
        literal_types: LiteralTypes {
            integer: Some(23),
            text: Some(25),
            boolean: Some(16),
        },
        casts: Vec::new(),
        null_test_allowed: true,
        large_table_row_threshold: 100,
    };
    for sql in [
        "SELECT COUNT(*), COUNT(order_id), COUNT(DISTINCT order_id) FROM logistics.shipment",
        "SELECT region, COUNT(DISTINCT order_id) AS total FROM logistics.shipment GROUP BY region ORDER BY total DESC, region",
        "SELECT region, COUNT(*) AS total FROM logistics.shipment GROUP BY region ORDER BY COUNT(*) DESC, region",
        "SELECT region AS order_id FROM logistics.shipment ORDER BY order_id",
        "SELECT region, COUNT(DISTINCT order_id) AS \"total\" FROM logistics.shipment WHERE order_id IS NOT NULL GROUP BY region ORDER BY \"total\" DESC, region",
    ] {
        let plan = guard_select(&identity, &catalog, sql).expect("admit");
        let expected = fixture.text(sql);
        let actual = fixture.text(&format!(
            "SET ROLE fixture_reader; SET search_path TO ''; SET default_transaction_read_only=on; {}",
            plan.executed_sql
        ));
        assert_eq!(actual, expected, "{sql}");
    }
    let count = guard_select(
        &identity,
        &catalog,
        "SELECT COUNT(*), COUNT(order_id), COUNT(DISTINCT order_id) FROM logistics.shipment",
    )
    .unwrap();
    assert_eq!(fixture.text(&count.executed_sql), "6|5|4");
    assert!(matches!(
        guard_select(
            &identity,
            &catalog,
            "SELECT COUNT(DISTINCT secret) FROM logistics.shipment"
        ),
        Err(GuardError::ColumnNotAllowed { .. })
    ));
    assert!(
        !fixture
            .sql("SET ROLE fixture_reader; SELECT secret FROM _oo.pt_ship")
            .status
            .success()
    );
    assert!(
        !fixture
            .sql("SET ROLE fixture_reader; DELETE FROM _oo.pt_ship")
            .status
            .success()
    );

    fixture.text(
        "CREATE TABLE _oo.pt_receipt(pc_amount numeric, pc_paid timestamp, pc_day date);
         INSERT INTO _oo.pt_receipt VALUES
           (100.10,'2026-09-01','2026-09-01'),
           (-20.05,'2026-09-30 23:59:59.999999','2026-09-30'),
           (999.00,'2026-10-01','2026-10-01'),
           (888.00,'2026-08-31 23:59:59.999999','2026-08-31'),
           (NULL,'2026-09-15','2026-09-15'),
           (777.00,NULL,NULL);
         CREATE VIEW logistics.receipt AS
           SELECT pc_amount AS amount, pc_paid AS paid_at, pc_day AS paid_day FROM _oo.pt_receipt;
         GRANT SELECT(pc_amount,pc_paid,pc_day) ON _oo.pt_receipt TO fixture_reader;",
    );
    catalog.relations.push(VisibleRelation {
        schema: "logistics".into(),
        name: "receipt".into(),
        physical_schema: "_oo".into(),
        physical_name: "pt_receipt".into(),
        columns: vec![
            VisibleColumn {
                name: "amount".into(),
                physical_name: "pc_amount".into(),
                type_oid: 1700,
            },
            VisibleColumn {
                name: "paid_at".into(),
                physical_name: "pc_paid".into(),
                type_oid: 1114,
            },
            VisibleColumn {
                name: "paid_day".into(),
                physical_name: "pc_day".into(),
                type_oid: 1082,
            },
        ],
        pending_unclassified: 0,
        estimated_rows: 6,
    });
    catalog.functions.push(ResolvedFunction {
        oid: fixture
            .text("SELECT 'pg_catalog.sum(numeric)'::regprocedure::oid")
            .parse()
            .unwrap(),
        schema: "pg_catalog".into(),
        name: "sum".into(),
        argument_type_oids: vec![1700],
        aggregate_star: false,
    });
    for oid in [1114, 1082] {
        for name in [">=", "<"] {
            let signature = fixture.text(&format!(
                "SELECT oid, oprcode::oid FROM pg_operator
                 WHERE oprnamespace='pg_catalog'::regnamespace AND oprname='{name}'
                   AND oprleft={oid} AND oprright={oid}"
            ));
            let (operator, procedure) = signature.split_once('|').unwrap();
            catalog.operators.push(ResolvedOperator {
                oid: operator.parse().unwrap(),
                procedure_oid: procedure.parse().unwrap(),
                schema: "pg_catalog".into(),
                name: name.into(),
                left_type_oid: oid,
                right_type_oid: oid,
                result_type_oid: 16,
            });
        }
    }
    for (column, literal, start, end) in [
        (
            "paid_at",
            "TIMESTAMP",
            "2026-09-01 00:00:00",
            "2026-10-01 00:00:00",
        ),
        ("paid_day", "DATE", "2026-09-01", "2026-10-01"),
        ("paid_day", "", "2026-09-01", "2026-10-01"),
        ("paid_at", "", "2026-09-01", "2026-10-01"),
    ] {
        let sql = format!(
            "SELECT COUNT(*), SUM(amount) FROM logistics.receipt
             WHERE {column} >= {literal} '{start}' AND {column} < {literal} '{end}'"
        );
        let plan = guard_select(&identity, &catalog, &sql).unwrap();
        let expected = fixture.text(&sql);
        assert_eq!(expected, "3|80.05");
        assert_eq!(fixture.text(&format!(
            "SET ROLE fixture_reader; SET search_path TO ''; SET default_transaction_read_only=on; {}",
            plan.executed_sql
        )), expected);
    }
    assert!(matches!(
        guard_select(
            &identity,
            &catalog,
            "SELECT SUM(amount) FROM logistics.receipt WHERE paid_at >= 'today'"
        ),
        Err(GuardError::SignatureNotAllowed { operator: true, .. })
    ));
}
