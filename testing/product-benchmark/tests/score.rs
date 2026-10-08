use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use product_benchmark::score::{self, FREEZE_SHA256};
use product_benchmark::sha256::sha256_hex;

fn freeze_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("FREEZE.json")
}

fn scratch(name: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
    let path = std::env::temp_dir().join(format!("oo-a389-{name}-{nanos}"));
    fs::create_dir_all(&path).expect("dir");
    path
}

fn write(dir: &Path, name: &str, body: &str) {
    fs::write(dir.join(name), body).expect(name);
}

fn summary() -> String {
    "{\"record\":\"summary\",\"turn\":0,\"status\":\"succeeded\",\"truncated\":false,\"returned_rows\":1,\"result_bytes\":1,\"rollback_confirmed\":true,\"socket_dropped\":false,\"end_audit_ok\":true,\"stop\":null,\"sqlstate\":null}\n".to_string()
}

fn query(dir: &Path, stem: &str, rows: &str) {
    write(dir, &format!("{stem}.stdout"), &format!("{rows}{}", summary()));
    write(dir, &format!("{stem}.exit"), "0\n");
}

fn passing(dir: &Path) {
    write(
        dir,
        "import-result.json",
        r#"{"protocol":"x","tables":[{"name":"party","rows":2000},{"name":"account","rows":8000},{"name":"entry","rows":90000}]}"#,
    );
    query(dir, "query-wide-count", "{\"record\":\"row\",\"turn\":0,\"cells\":[{\"kind\":\"int\",\"value\":\"90000\"}]}\n");
    query(
        dir,
        "query-wide-null",
        "{\"record\":\"row\",\"turn\":0,\"cells\":[{\"kind\":\"text\",\"value\":\"p00000\"},{\"kind\":\"null\"},{\"kind\":\"bool\",\"value\":true}]}\n",
    );
    query(
        dir,
        "query-wide-qty",
        "{\"record\":\"row\",\"turn\":0,\"cells\":[{\"kind\":\"int\",\"value\":\"0\"},{\"kind\":\"int\",\"value\":\"-9223372036854775808\"}]}\n{\"record\":\"row\",\"turn\":0,\"cells\":[{\"kind\":\"int\",\"value\":\"1\"},{\"kind\":\"int\",\"value\":\"9223372036854775807\"}]}\n{\"record\":\"row\",\"turn\":0,\"cells\":[{\"kind\":\"int\",\"value\":\"2\"},{\"kind\":\"int\",\"value\":\"0\"}]}\n",
    );
    query(
        dir,
        "query-wide-fk",
        "{\"record\":\"row\",\"turn\":0,\"cells\":[{\"kind\":\"int\",\"value\":\"0\"},{\"kind\":\"text\",\"value\":\"p00000\"}]}\n",
    );
    query(dir, "query-wide-phone", "{\"record\":\"row\",\"turn\":0,\"cells\":[{\"kind\":\"text\",\"value\":\"tel-00001\"}]}\n");
    query(
        dir,
        "query-narrow-party",
        "{\"record\":\"row\",\"turn\":0,\"cells\":[{\"kind\":\"text\",\"value\":\"p00001\"},{\"kind\":\"text\",\"value\":\"n00001\"}]}\n",
    );
    query(dir, "query-party-count", "{\"record\":\"row\",\"turn\":0,\"cells\":[{\"kind\":\"int\",\"value\":\"2000\"}]}\n");
    write(dir, "query-wide-hidden.stdout", "");
    write(dir, "query-wide-hidden.exit", "6\n");
    let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    write(
        dir,
        "builder-first.json",
        &format!(
            r#"{{"protocol":"oo.pg-builder.generation/v1","cache":"miss","calls":[{{"cache":"miss","transport":{{"model_id":"grok-4.7","http_status":200,"request_input_sha256":"{sha}","socket":"opened","attempt":1,"usage":{{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}},"declared_cost":{{"status":"unknown","amount":null,"invoice":false}}}}}}]}}"#
        ),
    );
    write(
        dir,
        "builder-hit.json",
        r#"{"protocol":"oo.pg-builder.generation/v1","cache":"hit","calls":[{"cache":"hit","transport":{"socket":"not_opened","attempt":0}}]}"#,
    );
    write(dir, "dialogue-config.json", r#"{"endpoint":{"model_id":"grok-4.7"}}"#);
    write(dir, "dialogue-proposal.json", r#"{"sql":"SELECT count(*) FROM shop.entry","authenticated":false}"#);
    write(dir, "dialogue-proposal.exit", "0\n");
    write(
        dir,
        "dialogue-execution.json",
        r#"{"query_receipt":"clean","rows":[[{"kind":"int","value":"90000"}]],"citation":{"citation_status":"ok","missing_citation":false,"authenticated_audit":false,"entailment":"not_judged"}}"#,
    );
    write(dir, "dialogue-execution.exit", "0\n");
    write(dir, "wrong-model-config.json", r#"{"endpoint":{"model_id":"grok-4.3"}}"#);
    write(dir, "wrong-model.exit", "1\n");
    write(
        dir,
        "citation-invalid.json",
        r#"{"citation_status":"invalid","missing_citation":true,"authenticated_audit":false}"#,
    );
    write(dir, "citation-invalid.exit", "1\n");
    write(
        dir,
        "apply-outcome.json",
        r#"{"status":"ready_for_trusted_query","hidden":[{"dataset":"shop.party","column":"national_id","reason":"classification.absent"}],"provision":[{"effect":"installed","slug":"wide"},{"effect":"replay","slug":"narrow"}],"non_claims":{"authentication":"not_claimed","parent_acceptance":false,"public_auth":false,"natural_language":"missing","harness_mount":"not_performed","wal":"missing","semantic_publication":"missing"}}"#,
    );
    write(
        dir,
        "whole-product.json",
        r#"{"public_auth":"notrun","session_close_api":"notrun","natural_language_harness":"notrun","runsc":"notrun","wal":"notrun","semantic_publication":"notrun","customer_uat":"notrun"}"#,
    );
}

#[test]
fn freeze_matches_pre_edit_hash() {
    let bytes = fs::read(freeze_path()).expect("freeze");
    assert_eq!(sha256_hex(&bytes), FREEZE_SHA256);
}

#[test]
fn full_synthetic_loop_passes_without_integration() {
    let dir = scratch("pass");
    passing(&dir);
    let report = score::score(&freeze_path(), &dir);
    assert!(report.freeze_unchanged);
    assert!(report.benchmark_loop_pass, "{:?}", report.failures);
    assert!(!report.integration_passed);
    assert!(!report.parent_acceptance);
    assert!(!report.http_200_alone_is_e2e_pass);
    assert_eq!(report.party, Some(2000));
    assert_eq!(report.entry, Some(90000));
    assert_eq!(report.http_status, Some(200));
    assert_eq!(report.cache_hit, "hit");
    assert_eq!(report.cache_socket, "not_opened");
    assert_eq!(report.cost_status, "unknown");
    assert!(report.cost_amount_null);
    assert!(report.usage_sum_ok);
    assert_eq!(report.session_close_api, "notrun");
    let wrong = report.questions.iter().find(|item| item.id == "q-wrong-model").expect("wrong");
    assert_eq!(wrong.verdict, "fail");
    let hidden = report.questions.iter().find(|item| item.id == "q-hidden").expect("hidden");
    assert_eq!(hidden.verdict, "fail");
    let citation = report.questions.iter().find(|item| item.id == "q-citation-false").expect("citation");
    assert_eq!(citation.verdict, "fail");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn http_200_with_wrong_cells_is_not_e2e() {
    let dir = scratch("http");
    passing(&dir);
    write(
        &dir,
        "dialogue-execution.json",
        r#"{"query_receipt":"clean","rows":[[{"kind":"int","value":"1"}]],"citation":{"citation_status":"ok","missing_citation":false,"authenticated_audit":false}}"#,
    );
    let report = score::score(&freeze_path(), &dir);
    assert!(!report.benchmark_loop_pass);
    assert_eq!(report.http_status, Some(200));
    assert!(!report.integration_passed);
    let entry = report.questions.iter().find(|item| item.id == "q-entry-count").expect("entry");
    assert_eq!(entry.verdict, "fail");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn citation_ok_does_not_satisfy_the_false_control() {
    let dir = scratch("cite");
    passing(&dir);
    write(
        &dir,
        "citation-invalid.json",
        r#"{"citation_status":"ok","missing_citation":false,"authenticated_audit":false}"#,
    );
    write(&dir, "citation-invalid.exit", "0\n");
    let report = score::score(&freeze_path(), &dir);
    assert!(!report.benchmark_loop_pass);
    assert!(report.failures.iter().any(|item| item.contains("citation false")));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn accepted_hidden_query_is_not_a_counted_rejection() {
    let dir = scratch("hide");
    passing(&dir);
    write(&dir, "query-wide-hidden.exit", "0\n");
    write(
        &dir,
        "query-wide-hidden.stdout",
        "{\"record\":\"row\",\"turn\":0,\"cells\":[{\"kind\":\"text\",\"value\":\"nid-00000\"}]}\n",
    );
    let report = score::score(&freeze_path(), &dir);
    assert!(!report.benchmark_loop_pass);
    assert!(report.failures.iter().any(|item| item.contains("query rejection")));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn missing_producer_file_does_not_pass() {
    let dir = scratch("miss");
    passing(&dir);
    fs::remove_file(dir.join("builder-hit.json")).expect("remove");
    let report = score::score(&freeze_path(), &dir);
    assert!(!report.benchmark_loop_pass);
    assert!(!report.integration_passed);
    let _ = fs::remove_dir_all(&dir);
}
