//! Process tests for `parse-file`.
//!
//! The compiled command is the only parser under test. Expected profile and
//! config digests come from the public profile constructors. Expected row
//! offsets come from the fixture bytes. The production `oo-file-parser-worker`
//! is the executable named in the request. No Python and no PostgreSQL.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use oo_connectors::{
    BomPolicy, DelimitedColumn, DelimitedDialect, DelimitedProfile, DelimitedType, HeaderPolicy,
    JsonlColumn, JsonlProfile, JsonlType, NullPolicy, DELIMITED_PROTOCOL, FILE_WORKER_PARENT_PROTOCOL,
    JSONL_PROTOCOL,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const REQUEST_PROTOCOL: &str = "oo.connectors.parse-file/v1";
const RESULT_PROTOCOL: &str = "oo.connectors.parse-file-result/v1";
const ERROR_PROTOCOL: &str = "oo.connectors.parse-file-error/v1";
const PATH_TOKEN: &str = "PATHTOKEN_a245";
const CELL_TOKEN: &str = "CELLTOKEN_a245";
const QUOTE_TOKEN: &str = "QUOTETOKEN_a245";

const RESULT_KEYS: &[&str] = &[
    "admits_import",
    "arrow_bytes",
    "arrow_ipc",
    "batches",
    "columns",
    "config_identity",
    "kernel_restriction",
    "lineage",
    "lineage_sha256",
    "object_sha256",
    "parent_protocol",
    "pg_run_complete",
    "profile_digest",
    "profile_id",
    "protocol",
    "published",
    "raw_bytes",
    "reader_protocol",
    "rows",
    "schema_identity",
    "types",
];

const LINEAGE_KEYS: &[&str] = &[
    "empty",
    "fields",
    "first_byte_offset",
    "first_line",
    "last_byte_offset",
    "last_line",
    "missing",
    "null",
    "rows",
    "value",
];

const ERROR_KEYS: &[&str] = &[
    "exit_code",
    "fact",
    "group_signal",
    "outcome",
    "protocol",
    "reaped",
    "term_signal",
    "worker_token",
];

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let pid = std::process::id();
        let path = env::temp_dir().join(format!("{PATH_TOKEN}-{label}-{pid}-{nanos}"));
        fs::create_dir_all(&path).expect("scratch");
        Self(path)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).expect("write fixture");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn status_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(127)
}

fn cargo_bin(underscore: &str, hyphen: &str) -> String {
    env::var(underscore)
        .or_else(|_| env::var(hyphen))
        .unwrap_or_else(|_| panic!("missing {underscore}"))
}

fn parse_bin() -> PathBuf {
    PathBuf::from(cargo_bin("CARGO_BIN_EXE_parse_file", "CARGO_BIN_EXE_parse-file"))
}

fn worker() -> (PathBuf, String) {
    static ONCE: OnceLock<(PathBuf, String)> = OnceLock::new();
    ONCE.get_or_init(|| {
        let path = PathBuf::from(cargo_bin(
            "CARGO_BIN_EXE_oo_file_parser_worker",
            "CARGO_BIN_EXE_oo-file-parser-worker",
        ));
        assert!(path.is_absolute(), "worker path");
        (path.clone(), file_sha(&path))
    })
    .clone()
}

fn run_parse(args: &[&str], stdin_bytes: Option<&[u8]>) -> Run {
    let mut command = Command::new(parse_bin());
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin_bytes.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    let mut child = command.spawn().expect("spawn parse-file");
    if let Some(bytes) = stdin_bytes {
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(bytes)
            .expect("stdin write");
    }
    let output = child.wait_with_output().expect("wait parse-file");
    Run {
        code: status_code(output.status),
        stdout: String::from_utf8(output.stdout).expect("stdout utf8"),
        stderr: String::from_utf8(output.stderr).expect("stderr utf8"),
    }
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn file_sha(path: &Path) -> String {
    let mut file = File::open(path).expect("open hash");
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 8192];
    loop {
        let count = file.read(&mut buf).expect("read hash");
        if count == 0 {
            break;
        }
        hasher.update(&buf[..count]);
    }
    let digest: [u8; 32] = hasher.finalize().into();
    hex(&digest)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn flip_sha(hex_text: &str) -> String {
    let mut chars: Vec<char> = hex_text.chars().collect();
    chars[0] = if chars[0] == '0' { '1' } else { '0' };
    chars.into_iter().collect()
}

fn keys(value: &Value) -> Vec<&str> {
    value.as_object().expect("object").keys().map(String::as_str).collect()
}

fn one_json(text: &str) -> Value {
    assert_eq!(text.matches('\n').count(), 1, "one trailing newline");
    assert!(text.ends_with('\n'));
    serde_json::from_str(text.trim_end()).expect("json")
}

fn assert_known_sha256() {
    assert_eq!(
        hex(&sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

fn data_rows(bytes: &[u8], skip_header: bool) -> Vec<(u64, u64)> {
    let mut rows = Vec::new();
    let mut index = 0_usize;
    let mut line = 1_u64;
    let mut skipping = skip_header;
    while index < bytes.len() {
        let start = index;
        while index < bytes.len() && bytes[index] != b'\n' {
            index += 1;
        }
        let has_newline = index < bytes.len();
        if has_newline {
            index += 1;
        }
        if skipping {
            skipping = false;
        } else {
            rows.push((line, u64::try_from(start).expect("offset")));
        }
        line += 1;
        if !has_newline {
            break;
        }
    }
    rows
}

fn lineage_preimage(lineage: &Value, raw_bytes: u64, arrow_bytes: u64) -> String {
    format!(
        "\
oo.connectors.parse-file-lineage/v1
rows={}
fields={}
value={}
null={}
empty={}
missing={}
first_line={}
last_line={}
first_byte_offset={}
last_byte_offset={}
raw_bytes={raw_bytes}
arrow_bytes={arrow_bytes}
",
        lineage["rows"],
        lineage["fields"],
        lineage["value"],
        lineage["null"],
        lineage["empty"],
        lineage["missing"],
        lineage["first_line"],
        lineage["last_line"],
        lineage["first_byte_offset"],
        lineage["last_byte_offset"],
    )
}

fn budget(delimited: bool, batch_rows: u64, worker_ms: u64) -> Value {
    let mut value = json!({
        "batch_bytes": 1_048_576,
        "batch_rows": batch_rows,
        "deadline_ms": worker_ms,
        "max_columns": 8,
        "max_fields": 8,
        "max_record_bytes": 65_536,
        "max_rows": 100,
        "max_total_bytes": 1_048_576
    });
    if delimited {
        value
            .as_object_mut()
            .expect("budget")
            .insert("max_field_bytes".to_owned(), json!(4096));
    }
    value
}

fn document(
    input: &Path,
    input_sha: &str,
    parent_ms: u64,
    format: &str,
    profile: Value,
    budget: Value,
    executable_sha: &str,
) -> Vec<u8> {
    let (exe, real_sha) = worker();
    let sha = if executable_sha.is_empty() { real_sha } else { executable_sha.to_owned() };
    serde_json::to_vec(&json!({
        "budget": budget,
        "executable": {"path": exe, "sha256": sha},
        "format": format,
        "input": {"path": input, "sha256": input_sha},
        "parent_deadline_ms": parent_ms,
        "profile": profile,
        "protocol": REQUEST_PROTOCOL
    }))
    .expect("request json")
}

fn assert_silent(run: &Run, extra: &[&str]) {
    let (exe, _) = worker();
    let exe_text = exe.to_str().expect("utf8 exe");
    for needle in [PATH_TOKEN, exe_text].into_iter().chain(extra.iter().copied()) {
        assert!(!run.stdout.contains(needle), "stdout leaked a forbidden span");
        assert!(!run.stderr.contains(needle), "stderr leaked a forbidden span");
    }
}

fn assert_error(run: &Run, code: i32, outcome: &str, fact: &str) -> Value {
    assert_eq!(run.code, code, "stdout={stdout} stderr={stderr}", stdout = run.stdout, stderr = run.stderr);
    assert!(run.stdout.is_empty(), "error stdout {stdout}", stdout = run.stdout);
    let doc = one_json(&run.stderr);
    assert_eq!(keys(&doc), ERROR_KEYS);
    assert_eq!(doc["protocol"], ERROR_PROTOCOL);
    assert_eq!(doc["outcome"], outcome);
    assert_eq!(doc["fact"], fact);
    doc
}

fn assert_summary(run: &Run, rows: u64, batches: u64, columns: u64) -> Value {
    assert_eq!(run.code, 0, "stdout={stdout} stderr={stderr}", stdout = run.stdout, stderr = run.stderr);
    assert!(run.stderr.is_empty(), "success stderr {stderr}", stderr = run.stderr);
    let doc = one_json(&run.stdout);
    assert_eq!(keys(&doc), RESULT_KEYS);
    assert_eq!(keys(&doc["lineage"]), LINEAGE_KEYS);
    assert_eq!(doc["protocol"], RESULT_PROTOCOL);
    assert_eq!(doc["parent_protocol"], FILE_WORKER_PARENT_PROTOCOL);
    assert_eq!(doc["kernel_restriction"], "unsupported");
    assert_eq!(doc["admits_import"], false);
    assert_eq!(doc["published"], false);
    assert_eq!(doc["pg_run_complete"], false);
    assert_eq!(doc["rows"], rows);
    assert_eq!(doc["batches"], batches);
    assert_eq!(doc["columns"], columns);
    assert_eq!(doc["arrow_ipc"], batches > 0);
    assert_eq!(doc["lineage"]["rows"], rows);
    let raw = doc["raw_bytes"].as_u64().expect("raw");
    let arrow = doc["arrow_bytes"].as_u64().expect("arrow");
    if rows == 0 {
        assert_eq!(raw, 0);
        assert_eq!(arrow, 0);
        for key in LINEAGE_KEYS {
            assert_eq!(doc["lineage"][key], 0, "{key}");
        }
    } else {
        assert!(raw > 0, "raw bytes");
        assert!(arrow > 0, "arrow bytes");
    }
    let preimage = lineage_preimage(&doc["lineage"], raw, arrow);
    assert_eq!(doc["lineage_sha256"], hex(&sha256(preimage.as_bytes())));
    let schema = doc["schema_identity"].as_str().expect("schema");
    assert_eq!(schema.len(), 64);
    assert!(schema.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')));
    doc
}

#[test]
fn help_lists_the_closed_contract() {
    let run = run_parse(&["--help"], None);
    assert_eq!(run.code, 0, "{stderr}", stderr = run.stderr);
    assert!(run.stderr.is_empty());
    for phrase in [
        REQUEST_PROTOCOL,
        "1048576",
        "CSV",
        "TSV",
        "JSONL",
        "does not infer",
        "does not import",
        "does not publish",
        "does not open a database",
        "ParentParseComplete",
        "no raw rows",
        "unsupported",
        "parent_deadline_ms",
        "clean EOF",
        "Exit 0",
        "Exit 2",
        "Exit 3",
        "Exit 4",
        "Exit 5",
        "Exit 6",
        "Exit 7",
    ] {
        assert!(run.stdout.contains(phrase), "help missing {phrase}");
    }
}

#[test]
fn unknown_arguments_are_usage() {
    let extra = run_parse(&["--help", "--request"], None);
    assert_error(&extra, 2, "usage", "unknown_flag");
    let request = run_parse(&["--request"], Some(b"{}"));
    assert_error(&request, 2, "usage", "unknown_flag");
    let short = run_parse(&["-h"], None);
    assert_error(&short, 2, "usage", "unknown_flag");
}

#[test]
fn request_shape_stops_before_a_child() {
    assert_known_sha256();
    let empty = run_parse(&[], Some(b""));
    assert_error(&empty, 3, "config", "request_json");
    assert!(empty.stderr.contains("\"reaped\":false"));

    let duplicate = run_parse(
        &[],
        Some(br#"{"protocol":"oo.connectors.parse-file/v1","protocol":"other"}"#),
    );
    assert_error(&duplicate, 3, "config", "request_json");

    let mut huge = vec![b' '; 1_048_577];
    huge[0] = b'{';
    let over = run_parse(&[], Some(&huge));
    assert_error(&over, 3, "config", "request_bytes");

    let scratch = Scratch::new("shape");
    let input = scratch.write("in.jsonl", b"{}\n");
    let (exe, sha) = worker();
    let secret = "SECRETVALUE_a245";
    let credential = serde_json::to_vec(&json!({
        "budget": budget(false, 1, 1000),
        "executable": {"path": exe, "sha256": sha},
        "format": "jsonl",
        "input": {"path": input, "sha256": file_sha(&input)},
        "parent_deadline_ms": 1000,
        "password": secret,
        "profile": {"columns": [], "id": "x"},
        "protocol": REQUEST_PROTOCOL
    }))
    .unwrap();
    let leaked = run_parse(&[], Some(&credential));
    let doc = assert_error(&leaked, 3, "config", "credentials");
    assert!(doc["worker_token"].is_null());
    assert!(!leaked.stderr.contains(secret));
    assert!(!leaked.stdout.contains(secret));

    let upper = document(
        &input,
        &file_sha(&input),
        1000,
        "jsonl",
        json!({"columns": [{"name": "n", "nullable": false, "type": "int64"}], "id": "up"}),
        budget(false, 1, 1000),
        &"A".repeat(64),
    );
    assert_error(&run_parse(&[], Some(&upper)), 3, "config", "executable_sha256");

    let mut relative = document(
        &input,
        &file_sha(&input),
        1000,
        "jsonl",
        json!({"columns": [{"name": "n", "nullable": false, "type": "int64"}], "id": "rel"}),
        budget(false, 1, 1000),
        "",
    );
    let mut value: Value = serde_json::from_slice(&relative).unwrap();
    value["executable"]["path"] = json!("oo-file-parser-worker");
    relative = serde_json::to_vec(&value).unwrap();
    let refused = run_parse(&[], Some(&relative));
    let doc = assert_error(&refused, 3, "config", "worker_path");
    assert_eq!(doc["reaped"], false);
    assert!(doc["worker_token"].is_null());

    let zero_worker = document(
        &input,
        &file_sha(&input),
        1000,
        "jsonl",
        json!({"columns": [{"name": "n", "nullable": false, "type": "int64"}], "id": "clock"}),
        budget(false, 1, 0),
        "",
    );
    assert_error(&run_parse(&[], Some(&zero_worker)), 3, "config", "worker_deadline");

    let crossed = document(
        &input,
        &file_sha(&input),
        1000,
        "jsonl",
        json!({"columns": [{"name": "n", "nullable": false, "type": "int64"}], "id": "cross"}),
        budget(true, 1, 1000),
        "",
    );
    assert_error(&run_parse(&[], Some(&crossed)), 3, "config", "unknown_field");

    let dialect = document(
        &input,
        &file_sha(&input),
        1000,
        "csv",
        json!({
            "bom": "refuse",
            "columns": [{"name": "n", "nullable": false, "type": "utf8"}],
            "dialect": "tsv",
            "header": "absent",
            "id": "bad-dialect",
            "null": {"policy": "empty"}
        }),
        budget(true, 1, 1000),
        "",
    );
    assert_error(&run_parse(&[], Some(&dialect)), 3, "config", "dialect");
}

fn csv_bytes() -> Vec<u8> {
    "name,flag,n,price\n\
熊猫,true,-9223372036854775808,1.20\n\
~,false,0,~\n\
\"\",true,9223372036854775807,10\n"
        .as_bytes()
        .to_vec()
}

fn csv_profile_json() -> Value {
    json!({
        "bom": "refuse",
        "columns": [
            {"name": "name", "nullable": true, "type": "utf8"},
            {"name": "flag", "nullable": false, "type": "bool"},
            {"name": "n", "nullable": false, "type": "int64"},
            {"name": "price", "nullable": true, "precision": 10, "scale": 2, "type": "decimal"}
        ],
        "dialect": "csv",
        "header": "exact",
        "id": "a245lot",
        "null": {"policy": "token", "text": "~"}
    })
}

fn csv_profile() -> DelimitedProfile {
    DelimitedProfile::try_new(
        "a245lot",
        DelimitedDialect::try_from_separator(b',').expect("comma"),
        BomPolicy::Refuse,
        HeaderPolicy::Exact,
        NullPolicy::Token("~".to_owned()),
        vec![
            DelimitedColumn::try_new("name", DelimitedType::Utf8, true).expect("name"),
            DelimitedColumn::try_new("flag", DelimitedType::Bool, false).expect("flag"),
            DelimitedColumn::try_new("n", DelimitedType::Int64, false).expect("n"),
            DelimitedColumn::try_new(
                "price",
                DelimitedType::Decimal { precision: 10, scale: 2 },
                true,
            )
            .expect("price"),
        ],
    )
    .expect("csv profile")
}

fn assert_delimited_identity(doc: &Value, profile: &DelimitedProfile, object_sha: &str) {
    let raw = decode_sha(object_sha);
    assert_eq!(doc["profile_id"], profile.id());
    assert_eq!(doc["profile_digest"], hex(&profile.profile_digest()));
    assert_eq!(doc["config_identity"], hex(&profile.config_identity(raw)));
    assert_eq!(doc["object_sha256"], object_sha);
    assert_eq!(doc["reader_protocol"], DELIMITED_PROTOCOL);
}

fn assert_jsonl_identity(doc: &Value, profile: &JsonlProfile, object_sha: &str) {
    let raw = decode_sha(object_sha);
    assert_eq!(doc["profile_id"], profile.id());
    assert_eq!(doc["profile_digest"], hex(&profile.profile_digest()));
    assert_eq!(doc["config_identity"], hex(&profile.config_identity(raw)));
    assert_eq!(doc["object_sha256"], object_sha);
    assert_eq!(doc["reader_protocol"], JSONL_PROTOCOL);
}

fn decode_sha(text: &str) -> [u8; 32] {
    let mut out = [0_u8; 32];
    for (index, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).expect("hex");
    }
    out
}

fn assert_offsets(doc: &Value, bytes: &[u8], skip_header: bool) {
    let rows = data_rows(bytes, skip_header);
    let lineage = &doc["lineage"];
    if rows.is_empty() {
        assert_eq!(lineage["first_line"], 0);
        assert_eq!(lineage["last_line"], 0);
        return;
    }
    assert_eq!(lineage["first_line"], rows[0].0);
    assert_eq!(lineage["first_byte_offset"], rows[0].1);
    assert_eq!(lineage["last_line"], rows.last().expect("last").0);
    assert_eq!(lineage["last_byte_offset"], rows.last().expect("last").1);
}

#[test]
fn csv_summary_matches_the_profile_and_hides_cells() {
    let scratch = Scratch::new("csv");
    let bytes = csv_bytes();
    let input = scratch.write("lot.csv", &bytes);
    let sha = file_sha(&input);
    let request = document(&input, &sha, 20_000, "csv", csv_profile_json(), budget(true, 2, 60_000), "");
    let first = run_parse(&[], Some(&request));
    let doc = assert_summary(&first, 3, 2, 4);
    assert_delimited_identity(&doc, &csv_profile(), &sha);
    assert_offsets(&doc, &bytes, true);
    assert_eq!(doc["lineage"]["fields"], 12);
    assert_eq!(doc["lineage"]["value"], 9);
    assert_eq!(doc["lineage"]["null"], 2);
    assert_eq!(doc["lineage"]["empty"], 1);
    assert_eq!(doc["lineage"]["missing"], 0);
    assert_eq!(doc["types"][3]["type"], "decimal");
    assert_eq!(doc["types"][3]["precision"], 10);
    assert_eq!(doc["types"][3]["scale"], 2);
    assert_silent(&first, &["熊猫", "1.20", "9223372036854775807", "-9223372036854775808"]);
    let second = run_parse(&[], Some(&request));
    let again = assert_summary(&second, 3, 2, 4);
    assert_eq!(again["schema_identity"], doc["schema_identity"]);
    assert_eq!(again["lineage_sha256"], doc["lineage_sha256"]);
    assert_eq!(again["raw_bytes"], doc["raw_bytes"]);
    assert_eq!(again["arrow_bytes"], doc["arrow_bytes"]);
}

#[test]
fn tsv_summary_counts_two_batches() {
    let scratch = Scratch::new("tsv");
    let bytes = "name\tn\n鹿\t-7\n\"b,c\"\t0\n".as_bytes().to_vec();
    let input = scratch.write("tab.tsv", &bytes);
    let sha = file_sha(&input);
    let profile_json = json!({
        "bom": "refuse",
        "columns": [
            {"name": "name", "nullable": true, "type": "utf8"},
            {"name": "n", "nullable": false, "type": "int64"}
        ],
        "dialect": "tsv",
        "header": "exact",
        "id": "a245tab",
        "null": {"policy": "empty"}
    });
    let profile = DelimitedProfile::try_new(
        "a245tab",
        DelimitedDialect::try_from_separator(b'\t').expect("tab"),
        BomPolicy::Refuse,
        HeaderPolicy::Exact,
        NullPolicy::Empty,
        vec![
            DelimitedColumn::try_new("name", DelimitedType::Utf8, true).expect("name"),
            DelimitedColumn::try_new("n", DelimitedType::Int64, false).expect("n"),
        ],
    )
    .expect("tsv profile");
    let request = document(&input, &sha, 20_000, "tsv", profile_json, budget(true, 1, 60_000), "");
    let run = run_parse(&[], Some(&request));
    let doc = assert_summary(&run, 2, 2, 2);
    assert_delimited_identity(&doc, &profile, &sha);
    assert_offsets(&doc, &bytes, true);
    assert_eq!(doc["lineage"]["fields"], 4);
    assert_eq!(doc["lineage"]["value"], 4);
    assert_eq!(doc["lineage"]["null"], 0);
    assert_eq!(doc["lineage"]["empty"], 0);
    assert_eq!(doc["lineage"]["missing"], 0);
    assert_silent(&run, &["鹿", "b,c"]);
}

#[test]
fn jsonl_summary_counts_null_and_missing() {
    let scratch = Scratch::new("jsonl");
    let bytes = "{\"name\":\"丙\",\"ok\":true,\"n\":-9223372036854775808}\n\
{\"name\":null,\"ok\":false,\"n\":1}\n\
{\"ok\":true,\"n\":4}\n"
        .as_bytes()
        .to_vec();
    let input = scratch.write("notes.jsonl", &bytes);
    let sha = file_sha(&input);
    let profile_json = json!({
        "columns": [
            {"name": "name", "nullable": true, "type": "utf8"},
            {"name": "ok", "nullable": false, "type": "bool"},
            {"name": "n", "nullable": false, "type": "int64"}
        ],
        "id": "a245notes"
    });
    let profile = JsonlProfile::try_new(
        "a245notes",
        vec![
            JsonlColumn::try_new("name", JsonlType::Utf8, true).expect("name"),
            JsonlColumn::try_new("ok", JsonlType::Bool, false).expect("ok"),
            JsonlColumn::try_new("n", JsonlType::Int64, false).expect("n"),
        ],
    )
    .expect("jsonl profile");
    let request = document(&input, &sha, 20_000, "jsonl", profile_json, budget(false, 1, 60_000), "");
    let run = run_parse(&[], Some(&request));
    let doc = assert_summary(&run, 3, 3, 3);
    assert_jsonl_identity(&doc, &profile, &sha);
    assert_offsets(&doc, &bytes, false);
    assert_eq!(doc["lineage"]["fields"], 9);
    assert_eq!(doc["lineage"]["value"], 7);
    assert_eq!(doc["lineage"]["null"], 1);
    assert_eq!(doc["lineage"]["missing"], 1);
    assert_eq!(doc["lineage"]["empty"], 0);
    assert_silent(&run, &["丙", "-9223372036854775808"]);
}

#[test]
fn empty_jsonl_completes_without_rows() {
    let scratch = Scratch::new("empty");
    let input = scratch.write("empty.jsonl", b"");
    let sha = file_sha(&input);
    let profile_json = json!({
        "columns": [
            {"name": "name", "nullable": true, "type": "utf8"},
            {"name": "ok", "nullable": true, "type": "bool"},
            {"name": "n", "nullable": true, "type": "int64"}
        ],
        "id": "a245empty"
    });
    let profile = JsonlProfile::try_new(
        "a245empty",
        vec![
            JsonlColumn::try_new("name", JsonlType::Utf8, true).expect("name"),
            JsonlColumn::try_new("ok", JsonlType::Bool, true).expect("ok"),
            JsonlColumn::try_new("n", JsonlType::Int64, true).expect("n"),
        ],
    )
    .expect("empty profile");
    let request = document(&input, &sha, 20_000, "jsonl", profile_json, budget(false, 8, 60_000), "");
    let run = run_parse(&[], Some(&request));
    let doc = assert_summary(&run, 0, 0, 3);
    assert_jsonl_identity(&doc, &profile, &sha);
    assert_eq!(doc["types"].as_array().expect("types").len(), 3);
    assert_silent(&run, &[]);
}

#[test]
fn malformed_quote_has_no_completion() {
    let scratch = Scratch::new("quote");
    let bytes = format!("id\n\"{QUOTE_TOKEN}\n");
    let input = scratch.write("bad.csv", bytes.as_bytes());
    let sha = file_sha(&input);
    let profile = json!({
        "bom": "refuse",
        "columns": [{"name": "id", "nullable": false, "type": "utf8"}],
        "dialect": "csv",
        "header": "exact",
        "id": "a245bad",
        "null": {"policy": "empty"}
    });
    let request = document(&input, &sha, 20_000, "csv", profile, budget(true, 4, 60_000), "");
    let run = run_parse(&[], Some(&request));
    let doc = assert_error(&run, 5, "parent", "ended");
    assert_eq!(doc["worker_token"], "protocol");
    assert_eq!(doc["exit_code"], 4);
    assert_eq!(doc["reaped"], true);
    assert_silent(&run, &[QUOTE_TOKEN]);
}

#[test]
fn hash_mismatch_is_the_parent_receipt_refusal() {
    let scratch = Scratch::new("hash");
    let bytes = format!("{{\"name\":\"{CELL_TOKEN}\",\"ok\":true,\"n\":1}}\n");
    let input = scratch.write("row.jsonl", bytes.as_bytes());
    let sha = flip_sha(&file_sha(&input));
    let profile = json!({
        "columns": [
            {"name": "name", "nullable": false, "type": "utf8"},
            {"name": "ok", "nullable": false, "type": "bool"},
            {"name": "n", "nullable": false, "type": "int64"}
        ],
        "id": "a245hash"
    });
    let request = document(&input, &sha, 20_000, "jsonl", profile, budget(false, 4, 60_000), "");
    let run = run_parse(&[], Some(&request));
    let doc = assert_error(&run, 6, "hash", "hash_mismatch");
    assert_eq!(doc["worker_token"], "hash_mismatch");
    assert_eq!(doc["exit_code"], 6);
    assert_eq!(doc["reaped"], true);
    assert_silent(&run, &[CELL_TOKEN]);
}

#[test]
fn exact_header_on_an_empty_file_has_no_completion() {
    let scratch = Scratch::new("header");
    let input = scratch.write("empty.csv", b"");
    let sha = file_sha(&input);
    let profile = json!({
        "bom": "refuse",
        "columns": [{"name": "id", "nullable": true, "type": "utf8"}],
        "dialect": "csv",
        "header": "exact",
        "id": "a245header",
        "null": {"policy": "empty"}
    });
    let request = document(&input, &sha, 20_000, "csv", profile, budget(true, 4, 60_000), "");
    let run = run_parse(&[], Some(&request));
    let doc = assert_error(&run, 5, "parent", "ended");
    assert_eq!(doc["worker_token"], "header_required");
    assert_eq!(doc["exit_code"], 4);
    assert_eq!(doc["reaped"], true);
    assert!(run.stdout.is_empty());
}

#[test]
fn executable_sha_mismatch_does_not_start_the_parent() {
    let scratch = Scratch::new("exe");
    let input = scratch.write("empty.jsonl", b"");
    let sha = file_sha(&input);
    let profile = json!({
        "columns": [{"name": "n", "nullable": true, "type": "int64"}],
        "id": "a245exe"
    });
    let (_, real) = worker();
    let request = document(
        &input,
        &sha,
        20_000,
        "jsonl",
        profile,
        budget(false, 4, 60_000),
        &flip_sha(&real),
    );
    let run = run_parse(&[], Some(&request));
    let doc = assert_error(&run, 6, "hash", "executable_sha256");
    assert!(doc["worker_token"].is_null());
    assert!(doc["exit_code"].is_null());
    assert!(doc["term_signal"].is_null());
    assert_eq!(doc["reaped"], false);
    assert_eq!(doc["group_signal"], "not_needed");
    assert_silent(&run, &[]);
}

fn busy_jsonl(rows: usize) -> Vec<u8> {
    let line = b"{\"n\":1}\n";
    let mut out = Vec::with_capacity(line.len().saturating_mul(rows));
    for _ in 0..rows {
        out.extend_from_slice(line);
    }
    out
}

fn busy_budget(batch_rows: u64, max_rows: u64, max_total_bytes: u64) -> Value {
    json!({
        "batch_bytes": 1_048_576,
        "batch_rows": batch_rows,
        "deadline_ms": 3_600_000,
        "max_columns": 4,
        "max_fields": 4,
        "max_record_bytes": 1_024,
        "max_rows": max_rows,
        "max_total_bytes": max_total_bytes
    })
}

fn timed_parse(stdin_bytes: &[u8]) -> (Duration, Run) {
    let started = std::time::Instant::now();
    let run = run_parse(&[], Some(stdin_bytes));
    (started.elapsed(), run)
}

#[test]
fn parent_deadline_kills_the_worker_and_the_next_call_completes() {
    const ROWS: u64 = 40_000;
    const BATCH_ROWS: u64 = 16;
    let scratch = Scratch::new("deadline");
    let empty = scratch.write("empty.jsonl", b"");
    let empty_sha = file_sha(&empty);
    let empty_profile = json!({
        "columns": [{"name": "n", "nullable": true, "type": "int64"}],
        "id": "a245after"
    });
    let empty_public = JsonlProfile::try_new(
        "a245after",
        vec![JsonlColumn::try_new("n", JsonlType::Int64, true).expect("n")],
    )
    .expect("after profile");
    let empty_request = document(
        &empty,
        &empty_sha,
        20_000,
        "jsonl",
        empty_profile.clone(),
        budget(false, 4, 60_000),
        "",
    );
    let (empty_elapsed, empty_run) = timed_parse(&empty_request);
    let empty_summary = assert_summary(&empty_run, 0, 0, 1);
    assert_jsonl_identity(&empty_summary, &empty_public, &empty_sha);

    let bytes = busy_jsonl(usize::try_from(ROWS).expect("rows"));
    let input = scratch.write("busy.jsonl", &bytes);
    let sha = file_sha(&input);
    let profile = json!({
        "columns": [{"name": "n", "nullable": false, "type": "int64"}],
        "id": "a245deadline"
    });
    let public = JsonlProfile::try_new(
        "a245deadline",
        vec![JsonlColumn::try_new("n", JsonlType::Int64, false).expect("n")],
    )
    .expect("deadline profile");
    let wide = busy_budget(BATCH_ROWS, ROWS, u64::try_from(bytes.len()).expect("len"));
    let full = document(&input, &sha, 120_000, "jsonl", profile.clone(), wide.clone(), "");
    let (busy_elapsed, parsed) = timed_parse(&full);
    let batches = ROWS.div_ceil(BATCH_ROWS);
    let summary = assert_summary(&parsed, ROWS, batches, 1);
    assert_jsonl_identity(&summary, &public, &sha);
    assert_eq!(summary["lineage"]["fields"], ROWS);
    assert_eq!(summary["lineage"]["value"], ROWS);
    assert_eq!(summary["lineage"]["first_line"], 1);
    assert_eq!(summary["lineage"]["first_byte_offset"], 0);
    assert_eq!(summary["lineage"]["last_line"], ROWS);
    assert_eq!(summary["lineage"]["last_byte_offset"], (ROWS - 1) * 8);
    assert!(
        busy_elapsed > empty_elapsed.saturating_mul(2),
        "empty {} ms busy {} ms",
        empty_elapsed.as_millis(),
        busy_elapsed.as_millis()
    );

    let gap = busy_elapsed.saturating_sub(empty_elapsed);
    let kill_ms = u64::try_from(gap.as_millis() / 2).expect("half gap");
    assert!((1..=120_000).contains(&kill_ms), "kill deadline {kill_ms}");
    assert!(
        u128::from(kill_ms) > empty_elapsed.as_millis(),
        "kill deadline {kill_ms} ms is inside the {} ms empty run",
        empty_elapsed.as_millis()
    );
    let killed = document(&input, &sha, kill_ms, "jsonl", profile, wide, "");
    let (kill_elapsed, run) = timed_parse(&killed);
    assert!(
        kill_elapsed < busy_elapsed,
        "kill took {} ms, full parse took {} ms, empty took {} ms",
        kill_elapsed.as_millis(),
        busy_elapsed.as_millis(),
        empty_elapsed.as_millis()
    );
    let doc = assert_error(&run, 7, "deadline", "timed_out");
    assert_eq!(doc["reaped"], true, "doc={doc}");
    assert_eq!(doc["group_signal"], "sent", "doc={doc}");
    assert!(doc["worker_token"].is_null(), "doc={doc}");
    let signal = doc["term_signal"].as_i64();
    let child_exit = doc["exit_code"].as_i64();
    let killed_by_signal = signal == Some(9) && child_exit.is_none();
    let exited_on_closed_pipe = signal.is_none() && child_exit == Some(7);
    assert!(
        killed_by_signal || exited_on_closed_pipe,
        "empty_ms={} busy_ms={} kill_ms={kill_ms} kill_elapsed_ms={} doc={doc}",
        empty_elapsed.as_millis(),
        busy_elapsed.as_millis(),
        kill_elapsed.as_millis(),
    );
    assert_silent(&run, &[]);

    let again = document(
        &empty,
        &empty_sha,
        20_000,
        "jsonl",
        empty_profile,
        budget(false, 4, 60_000),
        "",
    );
    let next = run_parse(&[], Some(&again));
    let recovered = assert_summary(&next, 0, 0, 1);
    assert_jsonl_identity(&recovered, &empty_public, &empty_sha);
    assert_silent(&next, &[]);
}
