//! Dataset order of `selected_datasets` for physical names `A` and `b`.
//!
//! `A` maps to ident `q_41`, so `(schema, name)` card order and dataset order
//! disagree. Every outcome must list `public.b` before `public.q_41`.
//! Cards stay in `(schema, name)` order. Candidates stay in dataset order.
//! Responses are hand-captured JSON. This file does not call a model or a database.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use oo_builder::{
    DESCRIPTION_CANDIDATE_COMPILER, DESCRIPTION_CANDIDATE_PROTOCOL, DESCRIPTION_PROMPT_VERSION,
    DESCRIPTION_REQUEST_COMMAND_PROTOCOL, DESCRIPTION_RESPONSE_PROTOCOL, CapturedPgDraft,
    compile_captured_pg_catalog, complete_description_candidates, prepare_description_request,
};

const MODEL: &str = "a262-captured";
const OWNER: &str = "tbl_owner";
const CAPTURED_AT: &str = "2026-10-06T12:00:00.000000Z";
const LOWER: &str = "public.b";
const UPPER: &str = "public.q_41";

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn ident(name: &str) -> String {
    let bytes = name.as_bytes();
    let safe = !bytes.is_empty()
        && bytes.len() <= 63
        && !name.starts_with("q_")
        && name
            .chars()
            .next()
            .is_some_and(|ch| ch == '_' || ch.is_ascii_lowercase())
        && name
            .chars()
            .skip(1)
            .all(|ch| ch == '_' || ch.is_ascii_lowercase() || ch.is_ascii_digit());
    if safe {
        name.to_string()
    } else {
        format!("q_{}", hex_bytes(name.as_bytes()))
    }
}

fn push_string(buf: &mut String, value: &str) {
    buf.push('"');
    for ch in value.chars() {
        match ch {
            '"' => buf.push_str("\\\""),
            '\\' => buf.push_str("\\\\"),
            ch => buf.push(ch),
        }
    }
    buf.push('"');
}

fn write_str(buf: &mut String, name: &str, value: &str) {
    push_string(buf, name);
    buf.push(':');
    push_string(buf, value);
}

fn write_bool(buf: &mut String, name: &str, value: bool) {
    push_string(buf, name);
    buf.push(':');
    buf.push_str(if value { "true" } else { "false" });
}

fn write_i32(buf: &mut String, name: &str, value: i32) {
    push_string(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn write_i64(buf: &mut String, name: &str, value: i64) {
    push_string(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn schema_preimage() -> String {
    let mut buf = String::from("{\"tables\":[");
    for (index, name) in ["A", "b"].into_iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_str(&mut buf, "schema", "public");
        buf.push(',');
        write_str(&mut buf, "name", name);
        buf.push(',');
        write_str(&mut buf, "relkind", "r");
        buf.push(',');
        write_str(&mut buf, "replica_identity", "d");
        buf.push(',');
        write_bool(&mut buf, "rls", false);
        buf.push(',');
        write_bool(&mut buf, "force_rls", false);
        buf.push_str(",\"columns\":[{");
        write_i32(&mut buf, "attnum", 1);
        buf.push(',');
        write_str(&mut buf, "name", "id");
        buf.push(',');
        write_i64(&mut buf, "type_oid", 23);
        buf.push(',');
        write_i32(&mut buf, "typmod", -1);
        buf.push(',');
        write_bool(&mut buf, "not_null", true);
        buf.push(',');
        write_i64(&mut buf, "collation_oid", 0);
        buf.push(',');
        write_str(&mut buf, "typtype", "b");
        buf.push(',');
        push_string(&mut buf, "base_type_oid");
        buf.push_str(":null,");
        write_str(&mut buf, "identity", "");
        buf.push(',');
        write_str(&mut buf, "generated", "");
        buf.push(',');
        push_string(&mut buf, "default_sha256");
        buf.push_str(":null,\"enum_labels\":[],\"domain_checks\":[]}],\"primary_key\":[{");
        write_i32(&mut buf, "attnum", 1);
        buf.push(',');
        write_str(&mut buf, "name", "id");
        buf.push_str(
            "}],\"primary_key_nulls_not_distinct\":false,\"unique_keys\":[],\"foreign_keys\":[],\"policies\":[]}",
        );
    }
    buf.push_str("]}");
    buf
}

fn column(name: &str) -> Value {
    json!({
        "attnum": 1,
        "name": name,
        "name_ident": ident(name),
        "type_oid": 23,
        "type_schema": "pg_catalog",
        "type_name": "int4",
        "typtype": "b",
        "typmod": -1,
        "base_type_oid": Value::Null,
        "numeric_precision": Value::Null,
        "numeric_scale": Value::Null,
        "not_null": true,
        "collation_oid": 0,
        "collation_name": Value::Null,
        "identity": "",
        "generated": "",
        "default_expr": Value::Null,
        "comment": Value::Null,
        "enum_labels": [],
        "domain_checks": [],
        "type_support": {"converted": false, "reason": "known_builtin_not_converted"},
        "current_privileges": {"select": true, "insert": false, "update": false}
    })
}

fn table(name: &str) -> Value {
    json!({
        "schema": "public",
        "schema_ident": ident("public"),
        "name": name,
        "name_ident": ident(name),
        "relkind": "r",
        "replica_identity": "d",
        "rls_enabled": false,
        "rls_forced": false,
        "owner": OWNER,
        "comment": Value::Null,
        "columns": [column("id")],
        "primary_key": [{"attnum": 1, "name": "id"}],
        "primary_key_nulls_not_distinct": false,
        "unique_keys": [],
        "foreign_keys": [],
        "policies": [],
        "privileges": [{"grantee": OWNER, "privilege": "SELECT", "grantable": false}]
    })
}

fn catalog() -> String {
    serde_json::to_string(&json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": "a262_src",
        "database": "a262db",
        "captured_at": CAPTURED_AT,
        "schema_hash": sha256_hex(schema_preimage().as_bytes()),
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": [table("A"), table("b")],
        "type_conversion": "unsupported"
    }))
    .expect("catalog json")
}

fn compile() -> CapturedPgDraft {
    compile_captured_pg_catalog(&catalog(), None, None).unwrap_or_else(|err| {
        panic!("catalog compile failed: {} {}", err.code, err.message);
    })
}

fn budget(max_tables: u32, max_response: u32) -> Value {
    json!({
        "max_caveat_chars": 2000,
        "max_caveats": 8,
        "max_columns_per_table": 64,
        "max_description_chars": 4000,
        "max_grain_chars": 256,
        "max_label_chars": 256,
        "max_open_questions": 8,
        "max_question_chars": 2000,
        "max_request_bytes": 65536,
        "max_response_bytes": max_response,
        "max_tables": max_tables
    })
}

fn visible(name: &str, evidence: &str, schema_hash: &str) -> Value {
    let name_ident = ident(name);
    json!({
        "dataset": format!("public.{name_ident}"),
        "evidence_hash": evidence,
        "name": name,
        "name_ident": name_ident,
        "schema": "public",
        "schema_ident": "public",
        "schema_hash": schema_hash,
        "visibility": "visible"
    })
}

fn command(compiled: &CapturedPgDraft, budget: Value) -> Value {
    json!({
        "admitted": true,
        "artifact": {
            "draft": serde_json::from_str::<Value>(&compiled.draft).expect("draft"),
            "evidence_envelope": serde_json::from_str::<Value>(&compiled.evidence_envelope).expect("envelope"),
            "protocol": "oo.builder.captured-pg-draft-command/v1",
            "wrapper": serde_json::from_str::<Value>(&compiled.wrapper).expect("wrapper")
        },
        "binding": {
            "model_id": MODEL,
            "prompt_version": DESCRIPTION_PROMPT_VERSION,
            "source_id": compiled.source_id
        },
        "budget": budget,
        "protocol": DESCRIPTION_REQUEST_COMMAND_PROTOCOL,
        "selection": {
            "evidence_hash": compiled.evidence_hash,
            "schema_hash": compiled.schema_hash,
            "tables": [
                visible("b", &compiled.evidence_hash, &compiled.schema_hash),
                visible("A", &compiled.evidence_hash, &compiled.schema_hash)
            ]
        }
    })
}

fn run_prepare(command: &Value) -> (String, String) {
    let input = serde_json::to_string(command).expect("command");
    let output = prepare_description_request(&input).unwrap_or_else(|err| {
        panic!("prepare failed: {} {}", err.code, err.message);
    });
    (input, output)
}

fn run_complete(command: &Value, captured: Value) -> (String, String) {
    let mut owned = command.clone();
    owned["captured_response"] = captured;
    let input = serde_json::to_string(&owned).expect("command");
    let output = complete_description_candidates(&input).unwrap_or_else(|err| {
        panic!("complete failed: {} {}", err.code, err.message);
    });
    (input, output)
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("json")
}

fn card_sha<'a>(request: &'a Value, dataset: &str) -> &'a str {
    request["boundary"]["data"]["tables"]
        .as_array()
        .expect("cards")
        .iter()
        .find(|card| card["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing card {dataset}"))["card_sha256"]
        .as_str()
        .expect("card sha")
}

fn candidate(dataset: &str, card: &str, label: &str) -> Value {
    json!({
        "card_sha256": card,
        "caveats": [],
        "confidence": "0",
        "dataset": dataset,
        "description": "A review-only description.",
        "grain": "one row",
        "label": label,
        "open_questions": []
    })
}

fn response_body(request: &Value) -> String {
    serde_json::to_string(&json!({
        "budget_sha256": request["budget_sha256"],
        "candidates": [
            candidate(UPPER, card_sha(request, UPPER), "Upper"),
            candidate(LOWER, card_sha(request, LOWER), "Lower")
        ],
        "complete": true,
        "evidence_hash": request["evidence_hash"],
        "model_id": request["model_id"],
        "prompt_sha256": request["prompt_sha256"],
        "prompt_version": request["prompt_version"],
        "protocol": DESCRIPTION_RESPONSE_PROTOCOL,
        "request_sha256": request["request_sha256"],
        "schema_hash": request["schema_hash"],
        "source_id": request["source_id"]
    }))
    .expect("response")
}

fn captured_body(status: &str, body: &str) -> Value {
    json!({"body": body, "byte_len": body.len(), "status": status})
}

fn note(mismatches: &mut Vec<String>, label: &str, outcome: &Value, expected: &Value) {
    if &outcome["selected_datasets"] != expected {
        mismatches.push(format!("{label}={}", outcome["selected_datasets"]));
    }
}

fn dump(phase: &str, name: &str, input: &str, output: &str) {
    let Ok(dir) = std::env::var("OO_A262_DUMP") else {
        return;
    };
    let path = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&path).expect("dump dir");
    std::fs::write(path.join(format!("{name}.phase")), phase).expect("dump phase");
    std::fs::write(path.join(format!("{name}.in")), input).expect("dump input");
    std::fs::write(path.join(format!("{name}.out")), output).expect("dump output");
}

#[test]
fn physical_a_and_b_sort_selected_datasets_on_every_outcome() {
    assert_eq!(ident("A"), "q_41");
    assert_eq!(ident("b"), "b");
    assert_eq!(CAPTURED_AT.len(), 27);
    let compiled = compile();
    assert_eq!(compiled.source_id, "a262_src");
    let expected = json!([LOWER, UPPER]);
    let full = command(&compiled, budget(16, 65536));

    let mut narrow = full.clone();
    narrow["budget"] = budget(1, 65536);
    let (early_in, early_out) = run_prepare(&narrow);
    let early = parse(&early_out);
    assert_eq!(early["outcome"], "budget_failure", "early-budget");
    assert_eq!(early["budget_reason"], "tables", "early-budget");
    assert!(early["request_sha256"].is_null(), "early-budget");
    assert_eq!(early["candidates"], json!([]), "early-budget");
    assert_eq!(early["selected_datasets"], expected, "early-budget");
    assert_eq!(early["compiler"], DESCRIPTION_CANDIDATE_COMPILER);

    let (request_in, request_out) = run_prepare(&full);
    assert!(!request_out.ends_with('\n'));
    let request = parse(&request_out);
    let cards = request["boundary"]["data"]["tables"]
        .as_array()
        .expect("cards");
    assert_eq!(cards.len(), 2, "card-order");
    assert_eq!(cards[0]["schema"], "public", "card-order");
    assert_eq!(cards[0]["name"], "A", "card-order");
    assert_eq!(cards[0]["dataset"], UPPER, "card-order");
    assert_eq!(cards[1]["schema"], "public", "card-order");
    assert_eq!(cards[1]["name"], "b", "card-order");
    assert_eq!(cards[1]["dataset"], LOWER, "card-order");

    let body = response_body(&request);
    let (success_in, success_out) = run_complete(&full, captured_body("complete", &body));
    let success = parse(&success_out);
    assert_eq!(success["protocol"], DESCRIPTION_CANDIDATE_PROTOCOL);
    assert_eq!(success["outcome"], "candidates");
    assert!(success["refusal"].is_null());
    assert!(success["budget_reason"].is_null());
    assert_eq!(success["request_sha256"], request["request_sha256"]);
    let rows = success["candidates"].as_array().expect("candidates");
    assert_eq!(rows.len(), 2, "candidate-order");
    assert_eq!(rows[0]["dataset"], LOWER, "candidate-order");
    assert_eq!(rows[1]["dataset"], UPPER, "candidate-order");
    assert_eq!(rows[0]["effect"], "review_only", "candidate-order");
    assert_eq!(rows[1]["effect"], "review_only", "candidate-order");
    assert!(rows[0].get("pii").is_none());

    let mut mismatches = Vec::new();
    note(&mut mismatches, "success", &success, &expected);
    let mut dumped = vec![
        ("prepare_request", "early-budget", early_in, early_out),
        ("prepare_request", "request", request_in, request_out),
        ("complete_candidates", "success", success_in, success_out),
    ];

    for status in ["truncated", "incomplete", "provider_failure"] {
        let (input, output) = run_complete(&full, captured_body(status, &body));
        let outcome = parse(&output);
        assert_eq!(outcome["outcome"], status, "{status}");
        assert!(outcome["refusal"].is_null(), "{status}");
        assert_eq!(outcome["candidates"], json!([]), "{status}");
        assert_eq!(outcome["request_sha256"], request["request_sha256"], "{status}");
        note(&mut mismatches, status, &outcome, &expected);
        dumped.push(("complete_candidates", status, input, output));
    }

    let duplicate = format!(
        "{{\"protocol\":\"{DESCRIPTION_RESPONSE_PROTOCOL}\",\"protocol\":\"{DESCRIPTION_RESPONSE_PROTOCOL}\"}}"
    );
    let (format_in, format_out) = run_complete(&full, captured_body("complete", &duplicate));
    let format = parse(&format_out);
    assert_eq!(format["outcome"], "format_rejected", "format");
    assert_eq!(format["refusal"], "duplicate_key", "format");
    assert_eq!(format["candidates"], json!([]), "format");
    assert_eq!(format["request_sha256"], request["request_sha256"], "format");
    note(&mut mismatches, "format", &format, &expected);
    dumped.push((
        "complete_candidates",
        "format",
        format_in,
        format_out,
    ));

    let mut tight = full.clone();
    tight["budget"] = budget(16, 1);
    let (response_in, response_out) = run_complete(
        &tight,
        json!({"body": "zz", "byte_len": 2, "status": "complete"}),
    );
    let response = parse(&response_out);
    assert_eq!(response["outcome"], "budget_failure", "response-budget");
    assert_eq!(response["budget_reason"], "response_bytes", "response-budget");
    assert_eq!(response["candidates"], json!([]), "response-budget");
    assert!(response["request_sha256"].is_string(), "response-budget");
    note(&mut mismatches, "response-budget", &response, &expected);
    dumped.push((
        "complete_candidates",
        "response-budget",
        response_in,
        response_out,
    ));

    assert!(
        mismatches.is_empty(),
        "dataset-order mismatches: {}",
        mismatches.join(" | ")
    );
    for (phase, name, input, output) in dumped {
        dump(phase, name, &input, &output);
    }
}
