//! Approved descriptions resealed through the captured, effective, and role-pack compilers.
//!
//! Schema hashes, field hashes, and candidate hashes in this file are computed
//! here with `payload_sha256`. They do not call the pack bridge to invent a seal.

#![forbid(unsafe_code)]

use std::sync::OnceLock;

use serde_json::{Map, Value, json};
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

const CAPTURED_AT: &str = "2026-03-20T08:15:00.000000Z";
const OWNER: &str = "tbl_owner";
const SOURCE: &str = "a292_src";
const DATABASE: &str = "a292db";
const MODEL: &str = "a292-captured";
const ORDERS: &str = "public.orders";
const LEDGER: &str = "public.ledger";
const ASIDE: &str = "public.aside";
const LABEL: &str = "Parcel register a292";
const GRAIN: &str = "one parcel row a292";
const DESCRIPTION: &str = "The vault_token column stores a bearer for the hidden field.";
const STRUCT_LABEL: &str = "Structural orders label";
const STRUCT_GRAIN: &str = "one structural order";
const STRUCT_DESCRIPTION: &str = "Structural orders prose";
const STRUCT_SOURCE: &str = "src_orders_bound";
const COLUMN_LABEL: &str = "Count structural";
const LEDGER_LABEL: &str = "LEDGER_CANDIDATE_DO_NOT_APPLY";
const LEDGER_BODY: &str = "seal_owner_a292 ledger prose";
const INCOMPLETE_BODY: &str = "INCOMPLETE_BODY_a292_DO_NOT_LEAK";
const ENV_CANARY: &str = "ENV_CANARY_a292_DO_NOT_LEAK";
const REVIEWER: &str = "ada.reviewer";
const REVIEWED_ON: &str = "2026-10-06";
const PROJECT: &str = "a292pack";
const ONTOLOGY: &str = "ont-a292";
const CATALOG_VERSION: &str = "cat-a292";

struct Col {
    attnum: i32,
    name: &'static str,
    type_oid: i64,
    type_name: &'static str,
    not_null: bool,
}

struct Ready {
    compiled_wrapper: String,
    artifact: Value,
    draft: Value,
    command_text: String,
    outcome_sha: String,
    schema_hash: String,
    evidence_hash: String,
}

fn ready() -> &'static Ready {
    static READY: OnceLock<Ready> = OnceLock::new();
    READY.get_or_init(|| {
        let compiled = compile_captured_pg_catalog(&catalog_json(), Some(&overlay_json()), None)
            .unwrap_or_else(|err| panic!("catalog compile failed: {} {}", err.code, err.message));
        let artifact = artifact_value(&compiled);
        let command = candidate_command(&compiled, &artifact);
        let command_text = serde_json::to_string(&command).expect("candidate command");
        let request_text = prepare_description_request(&command_text)
            .unwrap_or_else(|err| panic!("prepare failed: {} {}", err.code, err.message));
        let request: Value = serde_json::from_str(&request_text).expect("request");
        let produced = finish_candidates(&command, &request, candidates(&request));
        let draft: Value = serde_json::from_str(&compiled.draft).expect("draft");
        Ready {
            compiled_wrapper: compiled.wrapper_hash,
            artifact,
            draft,
            command_text: produced.0,
            outcome_sha: produced.1,
            schema_hash: compiled.schema_hash,
            evidence_hash: compiled.evidence_hash,
        }
    })
}

fn candidates(request: &Value) -> Value {
    json!([
        candidate(request, LEDGER, LEDGER_LABEL, LEDGER_BODY, "ledger grain stays out"),
        candidate(request, ORDERS, LABEL, DESCRIPTION, GRAIN),
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
    let outcome_sha = payload_sha256(outcome_text.as_bytes());
    (command_text, outcome_sha)
}

fn outcome_of(command_text: &str) -> Value {
    let outcome_text = complete_description_candidates(command_text)
        .unwrap_or_else(|err| panic!("complete failed: {} {}", err.code, err.message));
    serde_json::from_str(&outcome_text).expect("outcome")
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
        "tables": [table("ledger", ledger_columns()), table("orders", orders_columns())],
        "type_conversion": "unsupported"
    }))
    .expect("catalog")
}

fn ledger_columns() -> Vec<Col> {
    vec![
        Col { attnum: 1, name: "id", type_oid: 23, type_name: "int4", not_null: true },
        Col { attnum: 2, name: "secret_note", type_oid: 25, type_name: "text", not_null: false },
    ]
}

fn orders_columns() -> Vec<Col> {
    vec![
        Col { attnum: 1, name: "id", type_oid: 23, type_name: "int4", not_null: true },
        Col { attnum: 2, name: "parcel_count", type_oid: 23, type_name: "int4", not_null: false },
        Col { attnum: 3, name: "vault_token", type_oid: 25, type_name: "text", not_null: false },
    ]
}

fn schema_preimage() -> String {
    let mut tables = [("ledger", ledger_columns()), ("orders", orders_columns())];
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
    json!({
        "attnum": column.attnum,
        "name": column.name,
        "name_ident": column.name,
        "type_oid": column.type_oid,
        "type_schema": "pg_catalog",
        "type_name": column.type_name,
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
            "dataset": ORDERS,
            "entity": "parcel",
            "columns": [{
                "name": "parcel_count",
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
                visible("ledger", &compiled.evidence_hash, &compiled.schema_hash),
                visible("orders", &compiled.evidence_hash, &compiled.schema_hash)
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

fn orders_file() -> Value {
    dataset_file(json!({
        "schema_version": "oos/v1",
        "dataset": ORDERS,
        "source_name": "src_orders",
        "label": "Orders register",
        "description": Value::Null,
        "tier": "raw",
        "recommended": false,
        "grain": Value::Null,
        "primary_key": ["id"],
        "entity": "parcel",
        "row_count_estimate": 42,
        "time_columns": [{"name": "id", "role": "created_at"}],
        "columns": [{
            "name": "parcel_count",
            "label": "Parcel count",
            "data_type": "int4",
            "semantic_type": "quantity",
            "pii": "personal",
            "classification": {"state": "confirmed", "by": "ops.wang", "at": "2026-10-06"},
            "status": "deprecated",
            "description": "curated parcel note",
            "provenance": "human",
            "review": {"state": "approved", "by": "ada.curator", "at": "2026-10-01"}
        }],
        "caveats": ["existing curated caveat"]
    }))
}

fn ledger_file() -> Value {
    dataset_file(json!({
        "schema_version": "oos/v1",
        "dataset": LEDGER,
        "label": "Ledger book",
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

fn aside_file() -> Value {
    dataset_file(json!({
        "schema_version": "oos/v1",
        "dataset": ASIDE,
        "source_name": "src_aside",
        "label": "Aside register",
        "description": "aside stays outside the catalog seal",
        "tier": "raw",
        "grain": "one aside row",
        "entity": "parcel",
        "row_count_estimate": 7,
        "columns": [{
            "name": "id",
            "label": "Id",
            "data_type": "int4",
            "semantic_type": "identifier",
            "pii": "restricted",
            "classification": {"state": "pending", "by": "ops.wang", "at": "2026-10-06"},
            "status": "deprecated"
        }],
        "caveats": ["aside caveat"]
    }))
}

fn dataset_file(value: Value) -> Value {
    let file: DatasetFile = serde_json::from_value(value).unwrap_or_else(|err| {
        panic!("dataset file did not parse: {err}");
    });
    assert!(validate_dataset(&file).is_empty());
    let typed = serde_json::to_value(&file).expect("typed dataset");
    assert_eq!(canonical_sha(&typed), curated_dataset_sha256(&file));
    typed
}

fn curated_map() -> Value {
    json!({
        ORDERS: orders_file(),
        LEDGER: ledger_file(),
        ASIDE: aside_file(),
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
    let current = field_sha(Some("Orders register"));
    assert_eq!(current, descriptive_field_sha256(Some("Orders register")));
    json!({
        "label": {"overwrite": true, "current_sha256": current},
        "description": {"overwrite": false},
        "grain": {"overwrite": false}
    })
}

fn review_text(command_text: &str, outcome_sha: &str, schema: &str, evidence: &str, decisions: Vec<Value>) -> String {
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
    let orders = anchors(outcome, ORDERS, &canonical_sha(&orders_file()));
    let ledger = anchors(outcome, LEDGER, &canonical_sha(&ledger_file()));
    vec![
        decision(ORDERS, "approve", &orders, Some(approve_fields())),
        decision(LEDGER, "reject", &ledger, None),
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
        "reviewer": {"by": "ada.owner", "at": "2026-10-06"}
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
        class_decision(draft, ORDERS, "id", "none", "identifier"),
        class_decision(draft, ORDERS, "parcel_count", "restricted", "quantity"),
        class_decision(draft, ORDERS, "vault_token", "restricted", "free_text"),
        class_decision(draft, LEDGER, "id", "none", "identifier"),
    ]
}

fn settings() -> Value {
    json!({
        "project": PROJECT,
        "ontology_version": ONTOLOGY,
        "catalog_version": CATALOG_VERSION,
        "policy": {"id": "pol-a292", "version": "1"},
        "knowledge": {"id": "know-a292", "version": "1"},
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
        "policy": {"id": "pol-a292", "version": "1"},
        "knowledge": {"id": "know-a292", "version": "1"},
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
        visibility("oo_r_desk", "desk_user", json!([grant(ORDERS, &["id", "parcel_count"])])),
        visibility(
            "oo_r_pay",
            "pay_user",
            json!([
                grant(ORDERS, &["id", "parcel_count", "vault_token"]),
                grant(LEDGER, &["id"])
            ])
        )
    ])
}

fn pack_text(review: &str, artifact: &Value, decisions: Value, roles: Value) -> String {
    let review_bytes = apply_description_review(review)
        .unwrap_or_else(|err| panic!("review failed: {} {}", err.code, err.message));
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
    pack_text(&review, &world.artifact, Value::Array(classifications(&world.draft)), two_roles())
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
    payload_sha256(literal.as_bytes())
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

fn absent(text: &str) {
    assert!(!text.contains(LEDGER_LABEL), "ledger candidate leaked");
    assert!(!text.contains(LEDGER_BODY), "owner canary leaked");
    assert!(!text.contains(INCOMPLETE_BODY), "incomplete body leaked");
    assert!(!text.contains(ENV_CANARY), "env canary leaked");
}

#[test]
fn approved_text_is_resealed_and_packed_per_role() {
    assert_eq!(CAPTURED_AT.len(), 27);
    let world = ready();
    let before = recompile(&world.artifact);
    assert_eq!(before.wrapper_hash, world.compiled_wrapper);
    let command = happy_pack();
    let (text, packed) = apply_ok(&command);
    assert_bound_hash(&text, &packed);
    absent(&text);
    assert_eq!(packed["protocol"], PACK_PROTOCOL);
    assert_eq!(packed["compiler"], PACK_COMPILER);
    assert_eq!(packed["outcome"], "packed");
    assert_eq!(packed["statements"]["source_artifact"], "not_mutated");
    assert_eq!(packed["statements"]["database"], "not_used");
    assert_eq!(packed["statements"]["grants"], "not_authorized");
    assert_eq!(packed["statements"]["llm"], "not_used");
    assert_eq!(packed["statements"]["publication"], "unavailable");
    assert_eq!(packed["statements"]["reseal"], "compiler");
    assert_eq!(packed["original_wrapper_hash"], world.compiled_wrapper);
    assert_ne!(packed["resealed_wrapper_hash"], world.compiled_wrapper);
    let again = recompile(&world.artifact);
    assert_eq!(again.wrapper_hash, world.compiled_wrapper);

    let orders = document_of(&packed, ORDERS);
    assert_eq!(orders["document"]["label"], LABEL);
    assert_eq!(orders["document"]["description"], DESCRIPTION);
    assert_eq!(orders["document"]["grain"], GRAIN);
    assert_eq!(orders["document"]["entity"], "parcel");
    assert_eq!(orders["document"]["row_count_estimate"], 42);
    assert_eq!(orders["document"]["tier"], "raw");
    assert_eq!(orders["document"]["caveats"], json!(["existing curated caveat"]));
    assert_eq!(column_of(&orders["document"], "parcel_count")["pii"], "personal");
    assert_eq!(column_of(&orders["document"], "parcel_count")["status"], "deprecated");
    assert_eq!(
        column_of(&orders["document"], "parcel_count")["classification"]["state"],
        "confirmed"
    );
    assert_eq!(document_of(&packed, LEDGER)["document"], ledger_file());
    assert_eq!(document_of(&packed, ASIDE)["document"], aside_file());
    assert_eq!(document_of(&packed, ASIDE)["changed"], false);

    let resealed = recompile(&packed["resealed_artifact"]);
    assert_eq!(resealed.wrapper_hash, packed["resealed_wrapper_hash"]);
    assert_eq!(resealed.draft_hash, packed["resealed_draft_hash"]);
    assert_eq!(
        resealed.evidence_envelope_hash,
        packed["resealed_evidence_envelope_hash"]
    );
    let resealed_draft: Value = serde_json::from_str(&resealed.draft).expect("resealed draft");
    let curated_orders = draft_dataset(&resealed_draft, "curated", ORDERS);
    assert_eq!(curated_orders["entity"], "parcel");
    assert_eq!(column_of(curated_orders, "parcel_count")["pii"], "sensitive");
    assert_eq!(column_of(curated_orders, "parcel_count")["semantic_type"], "free_text");
    assert_eq!(column_of(curated_orders, "parcel_count")["status"], "draft");
    let effective_orders = draft_dataset(&resealed_draft, "effective", ORDERS);
    assert_eq!(effective_orders["label"], LABEL);
    assert_eq!(effective_orders["description"], DESCRIPTION);
    assert_eq!(effective_orders["grain"], GRAIN);
    assert!(column_of(effective_orders, "parcel_count")["pii"].is_null());
    assert_ne!(draft_dataset(&resealed_draft, "effective", LEDGER)["label"], LEDGER_LABEL);

    let submitted: Value = serde_json::from_str(&command).expect("submitted");
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
    let parcel = column_of(dataset_row(&effective["effective_oos"]["datasets"], ORDERS), "parcel_count");
    assert_eq!(parcel["pii"], "restricted");
    assert_eq!(parcel["semantic_type"], "quantity");
    assert_eq!(parcel["classification"]["state"], "confirmed");

    let oos = canonical_text(&effective["effective_oos"]);
    check_role_pack(&packed, "oo_r_desk", &oos, &two_roles()[0], false);
    check_role_pack(&packed, "oo_r_pay", &oos, &two_roles()[1], true);
}

fn check_role_pack(packed: &Value, role: &str, oos: &str, visibility: &Value, description_visible: bool) {
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
    assert!(bytes.contains(LABEL), "{role} lost the visible label");
    assert!(bytes.contains(GRAIN), "{role} lost the visible grain");
    assert!(!bytes.contains("secret_note"), "{role} rendered secret_note");
    assert!(!bytes.contains(LEDGER_LABEL), "{role} rendered the rejected candidate");
    assert!(!bytes.contains(LEDGER_BODY), "{role} rendered the owner canary");
    if description_visible {
        assert!(bytes.contains(DESCRIPTION), "{role} dropped an allowlisted description");
        assert!(bytes.contains("UNREVIEWED"), "{role} bypassed the description fence");
        assert!(bytes.contains("vault_token"), "{role} dropped an allowlisted name");
    } else {
        assert!(!bytes.contains(DESCRIPTION), "{role} kept a hidden-name description");
        assert!(!bytes.contains("vault_token"), "{role} rendered vault_token");
    }
}

#[test]
fn reject_and_defer_leave_the_original_seal() {
    let world = ready();
    let outcome = outcome_of(&world.command_text);
    let orders = anchors(&outcome, ORDERS, &canonical_sha(&orders_file()));
    let ledger = anchors(&outcome, LEDGER, &canonical_sha(&ledger_file()));
    let review = review_text(
        &world.command_text,
        &world.outcome_sha,
        &world.schema_hash,
        &world.evidence_hash,
        vec![
            decision(ORDERS, "defer", &orders, None),
            decision(LEDGER, "reject", &ledger, None),
        ],
    );
    let command = pack_text(
        &review,
        &world.artifact,
        Value::Array(classifications(&world.draft)),
        two_roles(),
    );
    let (text, packed) = apply_ok(&command);
    absent(&text);
    assert_eq!(packed["outcome"], "reviewed_unchanged");
    assert!(packed["resealed_artifact"].is_null());
    assert!(packed["resealed_wrapper_hash"].is_null());
    assert_eq!(packed["statements"]["reseal"], "not_required");
    assert_eq!(packed["original_wrapper_hash"], world.compiled_wrapper);
    assert_eq!(document_of(&packed, ORDERS)["document"], orders_file());
    assert_eq!(document_of(&packed, LEDGER)["document"], ledger_file());
    let desk = pack_for(&packed, "oo_r_desk");
    let artifact = serde_json::to_string(&desk["artifact"]).expect("desk");
    assert!(!artifact.contains(LABEL));
    assert!(!artifact.contains(DESCRIPTION));
    assert!(!artifact.contains("vault_token"));
    assert!(!artifact.contains("secret_note"));
}

#[test]
fn stale_schema_and_cas_refuse_without_a_document() {
    let world = ready();
    let outcome = outcome_of(&world.command_text);
    let mut decisions = happy_decisions(&outcome);
    decisions[0]["schema_hash"] = json!("ab".repeat(32));
    let review = review_text(
        &world.command_text,
        &world.outcome_sha,
        &world.schema_hash,
        &world.evidence_hash,
        decisions,
    );
    let command = pack_text(&review, &world.artifact, json!([]), two_roles());
    let err = apply_description_pack(&command).expect_err("schema");
    assert!(err.code.ends_with("/review.schema"), "{}", err.code);
    absent(&err.to_string());
    assert!(!err.to_string().contains(LABEL));
    assert!(!err.to_string().contains("vault_token"));

    let mut decisions = happy_decisions(&outcome);
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
    absent(&err.to_string());
    assert!(!err.to_string().contains(DESCRIPTION));
}

#[test]
fn review_hash_and_artifact_mismatch_refuse() {
    let mut command: Value = serde_json::from_str(&happy_pack()).expect("command");
    command["expected_review_sha256"] = json!("ef".repeat(32));
    let err = apply_description_pack(&serde_json::to_string(&command).expect("text")).expect_err("hash");
    assert!(err.code.ends_with("/review.hash"), "{}", err.code);
    absent(&err.to_string());
    assert!(!err.to_string().contains(LABEL));

    let mut command: Value = serde_json::from_str(&happy_pack()).expect("command");
    command["artifact"]["protocol"] = json!("oo.builder.other");
    let err = apply_description_pack(&serde_json::to_string(&command).expect("text")).expect_err("artifact");
    assert!(err.code.ends_with("/seal.artifact"), "{}", err.code);
    absent(&err.to_string());
}

#[test]
fn incomplete_compiler_does_not_leak_candidate_text() {
    let world = ready();
    let request_text = prepare_description_request(&serde_json::to_string(&candidate_command(
        &recompile(&world.artifact),
        &world.artifact,
    )).expect("command"))
    .unwrap_or_else(|err| panic!("prepare failed: {} {}", err.code, err.message));
    let request: Value = serde_json::from_str(&request_text).expect("request");
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
            ORDERS,
            "approve",
            &json!({
                "candidate": "aa".repeat(32),
                "card": "aa".repeat(32),
                "schema": world.schema_hash,
                "evidence": world.evidence_hash,
                "request": request["request_sha256"],
                "curated": canonical_sha(&orders_file())
            }),
            Some(approve_fields()),
        )],
    );
    let command = pack_text(&review, &world.artifact, json!([]), two_roles());
    let (text, packed) = apply_ok(&command);
    absent(&text);
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
        "item_id": format!("dataset/{ORDERS}"),
        "schema_hash": payload_sha256(schema_preimage().as_bytes())
    }]))
    .expect("reviews");
    let compiled = compile_captured_pg_catalog(&catalog_json(), Some(&overlay_json()), Some(&reviews))
        .unwrap_or_else(|err| panic!("occupied compile failed: {} {}", err.code, err.message));
    let artifact = artifact_value(&compiled);
    let command = candidate_command(&compiled, &artifact);
    let command_text = serde_json::to_string(&command).expect("command");
    let request_text = prepare_description_request(&command_text)
        .unwrap_or_else(|err| panic!("prepare failed: {} {}", err.code, err.message));
    let request: Value = serde_json::from_str(&request_text).expect("request");
    let (produced_text, outcome_sha) = finish_candidates(&command, &request, candidates(&request));
    let outcome = outcome_of(&produced_text);
    let review = review_text(
        &produced_text,
        &outcome_sha,
        &compiled.schema_hash,
        &compiled.evidence_hash,
        happy_decisions(&outcome),
    );
    let draft: Value = serde_json::from_str(&compiled.draft).expect("draft");
    let command = pack_text(&review, &artifact, Value::Array(classifications(&draft)), two_roles());
    let (text, packed) = apply_ok(&command);
    absent(&text);
    assert!(!text.contains(LABEL));
    assert!(!text.contains(DESCRIPTION));
    assert!(!text.contains("vault_token"));
    assert_eq!(packed["outcome"], "unsupported");
    assert_eq!(packed["seam"], "review.dataset_decision_occupied");
    assert!(packed.get("packs").is_none());
    assert!(packed.get("resealed_artifact").is_none());
    assert!(packed.get("curated_documents").is_none());
    assert_eq!(packed["statements"]["reseal"], "unsupported");
    assert_eq!(packed["statements"]["source_artifact"], "not_mutated");
    let again = compile_captured_pg_catalog(&catalog_json(), Some(&overlay_json()), Some(&reviews))
        .unwrap_or_else(|err| panic!("original compile changed: {} {}", err.code, err.message));
    assert_eq!(again.wrapper_hash, compiled.wrapper_hash);
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
        "oo_r_desk",
        "desk_user",
        json!([grant(LEDGER, &["secret_note"])])
    )]);
    let command = pack_text(
        &review,
        &world.artifact,
        Value::Array(classifications(&world.draft)),
        roles,
    );
    let err = apply_description_pack(&command).expect_err("pii");
    assert!(err.code.contains("classification.unknown_pii"), "{}", err.code);
    absent(&err.to_string());
    assert!(!err.to_string().contains("secret_note"));
    assert!(!err.to_string().contains(DESCRIPTION));
}

#[test]
fn cli_matches_the_library() {
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
    absent(&stderr);
    assert!(!stderr.contains(LABEL));
    assert!(!stderr.contains("vault_token"));
}

struct Fingerprints {
    schema_hash: String,
    dataset: String,
    column: String,
    primary_key: String,
    ledger_column: String,
}

struct Described {
    artifact: Value,
    draft: Value,
    command_text: String,
    outcome_sha: String,
    schema_hash: String,
    evidence_hash: String,
    wrapper_hash: String,
    dataset_review: Value,
    dataset_review_sha: String,
}

fn fingerprints() -> &'static Fingerprints {
    static FINGERPRINTS: OnceLock<Fingerprints> = OnceLock::new();
    FINGERPRINTS.get_or_init(|| {
        let compiled = compile_captured_pg_catalog(&catalog_json(), Some(&structural_overlay()), None)
            .unwrap_or_else(|err| panic!("structural probe failed: {} {}", err.code, err.message));
        let draft: Value = serde_json::from_str(&compiled.draft).expect("structural draft");
        Fingerprints {
            schema_hash: compiled.schema_hash,
            dataset: queue_detail(&draft, &format!("dataset/{ORDERS}"), "curated_not_applied"),
            column: column_fingerprint(&draft, ORDERS, "parcel_count"),
            primary_key: queue_kind(&draft, &format!("primary_key/{ORDERS}"), "structural_primary_key"),
            ledger_column: column_fingerprint(&draft, LEDGER, "id"),
        }
    })
}

fn structural_overlay() -> String {
    serde_json::to_string(&json!({
        "datasets": [{
            "dataset": ORDERS,
            "label": STRUCT_LABEL,
            "description": STRUCT_DESCRIPTION,
            "grain": STRUCT_GRAIN,
            "source_name": STRUCT_SOURCE,
            "entity": "parcel",
            "primary_key": ["id"],
            "columns": [{
                "name": "parcel_count",
                "label": COLUMN_LABEL,
                "pii": "sensitive",
                "semantic_type": "free_text",
                "status": "draft"
            }]
        }],
        "relationships": []
    }))
    .expect("structural overlay")
}

fn queue_detail(draft: &Value, item_id: &str, detail: &str) -> String {
    draft["review_queue"]
        .as_array()
        .expect("queue")
        .iter()
        .find(|item| item["item_id"] == item_id && item["detail"] == detail)
        .unwrap_or_else(|| panic!("missing detail {detail} {item_id}"))["fact_fingerprint"]
        .as_str()
        .expect("fingerprint")
        .to_owned()
}

fn queue_kind(draft: &Value, item_id: &str, kind: &str) -> String {
    draft["review_queue"]
        .as_array()
        .expect("queue")
        .iter()
        .find(|item| item["item_id"] == item_id && item["kind"] == kind)
        .unwrap_or_else(|| panic!("missing kind {kind} {item_id}"))["fact_fingerprint"]
        .as_str()
        .expect("fingerprint")
        .to_owned()
}

fn bound_reviews(action: &str, dataset_fingerprint: &str, fp: &Fingerprints) -> Value {
    json!([
        {
            "action": "accept",
            "fact_fingerprint": fp.ledger_column,
            "item_id": format!("column/{LEDGER}/id"),
            "schema_hash": fp.schema_hash
        },
        {
            "action": "override",
            "fact_fingerprint": fp.column,
            "item_id": format!("column/{ORDERS}/parcel_count"),
            "schema_hash": fp.schema_hash
        },
        {
            "action": action,
            "fact_fingerprint": dataset_fingerprint,
            "item_id": format!("dataset/{ORDERS}"),
            "schema_hash": fp.schema_hash
        },
        {
            "action": "override",
            "fact_fingerprint": fp.primary_key,
            "item_id": format!("primary_key/{ORDERS}"),
            "schema_hash": fp.schema_hash
        }
    ])
}

fn describe(action: &str, dataset_fingerprint: &str) -> Described {
    let fp = fingerprints();
    let reviews = bound_reviews(action, dataset_fingerprint, fp);
    let compiled = compile_captured_pg_catalog(
        &catalog_json(),
        Some(&structural_overlay()),
        Some(&canonical_text(&reviews)),
    )
    .unwrap_or_else(|err| panic!("bound compile failed: {} {}", err.code, err.message));
    assert_eq!(compiled.schema_hash, fp.schema_hash);
    let artifact = artifact_value(&compiled);
    let dataset_review = artifact["evidence_envelope"]["reviews"]
        .as_array()
        .expect("reviews")
        .iter()
        .find(|item| item["item_id"] == format!("dataset/{ORDERS}"))
        .unwrap_or_else(|| panic!("missing dataset review"))
        .clone();
    let command = candidate_command(&compiled, &artifact);
    let command_text = serde_json::to_string(&command).expect("candidate command");
    let request_text = prepare_description_request(&command_text)
        .unwrap_or_else(|err| panic!("prepare failed: {} {}", err.code, err.message));
    let request: Value = serde_json::from_str(&request_text).expect("request");
    let (produced, outcome_sha) = finish_candidates(&command, &request, candidates(&request));
    let draft: Value = serde_json::from_str(&compiled.draft).expect("bound draft");
    Described {
        artifact,
        draft,
        command_text: produced,
        outcome_sha,
        schema_hash: compiled.schema_hash,
        evidence_hash: compiled.evidence_hash,
        wrapper_hash: compiled.wrapper_hash,
        dataset_review_sha: canonical_sha(&dataset_review),
        dataset_review,
    }
}

fn occupied_ready() -> &'static Described {
    static READY: OnceLock<Described> = OnceLock::new();
    READY.get_or_init(|| describe("override", &fingerprints().dataset))
}

fn accept_ready() -> &'static Described {
    static READY: OnceLock<Described> = OnceLock::new();
    READY.get_or_init(|| describe("accept", &fingerprints().dataset))
}

fn stale_ready() -> &'static Described {
    static READY: OnceLock<Described> = OnceLock::new();
    READY.get_or_init(|| describe("override", &"11".repeat(32)))
}

fn cas_map(world: &Described) -> Value {
    json!({ format!("dataset/{ORDERS}"): world.dataset_review_sha })
}

fn pack_with(world: &Described, decisions: Vec<Value>, replace: bool, expected: Option<Value>) -> String {
    let review = review_text(
        &world.command_text,
        &world.outcome_sha,
        &world.schema_hash,
        &world.evidence_hash,
        decisions,
    );
    let review_bytes = apply_description_review(&review)
        .unwrap_or_else(|err| panic!("review failed: {} {}", err.code, err.message));
    let mut command = json!({
        "protocol": COMMAND_PROTOCOL,
        "review": review,
        "expected_review_sha256": payload_sha256(review_bytes.as_bytes()),
        "artifact": world.artifact,
        "settings": settings(),
        "decisions": Value::Array(classifications(&world.draft)),
        "roles": two_roles()
    });
    if replace {
        command["replace_existing_dataset_review"] = json!(true);
    }
    if let Some(expected) = expected {
        command["expected_existing_review_sha256"] = expected;
    }
    serde_json::to_string(&command).expect("occupied pack")
}

fn occupied_decisions(world: &Described) -> Vec<Value> {
    happy_decisions(&outcome_of(&world.command_text))
}

fn review_by_id<'a>(artifact: &'a Value, item_id: &str) -> &'a Value {
    artifact["evidence_envelope"]["reviews"]
        .as_array()
        .expect("reviews")
        .iter()
        .find(|item| item["item_id"] == item_id)
        .unwrap_or_else(|| panic!("missing review {item_id}"))
}

#[test]
fn occupied_override_without_authorization_stays_unsupported() {
    let world = occupied_ready();
    let command = pack_with(world, occupied_decisions(world), false, None);
    let (text, packed) = apply_ok(&command);
    absent(&text);
    assert!(!text.contains(LABEL));
    assert!(!text.contains(DESCRIPTION));
    assert_eq!(packed["outcome"], "unsupported");
    assert_eq!(packed["seam"], "review.dataset_decision_occupied");
    assert!(packed.get("occupied_replacements").is_none());
    assert!(packed.get("packs").is_none());
    assert!(packed.get("resealed_artifact").is_none());
    assert_eq!(packed["statements"]["reseal"], "unsupported");
    assert_eq!(packed["statements"]["source_artifact"], "not_mutated");
    let again = recompile(&world.artifact);
    assert_eq!(again.wrapper_hash, world.wrapper_hash);
    let draft: Value = serde_json::from_str(&again.draft).expect("draft");
    assert_eq!(draft_dataset(&draft, "effective", ORDERS)["label"], STRUCT_LABEL);
    assert_eq!(draft_dataset(&draft, "effective", ORDERS)["source_name"], STRUCT_SOURCE);
}

#[test]
fn occupied_override_approve_reseals_two_roles() {
    let world = occupied_ready();
    let before = recompile(&world.artifact);
    assert_eq!(before.wrapper_hash, world.wrapper_hash);
    let command = pack_with(world, occupied_decisions(world), true, Some(cas_map(world)));
    let library = apply_description_pack(&command)
        .unwrap_or_else(|err| panic!("pack failed: {} {}", err.code, err.message));
    let output = run_cli(&["apply"], command.as_bytes());
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout, library.as_bytes());
    assert!(output.stderr.is_empty());
    let packed: Value = serde_json::from_str(&library).expect("pack");
    assert_bound_hash(&library, &packed);
    absent(&library);
    assert_eq!(packed["outcome"], "packed");
    assert_eq!(packed["statements"]["reseal"], "compiler");
    assert_eq!(packed["statements"]["authentication"], "not_claimed");
    assert_eq!(packed["original_wrapper_hash"], world.wrapper_hash);
    assert_ne!(packed["resealed_wrapper_hash"], world.wrapper_hash);
    assert!(packed.get("occupied_replacements").is_some());
    let row = &packed["occupied_replacements"][0];
    assert_eq!(row["attribution"], "bridge_provenance");
    assert_eq!(row["item_id"], format!("dataset/{ORDERS}"));
    assert_eq!(row["prior_review"], world.dataset_review);
    assert_eq!(row["prior_review_sha256"], world.dataset_review_sha);
    assert_eq!(row["compiler_review_sha256"], world.dataset_review_sha);
    assert_eq!(row["descriptive_provenance"]["reviewer_label"], REVIEWER);
    assert_eq!(row["descriptive_provenance"]["reviewer_at"], REVIEWED_ON);
    assert_eq!(row["descriptive_provenance"]["authenticated"], false);
    assert_eq!(
        row["descriptive_provenance"]["fields"],
        json!(["description", "grain", "label"])
    );
    assert_ne!(row["descriptive_provenance"]["reviewer_label"], "ada.curator");

    let seal = canonical_text(&packed["resealed_artifact"]);
    assert!(!seal.contains(REVIEWER));
    assert!(!seal.contains("reviewer_label"));
    assert!(!seal.contains("ada.curator"));
    assert!(!seal.contains("bridge_provenance"));
    assert!(!seal.contains(STRUCT_LABEL));
    assert!(!seal.contains(STRUCT_DESCRIPTION));
    assert_eq!(
        canonical_text(&packed["resealed_artifact"]["evidence_envelope"]["reviews"]),
        canonical_text(&world.artifact["evidence_envelope"]["reviews"])
    );

    let orders = document_of(&packed, ORDERS);
    let file: DatasetFile = serde_json::from_value(orders["document"].clone()).expect("dataset file");
    assert!(validate_dataset(&file).is_empty());
    assert_eq!(orders["document"]["label"], LABEL);
    assert_eq!(orders["document"]["description"], DESCRIPTION);
    assert_eq!(orders["document"]["grain"], GRAIN);
    assert_eq!(orders["document"]["entity"], "parcel");
    assert_eq!(orders["document"]["row_count_estimate"], 42);
    assert_eq!(column_of(&orders["document"], "parcel_count")["pii"], "personal");
    assert_eq!(column_of(&orders["document"], "parcel_count")["status"], "deprecated");
    assert_eq!(column_of(&orders["document"], "parcel_count")["review"]["by"], "ada.curator");
    assert_eq!(document_of(&packed, LEDGER)["document"], ledger_file());
    assert_eq!(document_of(&packed, ASIDE)["document"], aside_file());

    let resealed = recompile(&packed["resealed_artifact"]);
    assert_eq!(resealed.wrapper_hash, packed["resealed_wrapper_hash"]);
    assert_eq!(resealed.draft_hash, packed["resealed_draft_hash"]);
    assert_eq!(resealed.evidence_envelope_hash, packed["resealed_evidence_envelope_hash"]);
    let resealed_draft: Value = serde_json::from_str(&resealed.draft).expect("resealed draft");
    let effective = draft_dataset(&resealed_draft, "effective", ORDERS);
    assert_eq!(effective["label"], LABEL);
    assert_eq!(effective["description"], DESCRIPTION);
    assert_eq!(effective["grain"], GRAIN);
    assert_eq!(effective["source_name"], STRUCT_SOURCE);
    assert_eq!(effective["primary_key"], json!(["id"]));
    let parcel = column_of(effective, "parcel_count");
    assert_eq!(parcel["label"], COLUMN_LABEL);
    assert_eq!(parcel["status"], "draft");
    assert!(parcel["pii"].is_null());
    let curated = draft_dataset(&resealed_draft, "curated", ORDERS);
    assert_eq!(curated["entity"], "parcel");
    assert_eq!(column_of(curated, "parcel_count")["pii"], "sensitive");
    assert_eq!(column_of(curated, "parcel_count")["semantic_type"], "free_text");
    assert_eq!(column_of(curated, "parcel_count")["status"], "draft");
    assert_eq!(
        review_by_id(&packed["resealed_artifact"], &format!("column/{LEDGER}/id"))["action"],
        "accept"
    );
    assert_eq!(
        review_by_id(&packed["resealed_artifact"], &format!("column/{ORDERS}/parcel_count"))["action"],
        "override"
    );
    assert_eq!(
        canonical_sha(review_by_id(
            &packed["resealed_artifact"],
            &format!("primary_key/{ORDERS}"),
        )),
        canonical_sha(review_by_id(
            &world.artifact,
            &format!("primary_key/{ORDERS}"),
        ))
    );
    let again = recompile(&world.artifact);
    assert_eq!(again.wrapper_hash, world.wrapper_hash);

    let submitted: Value = serde_json::from_str(&command).expect("submitted");
    let effective_command = canonical_text(&json!({
        "artifact": packed["resealed_artifact"],
        "decisions": submitted["decisions"],
        "protocol": REVIEWED_EFFECTIVE_COMMAND_PROTOCOL,
        "settings": submitted["settings"],
    }));
    let effective_text = compile_reviewed_effective(&effective_command)
        .unwrap_or_else(|err| panic!("effective failed: {} {}", err.code, err.message));
    assert_eq!(payload_sha256(effective_text.as_bytes()), packed["effective_sha256"]);
    let effective_doc: Value = serde_json::from_str(&effective_text).expect("effective");
    assert_eq!(effective_doc, packed["effective"]);
    let reviewed = column_of(
        dataset_row(&effective_doc["effective_oos"]["datasets"], ORDERS),
        "parcel_count",
    );
    assert_eq!(reviewed["pii"], "restricted");
    assert_eq!(reviewed["classification"]["by"], "ada.owner");
    let oos = canonical_text(&effective_doc["effective_oos"]);
    check_role_pack(&packed, "oo_r_desk", &oos, &two_roles()[0], false);
    check_role_pack(&packed, "oo_r_pay", &oos, &two_roles()[1], true);
    let desk = canonical_text(&pack_for(&packed, "oo_r_desk")["artifact"]);
    let pay = canonical_text(&pack_for(&packed, "oo_r_pay")["artifact"]);
    assert!(!desk.contains(STRUCT_LABEL));
    assert!(!pay.contains(STRUCT_LABEL));
    assert!(!desk.contains(STRUCT_DESCRIPTION));
    assert!(!pay.contains("ada.curator"));
}

#[test]
fn occupied_cas_conflict_preserves_the_seal() {
    let world = occupied_ready();
    let mut wrong = cas_map(world);
    wrong[format!("dataset/{ORDERS}")] = json!("ab".repeat(32));
    let cases = [
        pack_with(world, occupied_decisions(world), true, Some(wrong)),
        pack_with(world, occupied_decisions(world), true, Some(json!({}))),
        pack_with(
            world,
            occupied_decisions(world),
            true,
            Some({
                let mut extra = cas_map(world);
                extra["dataset/public.ledger"] = json!(world.dataset_review_sha.clone());
                extra
            }),
        ),
    ];
    for command in cases {
        let err = apply_description_pack(&command).expect_err("cas");
        assert!(err.code.ends_with("/review.occupied_cas"), "{}", err.code);
        assert!(!err.to_string().contains(LABEL));
        assert!(!err.to_string().contains(REVIEWER));
        assert!(!err.to_string().contains(&world.dataset_review_sha));
        assert!(!err.to_string().contains(STRUCT_LABEL));
    }
    let unauthorized = pack_with(world, occupied_decisions(world), false, Some(cas_map(world)));
    let err = apply_description_pack(&unauthorized).expect_err("flag");
    assert!(err.code.ends_with("/schema.invalid"), "{}", err.code);
    assert!(err.to_string().contains("a replacement hash requires authorization"));
    assert!(!err.to_string().contains(LABEL));
    let again = recompile(&world.artifact);
    assert_eq!(again.wrapper_hash, world.wrapper_hash);
    let draft: Value = serde_json::from_str(&again.draft).expect("draft");
    assert_eq!(draft_dataset(&draft, "effective", ORDERS)["label"], STRUCT_LABEL);
    assert_eq!(column_of(draft_dataset(&draft, "effective", ORDERS), "parcel_count")["status"], "draft");
}

#[test]
fn occupied_accept_is_not_rewritten() {
    let world = accept_ready();
    let command = pack_with(world, occupied_decisions(world), true, Some(cas_map(world)));
    let (text, packed) = apply_ok(&command);
    absent(&text);
    assert!(!text.contains(LABEL));
    assert!(!text.contains(DESCRIPTION));
    assert_eq!(packed["outcome"], "unsupported");
    assert_eq!(packed["seam"], "review.dataset_action_unsupported");
    assert!(packed.get("resealed_artifact").is_none());
    assert!(packed.get("occupied_replacements").is_none());
    assert!(packed.get("packs").is_none());
    let again = recompile(&world.artifact);
    assert_eq!(again.wrapper_hash, world.wrapper_hash);
    assert_eq!(
        review_by_id(&world.artifact, &format!("dataset/{ORDERS}"))["action"],
        "accept"
    );
    assert_eq!(
        review_by_id(&world.artifact, &format!("column/{ORDERS}/parcel_count"))["action"],
        "override"
    );
}

#[test]
fn occupied_stale_override_is_refused() {
    let world = stale_ready();
    let command = pack_with(world, occupied_decisions(world), true, Some(cas_map(world)));
    let err = apply_description_pack(&command).expect_err("stale");
    assert!(err.code.ends_with("/review.occupied_stale"), "{}", err.code);
    assert!(!err.to_string().contains(LABEL));
    assert!(!err.to_string().contains(REVIEWER));
    assert!(!err.to_string().contains(STRUCT_DESCRIPTION));
    let again = recompile(&world.artifact);
    assert_eq!(again.wrapper_hash, world.wrapper_hash);
    let draft: Value = serde_json::from_str(&again.draft).expect("draft");
    assert_ne!(draft_dataset(&draft, "effective", ORDERS)["label"], LABEL);
    assert_eq!(
        column_of(draft_dataset(&draft, "effective", ORDERS), "parcel_count")["status"],
        "draft"
    );
    let dataset = review_by_id(&world.artifact, &format!("dataset/{ORDERS}"));
    assert_eq!(dataset["action"], "override");
    assert_eq!(dataset["fact_fingerprint"], "11".repeat(32));
    assert_eq!(
        review_by_id(&world.artifact, &format!("primary_key/{ORDERS}"))["action"],
        "override"
    );
}

#[test]
fn occupied_defer_and_reject_leave_structural_overrides() {
    let world = occupied_ready();
    let outcome = outcome_of(&world.command_text);
    let orders = anchors(&outcome, ORDERS, &canonical_sha(&orders_file()));
    let ledger = anchors(&outcome, LEDGER, &canonical_sha(&ledger_file()));
    let command = pack_with(
        world,
        vec![
            decision(ORDERS, "defer", &orders, None),
            decision(LEDGER, "reject", &ledger, None),
        ],
        true,
        None,
    );
    let (text, packed) = apply_ok(&command);
    absent(&text);
    assert!(!text.contains(LABEL));
    assert!(!text.contains(DESCRIPTION));
    assert_eq!(packed["outcome"], "reviewed_unchanged");
    assert!(packed["resealed_artifact"].is_null());
    assert!(packed.get("occupied_replacements").is_none());
    assert_eq!(packed["statements"]["reseal"], "not_required");
    assert_eq!(packed["original_wrapper_hash"], world.wrapper_hash);
    assert_eq!(document_of(&packed, ORDERS)["document"], orders_file());
    let desk = canonical_text(&pack_for(&packed, "oo_r_desk")["artifact"]);
    assert!(desk.contains(STRUCT_LABEL));
    assert!(!desk.contains(LABEL));
    assert!(!desk.contains(DESCRIPTION));
    let again = recompile(&world.artifact);
    assert_eq!(again.wrapper_hash, world.wrapper_hash);
    let draft: Value = serde_json::from_str(&again.draft).expect("draft");
    assert_eq!(draft_dataset(&draft, "effective", ORDERS)["description"], STRUCT_DESCRIPTION);
    assert_eq!(draft_dataset(&draft, "effective", ORDERS)["source_name"], STRUCT_SOURCE);
    assert_eq!(
        column_of(draft_dataset(&draft, "effective", ORDERS), "parcel_count")["status"],
        "draft"
    );

    let unused = pack_with(
        world,
        vec![
            decision(ORDERS, "defer", &orders, None),
            decision(LEDGER, "reject", &ledger, None),
        ],
        true,
        Some(cas_map(world)),
    );
    let err = apply_description_pack(&unused).expect_err("unused cas");
    assert!(err.code.ends_with("/review.occupied_cas"), "{}", err.code);
    assert!(!err.to_string().contains(LABEL));
    let kept = recompile(&world.artifact);
    assert_eq!(kept.wrapper_hash, world.wrapper_hash);
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
