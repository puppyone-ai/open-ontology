//! Independent occupied-review oracle for review a315.
//!
//! The fixture is this review's. It calls the captured, reviewed-effective,
//! and role-pack compilers. It does not open a database, grant, or call a model.
//! The mixed fresh-and-occupied case is the one branch the author left untested.

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

const CAPTURED_AT: &str = "2026-05-02T11:45:00.000000Z";
const OWNER: &str = "tbl_owner";
const SOURCE: &str = "a315_src";
const DATABASE: &str = "a315db";
const MODEL: &str = "a315-sealed";
const BERTH: &str = "public.berth";
const HOLD: &str = "public.hold";
const WHARF: &str = "public.wharf";
const BERTH_CURRENT: &str = "Berth book";
const LABEL: &str = "Berth register a315";
const GRAIN: &str = "one berth call a315";
const DESCRIPTION: &str = "The bearer_token column stores a marker the tide role must not read.";
const STRUCT_LABEL: &str = "Bound berth label";
const STRUCT_GRAIN: &str = "one bound berth";
const STRUCT_DESCRIPTION: &str = "Bound berth prose";
const STRUCT_SOURCE: &str = "src_berth_bound";
const COLUMN_LABEL: &str = "Keel count bound";
const HOLD_CURRENT: &str = "Hold book";
const HOLD_LABEL: &str = "Hold register a315";
const HOLD_GRAIN: &str = "one hold row a315";
const HOLD_DESCRIPTION: &str = "Hold prose a315 is only for the keel grant.";
const HOLD_REJECT_LABEL: &str = "HOLD_CANDIDATE_a315_DO_NOT_APPLY";
const HOLD_REJECT_BODY: &str = "hold prose a315 stays out";
const ENV_CANARY: &str = "ENV_CANARY_a315_DO_NOT_LEAK";
const REVIEWER: &str = "nia.declared";
const REVIEWED_ON: &str = "2026-10-06";
const PROJECT: &str = "a315pack";
const ONTOLOGY: &str = "ont-a315";
const CATALOG_VERSION: &str = "cat-a315";
const REL_NAME: &str = "berth_hold_fk";
// Compiler curated status is only draft or rejected. Rejected leaves the
// effective list, so the preserved relationship status under test is draft.
const REL_STATUS: &str = "draft";
const REL_PREIMAGE: &str = "{\"name\":\"berth_hold_fk\",\"columns\":[{\"attnum\":4,\"name\":\"hold_id\",\"ref_schema\":\"public\",\"ref_table\":\"hold\",\"ref_attnum\":1,\"ref_name\":\"id\"}]}";

struct Col {
    attnum: i32,
    name: &'static str,
    type_oid: i64,
    type_name: &'static str,
    not_null: bool,
}

struct Fingerprints {
    schema_hash: String,
    dataset: String,
    column: String,
    primary_key: String,
    hold_column: String,
    relationship: String,
    relationship_item: String,
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

#[derive(Clone, Copy)]
enum HoldText {
    Rejected,
    Approved,
}

fn fingerprints() -> &'static Fingerprints {
    static FINGERPRINTS: OnceLock<Fingerprints> = OnceLock::new();
    FINGERPRINTS.get_or_init(|| {
        let compiled = compile_captured_pg_catalog(&catalog_json(), Some(&structural_overlay()), None)
            .unwrap_or_else(|err| panic!("structural probe failed: {} {}", err.code, err.message));
        let draft: Value = serde_json::from_str(&compiled.draft).expect("structural draft");
        let relationship = queue_kind(&draft, "candidate_relationship");
        Fingerprints {
            schema_hash: compiled.schema_hash,
            dataset: queue_detail(&draft, &format!("dataset/{BERTH}"), "curated_not_applied"),
            column: column_fingerprint(&draft, BERTH, "keel_count"),
            primary_key: queue_kind_item(&draft, &format!("primary_key/{BERTH}"), "structural_primary_key"),
            hold_column: column_fingerprint(&draft, HOLD, "id"),
            relationship: relationship.0,
            relationship_item: relationship.1,
        }
    })
}

fn occupied_ready() -> &'static Described {
    static READY: OnceLock<Described> = OnceLock::new();
    READY.get_or_init(|| describe("override", &fingerprints().dataset, HoldText::Rejected))
}

fn accept_ready() -> &'static Described {
    static READY: OnceLock<Described> = OnceLock::new();
    READY.get_or_init(|| describe("accept", &fingerprints().dataset, HoldText::Rejected))
}

fn stale_ready() -> &'static Described {
    static READY: OnceLock<Described> = OnceLock::new();
    READY.get_or_init(|| describe("override", &"11".repeat(32), HoldText::Rejected))
}

fn mixed_ready() -> &'static Described {
    static READY: OnceLock<Described> = OnceLock::new();
    READY.get_or_init(|| describe("override", &fingerprints().dataset, HoldText::Approved))
}

fn describe(action: &str, dataset_fingerprint: &str, hold: HoldText) -> Described {
    let fp = fingerprints();
    assert!(
        fp.relationship_item.ends_with(REL_NAME),
        "relationship item drifted: {}",
        fp.relationship_item
    );
    let reviews = bound_reviews(action, dataset_fingerprint, fp);
    let compiled = compile_captured_pg_catalog(
        &catalog_json(),
        Some(&structural_overlay()),
        Some(&canonical_text(&reviews)),
    )
    .unwrap_or_else(|err| panic!("bound compile failed: {} {}", err.code, err.message));
    assert_eq!(compiled.schema_hash, fp.schema_hash);
    let artifact = artifact_value(&compiled);
    let dataset_review = review_by_id(&artifact, &format!("dataset/{BERTH}")).clone();
    assert_compiler_review(&dataset_review);
    let command = candidate_command(&compiled, &artifact);
    let request_text = prepare_description_request(&serde_json::to_string(&command).expect("candidate"))
        .unwrap_or_else(|err| panic!("prepare failed: {} {}", err.code, err.message));
    let request: Value = serde_json::from_str(&request_text).expect("request");
    let (produced, outcome_sha) = finish_candidates(&command, &request, candidates(&request, hold));
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

fn candidates(request: &Value, hold: HoldText) -> Value {
    let (label, body, grain) = match hold {
        HoldText::Rejected => (HOLD_REJECT_LABEL, HOLD_REJECT_BODY, "hold grain stays out"),
        HoldText::Approved => (HOLD_LABEL, HOLD_DESCRIPTION, HOLD_GRAIN),
    };
    json!([
        candidate(request, HOLD, label, body, grain),
        candidate(request, BERTH, LABEL, DESCRIPTION, GRAIN),
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
        "tables": [
            table("berth", berth_columns(), berth_fk()),
            table("hold", hold_columns(), json!([]))
        ],
        "type_conversion": "unsupported"
    }))
    .expect("catalog")
}

fn berth_columns() -> Vec<Col> {
    vec![
        Col { attnum: 1, name: "id", type_oid: 23, type_name: "int4", not_null: true },
        Col { attnum: 2, name: "keel_count", type_oid: 23, type_name: "int4", not_null: false },
        Col { attnum: 3, name: "bearer_token", type_oid: 25, type_name: "text", not_null: false },
        Col { attnum: 4, name: "hold_id", type_oid: 23, type_name: "int4", not_null: true },
    ]
}

fn hold_columns() -> Vec<Col> {
    vec![
        Col { attnum: 1, name: "id", type_oid: 23, type_name: "int4", not_null: true },
        Col { attnum: 2, name: "secret_note", type_oid: 25, type_name: "text", not_null: false },
    ]
}

fn berth_fk() -> Value {
    json!([{
        "name": REL_NAME,
        "columns": [{
            "attnum": 4,
            "name": "hold_id",
            "ref_schema": "public",
            "ref_table": "hold",
            "ref_attnum": 1,
            "ref_name": "id"
        }]
    }])
}

fn schema_preimage() -> String {
    let mut buf = String::from("{\"tables\":[");
    write_table(&mut buf, "berth", &berth_columns(), Some(REL_PREIMAGE));
    buf.push(',');
    write_table(&mut buf, "hold", &hold_columns(), None);
    buf.push_str("]}");
    buf
}

fn write_table(buf: &mut String, name: &str, columns: &[Col], foreign: Option<&str>) {
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
    buf.push_str("}],\"primary_key_nulls_not_distinct\":false,\"unique_keys\":[],\"foreign_keys\":[");
    if let Some(foreign) = foreign {
        buf.push_str(foreign);
    }
    buf.push_str("],\"policies\":[]}");
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

fn table(name: &str, columns: Vec<Col>, foreign_keys: Value) -> Value {
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
        "foreign_keys": foreign_keys,
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

fn structural_overlay() -> String {
    serde_json::to_string(&json!({
        "datasets": [{
            "dataset": BERTH,
            "label": STRUCT_LABEL,
            "description": STRUCT_DESCRIPTION,
            "grain": STRUCT_GRAIN,
            "source_name": STRUCT_SOURCE,
            "entity": "keel",
            "primary_key": ["id"],
            "columns": [{
                "name": "keel_count",
                "label": COLUMN_LABEL,
                "pii": "sensitive",
                "semantic_type": "free_text",
                "status": "draft"
            }]
        }],
        "relationships": [{"name": REL_NAME, "status": REL_STATUS}]
    }))
    .expect("structural overlay")
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
                visible("berth", &compiled.evidence_hash, &compiled.schema_hash),
                visible("hold", &compiled.evidence_hash, &compiled.schema_hash)
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

fn berth_file() -> Value {
    dataset_file(json!({
        "schema_version": "oos/v1",
        "dataset": BERTH,
        "source_name": "src_berth",
        "label": BERTH_CURRENT,
        "description": Value::Null,
        "tier": "raw",
        "recommended": false,
        "grain": Value::Null,
        "primary_key": ["id"],
        "entity": "keel",
        "row_count_estimate": 18,
        "time_columns": [{"name": "id", "role": "created_at"}],
        "columns": [{
            "name": "keel_count",
            "label": "Keel count",
            "data_type": "int4",
            "semantic_type": "quantity",
            "pii": "personal",
            "classification": {"state": "confirmed", "by": "ops.lina", "at": "2026-10-06"},
            "status": "deprecated",
            "description": "curated keel note",
            "provenance": "human",
            "review": {"state": "approved", "by": "ken.curator", "at": "2026-10-01"}
        }],
        "caveats": ["kept berth caveat"]
    }))
}

fn hold_file() -> Value {
    dataset_file(json!({
        "schema_version": "oos/v1",
        "dataset": HOLD,
        "label": HOLD_CURRENT,
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

fn wharf_file() -> Value {
    dataset_file(json!({
        "schema_version": "oos/v1",
        "dataset": WHARF,
        "source_name": "src_wharf",
        "label": "Wharf register",
        "description": "wharf stays outside the catalog seal",
        "tier": "raw",
        "grain": "one wharf row",
        "entity": "keel",
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
        "caveats": ["wharf caveat"]
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
        BERTH: berth_file(),
        HOLD: hold_file(),
        WHARF: wharf_file(),
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

fn approve_fields(current_label: &str) -> Value {
    let current = field_sha(Some(current_label));
    assert_eq!(current, descriptive_field_sha256(Some(current_label)));
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

fn occupied_decisions(world: &Described) -> Vec<Value> {
    let outcome = outcome_of(&world.command_text);
    let berth = anchors(&outcome, BERTH, &canonical_sha(&berth_file()));
    let hold = anchors(&outcome, HOLD, &canonical_sha(&hold_file()));
    vec![
        decision(BERTH, "approve", &berth, Some(approve_fields(BERTH_CURRENT))),
        decision(HOLD, "reject", &hold, None),
    ]
}

fn mixed_decisions(world: &Described) -> Vec<Value> {
    let outcome = outcome_of(&world.command_text);
    let berth = anchors(&outcome, BERTH, &canonical_sha(&berth_file()));
    let hold = anchors(&outcome, HOLD, &canonical_sha(&hold_file()));
    vec![
        decision(BERTH, "approve", &berth, Some(approve_fields(BERTH_CURRENT))),
        decision(HOLD, "approve", &hold, Some(approve_fields(HOLD_CURRENT))),
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
        "reviewer": {"by": "nia.owner", "at": "2026-10-06"}
    })
}

fn column_fingerprint(draft: &Value, dataset: &str, column: &str) -> String {
    queue_kind_item(draft, &format!("column/{dataset}/{column}"), "structural_column")
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

fn queue_kind_item(draft: &Value, item_id: &str, kind: &str) -> String {
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

fn queue_kind(draft: &Value, kind: &str) -> (String, String) {
    let item = draft["review_queue"]
        .as_array()
        .expect("queue")
        .iter()
        .find(|item| item["kind"] == kind)
        .unwrap_or_else(|| panic!("missing kind {kind}"));
    (
        item["fact_fingerprint"].as_str().expect("fingerprint").to_owned(),
        item["item_id"].as_str().expect("item").to_owned(),
    )
}

fn classifications(draft: &Value) -> Vec<Value> {
    vec![
        class_decision(draft, BERTH, "id", "none", "identifier"),
        class_decision(draft, BERTH, "keel_count", "restricted", "quantity"),
        class_decision(draft, BERTH, "bearer_token", "restricted", "free_text"),
        class_decision(draft, HOLD, "id", "none", "identifier"),
    ]
}

fn settings() -> Value {
    json!({
        "project": PROJECT,
        "ontology_version": ONTOLOGY,
        "catalog_version": CATALOG_VERSION,
        "policy": {"id": "pol-a315", "version": "1"},
        "knowledge": {"id": "know-a315", "version": "1"},
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
        "policy": {"id": "pol-a315", "version": "1"},
        "knowledge": {"id": "know-a315", "version": "1"},
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
        visibility("oo_r_tide", "tide_user", json!([grant(BERTH, &["id", "keel_count"])])),
        visibility(
            "oo_r_keel",
            "keel_user",
            json!([
                grant(BERTH, &["id", "keel_count", "bearer_token"]),
                grant(HOLD, &["id"])
            ])
        )
    ])
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
    serde_json::to_string(&command).expect("pack")
}

fn cas_map(world: &Described) -> Value {
    json!({ format!("dataset/{BERTH}"): world.dataset_review_sha })
}

fn bound_reviews(action: &str, dataset_fingerprint: &str, fp: &Fingerprints) -> Value {
    json!([
        {
            "action": "accept",
            "fact_fingerprint": fp.hold_column,
            "item_id": format!("column/{HOLD}/id"),
            "schema_hash": fp.schema_hash
        },
        {
            "action": "override",
            "fact_fingerprint": fp.column,
            "item_id": format!("column/{BERTH}/keel_count"),
            "schema_hash": fp.schema_hash
        },
        {
            "action": action,
            "fact_fingerprint": dataset_fingerprint,
            "item_id": format!("dataset/{BERTH}"),
            "schema_hash": fp.schema_hash
        },
        {
            "action": "override",
            "fact_fingerprint": fp.primary_key,
            "item_id": format!("primary_key/{BERTH}"),
            "schema_hash": fp.schema_hash
        },
        {
            "action": "override",
            "fact_fingerprint": fp.relationship,
            "item_id": fp.relationship_item,
            "schema_hash": fp.schema_hash
        }
    ])
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

fn relationship_of<'a>(draft: &'a Value, layer: &str, name: &str) -> &'a Value {
    draft[layer]["relationships"]
        .as_array()
        .unwrap_or_else(|| panic!("missing {layer} relationships"))
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("missing relationship {name} in {layer}"))
}

fn pack_for<'a>(packed: &'a Value, role: &str) -> &'a Value {
    packed["packs"]
        .as_array()
        .expect("packs")
        .iter()
        .find(|item| item["role"] == role)
        .unwrap_or_else(|| panic!("missing pack {role}"))
}

fn review_by_id<'a>(artifact: &'a Value, item_id: &str) -> &'a Value {
    artifact["evidence_envelope"]["reviews"]
        .as_array()
        .expect("reviews")
        .iter()
        .find(|item| item["item_id"] == item_id)
        .unwrap_or_else(|| panic!("missing review {item_id}"))
}

fn assert_compiler_review(review: &Value) {
    let object = review.as_object().expect("review object");
    assert_eq!(object.len(), 4, "{review}");
    assert!(object.contains_key("item_id"));
    assert!(object.contains_key("action"));
    assert!(object.contains_key("schema_hash"));
    assert!(object.contains_key("fact_fingerprint"));
    assert!(review.get("reviewer_label").is_none());
    assert!(review.get("reviewer_at").is_none());
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
    compile_captured_pg_catalog(
        &canonical_text(&envelope["catalog"]),
        optional_canonical(envelope.get("curated")).as_deref(),
        optional_canonical(envelope.get("reviews")).as_deref(),
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

fn quiet(text: &str) {
    assert!(!text.contains(HOLD_REJECT_LABEL), "rejected hold label leaked");
    assert!(!text.contains(HOLD_REJECT_BODY), "rejected hold prose leaked");
    assert!(!text.contains(ENV_CANARY), "env canary leaked");
}

fn refusal_is_quiet(err: &description_pack::DescriptionPackError, world: &Described) {
    let text = err.to_string();
    assert!(!text.contains(LABEL), "{text}");
    assert!(!text.contains(DESCRIPTION), "{text}");
    assert!(!text.contains(REVIEWER), "{text}");
    assert!(!text.contains(STRUCT_LABEL), "{text}");
    assert!(!text.contains(&world.dataset_review_sha), "{text}");
    assert!(!text.contains(ENV_CANARY), "{text}");
    quiet(&text);
}

fn assert_prior_seal(world: &Described) {
    let again = recompile(&world.artifact);
    assert_eq!(again.wrapper_hash, world.wrapper_hash);
    let draft: Value = serde_json::from_str(&again.draft).expect("draft");
    assert_eq!(draft_dataset(&draft, "effective", BERTH)["label"], STRUCT_LABEL);
    assert_eq!(draft_dataset(&draft, "effective", BERTH)["source_name"], STRUCT_SOURCE);
    assert_eq!(draft_dataset(&draft, "effective", BERTH)["primary_key"], json!(["id"]));
    assert_eq!(
        column_of(draft_dataset(&draft, "effective", BERTH), "keel_count")["status"],
        "draft"
    );
    assert_eq!(relationship_of(&draft, "effective", REL_NAME)["status"], REL_STATUS);
    assert_eq!(relationship_of(&draft, "generated", REL_NAME)["status"], "draft");
}

fn preserved_reviews(before: &Value, after: &Value) {
    let prior = before["evidence_envelope"]["reviews"].as_array().expect("prior reviews");
    let next = after["evidence_envelope"]["reviews"].as_array().expect("next reviews");
    for review in prior {
        assert_compiler_review(review);
        let text = canonical_text(review);
        assert!(
            next.iter().any(|item| canonical_text(item) == text),
            "dropped review {}",
            review["item_id"]
        );
    }
}

fn assert_structural_unchanged(world: &Described, packed: &Value, resealed_draft: &Value) {
    let effective = draft_dataset(resealed_draft, "effective", BERTH);
    assert_eq!(effective["source_name"], STRUCT_SOURCE);
    assert_eq!(effective["primary_key"], json!(["id"]));
    assert!(effective.get("entity").is_none_or(Value::is_null));
    let keel = column_of(effective, "keel_count");
    assert_eq!(keel["label"], COLUMN_LABEL);
    assert_eq!(keel["status"], "draft");
    assert!(keel["pii"].is_null());
    let curated = draft_dataset(resealed_draft, "curated", BERTH);
    assert_eq!(curated["entity"], "keel");
    assert_eq!(column_of(curated, "keel_count")["pii"], "sensitive");
    assert_eq!(column_of(curated, "keel_count")["semantic_type"], "free_text");
    assert_eq!(column_of(curated, "keel_count")["status"], "draft");
    assert_eq!(
        packed["resealed_artifact"]["evidence_envelope"]["curated"]["relationships"],
        world.artifact["evidence_envelope"]["curated"]["relationships"]
    );
    assert_eq!(relationship_of(resealed_draft, "curated", REL_NAME)["status"], REL_STATUS);
    assert_eq!(relationship_of(resealed_draft, "effective", REL_NAME)["status"], REL_STATUS);
    assert_eq!(relationship_of(resealed_draft, "generated", REL_NAME)["status"], "draft");
    assert_eq!(
        canonical_sha(review_by_id(&packed["resealed_artifact"], &fingerprints().relationship_item)),
        canonical_sha(review_by_id(&world.artifact, &fingerprints().relationship_item))
    );
    assert_eq!(
        canonical_sha(review_by_id(&packed["resealed_artifact"], &format!("primary_key/{BERTH}"))),
        canonical_sha(review_by_id(&world.artifact, &format!("primary_key/{BERTH}")))
    );
    assert_eq!(
        review_by_id(&packed["resealed_artifact"], &format!("column/{HOLD}/id"))["action"],
        "accept"
    );
    assert_eq!(
        review_by_id(&packed["resealed_artifact"], &format!("column/{BERTH}/keel_count"))["action"],
        "override"
    );
    preserved_reviews(&world.artifact, &packed["resealed_artifact"]);
}

fn assert_provenance(packed: &Value, world: &Described) {
    let rows = packed["occupied_replacements"].as_array().expect("replacements");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row["attribution"], "bridge_provenance");
    assert_eq!(row["item_id"], format!("dataset/{BERTH}"));
    assert_eq!(row["prior_review"], world.dataset_review);
    assert_compiler_review(&row["prior_review"]);
    assert_eq!(row["prior_review_sha256"], world.dataset_review_sha);
    assert_eq!(row["compiler_review_sha256"], world.dataset_review_sha);
    assert_eq!(row["descriptive_provenance"]["reviewer_label"], REVIEWER);
    assert_eq!(row["descriptive_provenance"]["reviewer_at"], REVIEWED_ON);
    assert_eq!(row["descriptive_provenance"]["authenticated"], false);
    assert_eq!(row["descriptive_provenance"]["fields"], json!(["description", "grain", "label"]));
    let seal = canonical_text(&packed["resealed_artifact"]);
    assert!(!seal.contains(REVIEWER));
    assert!(!seal.contains("reviewer_label"));
    assert!(!seal.contains("reviewer_at"));
    assert!(!seal.contains("ken.curator"));
    assert!(!seal.contains("nia.owner"));
    assert!(!seal.contains("bridge_provenance"));
    assert!(!seal.contains(STRUCT_LABEL));
    assert!(!seal.contains(STRUCT_DESCRIPTION));
    assert!(!seal.contains(STRUCT_GRAIN));
}

fn assert_documents(packed: &Value) {
    let berth = document_of(packed, BERTH);
    let file: DatasetFile = serde_json::from_value(berth["document"].clone()).expect("dataset file");
    assert!(validate_dataset(&file).is_empty());
    assert_eq!(berth["document"]["label"], LABEL);
    assert_eq!(berth["document"]["description"], DESCRIPTION);
    assert_eq!(berth["document"]["grain"], GRAIN);
    assert_eq!(berth["document"]["entity"], "keel");
    assert_eq!(berth["document"]["row_count_estimate"], 18);
    assert_eq!(column_of(&berth["document"], "keel_count")["pii"], "personal");
    assert_eq!(column_of(&berth["document"], "keel_count")["status"], "deprecated");
    assert_eq!(column_of(&berth["document"], "keel_count")["classification"]["by"], "ops.lina");
    assert_eq!(column_of(&berth["document"], "keel_count")["review"]["by"], "ken.curator");
    assert_eq!(document_of(packed, WHARF)["document"], wharf_file());
}

fn assert_downstream(packed: &Value, command: &str, hold_applied: bool) {
    let resealed = recompile(&packed["resealed_artifact"]);
    assert_eq!(resealed.wrapper_hash, packed["resealed_wrapper_hash"]);
    assert_eq!(resealed.draft_hash, packed["resealed_draft_hash"]);
    assert_eq!(resealed.evidence_envelope_hash, packed["resealed_evidence_envelope_hash"]);
    let submitted: Value = serde_json::from_str(command).expect("submitted");
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
    let reviewed = column_of(dataset_row(&effective_doc["effective_oos"]["datasets"], BERTH), "keel_count");
    assert_eq!(reviewed["pii"], "restricted");
    assert_eq!(reviewed["semantic_type"], "quantity");
    assert_eq!(reviewed["classification"]["by"], "nia.owner");
    assert_eq!(reviewed["classification"]["state"], "confirmed");
    let oos = canonical_text(&effective_doc["effective_oos"]);
    check_role(packed, "oo_r_tide", &oos, &two_roles()[0], false, false);
    check_role(packed, "oo_r_keel", &oos, &two_roles()[1], true, hold_applied);
}

fn check_role(packed: &Value, role: &str, oos: &str, visibility: &Value, description_visible: bool, hold_applied: bool) {
    let compiled = compile_role_pack(oos, &canonical_text(visibility))
        .unwrap_or_else(|err| panic!("role pack failed: {} {}", err.code, err.message));
    let record = pack_for(packed, role);
    assert_eq!(record["pack_digest"], compiled.pack_digest);
    assert_eq!(record["manifest_sha256"], compiled.manifest_sha256);
    assert_eq!(record["cache_key_sha256"], compiled.cache_key_sha256);
    assert_eq!(record["artifact_sha256"], payload_sha256(compiled.artifact.as_bytes()));
    let bytes = compiled.artifact;
    assert!(bytes.contains(LABEL), "{role} lost the approved label");
    assert!(bytes.contains(GRAIN), "{role} lost the approved grain");
    assert!(!bytes.contains(STRUCT_LABEL), "{role} kept the structural label");
    assert!(!bytes.contains(STRUCT_DESCRIPTION), "{role} kept the structural prose");
    assert!(!bytes.contains("secret_note"), "{role} rendered secret_note");
    assert!(!bytes.contains(HOLD_REJECT_LABEL), "{role} rendered the rejected hold candidate");
    assert!(!bytes.contains(REVIEWER), "{role} invented a compiler reviewer");
    assert!(!bytes.contains("bridge_provenance"), "{role} copied bridge provenance");
    if description_visible {
        assert!(bytes.contains(DESCRIPTION), "{role} dropped an allowlisted description");
        assert!(bytes.contains("UNREVIEWED"), "{role} bypassed the description fence");
        assert!(bytes.contains("bearer_token"), "{role} dropped an allowlisted name");
    } else {
        assert!(!bytes.contains(DESCRIPTION), "{role} kept a hidden-name description");
        assert!(!bytes.contains("bearer_token"), "{role} rendered bearer_token");
    }
    if hold_applied && description_visible {
        assert!(bytes.contains(HOLD_LABEL), "{role} dropped the fresh hold label");
        assert!(bytes.contains(HOLD_DESCRIPTION), "{role} dropped the fresh hold prose");
    } else {
        assert!(!bytes.contains(HOLD_LABEL), "{role} rendered hold label");
        assert!(!bytes.contains(HOLD_DESCRIPTION), "{role} rendered hold prose");
    }
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

#[test]
fn occupied_override_reseals_approved_fields_only() {
    let world = occupied_ready();
    assert_prior_seal(world);
    let command = pack_with(world, occupied_decisions(world), true, Some(cas_map(world)));
    let library = apply_description_pack(&command)
        .unwrap_or_else(|err| panic!("pack failed: {} {}", err.code, err.message));
    let output = run_cli(&["apply"], command.as_bytes());
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout, library.as_bytes());
    assert!(output.stderr.is_empty());
    let packed: Value = serde_json::from_str(&library).expect("pack");
    assert_bound_hash(&library, &packed);
    quiet(&library);
    assert_eq!(packed["protocol"], PACK_PROTOCOL);
    assert_eq!(packed["compiler"], PACK_COMPILER);
    assert_eq!(packed["outcome"], "packed");
    assert_eq!(packed["statements"]["reseal"], "compiler");
    assert_eq!(packed["statements"]["authentication"], "not_claimed");
    assert_eq!(packed["statements"]["authorization"], "not_claimed");
    assert_eq!(packed["statements"]["database"], "not_used");
    assert_eq!(packed["statements"]["grants"], "not_authorized");
    assert_eq!(packed["statements"]["llm"], "not_used");
    assert_eq!(packed["statements"]["publication"], "unavailable");
    assert_eq!(packed["statements"]["semantic_publication"], "not_claimed");
    assert_eq!(packed["statements"]["source_artifact"], "not_mutated");
    assert_eq!(packed["original_wrapper_hash"], world.wrapper_hash);
    assert_ne!(packed["resealed_wrapper_hash"], world.wrapper_hash);
    assert_provenance(&packed, world);
    assert_eq!(
        canonical_text(&packed["resealed_artifact"]["evidence_envelope"]["reviews"]),
        canonical_text(&world.artifact["evidence_envelope"]["reviews"])
    );
    assert_documents(&packed);
    assert_eq!(document_of(&packed, HOLD)["document"], hold_file());
    let resealed_draft: Value = serde_json::from_str(
        &recompile(&packed["resealed_artifact"]).draft,
    )
    .expect("resealed draft");
    let effective = draft_dataset(&resealed_draft, "effective", BERTH);
    assert_eq!(effective["label"], LABEL);
    assert_eq!(effective["description"], DESCRIPTION);
    assert_eq!(effective["grain"], GRAIN);
    assert_structural_unchanged(world, &packed, &resealed_draft);
    assert_downstream(&packed, &command, false);
    assert_prior_seal(world);
}

#[test]
fn flag_absent_and_cas_refusals_keep_the_prior_seal() {
    let world = occupied_ready();
    let plain = pack_with(world, occupied_decisions(world), false, None);
    let (text, packed) = apply_ok(&plain);
    quiet(&text);
    assert!(!text.contains(LABEL));
    assert!(!text.contains(DESCRIPTION));
    assert_eq!(packed["outcome"], "unsupported");
    assert_eq!(packed["seam"], "review.dataset_decision_occupied");
    assert!(packed.get("occupied_replacements").is_none());
    assert!(packed.get("packs").is_none());
    assert!(packed.get("resealed_artifact").is_none());
    assert_eq!(packed["statements"]["reseal"], "unsupported");
    assert_eq!(packed["statements"]["source_artifact"], "not_mutated");
    assert_prior_seal(world);

    let mut wrong = cas_map(world);
    wrong[format!("dataset/{BERTH}")] = json!("ab".repeat(32));
    let mut extra = cas_map(world);
    extra[format!("dataset/{HOLD}")] = json!(world.dataset_review_sha.clone());
    let cases = [
        pack_with(world, occupied_decisions(world), true, Some(wrong)),
        pack_with(world, occupied_decisions(world), true, Some(json!({}))),
        pack_with(world, occupied_decisions(world), true, Some(extra)),
    ];
    for command in cases {
        let err = apply_description_pack(&command).expect_err("cas");
        assert!(err.code.ends_with("/review.occupied_cas"), "{}", err.code);
        refusal_is_quiet(&err, world);
        let output = run_cli(&["apply"], command.as_bytes());
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("description-pack:"), "{stderr}");
        assert!(stderr.contains("review.occupied_cas"), "{stderr}");
        assert!(!stderr.contains(LABEL), "{stderr}");
        assert!(!stderr.contains(REVIEWER), "{stderr}");
        assert!(!stderr.contains(ENV_CANARY), "{stderr}");
    }
    let unauthorized = pack_with(world, occupied_decisions(world), false, Some(cas_map(world)));
    let err = apply_description_pack(&unauthorized).expect_err("flag");
    assert!(err.code.ends_with("/schema.invalid"), "{}", err.code);
    assert!(err.to_string().contains("a replacement hash requires authorization"));
    refusal_is_quiet(&err, world);
    assert_prior_seal(world);
}

#[test]
fn accept_and_stale_fail_closed() {
    let world = accept_ready();
    let command = pack_with(world, occupied_decisions(world), true, Some(cas_map(world)));
    let (text, packed) = apply_ok(&command);
    quiet(&text);
    assert!(!text.contains(LABEL));
    assert_eq!(packed["outcome"], "unsupported");
    assert_eq!(packed["seam"], "review.dataset_action_unsupported");
    assert!(packed.get("resealed_artifact").is_none());
    assert!(packed.get("occupied_replacements").is_none());
    assert!(packed.get("packs").is_none());
    assert_eq!(review_by_id(&world.artifact, &format!("dataset/{BERTH}"))["action"], "accept");
    assert_eq!(
        review_by_id(&world.artifact, &format!("column/{BERTH}/keel_count"))["action"],
        "override"
    );
    let again = recompile(&world.artifact);
    assert_eq!(again.wrapper_hash, world.wrapper_hash);
    let draft: Value = serde_json::from_str(&again.draft).expect("draft");
    assert_ne!(draft_dataset(&draft, "effective", BERTH)["label"], LABEL);
    assert_eq!(draft_dataset(&draft, "effective", BERTH)["primary_key"], json!(["id"]));
    assert_eq!(
        column_of(draft_dataset(&draft, "effective", BERTH), "keel_count")["status"],
        "draft"
    );
    assert_eq!(relationship_of(&draft, "effective", REL_NAME)["status"], REL_STATUS);

    let stale = stale_ready();
    let command = pack_with(stale, occupied_decisions(stale), true, Some(cas_map(stale)));
    let err = apply_description_pack(&command).expect_err("stale");
    assert!(err.code.ends_with("/review.occupied_stale"), "{}", err.code);
    refusal_is_quiet(&err, stale);
    let again = recompile(&stale.artifact);
    assert_eq!(again.wrapper_hash, stale.wrapper_hash);
    let draft: Value = serde_json::from_str(&again.draft).expect("draft");
    assert_ne!(draft_dataset(&draft, "effective", BERTH)["label"], LABEL);
    assert_eq!(column_of(draft_dataset(&draft, "effective", BERTH), "keel_count")["status"], "draft");
    assert_eq!(relationship_of(&draft, "effective", REL_NAME)["status"], REL_STATUS);
    let dataset = review_by_id(&stale.artifact, &format!("dataset/{BERTH}"));
    assert_eq!(dataset["action"], "override");
    assert_eq!(dataset["fact_fingerprint"], "11".repeat(32));
    assert_eq!(review_by_id(&stale.artifact, &format!("primary_key/{BERTH}"))["action"], "override");
}

#[test]
fn reject_and_defer_leave_structural_fields() {
    let world = occupied_ready();
    let outcome = outcome_of(&world.command_text);
    let berth = anchors(&outcome, BERTH, &canonical_sha(&berth_file()));
    let hold = anchors(&outcome, HOLD, &canonical_sha(&hold_file()));
    let decisions = vec![
        decision(BERTH, "defer", &berth, None),
        decision(HOLD, "reject", &hold, None),
    ];
    let command = pack_with(world, decisions.clone(), true, None);
    let (text, packed) = apply_ok(&command);
    quiet(&text);
    assert!(!text.contains(LABEL));
    assert!(!text.contains(DESCRIPTION));
    assert_eq!(packed["outcome"], "reviewed_unchanged");
    assert!(packed["resealed_artifact"].is_null());
    assert!(packed.get("occupied_replacements").is_none());
    assert_eq!(packed["statements"]["reseal"], "not_required");
    assert_eq!(packed["original_wrapper_hash"], world.wrapper_hash);
    assert_eq!(document_of(&packed, BERTH)["document"], berth_file());
    let tide = canonical_text(&pack_for(&packed, "oo_r_tide")["artifact"]);
    assert!(tide.contains(STRUCT_LABEL));
    assert!(!tide.contains(LABEL));
    assert!(!tide.contains(DESCRIPTION));
    assert_prior_seal(world);

    let unused = pack_with(world, decisions, true, Some(cas_map(world)));
    let err = apply_description_pack(&unused).expect_err("unused cas");
    assert!(err.code.ends_with("/review.occupied_cas"), "{}", err.code);
    refusal_is_quiet(&err, world);
    assert_prior_seal(world);
}

#[test]
fn mixed_fresh_and_occupied_approvals_reseal_once() {
    let world = mixed_ready();
    let before = review_by_id(&world.artifact, &format!("dataset/{BERTH}")).clone();
    assert!(world.artifact["evidence_envelope"]["reviews"]
        .as_array()
        .expect("reviews")
        .iter()
        .all(|item| item["item_id"] != format!("dataset/{HOLD}")));
    let prior_len = world.artifact["evidence_envelope"]["reviews"].as_array().expect("reviews").len();
    let command = pack_with(world, mixed_decisions(world), true, Some(cas_map(world)));
    let library = apply_description_pack(&command)
        .unwrap_or_else(|err| panic!("mixed pack failed: {} {}", err.code, err.message));
    let output = run_cli(&["apply"], command.as_bytes());
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout, library.as_bytes());
    assert!(output.stderr.is_empty());
    let packed: Value = serde_json::from_str(&library).expect("pack");
    assert_bound_hash(&library, &packed);
    quiet(&library);
    assert_eq!(packed["outcome"], "packed");
    assert_eq!(packed["original_wrapper_hash"], world.wrapper_hash);
    assert_ne!(packed["resealed_wrapper_hash"], world.wrapper_hash);
    assert_provenance(&packed, world);
    let reviews = packed["resealed_artifact"]["evidence_envelope"]["reviews"]
        .as_array()
        .expect("reviews");
    assert_eq!(reviews.len(), prior_len + 1);
    preserved_reviews(&world.artifact, &packed["resealed_artifact"]);
    let fresh = review_by_id(&packed["resealed_artifact"], &format!("dataset/{HOLD}"));
    assert_compiler_review(fresh);
    assert_eq!(fresh["action"], "override");
    assert_eq!(fresh["schema_hash"], world.schema_hash);
    assert_ne!(fresh["fact_fingerprint"], before["fact_fingerprint"]);
    assert!(reviews.iter().any(|item| canonical_text(item) == canonical_text(&before)));
    let resealed = recompile(&packed["resealed_artifact"]);
    assert_eq!(resealed.wrapper_hash, packed["resealed_wrapper_hash"]);
    assert_eq!(resealed.draft_hash, packed["resealed_draft_hash"]);
    assert_eq!(resealed.evidence_envelope_hash, packed["resealed_evidence_envelope_hash"]);
    let resealed_draft: Value = serde_json::from_str(&resealed.draft).expect("resealed draft");
    assert_eq!(draft_dataset(&resealed_draft, "effective", BERTH)["label"], LABEL);
    assert_eq!(draft_dataset(&resealed_draft, "effective", BERTH)["description"], DESCRIPTION);
    assert_eq!(draft_dataset(&resealed_draft, "effective", BERTH)["grain"], GRAIN);
    assert_eq!(draft_dataset(&resealed_draft, "effective", HOLD)["label"], HOLD_LABEL);
    assert_eq!(draft_dataset(&resealed_draft, "effective", HOLD)["description"], HOLD_DESCRIPTION);
    assert_eq!(draft_dataset(&resealed_draft, "effective", HOLD)["grain"], HOLD_GRAIN);
    assert_structural_unchanged(world, &packed, &resealed_draft);
    assert_eq!(document_of(&packed, HOLD)["document"]["label"], HOLD_LABEL);
    assert_eq!(document_of(&packed, HOLD)["document"]["description"], HOLD_DESCRIPTION);
    assert_eq!(document_of(&packed, WHARF)["document"], wharf_file());
    assert_downstream(&packed, &command, true);
    let original = recompile(&world.artifact);
    assert_eq!(original.wrapper_hash, world.wrapper_hash);
}
