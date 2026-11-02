//! Formal router + real owned PG/ReviewLedger + native Pack compiler.
//! Identity is a pinned TLS contract fixture, NOT a customer login/provider.
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::AtomicBool};

use axum::body::{Body, to_bytes};
use axum::http::Request;
use selfbuild_control_api::{AppConfig, config, library, router, store};
use serde_json::{Value, json};
use tower::ServiceExt;

fn read(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn write(path: impl AsRef<Path>, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

async fn call(
    config: &AppConfig,
    method: &str,
    uri: &str,
    body: Value,
    session: &str,
) -> (u16, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("origin", &config.identity.origin)
        .header("x-oo-csrf", "c".repeat(43));
    if !session.is_empty() {
        request = request.header("cookie", format!("__Host-oo-session={session}"));
    }
    let response = router(config.clone())
        .oneshot(
            request
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 67_108_864).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn reviewed_build(config: &AppConfig, case_dir: &Path) -> store::BuildRow {
    let project = &config.projects[0];
    let (status, created) = call(
        config,
        "POST",
        "/control/v1/builds",
        json!({
            "project_id": project.project_id, "config_commit": project.config_commit,
            "source_ref": project.source_ref, "allowance_id": project.allowance_id,
            "llm_secret_ref": project.llm_secret_ref, "identity_revision": 1,
            "idempotency_key": "f20_create_0000000001"
        }),
        &"s".repeat(43),
    )
    .await;
    assert_eq!(status, 202, "{created}");
    let build_id = created["build_id"].as_str().unwrap();
    let row = store::load_build_any(&config.postgres, build_id)
        .await
        .unwrap()
        .unwrap();
    let dir = Path::new(&config.work_dir).join("builds").join(build_id);
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["facts.json", "drafts.json"] {
        std::fs::copy(case_dir.join(name), dir.join(name)).unwrap();
    }
    let db = store::connect(&config.postgres).await.unwrap();
    // Observation/drafting are retained synthetic fixtures. No worker/source/model starts.
    db.execute(
        "UPDATE oo_selfbuild_api.build_job SET status='running' WHERE build_id=$1",
        &[&build_id],
    )
    .await
    .unwrap();
    for stage in ["S6", "S7", "S8"] {
        assert!(
            library::run(
                &db,
                project,
                &row,
                stage,
                &dir,
                &Arc::new(AtomicBool::new(false))
            )
            .await,
            "{stage} failed in {}",
            dir.display()
        );
    }
    let members = store::list_members(&config.postgres, build_id)
        .await
        .unwrap();
    for (i, member) in members.iter().enumerate() {
        let hidden = project.hidden_columns.contains(&member.object_id);
        let mut body = json!({
            "build_id": build_id, "identity_revision": 1, "expected_revision": 1,
            "idempotency_key": format!("f20_label_{i:016}"),
            "decision_kind": "label_description", "choice": if hidden {"reject"} else {"approve"},
            "candidate_sha256": member.candidate_sha256
        });
        if !hidden {
            body["fields"] = json!({"label":{"overwrite":false},"description":{"overwrite":false}});
            if member.object_kind == "dataset" {
                body["fields"]["grain"] = json!({"overwrite":false});
            }
        }
        let (status, reply) = call(
            config,
            "POST",
            &format!("/control/v1/reviews/{}/decision", member.object_id),
            body,
            &"s".repeat(43),
        )
        .await;
        assert_eq!(status, 200, "{reply}");
        assert_eq!(reply["ledger_authenticated"], false);
        assert_eq!(reply["grants"], "not_authorized");
    }
    let review = read(case_dir.join("expected-review.json"));
    let items: Vec<Value> = review["columns"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(id, c)| {
            json!({"object_id":id, "pii":c["pii"], "semantic_type":c["semantic_type"],
               "explicit_allow":true, "reason":"caller must not authorize reads"})
        })
        .collect();
    let (status, reply) = call(
        config,
        "POST",
        "/control/v1/classifications/confirm",
        json!({
            "project_id":project.project_id, "build_id":build_id, "identity_revision":1,
            "idempotency_key":"f20_class_0000000001", "items":items
        }),
        &"s".repeat(43),
    )
    .await;
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["data_grants"], "none");
    assert_eq!(reply["grants"], "not_authorized");
    row
}

#[tokio::test]
#[ignore = "run with tests/run_f20_owned.py; requires owned PG and pinned binaries"]
async fn owned_pg_ledger_router_and_local_pack_parity() {
    let root = PathBuf::from(std::env::var("OO_F20_FIXTURE_ROOT").expect("owned F20 root"));
    let cases = read(root.join("cases.json"));
    let mut results = Vec::new();
    for name in cases
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
    {
        let dir = root.join(name);
        let config =
            config::load_config_file(dir.join("api-config.json").to_str().unwrap()).unwrap();
        assert!(
            config.postgres.database.starts_with("f20_") && config.postgres.host == "127.0.0.1"
        );
        store::install_schema(&config.postgres).await.unwrap();
        let row = reviewed_build(&config, &dir).await;
        let uri = format!("/control/v1/builds/{}/pack", row.build_id);
        let body = json!({"identity_revision":1,"idempotency_key":"f20_compile_00000001"});
        if name == "allowed" {
            let local = dir.join("local");
            let request = read(local.join("compiler-request.json"));
            let binding = read(local.join("oos-tree-binding.json"));
            oo_builder::compile_oos_tree(
                &local.join("ontology"),
                &binding.to_string(),
                &request["visibility"].to_string(),
                &config.projects[0].budgets.role_pack,
            )
            .expect("local native tree must be accepted with the same API budgets");
        }
        if name == "generation_drift" {
            let merged = Path::new(&config.work_dir)
                .join("builds")
                .join(&row.build_id)
                .join("merged.json");
            let mut changed = read(&merged);
            changed["tables"][0]["columns"][0]["comment"] = json!("drift after ledger review");
            write(&merged, &changed);
        }
        let (status, reply) = call(&config, "POST", &uri, body.clone(), &"s".repeat(43)).await;
        write(
            dir.join("route-result.json"),
            &json!({"status":status,"body":reply}),
        );
        if name != "allowed" {
            assert_ne!(status, 200, "{name} unexpectedly compiled: {reply}");
            assert!(
                store::find_pack(&config.postgres, &row.build_id)
                    .await
                    .unwrap()
                    .is_none()
            );
            let expected_reason = match name {
                "missing" | "absent_policy" => "pii_read_grant_required",
                "hidden" => "read_grant_hidden",
                "expired" => "read_grant_expired",
                "wrong_role" => "read_grant_scope",
                "digest" => "column_review_digest",
                "generation_drift" => "membership.refused",
                "required_assets" => "pack.refused",
                _ => panic!("unknown negative case"),
            };
            if matches!(name, "generation_drift" | "required_assets") {
                assert_eq!(reply["error"]["code"], expected_reason);
            } else {
                let child = read(
                    Path::new(&config.work_dir)
                        .join("builds")
                        .join(&row.build_id)
                        .join("compile-child.stdout"),
                );
                assert_eq!(child["reason"], expected_reason, "{child}");
            }
            results.push(
                json!({"case":name,"status":status,"code":reply["error"]["code"],
                               "reason":expected_reason,"pack_written":false}),
            );
            continue;
        }
        assert_eq!(status, 200, "{reply}");
        assert_eq!(reply["management_identity"], "verified");
        assert_eq!(reply["published"], false);
        assert_eq!(reply["data_grants"], "none");
        let local = dir.join("local");
        let local_request = read(local.join("compiler-request.json"));
        let binding = read(local.join("oos-tree-binding.json"));
        let compiled = oo_builder::compile_oos_tree(
            &local.join("ontology"),
            &binding.to_string(),
            &local_request["visibility"].to_string(),
            &config.projects[0].budgets.role_pack,
        )
        .unwrap();
        let artifact: Value = serde_json::from_str(&compiled.pack.artifact).unwrap();
        assert_eq!(compiled.effective_oos, local_request["effective_oos"]);
        assert_eq!(reply["producer"]["pack_digest"], compiled.pack.pack_digest);
        assert_eq!(reply["producer"]["artifact"], artifact);
        let http_dir = Path::new(&config.work_dir)
            .join("builds")
            .join(&row.build_id);
        assert_eq!(
            read(http_dir.join("effective-oos.json")),
            local_request["effective_oos"]
        );
        assert_eq!(
            read(http_dir.join("visibility.json")),
            local_request["visibility"]
        );
        write(dir.join("local-native-pack.json"), &artifact);
        let v = &local_request["visibility"]["datasets"][0]["columns"];
        assert_eq!(v, &json!(["bank_account", "id", "note", "phone"]));
        assert_eq!(
            call(&config, "GET", &uri, json!({}), &"s".repeat(43))
                .await
                .0,
            200
        );
        assert_eq!(
            call(&config, "POST", &uri, body.clone(), &"s".repeat(43))
                .await
                .0,
            200
        );
        assert_eq!(call(&config, "POST", &uri, body.clone(), "").await.0, 401);
        // business.record alone cannot compile, even though it can classify.
        assert_eq!(
            call(&config, "POST", &uri, body.clone(), &"b".repeat(43))
                .await
                .0,
            403
        );
        let policy_path = dir.join("operator-columns.json");
        let original = std::fs::read(&policy_path).unwrap();
        std::fs::write(&policy_path, b"{}").unwrap();
        let revoked_get = call(&config, "GET", &uri, json!({}), &"s".repeat(43))
            .await
            .0;
        let revoked_post = call(&config, "POST", &uri, body.clone(), &"s".repeat(43))
            .await
            .0;
        assert_ne!(revoked_get, 200);
        assert_ne!(revoked_post, 200);
        std::fs::write(&policy_path, original).unwrap();
        let mut moved_role = config.clone();
        moved_role.projects[0].visible_columns.pop();
        assert_ne!(
            call(&moved_role, "POST", &uri, body.clone(), &"s".repeat(43))
                .await
                .0,
            200
        );
        let grant = read(&policy_path);
        let expires = grant["review"]["columns"]["app.items.phone"]["read_grant_expires_at"]
            .as_u64()
            .unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        tokio::time::sleep(std::time::Duration::from_secs(
            expires.saturating_sub(now) + 1,
        ))
        .await;
        let expired_get = call(&config, "GET", &uri, json!({}), &"s".repeat(43))
            .await
            .0;
        let expired_post = call(&config, "POST", &uri, body, &"s".repeat(43)).await.0;
        assert_ne!(expired_get, 200);
        assert_ne!(expired_post, 200);
        assert_eq!(
            read(http_dir.join("compile-child.stdout"))["reason"],
            "read_grant_expired"
        );
        results.push(
            json!({"case":name,"status":status,"pack_digest":compiled.pack.pack_digest,
            "local_http_effective_visibility_artifact_equal":true, "anonymous":401,
            "business_record_only":403, "revoked_cached_get":revoked_get,
            "revoked_cached_compile":revoked_post, "expired_cached_get":expired_get,
            "expired_cached_compile":expired_post, "expiry_test_uses_real_clock":true}),
        );
    }
    write(
        root.join("route-results.json"),
        &json!({
            "evidence_kind":"formal_router_owned_pg_real_ledger_tls_identity_contract_fixture",
            "customer_http":false,"model_calls":0,"source_calls":0,"published":false,"cases":results
        }),
    );
}
