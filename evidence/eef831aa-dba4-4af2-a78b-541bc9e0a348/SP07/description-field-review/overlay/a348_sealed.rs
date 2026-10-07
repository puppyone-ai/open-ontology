//! Independent contract oracle for the column-candidate producer.
//!
//! The fixture is local. It does not open a database or call a model.
//! Hashes are computed here. They are not copied from the author tests.

#![forbid(unsafe_code)]

use std::sync::OnceLock;

use description_pack::{COMMAND_PROTOCOL, apply_description_pack};
use description_review::{
    REVIEW_PROTOCOL_V2, apply_description_review, curated_dataset_sha256, descriptive_field_sha256,
};
use oo_builder::{
    DESCRIPTION_PROMPT_VERSION, DESCRIPTION_REQUEST_COMMAND_PROTOCOL, DESCRIPTION_RESPONSE_PROTOCOL,
    compile_captured_pg_catalog, compile_role_pack, complete_description_candidates,
    prepare_description_request,
};
use oo_core::api::payload_sha256;
use oo_core::oos::{DatasetFile, validate_dataset};
use serde_json::{Map, Value, json};

const CAPTURED_AT: &str = "2026-03-20T08:15:00.000000Z";
const OWNER: &str = "tbl_owner";
const SOURCE: &str = "a348_src";
const DATABASE: &str = "a348db";
const MODEL: &str = "a348-captured";
const LANE: &str = "public.lane";
const BAY: &str = "public.bay";
const QUAY: &str = "public.quay";
const PROMPT_V2: &str = "oo.builder.description-prompt/v2";
const RESPONSE_V2: &str = "oo.builder.description-response/v2";
const PROMPT_V2_SHA: &str = "ec5355303ad18ce189e9a297677a55f9f6317e1ab59d553b057d05c81f6278c2";
const REVIEWER: &str = "nia.declared";
const REVIEWED_ON: &str = "2026-10-08";
const PROJECT: &str = "a348pack";
const ONTOLOGY: &str = "ont-a348";
const CATALOG_VERSION: &str = "cat-a348";
const CRATE_OLD_LABEL: &str = "Crate count kept";
const CRATE_OLD_BODY: &str = "kept crate note";
const BADGE_OLD_LABEL: &str = "Badge kept";
const CRATE_LABEL: &str = "Lane crate label a348";
const CRATE_BODY: &str = "Lane crate body a348 names badge_code in prose";
const CRATE_LABEL_2: &str = "Lane crate label two a348";
const CRATE_BODY_2: &str = "Lane crate body two a348";
const BADGE_LABEL: &str = "Lane badge label a348";
const BADGE_BODY: &str = "Lane badge body a348 personal gate";
const MARK_LABEL: &str = "Bay mark label a348";
const MARK_BODY: &str = "Bay mark body a348 hidden gate";
const CONFIDENCE: &str = "0.25";
const CAVEAT: &str = "a348 caveat stays on the queue";
const QUESTION: &str = "a348 question stays on the queue";
const CLASS_CANARY: &str = "a348_CLASS_DO_NOT_ADMIT";
const KEPT_REVIEW: &str = r#"{"at":"2026-10-01","by":"ken.curator","state":"approved"}"#;

struct Col {
    attnum: i32,
    name: &'static str,
    type_oid: i64,
    type_name: &'static str,
    not_null: bool,
}

struct Phys {
    dataset: String,
    name: String,
    attnum: i64,
    card_sha: String,
}

struct World {
    artifact: Value,
    draft: Value,
    wrapper_hash: String,
    schema_hash: String,
    evidence_hash: String,
    request: Value,
    produced_text: String,
    outcome_text: String,
    outcome: Value,
    outcome_sha: String,
}

fn world() -> &'static World {
    static WORLD: OnceLock<World> = OnceLock::new();
    WORLD.get_or_init(|| {
        let compiled = compile_captured_pg_catalog(&catalog_json(), None, None)
            .unwrap_or_else(|err| panic!("catalog compile failed: {} {}", err.code, err.message));
        let artifact = artifact_value(&compiled);
        let draft: Value = serde_json::from_str(&compiled.draft).expect("draft");
        let (produced_text, outcome_text, outcome, request) = bind_v2(&artifact, &compiled);
        let outcome_sha = payload_sha256(outcome_text.as_bytes());
        World {
            artifact,
            draft,
            wrapper_hash: compiled.wrapper_hash,
            schema_hash: compiled.schema_hash,
            evidence_hash: compiled.evidence_hash,
            request,
            produced_text,
            outcome_text,
            outcome_sha,
            outcome,
        }
    })
}

fn lane_columns() -> Vec<Col> {
    vec![
        Col { attnum: 1, name: "id", type_oid: 23, type_name: "int4", not_null: true },
        Col { attnum: 2, name: "crate_count", type_oid: 23, type_name: "int4", not_null: false },
        Col { attnum: 3, name: "badge_code", type_oid: 25, type_name: "text", not_null: false },
    ]
}

fn bay_columns() -> Vec<Col> {
    vec![
        Col { attnum: 1, name: "id", type_oid: 23, type_name: "int4", not_null: true },
        Col { attnum: 2, name: "private_mark", type_oid: 25, type_name: "text", not_null: false },
    ]
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
        "tables": [table("bay", bay_columns()), table("lane", lane_columns())],
        "type_conversion": "unsupported"
    }))
    .expect("catalog")
}

fn schema_preimage() -> String {
    let mut tables = [("bay", bay_columns()), ("lane", lane_columns())];
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

fn artifact_value(compiled: &oo_builder::CapturedPgDraft) -> Value {
    json!({
        "draft": serde_json::from_str::<Value>(&compiled.draft).expect("draft"),
        "evidence_envelope": serde_json::from_str::<Value>(&compiled.evidence_envelope).expect("envelope"),
        "protocol": "oo.builder.captured-pg-draft-command/v1",
        "wrapper": serde_json::from_str::<Value>(&compiled.wrapper).expect("wrapper")
    })
}

fn budget() -> Value {
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
        "max_response_bytes": 65536,
        "max_tables": 16
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

fn command_for(compiled: &oo_builder::CapturedPgDraft, artifact: &Value, prompt: &str) -> Value {
    json!({
        "admitted": true,
        "artifact": artifact,
        "binding": {
            "model_id": MODEL,
            "prompt_version": prompt,
            "source_id": compiled.source_id
        },
        "budget": budget(),
        "protocol": DESCRIPTION_REQUEST_COMMAND_PROTOCOL,
        "selection": {
            "evidence_hash": compiled.evidence_hash,
            "schema_hash": compiled.schema_hash,
            "tables": [
                visible("bay", &compiled.evidence_hash, &compiled.schema_hash),
                visible("lane", &compiled.evidence_hash, &compiled.schema_hash)
            ]
        }
    })
}

fn bind_v2(
    artifact: &Value,
    compiled: &oo_builder::CapturedPgDraft,
) -> (String, String, Value, Value) {
    let bare = command_for(compiled, artifact, PROMPT_V2);
    let bare_text = serde_json::to_string(&bare).expect("bare");
    let request_text = prepare_description_request(&bare_text)
        .unwrap_or_else(|err| panic!("prepare failed: {} {}", err.code, err.message));
    let request: Value = serde_json::from_str(&request_text).expect("request");
    let columns = physical(&request);
    let response = response_v2(&request, dataset_bodies(&request), labeled_columns(&columns));
    let (produced_text, outcome_text, outcome) = finish(bare, response);
    (produced_text, outcome_text, outcome, request)
}

fn physical(request: &Value) -> Vec<Phys> {
    let mut out = Vec::new();
    for card in request["boundary"]["data"]["tables"].as_array().expect("cards") {
        let dataset = card["dataset"].as_str().expect("dataset").to_owned();
        let card_sha = card["card_sha256"].as_str().expect("card").to_owned();
        for column in card["columns"].as_array().expect("columns") {
            out.push(Phys {
                dataset: dataset.clone(),
                name: column["name"].as_str().expect("name").to_owned(),
                attnum: column["attnum"].as_i64().expect("attnum"),
                card_sha: card_sha.clone(),
            });
        }
    }
    out
}

fn column_input(
    column: &Phys,
    label: &str,
    description: &str,
    confidence: &str,
    caveats: Value,
    questions: Value,
) -> Value {
    json!({
        "attnum": column.attnum,
        "card_sha256": column.card_sha,
        "caveats": caveats,
        "column": column.name,
        "confidence": confidence,
        "dataset": column.dataset,
        "description": description,
        "label": label,
        "open_questions": questions
    })
}

fn labeled_columns(columns: &[Phys]) -> Value {
    Value::Array(
        columns
            .iter()
            .map(|column| {
                if column.dataset == LANE && column.name == "crate_count" {
                    column_input(
                        column,
                        CRATE_LABEL,
                        CRATE_BODY,
                        CONFIDENCE,
                        json!([CAVEAT]),
                        json!([QUESTION]),
                    )
                } else if column.dataset == LANE && column.name == "badge_code" {
                    column_input(column, BADGE_LABEL, BADGE_BODY, "0", json!([]), json!([]))
                } else if column.dataset == BAY && column.name == "private_mark" {
                    column_input(column, MARK_LABEL, MARK_BODY, "0", json!([]), json!([]))
                } else {
                    column_input(
                        column,
                        &format!("kept {} {}", column.dataset, column.name),
                        "kept body",
                        "0",
                        json!([]),
                        json!([]),
                    )
                }
            })
            .collect(),
    )
}

fn dataset_bodies(request: &Value) -> Value {
    Value::Array(
        request["boundary"]["data"]["tables"]
            .as_array()
            .expect("cards")
            .iter()
            .map(|card| {
                json!({
                    "card_sha256": card["card_sha256"],
                    "caveats": [],
                    "confidence": "0",
                    "dataset": card["dataset"],
                    "description": "dataset prose stays off the column",
                    "grain": "one local row",
                    "label": "dataset label stays off the column",
                    "open_questions": []
                })
            })
            .collect(),
    )
}

fn response_v2(request: &Value, candidates: Value, columns: Value) -> Value {
    json!({
        "budget_sha256": request["budget_sha256"],
        "candidates": candidates,
        "columns": columns,
        "complete": true,
        "evidence_hash": request["evidence_hash"],
        "model_id": request["model_id"],
        "prompt_sha256": request["prompt_sha256"],
        "prompt_version": request["prompt_version"],
        "protocol": RESPONSE_V2,
        "request_sha256": request["request_sha256"],
        "schema_hash": request["schema_hash"],
        "source_id": request["source_id"]
    })
}

fn finish(mut command: Value, response: Value) -> (String, String, Value) {
    let body = serde_json::to_string(&response).expect("response");
    command["captured_response"] = json!({
        "body": body,
        "byte_len": body.len(),
        "status": "complete"
    });
    let produced_text = serde_json::to_string(&command).expect("produced");
    let outcome_text = complete_description_candidates(&produced_text)
        .unwrap_or_else(|err| panic!("complete failed: {} {}", err.code, err.message));
    let outcome: Value = serde_json::from_str(&outcome_text).expect("outcome");
    (produced_text, outcome_text, outcome)
}

fn dataset_file(value: Value) -> Value {
    let file: DatasetFile = serde_json::from_value(value.clone())
        .unwrap_or_else(|err| panic!("dataset file did not parse: {err} value={value}"));
    assert!(validate_dataset(&file).is_empty(), "dataset file failed validation");
    let typed = serde_json::to_value(&file).expect("typed dataset");
    assert_eq!(canonical_sha(&typed), curated_dataset_sha256(&file));
    typed
}

fn lane_file() -> Value {
    dataset_file(json!({
        "schema_version": "oos/v1",
        "dataset": LANE,
        "source_name": "src_lane",
        "label": "Lane register",
        "description": Value::Null,
        "tier": "raw",
        "recommended": false,
        "grain": Value::Null,
        "primary_key": ["id"],
        "entity": "crate",
        "columns": [
            {
                "name": "id",
                "label": "Id",
                "data_type": "int4",
                "semantic_type": "identifier",
                "pii": "none"
            },
            {
                "name": "crate_count",
                "label": CRATE_OLD_LABEL,
                "data_type": "int4",
                "semantic_type": "quantity",
                "pii": "personal",
                "classification": {"state": "confirmed", "by": "ops.wang", "at": "2026-10-06"},
                "status": "deprecated",
                "description": CRATE_OLD_BODY,
                "provenance": "human",
                "review": {"state": "approved", "by": "ken.curator", "at": "2026-10-01"}
            },
            {
                "name": "badge_code",
                "label": BADGE_OLD_LABEL,
                "data_type": "text",
                "semantic_type": "free_text",
                "pii": "restricted",
                "classification": {"state": "confirmed", "by": "ops.wang", "at": "2026-10-06"}
            }
        ],
        "caveats": ["lane caveat stays"]
    }))
}

fn bay_file() -> Value {
    dataset_file(json!({
        "schema_version": "oos/v1",
        "dataset": BAY,
        "label": "Bay register",
        "tier": "raw",
        "columns": [
            {
                "name": "id",
                "label": "Id",
                "data_type": "int4",
                "semantic_type": "identifier",
                "pii": "none"
            },
            {
                "name": "private_mark",
                "label": "Private mark kept",
                "data_type": "text"
            }
        ]
    }))
}

fn quay_file() -> Value {
    dataset_file(json!({
        "schema_version": "oos/v1",
        "dataset": QUAY,
        "label": "Quay register",
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

fn curated_map(lane: &Value, bay: &Value) -> Value {
    json!({
        LANE: lane,
        BAY: bay,
        QUAY: quay_file(),
    })
}

fn outcome_column<'a>(outcome: &'a Value, dataset: &str, name: &str) -> &'a Value {
    outcome["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|item| item["dataset"] == dataset && item["column"] == name)
        .unwrap_or_else(|| panic!("missing outcome column {dataset}.{name}"))
}

fn column_decision(
    world: &World,
    column: &Phys,
    kind: &str,
    curated_hash: &str,
    fields: Option<Value>,
) -> Value {
    let object = outcome_column(&world.outcome, &column.dataset, &column.name);
    let mut value = json!({
        "attnum": column.attnum,
        "candidate_sha256": canonical_sha(object),
        "card_sha256": object["card_sha256"],
        "column": column.name,
        "curated_sha256": curated_hash,
        "dataset": column.dataset,
        "decision": kind,
        "evidence_hash": world.outcome["structural_draft"]["evidence_hash"],
        "request_sha256": world.outcome["request_sha256"],
        "reviewer_at": REVIEWED_ON,
        "reviewer_label": REVIEWER,
        "schema_hash": world.outcome["structural_draft"]["schema_hash"]
    });
    if let Some(fields) = fields {
        value["fields"] = fields;
    }
    value
}

fn dataset_decision(world: &World, dataset: &str, kind: &str, curated_hash: &str) -> Value {
    let candidate = world.outcome["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .find(|item| item["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing dataset candidate {dataset}"));
    json!({
        "candidate_sha256": canonical_sha(candidate),
        "card_sha256": candidate["card_sha256"],
        "curated_sha256": curated_hash,
        "dataset": dataset,
        "decision": kind,
        "evidence_hash": world.outcome["structural_draft"]["evidence_hash"],
        "request_sha256": world.outcome["request_sha256"],
        "reviewer_at": REVIEWED_ON,
        "reviewer_label": REVIEWER,
        "schema_hash": world.outcome["structural_draft"]["schema_hash"]
    })
}

fn overwrite_fields(label: Option<&str>, description: Option<&str>) -> Value {
    json!({
        "description": {"overwrite": true, "current_sha256": descriptive_field_sha256(description)},
        "label": {"overwrite": true, "current_sha256": descriptive_field_sha256(label)}
    })
}

fn review_command(world: &World, lane: &Value, bay: &Value, columns: Vec<Value>) -> String {
    let lane_hash = canonical_sha(lane);
    let bay_hash = canonical_sha(bay);
    serde_json::to_string(&json!({
        "column_decisions": columns,
        "command": world.produced_text,
        "current": {"evidence_hash": world.evidence_hash, "schema_hash": world.schema_hash},
        "curated": curated_map(lane, bay),
        "decisions": [
            dataset_decision(world, LANE, "reject", &lane_hash),
            dataset_decision(world, BAY, "reject", &bay_hash)
        ],
        "expected_outcome_sha256": world.outcome_sha,
        "protocol": description_review::COMMAND_PROTOCOL
    }))
    .expect("review")
}

fn positive_review(world: &World) -> (String, Value, Value) {
    let lane = lane_file();
    let bay = bay_file();
    let lane_hash = canonical_sha(&lane);
    let bay_hash = canonical_sha(&bay);
    let columns = physical(&world.request);
    let mut decisions = Vec::new();
    for column in &columns {
        let hash = if column.dataset == LANE { lane_hash.as_str() } else { bay_hash.as_str() };
        let decision = if column.dataset == LANE && column.name == "crate_count" {
            column_decision(
                world,
                column,
                "approve",
                hash,
                Some(overwrite_fields(Some(CRATE_OLD_LABEL), Some(CRATE_OLD_BODY))),
            )
        } else if column.dataset == LANE && column.name == "badge_code" {
            column_decision(
                world,
                column,
                "approve",
                hash,
                Some(overwrite_fields(Some(BADGE_OLD_LABEL), None)),
            )
        } else if column.name == "id" && column.dataset == LANE {
            column_decision(world, column, "defer", hash, None)
        } else {
            column_decision(world, column, "reject", hash, None)
        };
        decisions.push(decision);
    }
    (review_command(world, &lane, &bay, decisions), lane, bay)
}

fn settings(tokens: &[&str]) -> Value {
    json!({
        "project": PROJECT,
        "ontology_version": ONTOLOGY,
        "catalog_version": CATALOG_VERSION,
        "policy": {"id": "pol-a348", "version": "1"},
        "knowledge": {"id": "know-a348", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": PROJECT,
            "locale": "en-US",
            "include_schemas": ["public"],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": tokens},
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
        "policy": {"id": "pol-a348", "version": "1"},
        "knowledge": {"id": "know-a348", "version": "1"},
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
        visibility("oo_r_north", "north_user", json!([grant(LANE, &["id", "crate_count"])])),
        visibility(
            "oo_r_south",
            "south_user",
            json!([
                grant(LANE, &["id", "crate_count", "badge_code"]),
                grant(BAY, &["id"])
            ])
        )
    ])
}

fn class_decision(draft: &Value, dataset: &str, column: &str, pii: &str, semantic: &str) -> Value {
    json!({
        "dataset": dataset,
        "column": column,
        "schema_hash": draft["provenance"]["schema_hash"],
        "fact_fingerprint": column_fingerprint(draft, dataset, column),
        "pii": pii,
        "semantic_type": semantic,
        "reviewer": {"by": "nia.owner", "at": "2026-10-08"}
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
        class_decision(draft, LANE, "id", "none", "identifier"),
        class_decision(draft, LANE, "crate_count", "none", "quantity"),
        class_decision(draft, LANE, "badge_code", "personal", "free_text"),
        class_decision(draft, BAY, "id", "none", "identifier"),
    ]
}

fn pack_command(
    review: &str,
    artifact: &Value,
    decisions: Value,
    roles: Value,
    tokens: &[&str],
    replace_column: bool,
    expected_column: Option<Value>,
) -> String {
    let review_bytes = apply_description_review(review)
        .unwrap_or_else(|err| panic!("review failed: {} {}", err.code, err.message));
    let mut command = json!({
        "artifact": artifact,
        "decisions": decisions,
        "expected_review_sha256": payload_sha256(review_bytes.as_bytes()),
        "protocol": COMMAND_PROTOCOL,
        "review": review,
        "roles": roles,
        "settings": settings(tokens)
    });
    if replace_column {
        command["replace_existing_column_review"] = json!(true);
    }
    if let Some(expected) = expected_column {
        command["expected_existing_column_review_sha256"] = expected;
    }
    serde_json::to_string(&command).expect("pack")
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

fn role_text(packed: &Value, role: &str) -> String {
    canonical_text(&pack_for(packed, role)["artifact"])
}

fn review_by_id<'a>(artifact: &'a Value, item_id: &str) -> &'a Value {
    artifact["evidence_envelope"]["reviews"]
        .as_array()
        .expect("reviews")
        .iter()
        .find(|item| item["item_id"] == item_id)
        .unwrap_or_else(|| panic!("missing review {item_id}"))
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

fn find_phys<'a>(columns: &'a [Phys], dataset: &str, name: &str) -> &'a Phys {
    columns
        .iter()
        .find(|column| column.dataset == dataset && column.name == name)
        .unwrap_or_else(|| panic!("missing {dataset}.{name}"))
}

fn decision_row<'a>(review: &'a Value, column: &str) -> &'a Value {
    review["column_decisions"]
        .as_array()
        .expect("column decisions")
        .iter()
        .find(|item| item["column"] == column)
        .unwrap_or_else(|| panic!("missing decision {column}"))
}

fn queue_has(review: &Value, column: &str, reason: &str) -> bool {
    review["queue"].as_array().expect("queue").iter().any(|item| {
        item["column"] == column && item["reason"] == reason && item["kind"] == "pending"
    })
}

fn absent_review_only(text: &str) {
    assert!(!text.contains(CONFIDENCE), "confidence leaked: {text}");
    assert!(!text.contains(CAVEAT), "caveat leaked");
    assert!(!text.contains(QUESTION), "question leaked");
    assert!(!text.contains(CLASS_CANARY), "classification canary leaked");
}

fn effective_column<'a>(packed: &'a Value, dataset: &str, name: &str) -> &'a Value {
    let datasets = &packed["effective"]["effective_oos"]["datasets"];
    column_of(
        datasets
            .as_array()
            .expect("effective datasets")
            .iter()
            .find(|item| item["dataset"] == dataset)
            .unwrap_or_else(|| panic!("missing effective {dataset}")),
        name,
    )
}

fn assert_compiler_role(packed: &Value, role_name: &str, role: &Value) {
    let effective = &packed["effective"]["effective_oos"];
    let fresh = compile_role_pack(&canonical_text(effective), &canonical_text(role))
        .unwrap_or_else(|err| panic!("role compile failed: {} {}", err.code, err.message));
    let row = pack_for(packed, role_name);
    assert_eq!(row["pack_digest"], fresh.pack_digest);
    assert_eq!(row["artifact_sha256"], payload_sha256(fresh.artifact.as_bytes()));
    let parsed: Value = serde_json::from_str(&fresh.artifact).expect("role artifact");
    assert_eq!(parsed, row["artifact"]);
}

fn kept_review() -> Value {
    serde_json::from_str(KEPT_REVIEW).expect("kept review")
}

#[test]
fn explicit_column_review_reseals_two_roles_without_forging_review() {
    let world = world();
    assert_eq!(world.request["prompt_sha256"], PROMPT_V2_SHA);
    assert_eq!(world.request["prompt_version"], PROMPT_V2);
    assert!(!world.request.to_string().contains("sample_values"));
    assert_eq!(world.outcome["outcome"], "candidates");
    assert_eq!(world.outcome["protocol"], "oo.builder.description-candidate/v2");
    assert_eq!(world.outcome["statements"]["pii_classification"], "not_performed");
    assert_eq!(world.outcome["statements"]["auto_accept"], "not_authorized");
    assert_eq!(world.outcome["statements"]["confidence_effect"], "recorded_only");
    assert_eq!(world.outcome["columns"].as_array().expect("columns").len(), 5);
    let columns = physical(&world.request);
    let crate_col = find_phys(&columns, LANE, "crate_count");
    let admitted = outcome_column(&world.outcome, LANE, "crate_count");
    assert_eq!(admitted["attnum"], crate_col.attnum);
    assert_eq!(admitted["card_sha256"], crate_col.card_sha);
    assert_ne!(admitted["card_sha256"], world.evidence_hash);
    assert_eq!(admitted["ident"], "crate_count");
    assert_eq!(admitted["column"], "crate_count");
    assert_eq!(admitted["effect"], "review_only");
    assert_eq!(admitted["confidence"], CONFIDENCE);
    assert!(admitted.get("pii").is_none());
    assert!(admitted.get("review").is_none());

    let (review, _lane, _bay) = positive_review(world);
    let reviewed = apply_description_review(&review)
        .unwrap_or_else(|err| panic!("review {} {}", err.code, err.message));
    let reviewed_value: Value = serde_json::from_str(&reviewed).expect("review json");
    assert_eq!(reviewed_value["protocol"], REVIEW_PROTOCOL_V2);
    assert_eq!(reviewed_value["statements"]["pii_classification"], "not_performed");
    assert_eq!(reviewed_value["statements"]["column_acl"], "not_changed");
    assert_eq!(reviewed_value["statements"]["authentication"], "not_claimed");
    let crate_row = decision_row(&reviewed_value, "crate_count");
    assert_eq!(crate_row["applied"], true);
    assert_eq!(crate_row["authenticated"], false);
    assert_eq!(crate_row["confidence_effect"], "recorded_only");
    assert_eq!(crate_row["candidate_confidence"], CONFIDENCE);
    assert_eq!(crate_row["effect"], "review_only");
    assert_eq!(crate_row["label_disposition"], "replaced");
    assert_eq!(crate_row["description_disposition"], "replaced");
    assert!(queue_has(&reviewed_value, "crate_count", "caveats"));
    assert!(queue_has(&reviewed_value, "crate_count", "open_questions"));
    let badge_row = decision_row(&reviewed_value, "badge_code");
    assert_eq!(badge_row["applied"], true);
    assert_eq!(badge_row["authenticated"], false);
    let lane_doc = reviewed_value["datasets"]
        .as_array()
        .expect("datasets")
        .iter()
        .find(|item| item["dataset"] == LANE)
        .expect("lane");
    let written = column_of(&lane_doc["document"], "crate_count");
    assert_eq!(written["label"], CRATE_LABEL);
    assert_eq!(written["description"], CRATE_BODY);
    assert_eq!(written["pii"], "personal");
    assert_eq!(written["status"], "deprecated");
    assert_eq!(written["classification"]["state"], "confirmed");
    assert_eq!(written["review"], kept_review());
    assert!(written.get("confidence").is_none());
    assert!(written.get("caveats").is_none());
    assert!(written.get("open_questions").is_none());
    let badge = column_of(&lane_doc["document"], "badge_code");
    assert_eq!(badge["label"], BADGE_LABEL);
    assert_eq!(badge["description"], BADGE_BODY);
    assert_eq!(badge["pii"], "restricted");
    assert!(badge.get("review").is_none() || badge["review"].is_null());
    assert_eq!(lane_doc["document"]["caveats"], json!(["lane caveat stays"]));
    assert_eq!(lane_doc["document"]["label"], "Lane register");
    let quay = reviewed_value["datasets"]
        .as_array()
        .expect("datasets")
        .iter()
        .find(|item| item["dataset"] == QUAY)
        .expect("quay");
    assert_eq!(quay["changed"], false);
    assert_eq!(quay["document"], quay_file());

    let classes = Value::Array(classifications(&world.draft));
    let roles = two_roles();
    let plain = pack_command(&review, &world.artifact, classes.clone(), roles.clone(), &["dataset_descriptions"], false, None);
    let (plain_text, plain_packed) = apply_ok(&plain);
    absent_review_only(&plain_text);
    assert_eq!(plain_packed["outcome"], "packed");
    assert_eq!(plain_packed["statements"]["reseal"], "compiler");
    assert_eq!(plain_packed["statements"]["llm"], "not_used");
    assert_eq!(plain_packed["statements"]["source_artifact"], "not_mutated");
    assert_eq!(plain_packed["original_wrapper_hash"], world.wrapper_hash);
    assert_ne!(plain_packed["resealed_wrapper_hash"], world.wrapper_hash);
    let again = recompile(&world.artifact);
    assert_eq!(again.wrapper_hash, world.wrapper_hash);
    let resealed = recompile(&plain_packed["resealed_artifact"]);
    assert_eq!(resealed.wrapper_hash, plain_packed["resealed_wrapper_hash"]);
    let north = role_text(&plain_packed, "oo_r_north");
    let south = role_text(&plain_packed, "oo_r_south");
    assert!(north.contains(CRATE_LABEL), "north lost the visible label");
    assert!(!north.contains(CRATE_BODY), "unreviewed body published without the token");
    assert!(!north.contains("UNREVIEWED:"), "a missing token was marked as an unreviewed grant");
    assert!(!north.contains(BADGE_LABEL));
    assert!(!north.contains(BADGE_BODY));
    assert!(!north.contains("badge_code"));
    assert!(!north.contains(MARK_BODY));
    assert!(!north.contains("private_mark"));
    assert!(south.contains(CRATE_LABEL));
    assert!(south.contains(BADGE_LABEL));
    assert!(!south.contains(CRATE_BODY), "south published the body without the token");
    assert!(!south.contains(BADGE_BODY));
    assert!(!south.contains(MARK_BODY));
    assert!(!south.contains("private_mark"));
    assert_eq!(effective_column(&plain_packed, LANE, "crate_count")["description"], CRATE_BODY);
    assert!(effective_column(&plain_packed, LANE, "crate_count").get("review").is_none() || effective_column(&plain_packed, LANE, "crate_count")["review"].is_null());
    assert_eq!(effective_column(&plain_packed, LANE, "crate_count")["pii"], "none");
    assert_eq!(effective_column(&plain_packed, LANE, "badge_code")["pii"], "personal");
    assert_eq!(column_of(&document_of(&plain_packed, LANE)["document"], "crate_count")["review"], kept_review());
    let role_values = roles.as_array().expect("roles");
    assert_compiler_role(&plain_packed, "oo_r_north", &role_values[0]);
    assert_compiler_role(&plain_packed, "oo_r_south", &role_values[1]);
    assert_ne!(pack_for(&plain_packed, "oo_r_north")["pack_digest"], pack_for(&plain_packed, "oo_r_south")["pack_digest"]);

    let opted = pack_command(
        &review,
        &world.artifact,
        classes,
        roles,
        &["column_descriptions", "dataset_descriptions"],
        false,
        None,
    );
    let (opted_text, opted_packed) = apply_ok(&opted);
    absent_review_only(&opted_text);
    let north = role_text(&opted_packed, "oo_r_north");
    let south = role_text(&opted_packed, "oo_r_south");
    assert!(north.contains(CRATE_LABEL));
    assert!(!north.contains(CRATE_BODY), "hidden-column prose crossed the north role");
    assert!(!north.contains(BADGE_BODY));
    assert!(!north.contains("badge_code"));
    assert!(!north.contains(MARK_BODY));
    assert!(south.contains(&format!("UNREVIEWED: {CRATE_BODY}")));
    assert!(south.contains(&format!("UNREVIEWED: {BADGE_BODY}")));
    assert!(south.contains("badge_code"));
    assert!(south.contains("Samples are omitted because pii is personal."));
    assert!(!south.contains(MARK_BODY));
    assert!(!south.contains("private_mark"));
    assert!(!south.contains("not an approved review, and not a grant.\n\nLane crate"));
    assert_eq!(column_of(&document_of(&opted_packed, LANE)["document"], "crate_count")["review"], kept_review());
    assert!(effective_column(&opted_packed, LANE, "badge_code").get("review").is_none() || effective_column(&opted_packed, LANE, "badge_code")["review"].is_null());

    let bogus = pack_command(&review, &world.artifact, json!([]), two_roles(), &["descriptions"], false, None);
    let err = apply_description_pack(&bogus).expect_err("unsupported token");
    assert!(err.code.contains("include_unreviewed.unsupported"), "{}", err.code);
    assert!(err.message.contains("was not treated as a grant"));
    assert!(!err.to_string().contains(CRATE_BODY));
    assert!(!err.to_string().contains(BADGE_BODY));
    let duplicate = pack_command(
        &review,
        &world.artifact,
        json!([]),
        two_roles(),
        &["column_descriptions", "column_descriptions"],
        false,
        None,
    );
    let err = apply_description_pack(&duplicate).expect_err("duplicate token");
    assert!(err.code.contains("include_unreviewed.duplicate"), "{}", err.code);
    assert!(!err.to_string().contains(CRATE_BODY));
    let kept = recompile(&world.artifact);
    assert_eq!(kept.wrapper_hash, world.wrapper_hash);
}

#[test]
fn v1_missing_extra_and_failures_write_nothing() {
    let world = world();
    let compiled = recompile(&world.artifact);
    let v1_bare = command_for(&compiled, &world.artifact, DESCRIPTION_PROMPT_VERSION);
    let v1_text = serde_json::to_string(&v1_bare).expect("v1 bare");
    let v1_request_text = prepare_description_request(&v1_text)
        .unwrap_or_else(|err| panic!("v1 prepare {} {}", err.code, err.message));
    let v1_request: Value = serde_json::from_str(&v1_request_text).expect("v1 request");
    assert_eq!(v1_request["prompt_version"], DESCRIPTION_PROMPT_VERSION);
    assert_ne!(v1_request["prompt_sha256"], PROMPT_V2_SHA);
    let mut v1_response = response_v2(&v1_request, dataset_bodies(&v1_request), json!([]));
    v1_response["protocol"] = json!(DESCRIPTION_RESPONSE_PROTOCOL);
    v1_response["prompt_version"] = json!(DESCRIPTION_PROMPT_VERSION);
    v1_response["prompt_sha256"] = v1_request["prompt_sha256"].clone();
    v1_response.as_object_mut().expect("object").remove("columns");
    let (_produced, outcome_text, outcome) = finish(v1_bare.clone(), v1_response);
    assert_eq!(outcome["protocol"], "oo.builder.description-candidate/v1");
    assert!(outcome.get("columns").is_none(), "v1 outcome gained columns");
    assert_eq!(outcome["outcome"], "candidates");
    assert!(!outcome_text.contains(CRATE_BODY));

    let mut with_columns = response_v2(&v1_request, dataset_bodies(&v1_request), labeled_columns(&physical(&world.request)));
    with_columns["protocol"] = json!(DESCRIPTION_RESPONSE_PROTOCOL);
    with_columns["prompt_version"] = json!(DESCRIPTION_PROMPT_VERSION);
    with_columns["prompt_sha256"] = v1_request["prompt_sha256"].clone();
    let (_produced, rejected_text, rejected) = finish(v1_bare, with_columns);
    assert_eq!(rejected["outcome"], "format_rejected");
    assert_eq!(rejected["refusal"], "unknown_key");
    assert!(rejected.get("columns").is_none());
    assert_eq!(rejected["candidates"], json!([]));
    assert!(!rejected_text.contains(CRATE_BODY));

    let v1_outcome_sha = payload_sha256(outcome_text.as_bytes());
    let mut v1_command = v1_bare_command(&compiled, &world.artifact);
    let v1_produced = {
        let body = serde_json::to_string(&{
            let mut response = response_v2(&v1_request, dataset_bodies(&v1_request), json!([]));
            response["protocol"] = json!(DESCRIPTION_RESPONSE_PROTOCOL);
            response["prompt_version"] = json!(DESCRIPTION_PROMPT_VERSION);
            response["prompt_sha256"] = v1_request["prompt_sha256"].clone();
            response.as_object_mut().expect("object").remove("columns");
            response
        })
        .expect("body");
        v1_command["captured_response"] = json!({"body": body, "byte_len": body.len(), "status": "complete"});
        serde_json::to_string(&v1_command).expect("v1 produced")
    };
    let lane = lane_file();
    let bay = bay_file();
    let lane_hash = canonical_sha(&lane);
    let review = serde_json::to_string(&json!({
        "column_decisions": [{
            "attnum": 2,
            "candidate_sha256": "aa".repeat(32),
            "card_sha256": "aa".repeat(32),
            "column": "crate_count",
            "curated_sha256": lane_hash,
            "dataset": LANE,
            "decision": "reject",
            "evidence_hash": world.evidence_hash,
            "request_sha256": "aa".repeat(32),
            "reviewer_at": REVIEWED_ON,
            "reviewer_label": REVIEWER,
            "schema_hash": world.schema_hash
        }],
        "command": v1_produced,
        "current": {"evidence_hash": world.evidence_hash, "schema_hash": world.schema_hash},
        "curated": curated_map(&lane, &bay),
        "decisions": [],
        "expected_outcome_sha256": v1_outcome_sha,
        "protocol": description_review::COMMAND_PROTOCOL
    }))
    .expect("v1 review");
    let err = apply_description_review(&review).expect_err("v1 column decision");
    assert!(err.code.ends_with("/decision.unknown"), "{}", err.code);
    assert!(!err.to_string().contains(CRATE_BODY));

    assert_refusal_shape(world, mutate_columns(world, |columns| { columns.pop(); }), "incomplete");
    assert_refusal_shape(world, mutate_columns(world, |columns| {
        columns.push(json!({
            "attnum": 9,
            "card_sha256": "ab".repeat(32),
            "caveats": [],
            "column": "invented",
            "confidence": "0",
            "dataset": LANE,
            "description": MARK_BODY,
            "label": MARK_LABEL,
            "open_questions": []
        }));
    }), "incomplete");
    assert_refusal_shape(world, mutate_columns(world, |columns| {
        let crate_row = columns.iter_mut().find(|item| item["column"] == "crate_count").expect("crate");
        crate_row["attnum"] = json!(9);
    }), "incomplete");
    let card_miss = mutate_columns(world, |columns| {
        let crate_row = columns.iter_mut().find(|item| item["column"] == "crate_count").expect("crate");
        crate_row["card_sha256"] = json!("cd".repeat(32));
    });
    let (_produced, _text, card_outcome) = complete_mutated(world, card_miss);
    assert_eq!(card_outcome["outcome"], "format_rejected");
    assert_eq!(card_outcome["refusal"], "evidence_identity");
    assert_eq!(card_outcome["columns"], json!([]));
    assert_eq!(card_outcome["candidates"], json!([]));
    let classed = mutate_columns(world, |columns| {
        let crate_row = columns.iter_mut().find(|item| item["column"] == "crate_count").expect("crate");
        crate_row["pii"] = json!(CLASS_CANARY);
    });
    let (_produced, _text, class_outcome) = complete_mutated(world, classed);
    assert_eq!(class_outcome["outcome"], "format_rejected");
    assert_eq!(class_outcome["refusal"], "unknown_key");
    assert_eq!(class_outcome["columns"], json!([]));
    assert!(!serde_json::to_string(&class_outcome).expect("json").contains(CLASS_CANARY));

    let mut failed = command_for(&compiled, &world.artifact, PROMPT_V2);
    failed["captured_response"] = json!({"status": "provider_failure"});
    let failed_text = serde_json::to_string(&failed).expect("failed");
    let failed_outcome_text = complete_description_candidates(&failed_text).expect("failure outcome");
    let failed_outcome: Value = serde_json::from_str(&failed_outcome_text).expect("failure json");
    assert_eq!(failed_outcome["outcome"], "provider_failure");
    assert_eq!(failed_outcome["candidates"], json!([]));
    assert_eq!(failed_outcome["columns"], json!([]));
    assert!(!failed_outcome_text.contains(CRATE_BODY));
    let failed_world = failure_world(world, failed_text, failed_outcome_text, failed_outcome);
    let failed_review = reject_review(&failed_world);
    let (failed_pack_text, failed_pack) = apply_ok(&pack_command(
        &failed_review,
        &world.artifact,
        json!([]),
        two_roles(),
        &["dataset_descriptions"],
        false,
        None,
    ));
    assert_eq!(failed_pack["outcome"], "not_candidates");
    assert!(failed_pack.get("resealed_artifact").is_none());
    assert!(failed_pack.get("packs").is_none());
    assert!(!failed_pack_text.contains(CRATE_BODY));
    assert!(!failed_pack_text.contains(BADGE_BODY));
    assert_eq!(recompile(&world.artifact).wrapper_hash, world.wrapper_hash);
}

fn v1_bare_command(compiled: &oo_builder::CapturedPgDraft, artifact: &Value) -> Value {
    command_for(compiled, artifact, DESCRIPTION_PROMPT_VERSION)
}

fn mutate_columns(world: &World, change: impl FnOnce(&mut Vec<Value>)) -> Value {
    let mut columns = labeled_columns(&physical(&world.request));
    let list = columns.as_array_mut().expect("columns");
    change(list);
    columns
}

fn complete_mutated(world: &World, columns: Value) -> (String, String, Value) {
    let compiled = recompile(&world.artifact);
    let bare = command_for(&compiled, &world.artifact, PROMPT_V2);
    let request = world.request.clone();
    let response = response_v2(&request, dataset_bodies(&request), columns);
    finish(bare, response)
}

fn assert_refusal_shape(world: &World, columns: Value, outcome_name: &str) {
    let (produced, outcome_text, outcome) = complete_mutated(world, columns);
    assert_eq!(outcome["outcome"], outcome_name);
    assert_eq!(outcome["candidates"], json!([]));
    assert_eq!(outcome["columns"], json!([]));
    assert!(!outcome_text.contains(CRATE_BODY));
    assert!(!outcome_text.contains(MARK_BODY));
    let failed_world = failure_world(world, produced, outcome_text, outcome);
    let review = reject_review(&failed_world);
    let (packed_text, packed) = apply_ok(&pack_command(
        &review,
        &world.artifact,
        json!([]),
        two_roles(),
        &["dataset_descriptions"],
        false,
        None,
    ));
    assert_eq!(packed["outcome"], "not_candidates");
    assert!(packed.get("resealed_artifact").is_none());
    assert!(packed.get("packs").is_none());
    assert!(!packed_text.contains(CRATE_BODY));
    assert_eq!(recompile(&world.artifact).wrapper_hash, world.wrapper_hash);
}

fn failure_world(world: &World, produced_text: String, outcome_text: String, outcome: Value) -> World {
    World {
        artifact: world.artifact.clone(),
        draft: world.draft.clone(),
        wrapper_hash: world.wrapper_hash.clone(),
        schema_hash: world.schema_hash.clone(),
        evidence_hash: world.evidence_hash.clone(),
        request: world.request.clone(),
        produced_text,
        outcome_text,
        outcome,
        outcome_sha: String::new(),
    }
}

fn reject_review(world: &World) -> String {
    let lane = lane_file();
    let bay = bay_file();
    let lane_hash = canonical_sha(&lane);
    serde_json::to_string(&json!({
        "column_decisions": [{
            "attnum": 2,
            "candidate_sha256": "aa".repeat(32),
            "card_sha256": "aa".repeat(32),
            "column": "crate_count",
            "curated_sha256": lane_hash,
            "dataset": LANE,
            "decision": "reject",
            "evidence_hash": world.evidence_hash,
            "request_sha256": "aa".repeat(32),
            "reviewer_at": REVIEWED_ON,
            "reviewer_label": REVIEWER,
            "schema_hash": world.schema_hash
        }],
        "command": world.produced_text,
        "current": {"evidence_hash": world.evidence_hash, "schema_hash": world.schema_hash},
        "curated": curated_map(&lane, &bay),
        "decisions": [{
            "candidate_sha256": "aa".repeat(32),
            "card_sha256": "aa".repeat(32),
            "curated_sha256": lane_hash,
            "dataset": LANE,
            "decision": "reject",
            "evidence_hash": world.evidence_hash,
            "request_sha256": "aa".repeat(32),
            "reviewer_at": REVIEWED_ON,
            "reviewer_label": REVIEWER,
            "schema_hash": world.schema_hash
        }],
        "expected_outcome_sha256": payload_sha256(world.outcome_text.as_bytes()),
        "protocol": description_review::COMMAND_PROTOCOL
    }))
    .expect("reject review")
}

#[test]
fn occupied_cas_stale_defer_and_unknown_pii_fail_closed() {
    let world = world();
    let (review, _lane, bay) = positive_review(world);
    let classes = Value::Array(classifications(&world.draft));
    let command = pack_command(
        &review,
        &world.artifact,
        classes.clone(),
        two_roles(),
        &["dataset_descriptions"],
        false,
        None,
    );
    let (_text, packed) = apply_ok(&command);
    let item_id = format!("column/{LANE}/crate_count");
    let prior = review_by_id(&packed["resealed_artifact"], &item_id).clone();
    assert_eq!(prior["action"], "override");
    assert_eq!(
        prior.as_object().expect("review").keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["action", "fact_fingerprint", "item_id", "schema_hash"]
    );
    let prior_sha = canonical_sha(&prior);
    assert!(!serde_json::to_string(&prior).expect("prior").contains(REVIEWER));

    let resealed_artifact = packed["resealed_artifact"].clone();
    let second = bind_second(&resealed_artifact);
    let second_lane = with_crate_text(CRATE_LABEL, CRATE_BODY);
    let second_hash = canonical_sha(&second_lane);
    let second_bay_hash = canonical_sha(&bay);
    let second_columns = physical(&second.request);
    let second_review = review_command_custom(
        &second,
        &second_lane,
        &bay,
        second_columns.iter().map(|column| {
            let hash = if column.dataset == LANE { second_hash.as_str() } else { second_bay_hash.as_str() };
            if column.dataset == LANE && column.name == "crate_count" {
                column_decision(
                    &second,
                    column,
                    "approve",
                    hash,
                    Some(overwrite_fields(Some(CRATE_LABEL), Some(CRATE_BODY))),
                )
            } else if column.name == "id" && column.dataset == LANE {
                column_decision(&second, column, "defer", hash, None)
            } else {
                column_decision(&second, column, "reject", hash, None)
            }
        }).collect(),
    );
    let absent = pack_command(
        &second_review,
        &resealed_artifact,
        classes.clone(),
        two_roles(),
        &["dataset_descriptions"],
        false,
        None,
    );
    let (absent_text, denied) = apply_ok(&absent);
    assert_eq!(denied["outcome"], "unsupported");
    assert_eq!(denied["seam"], "review.column_decision_occupied");
    assert!(denied.get("resealed_artifact").is_none());
    assert!(denied.get("packs").is_none());
    assert!(!absent_text.contains(CRATE_LABEL_2));
    assert!(!absent_text.contains(CRATE_BODY_2));

    let wrong = pack_command(
        &second_review,
        &resealed_artifact,
        classes.clone(),
        two_roles(),
        &["dataset_descriptions"],
        true,
        Some(json!({ item_id.clone(): "ab".repeat(32) })),
    );
    let err = apply_description_pack(&wrong).expect_err("wrong cas");
    assert!(err.code.ends_with("/review.occupied_cas"), "{}", err.code);
    assert!(!err.to_string().contains(CRATE_BODY_2));
    let extra = pack_command(
        &second_review,
        &resealed_artifact,
        classes.clone(),
        two_roles(),
        &["dataset_descriptions"],
        true,
        Some(json!({
            item_id.clone(): prior_sha,
            "column/public.bay/private_mark": prior_sha
        })),
    );
    let err = apply_description_pack(&extra).expect_err("extra cas");
    assert!(err.code.ends_with("/review.occupied_cas"), "{}", err.code);
    let unauthorized = pack_command(
        &second_review,
        &resealed_artifact,
        classes.clone(),
        two_roles(),
        &["dataset_descriptions"],
        false,
        Some(json!({ item_id.clone(): prior_sha })),
    );
    let err = apply_description_pack(&unauthorized).expect_err("flag");
    assert!(err.code.ends_with("/schema.invalid"), "{}", err.code);
    assert!(err.message.contains("a replacement hash requires authorization"));
    assert!(!err.to_string().contains(CRATE_BODY));

    let replaced = replace_crate_candidate(&second, &resealed_artifact);
    let replaced_lane = with_crate_text(CRATE_LABEL, CRATE_BODY);
    let replaced_hash = canonical_sha(&replaced_lane);
    let replaced_review = review_command_custom(
        &replaced,
        &replaced_lane,
        &bay,
        physical(&replaced.request).iter().map(|column| {
            let hash = if column.dataset == LANE { replaced_hash.as_str() } else { second_bay_hash.as_str() };
            if column.dataset == LANE && column.name == "crate_count" {
                column_decision(
                    &replaced,
                    column,
                    "approve",
                    hash,
                    Some(overwrite_fields(Some(CRATE_LABEL), Some(CRATE_BODY))),
                )
            } else {
                column_decision(&replaced, column, "reject", hash, None)
            }
        }).collect(),
    );
    let exact = pack_command(
        &replaced_review,
        &resealed_artifact,
        classes.clone(),
        two_roles(),
        &["dataset_descriptions"],
        true,
        Some(json!({ item_id.clone(): prior_sha })),
    );
    let (exact_text, exact_packed) = apply_ok(&exact);
    absent_review_only(&exact_text);
    assert_eq!(exact_packed["outcome"], "packed");
    let row = exact_packed["occupied_replacements"]
        .as_array()
        .expect("replacements")
        .iter()
        .find(|item| item["item_id"] == item_id)
        .expect("replacement");
    assert_eq!(row["attribution"], "bridge_provenance");
    assert_eq!(row["prior_review"], prior);
    assert_eq!(row["prior_review_sha256"], prior_sha);
    assert_eq!(row["compiler_review_sha256"], prior_sha);
    assert_eq!(row["descriptive_provenance"]["authenticated"], false);
    assert_eq!(row["descriptive_provenance"]["reviewer_label"], REVIEWER);
    assert_eq!(row["descriptive_provenance"]["fields"], json!(["description", "label"]));
    assert!(!serde_json::to_string(&review_by_id(&exact_packed["resealed_artifact"], &item_id)).expect("review").contains(REVIEWER));
    assert_eq!(review_by_id(&exact_packed["resealed_artifact"], &item_id), &prior);
    assert_eq!(column_of(&document_of(&exact_packed, LANE)["document"], "crate_count")["label"], CRATE_LABEL_2);
    assert_eq!(column_of(&document_of(&exact_packed, LANE)["document"], "crate_count")["review"], kept_review());
    assert_eq!(recompile(&resealed_artifact).wrapper_hash, exact_packed["original_wrapper_hash"]);

    let mut stale_review: Value = serde_json::from_str(&review).expect("review");
    let stale_row = stale_review["column_decisions"]
        .as_array_mut()
        .expect("columns")
        .iter_mut()
        .find(|item| item["column"] == "crate_count")
        .expect("crate");
    stale_row["schema_hash"] = json!("ab".repeat(32));
    let stale_text = serde_json::to_string(&stale_review).expect("stale");
    let err = apply_description_pack(&pack_command(
        &stale_text,
        &world.artifact,
        json!([]),
        two_roles(),
        &["dataset_descriptions"],
        false,
        None,
    ))
    .expect_err("stale");
    assert!(err.code.ends_with("/review.schema"), "{}", err.code);
    assert!(!err.to_string().contains(CRATE_BODY));

    let deferred = defer_review(world);
    let (deferred_text, deferred_packed) = apply_ok(&pack_command(
        &deferred,
        &world.artifact,
        classes.clone(),
        two_roles(),
        &["column_descriptions", "dataset_descriptions"],
        false,
        None,
    ));
    assert_eq!(deferred_packed["outcome"], "reviewed_unchanged");
    assert!(deferred_packed["resealed_artifact"].is_null());
    assert_eq!(deferred_packed["statements"]["reseal"], "not_required");
    assert!(!deferred_text.contains(CRATE_BODY));
    assert!(!deferred_text.contains(BADGE_BODY));
    assert_eq!(column_of(&document_of(&deferred_packed, LANE)["document"], "crate_count")["description"], CRATE_OLD_BODY);
    assert_eq!(column_of(&document_of(&deferred_packed, LANE)["document"], "crate_count")["review"], kept_review());
    let unused = pack_command(
        &deferred,
        &world.artifact,
        classes.clone(),
        two_roles(),
        &["dataset_descriptions"],
        true,
        Some(json!({ item_id: prior_sha })),
    );
    let err = apply_description_pack(&unused).expect_err("unused cas");
    assert!(err.code.ends_with("/review.occupied_cas"), "{}", err.code);

    let hidden = json!([visibility("oo_r_mark", "mark_user", json!([grant(BAY, &["private_mark"])]))]);
    let err = apply_description_pack(&pack_command(
        &review,
        &world.artifact,
        classes,
        hidden,
        &["column_descriptions"],
        false,
        None,
    ))
    .expect_err("unknown pii");
    assert!(err.code.contains("classification.unknown_pii"), "{}", err.code);
    assert!(!err.to_string().contains(MARK_BODY));
    assert!(!err.to_string().contains(CRATE_BODY));
    assert_eq!(recompile(&world.artifact).wrapper_hash, world.wrapper_hash);
}

fn with_crate_text(label: &str, description: &str) -> Value {
    let mut value = lane_file();
    let column = value["columns"]
        .as_array_mut()
        .expect("columns")
        .iter_mut()
        .find(|item| item["name"] == "crate_count")
        .expect("crate");
    column["label"] = json!(label);
    column["description"] = json!(description);
    dataset_file(value)
}

fn bind_second(artifact: &Value) -> World {
    let compiled = recompile(artifact);
    let draft: Value = serde_json::from_str(&compiled.draft).expect("draft");
    let (produced_text, outcome_text, outcome, request) = bind_v2(artifact, &compiled);
    World {
        artifact: artifact.clone(),
        draft,
        wrapper_hash: compiled.wrapper_hash,
        schema_hash: compiled.schema_hash,
        evidence_hash: compiled.evidence_hash,
        request,
        produced_text,
        outcome_sha: payload_sha256(outcome_text.as_bytes()),
        outcome_text,
        outcome,
    }
}

fn review_command_custom(world: &World, lane: &Value, bay: &Value, columns: Vec<Value>) -> String {
    let lane_hash = canonical_sha(lane);
    let bay_hash = canonical_sha(bay);
    serde_json::to_string(&json!({
        "column_decisions": columns,
        "command": world.produced_text,
        "current": {"evidence_hash": world.evidence_hash, "schema_hash": world.schema_hash},
        "curated": curated_map(lane, bay),
        "decisions": [
            dataset_decision(world, LANE, "defer", &lane_hash),
            dataset_decision(world, BAY, "reject", &bay_hash)
        ],
        "expected_outcome_sha256": world.outcome_sha,
        "protocol": description_review::COMMAND_PROTOCOL
    }))
    .expect("custom review")
}

fn replace_crate_candidate(base: &World, artifact: &Value) -> World {
    let compiled = recompile(artifact);
    let bare = command_for(&compiled, artifact, PROMPT_V2);
    let mut columns = labeled_columns(&physical(&base.request));
    for item in columns.as_array_mut().expect("columns") {
        if item["dataset"] == LANE && item["column"] == "crate_count" {
            item["label"] = json!(CRATE_LABEL_2);
            item["description"] = json!(CRATE_BODY_2);
            item["confidence"] = json!("0");
            item["caveats"] = json!([]);
            item["open_questions"] = json!([]);
        }
    }
    let request = base.request.clone();
    let response = response_v2(&request, dataset_bodies(&request), columns);
    let (produced_text, outcome_text, outcome) = finish(bare, response);
    assert_eq!(outcome["outcome"], "candidates");
    let draft: Value = serde_json::from_str(&compiled.draft).expect("draft");
    World {
        artifact: artifact.clone(),
        draft,
        wrapper_hash: compiled.wrapper_hash,
        schema_hash: compiled.schema_hash,
        evidence_hash: compiled.evidence_hash,
        request,
        produced_text,
        outcome_sha: payload_sha256(outcome_text.as_bytes()),
        outcome_text,
        outcome,
    }
}

fn defer_review(world: &World) -> String {
    let lane = lane_file();
    let bay = bay_file();
    let lane_hash = canonical_sha(&lane);
    let bay_hash = canonical_sha(&bay);
    let columns = physical(&world.request);
    review_command_custom(
        world,
        &lane,
        &bay,
        columns
            .iter()
            .map(|column| {
                let hash = if column.dataset == LANE { lane_hash.as_str() } else { bay_hash.as_str() };
                let kind = if column.name == "id" { "defer" } else { "reject" };
                column_decision(world, column, kind, hash, None)
            })
            .collect(),
    )
}
