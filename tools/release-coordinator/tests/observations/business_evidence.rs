use super::*;

struct Fixture {
    effective: Value,
    visibility: Value,
    coverage: Value,
    policy: Value,
    evals: BTreeMap<String, Value>,
    expected: BTreeMap<String, String>,
}

impl Fixture {
    fn new() -> Self {
        let result = canonical_sha(&json!({"rows":[{"total":"42.00"}],"complete":true}));
        let coverage = json!({"source":"fixture-local","dataset":"app.orders","scope":"all_rows"});
        let effective = json!({
            "protocol":"oo.builder.effective-oos/v1", "project":"fixture",
            "ontology":{"publish":{"require_eval":["totals-v1"]}},
            "datasets":[{"dataset":"app.orders","columns":[{"name":"amount","data_type":"numeric",
                "unit":"USD","organization_scope":"fixture-org","pii":"none"}]}],
            "relationships":[], "enums":[],
            "metrics":[{"metric":"revenue","certified":true,"unit":"USD","grain":["order"],
                "sql":"SELECT sum(amount) FROM app.orders",
                "tests":[{"params":{},"expect":{"rows":1}}],
                "control_total":{"sql":"SELECT sum(line_amount) FROM app.independent_lines",
                    "source":"independent-fixture-oracle","tolerance":{"abs":"0"}}}],
            "examples":[{"id":"orders-total","sql":"SELECT sum(amount) FROM app.orders",
                "uses":["dataset:app.orders"],"verified_by":"fixture-reviewer",
                "last_checked":{"status":"ok","result_hash":result}}]
        });
        let visibility = json!({"project":"fixture","role":"oo_r_reader","policy":{"id":"p","version":"1"},
            "datasets":[{"dataset":"app.orders","columns":["amount"]}]});
        let mut fixture = Self {
            effective,
            visibility,
            coverage: coverage.clone(),
            policy: json!({"issuer":"controlled-fixture-executor","issuer_class":"test_issuer",
                "scope":{"scope":"test_scope","project_id":"fixture","candidate_id":"candidate-1",
                    "role":"oo_r_reader","gate_ids":["G5a","G5b","G6","G10","G11"],
                    "source_coverage_digest":canonical_sha(&coverage)},
                "policy_version":"1","not_before":900,"expires_at":2000,"revoked":false}),
            evals: BTreeMap::from([(
                "totals-v1".into(),
                json!({"suite":"totals-v1",
                "question_ids":["total"],"oracle_revision":"independent-v1",
                "plan":{"sql":"SELECT sum(amount) FROM app.orders"}}),
            )]),
            expected: BTreeMap::new(),
        };
        for key in required(&fixture.context()).unwrap().keys() {
            fixture.expected.insert(key.clone(), result.clone());
        }
        fixture
    }

    fn context(&self) -> BusinessEvidenceContext<'_> {
        BusinessEvidenceContext {
            project_id: "fixture",
            candidate_id: "candidate-1",
            candidate_digest: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            base_candidate_digest: None,
            effective: &self.effective,
            visibility: &self.visibility,
            data_vector_digest: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            source_coverage: &self.coverage,
            execution_capability_version: "candidate-executor-v1",
            eval_plans: &self.evals,
            expected_results: &self.expected,
            issuer_policy: &self.policy,
            now_unix_seconds: 1000,
        }
    }

    fn receipts(&self) -> Vec<Value> {
        let ctx = self.context();
        required(&ctx).unwrap().iter().enumerate().map(|(i,(key,definition))| {
            let (gate,asset) = key.split_once('/').unwrap();
            let expected = &ctx.expected_results[key];
            let mut receipt = json!({
                "evidence_id":format!("receipt-{i}"),"evidence_kind":kind(gate).unwrap(),
                "evidence_schema":BUSINESS_EVIDENCE_SCHEMA,"issuer":self.policy["issuer"],
                "issuer_scope":self.policy["scope"],"issuer_policy_digest":canonical_sha(&self.policy),
                "candidate_id":ctx.candidate_id,"candidate_digest":ctx.candidate_digest,
                "semantic_definition_digest":canonical_sha(definition),
                "dependency_digest":canonical_sha(ctx.effective),"data_vector_digest":ctx.data_vector_digest,
                "source_coverage":self.coverage,"role":self.visibility["role"],
                "policy_digest":canonical_sha(&self.visibility),"sql_or_plan_digest":plan_digest(&ctx,gate,definition),
                "execution_capability_version":ctx.execution_capability_version,
                "expected_result_digest":expected,"observed_result_digest":expected,
                "started_at":950,"completed_at":980,"expires_at":1100,
                "outcome":"pass","unresolved_count":0,"gate_id":gate,"asset_id":asset
            });
            rebind_reference(&mut receipt);
            receipt
        }).collect()
    }
}

fn rebind_reference(receipt: &mut Value) {
    receipt
        .as_object_mut()
        .unwrap()
        .remove("authenticated_receipt_reference");
    receipt["authenticated_receipt_reference"] = json!({
        "issuer":receipt["issuer"],"receipt_id":receipt["evidence_id"],"body_sha256":canonical_sha(receipt)
    });
}

#[test]
fn all_five_well_bound_claims_remain_untrusted_not_a_test_or_production_pass() {
    let fixture = Fixture::new();
    let receipts = fixture.receipts();
    assert_eq!(receipts.len(), 5);
    let result = check_business_evidence(&fixture.context(), &receipts).unwrap();
    assert_eq!(result["bindings_validated"], true);
    assert_eq!(result["test_admissible"], false);
    assert_eq!(result["production_admissible"], false);
    assert_eq!(result["business_owner_signed"], false);
    assert_eq!(result["reason"], "business_issuer_unavailable");
    if let Ok(directory) = std::env::var("OO_RELEASE_GATE_EVIDENCE_DIR") {
        let path = std::path::Path::new(&directory).join("well-bound-but-untrusted.json");
        let document = json!({"fixture_only":true,"effective":fixture.effective,
            "visibility":fixture.visibility,"issuer_policy_claim":fixture.policy,
            "receipts":receipts,"validation_result":result,
            "actual_authenticated_issuer":false,"executed_customer_queries":false});
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        file.write_all(canonical(&document).as_bytes()).unwrap();
    }
}

#[test]
fn independently_rehashed_field_tampering_does_not_override_current_context() {
    let fixture = Fixture::new();
    let original = fixture.receipts();
    let changes = [
        ("candidate_id", json!("other")),
        ("candidate_digest", json!("c".repeat(64))),
        ("semantic_definition_digest", json!("c".repeat(64))),
        ("dependency_digest", json!("c".repeat(64))),
        ("data_vector_digest", json!("c".repeat(64))),
        ("role", json!("oo_r_other")),
        ("policy_digest", json!("c".repeat(64))),
        ("source_coverage", json!({"scope":"subset"})),
        ("sql_or_plan_digest", json!("c".repeat(64))),
        ("execution_capability_version", json!("v2")),
        ("observed_result_digest", json!("c".repeat(64))),
        ("expected_result_digest", json!("c".repeat(64))),
        ("issuer", json!("other")),
        ("issuer_policy_digest", json!("c".repeat(64))),
        ("issuer_scope", json!({"scope":"production"})),
        ("outcome", json!("fail")),
        ("unresolved_count", json!(1)),
        ("evidence_kind", json!("owner_signed")),
        ("evidence_schema", json!("unknown")),
        ("asset_id", json!("metric:not-required")),
    ];
    for (field, value) in changes {
        let mut receipts = original.clone();
        receipts[0][field] = value;
        rebind_reference(&mut receipts[0]);
        assert!(
            check_business_evidence(&fixture.context(), &receipts).is_err(),
            "{field}"
        );
    }
    let mut receipts = original.clone();
    receipts[0]["expected_result_digest"] = json!("c".repeat(64));
    receipts[0]["observed_result_digest"] = json!("c".repeat(64));
    rebind_reference(&mut receipts[0]);
    assert_eq!(
        check_business_evidence(&fixture.context(), &receipts).unwrap_err(),
        "business_result_binding"
    );
}

#[test]
fn sql_unit_organization_scope_and_dependency_changes_invalidate_existing_claims() {
    for path in [
        "/metrics/0/sql",
        "/metrics/0/unit",
        "/metrics/0/control_total/sql",
        "/datasets/0/columns/0/organization_scope",
        "/examples/0/sql",
    ] {
        let mut fixture = Fixture::new();
        let receipts = fixture.receipts();
        *fixture.effective.pointer_mut(path).unwrap() = json!("changed");
        assert!(
            check_business_evidence(&fixture.context(), &receipts).is_err(),
            "{path}"
        );
    }
    let mut fixture = Fixture::new();
    let receipts = fixture.receipts();
    fixture.evals.get_mut("totals-v1").unwrap()["oracle_revision"] = json!("independent-v2");
    assert!(check_business_evidence(&fixture.context(), &receipts).is_err());
}

#[test]
fn expiry_future_execution_scope_revocation_and_policy_version_are_rechecked() {
    let fixture = Fixture::new();
    for (field, value) in [
        ("started_at", json!(899)),
        ("started_at", json!(981)),
        ("completed_at", json!(1001)),
        ("expires_at", json!(1000)),
        ("expires_at", json!(2001)),
        ("started_at", json!(-1)),
    ] {
        let mut receipts = fixture.receipts();
        receipts[0][field] = value;
        rebind_reference(&mut receipts[0]);
        assert!(
            check_business_evidence(&fixture.context(), &receipts).is_err(),
            "{field}"
        );
    }
    let mut ctx = fixture.context();
    ctx.now_unix_seconds = 1100;
    assert!(check_business_evidence(&ctx, &fixture.receipts()).is_err());
    for (field, value) in [
        ("revoked", json!(true)),
        ("policy_version", json!("2")),
        ("expires_at", json!(999)),
        ("not_before", json!(1001)),
        ("issuer_class", json!("production_issuer")),
    ] {
        let mut fixture = Fixture::new();
        let receipts = fixture.receipts();
        fixture.policy[field] = value;
        assert!(
            check_business_evidence(&fixture.context(), &receipts).is_err(),
            "{field}"
        );
    }
    for field in [
        "scope",
        "project_id",
        "candidate_id",
        "role",
        "source_coverage_digest",
    ] {
        let mut fixture = Fixture::new();
        let receipts = fixture.receipts();
        fixture.policy["scope"][field] = json!("changed");
        assert!(
            check_business_evidence(&fixture.context(), &receipts).is_err(),
            "{field}"
        );
    }
}

#[test]
fn missing_duplicate_unrequired_claims_and_self_asserted_acceptance_are_rejected() {
    let fixture = Fixture::new();
    let original = fixture.receipts();
    assert!(check_business_evidence(&fixture.context(), &[]).is_err());
    for field in FIELDS {
        let mut receipts = original.clone();
        receipts[0].as_object_mut().unwrap().remove(*field);
        assert!(
            check_business_evidence(&fixture.context(), &receipts).is_err(),
            "{field}"
        );
    }
    for field in [
        "accepted",
        "authenticated",
        "business_owner_signed",
        "production_admissible",
        "force",
    ] {
        let mut receipts = original.clone();
        receipts[0][field] = json!(true);
        rebind_reference(&mut receipts[0]);
        assert!(
            check_business_evidence(&fixture.context(), &receipts).is_err(),
            "{field}"
        );
    }
    let mut duplicate = original.clone();
    duplicate[1] = duplicate[0].clone();
    assert!(check_business_evidence(&fixture.context(), &duplicate).is_err());
    let mut fixture = Fixture::new();
    let receipts = fixture.receipts();
    fixture.effective["metrics"] = json!([]);
    assert!(check_business_evidence(&fixture.context(), &receipts).is_err());
}

#[test]
fn reference_hash_is_only_integrity_and_cannot_prove_issuer_identity() {
    let fixture = Fixture::new();
    let mut receipts = fixture.receipts();
    receipts[0]["authenticated_receipt_reference"]["body_sha256"] = json!("c".repeat(64));
    assert_eq!(
        check_business_evidence(&fixture.context(), &receipts).unwrap_err(),
        "business_reference_binding"
    );
    rebind_reference(&mut receipts[0]);
    assert_eq!(
        check_business_evidence(&fixture.context(), &receipts).unwrap()["test_admissible"],
        false
    );
}

#[test]
fn duplicate_reference_and_narrowed_issuer_scope_are_rejected_even_after_rehashing() {
    let fixture = Fixture::new();
    let mut receipts = fixture.receipts();
    receipts[1]["evidence_id"] = receipts[0]["evidence_id"].clone();
    rebind_reference(&mut receipts[1]);
    assert_eq!(
        check_business_evidence(&fixture.context(), &receipts).unwrap_err(),
        "business_duplicate_reference"
    );
    let mut fixture = Fixture::new();
    let mut receipts = fixture.receipts();
    fixture.policy["scope"]["gate_ids"] = json!(["G5a", "G5b", "G6", "G10"]);
    for receipt in &mut receipts {
        receipt["issuer_scope"] = fixture.policy["scope"].clone();
        receipt["issuer_policy_digest"] = json!(canonical_sha(&fixture.policy));
        rebind_reference(receipt);
    }
    assert_eq!(
        check_business_evidence(&fixture.context(), &receipts).unwrap_err(),
        "business_issuer_binding"
    );
}

#[test]
fn current_data_vector_or_independent_oracle_changes_require_new_evidence() {
    let fixture = Fixture::new();
    let receipts = fixture.receipts();
    let new_vector = "c".repeat(64);
    let mut ctx = fixture.context();
    ctx.data_vector_digest = &new_vector;
    assert_eq!(
        check_business_evidence(&ctx, &receipts).unwrap_err(),
        "business_context_binding"
    );
    let mut fixture = Fixture::new();
    let receipts = fixture.receipts();
    for value in fixture.expected.values_mut() {
        *value = "d".repeat(64);
    }
    assert_eq!(
        check_business_evidence(&fixture.context(), &receipts).unwrap_err(),
        "business_result_binding"
    );
}

#[test]
fn current_business_requirements_override_legacy_test_passes_and_stale_maintenance() {
    let fixture = Fixture::new();
    let facts: Vec<_> = crate::decide::GATE_IDS
        .iter()
        .map(|gate| crate::decide::GateFact {
            gate_id: (*gate).into(),
            source_hash: "a".repeat(64),
            issuer_class: "test_issuer".into(),
            status: "pass".into(),
            blocking: false,
            scope: "test_scope".into(),
            report_type: "test_issuer".into(),
            authority_statement_sha256: None,
            certificate_sha256: canonical_sha(&json!(gate)),
        })
        .collect();
    let mut gates = crate::decide::summarize_gates(&facts, &"a".repeat(64), &"b".repeat(64), &[]);
    assert!(gates.iter().all(|gate| gate.test_admissible));
    for gate in &mut gates {
        if ["G5a", "G5b", "G6", "G10", "G11"].contains(&gate.gate_id.as_str()) {
            gate.disposition = "not_applicable".into();
            gate.test_admissible = false;
        }
    }
    crate::apply::enforce_preconditions(&fixture.effective, &mut gates).unwrap();
    for gate in gates {
        if ["G5a", "G5b", "G6", "G10", "G11"].contains(&gate.gate_id.as_str()) {
            assert_eq!(gate.disposition, "blocked");
            assert!(!gate.test_admissible);
            assert!(
                crate::apply::blocked_precondition(&fixture.effective, &gate.gate_id)
                    .unwrap()
                    .is_some()
            );
        } else {
            assert!(gate.test_admissible);
        }
        assert!(!gate.production_admissible);
    }
}
