use serde_json::{Value, json};

fn hash() -> Value {
    json!({
        "adapter": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "comparator": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "query_pg": "0000000000000000000000000000000000000000000000000000000000000000"
    })
}

fn tuple(project: &str) -> Value {
    json!({
        "as_of": "2026-10-06",
        "budget": "k1",
        "dataset": "desk",
        "harness": "sql-runner",
        "model": "none",
        "project": project,
        "role": "desk",
        "scorer_version": "oo.benchmark.sql-runner/v1",
        "snapshot": "declared-label",
        "suite": "bounds"
    })
}

fn trial(id: &str, index: u64, status: &str, reason: &str, verdict: &str, cleanup: Value) -> Value {
    let exit = if status == "pass" || reason == "mismatch" || reason == "truncated" {
        json!(0)
    } else {
        Value::Null
    };
    json!({
        "adapter_exit": exit,
        "backend_cleanup": cleanup,
        "case_id": id,
        "comparator_exit": exit,
        "comparator_verdict": verdict,
        "end_audit_ok": if exit.is_null() { Value::Null } else { json!(true) },
        "index": index,
        "partial": if exit.is_null() { Value::Null } else { json!(false) },
        "query_exit": exit,
        "reason": reason,
        "status": status,
        "truncated": if reason == "truncated" {
            json!(true)
        } else if exit.is_null() {
            Value::Null
        } else {
            json!(false)
        }
    })
}

fn pass(id: &str, index: u64) -> Value {
    trial(id, index, "pass", "match", "match", json!("unknown"))
}

fn report(trials: Vec<Value>, question_count: u64, project: &str) -> Value {
    let mut pass_count = 0u64;
    let mut fail_count = 0u64;
    let mut timeout_count = 0u64;
    let mut not_run_count = 0u64;
    let mut reannotation_count = 0u64;
    for item in &trials {
        match item["status"].as_str() {
            Some("pass") => pass_count += 1,
            Some("fail") => fail_count += 1,
            Some("timeout") => timeout_count += 1,
            Some("not_run") => not_run_count += 1,
            Some("needs_reannotation") => reannotation_count += 1,
            _ => {}
        }
    }
    let trial_count = u64::try_from(trials.len()).expect("len");
    json!({
        "executable_sha256": hash(),
        "fail_count": fail_count,
        "lane": "trusted_sql",
        "metadata": "declared",
        "mode": "synthetic",
        "not_run_count": not_run_count,
        "pass_count": pass_count,
        "passed": trial_count > 0 && pass_count == trial_count,
        "question_count": question_count,
        "reannotation_count": reannotation_count,
        "timeout_count": timeout_count,
        "trial_count": trial_count,
        "trials": trials,
        "tuple": tuple(project),
        "version": "oo.benchmark.sql-runner-report/v1"
    })
}

fn document(left: Value, right: Value) -> String {
    serde_json::to_string(&json!({
        "algorithm": "oo.benchmark.paired-question-bootstrap/v1",
        "labels": {
            "budget": "k1",
            "scorer_version": "oo.benchmark.sql-runner/v1",
            "variants": [{"id": "left"}, {"id": "right"}]
        },
        "reports": [left, right],
        "resamples": 4,
        "seed": "0",
        "version": "oo.benchmark.trial-report-adapter/v1"
    }))
    .expect("json")
}

fn adapt_err(input: &str) -> String {
    oo_trial_report_adapter::adapt(input)
        .expect_err("config")
        .stderr_line()
}

#[test]
fn truncated_refusal_and_reannotation_keep_their_rows() {
    let left = report(
        vec![
            trial("desk", 0, "fail", "truncated", "match", json!("unknown")),
            trial(
                "desk",
                1,
                "needs_reannotation",
                "needs_reannotation",
                "match",
                json!("unknown"),
            ),
        ],
        1,
        "dispatch",
    );
    let right = report(vec![pass("desk", 0), pass("desk", 1)], 1, "dispatch");
    let output = oo_trial_report_adapter::adapt(&document(left, right)).expect("adapt");
    assert!(output.contains("\"status\":\"fail\""));
    assert!(output.contains("\"status\":\"needs_reannotation\""));
    assert!(output.contains("\"model\":\"trusted_sql\""));
    assert!(!output.contains("\"model\":\"none\""));
}

#[test]
fn a_short_vector_is_not_padded() {
    let left = report(vec![pass("desk", 0), pass("desk", 1)], 1, "dispatch");
    let right = report(vec![pass("desk", 0)], 1, "dispatch");
    assert_eq!(
        adapt_err(&document(left, right)),
        "trial-report-adapter: trials /reports/1/trials\n"
    );
}

#[test]
fn an_extra_case_is_not_dropped() {
    let left = report(vec![pass("desk", 0)], 1, "dispatch");
    let right = report(vec![pass("desk", 0), pass("gate", 0)], 2, "dispatch");
    assert_eq!(
        adapt_err(&document(left, right)),
        "trial-report-adapter: schema /reports/1/trials\n"
    );
}

#[test]
fn a_hole_in_the_index_is_trials() {
    let broken = report(vec![pass("desk", 0), pass("desk", 2)], 1, "dispatch");
    let right = report(vec![pass("desk", 0), pass("desk", 1)], 1, "dispatch");
    let err = adapt_err(&document(broken, right));
    assert!(err.starts_with("trial-report-adapter: trials "), "{err}");
}

#[test]
fn model_none_is_required_and_a_model_name_is_refused() {
    let mut left = report(vec![pass("desk", 0)], 1, "dispatch");
    left["tuple"]["model"] = json!("trusted_sql");
    let right = report(vec![pass("desk", 0)], 1, "dispatch");
    assert_eq!(
        adapt_err(&document(left, right)),
        "trial-report-adapter: tuple /reports/0/tuple/model\n"
    );
}

#[test]
fn lane_must_be_trusted_sql() {
    let mut left = report(vec![pass("desk", 0)], 1, "dispatch");
    left["lane"] = json!("model");
    let right = report(vec![pass("desk", 0)], 1, "dispatch");
    assert_eq!(
        adapt_err(&document(left, right)),
        "trial-report-adapter: lane /reports/0/lane\n"
    );
}

#[test]
fn label_budget_must_match_the_reports() {
    let left = report(vec![pass("desk", 0)], 1, "dispatch");
    let right = report(vec![pass("desk", 0)], 1, "dispatch");
    let mut value: Value = serde_json::from_str(&document(left, right)).expect("parse");
    value["labels"]["budget"] = json!("other");
    let text = serde_json::to_string(&value).expect("json");
    assert_eq!(
        adapt_err(&text),
        "trial-report-adapter: tuple /labels/budget\n"
    );
}

#[test]
fn swapped_case_order_is_not_realigned() {
    let left = report(vec![pass("desk", 0), pass("gate", 0)], 2, "dispatch");
    let right = report(vec![pass("gate", 0), pass("desk", 0)], 2, "dispatch");
    assert_eq!(
        adapt_err(&document(left, right)),
        "trial-report-adapter: schema /reports/1/trials/0/case_id\n"
    );
}

#[test]
fn a_lying_pass_count_is_schema() {
    let mut left = report(vec![pass("desk", 0)], 1, "dispatch");
    left["pass_count"] = json!(0);
    left["passed"] = json!(false);
    let right = report(vec![pass("desk", 0)], 1, "dispatch");
    assert_eq!(
        adapt_err(&document(left, right)),
        "trial-report-adapter: schema /reports/0/pass_count\n"
    );
}

#[test]
fn resamples_and_seed_use_the_statistics_spelling() {
    let left = report(vec![pass("desk", 0)], 1, "dispatch");
    let right = report(vec![pass("desk", 0)], 1, "dispatch");
    let mut value: Value = serde_json::from_str(&document(left, right)).expect("parse");
    value["resamples"] = json!(0);
    assert_eq!(
        adapt_err(&serde_json::to_string(&value).expect("json")),
        "trial-report-adapter: budget /resamples\n"
    );
    value["resamples"] = json!(4);
    value["seed"] = json!("01");
    assert_eq!(
        adapt_err(&serde_json::to_string(&value).expect("json")),
        "trial-report-adapter: schema /seed\n"
    );
}
