//! Uses real pipeline full_import, mapping, ACK and publication on a new fixture.
use crate::business_positive_pg::{Fixture, business_gates, write_new};
use serde_json::{Value, json};

async fn publish(root: &tokio_postgres::Client, generation: i64) -> Value {
    let raw: String = root
        .query_one(
            "SELECT native_business_fixture.load($1,12000)::text",
            &[&generation],
        )
        .await
        .unwrap()
        .get(0);
    let mut context: Value = serde_json::from_str(&raw).unwrap();
    let receipt = context["publication"]["receipt"].clone();
    let hash = crate::canon::canonical_sha(&receipt);
    context["publication"]["receipt_sha256"] = json!(hash);
    crate::validate_native_context(&context).unwrap();
    let publication = crate::native_publication_context(&context)
        .unwrap()
        .unwrap();
    let raw: String = root
        .query_one(
            crate::native_publication::publication_sql(&receipt),
            &[
                &publication.run_id,
                &hash,
                &crate::canon::canonical(&receipt),
            ],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap()["outcome"],
        "published"
    );
    context
}

#[tokio::test]
#[ignore = "new private TLS PG via run_business_guard_fixture.py --native"]
async fn native_publication_aggregate_chain() {
    let f = Fixture::load();
    assert_eq!(f.config["native"], true);
    let root = crate::store::open(&f.endpoint, "postgres", f.bootstrap.trim(), &f.ca)
        .await
        .unwrap();
    let admin = crate::store::open(&f.endpoint, "oo_release_admin", f.password.trim(), &f.ca)
        .await
        .unwrap();
    let context = publish(&root.client, 1).await;
    write_new(&f.out.join("native-context-1.json"), &context);
    let covers = crate::GATE_IDS
        .iter()
        .map(|g| (*g).into())
        .collect::<Vec<_>>();
    assert_eq!(
        crate::install(&crate::InstallRequest {
            endpoint: &f.endpoint,
            user: "oo_release_admin",
            password: f.password.trim(),
            ca_pem: &f.ca,
            project_id: "shop",
            authority_id: "native-test",
            covers: &covers
        })
        .await
        .unwrap()
        .code,
        0
    );
    let (effective, visibility) = crate::business_guard_pg::business_candidate();
    let staged = crate::stage(&crate::StageRequest {
        endpoint: &f.endpoint,
        user: "oo_release_admin",
        password: f.password.trim(),
        ca_pem: &f.ca,
        project_id: "shop",
        candidate_id: "positive",
        version: "native-1",
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
    let source = staged.body["source_hash"].as_str().unwrap().to_owned();
    let mut journal = vec![json!({"operation":"stage","receipt":staged.body})];
    for gate in crate::GATE_IDS {
        if ["G5a", "G5b", "G6", "G10", "G11"].contains(gate) {
            continue;
        }
        let recorded = crate::gate_record(&crate::GateRequest {
            endpoint: &f.endpoint,
            user: "oo_release_admin",
            password: f.password.trim(),
            ca_pem: &f.ca,
            project_id: "shop",
            candidate_id: "positive",
            source_hash: &source,
            gate_id: gate,
            issuer_class: if *gate == "G8" {
                "component_authority"
            } else {
                "test_issuer"
            },
            status: "pass",
            blocking: false,
            report_type: "test_issuer",
            authority_statement_sha256: None,
        })
        .await
        .unwrap();
        assert_eq!(recorded.code, 0);
    }
    for (kind, barrier) in [("fence", "closed"), ("replica", "missing")] {
        assert_eq!(
            crate::checkpoint(&crate::CheckpointRequest {
                endpoint: &f.endpoint,
                user: "oo_release_admin",
                password: f.password.trim(),
                ca_pem: &f.ca,
                project_id: "shop",
                candidate_id: "positive",
                kind,
                source_hash: &source,
                epoch: "native-test-epoch",
                barrier,
                sessions_terminated: false,
                issuer_class: "test_issuer",
                authority_statement_sha256: None
            })
            .await
            .unwrap()
            .code,
            0
        );
    }
    let now: i64 = root
        .client
        .query_one(
            "SELECT floor(extract(epoch FROM clock_timestamp()))::bigint",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    let query = json!({"aggregate":"count","column":"id","relation":"public.customer"});
    let oracle = json!({"aggregate":"sum","column":"n","relation":"public.independent_counts"});
    let mut policy = json!({"protocol":"oo.release-coordinator/business-policy/v2","scope":"test_scope","policy_revision":"native-one",
        "execution_capability_version":crate::business_execution_capability(),"project_id":"shop","candidate_id":"positive","candidate_digest":source,
        "visibility_sha256":crate::canon::canonical_sha(&visibility),"effective_sha256":crate::canon::canonical_sha(&effective),
        "role":"oo_r_clerk","actors":{"executor":"biz_executor","oracle":"biz_oracle","evaluator":"biz_evaluator","reviewer":"biz_reviewer"},
        "relations":[{"name":"public.customer","role":"oo_r_clerk","columns":["id"]},{"name":"public.independent_counts","role":"biz_oracle","columns":["n"]}],
        "checks":[{"asset":"metric:customer_count","query":query,"oracle":oracle},{"asset":"example:customer-count","query":query,"oracle":oracle},
            {"asset":"eval:business-fixture","query":query,"oracle":oracle}],"expires_at":now+1800,"receipt_ttl_seconds":300,
        "native_mapping":context,"statement_timeout_ms":5000,"execution_deadline_ms":20000});
    let pin = f.install(&policy).await;
    f.execute_review(&pin, "native-good", &mut journal).await;
    let ready = crate::plan(&f.plan(&source)).await.unwrap();
    assert_eq!(ready.code, 0, "{}", ready.body);
    business_gates(&ready, true);
    journal.push(json!({"operation":"native_plan","exit":ready.code,"receipt":ready.body}));
    let observed = journal
        .iter()
        .find(|e| e["operation"] == "execute")
        .unwrap()["receipt"]["binding"]["data_vector"]
        .clone();
    assert_eq!(observed["data_rows_transferred"], 0);
    assert_eq!(
        observed["protocol"],
        "oo.release-coordinator/native-publication-vector/v1"
    );
    assert!(
        observed["relations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["sealed_table"]["applied_rows"] == 12000)
    );
    let target = observed["relations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["relation"] == "public.customer")
        .unwrap();
    let physical = format!(
        "{}.{}",
        target["physical_schema"].as_str().unwrap(),
        target["physical_relation"].as_str().unwrap()
    );
    let column = target["physical_columns"][0].as_str().unwrap();
    assert!(
        !admin
            .client
            .query_one(
                "SELECT has_table_privilege('oo_release_admin',$1,'SELECT')",
                &[&physical]
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    assert!(
        !admin
            .client
            .query_one(
                "SELECT has_table_privilege('oo_r_clerk',$1,'SELECT')",
                &[&physical]
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    let secret:String=root.client.query_one("SELECT _oo.lookup_physical_mapping('native_business_fixture',repeat('a',64),1,'column','public','customer','secret')",&[]).await.unwrap().get(0);
    assert!(
        !admin
            .client
            .query_one(
                "SELECT has_column_privilege('oo_r_clerk',$1,$2,'SELECT')",
                &[&physical, &secret]
            )
            .await
            .unwrap()
            .get::<_, bool>(0)
    );
    let executor = crate::store::open(
        &f.endpoint,
        "biz_executor",
        f.actors["executor"].trim(),
        &f.ca,
    )
    .await
    .unwrap();
    assert!(
        executor
            .client
            .query(&format!("SELECT \"{secret}\" FROM {physical} LIMIT 1"), &[])
            .await
            .is_err()
    );
    assert!(
        executor
            .client
            .query(&format!("SELECT _oo_pos FROM {physical} LIMIT 1"), &[])
            .await
            .is_err()
    );
    // ACL changes invalidate old proof without reading the business table.
    root.client
        .batch_execute(&format!(
            "REVOKE SELECT(\"{column}\") ON {physical} FROM oo_r_clerk"
        ))
        .await
        .unwrap();
    assert!(crate::plan(&f.plan(&source)).await.is_err());
    root.client
        .batch_execute(&format!(
            "GRANT SELECT(\"{column}\") ON {physical} TO oo_r_clerk"
        ))
        .await
        .unwrap();
    // Even a same-name view over the actual sealed table is not a supported
    // ordinary C1 shadow. Do not infer overlay authority or grant its base.
    let table_name = target["physical_relation"].as_str().unwrap();
    root.client
        .batch_execute(&format!(
            "ALTER TABLE {physical} RENAME TO native_view_backup; \
         CREATE VIEW _oo.\"{table_name}\" AS SELECT * FROM _oo.native_view_backup"
        ))
        .await
        .unwrap();
    assert!(crate::plan(&f.plan(&source)).await.is_err());
    root.client.batch_execute(&format!(
        "DROP VIEW _oo.\"{table_name}\"; ALTER TABLE _oo.native_view_backup RENAME TO \"{table_name}\""
    )).await.unwrap();
    journal
        .push(json!({"operation":"native_unknown_view","refused":true,"extra_base_grant":false}));
    // A mismatched caller receipt is not rescued by valid physical mappings.
    let mut bad = policy.clone();
    bad["native_mapping"]["publication"]["receipt_sha256"] = json!("0".repeat(64));
    let bad_path = f.out.join("tampered-policy.json");
    let raw = crate::canon::canonical(&bad);
    std::fs::write(&bad_path, &raw).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bad_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        crate::install_business_policy(&f.control(), &bad_path, &crate::sha256_hex(raw.as_bytes()))
            .await
            .is_err()
    );
    // Retain a reviewed generation while the real loader publishes a new data-only cut.
    assert_eq!(
        crate::enter_applying(&f.plan(&source)).await.unwrap().code,
        0
    );
    let next = publish(&root.client, 2).await;
    write_new(&f.out.join("native-context-2.json"), &next);
    assert!(crate::commit_activation(&f.plan(&source)).await.is_err());
    assert!(
        admin
            .client
            .query_one(
                "SELECT candidate_id FROM oo_release_control.active_pointer WHERE singleton",
                &[]
            )
            .await
            .unwrap()
            .get::<_, Option<String>>(0)
            .is_none()
    );
    journal.push(json!({"operation":"real_data_only_generation_after_plan","old_receipt_rejected":true,
        "old_vector":context["publication"]["receipt"],"new_vector":next["publication"]["receipt"]}));
    assert_eq!(
        crate::recover(&crate::RecoverRequest {
            endpoint: &f.endpoint,
            user: "oo_release_admin",
            password: f.password.trim(),
            ca_pem: &f.ca,
            action: "recover",
            project_id: "shop",
            candidate_id: "positive",
            expected_version: None
        })
        .await
        .unwrap()
        .code,
        0
    );
    crate::revoke_business_policy(&f.control(), &pin)
        .await
        .unwrap();
    policy["native_mapping"] = next;
    policy["policy_revision"] = json!("native-two");
    let pin2 = f.install(&policy).await;
    f.execute_review(&pin2, "native-two", &mut journal).await;
    let ready = crate::plan(&f.plan(&source)).await.unwrap();
    assert_eq!(ready.code, 0);
    business_gates(&ready, true);
    // Scope-limited lock/statement/deadline refusal leaves no receipt or lingering work.
    crate::revoke_business_policy(&f.control(), &pin2)
        .await
        .unwrap();
    policy["policy_revision"] = json!("native-budget");
    policy["statement_timeout_ms"] = json!(100);
    policy["execution_deadline_ms"] = json!(500);
    let short = f.install(&policy).await;
    let nexttable = policy["native_mapping"]["publication"]["receipt"]["tables"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["table"] == "customer")
        .unwrap()["physical_name"]
        .as_str()
        .unwrap()
        .to_owned();
    root.client
        .batch_execute(&format!(
            "BEGIN;LOCK TABLE _oo.\"{nexttable}\" IN ACCESS EXCLUSIVE MODE"
        ))
        .await
        .unwrap();
    let start = std::time::Instant::now();
    let timeout=f.cli("business-execute",json!({"protocol":"oo.release-coordinator/business-execute/v1","policy_sha256":short,"run_id":"blocked"}),None).await;
    assert_ne!(timeout.code, 0);
    assert!(start.elapsed() < std::time::Duration::from_secs(15));
    root.client.batch_execute("ROLLBACK").await.unwrap();
    let count:i64=admin.client.query_one("SELECT count(*) FROM oo_release_control.business_receipt WHERE policy_sha=$1 AND receipt_id='blocked'",&[&short]).await.unwrap().get(0);
    assert_eq!(count, 0);
    journal.push(json!({"operation":"aggregate_statement_deadline","raw_exit":timeout.code,"elapsed_ms":start.elapsed().as_millis(),"receipts":count}));
    crate::revoke_business_policy(&f.control(), &short)
        .await
        .unwrap();
    policy["policy_revision"] = json!("native-cancel");
    policy["statement_timeout_ms"] = json!(5000);
    policy["execution_deadline_ms"] = json!(100);
    let cancelpin = f.install(&policy).await;
    root.client
        .batch_execute(&format!(
            "BEGIN;LOCK TABLE _oo.\"{nexttable}\" IN ACCESS EXCLUSIVE MODE"
        ))
        .await
        .unwrap();
    let start = std::time::Instant::now();
    let canceled = f
        .cli(
            "business-execute",
            json!({"protocol":"oo.release-coordinator/business-execute/v1",
        "policy_sha256":cancelpin,"run_id":"cancelled"}),
            None,
        )
        .await;
    assert_ne!(canceled.code, 0);
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
    root.client.batch_execute("ROLLBACK").await.unwrap();
    let remaining: i64 = root
        .client
        .query_one(
            "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() \
        AND application_name='release-coordinator' AND state='active' AND pid<>pg_backend_pid()",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(remaining, 0);
    let count:i64=admin.client.query_one("SELECT count(*) FROM oo_release_control.business_receipt WHERE policy_sha=$1 AND receipt_id='cancelled'",
        &[&cancelpin]).await.unwrap().get(0);
    assert_eq!(count, 0);
    journal.push(json!({"operation":"aggregate_client_cancel","raw_exit":canceled.code,"elapsed_ms":start.elapsed().as_millis(),
        "active_queries_after_cancel":remaining,"receipts":count}));
    crate::revoke_business_policy(&f.control(), &cancelpin)
        .await
        .unwrap();
    policy["policy_revision"] = json!("native-final");
    policy["statement_timeout_ms"] = json!(5000);
    policy["execution_deadline_ms"] = json!(20000);
    let finalpin = f.install(&policy).await;
    f.execute_review(&finalpin, "native-final", &mut journal)
        .await;
    let mut session = crate::store::open(&f.endpoint, "oo_release_admin", f.password.trim(), &f.ca)
        .await
        .unwrap();
    let proof = crate::business_positive::preflight(
        &mut session,
        "positive",
        &source,
        &effective,
        &visibility,
    )
    .await
    .unwrap()
    .unwrap();
    assert!(proof.matches_native(Some(&policy["native_mapping"])));
    assert!(!proof.matches_native(Some(&context)));
    assert_eq!(crate::plan(&f.plan(&source)).await.unwrap().code, 0);
    let active = crate::activate(&f.plan(&source)).await.unwrap();
    assert_eq!(active.code, 0, "{}", active.body);
    assert_eq!(
        active.body["certificate_class"],
        "test_scope_not_production"
    );
    assert_eq!(active.body["production_ready_certificate"], false);
    journal.push(
        json!({"operation":"native_test_activation","exit":active.code,"receipt":active.body}),
    );
    let stored=admin.client.query("SELECT kind,actor,body,body_sha FROM oo_release_control.business_receipt ORDER BY recorded_at",&[]).await.unwrap();
    let receipts=stored.iter().map(|r|json!({"kind":r.get::<_,String>(0),"authenticated_actor":r.get::<_,String>(1),
        "body":serde_json::from_str::<Value>(&r.get::<_,String>(2)).unwrap(),"body_sha":r.get::<_,String>(3)})).collect::<Vec<_>>();
    for r in &receipts {
        if let Some(results) = r["body"]["results"].as_object() {
            for result in results.values() {
                assert_eq!(result["scalar"], "12000");
                assert_eq!(result["returned_rows"], 1);
            }
        }
    }
    write_new(
        &f.out.join("native-positive-chain.json"),
        &json!({"journal":journal,"immutable_receipts":receipts,
        "native_rows":12000,"data_hash_rows_transferred":0,"customer_db_access":false,"actual_mysql_source_capture":false,
        "real_pipeline_migrations_and_publication":true,"production_admissible":false,"business_owner_signed":false}),
    );
}
