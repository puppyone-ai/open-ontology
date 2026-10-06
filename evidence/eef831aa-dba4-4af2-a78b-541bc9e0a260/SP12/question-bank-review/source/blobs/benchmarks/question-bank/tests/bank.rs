use std::io::{self, Write};

use oo_core::decimal::ExactDecimal;
use serde_json::Value;

fn load(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {name}: {error}"))
}

fn evaluate(text: &str) -> oo_question_bank::Evaluation {
    oo_question_bank::evaluate(text).unwrap_or_else(|error| panic!("{}", error.stderr_line()))
}

fn report_of(text: &str) -> Value {
    serde_json::from_str(&evaluate(text).report).unwrap()
}

fn hashes(report: &Value) -> (String, String, String) {
    let hashes = &report["content_hashes"];
    (
        hashes["agent_view"].as_str().unwrap().to_owned(),
        hashes["manifest"].as_str().unwrap().to_owned(),
        hashes["scorer_view"].as_str().unwrap().to_owned(),
    )
}

fn digest(doc: &Value) -> String {
    oo_question_bank::sha256_hex(serde_json::to_string(doc).unwrap().as_bytes())
}

fn config(text: &str) -> oo_question_bank::ConfigError {
    oo_question_bank::evaluate(text).unwrap_err()
}

const CANARIES: [&str; 4] = [
    "GOLD_SQL_CANARY_7c21e9",
    "GOLD_EXPECTED_CANARY_b44e",
    "CONTROL_TOTAL_CANARY_9f3a",
    "ANNOTATION_CANARY_c18d",
];

const FORBIDDEN: [&str; 14] = [
    "accepted_interpretations",
    "annotations",
    "control_totals",
    "expected",
    "gold_sql",
    "key_columns",
    "ordered",
    "provenance",
    "review",
    "role_group",
    "tolerance",
    "typed_gold",
    "unresolved",
    "value_columns",
];

fn forbidden_keys(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if FORBIDDEN.contains(&key.as_str()) {
                    found.push(key.clone());
                }
                forbidden_keys(child, found);
            }
        }
        Value::Array(items) => {
            for item in items {
                forbidden_keys(item, found);
            }
        }
        _ => {}
    }
}

#[test]
fn sha256_matches_fips_180_4() {
    assert_eq!(
        oo_question_bank::sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        oo_question_bank::sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let alphabet = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    assert_eq!(
        oo_question_bank::sha256_hex(alphabet),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

#[test]
fn mixed_bank_keeps_the_denominator_and_hides_gold() {
    let text = load("mixed.json");
    let first = evaluate(&text);
    let second = evaluate(&text);
    assert!(!first.gate_passed);
    assert_eq!(first.report, second.report);
    assert!(!first.report.contains('\n'));

    let sorted = serde_json::to_string(&serde_json::from_str::<Value>(&text).unwrap()).unwrap();
    assert_eq!(evaluate(&sorted).report, first.report);

    let report = serde_json::from_str::<Value>(&first.report).unwrap();
    let (agent_hash, manifest_hash, scorer_hash) = hashes(&report);
    assert_eq!(agent_hash, digest(&report["agent_view"]));
    assert_eq!(scorer_hash, digest(&report["scorer_view"]));
    assert_eq!(manifest_hash, digest(&report["manifest"]));
    assert_eq!(report["manifest"]["agent_view_sha256"], agent_hash);
    assert_eq!(report["manifest"]["scorer_view_sha256"], scorer_hash);
    assert_eq!(report["manifest"]["question_count"], 5);
    assert_eq!(report["manifest"]["fully_approved_count"], 3);
    assert_eq!(report["manifest"]["needs_reannotation_count"], 2);
    assert_eq!(report["scorer_view"]["question_count"], 5);
    assert_eq!(report["manifest"]["distribution"], "public_synthetic");
    assert_eq!(report["manifest"]["scale"], "xs");
    assert_eq!(report["manifest"]["dataset_id"], "oo-logistics");
    assert_eq!(report["manifest"]["suite_id"], "public-smoke");
    assert_eq!(report["manifest"]["track"], "E");

    let ids: Vec<&str> = report["manifest"]["questions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "pub-fin-receipts",
            "pub-disp-receipts",
            "pub-competitor",
            "pub-missing-gold",
            "pub-ambiguous",
        ]
    );
    assert_eq!(
        report["manifest"]["questions"][3]["review_status"],
        "needs_reannotation"
    );
    assert_eq!(
        report["manifest"]["questions"][2]["dual_annotation"],
        "declared_dual_agree"
    );
    assert_eq!(
        report["scorer_view"]["questions"][2]["review"]["attestation"],
        "label_only"
    );
    assert_eq!(
        report["agent_view"]["questions"][4]["question"],
        "这条 lane 指东区月台还是承运线路？"
    );
    assert_eq!(
        report["agent_view"]["questions"][4]["as_of"],
        "2026-09-30T16:00:00+08:00"
    );

    let agent_bytes = serde_json::to_string(&report["agent_view"]).unwrap();
    let scorer_bytes = serde_json::to_string(&report["scorer_view"]).unwrap();
    for canary in CANARIES {
        assert!(!agent_bytes.contains(canary), "{canary}");
        assert!(scorer_bytes.contains(canary), "{canary}");
    }
    let mut found = Vec::new();
    forbidden_keys(&report["agent_view"], &mut found);
    assert!(found.is_empty(), "{found:?}");

    let gold = &report["scorer_view"]["questions"][2]["expected"]["typed_gold"];
    assert_eq!(
        gold.as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        ["completion", "kind", "warnings"]
    );
    assert_eq!(gold["completion"], "complete");
    assert_eq!(gold["kind"], "not_found");
    assert_eq!(gold["warnings"], serde_json::json!([]));
    assert!(gold.get("version").is_none());
    assert!(gold.get("row_comparator").is_none());
    assert!(gold.get("actual").is_none());
}

#[test]
fn content_hashes_follow_the_changed_document() {
    let base = report_of(&load("mixed.json"));
    let (agent, manifest, scorer) = hashes(&base);

    let mut scale = serde_json::from_str::<Value>(&load("mixed.json")).unwrap();
    scale["dataset"]["scale"] = Value::String("s".to_owned());
    let scaled = report_of(&serde_json::to_string(&scale).unwrap());
    let (agent_s, manifest_s, scorer_s) = hashes(&scaled);
    assert_eq!(agent_s, agent);
    assert_eq!(scorer_s, scorer);
    assert_ne!(manifest_s, manifest);
    assert_eq!(scaled["manifest"]["scale"], "s");

    let mut gold = serde_json::from_str::<Value>(&load("mixed.json")).unwrap();
    gold["questions"][0]["expected"]["gold_sql"] = Value::String("select 1".to_owned());
    let gilded = report_of(&serde_json::to_string(&gold).unwrap());
    let (agent_g, manifest_g, scorer_g) = hashes(&gilded);
    assert_eq!(agent_g, agent);
    assert_ne!(scorer_g, scorer);
    assert_ne!(manifest_g, manifest);

    let mut text = serde_json::from_str::<Value>(&load("mixed.json")).unwrap();
    text["questions"][0]["question"] = Value::String("A different public question?".to_owned());
    let rewritten = report_of(&serde_json::to_string(&text).unwrap());
    assert_ne!(hashes(&rewritten).0, agent);

    let mut order = serde_json::from_str::<Value>(&load("mixed.json")).unwrap();
    order["questions"].as_array_mut().unwrap().swap(0, 1);
    let swapped = report_of(&serde_json::to_string(&order).unwrap());
    assert_eq!(swapped, report_of(&load("reordered.json")));
    let (agent_r, manifest_r, scorer_r) = hashes(&swapped);
    assert_ne!(agent_r, agent);
    assert_ne!(manifest_r, manifest);
    assert_ne!(scorer_r, scorer);
}

#[test]
fn approved_decimal_stays_lexical_and_rescales_exactly() {
    let first = evaluate(&load("approved.json"));
    assert!(first.gate_passed);
    assert_eq!(first.report, evaluate(&load("approved.json")).report);
    let report = serde_json::from_str::<Value>(&first.report).unwrap();
    assert_eq!(report["manifest"]["fully_approved_count"], 1);
    assert_eq!(report["manifest"]["needs_reannotation_count"], 0);
    let cell = &report["scorer_view"]["questions"][0]["expected"]["typed_gold"]["rows"][0][1];
    assert_eq!(cell["coefficient"], "120");
    assert_eq!(cell["scale"], 2);
    assert_eq!(cell["negative"], false);
    let stored = ExactDecimal::new(false, "120", 2).unwrap();
    let wider = ExactDecimal::new(false, "1200", 3).unwrap();
    assert_eq!(stored.rescale_exact(3).unwrap(), wider);
    let qty = &report["scorer_view"]["questions"][0]["expected"]["typed_gold"]["rows"][0][2];
    assert_eq!(qty["value"], "7");
}

#[test]
fn declared_unresolved_is_not_upgraded() {
    let mut value = serde_json::from_str::<Value>(&load("approved.json")).unwrap();
    value["questions"][0]["review"]["status"] = Value::String("needs_reannotation".to_owned());
    value["questions"][0]["expected"]["unresolved"] =
        Value::String("declared_unresolved".to_owned());
    let evaluation = evaluate(&serde_json::to_string(&value).unwrap());
    assert!(!evaluation.gate_passed);
    let report = serde_json::from_str::<Value>(&evaluation.report).unwrap();
    assert_eq!(report["manifest"]["fully_approved_count"], 0);
    assert_eq!(report["manifest"]["needs_reannotation_count"], 1);
    assert_eq!(
        report["scorer_view"]["questions"][0]["review"]["status"],
        "needs_reannotation"
    );
    assert_eq!(
        report["scorer_view"]["questions"][0]["expected"]["unresolved"],
        "declared_unresolved"
    );
}

#[test]
fn scalar_top_k_and_bool_gold_match_the_typed_shape() {
    let scalar = r#"{
      "bank_version": "public-fixture-1",
      "dataset": {"distribution": "public_synthetic", "id": "oo-logistics", "scale": "xs"},
      "questions": [{
        "accepted_interpretations": [],
        "annotations": [],
        "as_of": "2026-10-01T00:00:00+08:00",
        "asker": {"claims": {}, "role": "finance_east"},
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
            "rows": [[{"coefficient": "120", "negative": false, "presence": "value", "scale": 2}]],
            "warnings": []
          },
          "value_columns": [{"name": "amount", "tolerance": "exact"}]
        },
        "id": "pub-amount",
        "provenance": {"annotator": "unassigned", "origin": "public_synthetic_fixture", "reviewed_by": "unassigned"},
        "question": "What amount did the public fixture post?",
        "required_metrics": [],
        "review": {"attestation": "label_only", "dual_annotation": "not_declared", "status": "fully_approved"},
        "role_group": "amount",
        "split": "public",
        "version": "public-fixture-1"
      }],
      "suite": {"id": "public-smoke", "track": "E"},
      "version": "oo.benchmark.question-bank/v1"
    }"#;
    assert!(evaluate(scalar).gate_passed);

    let top_k = scalar
        .replace("\"kind\": \"scalar\"", "\"kind\": \"top_k\"")
        .replace("\"key_columns\": []", "\"key_columns\": [\"name\"]")
        .replace(
            "\"columns\": [{\"name\": \"amount\", \"type\": \"decimal\"}]",
            "\"columns\": [{\"name\": \"name\", \"type\": \"text\"}, {\"name\": \"amount\", \"type\": \"decimal\"}]",
        )
        .replace(
            "[[{\"coefficient\": \"120\", \"negative\": false, \"presence\": \"value\", \"scale\": 2}]]",
            "[[{\"presence\": \"value\", \"value\": \"east\"}, {\"coefficient\": \"120\", \"negative\": false, \"presence\": \"value\", \"scale\": 2}]]",
        );
    assert!(evaluate(&top_k).gate_passed);

    let posted = scalar.replace(
        "\"columns\": [{\"name\": \"amount\", \"type\": \"decimal\"}]",
        "\"columns\": [{\"name\": \"posted\", \"type\": \"bool\"}]",
    );
    let posted = posted
        .replace(
            "[[{\"coefficient\": \"120\", \"negative\": false, \"presence\": \"value\", \"scale\": 2}]]",
            "[[{\"presence\": \"value\", \"value\": true}]]",
        )
        .replace(
            "\"value_columns\": [{\"name\": \"amount\", \"tolerance\": \"exact\"}]",
            "\"value_columns\": [{\"name\": \"posted\", \"tolerance\": \"exact\"}]",
        );
    let report = report_of(&posted);
    assert_eq!(
        report["scorer_view"]["questions"][0]["expected"]["typed_gold"]["rows"][0][0]["value"],
        true
    );
}

#[test]
fn shape_and_label_failures_use_the_contract_tokens() {
    let unordered = load("approved.json").replace("\"ordered\": true", "\"ordered\": false");
    let error = config(&unordered.replace("\"kind\": \"table\"", "\"kind\": \"scalar\""));
    assert_eq!(error.token(), "schema");
    assert_eq!(error.pointer(), Some("/questions/0/expected/ordered"));

    let empty_keys = r#"{
      "bank_version": "public-fixture-1",
      "dataset": {"distribution": "public_synthetic", "id": "oo-logistics", "scale": "xs"},
      "questions": [{
        "accepted_interpretations": [],
        "annotations": [],
        "as_of": "2026-10-01T00:00:00+08:00",
        "asker": {"claims": {}, "role": "analyst"},
        "category": ["top_k"],
        "control_totals": [],
        "difficulty": "easy",
        "expected": {
          "gold_sql": "select name from public_receipts",
          "key_columns": [],
          "kind": "top_k",
          "ordered": true,
          "typed_gold": {
            "columns": [{"name": "name", "type": "text"}],
            "completion": "complete",
            "kind": "rows",
            "rows": [[{"presence": "value", "value": "east"}]],
            "warnings": []
          },
          "value_columns": []
        },
        "id": "pub-top",
        "provenance": {"annotator": "unassigned", "origin": "public_synthetic_fixture", "reviewed_by": "unassigned"},
        "question": "Which public name sorts first?",
        "required_metrics": [],
        "review": {"attestation": "label_only", "dual_annotation": "not_declared", "status": "fully_approved"},
        "role_group": "top",
        "split": "public",
        "version": "public-fixture-1"
      }],
      "suite": {"id": "public-smoke", "track": "E"},
      "version": "oo.benchmark.question-bank/v1"
    }"#;
    let error = config(empty_keys);
    assert_eq!(error.token(), "schema");
    assert_eq!(error.pointer(), Some("/questions/0/expected/key_columns"));

    let bad_int = load("bad-decimal.json")
        .replace("\"type\": \"decimal\"", "\"type\": \"int64\"")
        .replace(
            "{ \"coefficient\": \"0120\", \"negative\": false, \"presence\": \"value\", \"scale\": 2 }",
            "{ \"presence\": \"value\", \"value\": \"01\" }",
        );
    let error = config(&bad_int);
    assert_eq!(error.token(), "int64");
    assert_eq!(
        error.pointer(),
        Some("/questions/0/expected/typed_gold/rows/0/0/value")
    );

    let negative_zero = load("bad-decimal.json")
        .replace("\"coefficient\": \"0120\"", "\"coefficient\": \"0\"")
        .replace("\"negative\": false", "\"negative\": true");
    let error = config(&negative_zero);
    assert_eq!(error.token(), "decimal");
    assert_eq!(
        error.pointer(),
        Some("/questions/0/expected/typed_gold/rows/0/0/negative")
    );

    let error = config("null");
    assert_eq!(error.token(), "schema");
    assert_eq!(error.pointer(), Some("/"));

    let mut many = serde_json::from_str::<Value>(&load("bad-version.json")).unwrap();
    many["questions"] = Value::Array(vec![Value::Null; 65]);
    let error = config(&serde_json::to_string(&many).unwrap());
    assert_eq!(error.token(), "budget");
    assert_eq!(error.pointer(), Some("/questions"));

    many["questions"] = Value::Array(Vec::new());
    let error = config(&serde_json::to_string(&many).unwrap());
    assert_eq!(error.token(), "schema");
    assert_eq!(error.pointer(), Some("/questions"));
}

#[test]
fn clarify_not_found_with_extra_fields_stays_unresolved() {
    let text = r#"{
      "bank_version": "public-fixture-1",
      "dataset": {"distribution": "public_synthetic", "id": "oo-logistics", "scale": "xs"},
      "questions": [{
        "accepted_interpretations": [{
          "gold_sql": "select id from public_parties where name = 'other'",
          "id": "missing-party",
          "selected": true,
          "statement": "The name is a party that this public fixture does not hold."
        }],
        "annotations": [],
        "as_of": "2026-10-01T00:00:00+08:00",
        "asker": {"claims": {}, "role": "analyst"},
        "category": ["ambiguity"],
        "control_totals": [],
        "difficulty": "medium",
        "expected": {
          "gold_sql": "select id from public_parties where name = 'other'",
          "kind": "clarify",
          "ordered": true,
          "typed_gold": {"completion": "complete", "kind": "not_found", "warnings": []},
          "unresolved": "missing_gold"
        },
        "id": "pub-which-party",
        "provenance": {"annotator": "unassigned", "origin": "public_synthetic_fixture", "reviewed_by": "unassigned"},
        "question": "Which public party does the selected reading name?",
        "required_metrics": [],
        "review": {"attestation": "label_only", "dual_annotation": "not_declared", "status": "needs_reannotation"},
        "role_group": "which-party",
        "split": "public",
        "version": "public-fixture-1"
      }],
      "suite": {"id": "public-smoke", "track": "E"},
      "version": "oo.benchmark.question-bank/v1"
    }"#;
    let evaluation = evaluate(text);
    assert!(!evaluation.gate_passed);
    let report = serde_json::from_str::<Value>(&evaluation.report).unwrap();
    assert_eq!(
        report["scorer_view"]["questions"][0]["expected"]["unresolved"],
        "missing_gold"
    );

    let clean = text
        .replace("\"ordered\": true,\n          ", "")
        .replace(",\n          \"unresolved\": \"missing_gold\"", "")
        .replace(
            "\"status\": \"needs_reannotation\"",
            "\"status\": \"fully_approved\"",
        );
    assert!(evaluate(&clean).gate_passed);
}

#[test]
fn payload_cap_ignores_identity_bytes() {
    let piece = "a".repeat(64);
    let claims = serde_json::json!({
        "lane": [piece.clone(), piece.clone()],
        "region": [piece.clone(), piece.clone(), piece.clone(), piece],
    });
    let full = "t".repeat(500);
    let short = "t".repeat(472);
    let mut rows = Vec::new();
    for index in 0..27 {
        let text = if index == 26 {
            short.clone()
        } else {
            full.clone()
        };
        rows.push(serde_json::json!([{ "presence": "value", "value": text }]));
    }
    let mut bank = serde_json::json!({
        "bank_version": "public-fixture-1",
        "dataset": {"distribution": "public_synthetic", "id": "oo-logistics", "scale": "xs"},
        "questions": [{
            "accepted_interpretations": [],
            "annotations": [],
            "as_of": "2026-10-01T00:00:00+08:00",
            "asker": {"claims": claims, "role": "r"},
            "category": ["metric"],
            "control_totals": [],
            "difficulty": "easy",
            "expected": {
                "gold_sql": "s".repeat(2000),
                "key_columns": [],
                "kind": "table",
                "ordered": true,
                "typed_gold": {
                    "columns": [{"name": "c", "type": "text"}],
                    "completion": "complete",
                    "kind": "rows",
                    "rows": rows,
                    "warnings": []
                },
                "value_columns": [{"name": "c", "tolerance": "exact"}]
            },
            "id": "q",
            "provenance": {"annotator": "unassigned", "origin": "public_synthetic_fixture", "reviewed_by": "unassigned"},
            "question": "q".repeat(500),
            "required_metrics": [],
            "review": {"attestation": "label_only", "dual_annotation": "not_declared", "status": "fully_approved"},
            "role_group": "g",
            "split": "public",
            "version": "public-fixture-1"
        }],
        "suite": {"id": "public-smoke", "track": "E"},
        "version": "oo.benchmark.question-bank/v1"
    });
    assert!(evaluate(&serde_json::to_string(&bank).unwrap()).gate_passed);
    bank["questions"][0]["expected"]["typed_gold"]["rows"][26][0]["value"] =
        Value::String("t".repeat(473));
    let error = config(&serde_json::to_string(&bank).unwrap());
    assert_eq!(error.token(), "budget");
    assert_eq!(error.pointer(), Some("/"));
}

#[test]
fn stdout_write_failure_is_io() {
    struct Fail;
    impl Write for Fail {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("fail"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut err = Vec::new();
    let code = oo_question_bank::run(["--help".to_owned()], std::io::empty(), Fail, &mut err);
    assert_eq!(code, 2);
    assert_eq!(err, b"question-bank: io\n");
}
