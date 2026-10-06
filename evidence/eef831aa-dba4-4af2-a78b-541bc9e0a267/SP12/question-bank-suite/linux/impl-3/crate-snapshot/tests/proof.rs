mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use support::{
    AS_OF, CELL_NOTE, CELL_TABLE, CELL_TOPK, ROLE, SQL_ABSENT, SQL_NOTE, SQL_SCALAR, SQL_TABLE,
    SQL_TOPK, World, assert_clean, document, hand_bank,
};

fn env_path(name: &str) -> PathBuf {
    let value = std::env::var(name).unwrap_or_else(|_| {
        panic!("{name} is unset; proof binaries must be prebuilt, and this test does not run cargo")
    });
    let path = PathBuf::from(&value);
    assert!(path.is_file(), "{name} is not a file: {value}");
    path
}

fn text_cell(value: &str) -> Value {
    json!({"kind": "text", "value": value})
}

fn int_cell(value: &str) -> Value {
    json!({"kind": "int", "value": value})
}

fn row(cells: &[Value]) -> String {
    serde_json::to_string(&json!({"cells": cells, "record": "row", "turn": 0})).unwrap()
}

fn push_entry(body: &mut String, sql: &str, rows: &[String]) {
    body.push_str(sql);
    body.push('\n');
    body.push_str(&rows.len().to_string());
    body.push('\n');
    for line in rows {
        assert!(!line.contains('\n'));
        body.push_str(line);
        body.push('\n');
    }
}

fn write_responses(query_pg: &Path) {
    let mut body = String::from("OOQRESP1\n5\n");
    push_entry(
        &mut body,
        SQL_TABLE,
        &[row(&[text_cell(CELL_TABLE), text_cell("1.20")])],
    );
    push_entry(&mut body, SQL_SCALAR, &[row(&[int_cell("7")])]);
    push_entry(
        &mut body,
        SQL_TOPK,
        &[row(&[text_cell(CELL_TOPK), int_cell("2")])],
    );
    push_entry(&mut body, SQL_ABSENT, &[]);
    push_entry(&mut body, SQL_NOTE, &[row(&[text_cell(CELL_NOTE)])]);
    let path = query_pg
        .parent()
        .expect("query-pg parent")
        .join("responses.json");
    fs::write(&path, body).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

#[test]
fn sql_runner_consumes_the_generated_suite() {
    let query_pg = env_path("OO_A267_QUERY_PG");
    let adapter = env_path("OO_A267_ADAPTER");
    let comparator = env_path("OO_A267_COMPARATOR");
    let runner = env_path("OO_A267_SQL_RUNNER");
    let world = World::with_executables("proof", &query_pg, &adapter, &comparator);
    let doc = document(&hand_bank(), &world, ROLE, AS_OF, 2);
    let out = world.out("suite");
    let compiled = oo_question_bank_suite::compile(&doc, &out).expect("compile");
    assert_clean(compiled.report.as_bytes());
    let compiler_report: Value = serde_json::from_str(&compiled.report).unwrap();
    assert_eq!(compiler_report["question_count"], 5);
    assert_eq!(compiler_report["trial_count"], 10);
    assert_eq!(compiler_report["planned"].as_array().unwrap().len(), 5);
    write_responses(&query_pg);

    let run_dir = world.out("run");
    let suite = out.join("suite.json");
    let output = Command::new(&runner)
        .args([
            "--suite",
            suite.to_str().expect("suite path"),
            "--run-dir",
            run_dir.to_str().expect("run path"),
        ])
        .output()
        .expect("spawn sql-runner");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout {stdout}\nstderr {stderr}"
    );
    assert!(output.stderr.is_empty(), "{stderr}");
    let report_path = run_dir.join("report.json");
    let report_bytes =
        fs::read(&report_path).unwrap_or_else(|error| panic!("report: {error}\n{stdout}"));
    assert_eq!(output.stdout, report_bytes);
    assert_clean(&report_bytes);
    let report: Value = serde_json::from_slice(&report_bytes).unwrap();
    assert_eq!(report["version"], "oo.benchmark.sql-runner-report/v1");
    assert_eq!(report["lane"], "trusted_sql");
    assert_eq!(report["trial_count"], 10);
    assert_eq!(report["question_count"], 5);
    assert_eq!(report["pass_count"], 6);
    assert_eq!(report["fail_count"], 2);
    assert_eq!(report["reannotation_count"], 2);
    assert_eq!(report["not_run_count"], 0);
    assert_eq!(report["timeout_count"], 0);
    assert_eq!(report["passed"], false);
    assert_eq!(report["tuple"]["model"], "none");
    assert_eq!(report["tuple"]["harness"], "sql-runner");

    let trials = report["trials"].as_array().expect("trials");
    assert_eq!(trials.len(), 10);
    let mut seen = [0_u32; 5];
    for trial in trials {
        let id = trial["case_id"].as_str().unwrap();
        let status = trial["status"].as_str().unwrap();
        let reason = trial["reason"].as_str().unwrap();
        let verdict = trial["comparator_verdict"].as_str().unwrap();
        match id {
            "pub-table" | "pub-scalar" | "pub-topk" => {
                assert_eq!(status, "pass", "{trial}");
                assert_eq!(reason, "match", "{trial}");
                assert_eq!(verdict, "match", "{trial}");
            }
            "pub-absent" => {
                assert_eq!(status, "fail", "{trial}");
                assert_eq!(reason, "mismatch", "{trial}");
                assert_eq!(verdict, "mismatch", "{trial}");
            }
            "pub-note" => {
                assert_eq!(status, "needs_reannotation", "{trial}");
                assert_eq!(reason, "needs_reannotation", "{trial}");
                assert_eq!(verdict, "match", "{trial}");
            }
            other => panic!("unexpected case {other}: {trial}"),
        }
        let slot = match id {
            "pub-table" => 0,
            "pub-scalar" => 1,
            "pub-topk" => 2,
            "pub-absent" => 3,
            "pub-note" => 4,
            _ => unreachable!(),
        };
        seen[slot] += 1;
    }
    assert_eq!(seen, [2, 2, 2, 2, 2]);

    let table_adapter =
        fs::read_to_string(run_dir.join("case-pub-table/trial-0/adapter.stdout")).unwrap();
    assert!(table_adapter.contains("\"coefficient\":\"120\""));
    assert!(!table_adapter.contains("1.20"));
    assert!(table_adapter.contains(CELL_TABLE));
    let absent_adapter =
        fs::read_to_string(run_dir.join("case-pub-absent/trial-0/adapter.stdout")).unwrap();
    assert!(absent_adapter.contains("\"kind\":\"rows\""));
    assert!(absent_adapter.contains("\"rows\":[]"));
    assert!(!absent_adapter.contains("not_found"));
    let absent_gold = fs::read_to_string(out.join("questions/pub-absent/gold.json")).unwrap();
    assert!(absent_gold.contains("\"kind\":\"not_found\""));
    let audit = run_dir.join("case-pub-table/trial-0/audit.jsonl");
    assert!(audit.is_file());
    assert!(!fs::read(&audit).unwrap().is_empty());
}
