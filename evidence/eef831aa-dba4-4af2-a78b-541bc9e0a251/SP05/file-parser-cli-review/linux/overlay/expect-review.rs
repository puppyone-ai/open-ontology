//! Independent review of `parse-file` at the sealed archive.
//!
//! The compiled command is one process. A second call uses `ParentConfig` on
//! the same production `oo-file-parser-worker`. Arrow cells are not read.
//! `OO_A251_ORACLE_DIR` receives the fixtures. No Python and no PostgreSQL.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use oo_connectors::{
    BomPolicy, ChildRequest, DelimitedColumn, DelimitedDialect, DelimitedProfile, DelimitedType,
    Failure, FailureKind, GroupSignal, HeaderPolicy, JsonlColumn, JsonlProfile, JsonlType,
    KernelRestriction, NullPolicy, ParentConfig, ParentError, ParentParseComplete, PullBudget,
    SourceProfile, VerifiedBatch, DELIMITED_PROTOCOL, FILE_WORKER_PARENT_PROTOCOL, JSONL_PROTOCOL,
    KERNEL_RESTRICTION,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const REQUEST_PROTOCOL: &str = "oo.connectors.parse-file/v1";
const RESULT_PROTOCOL: &str = "oo.connectors.parse-file-result/v1";
const ERROR_PROTOCOL: &str = "oo.connectors.parse-file-error/v1";
const PATH_TOKEN: &str = "PATHTOKEN_a251";
const CELL_TOKEN: &str = "CELL_a251";
const SECRET_TOKEN: &str = "SECRET_a251";
const CRED_TOKEN: &str = "SECRET_a251_CRED";
const QUOTE_TOKEN: &str = "QUOTE_a251";

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
        let path = env::temp_dir().join(format!("{PATH_TOKEN}-{label}-{nanos}"));
        fs::create_dir_all(&path).expect("scratch");
        Self(path)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).expect("fixture");
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

struct Fold {
    rows: u64,
    fields: u64,
    value: u64,
    null: u64,
    empty: u64,
    missing: u64,
    first_line: u64,
    last_line: u64,
    first_offset: u64,
    last_offset: u64,
    raw_bytes: u64,
    arrow_bytes: u64,
    batches: u64,
    schema: Option<[u8; 32]>,
    profile_digest: Option<[u8; 32]>,
    config_identity: Option<[u8; 32]>,
}

impl Fold {
    const fn new() -> Self {
        Self {
            rows: 0,
            fields: 0,
            value: 0,
            null: 0,
            empty: 0,
            missing: 0,
            first_line: 0,
            last_line: 0,
            first_offset: 0,
            last_offset: 0,
            raw_bytes: 0,
            arrow_bytes: 0,
            batches: 0,
            schema: None,
            profile_digest: None,
            config_identity: None,
        }
    }
}

struct Hand {
    rows: u64,
    batches: u64,
    columns: u64,
    value: u64,
    null: u64,
    empty: u64,
    missing: u64,
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
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("oo-file-parser-worker")
        );
        (path.clone(), file_sha(&path))
    })
    .clone()
}

fn run_parse(args: &[&str], stdin_bytes: Option<&[u8]>) -> Run {
    let mut command = Command::new(parse_bin());
    command
        .args(args)
        .current_dir(env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin_bytes.is_none() {
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
    let digest: [u8; 32] = hasher.finalize().into();
    digest
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

fn decode_sha(text: &str) -> [u8; 32] {
    let mut out = [0_u8; 32];
    for (index, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).expect("hex");
    }
    out
}

fn flip_sha(hex_text: &str) -> String {
    let mut chars: Vec<char> = hex_text.chars().collect();
    chars[0] = if chars[0] == '0' { '1' } else { '0' };
    chars.into_iter().collect()
}

fn keys(value: &Value) -> Vec<&str> {
    value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect()
}

fn one_json(text: &str) -> Value {
    assert_eq!(text.matches('\n').count(), 1, "one trailing newline");
    assert!(text.ends_with('\n'));
    serde_json::from_str(text.trim_end()).expect("json")
}

fn save(name: &str, bytes: &[u8]) {
    let dir = env::var("OO_A251_ORACLE_DIR").expect("OO_A251_ORACLE_DIR");
    let path = Path::new(&dir).join(name);
    fs::write(path, bytes).expect("oracle write");
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

fn lineage_preimage(fold: &Fold) -> String {
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
raw_bytes={}
arrow_bytes={}
",
        fold.rows,
        fold.fields,
        fold.value,
        fold.null,
        fold.empty,
        fold.missing,
        fold.first_line,
        fold.last_line,
        fold.first_offset,
        fold.last_offset,
        fold.raw_bytes,
        fold.arrow_bytes,
    )
}

fn bump(slot: &mut u64) {
    *slot = slot.checked_add(1).expect("count");
}

fn absorb(fold: &mut Fold, batch: &VerifiedBatch) {
    assert_eq!(batch.batch_index, fold.batches, "batch index");
    assert_eq!(batch.record.num_rows(), batch.rows.len(), "record rows");
    if !batch.rows.is_empty() {
        assert_ne!(batch.raw_bytes, 0, "raw bytes");
        assert_ne!(batch.arrow_bytes, 0, "arrow bytes");
    }
    match fold.schema {
        None => {
            fold.schema = Some(batch.schema_identity);
            fold.profile_digest = Some(batch.profile_digest);
            fold.config_identity = Some(batch.config_identity);
        }
        Some(schema) => {
            assert_eq!(schema, batch.schema_identity, "schema");
            assert_eq!(fold.profile_digest, Some(batch.profile_digest), "profile");
            assert_eq!(fold.config_identity, Some(batch.config_identity), "config");
        }
    }
    fold.raw_bytes = fold.raw_bytes.checked_add(batch.raw_bytes).expect("raw");
    fold.arrow_bytes = fold.arrow_bytes.checked_add(batch.arrow_bytes).expect("arrow");
    for row in &batch.rows {
        if fold.rows == 0 {
            fold.first_line = row.line;
            fold.first_offset = row.byte_offset;
        }
        bump(&mut fold.rows);
        fold.last_line = row.line;
        fold.last_offset = row.byte_offset;
        for field in &row.fields {
            bump(&mut fold.fields);
            match field.presence {
                "value" => bump(&mut fold.value),
                "null" => bump(&mut fold.null),
                "empty" => bump(&mut fold.empty),
                "missing" => bump(&mut fold.missing),
                other => panic!("presence {other}"),
            }
        }
    }
    fold.batches += 1;
}

fn parent_success(
    path: &Path,
    object_sha: [u8; 32],
    deadline_ms: u64,
    profile: SourceProfile,
    budget: PullBudget,
) -> (Fold, ParentParseComplete) {
    let (worker_path, _) = worker();
    let config = ParentConfig::try_new(&worker_path, deadline_ms).expect("parent config");
    let child = ChildRequest::try_new(path, object_sha, profile, budget).expect("child request");
    let mut pull = config.start(&child).expect("parent start");
    let mut fold = Fold::new();
    loop {
        match pull.next_batch() {
            Ok(Some(batch)) => absorb(&mut fold, &batch),
            Ok(None) => break,
            Err(err) => panic!("next_batch failed: {err}"),
        }
    }
    let done = pull.finish().expect("parent finish");
    if fold.batches > 0 {
        assert_eq!(fold.schema, Some(done.schema_identity()));
        assert_eq!(fold.profile_digest, Some(done.profile_digest()));
        assert_eq!(fold.config_identity, Some(done.config_identity()));
    }
    assert_eq!(done.rows(), fold.rows);
    assert_eq!(done.batches(), fold.batches);
    assert!(!done.admits_import());
    assert!(!done.published());
    assert!(!done.pg_run_complete());
    assert_eq!(done.arrow_ipc(), fold.batches > 0);
    assert_eq!(KERNEL_RESTRICTION, KernelRestriction::Unsupported);
    (fold, done)
}

fn parent_failure(
    path: &Path,
    object_sha: [u8; 32],
    deadline_ms: u64,
    profile: SourceProfile,
    budget: PullBudget,
) -> Failure {
    let (worker_path, _) = worker();
    let config = ParentConfig::try_new(&worker_path, deadline_ms).expect("parent config");
    let child = ChildRequest::try_new(path, object_sha, profile, budget).expect("child request");
    let mut pull = config.start(&child).expect("parent start");
    loop {
        match pull.next_batch() {
            Ok(Some(batch)) => drop(batch),
            Ok(None) => match pull.finish() {
                Err(ParentError::Failed(failure)) => return failure,
                Ok(_) => panic!("parent returned a completion"),
                Err(ParentError::Config(token)) => panic!("parent config {token}"),
            },
            Err(ParentError::Failed(failure)) => return failure,
            Err(ParentError::Config(token)) => panic!("parent config {token}"),
        }
    }
}

fn delimited_budget(batch_rows: u64, max_rows: u64, max_total: u64, worker_ms: u64) -> Value {
    json!({
        "batch_bytes": 1_048_576,
        "batch_rows": batch_rows,
        "deadline_ms": worker_ms,
        "max_columns": 8,
        "max_field_bytes": 4096,
        "max_fields": 8,
        "max_record_bytes": 65_536,
        "max_rows": max_rows,
        "max_total_bytes": max_total
    })
}

fn jsonl_budget(batch_rows: u64, max_rows: u64, max_total: u64, worker_ms: u64) -> Value {
    json!({
        "batch_bytes": 1_048_576,
        "batch_rows": batch_rows,
        "deadline_ms": worker_ms,
        "max_columns": 8,
        "max_fields": 8,
        "max_record_bytes": 65_536,
        "max_rows": max_rows,
        "max_total_bytes": max_total
    })
}

fn pull_delimited(batch_rows: u32, max_rows: u64, max_total: u64, worker_ms: u64) -> PullBudget {
    PullBudget::delimited(
        batch_rows,
        1_048_576,
        65_536,
        4096,
        max_total,
        max_rows,
        8,
        8,
        worker_ms,
    )
    .expect("delimited budget")
}

fn pull_jsonl(batch_rows: u32, max_rows: u64, max_total: u64, worker_ms: u64) -> PullBudget {
    PullBudget::jsonl(batch_rows, 1_048_576, 65_536, max_total, max_rows, 8, 8, worker_ms)
        .expect("jsonl budget")
}

fn document(
    input: &Path,
    input_sha: &str,
    parent_ms: u64,
    format: &str,
    profile: Value,
    budget: Value,
    executable_sha: &str,
    executable_path: &str,
) -> Vec<u8> {
    let (exe, real_sha) = worker();
    let sha = if executable_sha.is_empty() {
        real_sha
    } else {
        executable_sha.to_owned()
    };
    let path = if executable_path.is_empty() {
        exe.to_str().expect("utf8 exe").to_owned()
    } else {
        executable_path.to_owned()
    };
    serde_json::to_vec(&json!({
        "budget": budget,
        "executable": {"path": path, "sha256": sha},
        "format": format,
        "input": {"path": input, "sha256": input_sha},
        "parent_deadline_ms": parent_ms,
        "profile": profile,
        "protocol": REQUEST_PROTOCOL
    }))
    .expect("request")
}

fn assert_silent(run: &Run, extra: &[&str]) {
    let (exe, _) = worker();
    let exe_text = exe.to_str().expect("utf8 exe");
    for needle in [PATH_TOKEN, exe_text]
        .into_iter()
        .chain(extra.iter().copied())
    {
        assert!(!run.stdout.contains(needle), "stdout leaked");
        assert!(!run.stderr.contains(needle), "stderr leaked");
    }
}

fn assert_error(run: &Run, code: i32, outcome: &str, fact: &str) -> Value {
    assert_eq!(run.code, code, "stderr={}", run.stderr);
    assert!(run.stdout.is_empty(), "error stdout");
    let doc = one_json(&run.stderr);
    assert_eq!(keys(&doc), ERROR_KEYS);
    assert_eq!(doc["protocol"], ERROR_PROTOCOL);
    assert_eq!(doc["outcome"], outcome);
    assert_eq!(doc["fact"], fact);
    doc
}

fn assert_closed(doc: &Value, rows: u64, batches: u64, columns: u64) {
    assert_eq!(keys(doc), RESULT_KEYS);
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
}

fn bind_parent(doc: &Value, fold: &Fold, done: &ParentParseComplete, hand: Hand, object_sha: &str) {
    assert_closed(doc, hand.rows, hand.batches, hand.columns);
    assert_eq!(fold.rows, hand.rows);
    assert_eq!(fold.batches, hand.batches);
    assert_eq!(fold.value, hand.value);
    assert_eq!(fold.null, hand.null);
    assert_eq!(fold.empty, hand.empty);
    assert_eq!(fold.missing, hand.missing);
    assert_eq!(u64::from(done.columns()), hand.columns);
    assert_eq!(doc["lineage"]["fields"], fold.fields);
    assert_eq!(doc["lineage"]["value"], fold.value);
    assert_eq!(doc["lineage"]["null"], fold.null);
    assert_eq!(doc["lineage"]["empty"], fold.empty);
    assert_eq!(doc["lineage"]["missing"], fold.missing);
    assert_eq!(doc["lineage"]["first_line"], fold.first_line);
    assert_eq!(doc["lineage"]["last_line"], fold.last_line);
    assert_eq!(doc["lineage"]["first_byte_offset"], fold.first_offset);
    assert_eq!(doc["lineage"]["last_byte_offset"], fold.last_offset);
    assert_eq!(doc["raw_bytes"], fold.raw_bytes);
    assert_eq!(doc["arrow_bytes"], fold.arrow_bytes);
    assert_eq!(doc["rows"], done.rows());
    assert_eq!(doc["batches"], done.batches());
    assert_eq!(doc["object_sha256"], object_sha);
    assert_eq!(doc["object_sha256"], hex(&done.object_sha256()));
    assert_eq!(doc["profile_id"], done.profile_id());
    assert_eq!(doc["profile_digest"], hex(&done.profile_digest()));
    assert_eq!(doc["config_identity"], hex(&done.config_identity()));
    assert_eq!(doc["schema_identity"], hex(&done.schema_identity()));
    assert_eq!(doc["reader_protocol"], done.reader_protocol());
    let preimage = lineage_preimage(fold);
    assert_eq!(doc["lineage_sha256"], hex(&sha256(preimage.as_bytes())));
    let schema = doc["schema_identity"].as_str().expect("schema");
    assert_eq!(schema.len(), 64);
    assert!(schema.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    if hand.rows == 0 {
        assert_eq!(fold.raw_bytes, 0);
        assert_eq!(fold.arrow_bytes, 0);
        assert_eq!(fold.first_line, 0);
        assert_eq!(fold.last_line, 0);
    } else {
        assert!(fold.raw_bytes > 0);
        assert!(fold.arrow_bytes > 0);
    }
}

fn assert_offsets(fold: &Fold, bytes: &[u8], skip_header: bool) {
    let rows = data_rows(bytes, skip_header);
    if rows.is_empty() {
        assert_eq!(fold.first_line, 0);
        assert_eq!(fold.last_line, 0);
        return;
    }
    assert_eq!(fold.first_line, rows[0].0);
    assert_eq!(fold.first_offset, rows[0].1);
    assert_eq!(fold.last_line, rows.last().expect("last").0);
    assert_eq!(fold.last_offset, rows.last().expect("last").1);
}

fn group_name(signal: GroupSignal) -> &'static str {
    match signal {
        GroupSignal::NotNeeded => "not_needed",
        GroupSignal::Sent => "sent",
        GroupSignal::SkippedIdentity => "skipped_identity",
        GroupSignal::Rejected => "rejected",
    }
}

fn assert_parent_error(doc: &Value, failure: &Failure, kind: FailureKind, token: Option<&str>) {
    assert_eq!(failure.kind, kind);
    assert_eq!(failure.token, token);
    match token {
        Some(text) => assert_eq!(doc["worker_token"], text),
        None => assert!(doc["worker_token"].is_null()),
    }
    assert_eq!(doc["reaped"], failure.teardown.reaped);
    assert_eq!(doc["group_signal"], group_name(failure.teardown.group_signal));
    let exit = failure.teardown.exit_code.map(i64::from);
    let signal = failure.teardown.term_signal.map(i64::from);
    assert_eq!(doc["exit_code"].as_i64(), exit);
    assert_eq!(doc["term_signal"].as_i64(), signal);
}

fn save_success(stem: &str, bytes: &[u8], object_sha: &str, run: &Run, fold: &Fold) {
    let preimage = lineage_preimage(fold);
    save(&format!("{stem}.bin"), bytes);
    save(&format!("{stem}.sha256"), format!("{object_sha}\n").as_bytes());
    save(&format!("{stem}.preimage.txt"), preimage.as_bytes());
    save(
        &format!("{stem}.lineage.sha256"),
        format!("{}\n", hex(&sha256(preimage.as_bytes()))).as_bytes(),
    );
    save(&format!("{stem}.stdout.json"), run.stdout.as_bytes());
}

fn csv_bytes() -> Vec<u8> {
    format!(
        "name,flag,n,price\n\
{CELL_TOKEN},true,-1,1.20\n\
~,false,0,~\n\
\"\",true,7,10\n\
{SECRET_TOKEN},false,2,~\n"
    )
    .into_bytes()
}

fn csv_profile() -> DelimitedProfile {
    DelimitedProfile::try_new(
        "a251csv",
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
        "id": "a251csv",
        "null": {"policy": "token", "text": "~"}
    })
}

fn jsonl_columns() -> Vec<JsonlColumn> {
    vec![
        JsonlColumn::try_new("name", JsonlType::Utf8, true).expect("name"),
        JsonlColumn::try_new("ok", JsonlType::Bool, false).expect("ok"),
        JsonlColumn::try_new("n", JsonlType::Int64, false).expect("n"),
    ]
}

fn jsonl_profile(id: &str) -> JsonlProfile {
    JsonlProfile::try_new(id, jsonl_columns()).expect("jsonl profile")
}

fn jsonl_profile_json(id: &str) -> Value {
    json!({
        "columns": [
            {"name": "name", "nullable": true, "type": "utf8"},
            {"name": "ok", "nullable": false, "type": "bool"},
            {"name": "n", "nullable": false, "type": "int64"}
        ],
        "id": id
    })
}

#[test]
fn help_states_the_closed_limits() {
    let bin = parse_bin();
    assert_eq!(bin.file_name().and_then(|name| name.to_str()), Some("parse-file"));
    let run = run_parse(&["--help"], None);
    assert_eq!(run.code, 0);
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
        "clean EOF",
        "unsupported",
        "parent_deadline_ms",
    ] {
        assert!(run.stdout.contains(phrase), "help missing {phrase}");
    }
    let unknown = run_parse(&["--help", "--extra"], None);
    assert_error(&unknown, 2, "usage", "unknown_flag");
}

#[test]
fn bounded_csv_summary_matches_parent_and_hides_cells() {
    assert_known_sha256();
    let scratch = Scratch::new("csv");
    let bytes = csv_bytes();
    let input = scratch.write("lot.csv", &bytes);
    let sha = file_sha(&input);
    let profile = csv_profile();
    let request = document(
        &input,
        &sha,
        20_000,
        "csv",
        csv_profile_json(),
        delimited_budget(2, 100, 1_048_576, 60_000),
        "",
        "",
    );
    let run = run_parse(&[], Some(&request));
    assert_eq!(run.code, 0, "stderr={}", run.stderr);
    assert!(run.stderr.is_empty());
    let doc = one_json(&run.stdout);
    let (fold, done) = parent_success(
        &input,
        decode_sha(&sha),
        20_000,
        SourceProfile::Delimited(profile.clone()),
        pull_delimited(2, 100, 1_048_576, 60_000),
    );
    bind_parent(
        &doc,
        &fold,
        &done,
        Hand { rows: 4, batches: 2, columns: 4, value: 12, null: 3, empty: 1, missing: 0 },
        &sha,
    );
    assert_offsets(&fold, &bytes, true);
    assert_eq!(done.reader_protocol(), DELIMITED_PROTOCOL);
    assert_eq!(doc["profile_digest"], hex(&profile.profile_digest()));
    assert_eq!(doc["config_identity"], hex(&profile.config_identity(decode_sha(&sha))));
    assert_eq!(doc["types"][3]["type"], "decimal");
    assert_eq!(doc["types"][3]["precision"], 10);
    assert_eq!(doc["types"][3]["scale"], 2);
    assert_silent(&run, &[CELL_TOKEN, SECRET_TOKEN, "1.20"]);
    save_success("csv", &bytes, &sha, &run, &fold);
    let again = run_parse(&[], Some(&request));
    assert_eq!(again.code, 0, "stderr={}", again.stderr);
    let second = one_json(&again.stdout);
    assert_eq!(second["schema_identity"], doc["schema_identity"]);
    assert_eq!(second["lineage_sha256"], doc["lineage_sha256"]);
    assert_eq!(second["raw_bytes"], doc["raw_bytes"]);
    println!(
        "parent_match csv rows=4 batches=2 schema={} profile={}",
        doc["schema_identity"].as_str().unwrap_or(""),
        doc["profile_digest"].as_str().unwrap_or("")
    );
}

#[test]
fn bounded_tsv_summary_matches_parent() {
    let scratch = Scratch::new("tsv");
    let bytes = "name\tn\n甲\t-3\n\t0\n".as_bytes().to_vec();
    let input = scratch.write("tab.tsv", &bytes);
    let sha = file_sha(&input);
    let profile = DelimitedProfile::try_new(
        "a251tsv",
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
    let profile_json = json!({
        "bom": "refuse",
        "columns": [
            {"name": "name", "nullable": true, "type": "utf8"},
            {"name": "n", "nullable": false, "type": "int64"}
        ],
        "dialect": "tsv",
        "header": "exact",
        "id": "a251tsv",
        "null": {"policy": "empty"}
    });
    let request = document(
        &input,
        &sha,
        20_000,
        "tsv",
        profile_json,
        delimited_budget(1, 100, 1_048_576, 60_000),
        "",
        "",
    );
    let run = run_parse(&[], Some(&request));
    assert_eq!(run.code, 0, "stderr={}", run.stderr);
    let doc = one_json(&run.stdout);
    let (fold, done) = parent_success(
        &input,
        decode_sha(&sha),
        20_000,
        SourceProfile::Delimited(profile.clone()),
        pull_delimited(1, 100, 1_048_576, 60_000),
    );
    bind_parent(
        &doc,
        &fold,
        &done,
        Hand { rows: 2, batches: 2, columns: 2, value: 3, null: 1, empty: 0, missing: 0 },
        &sha,
    );
    assert_offsets(&fold, &bytes, true);
    assert_eq!(done.reader_protocol(), DELIMITED_PROTOCOL);
    assert_eq!(doc["profile_digest"], hex(&profile.profile_digest()));
    assert_silent(&run, &["甲", "-3"]);
    save_success("tsv", &bytes, &sha, &run, &fold);
    println!(
        "parent_match tsv rows=2 batches=2 schema={}",
        doc["schema_identity"].as_str().unwrap_or("")
    );
}

#[test]
fn bounded_jsonl_summary_matches_parent() {
    let scratch = Scratch::new("jsonl");
    let bytes = "{\"name\":\"丙\",\"ok\":true,\"n\":1}\n\
{\"name\":null,\"ok\":false,\"n\":2}\n\
{\"ok\":true,\"n\":4}\n"
        .as_bytes()
        .to_vec();
    let input = scratch.write("notes.jsonl", &bytes);
    let sha = file_sha(&input);
    let profile = jsonl_profile("a251jsonl");
    let request = document(
        &input,
        &sha,
        20_000,
        "jsonl",
        jsonl_profile_json("a251jsonl"),
        jsonl_budget(2, 100, 1_048_576, 60_000),
        "",
        "",
    );
    let run = run_parse(&[], Some(&request));
    assert_eq!(run.code, 0, "stderr={}", run.stderr);
    let doc = one_json(&run.stdout);
    let (fold, done) = parent_success(
        &input,
        decode_sha(&sha),
        20_000,
        SourceProfile::Jsonl(profile.clone()),
        pull_jsonl(2, 100, 1_048_576, 60_000),
    );
    bind_parent(
        &doc,
        &fold,
        &done,
        Hand { rows: 3, batches: 2, columns: 3, value: 7, null: 1, empty: 0, missing: 1 },
        &sha,
    );
    assert_offsets(&fold, &bytes, false);
    assert_eq!(done.reader_protocol(), JSONL_PROTOCOL);
    assert_eq!(doc["profile_digest"], hex(&profile.profile_digest()));
    assert_eq!(fold.empty, 0);
    assert_silent(&run, &["丙"]);
    save_success("jsonl", &bytes, &sha, &run, &fold);
    println!(
        "parent_match jsonl rows=3 batches=2 schema={}",
        doc["schema_identity"].as_str().unwrap_or("")
    );
}

#[test]
fn bounded_empty_jsonl_matches_parent() {
    let scratch = Scratch::new("empty");
    let bytes = b"";
    let input = scratch.write("empty.jsonl", bytes);
    let sha = file_sha(&input);
    assert_eq!(sha, hex(&sha256(b"")));
    let profile = JsonlProfile::try_new(
        "a251empty",
        vec![JsonlColumn::try_new("name", JsonlType::Utf8, true).expect("name")],
    )
    .expect("empty profile");
    let profile_json = json!({
        "columns": [{"name": "name", "nullable": true, "type": "utf8"}],
        "id": "a251empty"
    });
    let request = document(
        &input,
        &sha,
        20_000,
        "jsonl",
        profile_json,
        jsonl_budget(4, 100, 1_048_576, 60_000),
        "",
        "",
    );
    let run = run_parse(&[], Some(&request));
    assert_eq!(run.code, 0, "stderr={}", run.stderr);
    let doc = one_json(&run.stdout);
    let (fold, done) = parent_success(
        &input,
        decode_sha(&sha),
        20_000,
        SourceProfile::Jsonl(profile.clone()),
        pull_jsonl(4, 100, 1_048_576, 60_000),
    );
    bind_parent(
        &doc,
        &fold,
        &done,
        Hand { rows: 0, batches: 0, columns: 1, value: 0, null: 0, empty: 0, missing: 0 },
        &sha,
    );
    assert_eq!(done.reader_protocol(), JSONL_PROTOCOL);
    assert_eq!(doc["arrow_ipc"], false);
    assert_silent(&run, &[]);
    save_success("empty", bytes, &sha, &run, &fold);
    println!("parent_match empty rows=0 batches=0 object={sha}");
}

#[test]
fn wrong_executable_hash_does_not_start() {
    let scratch = Scratch::new("exe");
    let input = scratch.write("empty.jsonl", b"");
    let sha = file_sha(&input);
    let (_, real) = worker();
    let request = document(
        &input,
        &sha,
        20_000,
        "jsonl",
        jsonl_profile_json("a251exe"),
        jsonl_budget(4, 100, 1_048_576, 60_000),
        &flip_sha(&real),
        "",
    );
    let run = run_parse(&[], Some(&request));
    let doc = assert_error(&run, 6, "hash", "executable_sha256");
    assert!(doc["worker_token"].is_null());
    assert!(doc["exit_code"].is_null());
    assert!(doc["term_signal"].is_null());
    assert_eq!(doc["reaped"], false);
    assert_eq!(doc["group_signal"], "not_needed");
    assert_silent(&run, &[]);
    save("exe-hash.stderr.json", run.stderr.as_bytes());
}

#[test]
fn wrong_input_hash_has_no_completion() {
    let scratch = Scratch::new("hash");
    let bytes = format!("{{\"name\":\"{CELL_TOKEN}\",\"ok\":true,\"n\":1}}\n");
    let input = scratch.write("row.jsonl", bytes.as_bytes());
    let real = file_sha(&input);
    let flipped = flip_sha(&real);
    let profile = jsonl_profile("a251hash");
    let budget = jsonl_budget(4, 100, 1_048_576, 60_000);
    let request = document(&input, &flipped, 20_000, "jsonl", jsonl_profile_json("a251hash"), budget, "", "");
    let run = run_parse(&[], Some(&request));
    let doc = assert_error(&run, 6, "hash", "hash_mismatch");
    let failure = parent_failure(
        &input,
        decode_sha(&flipped),
        20_000,
        SourceProfile::Jsonl(profile),
        pull_jsonl(4, 100, 1_048_576, 60_000),
    );
    assert_parent_error(&doc, &failure, FailureKind::HashMismatch, Some("hash_mismatch"));
    assert_eq!(failure.teardown.exit_code, Some(6));
    assert!(failure.teardown.reaped);
    assert_silent(&run, &[CELL_TOKEN]);
    save("input-hash.stderr.json", run.stderr.as_bytes());
}

#[test]
fn malformed_quote_has_no_completion() {
    let scratch = Scratch::new("quote");
    let bytes = format!("id\n\"{QUOTE_TOKEN}\n");
    let input = scratch.write("bad.csv", bytes.as_bytes());
    let sha = file_sha(&input);
    let profile_json = json!({
        "bom": "refuse",
        "columns": [{"name": "id", "nullable": false, "type": "utf8"}],
        "dialect": "csv",
        "header": "exact",
        "id": "a251bad",
        "null": {"policy": "empty"}
    });
    let profile = DelimitedProfile::try_new(
        "a251bad",
        DelimitedDialect::try_from_separator(b',').expect("comma"),
        BomPolicy::Refuse,
        HeaderPolicy::Exact,
        NullPolicy::Empty,
        vec![DelimitedColumn::try_new("id", DelimitedType::Utf8, false).expect("id")],
    )
    .expect("bad profile");
    let request = document(
        &input,
        &sha,
        20_000,
        "csv",
        profile_json,
        delimited_budget(4, 100, 1_048_576, 60_000),
        "",
        "",
    );
    let run = run_parse(&[], Some(&request));
    let doc = assert_error(&run, 5, "parent", "ended");
    let failure = parent_failure(
        &input,
        decode_sha(&sha),
        20_000,
        SourceProfile::Delimited(profile),
        pull_delimited(4, 100, 1_048_576, 60_000),
    );
    assert_parent_error(&doc, &failure, FailureKind::Ended, Some("protocol"));
    assert_eq!(failure.teardown.exit_code, Some(4));
    assert_silent(&run, &[QUOTE_TOKEN]);
    save("malformed.stderr.json", run.stderr.as_bytes());
}

#[test]
fn refusal_exact_header_has_no_completion() {
    let scratch = Scratch::new("header");
    let input = scratch.write("empty.csv", b"");
    let sha = file_sha(&input);
    let profile_json = json!({
        "bom": "refuse",
        "columns": [{"name": "id", "nullable": true, "type": "utf8"}],
        "dialect": "csv",
        "header": "exact",
        "id": "a251header",
        "null": {"policy": "empty"}
    });
    let profile = DelimitedProfile::try_new(
        "a251header",
        DelimitedDialect::try_from_separator(b',').expect("comma"),
        BomPolicy::Refuse,
        HeaderPolicy::Exact,
        NullPolicy::Empty,
        vec![DelimitedColumn::try_new("id", DelimitedType::Utf8, true).expect("id")],
    )
    .expect("header profile");
    let request = document(
        &input,
        &sha,
        20_000,
        "csv",
        profile_json,
        delimited_budget(4, 100, 1_048_576, 60_000),
        "",
        "",
    );
    let run = run_parse(&[], Some(&request));
    let doc = assert_error(&run, 5, "parent", "ended");
    let failure = parent_failure(
        &input,
        decode_sha(&sha),
        20_000,
        SourceProfile::Delimited(profile),
        pull_delimited(4, 100, 1_048_576, 60_000),
    );
    assert_parent_error(&doc, &failure, FailureKind::Ended, Some("header_required"));
    assert_eq!(failure.teardown.exit_code, Some(4));
    assert!(run.stdout.is_empty());
    save("header.stderr.json", run.stderr.as_bytes());
}

#[test]
fn refusal_credential_does_not_start() {
    let scratch = Scratch::new("cred");
    let input = scratch.write("empty.jsonl", b"");
    let sha = file_sha(&input);
    let mut request = document(
        &input,
        &sha,
        20_000,
        "jsonl",
        jsonl_profile_json("a251cred"),
        jsonl_budget(4, 100, 1_048_576, 60_000),
        "",
        "",
    );
    let mut value: Value = serde_json::from_slice(&request).expect("request");
    value
        .as_object_mut()
        .expect("object")
        .insert("password".to_owned(), json!(CRED_TOKEN));
    request = serde_json::to_vec(&value).expect("request");
    let run = run_parse(&[], Some(&request));
    let doc = assert_error(&run, 3, "config", "credentials");
    assert_eq!(doc["reaped"], false);
    assert!(doc["worker_token"].is_null());
    assert!(doc["exit_code"].is_null());
    assert_silent(&run, &[CRED_TOKEN, "password"]);
    save("credential.stderr.json", run.stderr.as_bytes());
}

#[test]
fn refusal_worker_path_does_not_hash() {
    let scratch = Scratch::new("path");
    let input = scratch.write("empty.jsonl", b"");
    let sha = file_sha(&input);
    let (_, real) = worker();
    let request = document(
        &input,
        &sha,
        20_000,
        "jsonl",
        jsonl_profile_json("a251path"),
        jsonl_budget(4, 100, 1_048_576, 60_000),
        &real,
        "PATHTOKEN_a251/worker",
    );
    let run = run_parse(&[], Some(&request));
    let doc = assert_error(&run, 3, "config", "worker_path");
    assert_eq!(doc["reaped"], false);
    assert_eq!(doc["group_signal"], "not_needed");
    assert!(doc["worker_token"].is_null());
    assert!(doc["exit_code"].is_null());
    assert_silent(&run, &[]);
    save("worker-path.stderr.json", run.stderr.as_bytes());
}

fn busy_jsonl(rows: usize) -> Vec<u8> {
    let line = b"{\"n\":1}\n";
    let mut out = Vec::with_capacity(line.len().saturating_mul(rows));
    for _ in 0..rows {
        out.extend_from_slice(line);
    }
    out
}

fn timed(stdin_bytes: &[u8]) -> (Duration, Run) {
    let started = Instant::now();
    let run = run_parse(&[], Some(stdin_bytes));
    (started.elapsed(), run)
}

#[test]
fn z_deadline_kills_then_the_next_call_completes() {
    const ROWS: u64 = 40_000;
    const BATCH_ROWS: u64 = 16;
    let scratch = Scratch::new("deadline");
    let empty = scratch.write("empty.jsonl", b"");
    let empty_sha = file_sha(&empty);
    let empty_profile = JsonlProfile::try_new(
        "a251after",
        vec![JsonlColumn::try_new("n", JsonlType::Int64, true).expect("n")],
    )
    .expect("after");
    let empty_json = json!({
        "columns": [{"name": "n", "nullable": true, "type": "int64"}],
        "id": "a251after"
    });
    let empty_request = document(
        &empty,
        &empty_sha,
        20_000,
        "jsonl",
        empty_json.clone(),
        jsonl_budget(4, 100, 1_048_576, 60_000),
        "",
        "",
    );
    let (empty_elapsed, empty_run) = timed(&empty_request);
    println!("empty_ms={}", empty_elapsed.as_millis());
    assert_eq!(empty_run.code, 0, "stderr={}", empty_run.stderr);
    let empty_doc = one_json(&empty_run.stdout);
    let (empty_fold, empty_done) = parent_success(
        &empty,
        decode_sha(&empty_sha),
        20_000,
        SourceProfile::Jsonl(empty_profile.clone()),
        pull_jsonl(4, 100, 1_048_576, 60_000),
    );
    bind_parent(
        &empty_doc,
        &empty_fold,
        &empty_done,
        Hand { rows: 0, batches: 0, columns: 1, value: 0, null: 0, empty: 0, missing: 0 },
        &empty_sha,
    );

    let bytes = busy_jsonl(usize::try_from(ROWS).expect("rows"));
    let input = scratch.write("busy.jsonl", &bytes);
    let sha = file_sha(&input);
    let total = u64::try_from(bytes.len()).expect("len");
    let busy_json = json!({
        "columns": [{"name": "n", "nullable": false, "type": "int64"}],
        "id": "a251busy"
    });
    let wide = jsonl_budget(BATCH_ROWS, ROWS, total, 3_600_000);
    let full = document(&input, &sha, 120_000, "jsonl", busy_json.clone(), wide.clone(), "", "");
    let (busy_elapsed, parsed) = timed(&full);
    println!("busy_ms={}", busy_elapsed.as_millis());
    assert_eq!(parsed.code, 0, "stderr={}", parsed.stderr);
    let summary = one_json(&parsed.stdout);
    let batches = ROWS.div_ceil(BATCH_ROWS);
    assert_closed(&summary, ROWS, batches, 1);
    assert_eq!(summary["lineage"]["value"], ROWS);
    assert_eq!(summary["lineage"]["first_line"], 1);
    assert_eq!(summary["lineage"]["last_line"], ROWS);
    assert_eq!(summary["lineage"]["last_byte_offset"], (ROWS - 1) * 8);
    assert_eq!(summary["admits_import"], false);
    assert_eq!(summary["kernel_restriction"], "unsupported");
    assert!(
        busy_elapsed > empty_elapsed.saturating_mul(2),
        "empty_ms={} busy_ms={}",
        empty_elapsed.as_millis(),
        busy_elapsed.as_millis()
    );

    let gap = busy_elapsed.saturating_sub(empty_elapsed);
    let kill_ms = u64::try_from(gap.as_millis() / 2).expect("half gap");
    println!("kill_ms={kill_ms}");
    assert!((1..=120_000).contains(&kill_ms), "kill deadline {kill_ms}");
    assert!(
        u128::from(kill_ms) > empty_elapsed.as_millis(),
        "kill_ms={kill_ms} empty_ms={}",
        empty_elapsed.as_millis()
    );
    let killed = document(&input, &sha, kill_ms, "jsonl", busy_json, wide, "", "");
    let (kill_elapsed, run) = timed(&killed);
    println!("kill_elapsed_ms={}", kill_elapsed.as_millis());
    assert!(
        kill_elapsed < busy_elapsed,
        "kill_ms={} busy_ms={}",
        kill_elapsed.as_millis(),
        busy_elapsed.as_millis()
    );
    let doc = assert_error(&run, 7, "deadline", "timed_out");
    assert_eq!(doc["reaped"], true);
    assert_eq!(doc["group_signal"], "sent");
    assert!(doc["worker_token"].is_null());
    let signal = doc["term_signal"].as_i64();
    let child_exit = doc["exit_code"].as_i64();
    let killed_by_signal = signal == Some(9) && child_exit.is_none();
    let exited_on_closed_pipe = signal.is_none() && child_exit == Some(7);
    println!("term_signal={signal:?} exit_code={child_exit:?}");
    assert!(
        killed_by_signal || exited_on_closed_pipe,
        "empty_ms={} busy_ms={} kill_ms={kill_ms} kill_elapsed_ms={}",
        empty_elapsed.as_millis(),
        busy_elapsed.as_millis(),
        kill_elapsed.as_millis()
    );
    let teardown = if killed_by_signal { "signal9" } else { "closed_pipe_exit7" };
    let note = format!(
        "empty_ms={}\nbusy_ms={}\nkill_ms={kill_ms}\nkill_elapsed_ms={}\nterm_signal={signal:?}\nexit_code={child_exit:?}\nteardown={teardown}\n",
        empty_elapsed.as_millis(),
        busy_elapsed.as_millis(),
        kill_elapsed.as_millis()
    );
    save("deadline.txt", note.as_bytes());
    save("deadline.stderr.json", run.stderr.as_bytes());
    assert_silent(&run, &[]);

    let again = document(
        &empty,
        &empty_sha,
        20_000,
        "jsonl",
        empty_json,
        jsonl_budget(4, 100, 1_048_576, 60_000),
        "",
        "",
    );
    let next = run_parse(&[], Some(&again));
    assert_eq!(next.code, 0, "stderr={}", next.stderr);
    let recovered = one_json(&next.stdout);
    let (fold, done) = parent_success(
        &empty,
        decode_sha(&empty_sha),
        20_000,
        SourceProfile::Jsonl(empty_profile),
        pull_jsonl(4, 100, 1_048_576, 60_000),
    );
    bind_parent(
        &recovered,
        &fold,
        &done,
        Hand { rows: 0, batches: 0, columns: 1, value: 0, null: 0, empty: 0, missing: 0 },
        &empty_sha,
    );
    assert_silent(&next, &[]);
    println!("deadline_next=empty_ok teardown={teardown}");
}
