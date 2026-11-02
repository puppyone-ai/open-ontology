//! Owned fixture capabilities. These are engineering gates, not phase sign-off.

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use crate::report::{Issue, Report};

pub const SUITES: &[&str] = &["integration-a", "security-b", "ontology-d", "nightly-a-m"];
static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn receipt_valid(value: &Value, suite: &str) -> bool {
    value["protocol"] == "oo.ci.capability-suite/v1"
        && value["suite"] == suite
        && value["status"] == "passed"
        && value["acceptance_closed"] == false
        && value["cleanup_complete"] == true
        && value["tests_executed"].as_u64().is_some_and(|n| n > 0)
        && value["commands"].as_array().is_some_and(|commands| {
            !commands.is_empty()
                && commands.iter().all(|c| {
                    c["exit_code"] == 0
                        && c["argv"].as_array().is_some_and(|args| {
                            !args.is_empty() && args.iter().all(Value::is_string)
                        })
                        && c.get("timeout") != Some(&Value::Bool(true))
                })
        })
}

pub fn run(suite: &str) -> Report {
    let repo = crate::util::repo_root();
    let root = std::env::temp_dir().join(format!(
        "oo-capability-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let output = root.join("receipt.json");
    let script = if matches!(suite, "integration-a" | "nightly-a-m") {
        "ci/run_pipeline_suite.py"
    } else {
        "ci/run_gateway_builder_suite.py"
    };
    if fs::create_dir(&root).is_err() {
        return Report::fail(
            "test",
            "failed",
            1,
            vec![Issue::new(
                "suite.output",
                suite,
                "cannot create a fresh owned output directory",
            )],
            json!({}),
        );
    }
    let args = vec![
        "python3".to_owned(),
        script.to_owned(),
        "--suite".to_owned(),
        suite.to_owned(),
        "--output".to_owned(),
        output.display().to_string(),
    ];
    let source = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&repo)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .unwrap_or_default();
    let run = crate::runner_capture::execute_command(
        &repo,
        &args,
        Duration::from_secs(1200),
        suite,
        source.trim(),
    );
    let receipt = fs::metadata(&output)
        .ok()
        .filter(|meta| meta.len() <= 1_048_576)
        .and_then(|_| fs::read(&output).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or(Value::Null);
    let valid = run.exit_code() == 0
        && !run.timed_out()
        && !run.output_limited()
        && run.spawn_error().is_none()
        && !crate::containment::blocks_pass(
            run.containment(),
            run.spawned(),
            run.cleanup_incomplete(),
            run.containment_note(),
        )
        && receipt_valid(&receipt, suite);
    let details = json!({
        "suite": suite, "receipt": receipt, "receipt_path": output,
        "child_exit": run.exit_code(), "timed_out": run.timed_out(),
        "containment": run.containment(), "containment_note": run.containment_note(),
        "stdout_sha256": crate::util::sha256_hex(run.stdout()),
        "stderr_sha256": crate::util::sha256_hex(run.stderr()),
        "acceptance_closed": [], "release_authority": false,
    });
    if valid {
        Report::ok("test", "pass", details)
    } else {
        Report::fail(
            "test",
            "failed",
            1,
            vec![Issue::new(
                "suite.execution",
                suite,
                "owned suite must execute nonzero cases, finish cleanly and produce a matching successful receipt",
            )],
            details,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_empty_failed_wrong_suite_or_unclean_receipt_can_pass() {
        let good = json!({"protocol":"oo.ci.capability-suite/v1","suite":"integration-a",
            "status":"passed","tests_executed":3,"cleanup_complete":true,"acceptance_closed":false,
            "commands":[{"argv":["cargo","test"],"exit_code":0}]});
        assert!(receipt_valid(&good, "integration-a"));
        assert!(!receipt_valid(&good, "security-b"));
        for (field, value) in [
            ("tests_executed", json!(0)),
            ("cleanup_complete", json!(false)),
            ("acceptance_closed", json!(true)),
            ("commands", json!([])),
            ("status", json!("failed")),
        ] {
            let mut bad = good.clone();
            bad[field] = value;
            assert!(!receipt_valid(&bad, "integration-a"), "{field}");
        }
        let mut bad = good;
        bad["commands"][0]["exit_code"] = json!(1);
        assert!(!receipt_valid(&bad, "integration-a"));
    }
}
