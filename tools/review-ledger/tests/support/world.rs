//! One description-review command, built with the reviewed compiler entry points.
//! The ledger does not reimplement candidate completion.

use std::sync::OnceLock;

use serde_json::{Map, Value, json};
use description_pack::apply_description_pack;
use description_review::{
    COMMAND_PROTOCOL, descriptive_field_sha256, curated_dataset_sha256, apply_description_review,
};
use oo_builder::{
    DESCRIPTION_PROMPT_VERSION, DESCRIPTION_REQUEST_COMMAND_PROTOCOL, DESCRIPTION_RESPONSE_PROTOCOL,
    compile_captured_pg_catalog, complete_description_candidates, prepare_description_request,
};
use oo_core::api::payload_sha256;
use oo_core::oos::{DatasetFile, validate_dataset};

const MODEL: &str = "a277-captured";
const OWNER: &str = "tbl_owner";
const CAPTURED_AT: &str = "2026-10-06T12:00:00.000000Z";
const LOWER: &str = "public.b";
const UPPER: &str = "public.q_41";
const EXTRA: &str = "public.extra";
const REVIEWER: &str = "ada.reviewer";
const REVIEWED_ON: &str = "2026-10-06";

pub struct ReviewFixture {
    pub command_text: String,
    pub candidate_sha256: String,
    pub input_sha256: String,
    pub evidence_sha256: String,
    pub schema_sha256: String,
    pub curated_sha256: String,
    pub label_fields: Value,
}

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

pub struct PackFixture {
    pub command_text: String,
    pub outcome_text: String,
    pub outcome_name: String,
}

pub fn pack_fixture() -> &'static PackFixture {
    static FIXTURE: OnceLock<PackFixture> = OnceLock::new();
    FIXTURE.get_or_init(build_pack)
}

fn build_pack() -> PackFixture {
    let review = review_fixture();
    let review_command: Value = serde_json::from_str(&review.command_text).expect("review command");
    let inner = review_command["command"].as_str().expect("inner command");
    let candidate: Value = serde_json::from_str(inner).expect("candidate command");
    let artifact = candidate.get("artifact").cloned().expect("artifact");
    let draft = &artifact["draft"];
    let reviewed = apply_description_review(&review.command_text).expect("review document");
    let command = serde_json::to_string(&json!({
        "protocol": "oo.builder.description-pack-command/v1",
        "review": review.command_text,
        "expected_review_sha256": payload_sha256(reviewed.as_bytes()),
        "artifact": artifact,
        "settings": pack_settings(),
        "decisions": column_decisions(draft),
        "roles": [pack_role(draft)]
    }))
    .expect("pack command");
    let outcome_text = apply_description_pack(&command).unwrap_or_else(|error| {
        panic!("pack fixture failed: {} {}", error.code, error.message);
    });
    let outcome: Value = serde_json::from_str(&outcome_text).expect("pack json");
    let outcome_name = outcome["outcome"].as_str().expect("outcome").to_owned();
    assert!(
        outcome_name == "packed" || outcome_name == "reviewed_unchanged",
        "{outcome_name}"
    );
    assert_eq!(outcome["statements"]["authentication"], "not_claimed");
    assert_eq!(outcome["statements"]["publication"], "unavailable");
    assert_eq!(outcome["statements"]["grants"], "not_authorized");
    assert_eq!(outcome["compiler"], "oo-builder/description-pack/v1");
    PackFixture {
        command_text: command,
        outcome_text,
        outcome_name,
    }
}

fn pack_settings() -> Value {
    json!({
        "project": "shop",
        "ontology_version": "ont-1",
        "catalog_version": "cat-1",
        "policy": {"id": "pol-shop", "version": "1"},
        "knowledge": {"id": "know-shop", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "shop",
            "locale": "en-US",
            "include_schemas": ["public"],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": []},
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

fn column_decisions(draft: &Value) -> Vec<Value> {
    let schema = draft["provenance"]["schema_hash"].as_str().expect("schema hash");
    let mut decisions = Vec::new();
    for item in draft["review_queue"].as_array().expect("review queue") {
        if item["kind"] != "structural_column" {
            continue;
        }
        let item_id = item["item_id"].as_str().expect("item");
        let rest = item_id.strip_prefix("column/").expect("column item");
        let (dataset, column) = rest.rsplit_once('/').expect("column id");
        decisions.push(json!({
            "dataset": dataset,
            "column": column,
            "schema_hash": schema,
            "fact_fingerprint": item["fact_fingerprint"],
            "pii": "none",
            "semantic_type": "identifier",
            "reviewer": {"by": "ada.owner", "at": "2026-10-06"}
        }));
    }
    assert!(!decisions.is_empty());
    decisions
}

fn pack_role(draft: &Value) -> Value {
    let mut datasets = Map::new();
    for item in draft["review_queue"].as_array().expect("review queue") {
        if item["kind"] != "structural_column" {
            continue;
        }
        let item_id = item["item_id"].as_str().expect("item");
        let rest = item_id.strip_prefix("column/").expect("column item");
        let (dataset, column) = rest.rsplit_once('/').expect("column id");
        let columns = datasets.entry(dataset.to_owned()).or_insert_with(|| json!([]));
        columns.as_array_mut().expect("columns").push(json!(column));
    }
    let datasets = datasets
        .into_iter()
        .map(|(dataset, columns)| {
            json!({
                "dataset": dataset,
                "row_security": "none",
                "global_stats": "permitted",
                "columns": columns
            })
        })
        .collect::<Vec<_>>();
    json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": "shop",
        "principal": "desk_user",
        "role": "oo_r_desk",
        "ontology_version": "ont-1",
        "catalog_version": "cat-1",
        "policy": {"id": "pol-shop", "version": "1"},
        "knowledge": {"id": "know-shop", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": datasets,
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    })
}

pub fn review_fixture() -> &'static ReviewFixture {
    static FIXTURE: OnceLock<ReviewFixture> = OnceLock::new();
    FIXTURE.get_or_init(build_fixture)
}

fn build_fixture() -> ReviewFixture {
    let produced = produce(json!([
        candidate(LOWER, "Reviewed b 表", "candidate description that must not replace", "one row per b", "0", json!(["generated caveat"]), json!(["what grain?"])),
        candidate(UPPER, "Reviewed upper", "should not apply", "one row per upper", "1", json!([]), json!([]))
    ]));
    let lower = curated_document(LOWER, "Curated b", Some("curated description b"), None);
    let upper = curated_document(UPPER, "Curated upper", Some("curated description upper"), Some("curated grain upper"));
    let extra = curated_document(EXTRA, "extra curated label", Some("extra description"), Some("extra grain"));
    let lower_hash = doc_sha(&lower);
    assert_eq!(lower_hash, curated_dataset_sha256(&dataset_file(&lower)));
    let lower_anchors = anchors_for(&produced, LOWER, &lower_hash);
    let upper_anchors = anchors_for(&produced, UPPER, &doc_sha(&upper));
    assert_eq!(lower_anchors.schema, world().schema_hash);
    assert_eq!(lower_anchors.evidence, world().evidence_hash);
    let fields = json!({
        "description": {"overwrite": false},
        "grain": {"overwrite": false},
        "label": {"overwrite": true, "current_sha256": descriptive_field_sha256(Some("Curated b"))}
    });
    let command = review_text(
        &produced,
        &world().schema_hash,
        &world().evidence_hash,
        object_map(&[
            (LOWER, lower),
            (UPPER, upper),
            (EXTRA, extra),
        ]),
        vec![
            decision_json(LOWER, "approve", REVIEWER, &lower_anchors, Some(fields.clone())),
            decision_json(UPPER, "reject", REVIEWER, &upper_anchors, None),
        ],
        &produced.outcome_sha,
    );
    let reviewed = apply_description_review(&command).unwrap_or_else(|error| {
        panic!("review fixture failed: {} {}", error.code, error.message);
    });
    let value: Value = serde_json::from_str(&reviewed).expect("review json");
    assert_eq!(value["outcome"], "reviewed");
    assert_eq!(value["statements"]["files"], "not_written");
    assert_eq!(value["statements"]["authentication"], "not_claimed");
    ReviewFixture {
        command_text: command,
        candidate_sha256: lower_anchors.candidate,
        input_sha256: lower_anchors.request,
        evidence_sha256: lower_anchors.evidence,
        schema_sha256: lower_anchors.schema,
        curated_sha256: lower_anchors.curated,
        label_fields: fields,
    }
}

fn world() -> &'static World {
    static WORLD: OnceLock<World> = OnceLock::new();
    WORLD.get_or_init(|| {
        let compiled = compile_captured_pg_catalog(&catalog(), None, None).unwrap_or_else(|error| {
            panic!("catalog compile failed: {} {}", error.code, error.message);
        });
        let command = command_value(&compiled);
        let input = serde_json::to_string(&command).expect("command");
        let request_text = prepare_description_request(&input).unwrap_or_else(|error| {
            panic!("prepare failed: {} {}", error.code, error.message);
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
        && name.chars().next().is_some_and(|ch| ch == '_' || ch.is_ascii_lowercase())
        && name.chars().skip(1).all(|ch| ch == '_' || ch.is_ascii_lowercase() || ch.is_ascii_digit());
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
            ch if (ch as u32) < 0x20 => buf.push_str(&format!("\\u{:04x}", u32::from(ch))),
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

fn write_str(buf: &mut String, name: &str, value: &str) {
    push_preimage_string(buf, name);
    buf.push(':');
    push_preimage_string(buf, value);
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
        "source_id": "a277_src",
        "database": "a277db",
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
    let command_text = serde_json::to_string(&command).expect("command text");
    let outcome_text = complete_description_candidates(&command_text).unwrap_or_else(|error| {
        panic!("complete failed: {} {}", error.code, error.message);
    });
    let outcome: Value = serde_json::from_str(&outcome_text).expect("outcome json");
    Produced {
        command_text,
        outcome_sha: payload_sha256(outcome_text.as_bytes()),
        outcome,
    }
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
        "source_name": "src_name",
        "label": label,
        "description": description,
        "tier": "raw",
        "recommended": false,
        "superseded_by": "public.kept",
        "grain": grain,
        "primary_key": ["id"],
        "entity": "customer",
        "row_count_estimate": 42,
        "time_columns": [{"name": "id", "role": "created_at"}],
        "columns": [
            {
                "name": "id",
                "label": "Id",
                "data_type": "int4",
                "semantic_type": "identifier",
                "pii": "none"
            },
            {
                "name": "phone",
                "label": "Phone",
                "data_type": "text",
                "semantic_type": "phone",
                "searchable": false,
                "pii": "personal",
                "classification": {"state": "confirmed", "by": "ops.wang", "at": "2026-10-06"},
                "enum": format!("{schema}.{table}.code"),
                "status": "deprecated",
                "replaced_by": format!("{schema}.{table}.phone2"),
                "deprecated_at": "ontology/v14",
                "description": "curated phone note",
                "provenance": "human",
                "review": {"state": "approved", "by": "ada.curator", "at": "2026-10-01"}
            }
        ],
        "caveats": ["existing curated caveat"]
    });
    serde_json::to_value(dataset_file(&value)).expect("typed dataset")
}

fn dataset_file(value: &Value) -> DatasetFile {
    let file: DatasetFile = serde_json::from_value(value.clone()).unwrap_or_else(|error| {
        panic!("curated document did not parse: {error}");
    });
    assert!(validate_dataset(&file).is_empty());
    file
}

fn doc_sha(document: &Value) -> String {
    canonical_sha(document)
}

fn anchors_for(produced: &Produced, dataset: &str, curated_hash: &str) -> Anchors {
    let candidate = produced.outcome["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .find(|item| item["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing outcome candidate {dataset}"));
    Anchors {
        request: produced.outcome["request_sha256"].as_str().expect("request").to_owned(),
        card: candidate["card_sha256"].as_str().expect("card").to_owned(),
        schema: produced.outcome["structural_draft"]["schema_hash"].as_str().expect("schema").to_owned(),
        evidence: produced.outcome["structural_draft"]["evidence_hash"].as_str().expect("evidence").to_owned(),
        candidate: canonical_sha(candidate),
        curated: curated_hash.to_owned(),
    }
}

fn decision_json(
    dataset: &str,
    kind: &str,
    reviewer: &str,
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
        "reviewer_label": reviewer,
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
    expected: &str,
) -> String {
    serde_json::to_string(&json!({
        "command": produced.command_text,
        "current": {"evidence_hash": evidence, "schema_hash": schema},
        "curated": curated,
        "decisions": decisions,
        "expected_outcome_sha256": expected,
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
