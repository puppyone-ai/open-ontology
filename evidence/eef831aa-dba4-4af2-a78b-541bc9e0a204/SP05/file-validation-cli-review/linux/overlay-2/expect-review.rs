//! Independent review of `validate-file` at `59be14ba`.
//!
//! The harness spawns the compiled binary, pulls the same bytes through the
//! public reader, and calls `finish`. Counts come from that receipt and from
//! the batches the pull emitted. They are not a profile-builder tally.
//! Digests are this file's SHA-256 of the fixture bytes and of the documented
//! preimage. Arrow `Decimal128Array` cells are not downcast: `arrow-array` is
//! a private dependency of `oo-connectors`, and this review does not edit a
//! product manifest. Coefficient spellings use `oo_core::ExactDecimal`.
//! The a199 review already inspected Arrow coefficients on the public reader.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, File};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use oo_connectors::{
    BomPolicy, DelimitedBudget, DelimitedColumn, DelimitedDialect, DelimitedError,
    DelimitedFieldPresence, DelimitedHeader, DelimitedItem, DelimitedProfile, DelimitedReader,
    DelimitedType, FieldPresence, HeaderPolicy, JsonlBudget, JsonlColumn, JsonlError, JsonlHeader,
    JsonlItem, JsonlProfile, JsonlReader, JsonlType, NullPolicy, DELIMITED_LINEAGE_VERSION,
    DELIMITED_PROFILE_VERSION, DELIMITED_PROTOCOL, DELIMITED_RECEIPT_VERSION, JSONL_LINEAGE_VERSION,
    JSONL_PROFILE_VERSION, JSONL_PROTOCOL, JSONL_RECEIPT_VERSION,
};
use oo_core::{DecimalError, ExactDecimal};
use serde_json::{json, Value};

const REQUEST_PROTOCOL: &str = "oo.pipeline.validate-file/v1";
const RESULT_PROTOCOL: &str = "oo.pipeline.validate-file-result/v1";
const ERROR_PROTOCOL: &str = "oo.pipeline.validate-file-error/v1";
const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const REQUEST_MAX: usize = 1_048_576;

const CELL_ABSENT: &[&str] = &[
    "熊猫",
    "甲",
    "1.20",
    "1.2",
    "-0.00",
    "12.50",
    "1.251",
    "-9223372036854775808",
    "9223372036854775807",
    "{\"n\":1}",
    "SECRET_a204",
    "PATHTOKEN_a204",
];

#[derive(Clone, Debug, PartialEq, Eq)]
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
}

impl Fold {
    const fn zero() -> Self {
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
        }
    }

    fn note_row(&mut self, line: u64, offset: u64) {
        if self.rows == 0 {
            self.first_line = line;
            self.first_offset = offset;
        }
        self.rows += 1;
        self.last_line = line;
        self.last_offset = offset;
    }
}

struct Hand {
    file_len: usize,
    rows: u64,
    batches: u64,
    batch_sizes: &'static [u64],
    fields: u64,
    value: u64,
    null: u64,
    empty: u64,
    missing: u64,
    first_line: u64,
    last_line: u64,
    first_offset: u64,
    last_offset: u64,
    columns: u32,
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn status_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(127)
}

fn validate_bin() -> PathBuf {
    env::var_os("CARGO_BIN_EXE_validate_file")
        .or_else(|| env::var_os("CARGO_BIN_EXE_validate-file"))
        .map(PathBuf::from)
        .expect("CARGO_BIN_EXE_validate_file")
}

fn run_validate(args: &[&str]) -> Run {
    let output = Command::new(validate_bin())
        .args(args)
        .stdin(Stdio::null())
        .env_remove("OO_A197_ORACLE_DIR")
        .output()
        .expect("spawn validate-file");
    Run {
        code: status_code(output.status),
        stdout: String::from_utf8(output.stdout).expect("stdout utf8"),
        stderr: String::from_utf8(output.stderr).expect("stderr utf8"),
    }
}

fn case_dir(name: &str) -> PathBuf {
    let dir = env::temp_dir().join("a204-review").join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("case dir");
    dir
}

fn write_bytes(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).expect("write bytes");
}

fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).expect("request json")).expect("write request");
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(*byte & 0x0f)] as char);
    }
    out
}

fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
        0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
        0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
        0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
        0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
        0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut state = [
        0x6a09_e667_u32,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let bit_len = u64::try_from(input.len())
        .expect("len")
        .checked_mul(8)
        .expect("bits");
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut word = [0u32; 64];
        for (index, slot) in word.iter_mut().enumerate().take(16) {
            let start = index * 4;
            *slot = u32::from_be_bytes(chunk[start..start + 4].try_into().expect("word"));
        }
        for index in 16..64 {
            let earlier = word[index - 15];
            let later = word[index - 2];
            let s0 = earlier.rotate_right(7) ^ earlier.rotate_right(18) ^ (earlier >> 3);
            let s1 = later.rotate_right(17) ^ later.rotate_right(19) ^ (later >> 10);
            word[index] = word[index - 16]
                .wrapping_add(s0)
                .wrapping_add(word[index - 7])
                .wrapping_add(s1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(word[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut out = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}

fn save_oracle(name: &str, bytes: &[u8], preimage: &str, stdout: &str) {
    let Ok(dir) = env::var("OO_A204_ORACLE_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir);
    fs::create_dir_all(&dir).expect("oracle dir");
    fs::write(dir.join(format!("{name}.bin")), bytes).expect("oracle bytes");
    fs::write(dir.join(format!("{name}.preimage.txt")), preimage).expect("oracle preimage");
    fs::write(dir.join(format!("{name}.sha256")), format!("{}\n", hex(&sha256(bytes)))).expect("oracle sha");
    fs::write(dir.join(format!("{name}.stdout.json")), stdout).expect("oracle stdout");
}

fn flip_last_nibble(text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    let last = chars.len() - 1;
    chars[last] = if chars[last] == '0' { '1' } else { '0' };
    chars.into_iter().collect()
}

fn coefficient(text: &str, scale: i32) -> (bool, String, i32) {
    let parsed = ExactDecimal::parse_sql(text).expect("parse sql decimal");
    let scaled = parsed.rescale_exact(scale).expect("rescale exact");
    (scaled.is_negative(), scaled.coefficient().to_owned(), scaled.scale())
}

fn delimited_budget(max_rows: u64) -> DelimitedBudget {
    DelimitedBudget::try_new(
        2,
        1_048_576,
        1_048_576,
        1_048_576,
        16_777_216,
        max_rows,
        16,
        16,
        Instant::now() + Duration::from_millis(120_000),
    )
    .expect("delimited budget")
}

fn jsonl_budget(max_rows: u64) -> JsonlBudget {
    JsonlBudget::try_new(
        2,
        1_048_576,
        1_048_576,
        16_777_216,
        max_rows,
        16,
        16,
        Instant::now() + Duration::from_millis(120_000),
    )
    .expect("jsonl budget")
}

fn budget_value(max_rows: u64, jsonl: bool) -> Value {
    let mut value = json!({
        "batch_bytes": 1_048_576,
        "batch_rows": 2,
        "deadline_ms": 120_000,
        "max_columns": 16,
        "max_fields": 16,
        "max_record_bytes": 1_048_576,
        "max_rows": max_rows,
        "max_total_bytes": 16_777_216,
    });
    if !jsonl {
        value["max_field_bytes"] = json!(1_048_576);
    }
    value
}

fn csv_bytes() -> Vec<u8> {
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(b"label,ok,n,amount\n");
    bytes.extend_from_slice("熊猫,true,-9223372036854775808,1.20\n".as_bytes());
    bytes.extend_from_slice(b"\"\",false,9223372036854775807,-0.00\n");
    bytes.extend_from_slice("\"甲,乙\",true,10,1.2\n".as_bytes());
    bytes.extend_from_slice(b",true,0,10\n");
    assert_eq!(bytes.len(), 127);
    bytes
}

fn tsv_bytes() -> Vec<u8> {
    let text = "熊猫\ttrue\t1\t12.50\n\\N\tfalse\t-0\t-0.00\n\"\\N\"\ttrue\t9223372036854775807\t10\n";
    assert_eq!(text.len(), 71);
    text.as_bytes().to_vec()
}

fn jsonl_bytes() -> Vec<u8> {
    let text = concat!(
        "{\"label\":\"熊猫\",\"ok\":true,\"n\":-9223372036854775808,\"payload\":{\"n\":1}}\n",
        "{\"label\":\"\",\"ok\":false,\"n\":9223372036854775807,\"payload\":null}\n",
        "{\"ok\":true,\"n\":-0,\"payload\":\"甲\"}\n",
        "{\"label\":\"甲\",\"ok\":false,\"n\":10}\n",
    );
    assert_eq!(text.len(), 204);
    text.as_bytes().to_vec()
}

fn csv_preimage() -> String {
    let mut body = String::from("oo-delimited-profile/v1\n");
    body.push_str("id=a204-csv\n");
    body.push_str("dialect=csv\n");
    body.push_str("bom=strip\n");
    body.push_str("header=exact\n");
    body.push_str("null=empty\n");
    body.push_str("quote=double\n");
    body.push_str("record=lf-or-crlf\n");
    body.push_str("escape=double-quote\n");
    body.push_str("column=label\tutf8\t1\t-\t-\n");
    body.push_str("column=ok\tbool\t1\t-\t-\n");
    body.push_str("column=n\tint64\t1\t-\t-\n");
    body.push_str("column=amount\tdecimal\t1\t10\t2\n");
    body
}

fn tsv_preimage() -> String {
    let mut body = String::from("oo-delimited-profile/v1\n");
    body.push_str("id=a204-tsv\n");
    body.push_str("dialect=tsv\n");
    body.push_str("bom=refuse\n");
    body.push_str("header=absent\n");
    body.push_str("null=token\n");
    body.push_str("null_token=\\N\n");
    body.push_str("quote=double\n");
    body.push_str("record=lf-or-crlf\n");
    body.push_str("escape=double-quote\n");
    body.push_str("column=note\tutf8\t1\t-\t-\n");
    body.push_str("column=ok\tbool\t1\t-\t-\n");
    body.push_str("column=n\tint64\t1\t-\t-\n");
    body.push_str("column=amount\tdecimal\t1\t12\t2\n");
    body
}

fn jsonl_preimage() -> String {
    let mut body = String::from("oo-jsonl-profile/v1\n");
    body.push_str("id=a204-jsonl\n");
    body.push_str("column=label\tutf8\t1\n");
    body.push_str("column=ok\tbool\t1\n");
    body.push_str("column=n\tint64\t1\n");
    body.push_str("column=payload\tjson_text\t1\n");
    body
}

fn empty_preimage() -> String {
    let mut body = String::from("oo-delimited-profile/v1\n");
    body.push_str("id=a204-empty\n");
    body.push_str("dialect=csv\n");
    body.push_str("bom=refuse\n");
    body.push_str("header=absent\n");
    body.push_str("null=empty\n");
    body.push_str("quote=double\n");
    body.push_str("record=lf-or-crlf\n");
    body.push_str("escape=double-quote\n");
    body.push_str("column=label\tutf8\t1\t-\t-\n");
    body
}

fn config_preimage(prefix: &str, profile: &str, object_hex: &str) -> String {
    let mut body = String::from(prefix);
    body.push_str(profile);
    body.push_str("expected_sha256=");
    body.push_str(object_hex);
    body.push('\n');
    body
}

fn csv_profile() -> DelimitedProfile {
    let columns = vec![
        DelimitedColumn::try_new("label", DelimitedType::Utf8, true).expect("label"),
        DelimitedColumn::try_new("ok", DelimitedType::Bool, true).expect("ok"),
        DelimitedColumn::try_new("n", DelimitedType::Int64, true).expect("n"),
        DelimitedColumn::try_new(
            "amount",
            DelimitedType::Decimal {
                precision: 10,
                scale: 2,
            },
            true,
        )
        .expect("amount"),
    ];
    DelimitedProfile::try_new(
        "a204-csv",
        DelimitedDialect::try_from_separator(b',').expect("comma"),
        BomPolicy::Strip,
        HeaderPolicy::Exact,
        NullPolicy::Empty,
        columns,
    )
    .expect("csv profile")
}

fn csv_shaped(id: &str, bom: BomPolicy, header: HeaderPolicy) -> DelimitedProfile {
    let columns = vec![
        DelimitedColumn::try_new("label", DelimitedType::Utf8, true).expect("label"),
        DelimitedColumn::try_new("ok", DelimitedType::Bool, true).expect("ok"),
        DelimitedColumn::try_new("n", DelimitedType::Int64, true).expect("n"),
        DelimitedColumn::try_new(
            "amount",
            DelimitedType::Decimal {
                precision: 10,
                scale: 2,
            },
            true,
        )
        .expect("amount"),
    ];
    DelimitedProfile::try_new(
        id,
        DelimitedDialect::try_from_separator(b',').expect("comma"),
        bom,
        header,
        NullPolicy::Empty,
        columns,
    )
    .expect("shaped profile")
}

fn tsv_profile() -> DelimitedProfile {
    let columns = vec![
        DelimitedColumn::try_new("note", DelimitedType::Utf8, true).expect("note"),
        DelimitedColumn::try_new("ok", DelimitedType::Bool, true).expect("ok"),
        DelimitedColumn::try_new("n", DelimitedType::Int64, true).expect("n"),
        DelimitedColumn::try_new(
            "amount",
            DelimitedType::Decimal {
                precision: 12,
                scale: 2,
            },
            true,
        )
        .expect("amount"),
    ];
    DelimitedProfile::try_new(
        "a204-tsv",
        DelimitedDialect::try_from_separator(b'\t').expect("tab"),
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Token("\\N".to_owned()),
        columns,
    )
    .expect("tsv profile")
}

fn jsonl_profile() -> JsonlProfile {
    let columns = vec![
        JsonlColumn::try_new("label", JsonlType::Utf8, true).expect("label"),
        JsonlColumn::try_new("ok", JsonlType::Bool, true).expect("ok"),
        JsonlColumn::try_new("n", JsonlType::Int64, true).expect("n"),
        JsonlColumn::try_new("payload", JsonlType::JsonText, true).expect("payload"),
    ];
    JsonlProfile::try_new("a204-jsonl", columns).expect("jsonl profile")
}

fn empty_profile() -> DelimitedProfile {
    let columns = vec![DelimitedColumn::try_new("label", DelimitedType::Utf8, true).expect("label")];
    DelimitedProfile::try_new(
        "a204-empty",
        DelimitedDialect::try_from_separator(b',').expect("comma"),
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        columns,
    )
    .expect("empty profile")
}

fn csv_profile_value() -> Value {
    json!({
        "bom": "strip",
        "columns": [
            {"name": "label", "nullable": true, "type": "utf8"},
            {"name": "ok", "nullable": true, "type": "bool"},
            {"name": "n", "nullable": true, "type": "int64"},
            {"name": "amount", "nullable": true, "precision": 10, "scale": 2, "type": "decimal"}
        ],
        "dialect": "csv",
        "header": "exact",
        "id": "a204-csv",
        "null": {"policy": "empty"}
    })
}

fn tsv_profile_value() -> Value {
    json!({
        "bom": "refuse",
        "columns": [
            {"name": "note", "nullable": true, "type": "utf8"},
            {"name": "ok", "nullable": true, "type": "bool"},
            {"name": "n", "nullable": true, "type": "int64"},
            {"name": "amount", "nullable": true, "precision": 12, "scale": 2, "type": "decimal"}
        ],
        "dialect": "tsv",
        "header": "absent",
        "id": "a204-tsv",
        "null": {"policy": "token", "token": "\\N"}
    })
}

fn jsonl_profile_value() -> Value {
    json!({
        "columns": [
            {"name": "label", "nullable": true, "type": "utf8"},
            {"name": "ok", "nullable": true, "type": "bool"},
            {"name": "n", "nullable": true, "type": "int64"},
            {"name": "payload", "nullable": true, "type": "json_text"}
        ],
        "id": "a204-jsonl"
    })
}

fn empty_profile_value() -> Value {
    json!({
        "bom": "refuse",
        "columns": [{"name": "label", "nullable": true, "type": "utf8"}],
        "dialect": "csv",
        "header": "absent",
        "id": "a204-empty",
        "null": {"policy": "empty"}
    })
}

fn csv_types() -> Value {
    json!([
        {"name": "label", "nullable": true, "type": "utf8"},
        {"name": "ok", "nullable": true, "type": "bool"},
        {"name": "n", "nullable": true, "type": "int64"},
        {"name": "amount", "nullable": true, "precision": 10, "scale": 2, "type": "decimal"}
    ])
}

fn tsv_types() -> Value {
    json!([
        {"name": "note", "nullable": true, "type": "utf8"},
        {"name": "ok", "nullable": true, "type": "bool"},
        {"name": "n", "nullable": true, "type": "int64"},
        {"name": "amount", "nullable": true, "precision": 12, "scale": 2, "type": "decimal"}
    ])
}

fn jsonl_types() -> Value {
    json!([
        {"name": "label", "nullable": true, "type": "utf8"},
        {"name": "ok", "nullable": true, "type": "bool"},
        {"name": "n", "nullable": true, "type": "int64"},
        {"name": "payload", "nullable": true, "type": "json_text"}
    ])
}

fn empty_types() -> Value {
    json!([{"name": "label", "nullable": true, "type": "utf8"}])
}

#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
struct DelimitedSeen {
    rows: u64,
    batches: u64,
    batch_sizes: Vec<u64>,
    columns: u32,
    object_sha256: [u8; 32],
    profile_digest: [u8; 32],
    config_identity: [u8; 32],
    profile_id: String,
    admits_import: bool,
    published: bool,
    arrow_ipc: bool,
    executes_native: bool,
    fold: Fold,
}

#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
struct JsonlSeen {
    rows: u64,
    batches: u64,
    batch_sizes: Vec<u64>,
    columns: u32,
    object_sha256: [u8; 32],
    profile_digest: [u8; 32],
    config_identity: [u8; 32],
    profile_id: String,
    admits_import: bool,
    published: bool,
    arrow_ipc: bool,
    executes_native: bool,
    fold: Fold,
}

fn fold_delimited(fold: &mut Fold, row: &oo_connectors::DelimitedRowLineage) {
    fold.note_row(row.line, row.byte_offset);
    for field in &row.fields {
        fold.fields += 1;
        match field.presence {
            DelimitedFieldPresence::Value => fold.value += 1,
            DelimitedFieldPresence::Null => fold.null += 1,
            DelimitedFieldPresence::Empty => fold.empty += 1,
        }
    }
}

fn fold_jsonl(fold: &mut Fold, row: &oo_connectors::RowLineage) {
    fold.note_row(row.line, row.byte_offset);
    for field in &row.fields {
        fold.fields += 1;
        match field.presence {
            FieldPresence::Value => fold.value += 1,
            FieldPresence::Null => fold.null += 1,
            FieldPresence::Missing => fold.missing += 1,
        }
    }
}

fn pull_delimited(
    path: &Path,
    profile: DelimitedProfile,
    expected: [u8; 32],
    max_rows: u64,
    cancel: bool,
) -> Result<DelimitedSeen, DelimitedError> {
    let file = File::open(path).expect("open fixture");
    let mut reader = DelimitedReader::open(file, profile, expected, delimited_budget(max_rows))?;
    let header: DelimitedHeader = reader.header();
    if cancel {
        reader.cancel_handle().request();
    }
    let mut fold = Fold::zero();
    let mut batch_sizes = Vec::new();
    loop {
        match reader.next_batch() {
            Ok(DelimitedItem::Batch(batch)) => {
                assert!(!batch.resumable);
                assert!(!batch.header.admits_import);
                assert!(!batch.header.published);
                assert!(!batch.header.arrow_ipc);
                assert!(!batch.header.executes_native);
                let rows = u64::try_from(batch.lineage.len()).expect("batch rows");
                batch_sizes.push(rows);
                for row in &batch.lineage {
                    fold_delimited(&mut fold, row);
                }
            }
            Ok(DelimitedItem::Exhausted(done)) => {
                let complete = reader.finish()?;
                assert_eq!(complete.rows, done.rows);
                assert_eq!(complete.batches, done.batches);
                assert_eq!(complete.batches, u64::try_from(batch_sizes.len()).expect("batches"));
                assert_eq!(complete.rows, fold.rows);
                assert_eq!(complete.object_sha256, done.object_sha256);
                assert_eq!(complete.profile_id, header.profile_id);
                assert_eq!(complete.profile_digest, header.profile_digest);
                assert_eq!(complete.config_identity, header.config_identity);
                assert_eq!(complete.protocol, header.protocol);
                assert!(!complete.admits_import);
                assert!(!complete.published);
                return Ok(DelimitedSeen {
                    rows: complete.rows,
                    batches: complete.batches,
                    batch_sizes,
                    columns: complete.columns,
                    object_sha256: complete.object_sha256,
                    profile_digest: complete.profile_digest,
                    config_identity: complete.config_identity,
                    profile_id: complete.profile_id,
                    admits_import: complete.admits_import,
                    published: complete.published,
                    arrow_ipc: header.arrow_ipc,
                    executes_native: header.executes_native,
                    fold,
                });
            }
            Err(err) => {
                match reader.finish() {
                    Ok(_) => panic!("finish minted a receipt after {}", err.fact()),
                    Err(finish_err) => assert_eq!(finish_err.fact(), err.fact()),
                }
                return Err(err);
            }
        }
    }
}

fn pull_jsonl(
    path: &Path,
    profile: JsonlProfile,
    expected: [u8; 32],
    max_rows: u64,
    cancel: bool,
) -> Result<JsonlSeen, JsonlError> {
    let file = File::open(path).expect("open fixture");
    let mut reader = JsonlReader::open(file, profile, expected, jsonl_budget(max_rows))?;
    let header: JsonlHeader = reader.header();
    if cancel {
        reader.cancel_handle().request();
    }
    let mut fold = Fold::zero();
    let mut batch_sizes = Vec::new();
    loop {
        match reader.next_batch() {
            Ok(JsonlItem::Batch(batch)) => {
                assert!(!batch.resumable);
                assert!(!batch.header.admits_import);
                assert!(!batch.header.published);
                assert!(!batch.header.arrow_ipc);
                assert!(!batch.header.executes_native);
                let rows = u64::try_from(batch.lineage.len()).expect("batch rows");
                batch_sizes.push(rows);
                for row in &batch.lineage {
                    fold_jsonl(&mut fold, row);
                }
            }
            Ok(JsonlItem::Exhausted(done)) => {
                let complete = reader.finish()?;
                assert_eq!(complete.rows, done.rows);
                assert_eq!(complete.batches, done.batches);
                assert_eq!(complete.batches, u64::try_from(batch_sizes.len()).expect("batches"));
                assert_eq!(complete.rows, fold.rows);
                assert_eq!(complete.object_sha256, done.object_sha256);
                assert_eq!(complete.profile_id, header.profile_id);
                assert_eq!(complete.profile_digest, header.profile_digest);
                assert_eq!(complete.config_identity, header.config_identity);
                assert_eq!(complete.protocol, header.protocol);
                assert!(!complete.admits_import);
                assert!(!complete.published);
                return Ok(JsonlSeen {
                    rows: complete.rows,
                    batches: complete.batches,
                    batch_sizes,
                    columns: complete.columns,
                    object_sha256: complete.object_sha256,
                    profile_digest: complete.profile_digest,
                    config_identity: complete.config_identity,
                    profile_id: complete.profile_id,
                    admits_import: complete.admits_import,
                    published: complete.published,
                    arrow_ipc: header.arrow_ipc,
                    executes_native: header.executes_native,
                    fold,
                });
            }
            Err(err) => {
                match reader.finish() {
                    Ok(_) => panic!("finish minted a receipt after {}", err.fact()),
                    Err(finish_err) => assert_eq!(finish_err.fact(), err.fact()),
                }
                return Err(err);
            }
        }
    }
}

fn request_value(
    format: &str,
    input: &Path,
    object_hex: &str,
    profile: Value,
    max_rows: u64,
    jsonl: bool,
    cancel: bool,
) -> Value {
    json!({
        "budget": budget_value(max_rows, jsonl),
        "cancel_before_read": cancel,
        "expected_sha256": object_hex,
        "format": format,
        "input_path": input.to_str().expect("utf8 path"),
        "profile": profile,
        "protocol": REQUEST_PROTOCOL,
    })
}

fn assert_fold(fold: &Fold, hand: &Hand) {
    assert_eq!(fold.rows, hand.rows);
    assert_eq!(fold.fields, hand.fields);
    assert_eq!(fold.value, hand.value);
    assert_eq!(fold.null, hand.null);
    assert_eq!(fold.empty, hand.empty);
    assert_eq!(fold.missing, hand.missing);
    assert_eq!(fold.first_line, hand.first_line);
    assert_eq!(fold.last_line, hand.last_line);
    assert_eq!(fold.first_offset, hand.first_offset);
    assert_eq!(fold.last_offset, hand.last_offset);
}

fn assert_closed_text(stdout: &str, stderr: &str, absent: &[&str]) {
    for token in absent {
        assert!(!stdout.contains(token), "stdout contains {token}");
        assert!(!stderr.contains(token), "stderr contains {token}");
    }
}

struct SuccessExpect<'a> {
    source_protocol: &'a str,
    profile_version: &'a str,
    lineage_version: &'a str,
    receipt_version: &'a str,
    profile_id: &'a str,
    profile_digest: &'a [u8; 32],
    config_identity: &'a [u8; 32],
    object_sha256: &'a [u8; 32],
    rows: u64,
    columns: u32,
    batches: u64,
    fold: &'a Fold,
    types: &'a Value,
    input: &'a Path,
}

fn assert_success(run: &Run, expect: &SuccessExpect<'_>) {
    assert_eq!(run.code, 0, "stderr={}", run.stderr);
    assert!(run.stderr.is_empty(), "stderr={}", run.stderr);
    assert!(run.stdout.ends_with('\n'));
    assert_eq!(run.stdout.chars().filter(|ch| *ch == '\n').count(), 1);
    let value: Value = serde_json::from_str(run.stdout.trim_end()).expect("success json");
    let keys: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
    assert_eq!(
        keys,
        vec![
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
        ]
    );
    assert_eq!(value["protocol"], RESULT_PROTOCOL);
    assert_eq!(value["source_protocol"], expect.source_protocol);
    assert_eq!(value["profile_version"], expect.profile_version);
    assert_eq!(value["lineage_version"], expect.lineage_version);
    assert_eq!(value["receipt_version"], expect.receipt_version);
    assert_eq!(value["profile_id"], expect.profile_id);
    assert_eq!(value["profile_digest"], hex(expect.profile_digest));
    assert_eq!(value["config_identity"], hex(expect.config_identity));
    assert_eq!(value["object_sha256"], hex(expect.object_sha256));
    assert_eq!(value["rows"], expect.rows);
    assert_eq!(value["columns"], expect.columns);
    assert_eq!(value["batches"], expect.batches);
    assert_eq!(value["admits_import"], false);
    assert_eq!(value["published"], false);
    assert_eq!(value["arrow_ipc"], false);
    assert_eq!(value["executes_native"], false);
    assert_eq!(value["types"], *expect.types);
    let lineage = &value["lineage"];
    let lineage_keys: Vec<String> = lineage.as_object().expect("lineage").keys().cloned().collect();
    assert_eq!(
        lineage_keys,
        vec![
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
        ]
    );
    let fold = expect.fold;
    assert_eq!(lineage["rows"], fold.rows);
    assert_eq!(lineage["fields"], fold.fields);
    assert_eq!(lineage["value"], fold.value);
    assert_eq!(lineage["null"], fold.null);
    assert_eq!(lineage["empty"], fold.empty);
    assert_eq!(lineage["missing"], fold.missing);
    assert_eq!(lineage["first_line"], fold.first_line);
    assert_eq!(lineage["last_line"], fold.last_line);
    assert_eq!(lineage["first_byte_offset"], fold.first_offset);
    assert_eq!(lineage["last_byte_offset"], fold.last_offset);
    let rendered = serde_json::to_string(&value).expect("render") + "\n";
    assert_eq!(run.stdout, rendered);
    let mut absent = CELL_ABSENT.to_vec();
    absent.push(expect.input.to_str().expect("path"));
    assert_closed_text(&run.stdout, &run.stderr, &absent);
    assert!(!run.stdout.contains("input_path"));
    assert!(!run.stdout.contains("password"));
}

fn assert_failure(run: &Run, code: i32, outcome: &str, fact: &str, extra_absent: &[&str]) {
    assert_eq!(run.code, code, "stdout={} stderr={}", run.stdout, run.stderr);
    assert!(run.stdout.is_empty(), "stdout={}", run.stdout);
    assert!(!run.stdout.contains(RESULT_PROTOCOL));
    assert!(run.stderr.ends_with('\n'));
    assert_eq!(run.stderr.chars().filter(|ch| *ch == '\n').count(), 1);
    let value: Value = serde_json::from_str(run.stderr.trim_end()).expect("error json");
    assert_eq!(
        value.as_object().expect("object").keys().cloned().collect::<Vec<_>>(),
        vec!["fact", "outcome", "protocol"]
    );
    assert_eq!(value["protocol"], ERROR_PROTOCOL);
    assert_eq!(value["outcome"], outcome);
    assert_eq!(value["fact"], fact);
    let mut absent = CELL_ABSENT.to_vec();
    absent.extend_from_slice(extra_absent);
    assert_closed_text(&run.stdout, &run.stderr, &absent);
    assert!(run.stderr.len() < 256, "stderr too long: {}", run.stderr.len());
}

fn run_request(dir: &Path, body: &Value) -> Run {
    let request = dir.join("request.json");
    write_json(&request, body);
    run_validate(&["--request", request.to_str().expect("request path")])
}

#[test]
fn sha256_self_check() {
    assert_eq!(hex(&sha256(b"")), EMPTY_SHA);
    assert_eq!(hex(&sha256(b"abc")), ABC_SHA);
}

#[test]
fn decimal_api_coefficients_match_exact_spellings() {
    let cases = [("1.20", false, "120"), ("-0.00", false, "0"), ("1.2", false, "120"), ("10", false, "1000"), ("12.50", false, "1250")];
    for (text, negative, digits) in cases {
        let (sign, coefficient, scale) = coefficient(text, 2);
        assert_eq!(sign, negative);
        assert_eq!(coefficient, digits);
        assert_eq!(scale, 2);
        println!("decimal_api text={text} scale=2 negative={sign} coefficient={coefficient}");
    }
    let inexact = ExactDecimal::parse_sql("1.251").expect("1.251 parses");
    assert_eq!(inexact.rescale_exact(2), Err(DecimalError::Inexact));
    println!("decimal_api text=1.251 scale=2 inexact");
    println!("arrow_decimal128=not_inspected reason=private_arrow_dependency_no_product_manifest_change");
    println!("a199_public_reader_typed_proof=671507b229ad7d1bc6a2e948b5b74d6d7252393f");
}

#[test]
fn csv_finish_receipt_matches_command_summary() {
    let bytes = csv_bytes();
    let object = sha256(&bytes);
    let profile_text = csv_preimage();
    let profile = csv_profile();
    assert_eq!(profile.profile_preimage(), profile_text);
    let profile_digest = sha256(profile_text.as_bytes());
    assert_eq!(profile.profile_digest(), profile_digest);
    let config_text = config_preimage(
        "oo-delimited-config-identity/v1\n",
        &profile_text,
        &hex(&object),
    );
    let config_identity = sha256(config_text.as_bytes());
    assert_eq!(profile.config_identity(object), config_identity);
    assert_eq!(profile.config_preimage(object), config_text);
    let dir = case_dir("csv-positive");
    let input = dir.join("object.csv");
    write_bytes(&input, &bytes);
    let seen = pull_delimited(&input, csv_profile(), object, 100, false).expect("csv finish");
    let hand = Hand {
        file_len: 127,
        rows: 4,
        batches: 2,
        batch_sizes: &[2, 2],
        fields: 16,
        value: 14,
        null: 1,
        empty: 1,
        missing: 0,
        first_line: 2,
        last_line: 5,
        first_offset: 21,
        last_offset: 116,
        columns: 4,
    };
    assert_eq!(bytes.len(), hand.file_len);
    assert_eq!(seen.object_sha256, object);
    assert_eq!(seen.profile_digest, profile_digest);
    assert_eq!(seen.config_identity, config_identity);
    assert_eq!(seen.rows, hand.rows);
    assert_eq!(seen.batches, hand.batches);
    assert_eq!(seen.batch_sizes, hand.batch_sizes);
    assert_eq!(seen.columns, hand.columns);
    assert_eq!(seen.profile_id, "a204-csv");
    assert_fold(&seen.fold, &hand);
    let run = run_request(
        &dir,
        &request_value("csv", &input, &hex(&object), csv_profile_value(), 100, false, false),
    );
    let types = csv_types();
    assert_success(
        &run,
        &SuccessExpect {
            source_protocol: DELIMITED_PROTOCOL,
            profile_version: DELIMITED_PROFILE_VERSION,
            lineage_version: DELIMITED_LINEAGE_VERSION,
            receipt_version: DELIMITED_RECEIPT_VERSION,
            profile_id: "a204-csv",
            profile_digest: &seen.profile_digest,
            config_identity: &seen.config_identity,
            object_sha256: &seen.object_sha256,
            rows: seen.rows,
            columns: seen.columns,
            batches: seen.batches,
            fold: &seen.fold,
            types: &types,
            input: &input,
        },
    );
    save_oracle("positive-csv", &bytes, &profile_text, &run.stdout);
    println!(
        "csv_object_sha256={} csv_profile_digest={} csv_config_identity={} rows={} batches={} value={} null={} empty={}",
        hex(&seen.object_sha256),
        hex(&seen.profile_digest),
        hex(&seen.config_identity),
        seen.rows,
        seen.batches,
        seen.fold.value,
        seen.fold.null,
        seen.fold.empty
    );
}

#[test]
fn tsv_finish_receipt_matches_command_summary() {
    let bytes = tsv_bytes();
    let object = sha256(&bytes);
    let profile_text = tsv_preimage();
    let profile = tsv_profile();
    assert_eq!(profile.profile_preimage(), profile_text);
    let profile_digest = sha256(profile_text.as_bytes());
    assert_eq!(profile.profile_digest(), profile_digest);
    let config_text = config_preimage(
        "oo-delimited-config-identity/v1\n",
        &profile_text,
        &hex(&object),
    );
    let config_identity = sha256(config_text.as_bytes());
    assert_eq!(profile.config_identity(object), config_identity);
    let dir = case_dir("tsv-positive");
    let input = dir.join("object.tsv");
    write_bytes(&input, &bytes);
    let seen = pull_delimited(&input, tsv_profile(), object, 100, false).expect("tsv finish");
    let hand = Hand {
        file_len: 71,
        rows: 3,
        batches: 2,
        batch_sizes: &[2, 1],
        fields: 12,
        value: 11,
        null: 1,
        empty: 0,
        missing: 0,
        first_line: 1,
        last_line: 3,
        first_offset: 0,
        last_offset: 38,
        columns: 4,
    };
    assert_eq!(bytes.len(), hand.file_len);
    assert_eq!(seen.object_sha256, object);
    assert_eq!(seen.profile_digest, profile_digest);
    assert_eq!(seen.config_identity, config_identity);
    assert_eq!(seen.batch_sizes, hand.batch_sizes);
    assert_eq!(seen.columns, hand.columns);
    assert_fold(&seen.fold, &hand);
    let run = run_request(
        &dir,
        &request_value("tsv", &input, &hex(&object), tsv_profile_value(), 100, false, false),
    );
    let types = tsv_types();
    assert_success(
        &run,
        &SuccessExpect {
            source_protocol: DELIMITED_PROTOCOL,
            profile_version: DELIMITED_PROFILE_VERSION,
            lineage_version: DELIMITED_LINEAGE_VERSION,
            receipt_version: DELIMITED_RECEIPT_VERSION,
            profile_id: "a204-tsv",
            profile_digest: &seen.profile_digest,
            config_identity: &seen.config_identity,
            object_sha256: &seen.object_sha256,
            rows: seen.rows,
            columns: seen.columns,
            batches: seen.batches,
            fold: &seen.fold,
            types: &types,
            input: &input,
        },
    );
    save_oracle("positive-tsv", &bytes, &profile_text, &run.stdout);
    println!(
        "tsv_object_sha256={} tsv_profile_digest={} tsv_config_identity={} rows={} batches={} value={} null={}",
        hex(&seen.object_sha256),
        hex(&seen.profile_digest),
        hex(&seen.config_identity),
        seen.rows,
        seen.batches,
        seen.fold.value,
        seen.fold.null
    );
}

#[test]
fn jsonl_finish_receipt_matches_command_summary() {
    let bytes = jsonl_bytes();
    let object = sha256(&bytes);
    let profile_text = jsonl_preimage();
    let profile = jsonl_profile();
    assert_eq!(sha256(profile_text.as_bytes()), profile.profile_digest());
    let config_text = config_preimage("oo-jsonl-config-identity/v1\n", &profile_text, &hex(&object));
    let config_identity = sha256(config_text.as_bytes());
    assert_eq!(profile.config_identity(object), config_identity);
    let dir = case_dir("jsonl-positive");
    let input = dir.join("object.jsonl");
    write_bytes(&input, &bytes);
    let seen = pull_jsonl(&input, jsonl_profile(), object, 100, false).expect("jsonl finish");
    let hand = Hand {
        file_len: 204,
        rows: 4,
        batches: 2,
        batch_sizes: &[2, 2],
        fields: 16,
        value: 13,
        null: 1,
        empty: 0,
        missing: 2,
        first_line: 1,
        last_line: 4,
        first_offset: 0,
        last_offset: 170,
        columns: 4,
    };
    assert_eq!(bytes.len(), hand.file_len);
    assert_eq!(seen.object_sha256, object);
    assert_eq!(seen.profile_digest, sha256(profile_text.as_bytes()));
    assert_eq!(seen.config_identity, config_identity);
    assert_eq!(seen.batch_sizes, hand.batch_sizes);
    assert_eq!(seen.columns, hand.columns);
    assert_fold(&seen.fold, &hand);
    assert_eq!(seen.fold.empty, 0);
    let run = run_request(
        &dir,
        &request_value("jsonl", &input, &hex(&object), jsonl_profile_value(), 100, true, false),
    );
    let types = jsonl_types();
    assert_success(
        &run,
        &SuccessExpect {
            source_protocol: JSONL_PROTOCOL,
            profile_version: JSONL_PROFILE_VERSION,
            lineage_version: JSONL_LINEAGE_VERSION,
            receipt_version: JSONL_RECEIPT_VERSION,
            profile_id: "a204-jsonl",
            profile_digest: &seen.profile_digest,
            config_identity: &seen.config_identity,
            object_sha256: &seen.object_sha256,
            rows: seen.rows,
            columns: seen.columns,
            batches: seen.batches,
            fold: &seen.fold,
            types: &types,
            input: &input,
        },
    );
    save_oracle("positive-jsonl", &bytes, &profile_text, &run.stdout);
    println!(
        "jsonl_object_sha256={} jsonl_profile_digest={} jsonl_config_identity={} rows={} batches={} value={} null={} missing={}",
        hex(&seen.object_sha256),
        hex(&seen.profile_digest),
        hex(&seen.config_identity),
        seen.rows,
        seen.batches,
        seen.fold.value,
        seen.fold.null,
        seen.fold.missing
    );
}

#[test]
fn empty_file_finish_is_zero_rows() {
    let bytes = b"";
    let object = sha256(bytes);
    assert_eq!(hex(&object), EMPTY_SHA);
    let profile_text = empty_preimage();
    let profile = empty_profile();
    assert_eq!(profile.profile_preimage(), profile_text);
    let profile_digest = sha256(profile_text.as_bytes());
    assert_eq!(profile.profile_digest(), profile_digest);
    let dir = case_dir("csv-empty");
    let input = dir.join("empty.csv");
    write_bytes(&input, bytes);
    let seen = pull_delimited(&input, empty_profile(), object, 100, false).expect("empty finish");
    let hand = Hand {
        file_len: 0,
        rows: 0,
        batches: 0,
        batch_sizes: &[],
        fields: 0,
        value: 0,
        null: 0,
        empty: 0,
        missing: 0,
        first_line: 0,
        last_line: 0,
        first_offset: 0,
        last_offset: 0,
        columns: 1,
    };
    assert_eq!(seen.batch_sizes, hand.batch_sizes);
    assert_eq!(seen.object_sha256, object);
    assert_eq!(seen.profile_digest, profile_digest);
    assert_eq!(seen.columns, hand.columns);
    assert_fold(&seen.fold, &hand);
    let run = run_request(
        &dir,
        &request_value("csv", &input, &hex(&object), empty_profile_value(), 100, false, false),
    );
    let types = empty_types();
    assert_success(
        &run,
        &SuccessExpect {
            source_protocol: DELIMITED_PROTOCOL,
            profile_version: DELIMITED_PROFILE_VERSION,
            lineage_version: DELIMITED_LINEAGE_VERSION,
            receipt_version: DELIMITED_RECEIPT_VERSION,
            profile_id: "a204-empty",
            profile_digest: &seen.profile_digest,
            config_identity: &seen.config_identity,
            object_sha256: &seen.object_sha256,
            rows: 0,
            columns: 1,
            batches: 0,
            fold: &seen.fold,
            types: &types,
            input: &input,
        },
    );
    println!("empty_object_sha256={}", hex(&seen.object_sha256));
}

#[test]
fn failures_do_not_mint_a_success_receipt() {
    let dir = case_dir("failures");
    let csv = csv_bytes();
    let csv_path = dir.join("object.csv");
    write_bytes(&csv_path, &csv);
    let csv_sha = hex(&sha256(&csv));

    let mut exact = csv_profile_value();
    exact["header"] = json!("exact");
    exact["id"] = json!("a204-empty-exact");
    exact["bom"] = json!("refuse");
    let empty = dir.join("empty.csv");
    write_bytes(&empty, b"");
    let err = pull_delimited(
        &empty,
        csv_shaped("a204-empty-exact", BomPolicy::Refuse, HeaderPolicy::Exact),
        sha256(b""),
        100,
        false,
    )
    .expect_err("header");
    assert_eq!(err.fact(), "header_required");
    let empty_run = run_request(
        &dir,
        &request_value("csv", &empty, EMPTY_SHA, exact, 100, false, false),
    );
    assert_failure(&empty_run, 5, "parse", "header_required", &[]);

    let mut row = b"label,ok,n,amount\nx,true,1,1.251\n".to_vec();
    let inexact_path = dir.join("inexact.csv");
    write_bytes(&inexact_path, &row);
    let err = pull_delimited(&inexact_path, csv_profile(), sha256(&row), 100, false).expect_err("inexact");
    assert_eq!(err.fact(), "inexact_decimal");
    let inexact_run = run_request(
        &dir,
        &request_value("csv", &inexact_path, &hex(&sha256(&row)), csv_profile_value(), 100, false, false),
    );
    assert_failure(&inexact_run, 5, "parse", "inexact_decimal", &["1.251"]);

    row = b"label,ok,n,amount\nx,TRUE,1,1.00\n".to_vec();
    let bool_path = dir.join("bool.csv");
    write_bytes(&bool_path, &row);
    let err = pull_delimited(&bool_path, csv_profile(), sha256(&row), 100, false).expect_err("bool");
    assert_eq!(err.fact(), "not_bool");
    let bool_run = run_request(
        &dir,
        &request_value("csv", &bool_path, &hex(&sha256(&row)), csv_profile_value(), 100, false, false),
    );
    assert_failure(&bool_run, 5, "parse", "not_bool", &["TRUE"]);

    row = b"label,ok,n,amount\nx,true,01,1.00\n".to_vec();
    let int_path = dir.join("int.csv");
    write_bytes(&int_path, &row);
    let err = pull_delimited(&int_path, csv_profile(), sha256(&row), 100, false).expect_err("int");
    assert_eq!(err.fact(), "not_int64");
    let int_run = run_request(
        &dir,
        &request_value("csv", &int_path, &hex(&sha256(&row)), csv_profile_value(), 100, false, false),
    );
    assert_failure(&int_run, 5, "parse", "not_int64", &["01"]);

    let fraction = b"{\"label\":\"a\",\"ok\":true,\"n\":1.0,\"payload\":null}\n";
    let fraction_path = dir.join("fraction.jsonl");
    write_bytes(&fraction_path, fraction);
    let err = pull_jsonl(&fraction_path, jsonl_profile(), sha256(fraction), 100, false).expect_err("fraction");
    assert_eq!(err.fact(), "not_int64");
    let fraction_run = run_request(
        &dir,
        &request_value(
            "jsonl",
            &fraction_path,
            &hex(&sha256(fraction)),
            jsonl_profile_value(),
            100,
            true,
            false,
        ),
    );
    assert_failure(&fraction_run, 5, "parse", "not_int64", &["1.0"]);

    let err = pull_delimited(&csv_path, csv_profile(), sha256(&csv), 1, false).expect_err("max_rows");
    assert_eq!(err.fact(), "max_rows");
    let budget_run = run_request(
        &dir,
        &request_value("csv", &csv_path, &csv_sha, csv_profile_value(), 1, false, false),
    );
    assert_failure(&budget_run, 6, "budget", "max_rows", &[]);

    let flipped = flip_last_nibble(&csv_sha);
    assert_ne!(flipped, csv_sha);
    let mut flipped_bytes = [0u8; 32];
    for (index, slot) in flipped_bytes.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&flipped[index * 2..index * 2 + 2], 16).expect("hex");
    }
    let err = pull_delimited(&csv_path, csv_profile(), flipped_bytes, 100, false).expect_err("hash");
    assert_eq!(err.fact(), "hash_mismatch");
    let hash_run = run_request(
        &dir,
        &request_value("csv", &csv_path, &flipped, csv_profile_value(), 100, false, false),
    );
    assert_failure(&hash_run, 7, "hash", "hash_mismatch", &[]);

    let err = pull_delimited(
        &csv_path,
        csv_shaped("a204-csv", BomPolicy::Refuse, HeaderPolicy::Exact),
        sha256(&csv),
        100,
        false,
    )
    .expect_err("bom");
    assert_eq!(err.fact(), "bom_refused");
    let mut refuse = csv_profile_value();
    refuse["bom"] = json!("refuse");
    let bom_run = run_request(
        &dir,
        &request_value("csv", &csv_path, &csv_sha, refuse, 100, false, false),
    );
    assert_failure(&bom_run, 5, "parse", "bom_refused", &[]);

    let err = pull_delimited(&csv_path, csv_profile(), sha256(&csv), 100, true).expect_err("cancel");
    assert_eq!(err.fact(), "cancelled");
    let cancel_run = run_request(
        &dir,
        &request_value("csv", &csv_path, &csv_sha, csv_profile_value(), 100, false, true),
    );
    assert_failure(&cancel_run, 8, "cancel", "cancelled", &[]);
}

#[test]
fn regular_file_checks_do_not_echo_the_path() {
    let dir = case_dir("paths");
    let missing = dir.join("PATHTOKEN_a204");
    let missing_run = run_request(
        &dir,
        &request_value(
            "csv",
            &missing,
            EMPTY_SHA,
            csv_profile_value(),
            100,
            false,
            true,
        ),
    );
    assert_failure(&missing_run, 4, "read", "open", &[missing.to_str().expect("path")]);

    let folder = dir.join("a-directory");
    fs::create_dir(&folder).expect("dir");
    let dir_run = run_request(
        &dir,
        &request_value("csv", &folder, EMPTY_SHA, csv_profile_value(), 100, false, false),
    );
    assert_failure(&dir_run, 4, "read", "not_file", &[folder.to_str().expect("path")]);

    let bytes = csv_bytes();
    let target = dir.join("object.csv");
    write_bytes(&target, &bytes);
    let link = dir.join("linked.csv");
    symlink(&target, &link).expect("symlink");
    let object = hex(&sha256(&bytes));
    let seen = pull_delimited(&link, csv_profile(), sha256(&bytes), 100, false).expect("symlink finish");
    assert_eq!(seen.rows, 4);
    let run = run_request(
        &dir,
        &request_value("csv", &link, &object, csv_profile_value(), 100, false, false),
    );
    assert_eq!(run.code, 0, "stderr={}", run.stderr);
    assert!(!run.stdout.contains(link.to_str().expect("link")));
    assert!(run.stdout.contains(RESULT_PROTOCOL));
    println!("symlink_followed=1 rows={}", seen.rows);
}

#[test]
fn strict_request_shape_and_bounds() {
    let dir = case_dir("shape");
    let input = dir.join("object.csv");
    write_bytes(&input, &csv_bytes());
    let sha = hex(&sha256(&csv_bytes()));
    let input_text = serde_json::to_string(input.to_str().expect("path")).expect("path json");
    let sha_text = serde_json::to_string(&sha).expect("sha json");

    let duplicate = format!(
        "{{\"protocol\":{proto},\"protocol\":\"other\",\"format\":\"csv\",\"input_path\":{input_text},\"expected_sha256\":{sha_text},\"cancel_before_read\":false,\"profile\":{{}},\"budget\":{{}}}}",
        proto = serde_json::to_string(REQUEST_PROTOCOL).expect("protocol"),
        input_text = input_text,
        sha_text = sha_text,
    );
    let duplicate_path = dir.join("duplicate.json");
    write_bytes(&duplicate_path, duplicate.as_bytes());
    assert_failure(
        &run_validate(&["--request", duplicate_path.to_str().expect("dup")]),
        3,
        "config",
        "request_json",
        &[],
    );

    let mut extra = request_value("csv", &input, &sha, csv_profile_value(), 100, false, false);
    extra["password"] = json!("SECRET_a204");
    assert_failure(&run_request(&dir, &extra), 3, "config", "unknown_field", &["SECRET_a204"]);

    let mut upper = request_value("csv", &input, &sha, csv_profile_value(), 100, false, false);
    upper["expected_sha256"] = json!(sha.to_ascii_uppercase());
    assert_failure(&run_request(&dir, &upper), 3, "config", "expected_sha256", &[]);

    let mut bad_format = request_value("csv", &input, &sha, csv_profile_value(), 100, false, false);
    bad_format["format"] = json!("parquet");
    assert_failure(&run_request(&dir, &bad_format), 3, "config", "format", &[]);

    let mut disagree = csv_profile_value();
    disagree["dialect"] = json!("tsv");
    assert_failure(
        &run_request(
            &dir,
            &request_value("csv", &input, &sha, disagree, 100, false, false),
        ),
        3,
        "config",
        "dialect",
        &[],
    );

    let mut comma_null = csv_profile_value();
    comma_null["null"] = json!({"policy": "token", "token": ","});
    assert_failure(
        &run_request(
            &dir,
            &request_value("csv", &input, &sha, comma_null, 100, false, false),
        ),
        3,
        "config",
        "null_token",
        &[],
    );

    let mut floated = csv_profile_value();
    floated["columns"][0]["type"] = json!("float");
    assert_failure(
        &run_request(&dir, &request_value("csv", &input, &sha, floated, 100, false, false)),
        3,
        "config",
        "type",
        &[],
    );

    let mut with_dialect = jsonl_profile_value();
    with_dialect["dialect"] = json!("csv");
    assert_failure(
        &run_request(
            &dir,
            &request_value("jsonl", &input, &sha, with_dialect, 100, true, false),
        ),
        3,
        "config",
        "unknown_field",
        &[],
    );

    let mut field_budget = budget_value(100, true);
    field_budget["max_field_bytes"] = json!(8);
    let mut jsonl_req = request_value("jsonl", &input, &sha, jsonl_profile_value(), 100, true, false);
    jsonl_req["budget"] = field_budget;
    assert_failure(&run_request(&dir, &jsonl_req), 3, "config", "unknown_field", &[]);

    let mut decimal_jsonl = jsonl_profile_value();
    decimal_jsonl["columns"][0]["type"] = json!("decimal");
    assert_failure(
        &run_request(
            &dir,
            &request_value("jsonl", &input, &sha, decimal_jsonl, 100, true, false),
        ),
        3,
        "config",
        "type",
        &[],
    );

    for millis in [0_u64, 120_001] {
        let mut body = request_value("csv", &input, &sha, csv_profile_value(), 100, false, false);
        body["budget"]["deadline_ms"] = json!(millis);
        assert_failure(&run_request(&dir, &body), 3, "config", "deadline", &[]);
    }

    let mut null_protocol = request_value("csv", &input, &sha, csv_profile_value(), 100, false, false);
    null_protocol["protocol"] = Value::Null;
    assert_failure(&run_request(&dir, &null_protocol), 3, "config", "request_field", &[]);

    let over = dir.join("over.json");
    write_bytes(&over, &vec![b'x'; REQUEST_MAX + 1]);
    assert_failure(
        &run_validate(&["--request", over.to_str().expect("over")]),
        3,
        "config",
        "request_bytes",
        &[],
    );
    let exact = dir.join("exact.json");
    write_bytes(&exact, &vec![b'{'; REQUEST_MAX]);
    assert_failure(
        &run_validate(&["--request", exact.to_str().expect("exact")]),
        3,
        "config",
        "request_json",
        &[],
    );
}

#[test]
fn help_and_usage_are_bounded() {
    let help = run_validate(&["--help"]);
    assert_eq!(help.code, 0);
    assert!(help.stderr.is_empty());
    for needle in [
        REQUEST_PROTOCOL,
        "1048576",
        "CSV",
        "TSV",
        "JSONL",
        "does not infer a dialect",
        "does not import",
        "publish a table",
        "open a database",
        "Arrow IPC",
        "not interrupted",
        "does not authenticate a public caller",
    ] {
        assert!(help.stdout.contains(needle), "help missing {needle}");
    }

    let cases: &[(&[&str], &str)] = &[
        (&[], "usage"),
        (&["--request"], "usage"),
        (&["--nope"], "unknown_flag"),
        (&["-h"], "unknown_flag"),
        (&["--help", "SECRET_a204"], "unknown_flag"),
        (&["--request", "unused.json", "--extra"], "unknown_flag"),
    ];
    for (args, fact) in cases {
        let run = run_validate(args);
        assert_failure(&run, 2, "usage", fact, &["SECRET_a204", "unused.json"]);
    }
}
