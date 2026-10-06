//! Compiled sql-runner reports, then this adapter, then compiled trial-statistics.
//!
//! The handwritten k2 fixture is a four-case byte oracle. This live vector has
//! five cases. The runner rewrites every trial of a `needs_reannotation` case,
//! so a timeout does not survive beside that flag on one live case. Disarm is
//! the last case: it replaces the `query-pg` path after that trial has started.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const SECRET: &str = "pw-a250-secret";
const SNAPSHOT: &str = "declared-a250";
const SCORER: &str = "oo.benchmark.sql-runner/v1";

struct Tools {
    query: PathBuf,
    adapter: PathBuf,
    comparator: PathBuf,
    digest: String,
}

#[test]
fn compiled_runner_reports_pair_into_statistics_input() {
    assert!(
        std::env::var_os("PGPASSWORD").is_none(),
        "PGPASSWORD must stay unset"
    );
    let proof = required_dir("OO_PROOF_DIR");
    let target = required_dir("CARGO_TARGET_DIR");
    let sql_runner = required_path("OO_SQL_RUNNER");
    let statistics = required_path("OO_TRIAL_STATISTICS");
    fs::create_dir_all(&proof).expect("proof dir");
    let tools = compile_stub(&target, &proof);
    let scratch = PathBuf::from("/tmp/a250-proof");
    fs::create_dir_all(&scratch).expect("scratch");
    write_secrets(&scratch);
    let binding = fixture("note-header.json");
    let gold_full = fixture("gold-full.json");
    let gold_miss = scratch.join("gold-miss.json");
    fs::write(
        &gold_miss,
        "{\"expected\":{\"note\":\"GOLD-MISS\"},\"row_comparator\":\"ordered\"}\n",
    )
    .expect("gold");
    assert_clean("ca", &fs::read(scratch.join("ca.crt")).expect("ca"));

    let desk = Desk {
        scratch: &scratch,
        binding: &binding,
    };
    let (left, right) = k2_reports(&sql_runner, &proof, &tools, &desk, &gold_full, &gold_miss);
    restore(&tools);
    let paired = pair(&left, &right, "k2", "live-k2");
    let adapted = adapt_ok(&proof, "k2", &paired);
    assert_k2_vectors(&adapted);
    assert_trusted_sql(&adapted, "live-k2", "k2", &tools.digest);
    let stats = run_statistics(&statistics, &proof, "k2", adapted.as_bytes(), 1);
    assert_stats(&stats, "failed", 5, 1, 2, 1, 2);

    let (failed_left, failed_right) =
        allfail_reports(&sql_runner, &proof, &tools, &desk, &gold_full, &gold_miss);
    restore(&tools);
    let failed = pair(&failed_left, &failed_right, "k4", "live-allfail");
    let failed_input = adapt_ok(&proof, "allfail", &failed);
    assert_all_fail(&failed_input);
    assert_trusted_sql(&failed_input, "live-allfail", "k4", &tools.digest);
    let failed_stats = run_statistics(&statistics, &proof, "allfail", failed_input.as_bytes(), 0);
    assert_stats(&failed_stats, "passed", 1, 0, 4, 0, 0);

    refuse_tuple(&sql_runner, &proof, &tools, &desk, &gold_full);
    restore(&tools);
    refuse_missing_trial(&sql_runner, &proof, &tools, &desk, &gold_full);
}

struct Desk<'a> {
    scratch: &'a Path,
    binding: &'a Path,
}

fn required_dir(key: &str) -> PathBuf {
    let path = required_path(key);
    fs::create_dir_all(&path).unwrap_or_else(|error| panic!("create {key}: {error}"));
    fs::canonicalize(&path).unwrap_or_else(|error| panic!("canonicalize {key}: {error}"))
}

fn required_path(key: &str) -> PathBuf {
    let text = std::env::var(key).unwrap_or_else(|_| panic!("{key} is required"));
    let path = PathBuf::from(&text);
    assert!(path.is_absolute(), "{key} must be absolute");
    path
}

fn compile_stub(target: &Path, proof: &Path) -> Tools {
    let dir = target.join("a250-bins");
    fs::create_dir_all(&dir).expect("bin dir");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/stub.rs");
    let stub = dir.join("stub");
    let output = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            "-D",
            "unsafe_code",
            "-C",
            "debuginfo=0",
            "-o",
        ])
        .arg(&stub)
        .arg(&source)
        .output()
        .expect("rustc");
    fs::write(proof.join("stub-rustc.stdout"), &output.stdout).expect("save");
    fs::write(proof.join("stub-rustc.stderr"), &output.stderr).expect("save");
    assert!(
        output.status.success(),
        "stub rustc: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let query = dir.join("query-pg");
    let adapter = dir.join("query-result-adapter");
    let comparator = dir.join("result-comparator");
    for path in [&query, &adapter, &comparator] {
        let _ = fs::remove_file(path);
        fs::hard_link(&stub, path).unwrap_or_else(|error| panic!("link: {error}"));
    }
    let bytes = fs::read(&stub).expect("stub bytes");
    Tools {
        query,
        adapter,
        comparator,
        digest: hex_digest(&bytes),
    }
}

fn restore(tools: &Tools) {
    let _ = fs::remove_file(&tools.query);
    fs::hard_link(&tools.adapter, &tools.query).expect("restore query-pg");
}

fn hex_digest(bytes: &[u8]) -> String {
    oo_core::canonical::hash_bytes(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fixture(name: &str) -> PathBuf {
    let raw = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../sql-runner/fixtures")
        .join(name);
    let path = fs::canonicalize(&raw).unwrap_or_else(|error| panic!("{name}: {error}"));
    let text = path.to_str().expect("utf-8 path");
    assert!(
        text.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b':' | b'-')
        }),
        "{text}"
    );
    path
}

fn write_secrets(scratch: &Path) {
    let password = scratch.join("login.pass");
    let ca = scratch.join("ca.crt");
    assert!(!password.display().to_string().contains(SECRET));
    fs::write(&password, format!("{SECRET}\n")).expect("password");
    fs::write(
        &ca,
        "-----BEGIN CERTIFICATE-----\nMIIB-a250\n-----END CERTIFICATE-----\n",
    )
    .expect("ca");
}

fn k2_reports(
    sql_runner: &Path,
    proof: &Path,
    tools: &Tools,
    desk: &Desk<'_>,
    gold_full: &Path,
    gold_miss: &Path,
) -> (Value, Value) {
    let left_cases = vec![
        case(desk, "desk", "A250-SPLIT", "score", 30_000, 2, gold_full),
        case(desk, "gate", "A250-GATE", "score", 400, 2, gold_full),
        case(
            desk,
            "note",
            "A250-NOTE",
            "needs_reannotation",
            30_000,
            2,
            gold_full,
        ),
        case(desk, "slow", "A250-SLOW", "score", 400, 2, gold_full),
        case(desk, "drop", "A250-PASS", "score", 30_000, 2, gold_full),
    ];
    let right_cases = vec![
        case(desk, "desk", "A250-PASS", "score", 30_000, 2, gold_full),
        case(desk, "gate", "A250-PASS", "score", 30_000, 2, gold_miss),
        case(
            desk,
            "note",
            "A250-NOTE",
            "needs_reannotation",
            30_000,
            2,
            gold_full,
        ),
        case(desk, "slow", "A250-PASS", "score", 30_000, 2, gold_full),
        case(desk, "drop", "A250-DISARM", "score", 30_000, 2, gold_full),
    ];
    let left = run_suite(
        sql_runner,
        proof,
        "k2-left",
        &suite("synthetic", "dispatch", "live-k2", "k2", left_cases, tools),
        1,
    );
    let right = run_suite(
        sql_runner,
        proof,
        "k2-right",
        &suite(
            "frozen_eval",
            "dispatch",
            "live-k2",
            "k2",
            right_cases,
            tools,
        ),
        1,
    );
    assert_eq!(
        rows(&left),
        "\
desk:0:pass:match
desk:1:fail:mismatch
gate:0:fail:refused
gate:1:timeout:timeout
note:0:needs_reannotation:needs_reannotation
note:1:needs_reannotation:needs_reannotation
slow:0:timeout:timeout
slow:1:timeout:timeout
drop:0:pass:match
drop:1:pass:match"
    );
    assert_eq!(
        rows(&right),
        "\
desk:0:pass:match
desk:1:pass:match
gate:0:fail:mismatch
gate:1:fail:mismatch
note:0:needs_reannotation:needs_reannotation
note:1:needs_reannotation:needs_reannotation
slow:0:pass:match
slow:1:pass:match
drop:0:pass:match
drop:1:not_run:not_run"
    );
    assert!(trial(&right, "drop", 1)["backend_cleanup"].is_null());
    assert_eq!(
        trial(&right, "drop", 1)["comparator_verdict"].as_str(),
        Some("absent")
    );
    assert_eq!(
        trial(&left, "gate", 1)["backend_cleanup"].as_str(),
        Some("unknown")
    );
    assert_eq!(left["question_count"].as_u64(), Some(5));
    assert_eq!(left["trial_count"].as_u64(), Some(10));
    assert_eq!(left["passed"].as_bool(), Some(false));
    assert_eq!(left["tuple"]["model"].as_str(), Some("none"));
    assert_eq!(left["tuple"]["snapshot"].as_str(), Some(SNAPSHOT));
    assert_eq!(right["mode"].as_str(), Some("frozen_eval"));
    (left, right)
}

fn allfail_reports(
    sql_runner: &Path,
    proof: &Path,
    tools: &Tools,
    desk: &Desk<'_>,
    gold_full: &Path,
    gold_miss: &Path,
) -> (Value, Value) {
    let left_cases = vec![case(desk, "desk", "A250-QUAD", "score", 400, 4, gold_full)];
    let right_cases = vec![case(
        desk,
        "desk",
        "A250-PASS",
        "score",
        30_000,
        4,
        gold_miss,
    )];
    let left = run_suite(
        sql_runner,
        proof,
        "allfail-left",
        &suite(
            "synthetic",
            "dispatch",
            "live-allfail",
            "k4",
            left_cases,
            tools,
        ),
        1,
    );
    restore(tools);
    let right = run_suite(
        sql_runner,
        proof,
        "allfail-right",
        &suite(
            "synthetic",
            "dispatch",
            "live-allfail",
            "k4",
            right_cases,
            tools,
        ),
        1,
    );
    assert_eq!(
        rows(&left),
        "\
desk:0:timeout:timeout
desk:1:fail:refused
desk:2:fail:mismatch
desk:3:not_run:not_run"
    );
    assert_eq!(
        rows(&right),
        "\
desk:0:fail:mismatch
desk:1:fail:mismatch
desk:2:fail:mismatch
desk:3:fail:mismatch"
    );
    (left, right)
}

fn refuse_tuple(sql_runner: &Path, proof: &Path, tools: &Tools, desk: &Desk<'_>, gold_full: &Path) {
    let one = vec![case(
        desk,
        "desk",
        "A250-PASS",
        "score",
        30_000,
        1,
        gold_full,
    )];
    let left = run_suite(
        sql_runner,
        proof,
        "tuple-left",
        &suite(
            "synthetic",
            "dispatch",
            "live-tuple",
            "k1",
            one.clone(),
            tools,
        ),
        0,
    );
    let right = run_suite(
        sql_runner,
        proof,
        "tuple-right",
        &suite("synthetic", "other", "live-tuple", "k1", one, tools),
        0,
    );
    let (code, stdout, stderr) = adapt(proof, "tuple", &pair(&left, &right, "k1", "live-tuple"));
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        b"trial-report-adapter: tuple /reports/1/tuple/project\n"
    );
}

fn refuse_missing_trial(
    sql_runner: &Path,
    proof: &Path,
    tools: &Tools,
    desk: &Desk<'_>,
    gold_full: &Path,
) {
    let cases = vec![case(
        desk,
        "desk",
        "A250-PASS",
        "score",
        30_000,
        2,
        gold_full,
    )];
    let report = run_suite(
        sql_runner,
        proof,
        "missing-base",
        &suite("synthetic", "dispatch", "live-missing", "k2", cases, tools),
        0,
    );
    assert_eq!(rows(&report), "desk:0:pass:match\ndesk:1:pass:match");
    let short = drop_second_trial(&report);
    let (code, stdout, stderr) = adapt(
        proof,
        "missing",
        &pair(&report, &short, "k2", "live-missing"),
    );
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"trial-report-adapter: trials /reports/1/trials\n");
}

fn drop_second_trial(report: &Value) -> Value {
    let mut short = report.clone();
    let trials = short["trials"].as_array().expect("trials");
    assert_eq!(trials.len(), 2);
    short["trials"] = json!([trials[0].clone()]);
    short["trial_count"] = json!(1);
    short["pass_count"] = json!(1);
    short["fail_count"] = json!(0);
    short["timeout_count"] = json!(0);
    short["not_run_count"] = json!(0);
    short["reannotation_count"] = json!(0);
    short["question_count"] = json!(1);
    short["passed"] = json!(true);
    short
}

fn case(
    desk: &Desk<'_>,
    id: &str,
    marker: &str,
    annotation: &str,
    wall: u64,
    k: u64,
    gold: &Path,
) -> Value {
    let request = desk.scratch.join(format!("request-{id}-{marker}.json"));
    write_request(&request, marker, desk.scratch, gold);
    json!({
        "id": id,
        "role": "desk",
        "k": k,
        "annotation": annotation,
        "trial_wall_ms": wall,
        "request_file": request.display().to_string(),
        "binding_file": desk.binding.display().to_string(),
        "gold_file": gold.display().to_string()
    })
}

fn write_request(path: &Path, marker: &str, scratch: &Path, gold: &Path) {
    let value = json!({
        "version": "oo.gateway.query-pg/v1",
        "endpoint": {"host": "db.example", "port": 5432, "database": "oo_a250", "user": "oo_a250_login"},
        "password_file": scratch.join("login.pass").display().to_string(),
        "ca_file": scratch.join("ca.crt").display().to_string(),
        "audit_file": "/tmp/a250-proof/placeholder-audit.jsonl",
        "identity": {
            "project": "dispatch",
            "role": "oo_r_desk",
            "policy_id": "abababababababababababababababababababababababababababababababab"
        },
        "policies": [{
            "project": "dispatch",
            "config_digest": "abababababababababababababababababababababababababababababababab",
            "generation": 1,
            "role": "desk",
            "login": "oo_a250_login",
            "table": {"namespace": "dispatch", "name": "ticket", "columns": []}
        }],
        "statement": {
            "source": "request",
            "sql": format!("SELECT note FROM dispatch.ticket /* {marker} */"),
            "max_rows": 20,
            "max_result_bytes": 4096,
            "wall_clock_ms": 30000
        }
    });
    let bytes = serde_json::to_vec(&value).expect("request");
    assert_clean("request", &bytes);
    let gold_text = gold.display().to_string();
    assert!(
        !bytes
            .windows(gold_text.len())
            .any(|window| window == gold_text.as_bytes()),
        "request contains the gold path"
    );
    fs::write(path, bytes).expect("write request");
}

fn suite(
    mode: &str,
    project: &str,
    suite_name: &str,
    budget: &str,
    cases: Vec<Value>,
    tools: &Tools,
) -> Value {
    let k = cases[0]["k"].as_u64().expect("k");
    for case in &cases {
        assert_eq!(case["k"].as_u64(), Some(k));
    }
    json!({
        "version": "oo.benchmark.sql-runner/v1",
        "mode": mode,
        "metadata": "declared",
        "tuple": {
            "project": project,
            "snapshot": SNAPSHOT,
            "as_of": "2026-10-06",
            "dataset": "desk",
            "suite": suite_name,
            "model": "none",
            "harness": "sql-runner",
            "role": "desk",
            "budget": budget,
            "scorer_version": SCORER
        },
        "executables": {
            "query_pg": {"path": tools.query.display().to_string(), "sha256": tools.digest},
            "adapter": {"path": tools.adapter.display().to_string(), "sha256": tools.digest},
            "comparator": {"path": tools.comparator.display().to_string(), "sha256": tools.digest}
        },
        "cases": cases
    })
}

fn run_suite(sql_runner: &Path, proof: &Path, name: &str, suite: &Value, expect: i32) -> Value {
    let root = PathBuf::from("/tmp/a250-proof").join(name);
    fs::create_dir_all(&root).expect("suite dir");
    let suite_path = root.join("suite.json");
    let body = serde_json::to_vec(suite).expect("suite");
    assert_clean(name, &body);
    fs::write(&suite_path, &body).expect("write suite");
    let run = root.join("run");
    assert!(!run.exists(), "{name} run dir already exists");
    let output = Command::new(sql_runner)
        .args([
            "--suite",
            suite_path.to_str().expect("suite path"),
            "--run-dir",
            run.to_str().expect("run path"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("sql-runner");
    let code = output.status.code().unwrap_or(127);
    assert_clean(&format!("{name} stdout"), &output.stdout);
    assert_clean(&format!("{name} stderr"), &output.stderr);
    fs::write(proof.join(format!("{name}-runner.stdout")), &output.stdout).expect("save");
    fs::write(proof.join(format!("{name}-runner.stderr")), &output.stderr).expect("save");
    fs::write(
        proof.join(format!("{name}-runner.exit")),
        format!("{code}\n"),
    )
    .expect("save");
    assert_eq!(code, expect, "{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stderr.is_empty());
    assert!(output.stdout.ends_with(b"\n"));
    let report: Value = serde_json::from_slice(&output.stdout).expect("report");
    let stored = fs::read(run.join("report.json")).expect("report.json");
    assert_eq!(stored, output.stdout);
    report
}

fn pair(left: &Value, right: &Value, budget: &str, suite_name: &str) -> Vec<u8> {
    assert_eq!(left["tuple"]["suite"], suite_name);
    assert_eq!(right["tuple"]["suite"], suite_name);
    serde_json::to_vec(&json!({
        "algorithm": "oo.benchmark.paired-question-bootstrap/v1",
        "labels": {
            "budget": budget,
            "scorer_version": SCORER,
            "variants": [{"id": "left"}, {"id": "right"}]
        },
        "reports": [left, right],
        "resamples": 8,
        "seed": "11",
        "version": "oo.benchmark.trial-report-adapter/v1"
    }))
    .expect("pair")
}

fn adapt_ok(proof: &Path, name: &str, input: &[u8]) -> String {
    let (code, stdout, stderr) = adapt(proof, name, input);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&stderr));
    assert!(stderr.is_empty());
    String::from_utf8(stdout).expect("utf-8")
}

fn adapt(proof: &Path, name: &str, input: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
    assert_clean(name, input);
    let output = Command::new(env!("CARGO_BIN_EXE_trial-report-adapter"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("adapter");
    let (code, stdout, stderr) = wait_with_input(output, input);
    fs::write(proof.join(format!("{name}-adapter.stdin")), input).expect("save");
    fs::write(proof.join(format!("{name}-adapter.stdout")), &stdout).expect("save");
    fs::write(proof.join(format!("{name}-adapter.stderr")), &stderr).expect("save");
    fs::write(
        proof.join(format!("{name}-adapter.exit")),
        format!("{code}\n"),
    )
    .expect("save");
    assert_clean(&format!("{name} adapter"), &stdout);
    (code, stdout, stderr)
}

fn run_statistics(bin: &Path, proof: &Path, name: &str, input: &[u8], expect: i32) -> Value {
    let output = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("trial-statistics");
    let (code, stdout, stderr) = wait_with_input(output, input);
    fs::write(proof.join(format!("{name}-statistics.stdout")), &stdout).expect("save");
    fs::write(proof.join(format!("{name}-statistics.stderr")), &stderr).expect("save");
    fs::write(
        proof.join(format!("{name}-statistics.exit")),
        format!("{code}\n"),
    )
    .expect("save");
    assert_clean(&format!("{name} statistics"), &stdout);
    assert_eq!(code, expect, "{}", String::from_utf8_lossy(&stderr));
    assert!(stderr.is_empty());
    assert!(stdout.ends_with(b"\n"));
    serde_json::from_slice(&stdout).expect("statistics report")
}

fn wait_with_input(mut child: std::process::Child, input: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(input).expect("write stdin");
    }
    let output = child.wait_with_output().expect("wait");
    (
        output.status.code().unwrap_or(127),
        output.stdout,
        output.stderr,
    )
}

fn rows(report: &Value) -> String {
    report["trials"]
        .as_array()
        .expect("trials")
        .iter()
        .map(|trial| {
            format!(
                "{}:{}:{}:{}",
                trial["case_id"].as_str().expect("id"),
                trial["index"].as_u64().expect("index"),
                trial["status"].as_str().expect("status"),
                trial["reason"].as_str().expect("reason")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn trial<'a>(report: &'a Value, id: &str, index: u64) -> &'a Value {
    report["trials"]
        .as_array()
        .expect("trials")
        .iter()
        .find(|item| item["case_id"] == id && item["index"].as_u64() == Some(index))
        .unwrap_or_else(|| panic!("missing {id} {index}"))
}

fn assert_k2_vectors(document: &str) {
    let value: Value = serde_json::from_str(document.trim_end()).expect("json");
    assert_eq!(
        shape(&value, "left"),
        "desk:pass,fail;gate:fail,fail;note:needs_reannotation,needs_reannotation;slow:fail,fail;drop:pass,pass"
    );
    assert_eq!(
        shape(&value, "right"),
        "desk:pass,pass;gate:fail,fail;note:needs_reannotation,needs_reannotation;slow:pass,pass;drop:pass,fail"
    );
}

fn assert_all_fail(document: &str) {
    let value: Value = serde_json::from_str(document.trim_end()).expect("json");
    assert_eq!(shape(&value, "left"), "desk:fail,fail,fail,fail");
    assert_eq!(shape(&value, "right"), "desk:fail,fail,fail,fail");
    assert!(!document.contains("\"pass\""));
    assert!(!document.contains("needs_reannotation"));
}

fn shape(document: &Value, id: &str) -> String {
    vectors(document, id)
        .iter()
        .map(|(case_id, trials)| format!("{case_id}:{}", trials.join(",")))
        .collect::<Vec<_>>()
        .join(";")
}

fn vectors(document: &Value, id: &str) -> Vec<(String, Vec<String>)> {
    let variant = document["variants"]
        .as_array()
        .expect("variants")
        .iter()
        .find(|item| item["id"] == id)
        .unwrap_or_else(|| panic!("missing {id}"));
    variant["questions"]
        .as_array()
        .expect("questions")
        .iter()
        .map(|question| {
            let trials = question["trials"]
                .as_array()
                .expect("trials")
                .iter()
                .map(|trial| trial["status"].as_str().expect("status").to_owned())
                .collect();
            (question["id"].as_str().expect("id").to_owned(), trials)
        })
        .collect()
}

fn assert_trusted_sql(document: &str, suite_name: &str, budget: &str, digest: &str) {
    assert!(document.contains("\"model\":\"trusted_sql\""));
    assert!(!document.contains("\"model\":\"none\""));
    assert!(!document.contains("certified"));
    assert!(!document.contains("pass_k"));
    assert!(!document.contains("percentile"));
    assert!(!document.contains(digest));
    assert!(document.contains(&format!("\"snapshot\":\"{SNAPSHOT}\"")));
    assert!(document.contains(&format!("\"suite\":\"{suite_name}\"")));
    assert!(document.contains(&format!("\"budget\":\"{budget}\"")));
    assert!(document.contains("\"version\":\"oo.benchmark.trial-statistics/v1\""));
    assert!(!document.contains("oo.benchmark.trial-statistics-report/v1"));
}

fn assert_stats(
    report: &Value,
    gate: &str,
    questions: u64,
    reannotation: u64,
    k: u64,
    left_pass: u64,
    right_pass: u64,
) {
    assert_eq!(
        report["version"].as_str(),
        Some("oo.benchmark.trial-statistics-report/v1")
    );
    assert_eq!(report["gate"].as_str(), Some(gate));
    assert_eq!(report["question_count"].as_u64(), Some(questions));
    assert_eq!(report["reannotation_count"].as_u64(), Some(reannotation));
    assert_eq!(report["trial_count"].as_u64(), Some(k));
    assert_eq!(report["seed"].as_str(), Some("11"));
    assert_eq!(report["resamples"].as_u64(), Some(8));
    assert_eq!(report["variants"][0]["id"].as_str(), Some("left"));
    assert_eq!(report["variants"][1]["id"].as_str(), Some("right"));
    assert_eq!(
        report["variants"][0]["pass_k_count"].as_u64(),
        Some(left_pass)
    );
    assert_eq!(
        report["variants"][1]["pass_k_count"].as_u64(),
        Some(right_pass)
    );
    assert!(report.get("percentile_low").is_some());
    assert!(report.get("percentile_high").is_some());
}

fn assert_clean(label: &str, bytes: &[u8]) {
    if bytes
        .windows(SECRET.len())
        .any(|window| window == SECRET.as_bytes())
    {
        panic!("{label} contains the fixture password");
    }
    if bytes.windows(11).any(|window| window == b"PRIVATE KEY") {
        panic!("{label} contains a private key marker");
    }
}
