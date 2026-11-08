//! The receipt boundary for views, independently of the native overlay loader.
//! The T9 runner separately exercises actual HTTP queries on published tables.
use super::*;
use crate::policy::catalog::UnknownRows;
use crate::sql_guard::{LiteralTypes, VisibleColumn, VisibleRelation};

async fn observe(
    client: &mut tokio_postgres::Client,
    binding: &BindingCheck,
    catalog: &PolicyCatalog,
) -> Result<BindingReceipt, ExecError> {
    let tx = client
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await
        .unwrap();
    tx.batch_execute("SET LOCAL search_path TO pg_catalog")
        .await
        .unwrap();
    let result = binding.receipt_in_snapshot(&tx, catalog).await;
    tx.rollback().await.unwrap();
    result
}

fn view_binding(role: &str, view: &str) -> (BindingCheck, PolicyCatalog) {
    let identity = PolicyIdentity {
        project: "f22".into(),
        role: role.into(),
        policy_id: "t9".into(),
    };
    let catalog = PolicyCatalog {
        identity: identity.clone(),
        relations: vec![VisibleRelation {
            schema: "app".into(),
            name: view.into(),
            physical_schema: "_oo".into(),
            physical_name: view.into(),
            columns: vec![VisibleColumn {
                name: "amount".into(),
                physical_name: "amount".into(),
                type_oid: 20,
            }],
            pending_unclassified: 0,
            estimated_rows: 1,
        }],
        functions: vec![],
        operators: vec![],
        literal_types: LiteralTypes::default(),
        null_test_allowed: true,
        large_table_row_threshold: 100,
    };
    let binding = BindingCheck {
        expected: Expected {
            publication_id: "current".into(),
            data_vector_sha256: None,
        },
        identity,
        principal: "t9".into(),
        policy_version: "1".into(),
        knowledge_version: "1".into(),
        pack_digest: "a".repeat(64),
        source: Some("f22".into()),
        policies: vec![],
        limits: CatalogLimits {
            max_tables: 1,
            max_columns: 1,
            max_functions: 1,
            max_operators: 1,
            large_table_row_threshold: 100,
            unknown_rows: UnknownRows::Zero,
        },
        vector: None,
        native_catalog: None,
        admitted_catalog: Some(catalog.clone()),
    };
    (binding, catalog)
}

async fn owned_client() -> (
    tokio_postgres::Client,
    tokio::task::JoinHandle<Result<(), tokio_postgres::Error>>,
) {
    let path = std::env::var("OO_F22_OWNED_FIXTURE").expect("T9 owned fixture required");
    let config: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(config["owned_fixture"], "f22-owned/v1");
    assert_eq!(config["host"], "127.0.0.1");
    assert_eq!(config["database"], "f22_owned");
    let password = std::fs::read_to_string(config["password_file"].as_str().unwrap()).unwrap();
    let ca = std::fs::read(config["ca_file"].as_str().unwrap()).unwrap();
    let endpoint = crate::TlsEndpoint {
        host: "127.0.0.1".into(),
        port: config["port"].as_u64().unwrap() as u16,
        database: "f22_owned".into(),
        user: "postgres".into(),
    };
    let (client, connection) = crate::connect_require_tls(&endpoint, &password, &ca)
        .await
        .unwrap();
    (client, tokio::spawn(connection))
}

#[tokio::test]
#[ignore = "owned PG17 fixture with migration 0135; run tools/oo-test/t9_restore.py"]
async fn logical_view_binding_refuses_redefinition_and_dependency_swap() {
    let path = std::env::var("OO_F22_OWNED_FIXTURE").expect("T9 owned fixture required");
    let config: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(config["owned_fixture"], "f22-owned/v1");
    assert_eq!(config["host"], "127.0.0.1");
    assert_eq!(config["database"], "f22_owned");
    let password = std::fs::read_to_string(config["password_file"].as_str().unwrap()).unwrap();
    let ca = std::fs::read(config["ca_file"].as_str().unwrap()).unwrap();
    let endpoint = crate::TlsEndpoint {
        host: "127.0.0.1".into(),
        port: config["port"].as_u64().unwrap() as u16,
        database: "f22_owned".into(),
        user: "postgres".into(),
    };
    let (mut client, connection) = crate::connect_require_tls(&endpoint, &password, &ca)
        .await
        .unwrap();
    let driver = tokio::spawn(connection);
    client.batch_execute(
        "CREATE ROLE oo_r_t9_view;
         CREATE TABLE _oo.t9_base(amount bigint NOT NULL);
         INSERT INTO _oo.t9_base VALUES(10);
         CREATE VIEW _oo.t9_view AS SELECT amount FROM _oo.t9_base;
         GRANT SELECT,TRIGGER ON _oo.t9_base TO oo_target_admin;
         GRANT SELECT ON _oo.t9_view TO oo_target_admin;
         INSERT INTO _oo.table_freshness VALUES('f22','app','t9_view','t9_view',1,1,'live',NULL,now());
         INSERT INTO oo_publish_target.fence VALUES('oo_r_t9_view','_oo','t9_view','open',repeat('a',64),'generation1')"
    ).await.unwrap();
    let identity = PolicyIdentity {
        project: "f22".into(),
        role: "oo_r_t9_view".into(),
        policy_id: "t9".into(),
    };
    let catalog = PolicyCatalog {
        identity: identity.clone(),
        relations: vec![VisibleRelation {
            schema: "app".into(),
            name: "t9_view".into(),
            physical_schema: "_oo".into(),
            physical_name: "t9_view".into(),
            columns: vec![VisibleColumn {
                name: "amount".into(),
                physical_name: "amount".into(),
                type_oid: 20,
            }],
            pending_unclassified: 0,
            estimated_rows: 1,
        }],
        functions: vec![],
        operators: vec![],
        literal_types: LiteralTypes::default(),
        null_test_allowed: true,
        large_table_row_threshold: 100,
    };
    let binding = BindingCheck {
        expected: Expected {
            publication_id: "current".into(),
            data_vector_sha256: None,
        },
        identity,
        principal: "t9".into(),
        policy_version: "1".into(),
        knowledge_version: "1".into(),
        pack_digest: "a".repeat(64),
        source: Some("f22".into()),
        policies: vec![],
        limits: CatalogLimits {
            max_tables: 1,
            max_columns: 1,
            max_functions: 1,
            max_operators: 1,
            large_table_row_threshold: 100,
            unknown_rows: UnknownRows::Zero,
        },
        vector: None,
        native_catalog: None,
        admitted_catalog: Some(catalog.clone()),
    };
    let before = observe(&mut client, &binding, &catalog).await.unwrap();
    for (attack, repair) in [
        (
            "CREATE OR REPLACE VIEW _oo.t9_view AS SELECT amount+1 AS amount FROM _oo.t9_base",
            "CREATE OR REPLACE VIEW _oo.t9_view AS SELECT amount FROM _oo.t9_base",
        ),
        (
            "ALTER TABLE _oo.t9_base RENAME TO t9_saved; CREATE TABLE _oo.t9_base(amount bigint NOT NULL); INSERT INTO _oo.t9_base VALUES(999)",
            "DROP TABLE _oo.t9_base; ALTER TABLE _oo.t9_saved RENAME TO t9_base",
        ),
        (
            "DROP VIEW _oo.t9_view; CREATE VIEW _oo.t9_view AS SELECT 999::bigint AS amount",
            "CREATE OR REPLACE VIEW _oo.t9_view AS SELECT amount FROM _oo.t9_base; GRANT SELECT ON _oo.t9_view TO oo_target_admin",
        ),
    ] {
        client.batch_execute(attack).await.unwrap();
        assert!(matches!(
            observe(&mut client, &binding, &catalog).await,
            Err(ExecError::Protocol {
                step: "publication_drift"
            })
        ));
        client.batch_execute(repair).await.unwrap();
        assert!(observe(&mut client, &binding, &catalog).await.is_err());
        client
            .batch_execute(
                "SET ROLE oo_target_admin; UPDATE oo_publish_target.fence SET state='open'
             WHERE role_name='oo_r_t9_view'; RESET ROLE",
            )
            .await
            .unwrap();
        assert_eq!(
            observe(&mut client, &binding, &catalog).await.unwrap(),
            before
        );
    }
    drop(client);
    driver.await.unwrap().unwrap();
}

#[tokio::test]
#[ignore = "owned PG17 fixture with migrations 0135/0136; run tools/oo-test/t9_restore.py"]
async fn logical_view_binding_refuses_replaced_routine() {
    let (mut client, driver) = owned_client().await;
    // A routine owner who is neither the publisher nor the view owner.
    client.batch_execute(
        "CREATE ROLE oo_t9_routine_owner NOLOGIN;
         GRANT USAGE,CREATE ON SCHEMA _oo TO oo_t9_routine_owner;
         CREATE ROLE oo_r_t9_fview;
         CREATE TABLE _oo.t9_fbase(amount bigint NOT NULL);
         INSERT INTO _oo.t9_fbase VALUES(10);
         SET ROLE oo_t9_routine_owner;
         CREATE FUNCTION _oo.t9_inner(x bigint) RETURNS bigint LANGUAGE sql IMMUTABLE AS $$ SELECT x $$;
         CREATE FUNCTION _oo.t9_scale(x bigint) RETURNS bigint LANGUAGE sql IMMUTABLE RETURN _oo.t9_inner(x);
         RESET ROLE;
         CREATE VIEW _oo.t9_fview AS SELECT _oo.t9_scale(amount) AS amount FROM _oo.t9_fbase;
         GRANT SELECT,TRIGGER ON _oo.t9_fbase TO oo_target_admin;
         GRANT SELECT ON _oo.t9_fview TO oo_target_admin;
         INSERT INTO _oo.table_freshness VALUES('f22','app','t9_fview','t9_fview',1,1,'live',NULL,now());
         INSERT INTO oo_publish_target.fence VALUES('oo_r_t9_fview','_oo','t9_fview','open',repeat('a',64),'generation1')"
    ).await.unwrap();
    let (binding, catalog) = view_binding("oo_r_t9_fview", "t9_fview");
    let before = observe(&mut client, &binding, &catalog).await.unwrap();
    let identity: String = client
        .query_one(
            "SELECT _oo.publication_relation_identity('_oo','t9_fview')::text",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(identity.contains("t9_scale") && identity.contains("t9_inner"));
    for (attack, repair) in [
        (
            "SET ROLE oo_t9_routine_owner; CREATE OR REPLACE FUNCTION _oo.t9_scale(x bigint) RETURNS bigint \
             LANGUAGE sql IMMUTABLE RETURN _oo.t9_inner(x)+999; RESET ROLE",
            "SET ROLE oo_t9_routine_owner; CREATE OR REPLACE FUNCTION _oo.t9_scale(x bigint) RETURNS bigint \
             LANGUAGE sql IMMUTABLE RETURN _oo.t9_inner(x); RESET ROLE",
        ),
        (
            "SET ROLE oo_t9_routine_owner; CREATE OR REPLACE FUNCTION _oo.t9_inner(x bigint) RETURNS bigint \
             LANGUAGE sql IMMUTABLE AS $$ SELECT 999::bigint $$; RESET ROLE",
            "SET ROLE oo_t9_routine_owner; CREATE OR REPLACE FUNCTION _oo.t9_inner(x bigint) RETURNS bigint \
             LANGUAGE sql IMMUTABLE AS $$ SELECT x $$; RESET ROLE",
        ),
        (
            "ALTER FUNCTION _oo.t9_inner(bigint) VOLATILE",
            "ALTER FUNCTION _oo.t9_inner(bigint) IMMUTABLE",
        ),
        (
            "ALTER FUNCTION _oo.t9_scale(bigint) OWNER TO postgres",
            "ALTER FUNCTION _oo.t9_scale(bigint) OWNER TO oo_t9_routine_owner",
        ),
    ] {
        client.batch_execute(attack).await.unwrap();
        assert!(
            matches!(
                observe(&mut client, &binding, &catalog).await,
                Err(ExecError::Protocol {
                    step: "publication_drift"
                })
            ),
            "{attack}"
        );
        client.batch_execute(repair).await.unwrap();
        // Restoring the routine does not restore publisher approval.
        assert!(
            observe(&mut client, &binding, &catalog).await.is_err(),
            "{repair}"
        );
        client
            .batch_execute(
                "SET ROLE oo_target_admin; UPDATE oo_publish_target.fence SET state='open'
             WHERE role_name='oo_r_t9_fview'; RESET ROLE",
            )
            .await
            .unwrap();
        assert_eq!(
            observe(&mut client, &binding, &catalog).await.unwrap(),
            before
        );
    }
    drop(client);
    driver.await.unwrap().unwrap();
}
