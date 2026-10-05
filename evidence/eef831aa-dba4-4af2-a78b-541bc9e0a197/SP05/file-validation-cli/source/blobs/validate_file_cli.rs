//! Process tests for `validate-file`.
//!
//! Expected digests are SHA-256 of fixture bytes and of the profile preimage
//! written in this file. Expected presence comes from the fixture table.
//! `oo_core::ExactDecimal` checks the decimal texts. The test does not downcast
//! Arrow arrays: `arrow-array` is not a direct dependency of this package.
//! The compiled `validate-file` process has to agree with those expectations
//! and with a pull through the public reader constructors.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use oo_connectors::{
    BomPolicy, DELIMITED_LINEAGE_VERSION, DELIMITED_PROFILE_VERSION, DELIMITED_PROTOCOL,
    DELIMITED_RECEIPT_VERSION, DelimitedBudget, DelimitedColumn, DelimitedDialect,
    DelimitedFieldPresence, DelimitedItem, DelimitedProfile, DelimitedReader, DelimitedType,
    FieldPresence, HeaderPolicy, JSONL_LINEAGE_VERSION, JSONL_PROFILE_VERSION, JSONL_PROTOCOL,
    JSONL_RECEIPT_VERSION, JsonlBudget, JsonlColumn, JsonlItem, JsonlProfile, JsonlReader,
    JsonlType, NullPolicy,
};
use oo_core::ExactDecimal;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const REQUEST_PROTOCOL: &str = "oo.pipeline.validate-file/v1";
const RESULT_PROTOCOL: &str = "oo.pipeline.validate-file-result/v1";
const ERROR_PROTOCOL: &str = "oo.pipeline.validate-file-error/v1";
const REQUEST_MAX: usize = 1_048_576;

const RESULT_KEYS: &[&str] = &[
    "admits_import",
    "arrow_ipc",
    "batches",
    "columns",
    "config_identity",
    "executes_native",
    "lineage",
    "lineage_version",
    "object_sha256",
    "profile_digest",
    "profile_id",
    "profile_version",
    "protocol",
    "published",
    "receipt_version",
    "rows",
    "source_protocol",
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

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = env::temp_dir().join(format!("oo-a197-{label}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).expect("scratch");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
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

fn run_validate(args: &[&str]) -> Run {
    let bin = cargo_bin("CARGO_BIN_EXE_validate_file", "CARGO_BIN_EXE_validate-file");
    let output = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("spawn validate-file");
    Run {
        code: status_code(output.status),
        stdout: String::from_utf8(output.stdout).expect("stdout"),
        stderr: String::from_utf8(output.stderr).expect("stderr"),
    }
}

fn run_pipeline(args: &[&str], stdin_bytes: Option<&[u8]>) -> Run {
    let bin = cargo_bin("CARGO_BIN_EXE_oo_pipeline", "CARGO_BIN_EXE_oo-pipeline");
    let mut command = Command::new(bin);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin_bytes.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    let mut child = command.spawn().expect("spawn oo-pipeline");
    if let Some(bytes) = stdin_bytes {
        use std::io::Write;
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(bytes)
            .expect("stdin write");
    }
    let output = child.wait_with_output().expect("wait");
    Run {
        code: status_code(output.status),
        stdout: String::from_utf8(output.stdout).expect("stdout"),
        stderr: String::from_utf8(output.stderr).expect("stderr"),
    }
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
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

fn keys(value: &Value) -> Vec<&str> {
    value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect()
}

fn one_json(text: &str) -> Value {
    assert_eq!(text.matches('\n').count(), 1, "{text}");
    assert!(text.ends_with('\n'));
    serde_json::from_str(text).expect("json")
}

fn assert_failure(run: &Run, code: i32, outcome: &str, fact: &str) {
    assert_eq!(
        run.code, code,
        "stdout={} stderr={}",
        run.stdout, run.stderr
    );
    assert!(run.stdout.is_empty(), "{}", run.stdout);
    let err = one_json(&run.stderr);
    assert_eq!(keys(&err), ["fact", "outcome", "protocol"]);
    assert_eq!(err["protocol"], ERROR_PROTOCOL);
    assert_eq!(err["outcome"], outcome);
    assert_eq!(err["fact"], fact);
    assert!(!run.stderr.contains("awaiting_publish"));
    assert!(!run.stdout.contains(RESULT_PROTOCOL));
}

fn oracle(name: &str, bytes: &[u8]) {
    let Ok(dir) = env::var("OO_A197_ORACLE_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir);
    fs::create_dir_all(&dir).expect("oracle dir");
    fs::write(dir.join(name), bytes).expect("oracle write");
}

fn line_starts(bytes: &[u8], skip: usize) -> Vec<u64> {
    assert!(bytes.len() >= skip);
    let mut starts = vec![skip as u64];
    for (index, byte) in bytes[skip..].iter().enumerate() {
        if *byte == b'\n' && index + 1 < bytes.len() - skip {
            starts.push((skip + index + 1) as u64);
        }
    }
    starts
}

fn delimited_budget() -> DelimitedBudget {
    DelimitedBudget::try_new(
        1,
        1_048_576,
        4096,
        4096,
        1_048_576,
        100,
        8,
        8,
        Instant::now() + Duration::from_secs(60),
    )
    .expect("budget")
}

fn jsonl_budget_reader() -> JsonlBudget {
    JsonlBudget::try_new(
        1,
        1_048_576,
        4096,
        1_048_576,
        100,
        8,
        8,
        Instant::now() + Duration::from_secs(60),
    )
    .expect("budget")
}

fn budget_json(jsonl: bool) -> Value {
    let mut value = json!({
        "batch_bytes": 1_048_576,
        "batch_rows": 1,
        "deadline_ms": 60_000,
        "max_columns": 8,
        "max_field_bytes": 4096,
        "max_fields": 8,
        "max_record_bytes": 4096,
        "max_rows": 100,
        "max_total_bytes": 1_048_576,
    });
    if jsonl {
        value
            .as_object_mut()
            .expect("budget")
            .remove("max_field_bytes");
    }
    value
}

fn request(
    format: &str,
    input: &Path,
    sha: &str,
    profile: Value,
    budget: Value,
    cancel: bool,
) -> Value {
    json!({
        "budget": budget,
        "cancel_before_read": cancel,
        "expected_sha256": sha,
        "format": format,
        "input_path": input.to_str().expect("utf8 path"),
        "profile": profile,
        "protocol": REQUEST_PROTOCOL,
    })
}

fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).expect("request")).expect("write request");
}

fn config_text(prefix: &str, preimage: &str, object_hex: &str) -> String {
    format!("{prefix}{preimage}expected_sha256={object_hex}\n")
}

fn csv_bytes() -> Vec<u8> {
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(b"label,flag,count,amount\n");
    bytes.extend_from_slice(b"alpha,true,1,1.2\n");
    bytes.extend_from_slice(b"\"a,b\",false,,-0.00\n");
    bytes.extend_from_slice(b"\"\",true,-9223372036854775808,1.250\n");
    bytes.extend_from_slice(b"max,false,9223372036854775807,10\n");
    bytes
}

fn csv_preimage() -> String {
    "\
oo-delimited-profile/v1
id=sample
dialect=csv
bom=strip
header=exact
null=empty
quote=double
record=lf-or-crlf
escape=double-quote
column=label\tutf8\t1\t-\t-
column=flag\tbool\t0\t-\t-
column=count\tint64\t1\t-\t-
column=amount\tdecimal\t1\t10\t2
"
    .to_owned()
}

fn csv_profile() -> Value {
    json!({
        "bom": "strip",
        "columns": [
            {"name": "label", "nullable": true, "type": "utf8"},
            {"name": "flag", "nullable": false, "type": "bool"},
            {"name": "count", "nullable": true, "type": "int64"},
            {"name": "amount", "nullable": true, "precision": 10, "scale": 2, "type": "decimal"}
        ],
        "dialect": "csv",
        "header": "exact",
        "id": "sample",
        "null": {"policy": "empty"}
    })
}

fn tsv_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"ada\t1\ttrue\t1.20\n");
    bytes.extend_from_slice(b"\\N\t\\N\tfalse\t\\N\n");
    bytes.extend_from_slice(b"\"\\N\"\t0\ttrue\t10\n");
    bytes
}

fn tsv_preimage() -> String {
    "\
oo-delimited-profile/v1
id=sheet
dialect=tsv
bom=refuse
header=absent
null=token
null_token=\\N
quote=double
record=lf-or-crlf
escape=double-quote
column=name\tutf8\t1\t-\t-
column=qty\tint64\t1\t-\t-
column=ok\tbool\t0\t-\t-
column=price\tdecimal\t1\t12\t2
"
    .to_owned()
}

fn tsv_profile() -> Value {
    json!({
        "bom": "refuse",
        "columns": [
            {"name": "name", "nullable": true, "type": "utf8"},
            {"name": "qty", "nullable": true, "type": "int64"},
            {"name": "ok", "nullable": false, "type": "bool"},
            {"name": "price", "nullable": true, "precision": 12, "scale": 2, "type": "decimal"}
        ],
        "dialect": "tsv",
        "header": "absent",
        "id": "sheet",
        "null": {"policy": "token", "token": "\\N"}
    })
}

fn jsonl_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(
        b"{\"label\":\"alpha\",\"flag\":true,\"count\":1,\"payload\":{\"n\":1}}\n",
    );
    bytes.extend_from_slice(
        b"{\"label\":\"beta\",\"flag\":false,\"count\":null,\"payload\":null}\n",
    );
    bytes.extend_from_slice(b"{\"flag\":true,\"count\":-0,\"label\":\"gamma\"}\n");
    bytes.extend_from_slice(
        b"{\"label\":\"delta\",\"flag\":false,\"count\":9223372036854775807,\"payload\":[1,\"x\"]}\n",
    );
    bytes.extend_from_slice(
        b"{\"label\":\"eps\",\"flag\":true,\"count\":-9223372036854775808,\"payload\":\"text\"}\n",
    );
    bytes
}

fn jsonl_preimage() -> String {
    "\
oo-jsonl-profile/v1
id=events
column=label\tutf8\t0
column=flag\tbool\t0
column=count\tint64\t1
column=payload\tjson_text\t1
"
    .to_owned()
}

fn jsonl_profile() -> Value {
    json!({
        "columns": [
            {"name": "label", "nullable": false, "type": "utf8"},
            {"name": "flag", "nullable": false, "type": "bool"},
            {"name": "count", "nullable": true, "type": "int64"},
            {"name": "payload", "nullable": true, "type": "json_text"}
        ],
        "id": "events"
    })
}

fn assert_closed(doc: &Value) {
    assert_eq!(keys(doc), RESULT_KEYS);
    assert_eq!(doc["protocol"], RESULT_PROTOCOL);
    assert_eq!(doc["admits_import"].as_bool(), Some(false));
    assert_eq!(doc["published"].as_bool(), Some(false));
    assert_eq!(doc["arrow_ipc"].as_bool(), Some(false));
    assert_eq!(doc["executes_native"].as_bool(), Some(false));
    assert_eq!(keys(&doc["lineage"]), LINEAGE_KEYS);
}

#[allow(clippy::too_many_arguments)]
fn assert_lineage(
    doc: &Value,
    rows: u64,
    value: u64,
    nulls: u64,
    empty: u64,
    missing: u64,
    first_line: u64,
    last_line: u64,
    first_offset: u64,
    last_offset: u64,
) {
    let lineage = &doc["lineage"];
    let fields = rows * doc["columns"].as_u64().expect("columns");
    assert_eq!(doc["rows"], rows);
    assert_eq!(doc["batches"], rows);
    assert_eq!(lineage["rows"], rows);
    assert_eq!(lineage["fields"], fields);
    assert_eq!(lineage["value"], value);
    assert_eq!(lineage["null"], nulls);
    assert_eq!(lineage["empty"], empty);
    assert_eq!(lineage["missing"], missing);
    assert_eq!(lineage["first_line"], first_line);
    assert_eq!(lineage["last_line"], last_line);
    assert_eq!(lineage["first_byte_offset"], first_offset);
    assert_eq!(lineage["last_byte_offset"], last_offset);
}

fn assert_no_cells(stdout: &str, input: &Path) {
    assert!(!stdout.contains("alpha"));
    assert!(!stdout.contains("1.2"));
    assert!(!stdout.contains("a,b"));
    assert!(!stdout.contains("{\"n\":1}"));
    assert!(!stdout.contains(input.to_str().expect("path")));
    assert!(!stdout.contains("password"));
    assert!(!stdout.contains("secret"));
}

#[test]
fn positive_csv_matches_independent_receipt_and_exact_decimal() {
    assert_known_sha256();
    let scaled = ExactDecimal::parse_sql("1.2")
        .expect("1.2")
        .rescale_exact(2)
        .expect("scale");
    assert_eq!(
        (scaled.coefficient(), scaled.scale(), scaled.is_negative()),
        ("120", 2, false)
    );
    let zero = ExactDecimal::parse_sql("-0.00")
        .expect("zero")
        .rescale_exact(2)
        .expect("scale");
    assert_eq!((zero.coefficient(), zero.is_negative()), ("0", false));
    let trail = ExactDecimal::parse_sql("1.250")
        .expect("trail")
        .rescale_exact(2)
        .expect("scale");
    assert_eq!((trail.coefficient(), trail.scale()), ("125", 2));
    let ten = ExactDecimal::parse_sql("10")
        .expect("ten")
        .rescale_exact(2)
        .expect("scale");
    assert_eq!((ten.coefficient(), ten.scale()), ("1000", 2));
    assert!(
        ExactDecimal::parse_sql("1.251")
            .expect("inexact")
            .rescale_exact(2)
            .is_err()
    );

    let bytes = csv_bytes();
    let digest = sha256(&bytes);
    let preimage = csv_preimage();
    let profile_digest = sha256(preimage.as_bytes());
    let config = config_text(
        "oo-delimited-config-identity/v1\n",
        &preimage,
        &hex(&digest),
    );
    let config_digest = sha256(config.as_bytes());
    let starts = line_starts(&bytes, 3);
    assert_eq!(starts.len(), 5);

    let scratch = Scratch::new("csv");
    let input = scratch.join("input.csv");
    fs::write(&input, &bytes).expect("input");
    let profile = DelimitedProfile::try_new(
        "sample",
        DelimitedDialect::try_from_separator(b',').expect("csv"),
        BomPolicy::Strip,
        HeaderPolicy::Exact,
        NullPolicy::Empty,
        vec![
            DelimitedColumn::try_new("label", DelimitedType::Utf8, true).expect("col"),
            DelimitedColumn::try_new("flag", DelimitedType::Bool, false).expect("col"),
            DelimitedColumn::try_new("count", DelimitedType::Int64, true).expect("col"),
            DelimitedColumn::try_new(
                "amount",
                DelimitedType::Decimal {
                    precision: 10,
                    scale: 2,
                },
                true,
            )
            .expect("col"),
        ],
    )
    .expect("profile");
    assert_eq!(profile.profile_preimage(), preimage);
    assert_eq!(profile.profile_digest(), profile_digest);
    assert_eq!(profile.config_preimage(digest), config);
    assert_eq!(profile.config_identity(digest), config_digest);
    let observed = pull_delimited(&input, profile, digest);
    assert_eq!(
        observed,
        vec![
            row(
                2,
                starts[1],
                &["label", "flag", "count", "amount"],
                &["value", "value", "value", "value"]
            ),
            row(
                3,
                starts[2],
                &["label", "flag", "count", "amount"],
                &["value", "value", "null", "value"]
            ),
            row(
                4,
                starts[3],
                &["label", "flag", "count", "amount"],
                &["empty", "value", "value", "value"]
            ),
            row(
                5,
                starts[4],
                &["label", "flag", "count", "amount"],
                &["value", "value", "value", "value"]
            ),
        ]
    );

    let request_path = scratch.join("request.json");
    write_json(
        &request_path,
        &request(
            "csv",
            &input,
            &hex(&digest),
            csv_profile(),
            budget_json(false),
            false,
        ),
    );
    let run = run_validate(&["--request", request_path.to_str().expect("path")]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stderr.is_empty(), "{}", run.stderr);
    let doc = one_json(&run.stdout);
    assert_closed(&doc);
    assert_eq!(doc["source_protocol"], DELIMITED_PROTOCOL);
    assert_eq!(doc["profile_version"], DELIMITED_PROFILE_VERSION);
    assert_eq!(doc["lineage_version"], DELIMITED_LINEAGE_VERSION);
    assert_eq!(doc["receipt_version"], DELIMITED_RECEIPT_VERSION);
    assert_eq!(doc["profile_id"], "sample");
    assert_eq!(doc["profile_digest"], hex(&profile_digest));
    assert_eq!(doc["config_identity"], hex(&config_digest));
    assert_eq!(doc["object_sha256"], hex(&digest));
    assert_eq!(doc["columns"], 4);
    assert_lineage(&doc, 4, 14, 1, 1, 0, 2, 5, starts[1], starts[4]);
    assert_eq!(doc["types"][3]["type"], "decimal");
    assert_eq!(doc["types"][3]["precision"], 10);
    assert_eq!(doc["types"][3]["scale"], 2);
    assert_eq!(doc["types"][0]["type"], "utf8");
    assert_eq!(doc["types"][1]["type"], "bool");
    assert_eq!(doc["types"][2]["type"], "int64");
    assert_no_cells(&run.stdout, &input);
    oracle("positive-csv.bin", &bytes);
    oracle("positive-csv.preimage.txt", preimage.as_bytes());
    oracle(
        "positive-csv.sha256.txt",
        format!("{}\n", hex(&digest)).as_bytes(),
    );
    oracle("positive-csv.stdout.json", run.stdout.as_bytes());
}

#[test]
fn positive_tsv_token_null_and_quoted_token() {
    let bytes = tsv_bytes();
    let digest = sha256(&bytes);
    let preimage = tsv_preimage();
    let profile_digest = sha256(preimage.as_bytes());
    let config = config_text(
        "oo-delimited-config-identity/v1\n",
        &preimage,
        &hex(&digest),
    );
    let starts = line_starts(&bytes, 0);
    assert_eq!(starts.len(), 3);
    let scratch = Scratch::new("tsv");
    let input = scratch.join("input.tsv");
    fs::write(&input, &bytes).expect("input");
    let profile = DelimitedProfile::try_new(
        "sheet",
        DelimitedDialect::try_from_separator(b'\t').expect("tsv"),
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Token("\\N".to_owned()),
        vec![
            DelimitedColumn::try_new("name", DelimitedType::Utf8, true).expect("col"),
            DelimitedColumn::try_new("qty", DelimitedType::Int64, true).expect("col"),
            DelimitedColumn::try_new("ok", DelimitedType::Bool, false).expect("col"),
            DelimitedColumn::try_new(
                "price",
                DelimitedType::Decimal {
                    precision: 12,
                    scale: 2,
                },
                true,
            )
            .expect("col"),
        ],
    )
    .expect("profile");
    assert_eq!(profile.profile_preimage(), preimage);
    assert_eq!(profile.config_identity(digest), sha256(config.as_bytes()));
    let price = ExactDecimal::parse_sql("1.20")
        .expect("price")
        .rescale_exact(2)
        .expect("scale");
    assert_eq!((price.coefficient(), price.scale()), ("120", 2));
    let observed = pull_delimited(&input, profile, digest);
    assert_eq!(
        observed,
        vec![
            row(
                1,
                starts[0],
                &["name", "qty", "ok", "price"],
                &["value", "value", "value", "value"]
            ),
            row(
                2,
                starts[1],
                &["name", "qty", "ok", "price"],
                &["null", "null", "value", "null"]
            ),
            row(
                3,
                starts[2],
                &["name", "qty", "ok", "price"],
                &["value", "value", "value", "value"]
            ),
        ]
    );
    let request_path = scratch.join("request.json");
    write_json(
        &request_path,
        &request(
            "tsv",
            &input,
            &hex(&digest),
            tsv_profile(),
            budget_json(false),
            false,
        ),
    );
    let run = run_validate(&["--request", request_path.to_str().expect("path")]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let doc = one_json(&run.stdout);
    assert_closed(&doc);
    assert_eq!(doc["source_protocol"], "oo.source.delimited/v1");
    assert_eq!(doc["profile_id"], "sheet");
    assert_eq!(doc["profile_digest"], hex(&profile_digest));
    assert_eq!(doc["object_sha256"], hex(&digest));
    assert_eq!(doc["columns"], 4);
    assert_lineage(&doc, 3, 9, 3, 0, 0, 1, 3, starts[0], starts[2]);
    assert_eq!(doc["types"][3]["precision"], 12);
    assert_no_cells(&run.stdout, &input);
    oracle("positive-tsv.bin", &bytes);
    oracle("positive-tsv.preimage.txt", preimage.as_bytes());
    oracle(
        "positive-tsv.sha256.txt",
        format!("{}\n", hex(&digest)).as_bytes(),
    );
    oracle("positive-tsv.stdout.json", run.stdout.as_bytes());
}

#[test]
fn positive_jsonl_parses_every_type_including_nested_text() {
    let bytes = jsonl_bytes();
    let digest = sha256(&bytes);
    let preimage = jsonl_preimage();
    let profile_digest = sha256(preimage.as_bytes());
    let config = config_text("oo-jsonl-config-identity/v1\n", &preimage, &hex(&digest));
    let config_digest = sha256(config.as_bytes());
    let starts = line_starts(&bytes, 0);
    assert_eq!(starts.len(), 5);
    let scratch = Scratch::new("jsonl");
    let input = scratch.join("input.jsonl");
    fs::write(&input, &bytes).expect("input");
    let names = ["label", "flag", "count", "payload"];
    let profile = JsonlProfile::try_new(
        "events",
        vec![
            JsonlColumn::try_new("label", JsonlType::Utf8, false).expect("col"),
            JsonlColumn::try_new("flag", JsonlType::Bool, false).expect("col"),
            JsonlColumn::try_new("count", JsonlType::Int64, true).expect("col"),
            JsonlColumn::try_new("payload", JsonlType::JsonText, true).expect("col"),
        ],
    )
    .expect("profile");
    assert_eq!(profile.profile_digest(), profile_digest);
    assert_eq!(profile.config_identity(digest), config_digest);
    let observed = pull_jsonl(&input, profile, digest);
    assert_eq!(
        observed,
        vec![
            row(1, starts[0], &names, &["value", "value", "value", "value"]),
            row(2, starts[1], &names, &["value", "value", "null", "null"]),
            row(
                3,
                starts[2],
                &names,
                &["value", "value", "value", "missing"]
            ),
            row(4, starts[3], &names, &["value", "value", "value", "value"]),
            row(5, starts[4], &names, &["value", "value", "value", "value"]),
        ]
    );
    let request_path = scratch.join("request.json");
    write_json(
        &request_path,
        &request(
            "jsonl",
            &input,
            &hex(&digest),
            jsonl_profile(),
            budget_json(true),
            false,
        ),
    );
    let run = run_validate(&["--request", request_path.to_str().expect("path")]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let doc = one_json(&run.stdout);
    assert_closed(&doc);
    assert_eq!(doc["source_protocol"], JSONL_PROTOCOL);
    assert_eq!(doc["profile_version"], JSONL_PROFILE_VERSION);
    assert_eq!(doc["lineage_version"], JSONL_LINEAGE_VERSION);
    assert_eq!(doc["receipt_version"], JSONL_RECEIPT_VERSION);
    assert_eq!(doc["profile_id"], "events");
    assert_eq!(doc["profile_digest"], hex(&profile_digest));
    assert_eq!(doc["config_identity"], hex(&config_digest));
    assert_eq!(doc["object_sha256"], hex(&digest));
    assert_eq!(doc["columns"], 4);
    assert_lineage(&doc, 5, 17, 2, 0, 1, 1, 5, starts[0], starts[4]);
    assert_eq!(doc["types"][3]["type"], "json_text");
    assert_eq!(doc["types"][3]["nullable"].as_bool(), Some(true));
    assert_no_cells(&run.stdout, &input);
    oracle("positive-jsonl.bin", &bytes);
    oracle("positive-jsonl.preimage.txt", preimage.as_bytes());
    oracle(
        "positive-jsonl.sha256.txt",
        format!("{}\n", hex(&digest)).as_bytes(),
    );
    oracle("positive-jsonl.stdout.json", run.stdout.as_bytes());
}

#[test]
fn empty_absent_header_completes_with_the_empty_digest() {
    assert_known_sha256();
    let bytes = Vec::new();
    let digest = sha256(&bytes);
    assert_eq!(
        hex(&digest),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    let scratch = Scratch::new("empty");
    let input = scratch.join("empty.csv");
    fs::write(&input, &bytes).expect("input");
    let mut profile = csv_profile();
    let obj = profile.as_object_mut().expect("profile");
    obj.insert("id".into(), json!("empty"));
    obj.insert("bom".into(), json!("refuse"));
    obj.insert("header".into(), json!("absent"));
    obj.insert(
        "columns".into(),
        json!([{"name": "note", "nullable": false, "type": "utf8"}]),
    );
    let request_path = scratch.join("request.json");
    write_json(
        &request_path,
        &request(
            "csv",
            &input,
            &hex(&digest),
            profile,
            budget_json(false),
            false,
        ),
    );
    let run = run_validate(&["--request", request_path.to_str().expect("path")]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let doc = one_json(&run.stdout);
    assert_closed(&doc);
    assert_eq!(doc["rows"], 0);
    assert_eq!(doc["batches"], 0);
    assert_eq!(doc["object_sha256"], hex(&digest));
    assert_lineage(&doc, 0, 0, 0, 0, 0, 0, 0, 0, 0);
}

#[test]
fn failures_have_no_success_receipt() {
    let scratch = Scratch::new("fail");
    let bytes = csv_bytes();
    let input = scratch.join("input.csv");
    fs::write(&input, &bytes).expect("input");
    let digest = hex(&sha256(&bytes));
    let mut flipped: Vec<u8> = digest.clone().into_bytes();
    let last = flipped.len() - 1;
    flipped[last] = if flipped[last] == b'0' { b'1' } else { b'0' };
    let flipped = String::from_utf8(flipped).expect("hex");
    let request_path = scratch.join("hash.json");
    write_json(
        &request_path,
        &request(
            "csv",
            &input,
            &flipped,
            csv_profile(),
            budget_json(false),
            false,
        ),
    );
    let hash_run = run_validate(&["--request", request_path.to_str().expect("path")]);
    assert_failure(&hash_run, 7, "hash", "hash_mismatch");
    assert!(!hash_run.stderr.contains("alpha"));

    let mut bad = bytes.clone();
    let true_at = bad
        .windows(5)
        .position(|window| window == b",true")
        .expect("true");
    bad.splice(true_at + 1..true_at + 5, b"TRUE".iter().copied());
    let bad_path = scratch.join("bool.csv");
    fs::write(&bad_path, &bad).expect("bad");
    write_json(
        &request_path,
        &request(
            "csv",
            &bad_path,
            &hex(&sha256(&bad)),
            csv_profile(),
            budget_json(false),
            false,
        ),
    );
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        5,
        "parse",
        "not_bool",
    );

    let mut inexact = bytes.clone();
    let amount = inexact
        .windows(4)
        .position(|window| window == b"1.2\n")
        .expect("amount");
    inexact.splice(amount..amount + 3, b"1.251".iter().copied());
    let inexact_path = scratch.join("inexact.csv");
    fs::write(&inexact_path, &inexact).expect("inexact");
    write_json(
        &request_path,
        &request(
            "csv",
            &inexact_path,
            &hex(&sha256(&inexact)),
            csv_profile(),
            budget_json(false),
            false,
        ),
    );
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        5,
        "parse",
        "inexact_decimal",
    );

    let mut zero = bytes.clone();
    let count = zero
        .windows(3)
        .position(|window| window == b",1,")
        .expect("count");
    zero.splice(count..count + 3, b",01,".iter().copied());
    let zero_path = scratch.join("zero.csv");
    fs::write(&zero_path, &zero).expect("zero");
    write_json(
        &request_path,
        &request(
            "csv",
            &zero_path,
            &hex(&sha256(&zero)),
            csv_profile(),
            budget_json(false),
            false,
        ),
    );
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        5,
        "parse",
        "not_int64",
    );

    let mut budget = budget_json(false);
    budget["max_rows"] = json!(1);
    write_json(
        &request_path,
        &request("csv", &input, &digest, csv_profile(), budget, false),
    );
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        6,
        "budget",
        "max_rows",
    );

    let mut refused = csv_profile();
    refused["bom"] = json!("refuse");
    write_json(
        &request_path,
        &request("csv", &input, &digest, refused, budget_json(false), false),
    );
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        5,
        "parse",
        "bom_refused",
    );

    let mut header = csv_profile();
    header["bom"] = json!("refuse");
    header["header"] = json!("exact");
    let empty = scratch.join("empty.csv");
    fs::write(&empty, b"").expect("empty");
    let empty_digest = hex(&sha256(b""));
    write_json(
        &request_path,
        &request(
            "csv",
            &empty,
            &empty_digest,
            header,
            budget_json(false),
            false,
        ),
    );
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        5,
        "parse",
        "header_required",
    );
}

#[test]
fn jsonl_fraction_is_not_int64() {
    let scratch = Scratch::new("jsonl-bad");
    let mut bytes = jsonl_bytes();
    let needle = b"\"count\":1,";
    let at = bytes
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("count");
    bytes.splice(at..at + needle.len(), b"\"count\":1.0,".iter().copied());
    let input = scratch.join("bad.jsonl");
    fs::write(&input, &bytes).expect("input");
    let request_path = scratch.join("request.json");
    write_json(
        &request_path,
        &request(
            "jsonl",
            &input,
            &hex(&sha256(&bytes)),
            jsonl_profile(),
            budget_json(true),
            false,
        ),
    );
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        5,
        "parse",
        "not_int64",
    );
}

#[test]
fn unknown_profile_dialect_and_secret_field_are_config() {
    let scratch = Scratch::new("config");
    let input = scratch.join("input.csv");
    let bytes = csv_bytes();
    fs::write(&input, &bytes).expect("input");
    let digest = hex(&sha256(&bytes));
    let request_path = scratch.join("request.json");
    write_json(
        &request_path,
        &request("parquet", &input, &digest, json!({}), json!({}), false),
    );
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        3,
        "config",
        "format",
    );

    let mut profile = csv_profile();
    profile["columns"][0]["type"] = json!("float");
    write_json(
        &request_path,
        &request("csv", &input, &digest, profile, budget_json(false), false),
    );
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        3,
        "config",
        "type",
    );

    let mut empty_cols = csv_profile();
    empty_cols["columns"] = json!([]);
    write_json(
        &request_path,
        &request(
            "csv",
            &input,
            &digest,
            empty_cols,
            budget_json(false),
            false,
        ),
    );
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        3,
        "config",
        "empty_selection",
    );

    let mut mismatch = csv_profile();
    mismatch["dialect"] = json!("tsv");
    write_json(
        &request_path,
        &request("csv", &input, &digest, mismatch, budget_json(false), false),
    );
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        3,
        "config",
        "dialect",
    );

    let mut comma = csv_profile();
    comma["null"] = json!({"policy": "token", "token": ","});
    write_json(
        &request_path,
        &request("csv", &input, &digest, comma, budget_json(false), false),
    );
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        3,
        "config",
        "null_token",
    );

    let mut version = request(
        "csv",
        &input,
        &digest,
        csv_profile(),
        budget_json(false),
        false,
    );
    version["protocol"] = json!("oo.pipeline.validate-file/v0");
    write_json(&request_path, &version);
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        3,
        "config",
        "version",
    );

    let mut secret = request(
        "csv",
        &input,
        &digest,
        csv_profile(),
        budget_json(false),
        false,
    );
    secret["password"] = json!("secret-not-in-stderr");
    write_json(&request_path, &secret);
    let secret_run = run_validate(&["--request", request_path.to_str().expect("path")]);
    assert_failure(&secret_run, 3, "config", "unknown_field");
    assert!(!secret_run.stderr.contains("secret-not-in-stderr"));

    let mut jsonl = jsonl_profile();
    jsonl["dialect"] = json!("csv");
    let jsonl_input = scratch.join("input.jsonl");
    let jsonl_bytes = jsonl_bytes();
    fs::write(&jsonl_input, &jsonl_bytes).expect("jsonl");
    write_json(
        &request_path,
        &request(
            "jsonl",
            &jsonl_input,
            &hex(&sha256(&jsonl_bytes)),
            jsonl,
            budget_json(true),
            false,
        ),
    );
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        3,
        "config",
        "unknown_field",
    );

    let mut field = budget_json(true);
    field["max_field_bytes"] = json!(4096);
    write_json(
        &request_path,
        &request(
            "jsonl",
            &jsonl_input,
            &hex(&sha256(&jsonl_bytes)),
            jsonl_profile(),
            field,
            false,
        ),
    );
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        3,
        "config",
        "unknown_field",
    );
}

#[test]
fn request_size_is_capped_before_a_success_parse() {
    let scratch = Scratch::new("size");
    let over = scratch.join("over.json");
    fs::write(&over, vec![b'x'; REQUEST_MAX + 1]).expect("over");
    let over_run = run_validate(&["--request", over.to_str().expect("path")]);
    assert_failure(&over_run, 3, "config", "request_bytes");
    assert!(over_run.stderr.len() < 400, "{}", over_run.stderr.len());

    let exact = scratch.join("exact.json");
    fs::write(&exact, vec![b'{'; REQUEST_MAX]).expect("exact");
    assert_failure(
        &run_validate(&["--request", exact.to_str().expect("path")]),
        3,
        "config",
        "request_json",
    );

    let dup = scratch.join("dup.json");
    fs::write(
        dup.as_path(),
        br#"{"protocol":"oo.pipeline.validate-file/v1","protocol":"other"}"#,
    )
    .expect("dup");
    assert_failure(
        &run_validate(&["--request", dup.to_str().expect("path")]),
        3,
        "config",
        "request_json",
    );

    let upper = scratch.join("upper.json");
    let input = scratch.join("input.csv");
    fs::write(&input, csv_bytes()).expect("input");
    let mut sha = hex(&sha256(&fs::read(&input).expect("read")));
    sha.replace_range(0..1, "A");
    write_json(
        &upper,
        &request(
            "csv",
            &input,
            &sha,
            csv_profile(),
            budget_json(false),
            false,
        ),
    );
    assert_failure(
        &run_validate(&["--request", upper.to_str().expect("path")]),
        3,
        "config",
        "expected_sha256",
    );
}

#[test]
fn cancel_and_read_errors_are_distinct() {
    let scratch = Scratch::new("read");
    let input = scratch.join("input.csv");
    let bytes = csv_bytes();
    fs::write(&input, &bytes).expect("input");
    let digest = hex(&sha256(&bytes));
    let request_path = scratch.join("request.json");
    write_json(
        &request_path,
        &request(
            "csv",
            &input,
            &digest,
            csv_profile(),
            budget_json(false),
            true,
        ),
    );
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        8,
        "cancel",
        "cancelled",
    );

    let missing = scratch.join("missing-input-zz.csv");
    write_json(
        &request_path,
        &request(
            "csv",
            &missing,
            &digest,
            csv_profile(),
            budget_json(false),
            false,
        ),
    );
    let missing_run = run_validate(&["--request", request_path.to_str().expect("path")]);
    assert_failure(&missing_run, 4, "read", "open");
    assert!(!missing_run.stderr.contains("missing-input-zz"));

    write_json(
        &request_path,
        &request(
            "csv",
            &missing,
            &digest,
            csv_profile(),
            budget_json(false),
            true,
        ),
    );
    let hidden = run_validate(&["--request", request_path.to_str().expect("path")]);
    assert_failure(&hidden, 4, "read", "open");
    assert!(!hidden.stderr.contains("missing-input-zz"));

    write_json(
        &request_path,
        &request(
            "csv",
            &scratch.0,
            &digest,
            csv_profile(),
            budget_json(false),
            false,
        ),
    );
    assert_failure(
        &run_validate(&["--request", request_path.to_str().expect("path")]),
        4,
        "read",
        "not_file",
    );
}

#[test]
fn help_and_unknown_flags() {
    let help = run_validate(&["--help"]);
    assert_eq!(help.code, 0, "{}", help.stderr);
    assert!(help.stderr.is_empty(), "{}", help.stderr);
    assert!(help.stdout.contains("validate-file"));
    assert!(help.stdout.contains(REQUEST_PROTOCOL));
    assert!(help.stdout.contains("CSV"));
    assert!(help.stdout.contains("TSV"));
    assert!(help.stdout.contains("JSONL"));
    assert!(help.stdout.contains("does not infer a dialect"));
    assert!(help.stdout.contains("does not import"));
    assert!(help.stdout.contains("blocking read"));
    assert!(help.stdout.contains("1048576"));

    assert_failure(&run_validate(&[]), 2, "usage", "usage");
    assert_failure(&run_validate(&["--request"]), 2, "usage", "usage");
    assert_failure(&run_validate(&["--nope"]), 2, "usage", "unknown_flag");
    assert_failure(&run_validate(&["-h"]), 2, "usage", "unknown_flag");
    assert_failure(
        &run_validate(&["--help", "--request"]),
        2,
        "usage",
        "unknown_flag",
    );
    let scratch = Scratch::new("flag");
    let request = scratch.join("request.json");
    fs::write(&request, b"{}\n").expect("request");
    let path = request.to_str().expect("path");
    assert_failure(
        &run_validate(&["--request", path, "--extra"]),
        2,
        "usage",
        "unknown_flag",
    );
}

#[test]
fn import_command_usage_and_empty_stdin_stay_failed() {
    let usage = run_pipeline(&[], None);
    assert_eq!(usage.code, 2, "{}", usage.stderr);
    assert!(usage.stdout.is_empty(), "{}", usage.stdout);
    assert!(usage.stderr.contains("usage"), "{}", usage.stderr);

    let extra = run_pipeline(&["import-pg", "--help"], None);
    assert_eq!(extra.code, 2, "{}", extra.stderr);
    assert!(extra.stdout.is_empty(), "{}", extra.stdout);
    assert!(extra.stderr.contains("usage"), "{}", extra.stderr);

    let catalog = run_pipeline(&["catalog-pg", "extra"], None);
    assert_eq!(catalog.code, 2, "{}", catalog.stderr);
    assert!(catalog.stdout.is_empty());

    let empty = run_pipeline(&["import-pg"], Some(b""));
    assert_eq!(empty.code, 1, "{}", empty.stderr);
    assert!(empty.stdout.is_empty(), "{}", empty.stdout);
    assert!(empty.stderr.contains("invalid input"), "{}", empty.stderr);
    assert!(!empty.stdout.contains("awaiting_publish"));
    assert!(!empty.stderr.contains("awaiting_publish"));
}

type ObservedRow = (u64, u64, Vec<(String, String)>);

fn row(line: u64, offset: u64, names: &[&str], presence: &[&str]) -> ObservedRow {
    assert_eq!(names.len(), presence.len());
    (
        line,
        offset,
        names
            .iter()
            .zip(presence)
            .map(|(name, kind)| ((*name).to_owned(), (*kind).to_owned()))
            .collect(),
    )
}

fn pull_delimited(path: &Path, profile: DelimitedProfile, expected: [u8; 32]) -> Vec<ObservedRow> {
    let file = File::open(path).expect("open");
    let mut reader =
        DelimitedReader::open(file, profile, expected, delimited_budget()).expect("open reader");
    assert!(!reader.header().admits_import);
    assert!(!reader.header().published);
    assert!(!reader.header().arrow_ipc);
    assert!(!reader.header().executes_native);
    let mut rows = Vec::new();
    loop {
        match reader.next_batch().expect("pull") {
            DelimitedItem::Batch(batch) => {
                assert!(!batch.resumable);
                assert!(!batch.header.admits_import);
                for line in batch.lineage {
                    rows.push((
                        line.line,
                        line.byte_offset,
                        line.fields
                            .into_iter()
                            .map(|field| {
                                let kind = match field.presence {
                                    DelimitedFieldPresence::Value => "value",
                                    DelimitedFieldPresence::Null => "null",
                                    DelimitedFieldPresence::Empty => "empty",
                                };
                                (field.name, kind.to_owned())
                            })
                            .collect(),
                    ));
                }
            }
            DelimitedItem::Exhausted(_) => {
                let complete = reader.finish().expect("finish");
                assert!(!complete.admits_import);
                assert!(!complete.published);
                assert_eq!(complete.rows, rows.len() as u64);
                assert_eq!(complete.object_sha256, expected);
                return rows;
            }
        }
    }
}

fn pull_jsonl(path: &Path, profile: JsonlProfile, expected: [u8; 32]) -> Vec<ObservedRow> {
    let file = File::open(path).expect("open");
    let mut reader =
        JsonlReader::open(file, profile, expected, jsonl_budget_reader()).expect("open reader");
    assert!(!reader.header().admits_import);
    assert!(!reader.header().executes_native);
    let mut rows = Vec::new();
    loop {
        match reader.next_batch().expect("pull") {
            JsonlItem::Batch(batch) => {
                assert!(!batch.resumable);
                for line in batch.lineage {
                    rows.push((
                        line.line,
                        line.byte_offset,
                        line.fields
                            .into_iter()
                            .map(|field| {
                                let kind = match field.presence {
                                    FieldPresence::Value => "value",
                                    FieldPresence::Null => "null",
                                    FieldPresence::Missing => "missing",
                                };
                                (field.name, kind.to_owned())
                            })
                            .collect(),
                    ));
                }
            }
            JsonlItem::Exhausted(_) => {
                let complete = reader.finish().expect("finish");
                assert!(!complete.admits_import);
                assert!(!complete.published);
                assert_eq!(complete.rows, rows.len() as u64);
                assert_eq!(complete.object_sha256, expected);
                return rows;
            }
        }
    }
}
