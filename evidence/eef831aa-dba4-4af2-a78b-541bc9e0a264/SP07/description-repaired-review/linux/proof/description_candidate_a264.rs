//! Independent review fixture for physical names `A` and `b`.
//!
//! Caller selection order is `b` then `A`. Cards stay in `(schema, name)`
//! order, so `A` / `public.q_41` comes first. Every outcome's
//! `selected_datasets` is the dataset sort, `public.b` then `public.q_41`.
//! Candidates stay in dataset order. Responses are hand-captured JSON.
//! This file does not call a model or a database.

use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use oo_builder::{
    DESCRIPTION_CANDIDATE_COMPILER, DESCRIPTION_CANDIDATE_PROTOCOL, DESCRIPTION_PROMPT_VERSION,
    DESCRIPTION_REQUEST_COMMAND_PROTOCOL, DESCRIPTION_REQUEST_PROTOCOL,
    DESCRIPTION_RESPONSE_PROTOCOL, CapturedPgDraft, compile_captured_pg_catalog,
    complete_description_candidates, prepare_description_request,
};

const MODEL: &str = "a264-captured";
const OWNER: &str = "a264_owner_do_not_emit";
const SOURCE: &str = "a264_src";
const DATABASE: &str = "a264db";
const CAPTURED_AT: &str = "2026-10-06T18:05:00.000000Z";
const LOWER: &str = "public.b";
const UPPER: &str = "public.q_41";
const LABEL_B: &str = "b-label-kept";
const LABEL_A: &str = "A-label-kept";
const UNKNOWN: &str = "a264_secret_table_do_not_echo";
const ARGV_SECRET: &str = "a264-argv-secret-do-not-print";
const EXTRA_SECRET: &str = "a264-extra-argv-secret";
const STDIN_SECRET: &str = "a264-stdin-secret-do-not-print";
const DUP_SECRET: &str = "a264-dup-sentinel-do-not-print";
const RESPONSE_SECRET: &str = "a264-response-body-do-not-keep";
const PREPARE_SECRET: &str = "a264-prepare-body-do-not-print";
const STDIN_LIMIT: usize = 1_048_576;

const STATEMENTS: &[(&str, &str)] = &[
    ("auto_accept", "not_authorized"),
    ("certification", "none"),
    ("confidence_effect", "recorded_only"),
    ("cost", "not_metered"),
    ("curated", "not_written"),
    ("effective", "not_written"),
    ("grants", "not_authorized"),
    ("llm_transport", "not_used"),
    ("pii_classification", "not_performed"),
    ("publication", "unavailable"),
    ("residency", "not_approved"),
    ("samples", "not_included"),
];

struct Doc {
    name: &'static str,
    phase: &'static str,
    input: String,
    output: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
        format!("q_{}", sha256_hex_raw(name.as_bytes()))
    }
}

fn sha256_hex_raw(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn push_string(buf: &mut String, value: &str) {
    buf.push('"');
    for ch in value.chars() {
        match ch {
            '"' => buf.push_str("\\\""),
            '\\' => buf.push_str("\\\\"),
            other => buf.push(other),
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
        write_i64(&mut buf, "attnum", 1);
        buf.push(',');
        write_str(&mut buf, "name", "id");
        buf.push(',');
        write_i64(&mut buf, "type_oid", 23);
        buf.push(',');
        write_i64(&mut buf, "typmod", -1);
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
        write_i64(&mut buf, "attnum", 1);
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
        "source_id": SOURCE,
        "database": DATABASE,
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

fn budget(max_tables: u32, max_request: u32, max_response: u32) -> Value {
    json!({
        "max_caveat_chars": 2000,
        "max_caveats": 8,
        "max_columns_per_table": 64,
        "max_description_chars": 4000,
        "max_grain_chars": 256,
        "max_label_chars": 256,
        "max_open_questions": 8,
        "max_question_chars": 2000,
        "max_request_bytes": max_request,
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

fn command(compiled: &CapturedPgDraft, budget_value: Value) -> Value {
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
        "budget": budget_value,
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

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|err| panic!("json: {err}"))
}

fn prepare(command_value: &Value) -> (String, String) {
    let input = serde_json::to_string(command_value).expect("command");
    let output = prepare_description_request(&input).unwrap_or_else(|err| {
        panic!("prepare failed: {} {}", err.code, err.message);
    });
    (input, output)
}

fn complete(command_value: &Value, captured: Value) -> (String, String) {
    let mut owned = command_value.clone();
    owned["captured_response"] = captured;
    let input = serde_json::to_string(&owned).expect("command");
    let output = complete_description_candidates(&input).unwrap_or_else(|err| {
        panic!("complete failed: {} {}", err.code, err.message);
    });
    (input, output)
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
        "confidence": if dataset == LOWER { "0" } else { "1" },
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
            candidate(UPPER, card_sha(request, UPPER), LABEL_A),
            candidate(LOWER, card_sha(request, LOWER), LABEL_B)
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

fn doc(name: &'static str, phase: &'static str, input: String, output: String) -> Doc {
    Doc {
        name,
        phase,
        input,
        output,
    }
}

fn documents(compiled: &CapturedPgDraft) -> Vec<Doc> {
    let full = command(compiled, budget(16, 65536, 65536));
    let tables = full["selection"]["tables"].as_array().expect("tables");
    assert_eq!(tables[0]["name"], "b", "caller lists b first");
    assert_eq!(tables[1]["name"], "A", "caller lists A second");

    let mut narrow = full.clone();
    narrow["budget"] = budget(1, 65536, 65536);
    let (early_in, early_out) = prepare(&narrow);

    let mut tight_request = full.clone();
    tight_request["budget"] = budget(16, 1, 65536);
    let (bytes_in, bytes_out) = prepare(&tight_request);

    let (request_in, request_out) = prepare(&full);
    let request = parse(&request_out);
    let body = response_body(&request);
    let (success_in, success_out) = complete(&full, captured_body("complete", &body));

    let mut docs = vec![
        doc("early-budget", "prepare_request", early_in, early_out),
        doc("request-bytes", "prepare_request", bytes_in, bytes_out),
        doc("request", "prepare_request", request_in, request_out),
        doc("success", "complete_candidates", success_in, success_out),
    ];
    for status in ["truncated", "incomplete", "provider_failure"] {
        let (input, output) = complete(&full, captured_body(status, &body));
        docs.push(doc(status, "complete_candidates", input, output));
    }
    let duplicate = format!(
        "{{\"protocol\":\"{DESCRIPTION_RESPONSE_PROTOCOL}\",\"protocol\":\"{DUP_SECRET}\"}}"
    );
    let (malformed_in, malformed_out) = complete(&full, captured_body("complete", &duplicate));
    docs.push(doc(
        "malformed",
        "complete_candidates",
        malformed_in,
        malformed_out,
    ));
    let mut tight_response = full.clone();
    tight_response["budget"] = budget(16, 65536, 1);
    let (response_in, response_out) = complete(
        &tight_response,
        json!({"body": RESPONSE_SECRET, "byte_len": RESPONSE_SECRET.len(), "status": "complete"}),
    );
    docs.push(doc(
        "response-bytes",
        "complete_candidates",
        response_in,
        response_out,
    ));
    docs
}

fn assert_no_secret(text: &str, name: &str) {
    assert!(!text.contains(OWNER), "{name} emitted the owner");
    assert!(!text.contains(UNKNOWN), "{name} emitted the unknown table");
    assert!(!text.contains(DUP_SECRET), "{name} emitted the duplicate-key sentinel");
    assert!(
        !text.contains(RESPONSE_SECRET),
        "{name} kept the response-byte body"
    );
    assert!(
        !text.contains(PREPARE_SECRET),
        "{name} emitted the prepare body"
    );
}

fn assert_statements(outcome: &Value, name: &str) {
    for &(key, value) in STATEMENTS {
        assert_eq!(outcome["statements"][key], value, "{name} statements.{key}");
    }
}

fn assert_draft(outcome: &Value, compiled: &CapturedPgDraft, name: &str) {
    let draft = &outcome["structural_draft"];
    assert_eq!(draft["source_id"].as_str(), Some(compiled.source_id.as_str()), "{name} draft");
    assert_eq!(draft["schema_hash"].as_str(), Some(compiled.schema_hash.as_str()), "{name} draft");
    assert_eq!(draft["evidence_hash"].as_str(), Some(compiled.evidence_hash.as_str()), "{name} draft");
    assert_eq!(draft["draft_hash"].as_str(), Some(compiled.draft_hash.as_str()), "{name} draft");
    assert_eq!(draft["manifest_hash"].as_str(), Some(compiled.manifest_hash.as_str()), "{name} draft");
    assert_eq!(draft["protocol"], "oo.builder.structure-draft/v1", "{name} draft");
}

fn assert_sorted(outcome: &Value, name: &str) {
    assert_eq!(
        outcome["selected_datasets"],
        json!([LOWER, UPPER]),
        "{name} selected_datasets"
    );
    assert_ne!(
        outcome["selected_datasets"],
        json!([UPPER, LOWER]),
        "{name} followed card order"
    );
}

fn assert_cards(request: &Value) {
    let cards = request["boundary"]["data"]["tables"]
        .as_array()
        .expect("cards");
    assert_eq!(cards.len(), 2);
    assert_eq!(cards[0]["schema"], "public");
    assert_eq!(cards[0]["name"], "A");
    assert_eq!(cards[0]["dataset"], UPPER);
    assert_eq!(cards[1]["schema"], "public");
    assert_eq!(cards[1]["name"], "b");
    assert_eq!(cards[1]["dataset"], LOWER);
    assert_eq!(request["protocol"], DESCRIPTION_REQUEST_PROTOCOL);
    assert!(!request.to_string().contains(OWNER));
}

fn assert_success(outcome: &Value, request_sha: &Value, request: &Value) {
    assert_eq!(outcome["outcome"], "candidates");
    assert!(outcome["refusal"].is_null());
    assert!(outcome["budget_reason"].is_null());
    assert_eq!(outcome["request_sha256"], *request_sha);
    let rows = outcome["candidates"].as_array().expect("candidates");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["dataset"], LOWER);
    assert_eq!(rows[1]["dataset"], UPPER);
    assert_eq!(rows[0]["label"], LABEL_B);
    assert_eq!(rows[1]["label"], LABEL_A);
    assert_eq!(rows[0]["card_sha256"], card_sha(request, LOWER));
    assert_eq!(rows[1]["card_sha256"], card_sha(request, UPPER));
    assert_eq!(rows[0]["effect"], "review_only");
    assert_eq!(rows[1]["effect"], "review_only");
    assert_eq!(rows[0]["confidence"], "0");
    assert_eq!(rows[1]["confidence"], "1");
    assert!(rows[0].get("pii").is_none());
    assert!(rows[0].get("publication").is_none());
    assert!(rows[0].get("grants").is_none());
    assert!(rows[0].get("curated").is_none());
}

fn assert_empty_candidates(outcome: &Value, name: &str) {
    assert_eq!(outcome["candidates"], json!([]), "{name}");
    assert!(!outcome.to_string().contains(LABEL_B), "{name} kept a label");
    assert!(!outcome.to_string().contains(LABEL_A), "{name} kept a label");
}

fn assert_outcome(name: &str, outcome: &Value, compiled: &CapturedPgDraft, request_sha: &Value, request: &Value) {
    assert_eq!(outcome["protocol"], DESCRIPTION_CANDIDATE_PROTOCOL, "{name}");
    assert_eq!(outcome["compiler"], DESCRIPTION_CANDIDATE_COMPILER, "{name}");
    assert_sorted(outcome, name);
    assert_draft(outcome, compiled, name);
    assert_statements(outcome, name);
    match name {
        "early-budget" => {
            assert_eq!(outcome["outcome"], "budget_failure");
            assert_eq!(outcome["budget_reason"], "tables");
            assert!(outcome["request_sha256"].is_null());
            assert!(outcome["refusal"].is_null());
            assert_empty_candidates(outcome, name);
        }
        "request-bytes" => {
            assert_eq!(outcome["outcome"], "budget_failure");
            assert_eq!(outcome["budget_reason"], "request_bytes");
            assert!(outcome["request_sha256"].is_null());
            assert!(outcome["refusal"].is_null());
            assert_empty_candidates(outcome, name);
        }
        "response-bytes" => {
            assert_eq!(outcome["outcome"], "budget_failure");
            assert_eq!(outcome["budget_reason"], "response_bytes");
            assert_eq!(outcome["request_sha256"], *request_sha);
            assert!(outcome["refusal"].is_null());
            assert_empty_candidates(outcome, name);
        }
        "success" => assert_success(outcome, request_sha, request),
        "truncated" | "incomplete" | "provider_failure" => {
            assert_eq!(outcome["outcome"], name);
            assert!(outcome["refusal"].is_null());
            assert!(outcome["budget_reason"].is_null());
            assert_eq!(outcome["request_sha256"], *request_sha);
            assert_empty_candidates(outcome, name);
        }
        "malformed" => {
            assert_eq!(outcome["outcome"], "format_rejected");
            assert_eq!(outcome["refusal"], "duplicate_key");
            assert!(outcome["budget_reason"].is_null());
            assert_eq!(outcome["request_sha256"], *request_sha);
            assert_empty_candidates(outcome, name);
        }
        other => panic!("unexpected document {other}"),
    }
}

fn check_library(docs: &[Doc], compiled: &CapturedPgDraft) {
    let request_doc = docs.iter().find(|item| item.name == "request").expect("request");
    let request = parse(&request_doc.output);
    assert_cards(&request);
    let request_sha = request["request_sha256"].clone();
    assert!(request_sha.is_string());
    for item in docs {
        assert!(!item.output.ends_with('\n'), "{}", item.name);
        assert_no_secret(&item.output, item.name);
        if item.name == "request" {
            continue;
        }
        assert_outcome(item.name, &parse(&item.output), compiled, &request_sha, &request);
    }
}

fn run(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_description-candidate"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn description-candidate");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("write stdin");
    child.wait_with_output().expect("wait")
}

fn check_cli(docs: &[Doc]) {
    let bin = env!("CARGO_BIN_EXE_description-candidate");
    assert!(
        std::path::Path::new(bin)
            .file_name()
            .and_then(|name| name.to_str())
            == Some("description-candidate"),
        "{bin}"
    );
    for item in docs {
        let output = run(&[item.phase], item.input.as_bytes());
        assert_eq!(
            output.status.code(),
            Some(0),
            "{} cli status stderr={}",
            item.name,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "{} cli stderr {}",
            item.name,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.stdout,
            item.output.as_bytes(),
            "{} cli stdout differs from the library document",
            item.name
        );
    }
}

fn usage_text() -> String {
    [
        "description-candidate: usage: description-candidate <phase>",
        "description-candidate:   prepare_request        one oo.builder.description-request-command/v1 document on stdin, without captured_response",
        "description-candidate:   complete_candidates    the same document plus captured_response",
        "description-candidate: the phase is required and is not inferred",
        "description-candidate: stdout is the public compiler document and has no added newline",
        "description-candidate: this command does not call a model, open a network connection, read credentials, use PostgreSQL, write curated or effective text, grant, certify, authenticate, or publish",
    ]
    .join("\n")
        + "\n"
}

fn assert_closed(output: &Output, code: i32, stderr: &str, absent: &[&str]) {
    assert_eq!(output.status.code(), Some(code), "stderr={}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stdout.is_empty(), "stdout={}", String::from_utf8_lossy(&output.stdout));
    let text = String::from_utf8(output.stderr.clone()).expect("stderr utf-8");
    assert_eq!(text, stderr);
    for secret in absent {
        assert!(!text.contains(secret), "stderr contained {secret}");
    }
}

fn error_line(code: &str, message: &str) -> String {
    format!("description-candidate: {code} {message}\n")
}

fn command_code(name: &str) -> String {
    format!("{DESCRIPTION_REQUEST_COMMAND_PROTOCOL}/{name}")
}

fn candidate_code(name: &str) -> String {
    format!("{DESCRIPTION_CANDIDATE_PROTOCOL}/{name}")
}

fn check_boundaries(compiled: &CapturedPgDraft) {
    let secrets = [
        OWNER,
        UNKNOWN,
        ARGV_SECRET,
        EXTRA_SECRET,
        STDIN_SECRET,
        DUP_SECRET,
        RESPONSE_SECRET,
        PREPARE_SECRET,
        "prepare_description_request",
        "complete_description_candidates",
    ];
    assert_closed(&run(&[], b""), 2, &usage_text(), &secrets);
    assert_closed(
        &run(&[ARGV_SECRET], STDIN_SECRET.as_bytes()),
        2,
        "description-candidate: unknown phase\n",
        &secrets,
    );
    assert_closed(
        &run(&["prepare_description_request"], b""),
        2,
        "description-candidate: unknown phase\n",
        &secrets,
    );
    assert_closed(
        &run(&["complete_description_candidates"], b""),
        2,
        "description-candidate: unknown phase\n",
        &secrets,
    );
    assert_closed(
        &run(&["prepare_request", EXTRA_SECRET], STDIN_SECRET.as_bytes()),
        2,
        "description-candidate: the phase takes no arguments\n",
        &secrets,
    );
    assert_closed(
        &run(&["complete_candidates", EXTRA_SECRET], b""),
        2,
        "description-candidate: the phase takes no arguments\n",
        &secrets,
    );

    let mut oversize = STDIN_SECRET.repeat((STDIN_LIMIT / STDIN_SECRET.len()) + 2);
    oversize.truncate(STDIN_LIMIT + 1);
    assert_eq!(oversize.len(), STDIN_LIMIT + 1);
    assert_closed(
        &run(&["prepare_request"], oversize.as_bytes()),
        1,
        &error_line(&command_code("json.budget.bytes"), "document exceeds the byte budget"),
        &secrets,
    );
    assert_closed(
        &run(&["prepare_request"], &[0xff, 0xfe]),
        1,
        &error_line(&command_code("json.syntax"), "stdin is not UTF-8"),
        &secrets,
    );
    assert_closed(
        &run(&["prepare_request"], b"{"),
        1,
        &error_line(&command_code("json.syntax"), "description command JSON is not valid"),
        &secrets,
    );
    let duplicate = format!(
        "{{\"protocol\":\"{DESCRIPTION_REQUEST_COMMAND_PROTOCOL}\",\"protocol\":\"{DUP_SECRET}\"}}"
    );
    assert_closed(
        &run(&["prepare_request"], duplicate.as_bytes()),
        1,
        &error_line(&command_code("json.duplicate_key"), "duplicate JSON object key"),
        &secrets,
    );

    let full = command(compiled, budget(16, 65536, 65536));
    let mut with_response = full.clone();
    with_response["captured_response"] = json!({
        "body": PREPARE_SECRET,
        "byte_len": PREPARE_SECRET.len(),
        "status": "truncated"
    });
    let prepared = serde_json::to_string(&with_response).expect("prepare command");
    assert_closed(
        &run(&["prepare_request"], prepared.as_bytes()),
        1,
        &error_line(&command_code("field.unknown"), "prepare does not accept a captured response"),
        &secrets,
    );
    let bare = serde_json::to_string(&full).expect("bare command");
    assert_closed(
        &run(&["complete_candidates"], bare.as_bytes()),
        1,
        &error_line(&command_code("field.missing"), "complete requires a captured response"),
        &secrets,
    );

    let mut unknown = full.clone();
    unknown["selection"]["tables"][0]["name"] = json!(UNKNOWN);
    let unknown_text = serde_json::to_string(&unknown).expect("unknown command");
    assert!(unknown_text.contains(UNKNOWN));
    assert!(unknown_text.contains(OWNER));
    assert_closed(
        &run(&["prepare_request"], unknown_text.as_bytes()),
        1,
        &error_line(
            &candidate_code("selection.unknown"),
            "selected table is not in the sealed captured draft",
        ),
        &secrets,
    );
}

#[test]
fn physical_a_and_b_library_and_cli() {
    assert_eq!(ident("A"), "q_41");
    assert_eq!(ident("b"), "b");
    assert_eq!(CAPTURED_AT.len(), 27);
    let compiled = compile();
    assert_eq!(compiled.source_id, SOURCE);
    assert_eq!(compiled.database, DATABASE);
    let docs = documents(&compiled);
    assert_eq!(docs.len(), 9);
    check_library(&docs, &compiled);
    check_cli(&docs);
    check_boundaries(&compiled);
}
