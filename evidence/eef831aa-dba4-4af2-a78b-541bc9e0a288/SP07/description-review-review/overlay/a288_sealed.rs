//! Independent sealed `public.A` / `public.b` review.
//!
//! The catalog, candidate text, reviewer, and curated documents in this file
//! are not the author fixtures. The compiler is
//! `oo_builder::complete_description_candidates`. Canonical JSON and field
//! hashes are computed here.

#![forbid(unsafe_code)]

use std::process::Command;
use std::sync::OnceLock;

use description_review::{
    COMMAND_PROTOCOL, REVIEW_COMPILER, REVIEW_PROTOCOL, apply_description_review,
    curated_dataset_sha256, descriptive_field_sha256,
};
use oo_builder::{
    DESCRIPTION_PROMPT_VERSION, DESCRIPTION_REQUEST_COMMAND_PROTOCOL, DESCRIPTION_RESPONSE_PROTOCOL,
    compile_captured_pg_catalog, complete_description_candidates, prepare_description_request,
};
use oo_core::api::payload_sha256;
use oo_core::oos::{DatasetFile, validate_dataset};
use serde_json::{Map, Value, json};

const MODEL: &str = "a288-sealed";
const OWNER: &str = "seal_owner";
const CAPTURED_AT: &str = "2026-03-15T08:30:00.000000Z";
const SOURCE: &str = "a288_src";
const DATABASE: &str = "a288db";
const LOWER: &str = "public.b";
const UPPER: &str = "public.q_41";
const ASIDE: &str = "public.aside";
const REVIEWER: &str = "nia.declared";
const REVIEWED_ON: &str = "2026-03-15";
const TOKEN: &str = "A288_CLASS_STATUS_TOKEN";

struct World {
    schema_hash: String,
    evidence_hash: String,
    command: Value,
    request: Value,
}

struct Produced {
    command_text: String,
    outcome_sha: String,
    outcome: Value,
}

struct Anchors {
    request: String,
    card: String,
    schema: String,
    evidence: String,
    candidate: String,
    curated: String,
}

struct OneField {
    command: String,
    outcome_sha: String,
    outcome_statements: Value,
    expected_lower: Value,
    upper: Value,
    aside: Value,
}

fn world() -> &'static World {
    static WORLD: OnceLock<World> = OnceLock::new();
    WORLD.get_or_init(|| {
        let compiled = compile_captured_pg_catalog(&catalog(), None, None).unwrap_or_else(|err| {
            panic!("catalog compile failed: {} {}", err.code, err.message);
        });
        let command = command_value(&compiled);
        let input = serde_json::to_string(&command).expect("command");
        let request_text = prepare_description_request(&input).unwrap_or_else(|err| {
            panic!("prepare failed: {} {}", err.code, err.message);
        });
        let request: Value = serde_json::from_str(&request_text).expect("request json");
        World {
            schema_hash: compiled.schema_hash,
            evidence_hash: compiled.evidence_hash,
            command,
            request,
        }
    })
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

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn push_string(buf: &mut String, value: &str) {
    buf.push('"');
    for ch in value.chars() {
        match ch {
            '"' => buf.push_str("\\\""),
            '\\' => buf.push_str("\\\\"),
            '\u{0008}' => buf.push_str("\\b"),
            '\u{000c}' => buf.push_str("\\f"),
            '\n' => buf.push_str("\\n"),
            '\r' => buf.push_str("\\r"),
            '\t' => buf.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                let code = u32::from(ch);
                buf.push_str(&format!("\\u{code:04x}"));
            }
            ch => buf.push(ch),
        }
    }
    buf.push('"');
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => push_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                push_string(out, key);
                out.push(':');
                write_value(out, &map[*key]);
            }
            out.push('}');
        }
    }
}

fn canonical_text(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn canonical_sha(value: &Value) -> String {
    payload_sha256(canonical_text(value).as_bytes())
}

fn field_literal(value: Option<&str>) -> String {
    match value {
        None => "null".to_owned(),
        Some(text) => {
            let mut out = String::new();
            push_string(&mut out, text);
            out
        }
    }
}

fn field_sha(value: Option<&str>) -> String {
    payload_sha256(field_literal(value).as_bytes())
}

fn write_str(buf: &mut String, name: &str, value: &str) {
    push_preimage_string(buf, name);
    buf.push(':');
    push_preimage_string(buf, value);
}

fn push_preimage_string(buf: &mut String, value: &str) {
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

fn write_bool(buf: &mut String, name: &str, value: bool) {
    push_preimage_string(buf, name);
    buf.push(':');
    buf.push_str(if value { "true" } else { "false" });
}

fn write_i32(buf: &mut String, name: &str, value: i32) {
    push_preimage_string(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn write_i64(buf: &mut String, name: &str, value: i64) {
    push_preimage_string(buf, name);
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
        push_preimage_string(&mut buf, "base_type_oid");
        buf.push_str(":null,");
        write_str(&mut buf, "identity", "");
        buf.push(',');
        write_str(&mut buf, "generated", "");
        buf.push(',');
        push_preimage_string(&mut buf, "default_sha256");
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
        "source_id": SOURCE,
        "database": DATABASE,
        "captured_at": CAPTURED_AT,
        "schema_hash": payload_sha256(schema_preimage().as_bytes()),
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": [table("A"), table("b")],
        "type_conversion": "unsupported"
    }))
    .expect("catalog json")
}

fn command_value(compiled: &oo_builder::CapturedPgDraft) -> Value {
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
        "budget": {
            "max_caveat_chars": 2000,
            "max_caveats": 8,
            "max_columns_per_table": 64,
            "max_description_chars": 4000,
            "max_grain_chars": 256,
            "max_label_chars": 256,
            "max_open_questions": 8,
            "max_question_chars": 2000,
            "max_request_bytes": 65536,
            "max_response_bytes": 65536,
            "max_tables": 16
        },
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

fn card_sha(dataset: &str) -> String {
    world().request["boundary"]["data"]["tables"]
        .as_array()
        .expect("cards")
        .iter()
        .find(|card| card["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing card {dataset}"))["card_sha256"]
        .as_str()
        .expect("card")
        .to_owned()
}

fn candidate(
    dataset: &str,
    label: &str,
    description: &str,
    grain: &str,
    confidence: &str,
    caveats: Value,
    questions: Value,
) -> Value {
    json!({
        "card_sha256": card_sha(dataset),
        "caveats": caveats,
        "confidence": confidence,
        "dataset": dataset,
        "description": description,
        "grain": grain,
        "label": label,
        "open_questions": questions
    })
}

fn finish_produced(command: Value) -> Produced {
    let command_text = serde_json::to_string(&command).expect("command text");
    let outcome_text = complete_description_candidates(&command_text).unwrap_or_else(|err| {
        panic!("complete failed: {} {}", err.code, err.message);
    });
    let again = complete_description_candidates(&command_text).unwrap_or_else(|err| {
        panic!("reexecute failed: {} {}", err.code, err.message);
    });
    assert_eq!(again, outcome_text, "reexecute bytes");
    let outcome: Value = serde_json::from_str(&outcome_text).expect("outcome json");
    assert_eq!(outcome_text, canonical_text(&outcome), "outcome canonical");
    let outcome_sha = payload_sha256(outcome_text.as_bytes());
    Produced {
        command_text,
        outcome_sha,
        outcome,
    }
}

fn produce(candidates: Value) -> Produced {
    let body = serde_json::to_string(&json!({
        "budget_sha256": world().request["budget_sha256"],
        "candidates": candidates,
        "complete": true,
        "evidence_hash": world().request["evidence_hash"],
        "model_id": world().request["model_id"],
        "prompt_sha256": world().request["prompt_sha256"],
        "prompt_version": world().request["prompt_version"],
        "protocol": DESCRIPTION_RESPONSE_PROTOCOL,
        "request_sha256": world().request["request_sha256"],
        "schema_hash": world().request["schema_hash"],
        "source_id": world().request["source_id"]
    }))
    .expect("response");
    let mut command = world().command.clone();
    command["captured_response"] = json!({
        "body": body,
        "byte_len": body.len(),
        "status": "complete"
    });
    finish_produced(command)
}

fn curated_document(
    dataset: &str,
    label: &str,
    description: Option<&str>,
    grain: Option<&str>,
) -> Value {
    let (schema, table) = dataset.split_once('.').expect("dataset");
    let value = json!({
        "schema_version": "oos/v1",
        "dataset": dataset,
        "source_name": "sealed_src",
        "label": label,
        "description": description,
        "tier": "raw",
        "recommended": false,
        "superseded_by": "public.aside",
        "grain": grain,
        "primary_key": ["id"],
        "entity": "customer",
        "row_count_estimate": 17,
        "time_columns": [{"name": "id", "role": "created_at"}],
        "columns": [
            {
                "name": "id",
                "label": "Seal id",
                "data_type": "int4",
                "semantic_type": "identifier",
                "pii": "none"
            },
            {
                "name": "phone",
                "label": "Seal phone",
                "data_type": "text",
                "semantic_type": "phone",
                "searchable": false,
                "pii": "personal",
                "classification": {"state": "confirmed", "by": "ops.mina", "at": "2026-03-15"},
                "enum": format!("{schema}.{table}.code"),
                "status": "deprecated",
                "replaced_by": format!("{schema}.{table}.phone2"),
                "deprecated_at": "ontology/v9",
                "description": "sealed phone note",
                "provenance": "human",
                "review": {"state": "approved", "by": "nia.curator", "at": "2026-03-01"}
            }
        ],
        "caveats": ["sealed curated caveat"]
    });
    let file: DatasetFile = serde_json::from_value(value).unwrap_or_else(|err| {
        panic!("curated {dataset} did not parse: {err}");
    });
    let typed = serde_json::to_value(&file).expect("typed dataset");
    assert_eq!(canonical_sha(&typed), curated_dataset_sha256(&file));
    assert!(validate_dataset(&file).is_empty());
    typed
}

fn anchors_for(produced: &Produced, dataset: &str, curated_hash: &str) -> Anchors {
    let candidate = produced.outcome["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .find(|item| item["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing outcome candidate {dataset}"));
    Anchors {
        request: produced.outcome["request_sha256"]
            .as_str()
            .expect("request")
            .to_owned(),
        card: candidate["card_sha256"].as_str().expect("card").to_owned(),
        schema: produced.outcome["structural_draft"]["schema_hash"]
            .as_str()
            .expect("schema")
            .to_owned(),
        evidence: produced.outcome["structural_draft"]["evidence_hash"]
            .as_str()
            .expect("evidence")
            .to_owned(),
        candidate: canonical_sha(candidate),
        curated: curated_hash.to_owned(),
    }
}

fn decision_json(
    dataset: &str,
    kind: &str,
    anchors: &Anchors,
    fields: Option<Value>,
) -> Value {
    let mut value = json!({
        "candidate_sha256": anchors.candidate,
        "card_sha256": anchors.card,
        "curated_sha256": anchors.curated,
        "dataset": dataset,
        "decision": kind,
        "evidence_hash": anchors.evidence,
        "request_sha256": anchors.request,
        "reviewer_at": REVIEWED_ON,
        "reviewer_label": REVIEWER,
        "schema_hash": anchors.schema
    });
    if let Some(fields) = fields {
        value["fields"] = fields;
    }
    value
}

fn review_text(
    produced: &Produced,
    schema: &str,
    evidence: &str,
    curated: Value,
    decisions: Vec<Value>,
) -> String {
    serde_json::to_string(&json!({
        "command": produced.command_text,
        "current": {"evidence_hash": evidence, "schema_hash": schema},
        "curated": curated,
        "decisions": decisions,
        "expected_outcome_sha256": produced.outcome_sha,
        "protocol": COMMAND_PROTOCOL
    }))
    .expect("review command")
}

fn object_map(entries: &[(&str, Value)]) -> Value {
    let mut map = Map::new();
    for (key, value) in entries {
        map.insert((*key).to_owned(), value.clone());
    }
    Value::Object(map)
}

fn review_statements() -> Value {
    json!({
        "authentication": "not_claimed",
        "auto_accept": "not_authorized",
        "column_acl": "not_changed",
        "confidence_effect": "recorded_only",
        "database": "not_used",
        "effective": "not_written",
        "entity": "not_changed",
        "files": "not_written",
        "grants": "not_authorized",
        "metric": "not_changed",
        "owner_ui": "not_claimed",
        "pii_classification": "not_performed",
        "publication": "unavailable",
        "reviewer": "declared_only",
        "semantic_classification": "not_performed",
        "source_artifact": "not_mutated",
        "status": "not_changed"
    })
}

fn apply_ok(command: &str) -> Value {
    let saved = command.to_owned();
    let text = apply_description_review(command).unwrap_or_else(|err| {
        panic!("review failed: {} {}", err.code, err.message);
    });
    assert_eq!(command, saved, "command string");
    assert!(!text.ends_with('\n'), "added newline");
    let value: Value = serde_json::from_str(&text).expect("review json");
    assert_eq!(text, canonical_text(&value));
    let mut stripped = value.clone();
    stripped
        .as_object_mut()
        .expect("object")
        .remove("review_sha256");
    assert_eq!(
        value["review_sha256"].as_str().expect("review hash"),
        canonical_sha(&stripped)
    );
    assert_eq!(value["protocol"], REVIEW_PROTOCOL);
    assert_eq!(value["compiler"], REVIEW_COMPILER);
    assert_eq!(value["statements"], review_statements());
    assert_eq!(value["statements"]["database"], "not_used");
    assert_eq!(value["statements"]["publication"], "unavailable");
    assert_eq!(value["statements"]["authentication"], "not_claimed");
    assert_eq!(value["statements"]["files"], "not_written");
    assert_eq!(value["compiler_statements"]["curated"], "not_written");
    assert_eq!(value["compiler_statements"]["publication"], "unavailable");
    assert_eq!(value["compiler_statements"]["llm_transport"], "not_used");
    assert!(value["compiler_statements"].get("database").is_none());
    assert!(value["compiler_statements"].get("authentication").is_none());
    assert!(value["statements"].get("curated").is_none());
    assert!(value["statements"].get("llm_transport").is_none());
    if let Some(rows) = value["decisions"].as_array() {
        for row in rows {
            assert_eq!(row["authenticated"], json!(false));
            assert_eq!(row["confidence_effect"], "recorded_only");
        }
    }
    value
}

fn dataset_item<'a>(review: &'a Value, name: &str) -> &'a Value {
    review["datasets"]
        .as_array()
        .expect("datasets")
        .iter()
        .find(|item| item["dataset"] == name)
        .unwrap_or_else(|| panic!("missing dataset {name}"))
}

fn decision_row<'a>(review: &'a Value, dataset: &str) -> &'a Value {
    review["decisions"]
        .as_array()
        .expect("decisions")
        .iter()
        .find(|item| item["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing decision {dataset}"))
}

fn assert_core(document: &Value) {
    let file: DatasetFile = serde_json::from_value(document.clone()).expect("parse merged");
    assert!(validate_dataset(&file).is_empty(), "validate_dataset");
    assert_eq!(serde_json::to_value(&file).expect("serialize"), *document);
}

fn queue_item(dataset: &str, kind: &str, reason: &str) -> Value {
    json!({"dataset": dataset, "kind": kind, "reason": reason})
}

fn repeat_hash(ch: char) -> String {
    ch.to_string().repeat(64)
}

fn preserved_columns(document: &Value, dataset: &str) {
    let (schema, table) = dataset.split_once('.').expect("dataset");
    assert_eq!(document["columns"][0]["pii"], "none");
    assert_eq!(document["columns"][0]["semantic_type"], "identifier");
    assert_eq!(document["columns"][1]["pii"], "personal");
    assert_eq!(document["columns"][1]["semantic_type"], "phone");
    assert_eq!(document["columns"][1]["classification"]["state"], "confirmed");
    assert_eq!(document["columns"][1]["classification"]["by"], "ops.mina");
    assert_eq!(document["columns"][1]["status"], "deprecated");
    assert_eq!(
        document["columns"][1]["enum"],
        format!("{schema}.{table}.code")
    );
    assert_eq!(document["columns"][1]["description"], "sealed phone note");
    assert_eq!(document["caveats"], json!(["sealed curated caveat"]));
    assert_eq!(document["entity"], "customer");
    assert_eq!(document["tier"], "raw");
    assert_eq!(document["row_count_estimate"], 17);
    assert!(document.get("open_questions").is_none());
    assert!(document.get("confidence").is_none());
    assert!(document.get("classification").is_none());
    assert!(document.get("pii").is_none());
    assert!(document.get("status").is_none());
}

fn one_field() -> OneField {
    let replacement = "sealed replacement for one field";
    let current_description = "sealed current description";
    let produced = produce(json!([
        candidate(
            LOWER,
            "Candidate label must stay off the file",
            replacement,
            "candidate grain must stay off",
            "0",
            json!(["generated sealed caveat"]),
            json!(["which grain was sealed?"])
        ),
        candidate(
            UPPER,
            "Upper candidate must stay off",
            "upper candidate description",
            "upper candidate grain",
            "1",
            json!([]),
            json!([])
        )
    ]));
    assert_eq!(produced.outcome["outcome"], "candidates");
    assert_eq!(produced.outcome["selected_datasets"], json!([LOWER, UPPER]));
    assert!(produced.outcome["candidates"].as_array().unwrap().iter().all(|item| item["effect"] == "review_only"));
    assert_eq!(produced.outcome["statements"]["curated"], "not_written");
    let lower = curated_document(
        LOWER,
        "Sealed current label",
        Some(current_description),
        Some("sealed current grain"),
    );
    let upper = curated_document(
        UPPER,
        "Sealed upper label",
        Some("sealed upper description"),
        Some("sealed upper grain"),
    );
    let aside = curated_document(ASIDE, "Aside sealed label", Some("aside description"), Some("aside grain"));
    assert_eq!(
        field_sha(Some(current_description)),
        descriptive_field_sha256(Some(current_description))
    );
    let lower_anchors = anchors_for(&produced, LOWER, &canonical_sha(&lower));
    let upper_anchors = anchors_for(&produced, UPPER, &canonical_sha(&upper));
    let fields = json!({
        "description": {"overwrite": true, "current_sha256": field_sha(Some(current_description))},
        "label": {"overwrite": false}
    });
    let command = review_text(
        &produced,
        &world().schema_hash,
        &world().evidence_hash,
        object_map(&[
            (LOWER, lower.clone()),
            (UPPER, upper.clone()),
            (ASIDE, aside.clone()),
        ]),
        vec![
            decision_json(LOWER, "approve", &lower_anchors, Some(fields)),
            decision_json(UPPER, "reject", &upper_anchors, None),
        ],
    );
    let mut expected_lower = lower.clone();
    expected_lower["description"] = json!(replacement);
    OneField {
        command,
        outcome_sha: produced.outcome_sha,
        outcome_statements: produced.outcome["statements"].clone(),
        expected_lower,
        upper,
        aside,
    }
}

#[test]
fn sealed_reexecute_overwrites_one_field_and_reject_keeps_confidence() {
    assert_eq!(ident("A"), "q_41");
    assert_eq!(ident("b"), "b");
    assert_eq!(CAPTURED_AT.len(), 27);
    assert_ne!(world().schema_hash, repeat_hash('a'));
    let case = one_field();
    let review = apply_ok(&case.command);
    assert_eq!(review["outcome"], "reviewed");
    assert_eq!(review["outcome_sha256"], case.outcome_sha);
    assert_eq!(review["compiler_outcome"], "candidates");
    assert_eq!(review["compiler_statements"], case.outcome_statements);
    assert_eq!(review["compiler_statements"]["curated"], "not_written");
    assert_eq!(review["compiler_statements"]["auto_accept"], "not_authorized");
    assert_eq!(review["compiler_statements"]["confidence_effect"], "recorded_only");

    let lower_item = dataset_item(&review, LOWER);
    assert_eq!(lower_item["changed"], json!(true));
    assert_eq!(lower_item["document"], case.expected_lower);
    assert_eq!(
        lower_item["document_sha256"],
        canonical_sha(&case.expected_lower)
    );
    assert_core(&lower_item["document"]);
    preserved_columns(&lower_item["document"], LOWER);
    assert_eq!(lower_item["document"]["label"], "Sealed current label");
    assert_eq!(lower_item["document"]["grain"], "sealed current grain");
    assert_eq!(
        lower_item["document"]["description"],
        "sealed replacement for one field"
    );
    assert!(!lower_item["document"]
        .to_string()
        .contains("must stay off"));
    assert!(!lower_item["document"]
        .to_string()
        .contains("generated sealed caveat"));

    let upper_item = dataset_item(&review, UPPER);
    assert_eq!(upper_item["changed"], json!(false));
    assert_eq!(upper_item["document"], case.upper);
    assert_eq!(
        upper_item["document_sha256"],
        canonical_sha(&case.upper)
    );
    preserved_columns(&upper_item["document"], UPPER);
    assert!(!upper_item["document"]
        .to_string()
        .contains("Upper candidate"));
    let aside_item = dataset_item(&review, ASIDE);
    assert_eq!(aside_item["changed"], json!(false));
    assert_eq!(aside_item["document"], case.aside);

    let approved = decision_row(&review, LOWER);
    assert_eq!(approved["applied"], json!(true));
    assert_eq!(approved["authenticated"], json!(false));
    assert_eq!(approved["candidate_confidence"], "0");
    assert_eq!(approved["effect"], "review_only");
    assert_eq!(approved["label_disposition"], "kept");
    assert_eq!(approved["description_disposition"], "replaced");
    assert_eq!(approved["grain_disposition"], "not_selected");
    assert_eq!(approved["candidate_caveats"], json!(["generated sealed caveat"]));
    assert_eq!(
        approved["candidate_open_questions"],
        json!(["which grain was sealed?"])
    );
    let rejected = decision_row(&review, UPPER);
    assert_eq!(rejected["decision"], "reject");
    assert_eq!(rejected["applied"], json!(false));
    assert_eq!(rejected["authenticated"], json!(false));
    assert_eq!(rejected["candidate_confidence"], "1");
    assert_eq!(rejected["label_disposition"], "unchanged");
    assert_eq!(rejected["description_disposition"], "unchanged");
    assert_eq!(rejected["grain_disposition"], "unchanged");
    assert_eq!(rejected["effect"], "review_only");
    assert_eq!(
        review["queue"],
        json!([
            queue_item(LOWER, "pending", "caveats"),
            queue_item(LOWER, "pending", "open_questions")
        ])
    );
}

#[test]
fn defer_and_missing_decision_preserve_current_curated() {
    let produced = produce(json!([
        candidate(LOWER, "Missing decision candidate text", "missing desc", "missing grain", "0", json!([]), json!([])),
        candidate(UPPER, "Deferred candidate text", "defer desc", "defer grain", "1", json!([]), json!([]))
    ]));
    let lower = curated_document(LOWER, "Keep lower label", Some("keep lower desc"), Some("keep lower grain"));
    let upper = curated_document(UPPER, "Keep upper label", Some("keep upper desc"), Some("keep upper grain"));
    let aside = curated_document(ASIDE, "Keep aside label", Some("keep aside desc"), None);
    let upper_anchors = anchors_for(&produced, UPPER, &canonical_sha(&upper));
    let command = review_text(
        &produced,
        &world().schema_hash,
        &world().evidence_hash,
        object_map(&[(LOWER, lower.clone()), (UPPER, upper.clone()), (ASIDE, aside.clone())]),
        vec![decision_json(UPPER, "defer", &upper_anchors, None)],
    );
    let review = apply_ok(&command);
    assert_eq!(dataset_item(&review, LOWER)["document"], lower);
    assert_eq!(dataset_item(&review, LOWER)["changed"], json!(false));
    assert_eq!(dataset_item(&review, UPPER)["document"], upper);
    assert_eq!(dataset_item(&review, UPPER)["changed"], json!(false));
    assert_eq!(dataset_item(&review, ASIDE)["document"], aside);
    preserved_columns(&dataset_item(&review, LOWER)["document"], LOWER);
    preserved_columns(&dataset_item(&review, UPPER)["document"], UPPER);
    let deferred = decision_row(&review, UPPER);
    assert_eq!(deferred["decision"], "defer");
    assert_eq!(deferred["applied"], json!(false));
    assert_eq!(deferred["candidate_confidence"], "1");
    assert_eq!(deferred["label_disposition"], "unchanged");
    assert_eq!(review["decisions"].as_array().unwrap().len(), 1);
    assert_eq!(deferred["candidate_label"], "Deferred candidate text");
    assert!(!dataset_item(&review, LOWER)["document"]
        .to_string()
        .contains("Missing decision candidate text"));
    assert!(!dataset_item(&review, UPPER)["document"]
        .to_string()
        .contains("Deferred candidate text"));
    assert!(!review.to_string().contains("Missing decision candidate text"));
    assert_eq!(
        review["queue"],
        json!([
            queue_item(LOWER, "pending", "decision_missing"),
            queue_item(UPPER, "pending", "deferred")
        ])
    );
}

#[test]
fn stale_schema_evidence_card_request_curated_and_candidate_do_not_mutate() {
    let produced = produce(json!([
        candidate(LOWER, "Fresh lower label", "fresh lower desc", "fresh lower grain", "0", json!([]), json!([])),
        candidate(UPPER, "Fresh upper label", "fresh upper desc", "fresh upper grain", "0", json!([]), json!([]))
    ]));
    let lower = curated_document(LOWER, "Stale lower label", Some("stale lower desc"), Some("stale lower grain"));
    let upper = curated_document(UPPER, "Stale upper label", Some("stale upper desc"), Some("stale upper grain"));
    let lower_hash = canonical_sha(&lower);
    let upper_hash = canonical_sha(&upper);
    let lower_anchors = anchors_for(&produced, LOWER, &lower_hash);
    let upper_anchors = anchors_for(&produced, UPPER, &upper_hash);
    let curated = object_map(&[(LOWER, lower.clone()), (UPPER, upper.clone())]);
    let approve_both = vec![
        decision_json(
            LOWER,
            "approve",
            &lower_anchors,
            Some(json!({"label": {"overwrite": true, "current_sha256": field_sha(Some("Stale lower label"))}})),
        ),
        decision_json(
            UPPER,
            "approve",
            &upper_anchors,
            Some(json!({"label": {"overwrite": true, "current_sha256": field_sha(Some("Stale upper label"))}})),
        ),
    ];
    let schema = review_text(
        &produced,
        &repeat_hash('a'),
        &world().evidence_hash,
        curated.clone(),
        approve_both.clone(),
    );
    let review = apply_ok(&schema);
    assert_eq!(dataset_item(&review, LOWER)["document"], lower);
    assert_eq!(dataset_item(&review, UPPER)["document"], upper);
    assert_eq!(dataset_item(&review, LOWER)["changed"], json!(false));
    assert_eq!(dataset_item(&review, UPPER)["changed"], json!(false));
    assert_eq!(
        review["queue"],
        json!([
            queue_item(LOWER, "stale", "schema"),
            queue_item(UPPER, "stale", "schema")
        ])
    );

    let evidence = review_text(
        &produced,
        &world().schema_hash,
        &repeat_hash('b'),
        curated.clone(),
        approve_both,
    );
    let review = apply_ok(&evidence);
    assert_eq!(dataset_item(&review, LOWER)["document"], lower);
    assert_eq!(dataset_item(&review, UPPER)["document"], upper);
    assert_eq!(
        review["queue"],
        json!([
            queue_item(LOWER, "stale", "evidence"),
            queue_item(UPPER, "stale", "evidence")
        ])
    );

    let approve_upper = decision_json(
        UPPER,
        "approve",
        &upper_anchors,
        Some(json!({"label": {"overwrite": true, "current_sha256": field_sha(Some("Stale upper label"))}})),
    );
    let mut request_anchors = anchors_for(&produced, LOWER, &lower_hash);
    request_anchors.request = repeat_hash('c');
    assert_one_anchor(&produced, &lower, &upper, &curated, request_anchors, &approve_upper, "request");

    let mut card_anchors = anchors_for(&produced, LOWER, &lower_hash);
    card_anchors.card = repeat_hash('d');
    assert_one_anchor(&produced, &lower, &upper, &curated, card_anchors, &approve_upper, "card");

    let mut curated_anchors = anchors_for(&produced, LOWER, &lower_hash);
    curated_anchors.curated = repeat_hash('e');
    assert_one_anchor(&produced, &lower, &upper, &curated, curated_anchors, &approve_upper, "curated");

    let mut candidate_anchors = anchors_for(&produced, LOWER, &lower_hash);
    candidate_anchors.candidate = repeat_hash('f');
    assert_one_anchor(
        &produced,
        &lower,
        &upper,
        &curated,
        candidate_anchors,
        &approve_upper,
        "candidate",
    );
}

fn assert_one_anchor(
    produced: &Produced,
    lower: &Value,
    upper: &Value,
    curated: &Value,
    anchors: Anchors,
    approve_upper: &Value,
    reason: &str,
) {
    let command = review_text(
        produced,
        &world().schema_hash,
        &world().evidence_hash,
        curated.clone(),
        vec![
            decision_json(
                LOWER,
                "approve",
                &anchors,
                Some(json!({"label": {"overwrite": true, "current_sha256": field_sha(Some("Stale lower label"))}})),
            ),
            approve_upper.clone(),
        ],
    );
    let review = apply_ok(&command);
    assert_eq!(dataset_item(&review, LOWER)["document"], *lower);
    assert_eq!(dataset_item(&review, LOWER)["changed"], json!(false));
    preserved_columns(&dataset_item(&review, LOWER)["document"], LOWER);
    assert_eq!(dataset_item(&review, UPPER)["document"]["label"], "Fresh upper label");
    assert_eq!(
        dataset_item(&review, UPPER)["document"]["description"],
        "stale upper desc"
    );
    assert_eq!(
        dataset_item(&review, UPPER)["document"]["grain"],
        "stale upper grain"
    );
    preserved_columns(&dataset_item(&review, UPPER)["document"], UPPER);
    assert_eq!(dataset_item(&review, UPPER)["document"]["caveats"], upper["caveats"]);
    assert!(!dataset_item(&review, LOWER)["document"]
        .to_string()
        .contains("Fresh lower"));
    assert_eq!(review["queue"], json!([queue_item(LOWER, "stale", reason)]));
}

#[test]
fn unknown_classification_and_status_keys_are_refused() {
    let produced = produce(json!([
        candidate(LOWER, "Plain lower", "plain desc", "plain grain", "0", json!([]), json!([])),
        candidate(UPPER, "Plain upper", "plain upper", "plain grain", "0", json!([]), json!([]))
    ]));
    let lower = curated_document(LOWER, "Plain curated", Some("plain curated desc"), Some("plain grain"));
    let command = review_text(
        &produced,
        &world().schema_hash,
        &world().evidence_hash,
        object_map(&[(LOWER, lower.clone())]),
        Vec::new(),
    );
    let mut value: Value = serde_json::from_str(&command).unwrap();
    value["classification"] = json!(TOKEN);
    value["status"] = json!(TOKEN);
    let err = apply_description_review(&serde_json::to_string(&value).unwrap()).expect_err("schema");
    let shown = format!("{} {}", err.code, err.message);
    assert!(!shown.contains(TOKEN), "{shown}");
    assert!(err.code.ends_with("/schema.invalid"), "{}", err.code);

    let mut poisoned = candidate(LOWER, "Plain lower", "plain desc", "plain grain", "0", json!([]), json!([]));
    poisoned["classification"] = json!(TOKEN);
    poisoned["status"] = json!(TOKEN);
    let other = candidate(UPPER, "Plain upper", "plain upper", "plain grain", "0", json!([]), json!([]));
    let produced = produce(json!([poisoned, other]));
    assert_eq!(produced.outcome["outcome"], "format_rejected");
    assert!(produced.command_text.contains(TOKEN));
    assert_eq!(produced.outcome["statements"]["curated"], "not_written");
    let command = review_text(
        &produced,
        &world().schema_hash,
        &world().evidence_hash,
        object_map(&[(LOWER, lower.clone())]),
        Vec::new(),
    );
    let review = apply_ok(&command);
    assert!(!serde_json::to_string(&review).unwrap().contains(TOKEN));
    assert_eq!(review["outcome"], "not_candidates");
    assert_eq!(review["compiler_outcome"], "format_rejected");
    assert_eq!(dataset_item(&review, LOWER)["document"], lower);
    assert_eq!(dataset_item(&review, LOWER)["changed"], json!(false));
    assert_eq!(
        review["queue"],
        json!([queue_item("", "compiler", "format_rejected")])
    );
}

#[test]
fn bounds_apply_whole_and_are_not_truncated() {
    let label = "N".repeat(256);
    let description = "M".repeat(4_000);
    let grain = "R".repeat(256);
    let produced = produce(json!([
        candidate(LOWER, &label, &description, &grain, "0", json!([]), json!([])),
        candidate(UPPER, "Short upper", "short upper desc", "short upper grain", "0", json!([]), json!([]))
    ]));
    assert_eq!(produced.outcome["outcome"], "candidates");
    let lower = curated_document(LOWER, "Short lower", Some("short lower desc"), Some("short lower grain"));
    let upper = curated_document(UPPER, "Short upper curated", Some("short upper curated"), Some("short upper grain curated"));
    let lower_anchors = anchors_for(&produced, LOWER, &canonical_sha(&lower));
    let upper_anchors = anchors_for(&produced, UPPER, &canonical_sha(&upper));
    let fields = json!({
        "label": {"overwrite": true, "current_sha256": field_sha(Some("Short lower"))},
        "description": {"overwrite": true, "current_sha256": field_sha(Some("short lower desc"))},
        "grain": {"overwrite": true, "current_sha256": field_sha(Some("short lower grain"))}
    });
    let command = review_text(
        &produced,
        &world().schema_hash,
        &world().evidence_hash,
        object_map(&[(LOWER, lower), (UPPER, upper.clone())]),
        vec![
            decision_json(LOWER, "approve", &lower_anchors, Some(fields)),
            decision_json(UPPER, "reject", &upper_anchors, None),
        ],
    );
    let review = apply_ok(&command);
    let document = &dataset_item(&review, LOWER)["document"];
    assert_eq!(document["label"].as_str().unwrap().chars().count(), 256);
    assert_eq!(document["description"].as_str().unwrap().chars().count(), 4_000);
    assert_eq!(document["grain"].as_str().unwrap().chars().count(), 256);
    assert_eq!(document["label"], label);
    assert_eq!(document["description"], description);
    assert_eq!(document["grain"], grain);
    assert_core(document);
    preserved_columns(document, LOWER);
    assert_eq!(dataset_item(&review, UPPER)["document"], upper);

    let token = format!("A288LONG{}", "x".repeat(257 - "A288LONG".len()));
    assert_eq!(token.chars().count(), 257);
    let produced = produce(json!([
        candidate(LOWER, &token, "desc", "grain", "0", json!([]), json!([])),
        candidate(UPPER, "Short upper", "short upper desc", "short upper grain", "0", json!([]), json!([]))
    ]));
    assert_eq!(produced.outcome["outcome"], "format_rejected");
    assert!(produced.command_text.contains(&token));
    let lower = curated_document(LOWER, "Short lower", Some("short lower desc"), Some("short lower grain"));
    let command = review_text(
        &produced,
        &world().schema_hash,
        &world().evidence_hash,
        object_map(&[(LOWER, lower.clone())]),
        Vec::new(),
    );
    let review = apply_ok(&command);
    let dumped = serde_json::to_string(&review).unwrap();
    assert!(!dumped.contains("A288LONG"));
    assert!(!dumped.contains(&"N".repeat(256)));
    assert_eq!(dataset_item(&review, LOWER)["document"]["label"], "Short lower");
    assert_eq!(dataset_item(&review, LOWER)["document"], lower);
    assert_eq!(review["outcome"], "not_candidates");
    assert_eq!(review["compiler_outcome"], "format_rejected");

    let control = "A288_CTRL\u{0001}tail";
    let produced = produce(json!([
        candidate(LOWER, "Short label", control, "short grain", "0", json!([]), json!([])),
        candidate(UPPER, "Short upper", "short upper desc", "short upper grain", "0", json!([]), json!([]))
    ]));
    assert_eq!(produced.outcome["outcome"], "format_rejected");
    assert_eq!(produced.outcome["refusal"], "type");
    let lower = curated_document(LOWER, "Short lower", Some("short lower desc"), Some("short lower grain"));
    let command = review_text(
        &produced,
        &world().schema_hash,
        &world().evidence_hash,
        object_map(&[(LOWER, lower.clone())]),
        Vec::new(),
    );
    let review = apply_ok(&command);
    assert!(!serde_json::to_string(&review).unwrap().contains("A288_CTRL"));
    assert_eq!(dataset_item(&review, LOWER)["document"], lower);
    assert_eq!(
        dataset_item(&review, LOWER)["document"]["description"],
        "short lower desc"
    );
    assert_eq!(review["compiler_outcome"], "format_rejected");
    assert_eq!(review["outcome"], "not_candidates");
}

#[test]
fn compiled_cli_equals_the_library_document() {
    let case = one_field();
    let library = apply_description_review(&case.command).expect("library");
    let output = run_cli(&["apply"], case.command.as_bytes());
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.ends_with('\n'));
    assert_eq!(stdout, library);
    let review: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(review["outcome_sha256"], case.outcome_sha);
    assert_eq!(review["compiler_statements"], case.outcome_statements);
    assert_eq!(review["statements"], review_statements());
    assert_eq!(dataset_item(&review, LOWER)["document"], case.expected_lower);
    assert_eq!(dataset_item(&review, UPPER)["document"], case.upper);
    assert_eq!(decision_row(&review, UPPER)["candidate_confidence"], "1");
    assert_eq!(decision_row(&review, UPPER)["applied"], json!(false));
}

fn run_cli(args: &[&str], stdin: &[u8]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_description-review"));
    command
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env_remove("RUST_BACKTRACE");
    let mut child = command.spawn().expect("spawn");
    use std::io::Write;
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(stdin)
        .expect("write stdin");
    drop(child.stdin.take());
    child.wait_with_output().expect("wait")
}
