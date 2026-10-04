//! `captured-pg-draft` reads one captured catalog and writes parsed objects.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use oo_builder::{compile_captured_pg_catalog, compile_structure_draft};
use oo_core::limits::DOCUMENT_MAX_BYTES;

const COMMAND_PROTOCOL: &str = "oo.builder.captured-pg-draft-command/v1";
const FIXTURE_SHA256: &str = "de094222fcd5cb4e54c95a5ec8950c8fd2d55837bd1cff8cd3dd49673f8d56d9";
const COMMENT_BODY: &str = "a157-review-comment-body-do-not-carry";
const OWNER: &str = "a157_owner";

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oo-builder"))
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out/catalog-before.json",
    )
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
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(ch)));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn queue_item<'a>(draft: &'a Value, item_id: &str, kind: &str) -> &'a Value {
    draft["review_queue"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["item_id"] == item_id && item["kind"] == kind)
        .unwrap_or_else(|| panic!("missing {item_id} {kind}"))
}

#[test]
fn usage_unknown_and_extra_arguments_exit_2() {
    let bare = bin().output().unwrap();
    assert_eq!(bare.status.code(), Some(2));
    assert!(bare.stdout.is_empty());
    let stderr = String::from_utf8(bare.stderr).unwrap();
    assert!(stderr.contains("usage: oo-builder <command>"));
    assert!(stderr.contains("structure-draft"));
    assert!(stderr.contains("captured-pg-draft"));
    assert!(stderr.contains("review-pg-draft"));
    assert!(stderr.contains("one oo.source.pgsql/v1 catalog"));
    assert!(stderr.contains("do not connect"));
    assert!(!stderr.contains("process entry is not implemented"));
    assert!(!stderr.contains("SP00-W01 skeleton"));
    assert!(!stderr.contains("stay on the library API"));

    let unknown = bin().arg("publish").output().unwrap();
    assert_eq!(unknown.status.code(), Some(2));
    assert!(unknown.stdout.is_empty());
    assert!(String::from_utf8(unknown.stderr).unwrap().contains("unknown command"));

    let extra = bin()
        .args(["captured-pg-draft", "--out"])
        .output()
        .unwrap();
    assert_eq!(extra.status.code(), Some(2));
    assert!(extra.stdout.is_empty());
    let extra_err = String::from_utf8(extra.stderr).unwrap();
    assert!(extra_err.contains("takes no arguments"));
    assert!(!extra_err.contains(COMMENT_BODY));
}

#[test]
fn bad_json_utf8_and_oversize_exit_1_without_echo() {
    let marker = "a158-raw-catalog-do-not-echo";
    let bad = run(
        &["captured-pg-draft"],
        format!(r#"{{"owner":"{marker}","comment":"{marker}""#).as_bytes(),
    );
    assert_eq!(bad.status.code(), Some(1));
    assert_eq!(bad.stdout.len(), 0);
    let bad_err = String::from_utf8(bad.stderr).unwrap();
    assert!(bad_err.contains("json.syntax"));
    assert!(!bad_err.contains(marker));
    assert!(bad_err.len() < 400);

    let utf8 = run(&["captured-pg-draft"], &[0xff, 0xfe]);
    assert_eq!(utf8.status.code(), Some(1));
    assert_eq!(utf8.stdout.len(), 0);
    let utf8_err = String::from_utf8(utf8.stderr).unwrap();
    assert!(utf8_err.contains("stdin is not UTF-8"));
    assert!(!utf8_err.contains('\u{fffd}'));

    let huge = vec![b' '; DOCUMENT_MAX_BYTES + 1];
    let over = run(&["captured-pg-draft"], &huge);
    assert_eq!(over.status.code(), Some(1));
    assert_eq!(over.stdout.len(), 0, "stdout bytes {}", over.stdout.len());
    let over_err = String::from_utf8(over.stderr).unwrap();
    assert!(over_err.contains("json.budget.bytes"));
    assert!(over_err.contains("document exceeds the byte budget"));
    assert!(over_err.len() < 400);
    assert!(!over_err.contains(&" ".repeat(64)));
}

#[test]
fn captured_catalog_command_matches_the_public_api() {
    let path = fixture_path();
    let catalog = std::fs::read(&path).unwrap();
    assert_eq!(sha256_hex(&catalog), FIXTURE_SHA256);
    let text = std::str::from_utf8(&catalog).unwrap();
    assert!(!text.contains(COMMENT_BODY));
    let api = compile_captured_pg_catalog(text, None, None).unwrap();
    let output = run(&["captured-pg-draft"], &catalog);
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
    assert_eq!(
        doc.as_object().unwrap().keys().cloned().collect::<Vec<_>>(),
        vec![
            "draft".to_string(),
            "evidence_envelope".to_string(),
            "protocol".to_string(),
            "wrapper".to_string()
        ]
    );
    assert_eq!(doc["protocol"], COMMAND_PROTOCOL);
    assert_eq!(doc["wrapper"], serde_json::from_str::<Value>(&api.wrapper).unwrap());
    assert_eq!(
        doc["evidence_envelope"],
        serde_json::from_str::<Value>(&api.evidence_envelope).unwrap()
    );
    assert_eq!(doc["draft"], serde_json::from_str::<Value>(&api.draft).unwrap());
    assert_eq!(
        compile_structure_draft(&api.evidence_envelope).unwrap(),
        api.draft
    );

    assert_eq!(
        doc["wrapper"]["draft_hash"],
        sha256_hex(api.draft.as_bytes())
    );
    assert_eq!(
        doc["wrapper"]["evidence_envelope_hash"],
        sha256_hex(api.evidence_envelope.as_bytes())
    );
    assert_eq!(
        doc["wrapper"]["captured_catalog_hash"],
        sha256_hex(canonical_json(&doc["evidence_envelope"]["catalog"]).as_bytes())
    );
    let mut wrapper_pre = doc["wrapper"].clone();
    wrapper_pre.as_object_mut().unwrap().remove("wrapper_hash");
    assert_eq!(
        doc["wrapper"]["wrapper_hash"],
        sha256_hex(canonical_json(&wrapper_pre).as_bytes())
    );
    let mut draft_pre = doc["draft"].clone();
    draft_pre["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("manifest_hash");
    assert_eq!(
        doc["draft"]["provenance"]["manifest_hash"],
        sha256_hex(canonical_json(&draft_pre).as_bytes())
    );

    let draft = &doc["draft"];
    let relationship = draft["generated"]["relationships"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "invoice_party_fk")
        .unwrap();
    assert_eq!(relationship["cardinality"], "many_to_one");
    assert_eq!(relationship["provenance"], "declared_fk");
    assert_eq!(relationship["status"], "draft");
    assert!(relationship["confidence"].is_null());
    assert_eq!(relationship["join_hint"], "left");
    let email = draft["effective"]["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["dataset"] == "ledger.party")
        .unwrap()["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "email")
        .unwrap();
    assert!(email["pii"].is_null());
    assert!(email["semantic_type"].is_null());
    assert_eq!(
        queue_item(draft, "pii/ledger.party/email", "pii_unknown")["state"],
        "pending"
    );
    assert_eq!(
        queue_item(draft, "semantic/ledger.party/email", "semantic_unknown")["state"],
        "pending"
    );
    assert_eq!(doc["wrapper"]["row_import"], "not_proven");
    assert_eq!(doc["wrapper"]["grants"], "not_authorized");
    assert_eq!(doc["wrapper"]["live_catalog_review"], "pending");
    assert_eq!(doc["wrapper"]["llm"], "not_used");
    assert_eq!(doc["wrapper"]["source_connection"], "not_used");
    assert_eq!(
        doc["wrapper"]["historical"]["pg_catalog_integration"],
        "not_accepted"
    );

    assert!(!stdout.contains(COMMENT_BODY));
    assert!(!api.draft.contains(OWNER));
    assert!(!api.wrapper.contains(OWNER));
    assert!(!canonical_json(&doc["draft"]).contains(OWNER));
    assert!(!canonical_json(&doc["wrapper"]).contains(OWNER));
    assert!(doc.get("owner").is_none());
    assert_eq!(
        doc["evidence_envelope"]["catalog"]["tables"][0]["owner"],
        OWNER
    );
}
