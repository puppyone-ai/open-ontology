//! Independent process oracle for `oo-builder captured-pg-draft`.
//!
//! The test spawns the built binary. It does not call the command function
//! in-process. Expected objects come from `compile_captured_pg_catalog`.

#![forbid(unsafe_code)]

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use oo_builder::{compile_captured_pg_catalog, compile_structure_draft};
use oo_core::limits::DOCUMENT_MAX_BYTES;

const COMMAND_PROTOCOL: &str = "oo.builder.captured-pg-draft-command/v1";
const FIXTURE_SHA256: &str = "de094222fcd5cb4e54c95a5ec8950c8fd2d55837bd1cff8cd3dd49673f8d56d9";
const SCHEMA_HASH: &str = "a0efbfc8b5bc7536a121208a1c8e3ff41b16cdda4018b5618b047f40d415271c";
const COMMENT_SHA256: &str = "5b4c8687a0a47b150796d520ec9c5253d23eeb33e80cfd685831a7f29384df81";
const COMMENT_SENTENCE: &str = "a157-review-comment-body-do-not-carry";
const PLANTED_BODY: &str = "a160-planted-comment-body-do-not-echo";
const RAW_MARKER: &str = "a160-raw-catalog-do-not-echo";
const OWNER: &str = "a157_owner";
const SKELETON_LINE: &str = "oo-builder: process entry is not implemented (SP00-W01 skeleton)";

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oo-builder"))
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out/catalog-before.json",
    )
}

fn oracle_dir() -> PathBuf {
    PathBuf::from(std::env::var("A160_ORACLE_DIR").expect("A160_ORACLE_DIR"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        out.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    out
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

fn run(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = bin()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn oo-builder");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin)
        .expect("write stdin");
    child.wait_with_output().expect("wait")
}

fn save_output(dir: &Path, name: &str, output: &Output) {
    let code = output.status.code().unwrap_or(127);
    fs::write(dir.join(format!("{name}.exit")), format!("{code}\n")).expect("exit");
    fs::write(dir.join(format!("{name}.stdout")), &output.stdout).expect("stdout");
    fs::write(dir.join(format!("{name}.stderr")), &output.stderr).expect("stderr");
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap_or_else(|_| String::from("invalid-utf8"))
}

fn check(failures: &mut Vec<String>, cond: bool, message: &str) {
    if !cond {
        failures.push(message.to_string());
    }
}

fn put_json(map: &mut Map<String, Value>, key: &str, value: impl serde::Serialize) {
    map.insert(key.to_string(), serde_json::to_value(value).expect(key));
}

fn read_to_vec(mut reader: impl Read) -> Vec<u8> {
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).expect("read pipe");
    buf
}

/// Write `payload`, leave stdin open, and require the child to exit.
fn run_stdin_held(args: &[&str], payload: &[u8]) -> (Output, u128) {
    let mut child = bin()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let stderr = child.stderr.take().expect("stderr");
    let stdout_thread = thread::spawn(move || read_to_vec(stdout));
    let stderr_thread = thread::spawn(move || read_to_vec(stderr));
    stdin.write_all(payload).expect("write payload");
    stdin.flush().expect("flush");
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            let elapsed = started.elapsed().as_millis();
            drop(stdin);
            let output = Output {
                status,
                stdout: stdout_thread.join().expect("stdout thread"),
                stderr: stderr_thread.join().expect("stderr thread"),
            };
            return (output, elapsed);
        }
        if started.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            let _ = child.wait();
            drop(stdin);
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            panic!(
                "child kept stdin open past 30s args={args:?} bytes={}",
                payload.len()
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn queue_item<'a>(draft: &'a Value, item_id: &str, kind: &str) -> Option<&'a Value> {
    draft["review_queue"].as_array().and_then(|items| {
        items
            .iter()
            .find(|item| item["item_id"] == item_id && item["kind"] == kind)
    })
}

fn email_column(catalog: &Value) -> &Value {
    catalog["tables"]
        .as_array()
        .expect("tables")
        .iter()
        .find(|table| table["schema"] == "ledger" && table["name"] == "party")
        .expect("party")["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|column| column["name"] == "email")
        .expect("email")
}

#[test]
fn captured_pg_draft_process_oracle() {
    let dir = oracle_dir();
    fs::create_dir_all(&dir).expect("oracle dir");
    let mut failures: Vec<String> = Vec::new();

    let noargs_marker = format!(r#"{{"comment":"{RAW_MARKER}"}}"#);
    let (noargs, noargs_ms) = run_stdin_held(&[], noargs_marker.as_bytes());
    save_output(&dir, "noargs", &noargs);
    fs::write(dir.join("noargs.elapsed_ms"), format!("{noargs_ms}\n")).expect("elapsed");
    let noargs_err = stderr_text(&noargs);
    check(&mut failures, noargs.status.code() == Some(2), "noargs exit");
    check(&mut failures, noargs.stdout.is_empty(), "noargs stdout");
    check(
        &mut failures,
        noargs_err.contains(SKELETON_LINE),
        "noargs skeleton line",
    );
    check(
        &mut failures,
        noargs_err.contains("captured-pg-draft") && noargs_err.contains("structure-draft"),
        "noargs names commands",
    );
    check(
        &mut failures,
        !noargs_err.contains(RAW_MARKER),
        "noargs echoed marker",
    );

    let unknown = run(&["publish"], b"");
    save_output(&dir, "unknown", &unknown);
    check(&mut failures, unknown.status.code() == Some(2), "unknown exit");
    check(&mut failures, unknown.stdout.is_empty(), "unknown stdout");
    check(
        &mut failures,
        stderr_text(&unknown).contains("unknown command"),
        "unknown stderr",
    );

    let extra = run(&["captured-pg-draft", "--out"], RAW_MARKER.as_bytes());
    save_output(&dir, "extra", &extra);
    let extra_err = stderr_text(&extra);
    check(&mut failures, extra.status.code() == Some(2), "extra exit");
    check(&mut failures, extra.stdout.is_empty(), "extra stdout");
    check(
        &mut failures,
        extra_err.contains("takes no arguments"),
        "extra stderr",
    );
    check(
        &mut failures,
        !extra_err.contains(RAW_MARKER),
        "extra echoed marker",
    );

    let malformed = run(
        &["captured-pg-draft"],
        format!(r#"{{"owner":"{RAW_MARKER}","comment":"{RAW_MARKER}""#).as_bytes(),
    );
    save_output(&dir, "malformed", &malformed);
    let malformed_err = stderr_text(&malformed);
    check(
        &mut failures,
        malformed.status.code() == Some(1),
        "malformed exit",
    );
    check(
        &mut failures,
        malformed.stdout.is_empty(),
        "malformed stdout",
    );
    check(
        &mut failures,
        malformed_err.contains("json.syntax"),
        "malformed code",
    );
    check(
        &mut failures,
        !malformed_err.contains(RAW_MARKER) && malformed_err.len() < 400,
        "malformed echo or length",
    );

    let utf8 = run(&["captured-pg-draft"], &[0xff, 0xfe]);
    save_output(&dir, "utf8", &utf8);
    let utf8_err = stderr_text(&utf8);
    check(&mut failures, utf8.status.code() == Some(1), "utf8 exit");
    check(&mut failures, utf8.stdout.is_empty(), "utf8 stdout");
    check(
        &mut failures,
        utf8_err.contains("stdin is not UTF-8") && utf8_err.len() < 400,
        "utf8 stderr",
    );

    let exact = vec![b' '; DOCUMENT_MAX_BYTES];
    let exact_out = run(&["captured-pg-draft"], &exact);
    save_output(&dir, "exact-max", &exact_out);
    let exact_err = stderr_text(&exact_out);
    check(&mut failures, exact_out.status.code() == Some(1), "exact-max exit");
    check(
        &mut failures,
        exact_out.stdout.is_empty(),
        "exact-max stdout",
    );
    check(
        &mut failures,
        exact_err.contains("json.syntax") && !exact_err.contains("json.budget.bytes"),
        "exact-max is not the byte budget",
    );
    check(
        &mut failures,
        exact_err.len() < 400 && !exact_err.contains(&" ".repeat(64)),
        "exact-max echo",
    );

    let over_payload = vec![b' '; DOCUMENT_MAX_BYTES + 1];
    let (over, over_ms) = run_stdin_held(&["captured-pg-draft"], &over_payload);
    save_output(&dir, "oversize", &over);
    fs::write(dir.join("oversize.elapsed_ms"), format!("{over_ms}\n")).expect("over elapsed");
    fs::write(
        dir.join("oversize.note"),
        format!(
            "stdin_left_open_until_exit=yes bytes={} limit_plus_one={} elapsed_ms={over_ms}\n",
            over_payload.len(),
            DOCUMENT_MAX_BYTES + 1
        ),
    )
    .expect("over note");
    let over_err = stderr_text(&over);
    check(&mut failures, over.status.code() == Some(1), "oversize exit");
    check(&mut failures, over.stdout.is_empty(), "oversize stdout");
    check(
        &mut failures,
        over_err.contains("json.budget.bytes")
            && over_err.contains("document exceeds the byte budget"),
        "oversize budget diagnostic",
    );
    check(
        &mut failures,
        over_err.len() < 400 && !over_err.contains(&" ".repeat(64)),
        "oversize echo",
    );

    let catalog_bytes = fs::read(fixture_path()).expect("fixture");
    check(
        &mut failures,
        sha256_hex(&catalog_bytes) == FIXTURE_SHA256,
        "fixture sha256",
    );
    check(
        &mut failures,
        catalog_bytes.len() == 5644,
        "fixture length",
    );
    let catalog_text = std::str::from_utf8(&catalog_bytes).expect("fixture utf8");
    check(
        &mut failures,
        !catalog_text.contains(COMMENT_SENTENCE),
        "comment sentence in fixture",
    );
    let fixture_value: Value = serde_json::from_str(catalog_text).expect("fixture json");
    check(
        &mut failures,
        fixture_value["protocol"] == "oo.source.pgsql/v1",
        "fixture protocol",
    );
    check(
        &mut failures,
        fixture_value["schema_hash"] == SCHEMA_HASH,
        "fixture schema hash",
    );
    check(
        &mut failures,
        fixture_value["source_id"] == "ledger_src" && fixture_value["database"] == "oo_a157",
        "fixture source",
    );
    let fixture_comment = &email_column(&fixture_value)["comment"];
    check(
        &mut failures,
        fixture_comment["byte_len"] == 37
            && fixture_comment["sha256"] == COMMENT_SHA256
            && fixture_comment.get("body").is_none(),
        "fixture comment shape",
    );

    let mut planted_value = fixture_value.clone();
    planted_value["tables"]
        .as_array_mut()
        .expect("tables")
        .iter_mut()
        .find(|table| table["name"] == "party")
        .expect("party")["columns"]
        .as_array_mut()
        .expect("columns")
        .iter_mut()
        .find(|column| column["name"] == "email")
        .expect("email")["comment"]
        .as_object_mut()
        .expect("comment")
        .insert("body".to_string(), Value::String(PLANTED_BODY.to_string()));
    let planted = serde_json::to_vec(&planted_value).expect("planted");
    check(
        &mut failures,
        std::str::from_utf8(&planted).expect("planted utf8").contains(PLANTED_BODY),
        "planted input contains body",
    );
    let planted_out = run(&["captured-pg-draft"], &planted);
    save_output(&dir, "planted-body", &planted_out);
    let planted_err = stderr_text(&planted_out);
    check(
        &mut failures,
        planted_out.status.code() == Some(1),
        "planted body exit",
    );
    check(
        &mut failures,
        planted_out.stdout.is_empty(),
        "planted body stdout",
    );
    check(
        &mut failures,
        planted_err.contains("field.unknown")
            && !planted_err.contains(PLANTED_BODY)
            && planted_err.len() < 400,
        "planted body echoed",
    );

    let api = compile_captured_pg_catalog(catalog_text, None, None).expect("public api");
    let success = run(&["captured-pg-draft"], &catalog_bytes);
    save_output(&dir, "success", &success);
    let success_err = stderr_text(&success);
    check(&mut failures, success.status.code() == Some(0), "success exit");
    check(&mut failures, success.stderr.is_empty(), "success stderr");
    check(
        &mut failures,
        !success.stdout.ends_with(b"\n") && !success.stdout.contains(&b'\n'),
        "success newline",
    );
    let stdout = String::from_utf8(success.stdout.clone()).unwrap_or_default();
    let expected = format!(
        "{{\"draft\":{},\"evidence_envelope\":{},\"protocol\":\"{COMMAND_PROTOCOL}\",\"wrapper\":{}}}",
        api.draft, api.evidence_envelope, api.wrapper
    );
    check(&mut failures, stdout == expected, "stdout equals public api strings");
    let doc: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);
    check(
        &mut failures,
        canonical_json(&doc) == stdout,
        "stdout is canonical",
    );
    let keys: Vec<String> = doc
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();
    check(
        &mut failures,
        keys == ["draft", "evidence_envelope", "protocol", "wrapper"],
        "stdout keys",
    );
    check(&mut failures, doc["protocol"] == COMMAND_PROTOCOL, "protocol");
    let wrapper_value: Value = serde_json::from_str(&api.wrapper).unwrap_or(Value::Null);
    let envelope_value: Value = serde_json::from_str(&api.evidence_envelope).unwrap_or(Value::Null);
    let draft_value: Value = serde_json::from_str(&api.draft).unwrap_or(Value::Null);
    check(&mut failures, doc["wrapper"] == wrapper_value, "wrapper object");
    check(
        &mut failures,
        doc["evidence_envelope"] == envelope_value,
        "envelope object",
    );
    check(&mut failures, doc["draft"] == draft_value, "draft object");
    let round_trip = compile_structure_draft(&api.evidence_envelope).map(|bytes| bytes == api.draft);
    check(
        &mut failures,
        round_trip == Ok(true),
        "envelope recompiles to the draft bytes",
    );

    check(
        &mut failures,
        doc["wrapper"]["draft_hash"] == sha256_hex(api.draft.as_bytes()),
        "draft_hash",
    );
    check(
        &mut failures,
        doc["wrapper"]["evidence_envelope_hash"] == sha256_hex(api.evidence_envelope.as_bytes()),
        "evidence_envelope_hash",
    );
    check(
        &mut failures,
        api.draft == canonical_json(&doc["draft"]),
        "draft string is canonical",
    );
    check(
        &mut failures,
        api.evidence_envelope == canonical_json(&doc["evidence_envelope"]),
        "envelope string is canonical",
    );
    let catalog_hash = sha256_hex(canonical_json(&doc["evidence_envelope"]["catalog"]).as_bytes());
    check(
        &mut failures,
        doc["wrapper"]["captured_catalog_hash"] == catalog_hash,
        "captured_catalog_hash",
    );
    let mut wrapper_pre = doc["wrapper"].clone();
    wrapper_pre
        .as_object_mut()
        .map(|map| map.remove("wrapper_hash"));
    check(
        &mut failures,
        doc["wrapper"]["wrapper_hash"] == sha256_hex(canonical_json(&wrapper_pre).as_bytes()),
        "wrapper_hash",
    );
    let mut draft_pre = doc["draft"].clone();
    draft_pre["provenance"]
        .as_object_mut()
        .map(|map| map.remove("manifest_hash"));
    check(
        &mut failures,
        doc["draft"]["provenance"]["manifest_hash"]
            == sha256_hex(canonical_json(&draft_pre).as_bytes()),
        "manifest_hash",
    );
    check(
        &mut failures,
        doc["wrapper"]["manifest_hash"] == doc["draft"]["provenance"]["manifest_hash"],
        "wrapper manifest matches draft",
    );

    let relationship = doc["draft"]["generated"]["relationships"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["name"] == "invoice_party_fk"))
        .cloned()
        .unwrap_or(Value::Null);
    check(
        &mut failures,
        relationship["cardinality"] == "many_to_one"
            && relationship["provenance"] == "declared_fk"
            && relationship["status"] == "draft"
            && relationship["confidence"].is_null()
            && relationship["join_hint"] == "left",
        "invoice_party_fk",
    );
    let email = doc["draft"]["effective"]["datasets"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["dataset"] == "ledger.party"))
        .and_then(|dataset| dataset["columns"].as_array())
        .and_then(|columns| columns.iter().find(|column| column["name"] == "email"))
        .cloned()
        .unwrap_or(Value::Null);
    check(
        &mut failures,
        email["pii"].is_null() && email["semantic_type"].is_null(),
        "email pii and semantic type",
    );
    let pii_item = queue_item(&doc["draft"], "pii/ledger.party/email", "pii_unknown").cloned();
    let semantic_item =
        queue_item(&doc["draft"], "semantic/ledger.party/email", "semantic_unknown").cloned();
    check(
        &mut failures,
        pii_item.as_ref().is_some_and(|item| item["state"] == "pending"),
        "pii queue",
    );
    check(
        &mut failures,
        semantic_item.as_ref().is_some_and(|item| item["state"] == "pending"),
        "semantic queue",
    );
    check(
        &mut failures,
        doc["wrapper"]["row_import"] == "not_proven",
        "row_import",
    );
    check(
        &mut failures,
        doc["wrapper"]["grants"] == "not_authorized",
        "grants",
    );
    check(
        &mut failures,
        doc["wrapper"]["live_catalog_review"] == "pending",
        "live_catalog_review",
    );
    check(
        &mut failures,
        doc["wrapper"]["historical"]["pg_catalog_integration"] == "not_accepted",
        "historical integration",
    );
    check(
        &mut failures,
        doc["draft"]["provenance"]["pg_catalog_integration"] == "not_accepted",
        "draft integration",
    );
    check(
        &mut failures,
        doc["wrapper"]["llm"] == "not_used" && doc["wrapper"]["source_connection"] == "not_used",
        "llm and connection",
    );

    let envelope_catalog = &doc["evidence_envelope"]["catalog"];
    let tables = envelope_catalog["tables"].as_array().cloned().unwrap_or_default();
    let owners: Vec<String> = tables
        .iter()
        .filter_map(|table| table["owner"].as_str().map(str::to_string))
        .collect();
    let privilege_counts: Vec<usize> = tables
        .iter()
        .map(|table| table["privileges"].as_array().map(Vec::len).unwrap_or(0))
        .collect();
    let envelope_comment = &email_column(envelope_catalog)["comment"];
    check(
        &mut failures,
        owners == [OWNER, OWNER] && privilege_counts == [9, 9],
        "envelope keeps owner and privileges",
    );
    check(
        &mut failures,
        envelope_comment["byte_len"] == 37
            && envelope_comment["sha256"] == COMMENT_SHA256
            && envelope_comment.get("body").is_none(),
        "envelope comment has no body",
    );
    let draft_text = canonical_json(&doc["draft"]);
    let wrapper_text = canonical_json(&doc["wrapper"]);
    check(
        &mut failures,
        !draft_text.contains(OWNER) && !wrapper_text.contains(OWNER),
        "owner leaked into draft or wrapper",
    );
    check(
        &mut failures,
        !draft_text.contains("grantable") && !wrapper_text.contains("grantable"),
        "privileges leaked into draft or wrapper",
    );
    check(
        &mut failures,
        !stdout.contains(COMMENT_SENTENCE) && !stdout.contains(PLANTED_BODY),
        "comment body in stdout",
    );
    check(
        &mut failures,
        doc.get("owner").is_none(),
        "command object has no top-level owner",
    );

    let golden_input = fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/ontology-draft/goldens/customer.input.json"),
    )
    .expect("golden input");
    let golden_draft = fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/ontology-draft/goldens/customer.draft.json"),
    )
    .expect("golden draft");
    let legacy = run(&["structure-draft"], &golden_input);
    fs::write(
        dir.join("structure-draft.exit"),
        format!("{}\n", legacy.status.code().unwrap_or(127)),
    )
    .expect("legacy exit");
    fs::write(dir.join("structure-draft.stderr"), &legacy.stderr).expect("legacy stderr");
    fs::write(dir.join("structure-draft.stdout"), &legacy.stdout).expect("legacy stdout");
    fs::write(
        dir.join("structure-draft.stdout.sha256"),
        format!("{}\n", sha256_hex(&legacy.stdout)),
    )
    .expect("legacy hash");
    fs::write(
        dir.join("structure-draft.golden.sha256"),
        format!("{}\n", sha256_hex(&golden_draft)),
    )
    .expect("golden hash");
    check(&mut failures, legacy.status.code() == Some(0), "structure-draft exit");
    check(
        &mut failures,
        legacy.stdout == golden_draft && !legacy.stdout.ends_with(b"\n"),
        "structure-draft bytes",
    );
    check(&mut failures, legacy.stderr.is_empty(), "structure-draft stderr");
    let legacy_reject = run(
        &["structure-draft"],
        br#"{"protocol":"oo.builder.catalog-evidence/v1","protocol":"again"}"#,
    );
    save_output(&dir, "structure-draft-duplicate", &legacy_reject);
    check(
        &mut failures,
        legacy_reject.status.code() == Some(1) && legacy_reject.stdout.is_empty(),
        "structure-draft duplicate exit",
    );
    check(
        &mut failures,
        stderr_text(&legacy_reject).contains("json.duplicate_key"),
        "structure-draft duplicate code",
    );

    let mut summary = Map::new();
    put_json(&mut summary, "fixture_sha256", sha256_hex(&catalog_bytes));
    put_json(&mut summary, "fixture_bytes", catalog_bytes.len());
    put_json(&mut summary, "schema_hash", &fixture_value["schema_hash"]);
    put_json(&mut summary, "source_id", &fixture_value["source_id"]);
    put_json(&mut summary, "database", &fixture_value["database"]);
    put_json(
        &mut summary,
        "comment_sentence_in_fixture",
        catalog_text.contains(COMMENT_SENTENCE),
    );
    put_json(&mut summary, "success_exit", success.status.code());
    put_json(&mut summary, "success_stdout_bytes", stdout.len());
    put_json(&mut summary, "success_stdout_sha256", sha256_hex(stdout.as_bytes()));
    put_json(&mut summary, "success_stderr_empty", success_err.is_empty());
    put_json(
        &mut summary,
        "stdout_equals_public_api_strings",
        stdout == expected,
    );
    put_json(
        &mut summary,
        "stdout_canonical",
        canonical_json(&doc) == stdout,
    );
    put_json(&mut summary, "no_newline", !stdout.as_bytes().contains(&b'\n'));
    put_json(&mut summary, "draft_hash", &doc["wrapper"]["draft_hash"]);
    put_json(
        &mut summary,
        "evidence_envelope_hash",
        &doc["wrapper"]["evidence_envelope_hash"],
    );
    put_json(
        &mut summary,
        "captured_catalog_hash",
        &doc["wrapper"]["captured_catalog_hash"],
    );
    put_json(&mut summary, "wrapper_hash", &doc["wrapper"]["wrapper_hash"]);
    put_json(
        &mut summary,
        "manifest_hash",
        &doc["draft"]["provenance"]["manifest_hash"],
    );
    put_json(&mut summary, "row_import", &doc["wrapper"]["row_import"]);
    put_json(&mut summary, "grants", &doc["wrapper"]["grants"]);
    put_json(
        &mut summary,
        "live_catalog_review",
        &doc["wrapper"]["live_catalog_review"],
    );
    put_json(
        &mut summary,
        "historical_pg_catalog_integration",
        &doc["wrapper"]["historical"]["pg_catalog_integration"],
    );
    put_json(
        &mut summary,
        "draft_pg_catalog_integration",
        &doc["draft"]["provenance"]["pg_catalog_integration"],
    );
    put_json(&mut summary, "relationship", &relationship);
    put_json(
        &mut summary,
        "email_pii",
        email.get("pii").cloned().unwrap_or(Value::Null),
    );
    put_json(
        &mut summary,
        "email_semantic_type",
        email.get("semantic_type").cloned().unwrap_or(Value::Null),
    );
    put_json(&mut summary, "pii_queue", &pii_item);
    put_json(&mut summary, "semantic_queue", &semantic_item);
    put_json(&mut summary, "envelope_owners", &owners);
    put_json(&mut summary, "envelope_privilege_counts", &privilege_counts);
    put_json(&mut summary, "owner_in_draft", draft_text.contains(OWNER));
    put_json(&mut summary, "owner_in_wrapper", wrapper_text.contains(OWNER));
    put_json(
        &mut summary,
        "grantable_in_draft",
        draft_text.contains("grantable"),
    );
    put_json(
        &mut summary,
        "grantable_in_wrapper",
        wrapper_text.contains("grantable"),
    );
    put_json(
        &mut summary,
        "comment_sentence_in_stdout",
        stdout.contains(COMMENT_SENTENCE),
    );
    put_json(&mut summary, "envelope_comment", envelope_comment);
    put_json(&mut summary, "noargs_exit", noargs.status.code());
    put_json(
        &mut summary,
        "noargs_elapsed_ms",
        u64::try_from(noargs_ms).unwrap_or(u64::MAX),
    );
    put_json(&mut summary, "noargs_skeleton_line", SKELETON_LINE);
    put_json(&mut summary, "noargs_stderr", noargs_err);
    put_json(&mut summary, "oversize_exit", over.status.code());
    put_json(
        &mut summary,
        "oversize_elapsed_ms_stdin_held_open",
        u64::try_from(over_ms).unwrap_or(u64::MAX),
    );
    put_json(&mut summary, "oversize_bytes", DOCUMENT_MAX_BYTES + 1);
    put_json(&mut summary, "exact_max_exit", exact_out.status.code());
    put_json(&mut summary, "planted_body_exit", planted_out.status.code());
    put_json(&mut summary, "structure_draft_exit", legacy.status.code());
    put_json(
        &mut summary,
        "structure_draft_bytes_equal",
        legacy.stdout == golden_draft,
    );
    put_json(&mut summary, "failures", &failures);
    fs::write(
        dir.join("oracle-result.json"),
        serde_json::to_string_pretty(&Value::Object(summary)).expect("summary") + "\n",
    )
    .expect("summary write");
    if !failures.is_empty() {
        panic!("oracle failures: {}", failures.join("; "));
    }
}
