//! Scope, audit, and activation journeys. Library calls are not the CLI proof.

use std::collections::BTreeSet;

use serde_json::Value;
use source_conformance::activate::validate_activation;
use source_conformance::audit::parse_audit;
use source_conformance::gitobj::MemoryStore;
use source_conformance::{run_scope, ScopeInput};

const MATRIX: &str = r#"
schema: oo-support-matrix/v1
levels: [candidate, designed, experimental, verified, certified, excluded]
targets:
  - { id: postgres-selfmanaged, family: postgres, product: PostgreSQL, versions: ["16.9"], variant: self-managed,
      caps: { full: verified, cat: designed } }
  - { id: sqlserver-selfmanaged, family: mssql, product: "SQL Server", versions: ["2022"], variant: self-managed,
      caps: { full: designed } }
"#;

fn sha(byte: &str) -> String {
    byte.repeat(20)
}

fn codes(report: &source_conformance::ScopeReport) -> BTreeSet<String> {
    report.findings.iter().filter(|finding| finding.severity == "error").map(|finding| finding.code.clone()).collect()
}

fn dispatch() -> String {
    let pg = sha("11");
    let mysql = sha("12");
    let maria = sha("13");
    let doris = sha("14");
    let star = sha("15");
    let mongo = sha("16");
    let open = sha("17");
    let review75 = sha("75");
    let review76 = sha("76");
    let review77 = sha("77");
    let intervals = sha("74");
    let yaml = sha("71");
    format!(
        r#"{{"tasks":[
        {{"task_id":"DL01-W1-46","name":"postgres-product","status":"running","latest_author_delivery":"{pg}"}},
        {{"task_id":"DL01-W1-06","name":"mysql","status":"running","latest_author_delivery":"{mysql}"}},
        {{"task_id":"DL01-W1-39","name":"mariadb-driver-preflight","status":"waiting","latest_author_delivery":"{maria}"}},
        {{"task_id":"DL01-W1-09","name":"mssql","status":"implementation_author_pending_external_source","cli_exit":0}},
        {{"task_id":"DL01-W1-10","name":"oracle","status":"setup_dependency_review_passed","cli_exit":0}},
        {{"task_id":"DL01-W1-11","name":"clickhouse","status":"author_preparation_runtime_review_pending","cli_exit":0}},
        {{"task_id":"DL01-W1-69","name":"clickhouse-full-source-review","status":"fixed_review_passed","cli_exit":0}},
        {{"task_id":"DL01-W1-43","name":"doris-driver-preflight","status":"waiting","latest_author_delivery":"{doris}"}},
        {{"task_id":"DL01-W1-45","name":"starrocks-driver","status":"waiting","latest_author_delivery":"{star}"}},
        {{"task_id":"DL01-W1-41","name":"mongodb-driver-preflight","status":"waiting","latest_author_delivery":"{mongo}"}},
        {{"task_id":"DL01-W1-44","name":"opensearch-driver-preflight","status":"waiting","latest_author_delivery":"{open}"}},
        {{"task_id":"DL01-W1-75","name":"publish-maintenance-fixed-critical-review","status":"critical_review_request_changes","cli_exit":0,"latest_author_delivery":"{review75}"}},
        {{"task_id":"DL01-W1-76","name":"public-gateway-fixed-critical-review","status":"critical_review_request_changes","cli_exit":0,"latest_author_delivery":"{review76}"}},
        {{"task_id":"DL01-W1-77","name":"native-loader-fixed-critical-review","status":"critical_review_request_changes","cli_exit":0,"latest_author_delivery":"{review77}"}},
        {{"task_id":"DL01-W1-74","name":"capture-jobs-schedule-lease","status":"running","latest_author_delivery":"{intervals}","complete_capability_pending":"actual update/delete and configured intervals"}},
        {{"task_id":"DL01-W1-71","name":"full-selfbuild-yaml-maintenance","status":"running","latest_author_delivery":"{yaml}"}}
    ]}}"#
    )
}

fn intake() -> String {
    format!(
        r#"[
        {{"task":"06","fixed_sha":"{}","changed_manifests":[],"product_acceptance":false,"inventory_new_delivery_not_inferred_from_old":false}},
        {{"task":"41","fixed_sha":"{}","changed_manifests":[],"product_acceptance":false,"inventory_new_delivery_not_inferred_from_old":false}},
        {{"task":"75","fixed_sha":"{}","changed_manifests":[],"product_acceptance":false,"inventory_new_delivery_not_inferred_from_old":true}}
    ]"#,
        sha("12"),
        sha("99"),
        sha("75")
    )
}

fn frontier() -> String {
    r#"{"source_acceptance_count":0,"controlled_preview_accepted_source_products":{"mariadb":{"scope":"preview"}},"external_target_gaps":{"sqlserver":"endpoint gap"}}"#.to_owned()
}

fn store() -> MemoryStore {
    let mut store = MemoryStore::new();
    for byte in ["11", "12", "13", "14", "15", "16", "17", "71", "74"] {
        store.insert_commit(&sha(byte));
    }
    let review = "REQUEST_CHANGES\n这项是 notrun\n";
    store.insert(&sha("75"), "execution/reviews/dl01-wave1-publish-maintenance-fixed-review/REVIEW.md", &sha("a1"), review);
    store.insert(
        &sha("75"),
        "evidence/67a28659-717e-417f-a26b-2f981ae5c64c/DL01/publish-maintenance-fixed-review/linux/rework-699ca96/probe-elf.file",
        &sha("a2"),
        "ELF 64-bit\nprobe_elf_class=debug_review_not_author_release\n",
    );
    store.insert(
        &sha("75"),
        "evidence/67a28659-717e-417f-a26b-2f981ae5c64c/DL01/publish-maintenance-fixed-review/linux/rework-699ca96/probe.exit",
        &sha("a3"),
        "0\n",
    );
    store.insert(
        &sha("76"),
        "execution/reviews/dl01-wave1-public-gateway-fixed-review/REVIEW.md",
        &sha("b1"),
        "判定：`REQUEST_CHANGES`。\n没有 Agent 对话。\n",
    );
    store.insert(
        &sha("76"),
        "evidence/72f2dc43-1371-481f-bbe2-85e9765847c6/DL01/public-gateway-fixed-review/identity/file.txt",
        &sha("b2"),
        "ELF 64-bit LSB pie executable\n",
    );
    store.insert(
        &sha("76"),
        "evidence/72f2dc43-1371-481f-bbe2-85e9765847c6/DL01/public-gateway-fixed-review/identity/build.exit",
        &sha("b3"),
        "0\n",
    );
    store.insert(
        &sha("77"),
        "execution/reviews/dl01-wave1-native-loader-fixed-review/REVIEW.md",
        &sha("c1"),
        "判定：`REQUEST_CHANGES`。\nsource_bound true\njobs_lease_receipt 为 null\nexpired-pos-child\n无新批次\n",
    );
    store.insert(
        &sha("77"),
        "evidence/4f3857b5-e329-44e9-89a6-0c8b1a97c51f/DL01/native-loader-fixed-review/raw/phases/core-source-bound-retest.exit",
        &sha("c2"),
        "0\n",
    );
    store.insert(&sha("77"), "raw/fail.exit", &sha("c3"), "101\n");
    store.insert(&sha("77"), "raw/self.json", &sha("c4"), r#"{"source_bound":true,"jobs_lease_receipt":null}"#);
    store.insert(&sha("77"), "raw/audit-bad.txt", &sha("c5"), b"not an audit");
    store.insert(&sha("77"), "raw/audit-ok.txt", &sha("c6"), b"oo-source-audit/v1\npostgres R0 SELECT 1\n");
    store
}

fn input(claim: Option<String>) -> ScopeInput {
    ScopeInput {
        dispatch_json: dispatch(),
        intake_json: intake(),
        matrix_yaml: MATRIX.to_owned(),
        frontier_json: Some(frontier()),
        claim_json: claim,
        package_ok: None,
        package_error_codes: Vec::new(),
    }
}

#[test]
fn period_scope_lists_thirteen_cases_and_refuses_signoff() {
    let report = run_scope(&input(None), &store());
    assert!(report.report_complete);
    assert!(!report.ok);
    assert_eq!(report.exit_code, 1);
    assert!(!report.promoted);
    assert!(!report.matrix_rewritten);
    assert!(!report.sources_signed_off);
    assert_eq!(report.source_acceptance, "0/10");
    assert_eq!(report.accepted_sources, 0);
    assert_eq!(report.manufactured_nightly_days, 0);
    assert!(!report.any_level_raised);
    assert!(!report.parent.parent_accepted);
    assert_eq!(report.products.len(), 10);
    assert_eq!(report.case_map.len(), 13);
    for product in &report.products {
        assert_eq!(product.cc.len(), 13, "{}", product.product);
        assert!(!product.whole_source_accepted);
        assert!(product.cc.iter().all(|row| row.status != "pass" && !row.counts_as_probe));
    }
    let sqlserver = report.products.iter().find(|product| product.product == "sqlserver").unwrap();
    assert!(sqlserver.cc.iter().all(|row| row.status == "notrun"));
    assert_eq!(sqlserver.endpoint, "notrun");
    let mariadb = report.products.iter().find(|product| product.product == "mariadb").unwrap();
    assert!(mariadb.controlled_preview_not_whole_source);
    let mongo = report.products.iter().find(|product| product.product == "mongodb").unwrap();
    assert_eq!(mongo.sha_agreement, "drift");
    assert!(report.chains.iter().all(|chain| !chain.upgraded && !chain.counts_as_probe));
    let loader = report.chains.iter().find(|chain| chain.id == "native_loader_rereview").unwrap();
    assert!(loader.exit_zero_is_not_acceptance);
    assert_eq!(loader.original_exit, "request_changes");
    assert!(loader.review.is_some());
    let gateway = report.chains.iter().find(|chain| chain.id == "gateway_agent_multiturn").unwrap();
    assert!(gateway.elf.as_ref().is_some_and(|elf| elf.sha256.len() == 64));
    let verified = report.degradation.iter().find(|row| row.target_id == "postgres-selfmanaged" && row.capability == "batch_full").unwrap();
    assert_eq!(verified.before, "verified");
    assert_eq!(verified.after, "experimental");
    assert!(verified.downgrade_recommended);
    assert!(verified.unverified_this_cycle);
    assert!(!verified.matrix_rewritten);
    let designed = report.degradation.iter().find(|row| row.capability == "catalog").unwrap();
    assert_eq!(designed.before, "designed");
    assert_eq!(designed.after, "designed");
    assert_eq!(report.ci_activation.activation_count, 0);
    assert!(report.ci_activation.commands.is_empty());
    let found = codes(&report);
    for code in [
        "endpoint.notrun",
        "evidence.sha_drift",
        "claim.cli_exit_not_a_test",
        "review.request_changes_not_acceptance",
        "ontology.pack_version_not_accepted",
        "gateway.agent_multiturn_notrun",
        "trust.producer_self_trust_not_pass",
        "trust.empty_lease_not_pass",
        "position.expired_not_pass",
        "result.known_failure_not_pass",
        "intake.empty_inventory_not_proof",
        "chain.original_exit_not_upgraded",
    ] {
        assert!(found.contains(code), "missing {code} in {found:?}");
    }
}

#[test]
fn false_claims_are_rejected_and_not_rewritten_to_pass() {
    let commit = sha("77");
    let claim = format!(
        r#"{{
        "schema":"oo-source-conformance/scope-claim/v1",
        "assert_all_sources_accepted": true,
        "assert_accepted_count": 10,
        "activate_without_tests": true,
        "external_attestation_inline": {{"nightly_days":14,"roles":["dba"]}},
        "witnesses": [
            {{"kind":"unknown_source","product":"snowflake"}},
            {{"kind":"unverified_day","calendar_day":"2026-10-01"}},
            {{"kind":"missing","commit":"{commit}","path":"raw/absent.exit"}},
            {{"kind":"tamper","commit":"{commit}","path":"raw/fail.exit","expect_blob":"{blob}"}},
            {{"kind":"raw_exit","commit":"{commit}","path":"raw/fail.exit","disposition":"pass"}},
            {{"kind":"self_trust","commit":"{commit}","path":"raw/self.json","disposition":"pass"}},
            {{"kind":"expired_position","commit":"{commit}","path":"raw/fail.exit","disposition":"pass"}},
            {{"kind":"audit","commit":"{commit}","path":"raw/audit-bad.txt"}},
            {{"kind":"audit","commit":"{commit}","path":"raw/audit-ok.txt"}}
        ]
    }}"#,
        commit = commit,
        blob = sha("ff"),
    );
    let report = run_scope(&input(Some(claim)), &store());
    assert!(report.report_complete);
    assert!(!report.sources_signed_off);
    assert_eq!(report.manufactured_nightly_days, 0);
    assert_eq!(report.rejected_days, vec!["2026-10-01".to_owned()]);
    assert!(report.ci_activation.empty_run_refused);
    assert!(report.ci_activation.commands.is_empty());
    assert!(report.products.iter().all(|product| product.cc.iter().all(|row| row.status != "pass")));
    let found = codes(&report);
    for code in [
        "claim.all_sources_refused",
        "ci.empty_run_refused",
        "external.forged_attestation",
        "source.unknown",
        "nightly.unverified_day",
        "claim.missing_object",
        "evidence.hash_mismatch",
        "raw.failure_impersonates_pass",
        "trust.producer_self_trust_not_pass",
        "trust.empty_lease_not_pass",
        "position.expired_not_pass",
        "audit.parser_error",
    ] {
        assert!(found.contains(code), "missing {code}");
    }
    assert!(!found.contains("audit.zero_write_not_cc02_pass"));
}

#[test]
fn audit_parser_rejects_self_claims_and_write_masks() {
    let ok = parse_audit(b"oo-source-audit/v1\npostgres R0 SELECT 1\nmongodb R0 find {}\n");
    assert!(ok.ok && ok.zero_write);
    assert!(!ok.product_accepted);
    assert!(!ok.counts_as_cc02_pass);

    let parser = parse_audit(b"hello");
    assert!(!parser.ok);
    assert!(parser.codes.iter().any(|code| code.starts_with("audit.parser_error")));

    let trust = parse_audit(br#"{"source_bound":true,"zero_write":true}"#);
    assert!(trust.codes.iter().any(|code| code.starts_with("trust.producer_self_trust")));
    assert!(!trust.zero_write);

    let lease = parse_audit(br#"{"jobs_lease_receipt":null}"#);
    assert!(lease.codes.iter().any(|code| code.starts_with("trust.empty_lease_not_pass")));

    let masked = parse_audit(b"oo-source-audit/v1\nmysql R0 INSERT INTO t VALUES (1)\n");
    assert!(masked.codes.iter().any(|code| code.starts_with("audit.class_contradicts_statement")));

    let write = parse_audit(b"oo-source-audit/v1\nmssql R2 CREATE TABLE t(id int)\n");
    assert!(write.codes.iter().any(|code| code.starts_with("audit.write_detected")));

    let unknown = parse_audit(b"oo-source-audit/v1\nnotadb R0 SELECT 1\n");
    assert!(unknown.codes.iter().any(|code| code.starts_with("audit.unknown_engine")));

    let r1w = parse_audit(b"oo-source-audit/v1\noracle R1w UPDATE flush SET n = 1\n");
    assert!(r1w.codes.iter().any(|code| code.starts_with("audit.r1w_not_zero_write")));
}

#[test]
fn activation_refuses_a_tampered_enable_command() {
    let report = run_scope(&input(None), &store());
    let mut value = serde_json::to_value(&report).unwrap();
    let clean = validate_activation(&value).unwrap();
    assert_eq!(clean["activation_count"], 0);
    assert_eq!(clean["empty_run_is_not_a_suite"], true);
    assert_eq!(clean["sources_signed_off"], false);
    value["ci_activation"]["activation_count"] = Value::from(1);
    value["ci_activation"]["commands"] = serde_json::json!(["source-conformance check --matrix x --package y"]);
    let error = validate_activation(&value).unwrap_err();
    assert!(error.contains("ci.activation_unbacked"), "{error}");
    value["products"][0]["cc"][0]["status"] = Value::from("pass");
    value["products"][0]["cc"][0]["counts_as_probe"] = Value::from(true);
    value["ci_activation"]["activation_count"] = Value::from(0);
    value["ci_activation"]["commands"] = serde_json::json!([]);
    let error = validate_activation(&value).unwrap_err();
    assert!(error.contains("ci.activation_unbacked"), "{error}");
}
