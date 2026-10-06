mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use serde_json::{Value, json};

use oo_question_bank_suite::{compile, sha256_hex};

use support::{
    AS_OF, AS_OF_OTHER, CELL_TABLE, DATASET, PASSWORD, ROLE, SQL_ABSENT, SQL_NOTE, SQL_SCALAR,
    SQL_TABLE, SQL_TOPK, SUITE, World, assert_clean, bank, document, document_with_hash, file_hash,
    hand_bank, missing_gold_question, read_json, refuse_question, relative_question, stub_document,
    tiny_rows,
};

#[test]
fn hand_bank_keeps_every_selected_id_and_hides_gold() {
    let world = World::new("hand");
    let bank = hand_bank();
    let admitted = oo_question_bank::evaluate(&serde_json::to_string(&bank).unwrap()).unwrap();
    assert!(!admitted.gate_passed);
    let doc = document(&bank, &world, ROLE, AS_OF, 2);
    let out = world.out("suite");
    let compiled = compile(&doc, &out).unwrap();
    let report: Value = serde_json::from_str(&compiled.report).unwrap();
    assert_eq!(report["question_count"], 5);
    assert_eq!(report["trial_count"], 10);
    assert_eq!(report["k"], 2);
    assert_eq!(report["model"], "none");
    assert_eq!(report["quality_claim"], "none");
    assert_eq!(report["authentication"], "not_claimed");
    assert_eq!(report["operator_document"], "declared");
    assert_eq!(report["selection"]["role"], ROLE);
    assert_eq!(report["selection"]["as_of"], AS_OF);
    assert_eq!(report["dataset_id"], DATASET);
    assert_eq!(report["suite_id"], SUITE);
    assert_eq!(report["distribution"], "public_synthetic");
    assert_eq!(report["excluded_question_ids"], json!(["pub-west"]));
    let planned = report["planned"].as_array().unwrap();
    let ids = [
        "pub-table",
        "pub-scalar",
        "pub-topk",
        "pub-absent",
        "pub-note",
    ];
    let annotations = ["score", "score", "score", "score", "needs_reannotation"];
    assert_eq!(planned.len(), 5);
    for (index, question) in planned.iter().enumerate() {
        assert_eq!(question["id"], ids[index]);
        assert_eq!(question["annotation"], annotations[index]);
        assert_eq!(
            question["index"].as_u64(),
            Some(u64::try_from(index).unwrap())
        );
    }
    assert!(report.get("files").is_none());
    assert!(report.get("gold_sha256").is_none());
    assert_clean(compiled.report.as_bytes());

    let suite_bytes = fs::read(out.join("suite.json")).unwrap();
    let manifest_bytes = fs::read(out.join("manifest.json")).unwrap();
    let agent_bytes = fs::read(out.join("agent-view.json")).unwrap();
    assert_eq!(sha256_hex(&suite_bytes), report["suite_sha256"]);
    assert_eq!(sha256_hex(&agent_bytes), report["agent_view_sha256"]);
    assert_eq!(sha256_hex(&manifest_bytes), report["manifest_sha256"]);
    assert_clean(&suite_bytes);
    assert_clean(&manifest_bytes);
    assert_clean(&agent_bytes);
    assert!(
        std::str::from_utf8(&manifest_bytes)
            .unwrap()
            .contains("gold_sha256")
    );

    let suite = read_json(&out.join("suite.json"));
    assert_eq!(suite["version"], "oo.benchmark.sql-runner/v1");
    assert_eq!(suite["metadata"], "declared");
    assert_eq!(suite["tuple"]["model"], "none");
    assert_eq!(suite["tuple"]["harness"], "sql-runner");
    assert_eq!(suite["tuple"]["role"], ROLE);
    let cases = suite["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 5);
    for (index, case) in cases.iter().enumerate() {
        assert_eq!(case["id"], ids[index]);
        assert_eq!(case["annotation"], annotations[index]);
        assert_eq!(case["k"], 2);
        assert_eq!(case["role"], ROLE);
        assert_eq!(case["trial_wall_ms"], 30_000);
    }

    let agent = read_json(&out.join("agent-view.json"));
    assert_eq!(
        agent["version"],
        "oo.benchmark.question-bank-suite-agent-view/v1"
    );
    assert_eq!(agent["questions"].as_array().unwrap().len(), 5);
    assert!(!agent.to_string().contains("pub-west"));

    assert_question(&out, "pub-table", SQL_TABLE, "unordered", true);
    assert_question(&out, "pub-scalar", SQL_SCALAR, "ordered", false);
    assert_question(&out, "pub-topk", SQL_TOPK, "ordered", false);
    assert_question(&out, "pub-absent", SQL_ABSENT, "ordered", false);
    assert_question(&out, "pub-note", SQL_NOTE, "ordered", false);
    assert!(!out.join("questions/pub-west").exists());

    let gold = fs::read(out.join("questions/pub-table/gold.json")).unwrap();
    let gold_text = std::str::from_utf8(&gold).unwrap();
    assert!(gold_text.contains("\"coefficient\":\"120\""));
    assert!(gold_text.contains("\"scale\":2"));
    assert!(gold_text.contains(CELL_TABLE));
    assert!(!gold_text.contains("1.20"));
    assert!(!gold_text.contains("0120"));
    let request = fs::read_to_string(out.join("questions/pub-table/request.json")).unwrap();
    assert!(request.contains(SQL_TABLE));
    assert!(!request.contains(CELL_TABLE));
    assert!(request.contains("\"source\":\"request\""));
    assert!(request.contains("\"role\":\"oo_r_finance_east\""));
    assert!(!request.contains("select 1"));
    let binding = fs::read(out.join("questions/pub-table/binding.json")).unwrap();
    assert_clean(&binding);
    let binding_json = read_json(&out.join("questions/pub-table/binding.json"));
    assert_eq!(binding_json["columns"][0]["ordinal"], 0);
    assert_eq!(binding_json["columns"][0]["type"], "text");
    assert_eq!(binding_json["columns"][1]["type"], "decimal");
    let absent = read_json(&out.join("questions/pub-absent/binding.json"));
    assert_eq!(absent["columns"].as_array().unwrap().len(), 0);
    let absent_gold = read_json(&out.join("questions/pub-absent/gold.json"));
    assert_eq!(absent_gold["expected"]["kind"], "not_found");

    assert_eq!(
        fs::symlink_metadata(&out).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::symlink_metadata(out.join("questions/pub-table/gold.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let before = fs::read(out.join("suite.json")).unwrap();
    let again = compile(&doc, &out).unwrap_err();
    assert_eq!(again.token(), "out_dir");
    assert_eq!(again.pointer(), Some("/out_dir"));
    assert_eq!(fs::read(out.join("suite.json")).unwrap(), before);
}

#[test]
fn executable_bytes_are_hashed_and_not_spawned() {
    let world = World::new("noexec");
    let sentinel = world.root.join("sentinel");
    let script = format!("#!/bin/sh\ntouch {}\n", sentinel.display());
    fs::write(&world.query_pg, &script).unwrap();
    let doc = document(&hand_bank(), &world, ROLE, AS_OF, 1);
    let out = world.out("suite");
    compile(&doc, &out).unwrap();
    assert!(!sentinel.exists());
    let suite = read_json(&out.join("suite.json"));
    assert_eq!(
        suite["executables"]["query_pg"]["sha256"],
        file_hash(&world.query_pg)
    );
}

#[test]
fn symlink_executable_is_refused() {
    let world = World::new("symlink");
    let real = world.root.join("real-bytes");
    fs::write(&real, b"bytes").unwrap();
    fs::remove_file(&world.query_pg).unwrap();
    std::os::unix::fs::symlink(&real, &world.query_pg).unwrap();
    let doc = document(&hand_bank(), &world, ROLE, AS_OF, 1);
    let err = refuse(&doc, &world, "suite");
    assert_eq!(err.token(), "path");
    assert_eq!(err.pointer(), Some("/executables/query_pg/path"));
}

#[test]
fn bad_manifest_refuses_before_a_directory() {
    let world = World::new("manifest");
    let bank = hand_bank();
    let mut doc: Value = serde_json::from_str(&document(&bank, &world, ROLE, AS_OF, 1)).unwrap();
    let original = doc["manifest_sha256"].as_str().unwrap();
    let hash = if original.starts_with('0') {
        format!("1{}", &original[1..])
    } else {
        format!("0{}", &original[1..])
    };
    doc["manifest_sha256"] = json!(hash);
    let err = refuse(&serde_json::to_string(&doc).unwrap(), &world, "suite");
    assert_eq!(err.token(), "hash");
    assert_eq!(err.pointer(), Some("/manifest_sha256"));
}

#[test]
fn identity_mismatch_refuses_the_compile() {
    let world = World::new("identity");
    let bank = hand_bank();
    let good = document(&bank, &world, ROLE, AS_OF, 1);
    let parsed: Value = serde_json::from_str(&good).unwrap();
    let hash = parsed["manifest_sha256"].as_str().unwrap();
    let doc = document_with_hash(
        &bank,
        &world,
        ROLE,
        AS_OF,
        1,
        hash,
        ("other-logistics", SUITE),
    );
    let err = refuse(&doc, &world, "suite");
    assert_eq!(err.token(), "identity");
    assert_eq!(err.pointer(), Some("/tuple/dataset"));
}

#[test]
fn mixed_as_of_refuses_the_whole_role() {
    let bank = bank(vec![
        tiny_rows("pub-early", ROLE, "g-early", AS_OF),
        tiny_rows("pub-late", ROLE, "g-late", AS_OF_OTHER),
    ]);
    let text = serde_json::to_string(&bank).unwrap();
    let evaluation = oo_question_bank::evaluate(&text).unwrap();
    let report: Value = serde_json::from_str(&evaluation.report).unwrap();
    let hash = report["content_hashes"]["manifest"].as_str().unwrap();
    let doc = stub_document(&bank, hash, ROLE, AS_OF, 1);
    let world = World::new("asof");
    let err = refuse(&doc, &world, "suite");
    assert_eq!(err.token(), "group");
    assert_eq!(err.pointer(), Some("/selection/as_of"));
}

#[test]
fn unsupported_kind_refuses_the_sibling_too() {
    let bank = bank(vec![
        tiny_rows("pub-ok", ROLE, "g-ok", AS_OF),
        refuse_question("pub-deny", ROLE, "g-deny"),
    ]);
    let err = refuse_admitted(&bank, "unsupported");
    assert_eq!(err.token(), "unsupported");
    assert_eq!(err.pointer(), Some("/bank/questions/1/expected/kind"));
}

#[test]
fn missing_gold_refuses_the_whole_group() {
    let bank = bank(vec![
        tiny_rows("pub-ok", ROLE, "g-ok", AS_OF),
        missing_gold_question("pub-gap", ROLE, "g-gap"),
    ]);
    let err = refuse_admitted(&bank, "missing");
    assert_eq!(err.token(), "gold");
    assert_eq!(err.pointer(), Some("/bank/questions/1/expected"));
}

#[test]
fn relative_tolerance_is_not_exact_gold() {
    let bank = bank(vec![
        tiny_rows("pub-ok", ROLE, "g-ok", AS_OF),
        relative_question("pub-rel", ROLE, "g-rel"),
    ]);
    let err = refuse_admitted(&bank, "relative");
    assert_eq!(err.token(), "gold");
    assert_eq!(err.pointer(), Some("/bank/questions/1/expected"));
}

#[test]
fn more_than_32_cases_refuses_without_truncation() {
    let mut questions = Vec::new();
    for index in 1..=33 {
        questions.push(tiny_rows(
            &format!("q{index:02}"),
            ROLE,
            &format!("g{index:02}"),
            AS_OF,
        ));
    }
    let bank = bank(questions);
    let text = serde_json::to_string(&bank).unwrap();
    assert!(oo_question_bank::evaluate(&text).unwrap().gate_passed);
    let report: Value =
        serde_json::from_str(&oo_question_bank::evaluate(&text).unwrap().report).unwrap();
    let hash = report["content_hashes"]["manifest"].as_str().unwrap();
    let doc = stub_document(&bank, hash, ROLE, AS_OF, 1);
    let world = World::new("over32");
    let err = refuse(&doc, &world, "suite");
    assert_eq!(err.token(), "budget");
    assert_eq!(err.pointer(), Some("/cases"));
}

#[test]
fn bad_decimal_stays_a_bank_refusal() {
    let bank = bank(vec![json!({
        "accepted_interpretations": [],
        "annotations": [],
        "as_of": AS_OF,
        "asker": {"claims": {}, "role": ROLE},
        "category": ["metric"],
        "control_totals": [],
        "difficulty": "easy",
        "expected": {
            "gold_sql": "select amount from public_receipts",
            "key_columns": [],
            "kind": "scalar",
            "ordered": true,
            "typed_gold": {
                "columns": [{"name": "amount", "type": "decimal"}],
                "completion": "complete",
                "kind": "rows",
                "rows": [[{
                    "coefficient": "0120",
                    "negative": false,
                    "presence": "value",
                    "scale": 2
                }]],
                "warnings": []
            },
            "value_columns": [{"name": "amount", "tolerance": "exact"}]
        },
        "id": "pub-amount",
        "provenance": {
            "annotator": "unassigned",
            "origin": "public_synthetic_fixture",
            "reviewed_by": "unassigned"
        },
        "question": "What amount did the public fixture post?",
        "required_metrics": [],
        "review": {
            "attestation": "label_only",
            "dual_annotation": "not_declared",
            "status": "fully_approved"
        },
        "role_group": "amount",
        "split": "public",
        "version": "public-fixture-1"
    })]);
    let doc = stub_document(&bank, &"ab".repeat(32), ROLE, AS_OF, 1);
    let world = World::new("decimal");
    let err = refuse(&doc, &world, "suite");
    assert_eq!(err.token(), "bank");
    assert_eq!(
        err.pointer(),
        Some("/bank/questions/0/expected/typed_gold/rows/0/0/coefficient")
    );
}

#[test]
fn k_above_8_and_a_banned_mode_refuse() {
    let world = World::new("bounds");
    let doc: Value = serde_json::from_str(&document(&hand_bank(), &world, ROLE, AS_OF, 1)).unwrap();
    let mut high = doc.clone();
    high["k"] = json!(9);
    let err = refuse(&serde_json::to_string(&high).unwrap(), &world, "high");
    assert_eq!(err.token(), "budget");
    assert_eq!(err.pointer(), Some("/k"));
    let mut mode = doc;
    mode["mode"] = json!("production");
    let err = refuse(&serde_json::to_string(&mode).unwrap(), &world, "mode");
    assert_eq!(err.token(), "mode");
    assert_eq!(err.pointer(), Some("/mode"));
}

fn assert_question(out: &std::path::Path, id: &str, sql: &str, comparator: &str, decimal: bool) {
    let request = fs::read_to_string(out.join(format!("questions/{id}/request.json"))).unwrap();
    assert!(request.contains(sql));
    assert!(!request.contains("GOLD_CELL"));
    assert!(!request.contains(PASSWORD));
    let gold = read_json(&out.join(format!("questions/{id}/gold.json")));
    assert_eq!(gold["row_comparator"], comparator);
    assert_eq!(
        gold["expected"]["kind"],
        if id == "pub-absent" {
            "not_found"
        } else {
            "rows"
        }
    );
    if decimal {
        assert_eq!(gold["expected"]["rows"][0][1]["coefficient"], "120");
        assert_eq!(gold["expected"]["rows"][0][1]["scale"], 2);
    }
}

fn refuse_admitted(bank: &Value, name: &str) -> oo_question_bank_suite::ConfigError {
    let text = serde_json::to_string(bank).unwrap();
    let evaluation = oo_question_bank::evaluate(&text).unwrap();
    let report: Value = serde_json::from_str(&evaluation.report).unwrap();
    let hash = report["content_hashes"]["manifest"].as_str().unwrap();
    let doc = stub_document(bank, hash, ROLE, AS_OF, 1);
    let world = World::new(name);
    refuse(&doc, &world, "suite")
}

fn refuse(doc: &str, world: &World, name: &str) -> oo_question_bank_suite::ConfigError {
    let out = world.out(name);
    let err = compile(doc, &out).unwrap_err();
    assert!(!out.exists(), "{}", err.stderr_line());
    err
}
