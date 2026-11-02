use release_coordinator::{Endpoint, FenceRef, TargetConn, VisibilityRef};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn canonical_sha(value: &Value) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(value).unwrap()))
}

#[tokio::test]
async fn library_target_commit_visibility_and_fail_closed() {
    let fixture: Value = serde_json::from_slice(
        &std::fs::read(std::env::var("OO_LIBRARY_FIXTURE").expect("owned fixture export")).unwrap(),
    )
    .unwrap();
    let endpoint = Endpoint {
        pack_ceilings: None,
        host: "127.0.0.1".into(),
        port: std::env::var("OO_LIBRARY_PG_PORT")
            .unwrap()
            .parse()
            .unwrap(),
        database: fixture["database"].as_str().unwrap().into(),
    };
    let ca = std::fs::read(std::env::var("OO_LIBRARY_CA").unwrap()).unwrap();
    let conn = TargetConn {
        endpoint: &endpoint,
        user: "oo_target_admin",
        password: "fixture-only",
        ca_pem: &ca,
    };
    let bindings: Vec<(String, String, String, Vec<String>)> =
        serde_json::from_value(fixture["bindings"].clone()).unwrap();
    assert_eq!(bindings.len(), 36);
    let receipt = fixture["receipt"].clone();
    let context = json!({"publication":{"protocol":"oo.pipeline.native-library-publication/v1","run_id":receipt["shards"][0]["anchor_run"],"receipt_sha256":canonical_sha(&receipt),"receipt":receipt}});
    let publication = release_coordinator::native_publication_context(&context)
        .unwrap()
        .unwrap();
    let old_bindings: Vec<_> = bindings[..12]
        .iter()
        .map(|b| {
            (
                b.0.clone(),
                b.1.clone(),
                "oo_r_lib_old".to_string(),
                b.3.clone(),
            )
        })
        .collect();
    for (schema, table, role, columns) in &old_bindings {
        let fence = FenceRef {
            schema_name: schema,
            relation_name: table,
            role_name: role,
        };
        release_coordinator::install_target(&conn, &fence)
            .await
            .unwrap();
        release_coordinator::close_target(&conn, &fence)
            .await
            .unwrap();
        release_coordinator::grant_target(&conn, &fence, columns)
            .await
            .unwrap();
    }
    let old_receipt = &receipt["shards"][0]["receipt"];
    let old_context = json!({"publication":{"protocol":"oo.pipeline.native-publication-receipt/v1","run_id":old_receipt["run_id"],"receipt_sha256":canonical_sha(old_receipt),"receipt":old_receipt}});
    let old_publication = release_coordinator::native_publication_context(&old_context)
        .unwrap()
        .unwrap();
    assert_eq!(
        release_coordinator::open_native_collection(
            &conn,
            &old_bindings,
            &"b".repeat(64),
            "source-v1",
            "source-candidate",
            &old_publication
        )
        .await
        .unwrap()
        .len(),
        12
    );
    for (schema, table, role, columns) in &bindings {
        let fence = FenceRef {
            schema_name: schema,
            relation_name: table,
            role_name: role,
        };
        assert_eq!(
            release_coordinator::prepare_library_target(&conn, &fence)
                .await
                .unwrap()
                .code,
            0
        );
        assert_eq!(
            release_coordinator::close_target(&conn, &fence)
                .await
                .unwrap()
                .code,
            0
        );
        assert_eq!(
            release_coordinator::grant_target(&conn, &fence, columns)
                .await
                .unwrap()
                .code,
            0
        );
    }
    let pack = "a".repeat(64);
    let rows = release_coordinator::open_native_collection(
        &conn,
        &bindings,
        &pack,
        "library-v1",
        "library-candidate",
        &publication,
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 36);
    assert!(
        rows.iter()
            .all(|r| r.code == 0 && r.body["fence_state"] == "open")
    );
    release_coordinator::install_visibility_reader(&conn, "oo_library_reader")
        .await
        .unwrap();
    let observation = release_coordinator::verify_library_visibility(
        &conn,
        &conn,
        "oo_library_reader",
        "fixture-only",
        VisibilityRef {
            bindings: &bindings,
            version: "library-v1",
            pack: &pack,
            candidate: "library-candidate",
            mapping: None,
        },
        &publication,
        false,
    )
    .await
    .unwrap();
    assert_eq!(
        observation["library_receipt_sha256"],
        publication.receipt_sha256
    );
    let (client, connection) = tokio_postgres::Config::new()
        .host(&endpoint.host)
        .port(endpoint.port)
        .dbname(&endpoint.database)
        .user("oo_target_admin")
        .password("fixture-only")
        .connect(release_coordinator::rustls_connector(&ca).unwrap())
        .await
        .unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let pointers:i64=client.query_one("SELECT count(DISTINCT (candidate_id,opened_version,opened_pack_digest)) FROM oo_publish_target.fence WHERE state='open'",&[]).await.unwrap().get(0);
    assert_eq!(pointers, 1);
    if let Ok(port) = std::env::var("OO_LIBRARY_REPLICA_PORT") {
        let standby_endpoint = Endpoint {
            pack_ceilings: None,
            host: endpoint.host.clone(),
            port: port.parse().unwrap(),
            database: endpoint.database.clone(),
        };
        let standby = TargetConn {
            endpoint: &standby_endpoint,
            user: "oo_target_admin",
            password: "fixture-only",
            ca_pem: &ca,
        };
        release_coordinator::replica_barrier(&conn, &standby)
            .await
            .unwrap();
        let observed = release_coordinator::verify_library_visibility(
            &conn,
            &standby,
            "oo_library_reader",
            "fixture-only",
            VisibilityRef {
                bindings: &bindings,
                version: "library-v1",
                pack: &pack,
                candidate: "library-candidate",
                mapping: None,
            },
            &publication,
            true,
        )
        .await
        .unwrap();
        assert_eq!(observed["ready_replay"]["in_recovery"], true);
        assert_eq!(observed["readonly_replica"], true);
        assert_eq!(observed["deployment_mode"], "primary_replica");
    }
    let last = &bindings[35];
    client
        .batch_execute(&format!(
            "REVOKE SELECT ({}) ON _oo.{} FROM oo_r_lib_all",
            last.3[0], last.1
        ))
        .await
        .unwrap();
    let failure = release_coordinator::verify_library_visibility(
        &conn,
        &conn,
        "oo_library_reader",
        "fixture-only",
        VisibilityRef {
            bindings: &bindings,
            version: "library-v1",
            pack: &pack,
            candidate: "library-candidate",
            mapping: None,
        },
        &publication,
        false,
    )
    .await;
    assert!(failure.is_err());
    let state: String = client
        .query_one("SELECT state FROM _oo.native_library_head", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(state, "closed");
    let open: i64 = client
        .query_one(
            "SELECT count(*) FROM oo_publish_target.fence WHERE state<>'closed'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        open, 0,
        "visibility failure after commit must close all 36 fences"
    );
    for (schema, table, role, columns) in &bindings {
        let denied = release_coordinator::observe_target(
            &conn,
            &FenceRef {
                schema_name: schema,
                relation_name: table,
                role_name: role,
            },
            columns,
            "oo_library_reader",
            "fixture-only",
        )
        .await
        .unwrap();
        assert_ne!(
            denied.code, 0,
            "closed library must not permit the old role query"
        );
    }
}
