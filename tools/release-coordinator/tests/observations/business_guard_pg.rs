//! Opt-in, dedicated loopback database only. This tests refusal, not an issuer.
use serde_json::{Value, json};

use crate::{Endpoint, GateRequest, InstallRequest, PlanRequest, StageRequest};

pub(crate) fn business_candidate() -> (Value, Value) {
    let (mut effective, visibility) = crate::live::shop(&["id"]);
    effective["metrics"] = json!([{
        "metric":"customer_count","label":"Fixture records","owner":"fixture/test","certified":true,
        "status":"active","unit":"record","default_anchor":"month_start",
        "sql":"SELECT count(id) FROM public.customer",
        "tests":[{"params":{},"expect":{"rows":1}}],
        "control_total":{"as_of":"month_start","source":"independent_fixture",
            "sql":"SELECT sum(n) FROM public.independent_counts","tolerance":{"abs":"0"}}
    }]);
    effective["examples"] = json!([{
        "id":"customer-count","question":"Fixture record count",
        "sql":"SELECT count(id) FROM public.customer",
        "uses":["dataset:public.customer","metric:customer_count"],
        "verified_by":"fixture/test","verified_at":"2026-10-27",
        "last_checked":{"at":"2026-10-27","status":"ok","result_hash":"a".repeat(64)}
    }]);
    effective["ontology"]["publish"]["require_eval"] = json!(["business-fixture"]);
    (effective, visibility)
}

#[test]
fn candidate_fixture_uses_the_real_pack_compiler_without_execution_approval() {
    let (effective, visibility) = business_candidate();
    oo_builder::compile_role_pack(
        &crate::canon::canonical(&effective),
        &crate::canon::canonical(&visibility),
    )
    .unwrap_or_else(|error| panic!("fixture compiler: {error:?}"));
}

#[tokio::test]
#[ignore = "requires a new dedicated loopback database via run_business_guard_fixture.py"]
async fn authenticated_legacy_test_claims_cannot_bypass_business_preconditions() {
    let config: Value = serde_json::from_slice(
        &std::fs::read(std::env::var("OO_BUSINESS_GUARD_CONFIG").expect("private fixture config"))
            .expect("fixture config"),
    )
    .unwrap();
    assert_eq!(config["host"], "127.0.0.1");
    let database = config["database"].as_str().unwrap();
    assert!(database.starts_with("oo_business_guard_"));
    let endpoint = Endpoint {
        host: "127.0.0.1".into(),
        port: config["port"].as_u64().unwrap().try_into().unwrap(),
        database: database.into(),
        pack_ceilings: None,
    };
    let password = std::fs::read_to_string(config["password_file"].as_str().unwrap()).unwrap();
    let ca = std::fs::read(config["ca_file"].as_str().unwrap()).unwrap();
    let session = crate::store::open(&endpoint, "oo_release_admin", password.trim(), &ca)
        .await
        .unwrap();
    crate::store::require_tls(&session.client).await.unwrap();
    let row = session
        .client
        .query_one(
            "SELECT current_database(),current_user,to_regnamespace('oo_release_control')::text",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(row.get::<_, String>(0), database);
    assert_eq!(row.get::<_, String>(1), "oo_release_admin");
    assert!(
        row.get::<_, Option<String>>(2).is_none(),
        "refuse existing control schemas"
    );
    let covers: Vec<_> = crate::GATE_IDS.iter().map(|g| (*g).to_owned()).collect();
    assert_eq!(
        crate::install(&InstallRequest {
            endpoint: &endpoint,
            user: "oo_release_admin",
            password: password.trim(),
            ca_pem: &ca,
            project_id: "shop",
            authority_id: "local-business-guard-fixture",
            covers: &covers,
        })
        .await
        .unwrap()
        .code,
        0
    );
    let (effective, visibility) = business_candidate();
    let staged = crate::stage(&StageRequest {
        endpoint: &endpoint,
        user: "oo_release_admin",
        password: password.trim(),
        ca_pem: &ca,
        project_id: "shop",
        candidate_id: "business-guard",
        version: "guard-1",
        base_version: None,
        effective: &effective,
        visibility: &visibility,
        policy_id: "pol-shop",
        policy_version: "1",
        knowledge_id: "know-shop",
        knowledge_version: "1",
        catalog_version: "cat-1",
    })
    .await
    .unwrap();
    assert_eq!(staged.code, 0, "{}", staged.body);
    let source = staged.body["source_hash"].as_str().unwrap();
    let required = ["G5a", "G5b", "G6", "G10", "G11"];
    let mut raw =
        vec![json!({"operation":"stage","exit":staged.code,"receipt":staged.body.clone()})];
    for gate in crate::GATE_IDS {
        let recorded = crate::gate_record(&GateRequest {
            endpoint: &endpoint,
            user: "oo_release_admin",
            password: password.trim(),
            ca_pem: &ca,
            project_id: "shop",
            candidate_id: "business-guard",
            source_hash: source,
            gate_id: gate,
            issuer_class: "test_issuer",
            status: "pass",
            blocking: false,
            report_type: "test_issuer",
            authority_statement_sha256: None,
        })
        .await
        .unwrap();
        if required.contains(gate) {
            assert_eq!(recorded.code, 2);
            assert_eq!(recorded.body["reason"], "bound_evidence_required");
            assert_eq!(recorded.body["gate_written"], false);
        } else {
            assert_eq!(recorded.code, 0);
        }
        raw.push(json!({"gate":gate,"exit":recorded.code,"receipt":recorded.body}));
    }
    let count: i64 = session.client.query_one(
        "SELECT count(*) FROM oo_release_control.gate_report WHERE gate_id IN ('G5a','G5b','G6','G10','G11')",
        &[]
    ).await.unwrap().get(0);
    assert_eq!(count, 0);
    // Simulate historical generic rows, not authenticated business receipts.
    for gate in required {
        let certificate = crate::sha256_hex(format!("legacy:{gate}").as_bytes());
        session
            .client
            .execute(
                crate::sql::INSERT_GATE,
                &[
                    &"business-guard",
                    &certificate,
                    &gate,
                    &source,
                    &"test_issuer",
                    &"pass",
                    &false,
                    &"test_issuer",
                    &Option::<String>::None,
                    &"admissible",
                    &"caller_receipt",
                ],
            )
            .await
            .unwrap();
    }
    let control = crate::ControlConn {
        endpoint: &endpoint,
        user: "oo_release_admin",
        password: password.trim(),
        ca_pem: &ca,
    };
    assert_eq!(
        crate::maintain_install(&control, "shop")
            .await
            .unwrap()
            .code,
        0
    );
    for gate in required {
        session
            .client
            .execute(
                crate::sql::UPSERT_MAINTENANCE_GATE,
                &[
                    &"business-guard",
                    &gate,
                    &"not_applicable",
                    &"stale fixture overlay",
                    &"c".repeat(64),
                ],
            )
            .await
            .unwrap();
    }
    let request = PlanRequest {
        endpoint: &endpoint,
        user: "oo_release_admin",
        password: password.trim(),
        ca_pem: &ca,
        project_id: "shop",
        candidate_id: "business-guard",
        source_hash: source,
        scope: "test_scope",
        expected_version: None,
    };
    for (name, receipt) in [
        ("plan", crate::plan(&request).await.unwrap()),
        ("activate", crate::activate(&request).await.unwrap()),
    ] {
        assert_eq!(receipt.code, 2, "{}", receipt.body);
        for gate in required {
            assert!(
                receipt.body["reasons"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(format!("{gate}_blocked")))
            );
            if let Some(gates) = receipt.body["gates"].as_array() {
                let summary = gates.iter().find(|g| g["gate_id"] == gate).unwrap();
                assert_eq!(summary["disposition"], "blocked");
                assert_eq!(summary["test_admissible"], false);
                assert_eq!(summary["production_admissible"], false);
            }
        }
        raw.push(json!({"operation":name,"exit":receipt.code,"receipt":receipt.body}));
    }
    let active: Option<String> = session
        .client
        .query_one(
            "SELECT candidate_id FROM oo_release_control.active_pointer WHERE singleton",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(active.is_none());
    let path = std::path::PathBuf::from(std::env::var("OO_RELEASE_GATE_EVIDENCE_DIR").unwrap())
        .join("authenticated-ledger-refusals.json");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(
        crate::canon::canonical(&json!({
            "fixture_only":true,"transport":"loopback_tls","control_login":"oo_release_admin",
            "customer_source_access":false,"business_sql_executed":false,
            "actual_business_issuer_available":false,"published":false,"receipts":raw,
        }))
        .as_bytes(),
    )
    .unwrap();
}
