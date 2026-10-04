//! Independent process oracle for `oo-builder review-pg-draft`.
//!
//! The test spawns the built binary. Expected objects come from
//! `compile_captured_pg_catalog` on the a157 catalogs. The overlay does not
//! edit the compiler.

#![forbid(unsafe_code)]

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use oo_builder::{CapturedPgDraft, compile_captured_pg_catalog, compile_structure_draft};
use oo_core::limits::DOCUMENT_MAX_BYTES;

const COMMAND_PROTOCOL: &str = "oo.builder.captured-pg-draft-command/v1";
const BEFORE_FILE: &str = "de094222fcd5cb4e54c95a5ec8950c8fd2d55837bd1cff8cd3dd49673f8d56d9";
const AFTER_FILE: &str = "53e2fe7e68197f8698a2a9e2683dc2131426a0faea6bfb277594f9ba9ce52903";
const BEFORE_SCHEMA: &str = "a0efbfc8b5bc7536a121208a1c8e3ff41b16cdda4018b5618b047f40d415271c";
const AFTER_SCHEMA: &str = "c123519256e1c88ab063a41f250ac02483822983d3cf95c364d25983e86fde4d";
const BEFORE_DRAFT: &str = "d6980c138fa4939195bd673582938c94e9adca72a9acf710ddcebae407b138ef";
const BEFORE_WRAPPER: &str = "e525520f010677050108f4aed9d899ecca28e2c56cbcb91e10d9485d20e3267e";
const BEFORE_CAPTURED: &str = "5c17683d74bcc7916954dfb23d22004e5ccbd4058f54bb1d3699ca679a8b90b0";
const AFTER_CAPTURED: &str = "6f4fca46a3144f9e5c0073fec0945af70717cde52a18c257ec74b0d60109c92c";
const AFTER_STALE_DRAFT: &str = "8e00ba219f9e99044a739acb3f96eff04f7ce44784a42bec1609e32ee7ae688f";
const HISTORICAL: &str = "b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74";
const EMAIL_ITEM: &str = "column/ledger.party/email";
const ID_ITEM: &str = "column/ledger.party/id";
const REL_ITEM: &str = "relationship/ledger.invoice/invoice_party_fk";
const MARKER: &str = "a163-do-not-echo";
const OWNER: &str = "a157_owner";
const COMMENT_SENTENCE: &str = "a157-review-comment-body-do-not-carry";
const USAGE: &str = "\
oo-builder: usage: oo-builder <command>\n\
oo-builder: public draft commands read one JSON document from stdin and take no arguments\n\
oo-builder:   structure-draft       one oo.builder.catalog-evidence/v1 envelope\n\
oo-builder:   captured-pg-draft     one oo.source.pgsql/v1 catalog\n\
oo-builder:   review-pg-draft       catalog object, optional curated object, optional reviews array\n\
oo-builder: review-pg-draft passes those values to compile_captured_pg_catalog\n\
oo-builder: these commands do not connect, call a model, import rows, grant, certify, or publish\n\
oo-builder: a review decision binds one structural item and does not approve semantics, grants, or publication\n";

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oo-builder"))
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out",
    )
}

fn oracle_dir() -> PathBuf {
    PathBuf::from(std::env::var("A163_ORACLE_DIR").expect("A163_ORACLE_DIR"))
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

fn object(pairs: Vec<(&str, Value)>) -> Value {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

fn check(failures: &mut Vec<String>, cond: bool, message: impl AsRef<str>) {
    if !cond {
        failures.push(message.as_ref().to_string());
    }
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap_or_else(|_| String::from("invalid-utf8"))
}

fn save_output(dir: &Path, name: &str, output: &Output) {
    let code = output.status.code().unwrap_or(127);
    fs::write(dir.join(format!("{name}.exit")), format!("{code}\n")).expect("exit");
    fs::write(dir.join(format!("{name}.stdout")), &output.stdout).expect("stdout");
    fs::write(dir.join(format!("{name}.stderr")), &output.stderr).expect("stderr");
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

fn read_to_vec(mut reader: impl Read) -> Vec<u8> {
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).expect("read pipe");
    buf
}

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
            panic!("child kept stdin open past 30s bytes={}", payload.len());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn load_catalog(name: &str, expected: &str) -> (Vec<u8>, Value) {
    let path = fixture_dir().join(name);
    let bytes = fs::read(&path).unwrap_or_else(|_| panic!("read {name}"));
    assert_eq!(sha256_hex(&bytes), expected, "{name} sha256");
    let text = std::str::from_utf8(&bytes).unwrap_or_else(|_| panic!("{name} utf8"));
    let value = serde_json::from_str(text.trim()).unwrap_or_else(|_| panic!("parse {name}"));
    (bytes, value)
}

fn command_stdout(api: &CapturedPgDraft) -> String {
    format!(
        "{{\"draft\":{},\"evidence_envelope\":{},\"protocol\":\"{COMMAND_PROTOCOL}\",\"wrapper\":{}}}",
        api.draft, api.evidence_envelope, api.wrapper
    )
}

fn recompute_wrapper_hash(wrapper: &str) -> Result<String, String> {
    let mut value: Value =
        serde_json::from_str(wrapper).map_err(|_| String::from("wrapper is not json"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| String::from("wrapper is not an object"))?;
    object.remove("wrapper_hash");
    Ok(sha256_hex(canonical_json(&value).as_bytes()))
}

fn queue<'a>(draft: &'a Value, item_id: &str, kind: &str) -> Option<&'a Value> {
    draft.get("review_queue")?.as_array()?.iter().find(|item| {
        item.get("item_id").and_then(Value::as_str) == Some(item_id)
            && item.get("kind").and_then(Value::as_str) == Some(kind)
    })
}

fn fingerprint(draft: &Value, item_id: &str, kind: &str) -> Option<String> {
    queue(draft, item_id, kind)
        .and_then(|item| item.get("fact_fingerprint"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn column<'a>(draft: &'a Value, layer: &str, dataset: &str, name: &str) -> Option<&'a Value> {
    let datasets = draft.get(layer)?.get("datasets")?.as_array()?;
    let row = datasets
        .iter()
        .find(|item| item.get("dataset").and_then(Value::as_str) == Some(dataset))?;
    row.get("columns")?.as_array()?.iter().find(|item| {
        item.get("name").and_then(Value::as_str) == Some(name)
    })
}

fn column_names(draft: &Value, layer: &str, dataset: &str) -> Vec<String> {
    let Some(datasets) = draft
        .get(layer)
        .and_then(|value| value.get("datasets"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let Some(row) = datasets
        .iter()
        .find(|item| item.get("dataset").and_then(Value::as_str) == Some(dataset))
    else {
        return Vec::new();
    };
    row.get("columns")
        .and_then(Value::as_array)
        .map(|columns| {
            columns
                .iter()
                .filter_map(|item| item.get("name").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn has_relationship(draft: &Value, layer: &str, name: &str) -> bool {
    draft
        .get(layer)
        .and_then(|value| value.get("relationships"))
        .and_then(Value::as_array)
        .is_some_and(|rows| {
            rows.iter()
                .any(|item| item.get("name").and_then(Value::as_str) == Some(name))
        })
}

fn review_of<'a>(draft: &'a Value, item_id: &str) -> Option<&'a Value> {
    draft.get("reviews")?.as_array()?.iter().find(|item| {
        item.get("item_id").and_then(Value::as_str) == Some(item_id)
    })
}

fn curated_column(name: &str, fields: Vec<(&str, Value)>) -> Value {
    let mut pairs = vec![("name", Value::String(name.to_string()))];
    pairs.extend(fields);
    object(pairs)
}

fn curated_doc(column: Value) -> Value {
    let dataset = object(vec![
        ("columns", Value::Array(vec![column])),
        ("dataset", Value::String("ledger.party".to_string())),
    ]);
    object(vec![
        ("datasets", Value::Array(vec![dataset])),
        ("relationships", Value::Array(Vec::new())),
    ])
}

fn decision(item_id: &str, action: &str, schema_hash: &str, fact_fingerprint: &str) -> Value {
    object(vec![
        ("action", Value::String(action.to_string())),
        (
            "fact_fingerprint",
            Value::String(fact_fingerprint.to_string()),
        ),
        ("item_id", Value::String(item_id.to_string())),
        ("schema_hash", Value::String(schema_hash.to_string())),
    ])
}

fn review_stdin(catalog: &Value, curated: Option<&Value>, reviews: Option<&Value>) -> String {
    let mut map = Map::new();
    map.insert("catalog".to_string(), catalog.clone());
    if let Some(value) = curated {
        map.insert("curated".to_string(), value.clone());
    }
    if let Some(value) = reviews {
        map.insert("reviews".to_string(), value.clone());
    }
    canonical_json(&Value::Object(map))
}

struct Inputs {
    catalog_json: String,
    curated_json: Option<String>,
    reviews_json: Option<String>,
}

fn inputs_for(catalog: &Value, curated: Option<&Value>, reviews: Option<&Value>) -> Inputs {
    Inputs {
        catalog_json: canonical_json(catalog),
        curated_json: curated.map(canonical_json),
        reviews_json: reviews.map(canonical_json),
    }
}

fn expect_static(
    failures: &mut Vec<String>,
    dir: &Path,
    name: &str,
    output: &Output,
    exit_code: i32,
    stderr_exact: &str,
) {
    save_output(dir, name, output);
    let code = output.status.code();
    check(
        failures,
        code == Some(exit_code),
        format!("{name} exit {code:?} expected {exit_code}"),
    );
    check(
        failures,
        output.stdout.is_empty(),
        format!("{name} stdout {} bytes", output.stdout.len()),
    );
    let stderr = stderr_text(output);
    check(
        failures,
        stderr == stderr_exact,
        format!("{name} stderr {stderr:?}"),
    );
    check(
        failures,
        !stderr.contains(MARKER),
        format!("{name} echoed marker"),
    );
    check(
        failures,
        !output.stdout.windows(MARKER.len()).any(|window| window == MARKER.as_bytes()),
        format!("{name} stdout echoed marker"),
    );
}

fn expect_success(
    failures: &mut Vec<String>,
    summary: &mut Map<String, Value>,
    dir: &Path,
    name: &str,
    stdin: &str,
    inputs: &Inputs,
) -> Option<Value> {
    let api = match compile_captured_pg_catalog(
        &inputs.catalog_json,
        inputs.curated_json.as_deref(),
        inputs.reviews_json.as_deref(),
    ) {
        Ok(api) => api,
        Err(err) => {
            failures.push(format!("{name} api {} {}", err.code, err.message));
            return None;
        }
    };
    let output = run(&["review-pg-draft"], stdin.as_bytes());
    save_output(dir, name, &output);
    let code = output.status.code();
    check(
        failures,
        code == Some(0),
        format!("{name} exit {code:?} {}", stderr_text(&output)),
    );
    check(
        failures,
        output.stderr.is_empty(),
        format!("{name} stderr {}", stderr_text(&output)),
    );
    check(
        failures,
        !output.stdout.contains(&b'\n'),
        format!("{name} stdout contains newline"),
    );
    check(
        failures,
        output.stdout.last() == Some(&b'}'),
        format!("{name} stdout does not end on an object"),
    );
    let stdout = String::from_utf8(output.stdout.clone()).unwrap_or_default();
    let expected = command_stdout(&api);
    check(
        failures,
        stdout == expected,
        format!(
            "{name} stdout {} bytes expected {} bytes sha {}",
            stdout.len(),
            expected.len(),
            sha256_hex(stdout.as_bytes())
        ),
    );
    summary.insert(
        format!("{name}_stdout_sha256"),
        Value::String(sha256_hex(stdout.as_bytes())),
    );
    summary.insert(
        format!("{name}_draft_hash"),
        Value::String(api.draft_hash.clone()),
    );
    let doc: Value = match serde_json::from_str(&stdout) {
        Ok(value) => value,
        Err(_) => {
            failures.push(format!("{name} stdout is not json"));
            return None;
        }
    };
    check(
        failures,
        canonical_json(&doc) == stdout,
        format!("{name} stdout is not key-sorted canonical json"),
    );
    check(
        failures,
        doc.get("protocol").and_then(Value::as_str) == Some(COMMAND_PROTOCOL),
        format!("{name} protocol"),
    );
    check(
        failures,
        sha256_hex(api.draft.as_bytes()) == api.draft_hash,
        format!("{name} draft_hash"),
    );
    check(
        failures,
        sha256_hex(api.evidence_envelope.as_bytes()) == api.evidence_envelope_hash,
        format!("{name} evidence_envelope_hash"),
    );
    match recompute_wrapper_hash(&api.wrapper) {
        Ok(hash) => check(
            failures,
            hash == api.wrapper_hash,
            format!("{name} wrapper_hash recomputed {hash} api {}", api.wrapper_hash),
        ),
        Err(err) => failures.push(format!("{name} wrapper_hash {err}")),
    }
    check(
        failures,
        doc.get("wrapper")
            .and_then(|value| value.get("draft_hash"))
            .and_then(Value::as_str)
            == Some(api.draft_hash.as_str()),
        format!("{name} wrapper draft_hash field"),
    );
    check(
        failures,
        doc.get("wrapper")
            .and_then(|value| value.get("wrapper_hash"))
            .and_then(Value::as_str)
            == Some(api.wrapper_hash.as_str()),
        format!("{name} wrapper_hash field"),
    );
    check(
        failures,
        doc.get("wrapper")
            .and_then(|value| value.get("captured_catalog_hash"))
            .and_then(Value::as_str)
            == Some(api.captured_catalog_hash.as_str()),
        format!("{name} captured_catalog_hash"),
    );
    check(
        failures,
        doc.get("draft")
            .and_then(|value| value.get("provenance"))
            .and_then(|value| value.get("manifest_hash"))
            .and_then(Value::as_str)
            == Some(api.manifest_hash.as_str()),
        format!("{name} manifest_hash"),
    );
    match compile_structure_draft(&api.evidence_envelope) {
        Ok(again) => check(
            failures,
            again == api.draft,
            format!("{name} envelope does not compile to the draft bytes"),
        ),
        Err(err) => failures.push(format!("{name} roundtrip {} {}", err.code, err.message)),
    }
    let wrapper = doc.get("wrapper").cloned().unwrap_or(Value::Null);
    let draft = doc.get("draft").cloned().unwrap_or(Value::Null);
    for (field, expected) in [
        ("live_catalog_review", "pending"),
        ("row_import", "not_proven"),
        ("grants", "not_authorized"),
        ("publication", "unavailable"),
        ("certification", "none"),
        ("llm", "not_used"),
        ("source_connection", "not_used"),
    ] {
        check(
            failures,
            wrapper.get(field).and_then(Value::as_str) == Some(expected),
            format!("{name} wrapper {field} {:?}", wrapper.get(field)),
        );
    }
    check(
        failures,
        wrapper
            .get("historical")
            .and_then(|value| value.get("pg_catalog_integration"))
            .and_then(Value::as_str)
            == Some("not_accepted"),
        format!("{name} wrapper historical integration"),
    );
    check(
        failures,
        wrapper
            .get("historical")
            .and_then(|value| value.get("pg_catalog_contract_commit"))
            .and_then(Value::as_str)
            == Some(HISTORICAL),
        format!("{name} wrapper historical commit"),
    );
    let provenance = draft.get("provenance").cloned().unwrap_or(Value::Null);
    check(
        failures,
        provenance
            .get("pg_catalog_integration")
            .and_then(Value::as_str)
            == Some("not_accepted"),
        format!("{name} draft integration"),
    );
    check(
        failures,
        provenance
            .get("pg_catalog_contract_commit")
            .and_then(Value::as_str)
            == Some(HISTORICAL),
        format!("{name} draft historical commit"),
    );
    check(
        failures,
        provenance.get("publication").and_then(Value::as_str) == Some("unavailable"),
        format!("{name} draft publication"),
    );
    check(
        failures,
        provenance.get("certification").and_then(Value::as_str) == Some("none"),
        format!("{name} draft certification"),
    );
    check(
        failures,
        !api.draft.contains(OWNER),
        format!("{name} draft contains owner"),
    );
    check(
        failures,
        !api.wrapper.contains(OWNER),
        format!("{name} wrapper contains owner"),
    );
    check(
        failures,
        api.evidence_envelope.contains(OWNER),
        format!("{name} envelope dropped owner"),
    );
    check(
        failures,
        !stdout.contains(COMMENT_SENTENCE),
        format!("{name} stdout contains the comment sentence"),
    );
    check(
        failures,
        !stdout.contains(MARKER),
        format!("{name} stdout contains marker"),
    );
    Some(doc)
}

fn require_queue<'a>(
    failures: &mut Vec<String>,
    draft: &'a Value,
    name: &str,
    item_id: &str,
    kind: &str,
) -> Option<&'a Value> {
    let found = queue(draft, item_id, kind);
    check(
        failures,
        found.is_some(),
        format!("{name} missing queue {kind} {item_id}"),
    );
    found
}

#[test]
fn review_pg_draft_process_matches_the_public_api() {
    let dir = oracle_dir();
    fs::create_dir_all(&dir).expect("oracle dir");
    let mut failures = Vec::new();
    let mut summary = Map::new();

    let (before_bytes, before) = load_catalog("catalog-before.json", BEFORE_FILE);
    let (after_bytes, after) = load_catalog("catalog-after.json", AFTER_FILE);
    summary.insert(
        "catalog_before_bytes".to_string(),
        Value::from(before_bytes.len() as u64),
    );
    summary.insert(
        "catalog_after_bytes".to_string(),
        Value::from(after_bytes.len() as u64),
    );
    let before_text = std::str::from_utf8(&before_bytes).expect("before utf8");
    let raw_api = compile_captured_pg_catalog(before_text, None, None).expect("raw api");
    let canonical_api =
        compile_captured_pg_catalog(&canonical_json(&before), None, None).expect("canonical api");
    check(
        &mut failures,
        raw_api.draft == canonical_api.draft
            && raw_api.wrapper == canonical_api.wrapper
            && raw_api.evidence_envelope == canonical_api.evidence_envelope,
        "canonical catalog changed the bare seals",
    );

    let (noargs, noargs_ms) = run_stdin_held(&[], MARKER.as_bytes());
    fs::write(dir.join("noargs.elapsed_ms"), format!("{noargs_ms}\n")).expect("elapsed");
    expect_static(&mut failures, &dir, "noargs", &noargs, 2, USAGE);
    check(
        &mut failures,
        noargs_ms < 5_000,
        format!("noargs elapsed {noargs_ms} ms"),
    );
    check(
        &mut failures,
        !stderr_text(&noargs).contains("process entry is not implemented"),
        "noargs still prints the skeleton sentence",
    );
    check(
        &mut failures,
        !stderr_text(&noargs).contains("SP00-W01 skeleton"),
        "noargs still names the skeleton",
    );

    let unknown = run(&["publish"], b"");
    expect_static(
        &mut failures,
        &dir,
        "unknown",
        &unknown,
        2,
        "oo-builder: unknown command\n",
    );
    let review_extra = run(&["review-pg-draft", MARKER], MARKER.as_bytes());
    expect_static(
        &mut failures,
        &dir,
        "review-extra",
        &review_extra,
        2,
        "oo-builder: review-pg-draft reads one JSON document from stdin and takes no arguments\n",
    );
    let captured_extra = run(&["captured-pg-draft", "--out"], b"");
    expect_static(
        &mut failures,
        &dir,
        "captured-extra",
        &captured_extra,
        2,
        "oo-builder: captured-pg-draft reads one oo.source.pgsql/v1 JSON document from stdin and takes no arguments\n",
    );
    let structure_extra = run(&["structure-draft", "--publish"], b"");
    expect_static(
        &mut failures,
        &dir,
        "structure-extra",
        &structure_extra,
        2,
        "oo-builder: structure-draft reads JSON from stdin and takes no arguments\n",
    );

    let bare_stdin = review_stdin(&before, None, None);
    let bare_inputs = inputs_for(&before, None, None);
    let bare_doc = expect_success(
        &mut failures,
        &mut summary,
        &dir,
        "bare",
        &bare_stdin,
        &bare_inputs,
    );
    if let Some(doc) = &bare_doc {
        check(
            &mut failures,
            doc["wrapper"]["schema_hash"].as_str() == Some(BEFORE_SCHEMA),
            "bare schema hash",
        );
        check(
            &mut failures,
            doc["wrapper"]["draft_hash"].as_str() == Some(BEFORE_DRAFT),
            format!("bare draft hash {:?}", doc["wrapper"]["draft_hash"]),
        );
        check(
            &mut failures,
            doc["wrapper"]["wrapper_hash"].as_str() == Some(BEFORE_WRAPPER),
            "bare wrapper hash",
        );
        check(
            &mut failures,
            doc["wrapper"]["captured_catalog_hash"].as_str() == Some(BEFORE_CAPTURED),
            "bare captured catalog hash",
        );
        let captured = run(&["captured-pg-draft"], &before_bytes);
        save_output(&dir, "captured-bare", &captured);
        check(
            &mut failures,
            captured.status.code() == Some(0),
            format!("captured-pg-draft exit {:?}", captured.status.code()),
        );
        check(
            &mut failures,
            captured.stdout == fs::read(dir.join("bare.stdout")).unwrap_or_default(),
            "captured-pg-draft stdout differs from review-pg-draft catalog-only",
        );
        check(
            &mut failures,
            !captured.stdout.contains(&b'\n'),
            "captured-pg-draft stdout contains newline",
        );
    }

    let empty_reviews = Value::Array(Vec::new());
    let empty_doc = expect_success(
        &mut failures,
        &mut summary,
        &dir,
        "empty-reviews",
        &review_stdin(&before, None, Some(&empty_reviews)),
        &inputs_for(&before, None, Some(&empty_reviews)),
    );
    if let Some(doc) = &empty_doc {
        check(
            &mut failures,
            doc["wrapper"]["draft_hash"].as_str() == Some(BEFORE_DRAFT),
            "empty reviews changed the bare draft hash",
        );
    }

    let bare_draft: Value = serde_json::from_str(&raw_api.draft).expect("bare draft json");
    let email_fp = fingerprint(&bare_draft, EMAIL_ITEM, "structural_column").unwrap_or_default();
    let id_fp = fingerprint(&bare_draft, ID_ITEM, "structural_column").unwrap_or_default();
    let rel_fp = fingerprint(&bare_draft, REL_ITEM, "candidate_relationship").unwrap_or_default();
    check(
        &mut failures,
        email_fp.len() == 64 && email_fp.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()),
        format!("email fingerprint {email_fp}"),
    );
    check(
        &mut failures,
        id_fp.len() == 64,
        format!("id fingerprint length {}", id_fp.len()),
    );
    check(
        &mut failures,
        rel_fp.len() == 64,
        format!("relationship fingerprint length {}", rel_fp.len()),
    );
    summary.insert("email_fact_fingerprint".to_string(), Value::String(email_fp.clone()));
    summary.insert("id_fact_fingerprint".to_string(), Value::String(id_fp.clone()));
    summary.insert("rel_fact_fingerprint".to_string(), Value::String(rel_fp.clone()));

    let accept = Value::Array(vec![decision(EMAIL_ITEM, "accept", BEFORE_SCHEMA, &email_fp)]);
    let accepted = expect_success(
        &mut failures,
        &mut summary,
        &dir,
        "accept",
        &review_stdin(&before, None, Some(&accept)),
        &inputs_for(&before, None, Some(&accept)),
    );
    if let Some(doc) = &accepted {
        let draft = &doc["draft"];
        if let Some(item) = require_queue(&mut failures, draft, "accept", EMAIL_ITEM, "structural_column") {
            check(&mut failures, item["state"] == "accepted", "accept state");
            check(&mut failures, item["publishable"] == false, "accept publishable");
        }
        if let Some(email) = column(draft, "effective", "ledger.party", "email") {
            check(&mut failures, email["pii"].is_null(), "accept effective pii");
            check(
                &mut failures,
                email["semantic_type"].is_null(),
                "accept effective semantic type",
            );
        } else {
            failures.push(String::from("accept missing effective email"));
        }
        if let Some(item) = require_queue(&mut failures, draft, "accept", "pii/ledger.party/email", "pii_unknown") {
            check(&mut failures, item["state"] == "pending", "accept pii state");
            check(&mut failures, item["publishable"] == false, "accept pii publishable");
        }
        if let Some(item) = require_queue(
            &mut failures,
            draft,
            "accept",
            "semantic/ledger.party/email",
            "semantic_unknown",
        ) {
            check(
                &mut failures,
                item["publishable"] == false,
                "accept semantic publishable",
            );
        }
        check(
            &mut failures,
            doc["wrapper"]["schema_hash"] == BEFORE_SCHEMA,
            "accept changed schema hash",
        );
        check(
            &mut failures,
            doc["wrapper"]["draft_hash"] != BEFORE_DRAFT,
            "accept left the bare draft hash in place",
        );
        check(
            &mut failures,
            doc["wrapper"]["live_catalog_review"] == "pending",
            "accept changed live_catalog_review",
        );
    }

    let closure_curated = curated_doc(curated_column(
        "id",
        vec![
            ("label", Value::String("Kept label".to_string())),
            ("status", Value::String("rejected".to_string())),
        ],
    ));
    let closure_reviews = Value::Array(vec![
        decision(REL_ITEM, "accept", BEFORE_SCHEMA, &rel_fp),
        decision(ID_ITEM, "reject", BEFORE_SCHEMA, &id_fp),
    ]);
    let closed = expect_success(
        &mut failures,
        &mut summary,
        &dir,
        "closure",
        &review_stdin(&before, Some(&closure_curated), Some(&closure_reviews)),
        &inputs_for(&before, Some(&closure_curated), Some(&closure_reviews)),
    );
    if let Some(doc) = &closed {
        let draft = &doc["draft"];
        let generated_names = column_names(draft, "generated", "ledger.party");
        let effective_names = column_names(draft, "effective", "ledger.party");
        check(
            &mut failures,
            generated_names.iter().any(|name| name == "id"),
            "closure dropped generated id",
        );
        check(
            &mut failures,
            !effective_names.iter().any(|name| name == "id"),
            format!("closure effective still has id {effective_names:?}"),
        );
        if let Some(id) = column(draft, "curated", "ledger.party", "id") {
            check(&mut failures, id["status"] == "rejected", "closure curated status");
            check(&mut failures, id["label"] == "Kept label", "closure curated label");
        } else {
            failures.push(String::from("closure missing curated id"));
        }
        check(
            &mut failures,
            has_relationship(draft, "generated", "invoice_party_fk"),
            "closure dropped generated relationship",
        );
        check(
            &mut failures,
            !has_relationship(draft, "effective", "invoice_party_fk"),
            "closure left the relationship effective",
        );
        if let Some(item) = require_queue(
            &mut failures,
            draft,
            "closure",
            REL_ITEM,
            "relationship_endpoint_not_effective",
        ) {
            check(
                &mut failures,
                item["detail"] == "to_column_not_effective",
                format!("closure detail {:?}", item["detail"]),
            );
            check(&mut failures, item["state"] == "pending", "closure endpoint state");
            check(
                &mut failures,
                item["publishable"] == false,
                "closure endpoint publishable",
            );
        }
        if let Some(item) = require_queue(&mut failures, draft, "closure", REL_ITEM, "candidate_relationship") {
            check(&mut failures, item["state"] == "accepted", "closure relationship state");
        }
        if let Some(item) = review_of(draft, REL_ITEM) {
            check(&mut failures, item["action"] == "accept", "closure review action");
        } else {
            failures.push(String::from("closure missing relationship review"));
        }
        if let Some(item) = review_of(draft, ID_ITEM) {
            check(&mut failures, item["action"] == "reject", "closure id action");
        } else {
            failures.push(String::from("closure missing id review"));
        }
    }

    let overlay = curated_doc(curated_column(
        "email",
        vec![
            ("label", Value::String("Correspondence".to_string())),
            ("pii", Value::String("sensitive".to_string())),
        ],
    ));
    let override_review = Value::Array(vec![decision(EMAIL_ITEM, "override", BEFORE_SCHEMA, &email_fp)]);
    let overridden = expect_success(
        &mut failures,
        &mut summary,
        &dir,
        "override",
        &review_stdin(&before, Some(&overlay), Some(&override_review)),
        &inputs_for(&before, Some(&overlay), Some(&override_review)),
    );
    if let Some(doc) = &overridden {
        let draft = &doc["draft"];
        if let Some(email) = column(draft, "effective", "ledger.party", "email") {
            check(
                &mut failures,
                email["label"] == "Correspondence",
                format!("override effective label {:?}", email["label"]),
            );
            check(&mut failures, email["pii"].is_null(), "override applied pii");
            check(
                &mut failures,
                email["semantic_type"].is_null(),
                "override applied semantic type",
            );
        } else {
            failures.push(String::from("override missing effective email"));
        }
        if let Some(email) = column(draft, "curated", "ledger.party", "email") {
            check(&mut failures, email["pii"] == "sensitive", "override curated pii");
            check(
                &mut failures,
                email["label"] == "Correspondence",
                "override curated label",
            );
        } else {
            failures.push(String::from("override missing curated email"));
        }
        if let Some(item) = require_queue(
            &mut failures,
            draft,
            "override",
            "pii/ledger.party/email",
            "semantic_field_not_applied",
        ) {
            check(
                &mut failures,
                item["detail"] == "pii",
                format!("override semantic detail {:?}", item["detail"]),
            );
        }
        if let Some(item) = require_queue(&mut failures, draft, "override", "pii/ledger.party/email", "pii_unknown") {
            check(&mut failures, item["state"] == "pending", "override pii state");
            check(&mut failures, item["publishable"] == false, "override pii publishable");
        }
        check(
            &mut failures,
            doc["draft"]["reviews"]
                .as_array()
                .and_then(|items| items.iter().find(|item| item["action"] == "override"))
                .is_some(),
            "override review was not recorded",
        );
    }

    let prior = Value::Array(vec![decision(EMAIL_ITEM, "accept", BEFORE_SCHEMA, &email_fp)]);
    let stale = expect_success(
        &mut failures,
        &mut summary,
        &dir,
        "stale-schema",
        &review_stdin(&after, Some(&overlay), Some(&prior)),
        &inputs_for(&after, Some(&overlay), Some(&prior)),
    );
    if let Some(doc) = &stale {
        let draft = &doc["draft"];
        check(
            &mut failures,
            doc["wrapper"]["schema_hash"] == AFTER_SCHEMA,
            format!("stale schema {:?}", doc["wrapper"]["schema_hash"]),
        );
        check(
            &mut failures,
            doc["wrapper"]["captured_catalog_hash"] == AFTER_CAPTURED,
            "stale captured catalog hash",
        );
        check(
            &mut failures,
            doc["wrapper"]["draft_hash"] == AFTER_STALE_DRAFT,
            format!("stale draft hash {:?}", doc["wrapper"]["draft_hash"]),
        );
        if let Some(email) = column(draft, "effective", "ledger.party", "email") {
            check(&mut failures, email["label"] == "email", "stale applied curated label");
            check(&mut failures, email["pii"].is_null(), "stale applied curated pii");
        } else {
            failures.push(String::from("stale missing effective email"));
        }
        if let Some(email) = column(draft, "curated", "ledger.party", "email") {
            check(&mut failures, email["label"] == "Correspondence", "stale curated label");
            check(&mut failures, email["pii"] == "sensitive", "stale curated pii");
        } else {
            failures.push(String::from("stale missing curated email"));
        }
        if let Some(item) = require_queue(&mut failures, draft, "stale-schema", EMAIL_ITEM, "stale_decision") {
            check(
                &mut failures,
                item["detail"] == "stale_schema_hash",
                format!("stale detail {:?}", item["detail"]),
            );
            check(&mut failures, item["state"] == "pending", "stale state");
            check(&mut failures, item["publishable"] == false, "stale publishable");
        }
        if let Some(item) = require_queue(&mut failures, draft, "stale-schema", EMAIL_ITEM, "structural_column") {
            check(&mut failures, item["state"] == "pending", "stale structural state");
        }
        check(
            &mut failures,
            column_names(draft, "generated", "ledger.party")
                == vec!["tenant_id".to_string(), "id".to_string(), "email".to_string(), "note".to_string()],
            "stale generated columns",
        );
    }

    let wrong_fact = Value::Array(vec![decision(
        EMAIL_ITEM,
        "accept",
        BEFORE_SCHEMA,
        "0000000000000000000000000000000000000000000000000000000000000000",
    )]);
    let fact = expect_success(
        &mut failures,
        &mut summary,
        &dir,
        "stale-fact",
        &review_stdin(&before, None, Some(&wrong_fact)),
        &inputs_for(&before, None, Some(&wrong_fact)),
    );
    if let Some(doc) = &fact {
        let draft = &doc["draft"];
        if let Some(item) = require_queue(&mut failures, draft, "stale-fact", EMAIL_ITEM, "stale_decision") {
            check(
                &mut failures,
                item["detail"] == "stale_fact_fingerprint",
                format!("stale fact detail {:?}", item["detail"]),
            );
            check(&mut failures, item["state"] == "pending", "stale fact state");
            check(&mut failures, item["publishable"] == false, "stale fact publishable");
        }
        if let Some(item) = require_queue(&mut failures, draft, "stale-fact", EMAIL_ITEM, "structural_column") {
            check(&mut failures, item["state"] == "pending", "stale fact structural state");
        }
        check(
            &mut failures,
            doc["wrapper"]["schema_hash"] == BEFORE_SCHEMA,
            "stale fact changed schema hash",
        );
        check(
            &mut failures,
            doc["wrapper"]["draft_hash"] != BEFORE_DRAFT,
            "stale fact kept the bare draft hash",
        );
    }

    let duplicate = format!(
        "{{\"catalog\":{{\"note\":\"{MARKER}\"}},\"catalog\":{{\"note\":\"again\"}}}}"
    );
    expect_static(
        &mut failures,
        &dir,
        "duplicate",
        &run(&["review-pg-draft"], duplicate.as_bytes()),
        1,
        "oo-builder: review-pg-draft: oo.builder.catalog-evidence/v1/json.duplicate_key duplicate JSON object key\n",
    );
    let unknown_field = format!("{{\"catalog\":{{}},\"{MARKER}\":\"{MARKER}\"}}");
    expect_static(
        &mut failures,
        &dir,
        "unknown-field",
        &run(&["review-pg-draft"], unknown_field.as_bytes()),
        1,
        "oo-builder: review-pg-draft: oo.builder.catalog-evidence/v1/field.unknown review document has an unknown field\n",
    );
    expect_static(
        &mut failures,
        &dir,
        "catalog-array",
        &run(&["review-pg-draft"], br#"{"catalog":[]}"#),
        1,
        "oo-builder: review-pg-draft: oo.builder.catalog-evidence/v1/schema.invalid catalog is not an object\n",
    );
    expect_static(
        &mut failures,
        &dir,
        "catalog-null",
        &run(&["review-pg-draft"], br#"{"catalog":null}"#),
        1,
        "oo-builder: review-pg-draft: oo.builder.catalog-evidence/v1/schema.invalid catalog is not an object\n",
    );
    let curated_string = format!(r#"{{"catalog":{{}},"curated":"{MARKER}"}}"#);
    expect_static(
        &mut failures,
        &dir,
        "curated-string",
        &run(&["review-pg-draft"], curated_string.as_bytes()),
        1,
        "oo-builder: review-pg-draft: oo.builder.catalog-evidence/v1/schema.invalid review document is not catalog with optional curated and reviews\n",
    );
    expect_static(
        &mut failures,
        &dir,
        "reviews-object",
        &run(&["review-pg-draft"], br#"{"catalog":{},"reviews":{"action":"accept"}}"#),
        1,
        "oo-builder: review-pg-draft: oo.builder.catalog-evidence/v1/schema.invalid review document is not catalog with optional curated and reviews\n",
    );
    expect_static(
        &mut failures,
        &dir,
        "curated-null",
        &run(&["review-pg-draft"], br#"{"catalog":{},"curated":null}"#),
        1,
        "oo-builder: review-pg-draft: oo.builder.catalog-evidence/v1/schema.invalid review document is not catalog with optional curated and reviews\n",
    );
    expect_static(
        &mut failures,
        &dir,
        "reviews-null",
        &run(&["review-pg-draft"], br#"{"catalog":{},"reviews":null}"#),
        1,
        "oo-builder: review-pg-draft: oo.builder.catalog-evidence/v1/schema.invalid review document is not catalog with optional curated and reviews\n",
    );
    expect_static(
        &mut failures,
        &dir,
        "missing-catalog",
        &run(&["review-pg-draft"], b"{}"),
        1,
        "oo-builder: review-pg-draft: oo.builder.catalog-evidence/v1/field.missing review document is missing a required field\n",
    );
    expect_static(
        &mut failures,
        &dir,
        "utf8",
        &run(&["review-pg-draft"], &[0xff, 0xfe]),
        1,
        "oo-builder: review-pg-draft: oo.builder.catalog-evidence/v1/json.syntax stdin is not UTF-8\n",
    );
    let malformed = format!("{{\"catalog\":{MARKER}");
    expect_static(
        &mut failures,
        &dir,
        "malformed",
        &run(&["review-pg-draft"], malformed.as_bytes()),
        1,
        "oo-builder: review-pg-draft: oo.builder.catalog-evidence/v1/json.syntax review document JSON is not valid\n",
    );
    let trailing = format!("{{\"catalog\":{{}}}} {MARKER}");
    expect_static(
        &mut failures,
        &dir,
        "trailing",
        &run(&["review-pg-draft"], trailing.as_bytes()),
        1,
        "oo-builder: review-pg-draft: oo.builder.catalog-evidence/v1/json.syntax review document JSON is not valid\n",
    );
    let unknown_item = Value::Array(vec![decision(
        "column/ledger.party/not_a_column",
        "accept",
        BEFORE_SCHEMA,
        &email_fp,
    )]);
    expect_static(
        &mut failures,
        &dir,
        "unknown-item",
        &run(
            &["review-pg-draft"],
            review_stdin(&before, None, Some(&unknown_item)).as_bytes(),
        ),
        1,
        "oo-builder: review-pg-draft: oo.builder.catalog-evidence/v1/review.unknown_item review item is not in this catalog\n",
    );

    let exact = vec![b' '; DOCUMENT_MAX_BYTES];
    let exact_out = run(&["review-pg-draft"], &exact);
    save_output(&dir, "exact-max", &exact_out);
    check(
        &mut failures,
        exact_out.status.code() == Some(1),
        format!("exact-max exit {:?}", exact_out.status.code()),
    );
    check(
        &mut failures,
        exact_out.stdout.is_empty(),
        "exact-max stdout",
    );
    let exact_err = stderr_text(&exact_out);
    check(
        &mut failures,
        exact_err.contains("/json.syntax"),
        format!("exact-max stderr {exact_err:?}"),
    );
    check(
        &mut failures,
        !exact_err.contains("json.budget.bytes"),
        "exact-max used the byte budget",
    );

    let mut oversize = vec![b' '; DOCUMENT_MAX_BYTES + 1];
    oversize.extend(MARKER.as_bytes());
    let (oversize_out, oversize_ms) = run_stdin_held(&["review-pg-draft"], &oversize);
    fs::write(dir.join("oversize.elapsed_ms"), format!("{oversize_ms}\n")).expect("elapsed");
    save_output(&dir, "oversize", &oversize_out);
    let oversize_err = stderr_text(&oversize_out);
    check(
        &mut failures,
        oversize_out.status.code() == Some(1),
        format!("oversize exit {:?}", oversize_out.status.code()),
    );
    check(
        &mut failures,
        oversize_out.stdout.is_empty(),
        "oversize stdout",
    );
    check(
        &mut failures,
        oversize_err == "oo-builder: review-pg-draft: oo.builder.catalog-evidence/v1/json.budget.bytes document exceeds the byte budget\n",
        format!("oversize stderr {oversize_err:?}"),
    );
    check(
        &mut failures,
        !oversize_err.contains(MARKER),
        "oversize echoed marker",
    );
    check(
        &mut failures,
        oversize_ms < 5_000,
        format!("oversize elapsed {oversize_ms} ms"),
    );

    let golden_input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/ontology-draft/goldens/customer.input.json");
    let golden_draft = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/ontology-draft/goldens/customer.draft.json");
    let input_bytes = fs::read(&golden_input).expect("customer input");
    let expected_draft = fs::read(&golden_draft).expect("customer draft");
    let structure = run(&["structure-draft"], &input_bytes);
    save_output(&dir, "structure-draft", &structure);
    fs::write(
        dir.join("structure-draft.stdout.sha256"),
        format!("{}\n", sha256_hex(&structure.stdout)),
    )
    .expect("sha");
    fs::write(
        dir.join("structure-draft.golden.sha256"),
        format!("{}\n", sha256_hex(&expected_draft)),
    )
    .expect("sha");
    check(
        &mut failures,
        structure.status.code() == Some(0),
        format!("structure-draft exit {:?}", structure.status.code()),
    );
    check(
        &mut failures,
        structure.stdout == expected_draft,
        "structure-draft stdout changed",
    );
    check(
        &mut failures,
        !structure.stdout.ends_with(b"\n"),
        "structure-draft newline",
    );
    let structure_duplicate = run(
        &["structure-draft"],
        br#"{"protocol":"oo.builder.catalog-evidence/v1","protocol":"again"}"#,
    );
    save_output(&dir, "structure-draft-duplicate", &structure_duplicate);
    check(
        &mut failures,
        structure_duplicate.status.code() == Some(1),
        "structure-draft duplicate exit",
    );
    check(
        &mut failures,
        structure_duplicate.stdout.is_empty(),
        "structure-draft duplicate stdout",
    );
    check(
        &mut failures,
        stderr_text(&structure_duplicate).contains("/json.duplicate_key"),
        "structure-draft duplicate diagnostic",
    );

    summary.insert(
        "failure_count".to_string(),
        Value::from(failures.len() as u64),
    );
    summary.insert(
        "failures".to_string(),
        Value::Array(failures.iter().cloned().map(Value::String).collect()),
    );
    fs::write(
        dir.join("oracle-result.json"),
        serde_json::to_string_pretty(&Value::Object(summary)).expect("summary"),
    )
    .expect("summary file");
    assert!(failures.is_empty(), "{failures:#?}");
}
