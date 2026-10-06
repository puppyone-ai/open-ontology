//! Independent description-pack oracle for review a302.
//!
//! Two catalog datasets, one extra curated file, and two roles. Constants are
//! this review's. The schema preimage follows `pg-catalog-hash/v1` in
//! `crates/oo-builder/src/pgsql.rs`. Hashes use `payload_sha256` and the
//! public description-review field helpers. This file does not open a
//! database, grant, or call a model.

#![forbid(unsafe_code)]

use std::sync::OnceLock;

use description_pack::{COMMAND_PROTOCOL, PACK_COMPILER, PACK_PROTOCOL, apply_description_pack};
use description_review::{
    apply_description_review, curated_dataset_sha256, descriptive_field_sha256,
};
use oo_builder::{
    DESCRIPTION_PROMPT_VERSION, DESCRIPTION_REQUEST_COMMAND_PROTOCOL, DESCRIPTION_RESPONSE_PROTOCOL,
    REVIEWED_EFFECTIVE_COMMAND_PROTOCOL, compile_captured_pg_catalog, compile_reviewed_effective,
    compile_role_pack, complete_description_candidates, prepare_description_request,
};
use oo_core::api::payload_sha256;
use oo_core::oos::{DatasetFile, validate_dataset};
use serde_json::{Map, Value, json};

const CAPTURED_AT: &str = "2026-04-11T09:30:00.000000Z";
const OWNER: &str = "tbl_owner";
const SOURCE: &str = "a302_src";
const DATABASE: &str = "a302db";
const MODEL: &str = "a302-sealed";
const INVOICE: &str = "public.invoice";
const CRATE: &str = "public.crate";
const SHELF: &str = "public.shelf";
const CURRENT_LABEL: &str = "Invoice book";
const LABEL: &str = "Invoice register a302";
const GRAIN: &str = "one invoice line a302";
const CANDIDATE_GRAIN: &str = "CANDIDATE_GRAIN_a302_DO_NOT_KEEP";
const DESCRIPTION: &str = "The badge_code column stores a bearer the gate must not read.";
const CRATE_LABEL: &str = "CRATE_CANDIDATE_a302_DO_NOT_APPLY";
const CRATE_BODY: &str = "crate prose a302 stays out";
const INCOMPLETE_BODY: &str = "INCOMPLETE_BODY_a302_DO_NOT_LEAK";
const ENV_CANARY: &str = "ENV_CANARY_a302_DO_NOT_LEAK";
const REVIEWER: &str = "mina.declared";
const REVIEWED_ON: &str = "2026-10-06";
const PROJECT: &str = "a302pack";
const ONTOLOGY: &str = "ont-a302";
const CATALOG_VERSION: &str = "cat-a302";
const UNREVIEWED_FENCE: &str = "> UNREVIEWED: not an approved review, and not a grant.";

struct Col {
    attnum: i32,
    name: &'static str,
    type_oid: i64,
    not_null: bool,
}

struct Ready {
    wrapper_hash: String,
    artifact: Value,
    draft: Value,
    command_text: String,
    outcome_sha: String,
    schema_hash: String,
    evidence_hash: String,
}

fn ready() -> &'static Ready {
    static READY: OnceLock<Ready> = OnceLock::new();
    READY.get_or_init(|| seal(None))
}

fn seal(reviews: Option<&str>) -> Ready {
    let compiled = compile_captured_pg_catalog(&catalog_json(), Some(&overlay_json()), reviews)
        .unwrap_or_else(|err| panic!("catalog compile failed: {} {}", err.code, err.message));
    let artifact = artifact_value(&compiled);
    let command = candidate_command(&compiled, &artifact);
    let command_text = serde_json::to_string(&command).expect("candidate command");
    let request_text = prepare_description_request(&command_text)
        .unwrap_or_else(|err| panic!("prepare failed: {} {}", err.code, err.message));
    let request: Value = serde_json::from_str(&request_text).expect("request");
    let produced = finish_candidates(&command, &request, candidates(&request));
    let again = complete_description_candidates(&produced.0)
        .unwrap_or_else(|err| panic!("second complete failed: {} {}", err.code, err.message));
    let first = complete_description_candidates(&produced.0)
        .unwrap_or_else(|err| panic!("complete reread failed: {} {}", err.code, err.message));
    assert_eq!(first, again, "candidate compiler bytes moved");
    assert_eq!(payload_sha256(first.as_bytes()), produced.1);
    let draft: Value = serde_json::from_str(&compiled.draft).expect("draft");
    Ready {
        wrapper_hash: compiled.wrapper_hash,
        artifact,
        draft,
        command_text: produced.0,
        outcome_sha: produced.1,
        schema_hash: compiled.schema_hash,
        evidence_hash: compiled.evidence_hash,
    }
}

fn candidates(request: &Value) -> Value {
    json!([
        candidate(request, CRATE, CRATE_LABEL, CRATE_BODY, "crate grain stays out"),
        candidate(request, INVOICE, LABEL, DESCRIPTION, CANDIDATE_GRAIN),
    ])
}

fn finish_candidates(command: &Value, request: &Value, candidates: Value) -> (String, String) {
    let body = serde_json::to_string(&json!({
        "budget_sha256": request["budget_sha256"],
        "candidates": candidates,
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
    .expect("response");
    let mut command = command.clone();
    command["captured_response"] = json!({
        "body": body,
        "byte_len": body.len(),
        "status": "complete"
    });
    let command_text = serde_json::to_string(&command).expect("complete command");
    let outcome_text = complete_description_candidates(&command_text)
        .unwrap_or_else(|err| panic!("complete failed: {} {}", err.code, err.message));
    (command_text, payload_sha256(outcome_text.as_bytes()))
}

fn outcome_of(command_text: &str) -> Value {
    let outcome_text = complete_description_candidates(command_text)
        .unwrap_or_else(|err| panic!("complete failed: {} {}", err.code, err.message));
    serde_json::from_str(&outcome_text).expect("outcome")
}

fn invoice_columns() -> Vec<Col> {
    vec![
        Col { attnum: 1, name: "id", type_oid: 23, not_null: true },
        Col { attnum: 2, name: "qty", type_oid: 23, not_null: false },
        Col { attnum: 3, name: "badge_code", type_oid: 25, not_null: false },
    ]
}

fn crate_columns() -> Vec<Col> {
    vec![
        Col { attnum: 1, name: "id", type_oid: 23, not_null: true },
        Col { attnum: 2, name: "seal_note", type_oid: 25, not_null: false },
    ]
}

fn schema_preimage() -> String {
    let mut tables = [("crate", crate_columns()), ("invoice", invoice_columns())];
    tables.sort_by(|left, right| left.0.cmp(right.0));
    let mut buf = String::from("{\"tables\":[");
    for (index, (name, columns)) in tables.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        write_table(&mut buf, name, columns);
    }
    buf.push_str("]}");
    buf
}

fn write_table(buf: &mut String, name: &str, columns: &[Col]) {
    buf.push('{');
    write_str(buf, "schema", "public");
    buf.push(',');
    write_str(buf, "name", name);
    buf.push(',');
    write_str(buf, "relkind", "r");
    buf.push(',');
    write_str(buf, "replica_identity", "d");
    buf.push(',');
    write_bool(buf, "rls", false);
    buf.push(',');
    write_bool(buf, "force_rls", false);
    buf.push_str(",\"columns\":[");
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        write_column(buf, column);
    }
    buf.push_str("],\"primary_key\":[{");
    write_i32(buf, "attnum", 1);
    buf.push(',');
    write_str(buf, "name", "id");
    buf.push_str(
        "}],\"primary_key_nulls_not_distinct\":false,\"unique_keys\":[],\"foreign_keys\":[],\"policies\":[]}",
    );
}

fn write_column(buf: &mut String, column: &Col) {
    buf.push('{');
    write_i32(buf, "attnum", column.attnum);
    buf.push(',');
    write_str(buf, "name", column.name);
    buf.push(',');
    write_i64(buf, "type_oid", column.type_oid);
    buf.push(',');
    write_i32(buf, "typmod", -1);
    buf.push(',');
    write_bool(buf, "not_null", column.not_null);
    buf.push(',');
    write_i64(buf, "collation_oid", 0);
    buf.push(',');
    write_str(buf, "typtype", "b");
    buf.push(',');
    push_preimage(buf, "base_type_oid");
    buf.push_str(":null,");
    write_str(buf, "identity", "");
    buf.push(',');
    write_str(buf, "generated", "");
    buf.push(',');
    push_preimage(buf, "default_sha256");
    buf.push_str(":null,\"enum_labels\":[],\"domain_checks\":[]}");
}

fn write_str(buf: &mut String, name: &str, value: &str) {
    push_preimage(buf, name);
    buf.push(':');
    push_preimage(buf, value);
}

fn write_bool(buf: &mut String, name: &str, value: bool) {
    push_preimage(buf, name);
    buf.push(':');
    buf.push_str(if value { "true" } else { "false" });
}

fn write_i32(buf: &mut String, name: &str, value: i32) {
    push_preimage(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn write_i64(buf: &mut String, name: &str, value: i64) {
    push_preimage(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn push_preimage(buf: &mut String, value: &str) {
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

fn catalog_json() -> String {
    serde_json::to_string(&json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": SOURCE,
        "database": DATABASE,
        "captured_at": CAPTURED_AT,
        "schema_hash": payload_sha256(schema_preimage().as_bytes()),
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": [table("crate", crate_columns()), table("invoice", invoice_columns())],
        "type_conversion": "unsupported"
    }))
    .expect("catalog")
}

fn table(name: &str, columns: Vec<Col>) -> Value {
    json!({
        "schema": "public",
        "schema_ident": "public",
        "name": name,
        "name_ident": name,
        "relkind": "r",
        "replica_identity": "d",
        "rls_enabled": false,
        "rls_forced": false,
        "owner": OWNER,
        "comment": Value::Null,
        "columns": columns.iter().map(column_json).collect::<Vec<_>>(),
        "primary_key": [{"attnum": 1, "name": "id"}],
        "primary_key_nulls_not_distinct": false,
        "unique_keys": [],
        "foreign_keys": [],
        "policies": [],
        "privileges": [{"grantee": OWNER, "privilege": "SELECT", "grantable": false}]
    })
}

fn column_json(column: &Col) -> Value {
    let type_name = if column.type_oid == 23 { "int4" } else { "text" };
    json!({
        "attnum": column.attnum,
        "name": column.name,
        "name_ident": column.name,
        "type_oid": column.type_oid,
        "type_schema": "pg_catalog",
        "type_name": type_name,
        "typtype": "b",
        "typmod": -1,
        "base_type_oid": Value::Null,
        "numeric_precision": Value::Null,
        "numeric_scale": Value::Null,
        "not_null": column.not_null,
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

fn overlay_json() -> String {
    serde_json::to_string(&json!({
        "datasets": [{
            "dataset": INVOICE,
            "entity": "shipment",
            "columns": [{
                "name": "qty",
                "pii": "sensitive",
                "semantic_type": "free_text",
                "status": "draft"
            }]
        }],
        "relationships": []
    }))
    .expect("overlay")
}

fn artifact_value(compiled: &oo_builder::CapturedPgDraft) -> Value {
    json!({
        "draft": serde_json::from_str::<Value>(&compiled.draft).expect("draft"),
        "evidence_envelope": serde_json::from_str::<Value>(&compiled.evidence_envelope).expect("envelope"),
        "protocol": "oo.builder.captured-pg-draft-command/v1",
        "wrapper": serde_json::from_str::<Value>(&compiled.wrapper).expect("wrapper")
    })
}

fn candidate_command(compiled: &oo_builder::CapturedPgDraft, artifact: &Value) -> Value {
    json!({
        "admitted": true,
        "artifact": artifact,
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
                visible("crate", &compiled.evidence_hash, &compiled.schema_hash),
                visible("invoice", &compiled.evidence_hash, &compiled.schema_hash)
            ]
        }
    })
}

fn visible(name: &str, evidence: &str, schema_hash: &str) -> Value {
    json!({
        "dataset": format!("public.{name}"),
        "evidence_hash": evidence,
        "name": name,
        "name_ident": name,
        "schema": "public",
        "schema_ident": "public",
        "schema_hash": schema_hash,
        "visibility": "visible"
    })
}

fn card_sha(request: &Value, dataset: &str) -> String {
    request["boundary"]["data"]["tables"]
        .as_array()
        .expect("cards")
        .iter()
        .find(|card| card["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing card {dataset}"))["card_sha256"]
        .as_str()
        .expect("card sha")
        .to_owned()
}

fn candidate(request: &Value, dataset: &str, label: &str, description: &str, grain: &str) -> Value {
    json!({
        "card_sha256": card_sha(request, dataset),
        "caveats": [],
        "confidence": "0",
        "dataset": dataset,
        "description": description,
        "grain": grain,
        "label": label,
        "open_questions": []
    })
}

fn invoice_file() -> Value {
    dataset_file(json!({
        "schema_version": "oos/v1",
        "dataset": INVOICE,
        "source_name": "src_invoice",
        "label": CURRENT_LABEL,
        "description": Value::Null,
        "tier": "raw",
        "recommended": false,
        "grain": GRAIN,
        "primary_key": ["id"],
        "entity": "shipment",
        "row_count_estimate": 18,
        "time_columns": [{"name": "id", "role": "created_at"}],
        "columns": [{
            "name": "qty",
            "label": "Quantity",
            "data_type": "int4",
            "semantic_type": "quantity",
            "pii": "personal",
            "classification": {"state": "confirmed", "by": "ops.lina", "at": "2026-10-06"},
            "status": "deprecated",
            "description": "curated qty note",
            "provenance": "human",
            "review": {"state": "approved", "by": "ada.curator", "at": "2026-10-01"}
        }],
        "caveats": ["kept invoice caveat"]
    }))
}

fn crate_file() -> Value {
    dataset_file(json!({
        "schema_version": "oos/v1",
        "dataset": CRATE,
        "label": "Crate book",
        "tier": "raw",
        "columns": [{
            "name": "id",
            "label": "Id",
            "data_type": "int4",
            "semantic_type": "identifier",
            "pii": "none"
        }]
    }))
}

fn shelf_file() -> Value {
    dataset_file(json!({
        "schema_version": "oos/v1",
        "dataset": SHELF,
        "source_name": "src_shelf",
        "label": "Shelf register",
        "description": "shelf stays outside the catalog seal",
        "tier": "raw",
        "grain": "one shelf row",
        "entity": "shipment",
        "row_count_estimate": 4,
        "columns": [{
            "name": "id",
            "label": "Id",
            "data_type": "int4",
            "semantic_type": "identifier",
            "pii": "restricted",
            "classification": {"state": "pending", "by": "ops.lina", "at": "2026-10-06"},
            "status": "deprecated"
        }],
        "caveats": ["shelf caveat"]
    }))
}

fn dataset_file(value: Value) -> Value {
    let file: DatasetFile = serde_json::from_value(value).unwrap_or_else(|err| {
        panic!("dataset file did not parse: {err}");
    });
    assert!(validate_dataset(&file).is_empty(), "dataset validation");
    let typed = serde_json::to_value(&file).expect("typed dataset");
    assert_eq!(canonical_sha(&typed), curated_dataset_sha256(&file));
    typed
}

fn curated_map() -> Value {
    json!({
        INVOICE: invoice_file(),
        CRATE: crate_file(),
        SHELF: shelf_file(),
    })
}

fn anchors(outcome: &Value, dataset: &str, curated_hash: &str) -> Value {
    let candidate = outcome["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .find(|item| item["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing outcome candidate {dataset}"));
    json!({
        "candidate": canonical_sha(candidate),
        "card": candidate["card_sha256"].as_str().expect("card"),
        "schema": outcome["structural_draft"]["schema_hash"].as_str().expect("schema"),
        "evidence": outcome["structural_draft"]["evidence_hash"].as_str().expect("evidence"),
        "request": outcome["request_sha256"].as_str().expect("request"),
        "curated": curated_hash,
    })
}

fn decision(dataset: &str, kind: &str, anchors: &Value, fields: Option<Value>) -> Value {
    let mut value = json!({
        "candidate_sha256": anchors["candidate"],
        "card_sha256": anchors["card"],
        "curated_sha256": anchors["curated"],
        "dataset": dataset,
        "decision": kind,
        "evidence_hash": anchors["evidence"],
        "request_sha256": anchors["request"],
        "reviewer_at": REVIEWED_ON,
        "reviewer_label": REVIEWER,
        "schema_hash": anchors["schema"]
    });
    if let Some(fields) = fields {
        value["fields"] = fields;
    }
    value
}

fn approve_fields() -> Value {
    let current = field_sha(Some(CURRENT_LABEL));
    json!({
        "label": {"overwrite": true, "current_sha256": current},
        "description": {"overwrite": false},
        "grain": {"overwrite": false}
    })
}

fn review_text(
    command_text: &str,
    outcome_sha: &str,
    schema: &str,
    evidence: &str,
    decisions: Vec<Value>,
) -> String {
    serde_json::to_string(&json!({
        "command": command_text,
        "current": {"evidence_hash": evidence, "schema_hash": schema},
        "curated": curated_map(),
        "decisions": decisions,
        "expected_outcome_sha256": outcome_sha,
        "protocol": description_review::COMMAND_PROTOCOL
    }))
    .expect("review")
}

fn happy_decisions(outcome: &Value) -> Vec<Value> {
    let invoice = anchors(outcome, INVOICE, &canonical_sha(&invoice_file()));
    let crate_row = anchors(outcome, CRATE, &canonical_sha(&crate_file()));
    vec![
        decision(INVOICE, "approve", &invoice, Some(approve_fields())),
        decision(CRATE, "reject", &crate_row, None),
    ]
}

fn class_decision(draft: &Value, dataset: &str, column: &str, pii: &str, semantic: &str) -> Value {
    json!({
        "dataset": dataset,
        "column": column,
        "schema_hash": draft["provenance"]["schema_hash"],
        "fact_fingerprint": column_fingerprint(draft, dataset, column),
        "pii": pii,
        "semantic_type": semantic,
        "reviewer": {"by": "mina.owner", "at": "2026-10-06"}
    })
}

fn column_fingerprint(draft: &Value, dataset: &str, column: &str) -> String {
    let item_id = format!("column/{dataset}/{column}");
    draft["review_queue"]
        .as_array()
        .expect("queue")
        .iter()
        .find(|item| item["item_id"] == item_id && item["kind"] == "structural_column")
        .unwrap_or_else(|| panic!("missing fingerprint {item_id}"))["fact_fingerprint"]
        .as_str()
        .expect("fingerprint")
        .to_owned()
}

fn classifications(draft: &Value) -> Vec<Value> {
    vec![
        class_decision(draft, INVOICE, "id", "none", "identifier"),
        class_decision(draft, INVOICE, "qty", "restricted", "quantity"),
        class_decision(draft, INVOICE, "badge_code", "restricted", "free_text"),
        class_decision(draft, CRATE, "id", "none", "identifier"),
    ]
}

fn settings() -> Value {
    json!({
        "project": PROJECT,
        "ontology_version": ONTOLOGY,
        "catalog_version": CATALOG_VERSION,
        "policy": {"id": "pol-a302", "version": "1"},
        "knowledge": {"id": "know-a302", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": PROJECT,
            "locale": "en-US",
            "include_schemas": ["public"],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": ["dataset_descriptions"]},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        }
    })
}

fn visibility(role: &str, principal: &str, datasets: Value) -> Value {
    json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": PROJECT,
        "principal": principal,
        "role": role,
        "ontology_version": ONTOLOGY,
        "catalog_version": CATALOG_VERSION,
        "policy": {"id": "pol-a302", "version": "1"},
        "knowledge": {"id": "know-a302", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": datasets,
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    })
}

fn grant(dataset: &str, columns: &[&str]) -> Value {
    json!({
        "dataset": dataset,
        "row_security": "none",
        "global_stats": "permitted",
        "columns": columns
    })
}

fn two_roles() -> Value {
    json!([
        visibility("oo_r_gate", "gate_user", json!([grant(INVOICE, &["id", "qty"])])),
        visibility(
            "oo_r_lane",
            "lane_user",
            json!([
                grant(INVOICE, &["id", "qty", "badge_code"]),
                grant(CRATE, &["id"])
            ])
        )
    ])
}

fn pack_text(review: &str, artifact: &Value, decisions: Value, roles: Value) -> String {
    let review_bytes = apply_description_review(review)
        .unwrap_or_else(|err| panic!("review failed: {} {}", err.code, err.message));
    assert!(!review_bytes.ends_with('\n'));
    serde_json::to_string(&json!({
        "protocol": COMMAND_PROTOCOL,
        "review": review,
        "expected_review_sha256": payload_sha256(review_bytes.as_bytes()),
        "artifact": artifact,
        "settings": settings(),
        "decisions": decisions,
        "roles": roles
    }))
    .expect("pack command")
}

fn happy_pack() -> String {
    let world = ready();
    let outcome = outcome_of(&world.command_text);
    let review = review_text(
        &world.command_text,
        &world.outcome_sha,
        &world.schema_hash,
        &world.evidence_hash,
        happy_decisions(&outcome),
    );
    pack_text(
        &review,
        &world.artifact,
        Value::Array(classifications(&world.draft)),
        two_roles(),
    )
}

fn apply_ok(command: &str) -> (String, Value) {
    let text = apply_description_pack(command)
        .unwrap_or_else(|err| panic!("pack failed: {} {}", err.code, err.message));
    let value: Value = serde_json::from_str(&text).expect("pack json");
    (text, value)
}

fn document_of<'a>(packed: &'a Value, dataset: &str) -> &'a Value {
    packed["curated_documents"]
        .as_array()
        .expect("documents")
        .iter()
        .find(|item| item["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing curated document {dataset}"))
}

fn review_dataset<'a>(review: &'a Value, dataset: &str) -> &'a Value {
    review["datasets"]
        .as_array()
        .expect("review datasets")
        .iter()
        .find(|item| item["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing review dataset {dataset}"))
}

fn decision_row<'a>(review: &'a Value, dataset: &str) -> &'a Value {
    review["decisions"]
        .as_array()
        .expect("decisions")
        .iter()
        .find(|item| item["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing decision {dataset}"))
}

fn draft_dataset<'a>(draft: &'a Value, layer: &str, dataset: &str) -> &'a Value {
    dataset_row(&draft[layer]["datasets"], dataset)
}

fn dataset_row<'a>(datasets: &'a Value, dataset: &str) -> &'a Value {
    datasets
        .as_array()
        .expect("datasets")
        .iter()
        .find(|item| item["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing dataset {dataset}"))
}

fn column_of<'a>(dataset: &'a Value, name: &str) -> &'a Value {
    dataset["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("missing column {name}"))
}

fn pack_for<'a>(packed: &'a Value, role: &str) -> &'a Value {
    packed["packs"]
        .as_array()
        .expect("packs")
        .iter()
        .find(|item| item["role"] == role)
        .unwrap_or_else(|| panic!("missing pack {role}"))
}

fn assert_bound_hash(document: &str, value: &Value) {
    let mut unsigned = value.as_object().expect("object").clone();
    let recorded = unsigned
        .remove("pack_sha256")
        .and_then(|item| item.as_str().map(str::to_owned))
        .expect("pack sha");
    assert_eq!(recorded, canonical_sha(&Value::Object(unsigned)));
    assert_eq!(document, canonical_text(value));
    assert!(!document.ends_with('\n'));
}

fn recompile(artifact: &Value) -> oo_builder::CapturedPgDraft {
    let envelope = &artifact["evidence_envelope"];
    let curated = optional_canonical(envelope.get("curated"));
    let reviews = optional_canonical(envelope.get("reviews"));
    compile_captured_pg_catalog(
        &canonical_text(&envelope["catalog"]),
        curated.as_deref(),
        reviews.as_deref(),
    )
    .unwrap_or_else(|err| panic!("recompile failed: {} {}", err.code, err.message))
}

fn optional_canonical(value: Option<&Value>) -> Option<String> {
    match value {
        None | Some(Value::Null) => None,
        Some(inner) => Some(canonical_text(inner)),
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

fn field_sha(value: Option<&str>) -> String {
    let literal = match value {
        None => "null".to_owned(),
        Some(text) => {
            let mut out = String::new();
            push_string(&mut out, text);
            out
        }
    };
    let hashed = payload_sha256(literal.as_bytes());
    assert_eq!(hashed, descriptive_field_sha256(value));
    hashed
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
        Value::Object(map) => write_object(out, map),
    }
}

fn write_object(out: &mut String, map: &Map<String, Value>) {
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

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn leak_absent(text: &str) {
    assert!(!text.contains(CRATE_LABEL), "rejected candidate leaked");
    assert!(!text.contains(CRATE_BODY), "rejected prose leaked");
    assert!(!text.contains(CANDIDATE_GRAIN), "unapproved grain leaked");
    assert!(!text.contains(INCOMPLETE_BODY), "incomplete body leaked");
    assert!(!text.contains(ENV_CANARY), "env canary leaked");
}

fn check_role(packed: &Value, role: &str, oos: &str, visibility: &Value, description_visible: bool) {
    let compiled = compile_role_pack(oos, &canonical_text(visibility))
        .unwrap_or_else(|err| panic!("role pack failed: {} {}", err.code, err.message));
    let record = pack_for(packed, role);
    assert_eq!(record["pack_digest"], compiled.pack_digest);
    assert_eq!(record["manifest_sha256"], compiled.manifest_sha256);
    assert_eq!(record["cache_key_sha256"], compiled.cache_key_sha256);
    assert_eq!(
        record["artifact_sha256"],
        payload_sha256(compiled.artifact.as_bytes())
    );
    let bytes = compiled.artifact;
    assert!(bytes.contains(LABEL), "{role} lost the approved label");
    assert!(!bytes.contains(CRATE_LABEL), "{role} rendered the rejected candidate");
    assert!(!bytes.contains(CRATE_BODY), "{role} rendered the rejected prose");
    assert!(!bytes.contains(CANDIDATE_GRAIN), "{role} rendered the unapproved grain");
    assert!(!bytes.contains("seal_note"), "{role} rendered seal_note");
    if description_visible {
        assert!(bytes.contains(DESCRIPTION), "{role} dropped the allowlisted description");
        assert!(bytes.contains(UNREVIEWED_FENCE), "{role} dropped the UNREVIEWED fence");
        assert!(bytes.contains("badge_code"), "{role} dropped an allowlisted name");
    } else {
        assert!(!bytes.contains(DESCRIPTION), "{role} kept a hidden-name description");
        assert!(!bytes.contains("badge_code"), "{role} rendered badge_code");
        assert!(!bytes.contains(UNREVIEWED_FENCE), "{role} fenced a filtered description");
    }
}

#[test]
fn approved_text_reseals_through_the_compiler() {
    assert_eq!(CAPTURED_AT.len(), 27);
    let world = ready();
    let before = recompile(&world.artifact);
    assert_eq!(before.wrapper_hash, world.wrapper_hash);
    let outcome = outcome_of(&world.command_text);
    assert_eq!(outcome["outcome"], "candidates");
    let review_input = review_text(
        &world.command_text,
        &world.outcome_sha,
        &world.schema_hash,
        &world.evidence_hash,
        happy_decisions(&outcome),
    );
    let review_bytes = apply_description_review(&review_input)
        .unwrap_or_else(|err| panic!("review failed: {} {}", err.code, err.message));
    let review_sha = payload_sha256(review_bytes.as_bytes());
    let review: Value = serde_json::from_str(&review_bytes).expect("review");
    assert_eq!(review["outcome"], "reviewed");
    assert_eq!(review["statements"]["pii_classification"], "not_performed");
    assert_eq!(review["statements"]["database"], "not_used");
    let invoice_decision = decision_row(&review, INVOICE);
    assert_eq!(invoice_decision["applied"], true);
    assert_eq!(invoice_decision["authenticated"], false);
    assert_eq!(invoice_decision["label_disposition"], "replaced");
    assert_eq!(invoice_decision["description_disposition"], "filled");
    assert_eq!(invoice_decision["grain_disposition"], "kept");
    let crate_decision = decision_row(&review, CRATE);
    assert_eq!(crate_decision["applied"], false);
    assert_eq!(crate_decision["decision"], "reject");
    let mut expected_invoice = invoice_file();
    expected_invoice["label"] = json!(LABEL);
    expected_invoice["description"] = json!(DESCRIPTION);
    assert_eq!(review_dataset(&review, INVOICE)["document"], expected_invoice);
    assert_eq!(review_dataset(&review, INVOICE)["changed"], true);
    assert_eq!(review_dataset(&review, CRATE)["document"], crate_file());
    assert_eq!(review_dataset(&review, SHELF)["document"], shelf_file());
    assert_eq!(review_dataset(&review, SHELF)["changed"], false);
    assert!(!canonical_text(&review_dataset(&review, CRATE)["document"]).contains(CRATE_LABEL));

    let command = pack_text(
        &review_input,
        &world.artifact,
        Value::Array(classifications(&world.draft)),
        two_roles(),
    );
    let submitted: Value = serde_json::from_str(&command).expect("submitted");
    assert_eq!(submitted["expected_review_sha256"], review_sha);
    let (text, packed) = apply_ok(&command);
    assert_bound_hash(&text, &packed);
    leak_absent(&text);
    assert_eq!(packed["protocol"], PACK_PROTOCOL);
    assert_eq!(packed["compiler"], PACK_COMPILER);
    assert_eq!(packed["outcome"], "packed");
    assert_eq!(packed["review_sha256"], review_sha);
    assert_eq!(packed["statements"]["source_artifact"], "not_mutated");
    assert_eq!(packed["statements"]["database"], "not_used");
    assert_eq!(packed["statements"]["grants"], "not_authorized");
    assert_eq!(packed["statements"]["llm"], "not_used");
    assert_eq!(packed["statements"]["publication"], "unavailable");
    assert_eq!(packed["statements"]["authentication"], "not_claimed");
    assert_eq!(packed["statements"]["semantic_publication"], "not_claimed");
    assert_eq!(packed["statements"]["reseal"], "compiler");
    assert_eq!(packed["original_wrapper_hash"], world.wrapper_hash);
    assert_ne!(packed["resealed_wrapper_hash"], world.wrapper_hash);
    let again = recompile(&world.artifact);
    assert_eq!(again.wrapper_hash, world.wrapper_hash);

    let invoice = document_of(&packed, INVOICE);
    assert_eq!(invoice["document"], expected_invoice);
    assert_eq!(invoice["document"]["grain"], GRAIN);
    assert_eq!(invoice["document"]["entity"], "shipment");
    assert_eq!(invoice["document"]["row_count_estimate"], 18);
    assert_eq!(invoice["document"]["tier"], "raw");
    assert_eq!(invoice["document"]["caveats"], json!(["kept invoice caveat"]));
    let qty = column_of(&invoice["document"], "qty");
    assert_eq!(qty["pii"], "personal");
    assert_eq!(qty["status"], "deprecated");
    assert_eq!(qty["semantic_type"], "quantity");
    assert_eq!(qty["classification"]["state"], "confirmed");
    assert_eq!(qty["classification"]["by"], "ops.lina");
    assert_eq!(document_of(&packed, CRATE)["document"], crate_file());
    assert_eq!(document_of(&packed, SHELF)["document"], shelf_file());
    assert_eq!(document_of(&packed, SHELF)["changed"], false);

    let resealed = recompile(&packed["resealed_artifact"]);
    assert_eq!(resealed.wrapper_hash, packed["resealed_wrapper_hash"]);
    assert_eq!(resealed.draft_hash, packed["resealed_draft_hash"]);
    assert_eq!(resealed.evidence_envelope_hash, packed["resealed_evidence_envelope_hash"]);
    let resealed_draft: Value = serde_json::from_str(&resealed.draft).expect("resealed draft");
    let curated_invoice = draft_dataset(&resealed_draft, "curated", INVOICE);
    assert_eq!(curated_invoice["entity"], "shipment");
    assert_eq!(column_of(curated_invoice, "qty")["pii"], "sensitive");
    assert_eq!(column_of(curated_invoice, "qty")["semantic_type"], "free_text");
    assert_eq!(column_of(curated_invoice, "qty")["status"], "draft");
    let effective_invoice = draft_dataset(&resealed_draft, "effective", INVOICE);
    assert_eq!(effective_invoice["label"], LABEL);
    assert_eq!(effective_invoice["description"], DESCRIPTION);
    assert_ne!(effective_invoice["grain"], CANDIDATE_GRAIN);
    assert!(column_of(effective_invoice, "qty")["pii"].is_null());
    assert_ne!(draft_dataset(&resealed_draft, "effective", CRATE)["label"], CRATE_LABEL);

    let effective_command = canonical_text(&json!({
        "artifact": packed["resealed_artifact"],
        "decisions": submitted["decisions"],
        "protocol": REVIEWED_EFFECTIVE_COMMAND_PROTOCOL,
        "settings": submitted["settings"],
    }));
    let effective_text = compile_reviewed_effective(&effective_command)
        .unwrap_or_else(|err| panic!("effective failed: {} {}", err.code, err.message));
    assert_eq!(payload_sha256(effective_text.as_bytes()), packed["effective_sha256"]);
    let effective: Value = serde_json::from_str(&effective_text).expect("effective");
    assert_eq!(effective, packed["effective"]);
    let reviewed_qty = column_of(dataset_row(&effective["effective_oos"]["datasets"], INVOICE), "qty");
    assert_eq!(reviewed_qty["pii"], "restricted");
    assert_eq!(reviewed_qty["semantic_type"], "quantity");
    assert_eq!(reviewed_qty["classification"]["state"], "confirmed");
    assert_eq!(reviewed_qty["classification"]["by"], "mina.owner");
    assert_ne!(reviewed_qty["pii"], "personal");
    assert_ne!(reviewed_qty["pii"], "sensitive");
    assert_eq!(
        effective["effective_oos"]["ontology"]["classification"]["auto_accept"],
        "rule_high_confidence"
    );

    let oos = canonical_text(&effective["effective_oos"]);
    check_role(&packed, "oo_r_gate", &oos, &two_roles()[0], false);
    check_role(&packed, "oo_r_lane", &oos, &two_roles()[1], true);
}

#[test]
fn hash_tuple_cas_and_stale_field_refuse() {
    let world = ready();
    let outcome = outcome_of(&world.command_text);
    let mut command: Value = serde_json::from_str(&happy_pack()).expect("command");
    command["expected_review_sha256"] = json!("ef".repeat(32));
    let err = apply_description_pack(&serde_json::to_string(&command).expect("text")).expect_err("hash");
    assert!(err.code.ends_with("/review.hash"), "{}", err.code);
    leak_absent(&err.to_string());
    assert!(!err.to_string().contains(LABEL));

    let mut decisions = happy_decisions(&outcome);
    let review = review_text(
        &world.command_text,
        &world.outcome_sha,
        &"ab".repeat(32),
        &world.evidence_hash,
        decisions.clone(),
    );
    let command = pack_text(&review, &world.artifact, json!([]), two_roles());
    let err = apply_description_pack(&command).expect_err("current schema");
    assert!(err.code.ends_with("/review.schema"), "{}", err.code);
    leak_absent(&err.to_string());

    decisions = happy_decisions(&outcome);
    decisions[0]["fields"]["label"]["current_sha256"] = json!("cd".repeat(32));
    let review = review_text(
        &world.command_text,
        &world.outcome_sha,
        &world.schema_hash,
        &world.evidence_hash,
        decisions,
    );
    let command = pack_text(&review, &world.artifact, json!([]), two_roles());
    let err = apply_description_pack(&command).expect_err("cas");
    assert!(err.code.ends_with("/review.cas"), "{}", err.code);
    leak_absent(&err.to_string());
    assert!(!err.to_string().contains(DESCRIPTION));

    let mut decisions = happy_decisions(&outcome);
    decisions[0]["card_sha256"] = json!("ee".repeat(32));
    let review = review_text(
        &world.command_text,
        &world.outcome_sha,
        &world.schema_hash,
        &world.evidence_hash,
        decisions,
    );
    let command = pack_text(&review, &world.artifact, json!([]), two_roles());
    let err = apply_description_pack(&command).expect_err("stale field");
    assert!(err.code.ends_with("/review.anchor"), "{}", err.code);
    leak_absent(&err.to_string());

    let mut command: Value = serde_json::from_str(&happy_pack()).expect("command");
    command["artifact"]["protocol"] = json!("oo.builder.other");
    let err = apply_description_pack(&serde_json::to_string(&command).expect("text")).expect_err("artifact");
    assert!(err.code.ends_with("/seal.artifact"), "{}", err.code);
    leak_absent(&err.to_string());
    let again = recompile(&world.artifact);
    assert_eq!(again.wrapper_hash, world.wrapper_hash);
}

#[test]
fn rejected_and_deferred_leave_the_original_seal() {
    let world = ready();
    let outcome = outcome_of(&world.command_text);
    let invoice = anchors(&outcome, INVOICE, &canonical_sha(&invoice_file()));
    let crate_row = anchors(&outcome, CRATE, &canonical_sha(&crate_file()));
    let review = review_text(
        &world.command_text,
        &world.outcome_sha,
        &world.schema_hash,
        &world.evidence_hash,
        vec![
            decision(INVOICE, "defer", &invoice, None),
            decision(CRATE, "reject", &crate_row, None),
        ],
    );
    let command = pack_text(
        &review,
        &world.artifact,
        Value::Array(classifications(&world.draft)),
        two_roles(),
    );
    let (text, packed) = apply_ok(&command);
    leak_absent(&text);
    assert!(!text.contains(LABEL));
    assert!(!text.contains(DESCRIPTION));
    assert_eq!(packed["outcome"], "reviewed_unchanged");
    assert!(packed["resealed_artifact"].is_null());
    assert!(packed["resealed_wrapper_hash"].is_null());
    assert_eq!(packed["statements"]["reseal"], "not_required");
    assert_eq!(packed["original_wrapper_hash"], world.wrapper_hash);
    assert_eq!(document_of(&packed, INVOICE)["document"], invoice_file());
    assert_eq!(document_of(&packed, CRATE)["document"], crate_file());
    assert_eq!(document_of(&packed, SHELF)["document"], shelf_file());
    let again = recompile(&world.artifact);
    assert_eq!(again.wrapper_hash, world.wrapper_hash);
}

#[test]
fn incomplete_body_does_not_leak() {
    let world = ready();
    let mut command = candidate_command(&recompile(&world.artifact), &world.artifact);
    command["captured_response"] = json!({
        "body": INCOMPLETE_BODY,
        "byte_len": INCOMPLETE_BODY.len(),
        "status": "provider_failure"
    });
    let command_text = serde_json::to_string(&command).expect("command");
    let outcome_text = complete_description_candidates(&command_text)
        .unwrap_or_else(|err| panic!("complete failed: {} {}", err.code, err.message));
    assert!(!outcome_text.contains(INCOMPLETE_BODY));
    let outcome_sha = payload_sha256(outcome_text.as_bytes());
    let review = review_text(
        &command_text,
        &outcome_sha,
        &world.schema_hash,
        &world.evidence_hash,
        vec![decision(
            INVOICE,
            "approve",
            &json!({
                "candidate": "aa".repeat(32),
                "card": "aa".repeat(32),
                "schema": world.schema_hash,
                "evidence": world.evidence_hash,
                "request": "aa".repeat(32),
                "curated": canonical_sha(&invoice_file())
            }),
            Some(approve_fields()),
        )],
    );
    let command = pack_text(&review, &world.artifact, json!([]), two_roles());
    let (text, packed) = apply_ok(&command);
    leak_absent(&text);
    assert!(!text.contains(LABEL));
    assert!(!text.contains(DESCRIPTION));
    assert_eq!(packed["outcome"], "not_candidates");
    assert_eq!(packed["compiler_outcome"], "provider_failure");
    assert!(packed.get("packs").is_none());
    assert!(packed.get("curated_documents").is_none());
    assert!(packed.get("resealed_artifact").is_none());
    assert_eq!(packed["statements"]["reseal"], "not_required");
    assert_eq!(packed["queue"][0]["kind"], "compiler");
    assert_eq!(packed["queue"][0]["reason"], "provider_failure");
}

#[test]
fn occupied_dataset_review_is_unsupported() {
    let reviews = serde_json::to_string(&json!([{
        "action": "accept",
        "fact_fingerprint": "11".repeat(32),
        "item_id": format!("dataset/{INVOICE}"),
        "schema_hash": payload_sha256(schema_preimage().as_bytes())
    }]))
    .expect("reviews");
    let world = seal(Some(&reviews));
    let outcome = outcome_of(&world.command_text);
    let review = review_text(
        &world.command_text,
        &world.outcome_sha,
        &world.schema_hash,
        &world.evidence_hash,
        happy_decisions(&outcome),
    );
    let command = pack_text(
        &review,
        &world.artifact,
        Value::Array(classifications(&world.draft)),
        two_roles(),
    );
    let (text, packed) = apply_ok(&command);
    leak_absent(&text);
    assert!(!text.contains(LABEL));
    assert!(!text.contains(DESCRIPTION));
    assert!(!text.contains("badge_code"));
    assert_eq!(packed["outcome"], "unsupported");
    assert_eq!(packed["seam"], "review.dataset_decision_occupied");
    assert!(packed.get("packs").is_none());
    assert!(packed.get("resealed_artifact").is_none());
    assert!(packed.get("curated_documents").is_none());
    assert_eq!(packed["statements"]["reseal"], "unsupported");
    assert_eq!(packed["statements"]["source_artifact"], "not_mutated");
    let again = recompile(&world.artifact);
    assert_eq!(again.wrapper_hash, world.wrapper_hash);
}

#[test]
fn unclassified_allowlist_is_unknown_pii() {
    let world = ready();
    let outcome = outcome_of(&world.command_text);
    let review = review_text(
        &world.command_text,
        &world.outcome_sha,
        &world.schema_hash,
        &world.evidence_hash,
        happy_decisions(&outcome),
    );
    let roles = json!([visibility(
        "oo_r_gate",
        "gate_user",
        json!([grant(CRATE, &["seal_note"])])
    )]);
    let command = pack_text(
        &review,
        &world.artifact,
        Value::Array(classifications(&world.draft)),
        roles,
    );
    let err = apply_description_pack(&command).expect_err("pii");
    assert!(err.code.contains("classification.unknown_pii"), "{}", err.code);
    leak_absent(&err.to_string());
    assert!(!err.to_string().contains("seal_note"));
    assert!(!err.to_string().contains(DESCRIPTION));
    let again = recompile(&world.artifact);
    assert_eq!(again.wrapper_hash, world.wrapper_hash);
}

#[test]
fn compiled_cli_matches_the_library() {
    let command = happy_pack();
    let library = apply_description_pack(&command)
        .unwrap_or_else(|err| panic!("library failed: {} {}", err.code, err.message));
    let output = run_cli(&["apply"], command.as_bytes());
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout, library.as_bytes());
    assert!(output.stderr.is_empty());
    assert!(!library.ends_with('\n'));

    let usage = run_cli(&[], b"");
    assert_eq!(usage.status.code(), Some(2));
    let usage_text = String::from_utf8(usage.stderr).expect("usage utf8");
    assert!(usage_text.contains("description-pack: usage: description-pack apply"));
    assert!(usage.stdout.is_empty());

    let mut broken: Value = serde_json::from_str(&command).expect("command");
    broken["expected_review_sha256"] = json!("ab".repeat(32));
    let refused = run_cli(&["apply"], serde_json::to_string(&broken).expect("text").as_bytes());
    assert_eq!(refused.status.code(), Some(1));
    assert!(refused.stdout.is_empty());
    let stderr = String::from_utf8(refused.stderr).expect("stderr");
    assert!(stderr.starts_with("description-pack:"));
    assert!(stderr.contains("/review.hash"));
    leak_absent(&stderr);
    assert!(!stderr.contains(LABEL));
    assert!(!stderr.contains("badge_code"));
}

fn run_cli(args: &[&str], stdin: &[u8]) -> std::process::Output {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_description-pack"));
    command
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env(ENV_CANARY, ENV_CANARY)
        .env_remove("RUST_BACKTRACE");
    let mut child = command.spawn().expect("spawn");
    use std::io::Write;
    child.stdin.as_mut().expect("stdin").write_all(stdin).expect("write");
    drop(child.stdin.take());
    child.wait_with_output().expect("wait")
}
