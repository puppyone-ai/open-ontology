//! `review-pg-draft` passes a captured catalog and optional review values
//! to `compile_captured_pg_catalog`.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use oo_builder::compile_captured_pg_catalog;
use oo_core::limits::DOCUMENT_MAX_BYTES;

const COMMAND_PROTOCOL: &str = "oo.builder.captured-pg-draft-command/v1";
const HISTORICAL: &str = "b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74";
const BEFORE_FILE: &str = "de094222fcd5cb4e54c95a5ec8950c8fd2d55837bd1cff8cd3dd49673f8d56d9";
const AFTER_FILE: &str = "53e2fe7e68197f8698a2a9e2683dc2131426a0faea6bfb277594f9ba9ce52903";
const BEFORE_SCHEMA: &str = "a0efbfc8b5bc7536a121208a1c8e3ff41b16cdda4018b5618b047f40d415271c";
const AFTER_SCHEMA: &str = "c123519256e1c88ab063a41f250ac02483822983d3cf95c364d25983e86fde4d";
const BEFORE_DRAFT: &str = "d6980c138fa4939195bd673582938c94e9adca72a9acf710ddcebae407b138ef";
const BEFORE_WRAPPER: &str = "e525520f010677050108f4aed9d899ecca28e2c56cbcb91e10d9485d20e3267e";
const BEFORE_CAPTURED: &str = "5c17683d74bcc7916954dfb23d22004e5ccbd4058f54bb1d3699ca679a8b90b0";
const AFTER_CAPTURED: &str = "6f4fca46a3144f9e5c0073fec0945af70717cde52a18c257ec74b0d60109c92c";
const AFTER_STALE_DRAFT: &str = "8e00ba219f9e99044a739acb3f96eff04f7ce44784a42bec1609e32ee7ae688f";
const EMAIL_ITEM: &str = "column/ledger.party/email";
const ID_ITEM: &str = "column/ledger.party/id";
const REL_ITEM: &str = "relationship/ledger.invoice/invoice_party_fk";
const MARKER: &str = "a161-do-not-echo";
const OWNER: &str = "a157_owner";

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oo-builder"))
}

fn catalog_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out/{name}"
    ))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn run(args: &[&str], stdin: &[u8]) -> std::process::Output {
    let mut child = bin()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

fn read_catalog(name: &str, expected: &str) -> String {
    let bytes = fs::read(catalog_path(name)).unwrap();
    assert_eq!(sha256_hex(&bytes), expected);
    String::from_utf8(bytes).unwrap()
}

fn review_document(catalog: &str, curated: Option<&str>, reviews: Option<&str>) -> String {
    let mut out = String::from("{\"catalog\":");
    out.push_str(catalog.trim());
    if let Some(curated) = curated {
        out.push_str(",\"curated\":");
        out.push_str(curated);
    }
    if let Some(reviews) = reviews {
        out.push_str(",\"reviews\":");
        out.push_str(reviews);
    }
    out.push('}');
    out
}

fn review_decision<'a>(draft: &'a Value, item_id: &str) -> &'a Value {
    draft["reviews"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["item_id"] == item_id)
        .unwrap_or_else(|| panic!("missing review {item_id}"))
}

fn queue_item<'a>(draft: &'a Value, item_id: &str, kind: &str) -> &'a Value {
    draft["review_queue"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["item_id"] == item_id && item["kind"] == kind)
        .unwrap_or_else(|| panic!("missing {item_id} {kind}"))
}

fn fingerprint(draft: &Value, item_id: &str, kind: &str) -> String {
    queue_item(draft, item_id, kind)["fact_fingerprint"]
        .as_str()
        .unwrap()
        .to_string()
}

fn column<'a>(draft: &'a Value, layer: &str, dataset_name: &str, name: &str) -> &'a Value {
    draft[layer]["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["dataset"] == dataset_name)
        .unwrap()["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("missing {layer} {dataset_name} {name}"))
}

fn column_names(draft: &Value, layer: &str, dataset_name: &str) -> Vec<String> {
    draft[layer]["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["dataset"] == dataset_name)
        .unwrap()["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap().to_string())
        .collect()
}

fn relationship<'a>(draft: &'a Value, layer: &str, name: &str) -> Option<&'a Value> {
    draft[layer]["relationships"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
}

fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
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
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn assert_static_failure(output: &std::process::Output, needle: &str) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr.clone()).unwrap();
    assert!(stderr.contains(needle), "{stderr}");
    assert!(stderr.len() < 400, "stderr len {}", stderr.len());
    assert!(!stderr.contains(MARKER));
    assert!(!stderr.contains(OWNER));
}

fn assert_command(stdin: &str, catalog: &str, curated: Option<&str>, reviews: Option<&str>) -> Value {
    let api = compile_captured_pg_catalog(catalog, curated, reviews).unwrap();
    let output = run(&["review-pg-draft"], stdin.as_bytes());
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.ends_with(b"\n"));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let expected = format!(
        "{{\"draft\":{},\"evidence_envelope\":{},\"protocol\":\"{COMMAND_PROTOCOL}\",\"wrapper\":{}}}",
        api.draft, api.evidence_envelope, api.wrapper
    );
    assert_eq!(stdout, expected);
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(doc["protocol"], COMMAND_PROTOCOL);
    assert_eq!(
        doc["wrapper"],
        serde_json::from_str::<Value>(&api.wrapper).unwrap()
    );
    assert_eq!(
        doc["evidence_envelope"],
        serde_json::from_str::<Value>(&api.evidence_envelope).unwrap()
    );
    assert_eq!(doc["draft"], serde_json::from_str::<Value>(&api.draft).unwrap());
    assert_eq!(doc["wrapper"]["draft_hash"], sha256_hex(api.draft.as_bytes()));
    assert_eq!(
        doc["wrapper"]["evidence_envelope_hash"],
        sha256_hex(api.evidence_envelope.as_bytes())
    );
    assert_eq!(
        doc["wrapper"]["schema_hash"].as_str(),
        Some(api.schema_hash.as_str())
    );
    assert_eq!(
        doc["wrapper"]["captured_catalog_hash"].as_str(),
        Some(api.captured_catalog_hash.as_str())
    );
    assert_eq!(
        doc["wrapper"]["wrapper_hash"].as_str(),
        Some(api.wrapper_hash.as_str())
    );
    assert_eq!(
        doc["draft"]["provenance"]["manifest_hash"].as_str(),
        Some(api.manifest_hash.as_str())
    );
    assert_eq!(doc["wrapper"]["live_catalog_review"], "pending");
    assert_eq!(doc["wrapper"]["row_import"], "not_proven");
    assert_eq!(doc["wrapper"]["grants"], "not_authorized");
    assert_eq!(doc["wrapper"]["publication"], "unavailable");
    assert_eq!(doc["wrapper"]["certification"], "none");
    assert_eq!(doc["wrapper"]["llm"], "not_used");
    assert_eq!(doc["wrapper"]["source_connection"], "not_used");
    assert_eq!(
        doc["wrapper"]["historical"]["pg_catalog_integration"],
        "not_accepted"
    );
    assert_eq!(
        doc["wrapper"]["historical"]["pg_catalog_contract_commit"],
        HISTORICAL
    );
    assert_eq!(
        doc["draft"]["provenance"]["pg_catalog_integration"],
        "not_accepted"
    );
    assert_eq!(
        doc["draft"]["provenance"]["pg_catalog_contract_commit"],
        HISTORICAL
    );
    assert_eq!(doc["draft"]["provenance"]["publication"], "unavailable");
    assert!(!stdout.contains(MARKER));
    assert!(!api.draft.contains(OWNER));
    assert!(!api.wrapper.contains(OWNER));
    doc
}

#[test]
fn unknown_input_and_arguments_exit_without_echo() {
    let extra = bin().args(["review-pg-draft", MARKER]).output().unwrap();
    assert_eq!(extra.status.code(), Some(2));
    assert!(extra.stdout.is_empty());
    let extra_err = String::from_utf8(extra.stderr).unwrap();
    assert!(extra_err.contains("takes no arguments"));
    assert!(!extra_err.contains(MARKER));

    assert_static_failure(
        &run(
            &["review-pg-draft"],
            format!(r#"{{"catalog":{{"note":"{MARKER}"}},"catalog":{{"note":"again"}}}}"#).as_bytes(),
        ),
        "/json.duplicate_key",
    );
    assert_static_failure(
        &run(
            &["review-pg-draft"],
            format!(r#"{{"catalog":{{}},"{MARKER}":"{MARKER}"}}"#).as_bytes(),
        ),
        "/field.unknown",
    );
    assert_static_failure(
        &run(&["review-pg-draft"], br#"{"catalog":[]}"#),
        "catalog is not an object",
    );
    assert_static_failure(
        &run(
            &["review-pg-draft"],
            format!(r#"{{"catalog":{{}},"curated":"{MARKER}"}}"#).as_bytes(),
        ),
        "schema.invalid",
    );
    assert_static_failure(
        &run(
            &["review-pg-draft"],
            br#"{"catalog":{},"reviews":{"action":"accept"}}"#,
        ),
        "schema.invalid",
    );
    assert_static_failure(&run(&["review-pg-draft"], br"{}"), "/field.missing");
    assert_static_failure(
        &run(&["review-pg-draft"], &[0xff, 0xfe, b'a']),
        "stdin is not UTF-8",
    );
    let huge = vec![b' '; DOCUMENT_MAX_BYTES + 1];
    assert_static_failure(
        &run(&["review-pg-draft"], &huge),
        "document exceeds the byte budget",
    );
}

#[test]
fn bare_captured_catalog_matches_the_public_api_and_preserved_hashes() {
    let before = read_catalog("catalog-before.json", BEFORE_FILE);
    let after = read_catalog("catalog-after.json", AFTER_FILE);
    let parsed: Value = serde_json::from_str(before.trim()).unwrap();
    let sorted = compile_captured_pg_catalog(&canonical_json(&parsed), None, None).unwrap();
    let raw = compile_captured_pg_catalog(before.trim(), None, None).unwrap();
    assert_eq!(sorted.draft, raw.draft);
    assert_eq!(sorted.wrapper, raw.wrapper);
    assert_eq!(sorted.evidence_envelope, raw.evidence_envelope);

    let doc = assert_command(&review_document(before.trim(), None, None), before.trim(), None, None);
    assert_eq!(doc["wrapper"]["schema_hash"], BEFORE_SCHEMA);
    assert_eq!(doc["wrapper"]["draft_hash"], BEFORE_DRAFT);
    assert_eq!(doc["wrapper"]["wrapper_hash"], BEFORE_WRAPPER);
    assert_eq!(doc["wrapper"]["captured_catalog_hash"], BEFORE_CAPTURED);
    assert_eq!(doc["draft"]["provenance"]["schema_hash"], BEFORE_SCHEMA);
    let empty = assert_command(
        &review_document(before.trim(), None, Some("[]")),
        before.trim(),
        None,
        Some("[]"),
    );
    assert_eq!(empty["wrapper"]["draft_hash"], BEFORE_DRAFT);
    assert_eq!(empty["draft"], doc["draft"]);

    let after_doc = assert_command(&review_document(&after, None, None), after.trim(), None, None);
    assert_eq!(after_doc["wrapper"]["schema_hash"], AFTER_SCHEMA);
    assert_ne!(after_doc["wrapper"]["schema_hash"], BEFORE_SCHEMA);
    assert_eq!(after_doc["wrapper"]["captured_catalog_hash"], AFTER_CAPTURED);
    assert_ne!(after_doc["wrapper"]["draft_hash"], BEFORE_DRAFT);
    assert_eq!(
        column_names(&after_doc["draft"], "generated", "ledger.party"),
        vec!["tenant_id", "id", "email", "note"]
    );
}

#[test]
fn fresh_accept_reject_and_override_bind_the_captured_items() {
    let before = read_catalog("catalog-before.json", BEFORE_FILE);
    let bare = compile_captured_pg_catalog(before.trim(), None, None).unwrap();
    let bare_draft: Value = serde_json::from_str(&bare.draft).unwrap();
    assert_eq!(bare.schema_hash, BEFORE_SCHEMA);
    let email_fp = fingerprint(&bare_draft, EMAIL_ITEM, "structural_column");
    let id_fp = fingerprint(&bare_draft, ID_ITEM, "structural_column");
    let rel_fp = fingerprint(&bare_draft, REL_ITEM, "candidate_relationship");

    let accept = json!([{
        "item_id": EMAIL_ITEM,
        "action": "accept",
        "schema_hash": bare.schema_hash.as_str(),
        "fact_fingerprint": email_fp.as_str(),
    }])
    .to_string();
    let accepted = assert_command(
        &review_document(before.trim(), None, Some(&accept)),
        before.trim(),
        None,
        Some(&accept),
    );
    let draft = &accepted["draft"];
    assert_eq!(
        queue_item(draft, EMAIL_ITEM, "structural_column")["state"],
        "accepted"
    );
    assert_eq!(
        queue_item(draft, EMAIL_ITEM, "structural_column")["publishable"],
        false
    );
    let email = column(draft, "effective", "ledger.party", "email");
    assert!(email["pii"].is_null());
    assert!(email["semantic_type"].is_null());
    assert_eq!(
        queue_item(draft, "pii/ledger.party/email", "pii_unknown")["state"],
        "pending"
    );
    assert_eq!(
        queue_item(draft, "pii/ledger.party/email", "pii_unknown")["publishable"],
        false
    );
    assert_eq!(
        queue_item(draft, "semantic/ledger.party/email", "semantic_unknown")["publishable"],
        false
    );
    assert_eq!(accepted["wrapper"]["schema_hash"], BEFORE_SCHEMA);

    let curated = r#"{"datasets":[{"dataset":"ledger.party","columns":[{"name":"id","status":"rejected","label":"Kept label"}]}],"relationships":[]}"#;
    let closure = json!([
        {
            "item_id": REL_ITEM,
            "action": "accept",
            "schema_hash": bare.schema_hash.as_str(),
            "fact_fingerprint": rel_fp.as_str(),
        },
        {
            "item_id": ID_ITEM,
            "action": "reject",
            "schema_hash": bare.schema_hash.as_str(),
            "fact_fingerprint": id_fp.as_str(),
        }
    ])
    .to_string();
    let closed = assert_command(
        &review_document(before.trim(), Some(curated), Some(&closure)),
        before.trim(),
        Some(curated),
        Some(&closure),
    );
    let draft = &closed["draft"];
    assert!(column_names(draft, "generated", "ledger.party").contains(&"id".to_string()));
    assert!(!column_names(draft, "effective", "ledger.party").contains(&"id".to_string()));
    assert_eq!(column(draft, "curated", "ledger.party", "id")["status"], "rejected");
    assert_eq!(column(draft, "curated", "ledger.party", "id")["label"], "Kept label");
    assert!(relationship(draft, "generated", "invoice_party_fk").is_some());
    assert!(relationship(draft, "effective", "invoice_party_fk").is_none());
    let endpoint = queue_item(draft, REL_ITEM, "relationship_endpoint_not_effective");
    assert_eq!(endpoint["detail"], "to_column_not_effective");
    assert_eq!(endpoint["state"], "pending");
    assert_eq!(endpoint["publishable"], false);
    assert_eq!(
        queue_item(draft, REL_ITEM, "candidate_relationship")["state"],
        "accepted"
    );
    let accepted_rel = review_decision(draft, REL_ITEM);
    assert_eq!(accepted_rel["action"], "accept");
    assert_eq!(
        accepted_rel["fact_fingerprint"].as_str(),
        Some(rel_fp.as_str())
    );
    let rejected_id = review_decision(draft, ID_ITEM);
    assert_eq!(rejected_id["action"], "reject");
    assert_eq!(rejected_id["fact_fingerprint"].as_str(), Some(id_fp.as_str()));
    assert_eq!(closed["wrapper"]["live_catalog_review"], "pending");

    let overlay = r#"{"datasets":[{"dataset":"ledger.party","columns":[{"name":"email","label":"Correspondence","pii":"sensitive"}]}],"relationships":[]}"#;
    let override_review = json!([{
        "item_id": EMAIL_ITEM,
        "action": "override",
        "schema_hash": bare.schema_hash.as_str(),
        "fact_fingerprint": email_fp.as_str(),
    }])
    .to_string();
    let overridden = assert_command(
        &review_document(before.trim(), Some(overlay), Some(&override_review)),
        before.trim(),
        Some(overlay),
        Some(&override_review),
    );
    let draft = &overridden["draft"];
    assert_eq!(
        column(draft, "effective", "ledger.party", "email")["label"],
        "Correspondence"
    );
    assert!(column(draft, "effective", "ledger.party", "email")["pii"].is_null());
    assert!(
        column(draft, "effective", "ledger.party", "email")["semantic_type"].is_null()
    );
    assert_eq!(
        column(draft, "curated", "ledger.party", "email")["pii"],
        "sensitive"
    );
    assert_eq!(
        column(draft, "curated", "ledger.party", "email")["label"],
        "Correspondence"
    );
    assert_eq!(
        queue_item(draft, "pii/ledger.party/email", "semantic_field_not_applied")["detail"],
        "pii"
    );
    assert_eq!(
        queue_item(draft, "pii/ledger.party/email", "pii_unknown")["state"],
        "pending"
    );
    assert_eq!(
        queue_item(draft, "pii/ledger.party/email", "pii_unknown")["publishable"],
        false
    );
    assert_eq!(draft["reviews"][0]["action"], "override");
    assert_eq!(draft["provenance"]["certification"], "none");
}

#[test]
fn stale_schema_and_fact_decisions_stay_pending() {
    let before = read_catalog("catalog-before.json", BEFORE_FILE);
    let after = read_catalog("catalog-after.json", AFTER_FILE);
    let bare = compile_captured_pg_catalog(before.trim(), None, None).unwrap();
    let bare_draft: Value = serde_json::from_str(&bare.draft).unwrap();
    let email_fp = fingerprint(&bare_draft, EMAIL_ITEM, "structural_column");
    let curated = r#"{"datasets":[{"dataset":"ledger.party","columns":[{"name":"email","label":"Correspondence","pii":"sensitive"}]}],"relationships":[]}"#;
    let prior = json!([{
        "item_id": EMAIL_ITEM,
        "action": "accept",
        "schema_hash": bare.schema_hash.as_str(),
        "fact_fingerprint": email_fp.as_str(),
    }])
    .to_string();
    let stale = assert_command(
        &review_document(after.trim(), Some(curated), Some(&prior)),
        after.trim(),
        Some(curated),
        Some(&prior),
    );
    let draft = &stale["draft"];
    assert_eq!(stale["wrapper"]["schema_hash"], AFTER_SCHEMA);
    assert_ne!(stale["wrapper"]["schema_hash"], BEFORE_SCHEMA);
    assert_eq!(stale["wrapper"]["captured_catalog_hash"], AFTER_CAPTURED);
    assert_eq!(stale["wrapper"]["draft_hash"], AFTER_STALE_DRAFT);
    assert_eq!(
        column(draft, "effective", "ledger.party", "email")["label"],
        "email"
    );
    assert!(column(draft, "effective", "ledger.party", "email")["pii"].is_null());
    assert!(
        column(draft, "effective", "ledger.party", "email")["semantic_type"].is_null()
    );
    assert_eq!(
        column(draft, "curated", "ledger.party", "email")["label"],
        "Correspondence"
    );
    assert_eq!(
        column(draft, "curated", "ledger.party", "email")["pii"],
        "sensitive"
    );
    let decision = queue_item(draft, EMAIL_ITEM, "stale_decision");
    assert_eq!(decision["detail"], "stale_schema_hash");
    assert_eq!(decision["state"], "pending");
    assert_eq!(decision["publishable"], false);
    assert_eq!(
        queue_item(draft, EMAIL_ITEM, "structural_column")["state"],
        "pending"
    );
    assert_eq!(
        queue_item(draft, EMAIL_ITEM, "curated_unbound")["state"],
        "pending"
    );
    assert_eq!(
        queue_item(draft, "pii/ledger.party/email", "semantic_field_not_applied")["detail"],
        "pii"
    );
    assert_eq!(
        queue_item(draft, "pii/ledger.party/email", "pii_unknown")["publishable"],
        false
    );
    assert_eq!(
        queue_item(draft, "semantic/ledger.party/email", "semantic_unknown")["publishable"],
        false
    );
    assert_eq!(
        column_names(draft, "generated", "ledger.party"),
        vec!["tenant_id", "id", "email", "note"]
    );

    let wrong_fact = json!([{
        "item_id": EMAIL_ITEM,
        "action": "accept",
        "schema_hash": bare.schema_hash.as_str(),
        "fact_fingerprint": "0000000000000000000000000000000000000000000000000000000000000000",
    }])
    .to_string();
    let fact = assert_command(
        &review_document(before.trim(), None, Some(&wrong_fact)),
        before.trim(),
        None,
        Some(&wrong_fact),
    );
    let draft = &fact["draft"];
    let decision = queue_item(draft, EMAIL_ITEM, "stale_decision");
    assert_eq!(decision["detail"], "stale_fact_fingerprint");
    assert_eq!(decision["state"], "pending");
    assert_eq!(decision["publishable"], false);
    assert_eq!(
        queue_item(draft, EMAIL_ITEM, "structural_column")["state"],
        "pending"
    );
    assert!(column_names(draft, "effective", "ledger.party").contains(&"email".to_string()));
    assert_eq!(fact["wrapper"]["schema_hash"], BEFORE_SCHEMA);
    assert_ne!(fact["wrapper"]["draft_hash"], BEFORE_DRAFT);

    let unknown = json!([{
        "item_id": "column/ledger.party/not_a_column",
        "action": "accept",
        "schema_hash": bare.schema_hash.as_str(),
        "fact_fingerprint": email_fp.as_str(),
    }])
    .to_string();
    assert_static_failure(
        &run(
            &["review-pg-draft"],
            review_document(before.trim(), None, Some(&unknown)).as_bytes(),
        ),
        "/review.unknown_item",
    );
}
