//! Focused pull tests for the CSV and TSV reader.
//! Object hashes come from an in-process FIPS 180-4 SHA-256, not from sha2
//! and not from a shell checksum. Expected cells are hand-checked constants.

use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use arrow_array::{Array, BooleanArray, Decimal128Array, Int64Array, RecordBatch, StringArray};
use arrow_schema::DataType;
use oo_connectors::{
    BomPolicy, DelimitedBatch, DelimitedBudget, DelimitedColumn, DelimitedComplete,
    DelimitedDialect, DelimitedError, DelimitedErrorKind, DelimitedExhausted, DelimitedItem,
    DelimitedProfile, DelimitedReader, DelimitedType, HeaderPolicy, NullPolicy,
    DelimitedFieldLineage, DelimitedFieldPresence, DelimitedRowLineage, DELIMITED_LINEAGE_VERSION,
    DELIMITED_PROFILE_VERSION, DELIMITED_PROTOCOL, DELIMITED_RECEIPT_VERSION,
};

const ENTERPRISE_PREIMAGE: &str = "\
oo-delimited-profile/v1\n\
id=enterprise-import\n\
dialect=csv\n\
bom=strip\n\
header=exact\n\
null=token\n\
null_token=\\N\n\
quote=double\n\
record=lf-or-crlf\n\
escape=double-quote\n\
column=名称\tutf8\t1\t-\t-\n\
column=ok\tbool\t0\t-\t-\n\
column=n\tint64\t0\t-\t-\n\
column=amount\tdecimal\t0\t10\t2\n\
column=note\tutf8\t1\t-\t-\n";

const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[derive(Clone, Debug, PartialEq, Eq)]
struct OracleField {
    text: String,
    line: u64,
    offset: u64,
    quoted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OracleRow {
    line: u64,
    offset: u64,
    raw_len: u64,
    fields: Vec<OracleField>,
}

struct Pulled {
    batches: Vec<DelimitedBatch>,
    exhausted: DelimitedExhausted,
    done: DelimitedComplete,
}

fn raw_sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = u64::try_from(input.len()).unwrap().checked_mul(8).unwrap();
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
            *slot = u32::from_be_bytes(chunk[start..start + 4].try_into().unwrap());
        }
        for index in 16..64 {
            let s0 = word[index - 15].rotate_right(7)
                ^ word[index - 15].rotate_right(18)
                ^ (word[index - 15] >> 3);
            let s1 = word[index - 2].rotate_right(17)
                ^ word[index - 2].rotate_right(19)
                ^ (word[index - 2] >> 10);
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
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
    let mut out = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}

fn trusted_sha256(input: &[u8]) -> [u8; 32] {
    assert_eq!(hex(&raw_sha256(b"")), EMPTY_SHA);
    assert_eq!(hex(&raw_sha256(b"abc")), ABC_SHA);
    raw_sha256(input)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn budget_with(rows: u32, bytes: u64, record: u32, field: u32, total: u64, max_rows: u64) -> DelimitedBudget {
    DelimitedBudget::try_new(
        rows,
        bytes,
        record,
        field,
        total,
        max_rows,
        32,
        32,
        Instant::now() + Duration::from_secs(60),
    )
    .unwrap()
}

fn budget() -> DelimitedBudget {
    budget_with(64, 1024 * 1024, 4096, 1024, 1024 * 1024, 10_000)
}

fn col(name: &str, ty: DelimitedType, nullable: bool) -> DelimitedColumn {
    DelimitedColumn::try_new(name, ty, nullable).unwrap()
}

fn profile(
    dialect: DelimitedDialect,
    bom: BomPolicy,
    header: HeaderPolicy,
    nulls: NullPolicy,
    columns: Vec<DelimitedColumn>,
) -> DelimitedProfile {
    DelimitedProfile::try_new("sample", dialect, bom, header, nulls, columns).unwrap()
}

fn text_profile(header: HeaderPolicy, bom: BomPolicy) -> DelimitedProfile {
    profile(
        DelimitedDialect::Csv,
        bom,
        header,
        NullPolicy::Empty,
        vec![col("name", DelimitedType::Utf8, true)],
    )
}

fn enterprise_profile() -> DelimitedProfile {
    DelimitedProfile::try_new(
        "enterprise-import",
        DelimitedDialect::Csv,
        BomPolicy::Strip,
        HeaderPolicy::Exact,
        NullPolicy::Token("\\N".to_owned()),
        vec![
            col("名称", DelimitedType::Utf8, true),
            col("ok", DelimitedType::Bool, false),
            col("n", DelimitedType::Int64, false),
            col("amount", DelimitedType::Decimal { precision: 10, scale: 2 }, false),
            col("note", DelimitedType::Utf8, true),
        ],
    )
    .unwrap()
}

fn enterprise_csv() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    out.extend_from_slice("名称,ok,n,amount,note\r\n".as_bytes());
    out.extend_from_slice("熊,true,-9223372036854775808,1.20,\"甲,\"\"\r\n乙\"\r\n".as_bytes());
    out.extend_from_slice("猫,false,9223372036854775807,1.2,\"\"\r\n".as_bytes());
    out.extend_from_slice("\\N,true,-0,0.00,".as_bytes());
    out
}

fn sample_tsv() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice("熊\t\"a\tb\"\t7\n".as_bytes());
    out.extend_from_slice("猫,鼠\t上海\t-0".as_bytes());
    out
}

fn temp_file(bytes: &[u8], ext: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("oo-delimited-a192-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}.{ext}", NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, bytes).unwrap();
    path
}

fn pull<R: Read>(reader: DelimitedReader<R>) -> Pulled {
    let mut reader = reader;
    let mut batches = Vec::new();
    let exhausted = loop {
        match reader.next_batch().unwrap() {
            DelimitedItem::Batch(batch) => batches.push(*batch),
            DelimitedItem::Exhausted(exhausted) => break exhausted,
        }
    };
    let again = reader.next_batch().unwrap();
    let DelimitedItem::Exhausted(repeated) = again else {
        panic!("second pull after exhaustion returned a batch");
    };
    assert_eq!(repeated, exhausted);
    let done = reader.finish().unwrap();
    Pulled { batches, exhausted, done }
}

fn open_bytes(bytes: &[u8], profile: DelimitedProfile, budget: DelimitedBudget) -> DelimitedReader<Cursor<Vec<u8>>> {
    let digest = trusted_sha256(bytes);
    DelimitedReader::open(Cursor::new(bytes.to_vec()), profile, digest, budget).unwrap()
}

fn refuse(bytes: &[u8], profile: DelimitedProfile, budget: DelimitedBudget) -> DelimitedError {
    let mut reader = open_bytes(bytes, profile, budget);
    let err = reader.next_batch().unwrap_err();
    let later = reader.finish().unwrap_err();
    assert_eq!(later, err);
    assert_closed(&err, &[]);
    err
}

fn assert_closed(err: &DelimitedError, poisons: &[&str]) {
    let shown = format!("{err} | {err:?}");
    for poison in poisons {
        assert!(!shown.contains(poison), "{shown} contains {poison}");
    }
}

fn utf8_col(batch: &RecordBatch, index: usize) -> Vec<Option<String>> {
    let array = batch.column(index).as_any().downcast_ref::<StringArray>().unwrap();
    (0..array.len())
        .map(|row| if array.is_null(row) { None } else { Some(array.value(row).to_owned()) })
        .collect()
}

fn bool_col(batch: &RecordBatch, index: usize) -> Vec<Option<bool>> {
    let array = batch.column(index).as_any().downcast_ref::<BooleanArray>().unwrap();
    (0..array.len())
        .map(|row| if array.is_null(row) { None } else { Some(array.value(row)) })
        .collect()
}

fn i64_col(batch: &RecordBatch, index: usize) -> Vec<Option<i64>> {
    let array = batch.column(index).as_any().downcast_ref::<Int64Array>().unwrap();
    (0..array.len())
        .map(|row| if array.is_null(row) { None } else { Some(array.value(row)) })
        .collect()
}

fn dec_col(batch: &RecordBatch, index: usize) -> Vec<Option<i128>> {
    let array = batch.column(index).as_any().downcast_ref::<Decimal128Array>().unwrap();
    (0..array.len())
        .map(|row| if array.is_null(row) { None } else { Some(array.value(row)) })
        .collect()
}

fn oracle_rows(input: &[u8], sep: u8, strip_bom: bool) -> Result<Vec<OracleRow>, &'static str> {
    let mut index = 0usize;
    let mut line = 1u64;
    if strip_bom {
        if input.len() >= 3 && input[..3] == [0xEF, 0xBB, 0xBF] {
            index = 3;
        }
    } else if input.len() >= 3 && input[..3] == [0xEF, 0xBB, 0xBF] {
        return Err("bom");
    }
    let mut rows = Vec::new();
    while index < input.len() {
        let row_line = line;
        let row_off = u64::try_from(index).unwrap();
        let mut fields = Vec::new();
        loop {
            fields.push(oracle_field(input, &mut index, &mut line, sep)?);
            if index >= input.len() {
                break;
            }
            let byte = input[index];
            index += 1;
            if byte == sep {
                continue;
            }
            if byte == b'\n' {
                line += 1;
                break;
            }
            if byte == b'\r' {
                if index >= input.len() || input[index] != b'\n' {
                    return Err("cr");
                }
                index += 1;
                line += 1;
                break;
            }
            return Err("junk");
        }
        let end = u64::try_from(index).unwrap();
        rows.push(OracleRow {
            line: row_line,
            offset: row_off,
            raw_len: end - row_off,
            fields,
        });
    }
    Ok(rows)
}

fn oracle_field(input: &[u8], index: &mut usize, line: &mut u64, sep: u8) -> Result<OracleField, &'static str> {
    let start_line = *line;
    let start = u64::try_from(*index).unwrap();
    if *index >= input.len() || input[*index] == sep || input[*index] == b'\n' || input[*index] == b'\r' {
        return Ok(OracleField { text: String::new(), line: start_line, offset: start, quoted: false });
    }
    if input[*index] == b'"' {
        *index += 1;
        let mut raw = Vec::new();
        loop {
            if *index >= input.len() {
                return Err("quote");
            }
            let byte = input[*index];
            *index += 1;
            if byte == b'\n' {
                *line += 1;
            }
            if byte == b'"' {
                if *index < input.len() && input[*index] == b'"' {
                    *index += 1;
                    raw.push(b'"');
                } else {
                    break;
                }
            } else {
                raw.push(byte);
            }
        }
        let text = String::from_utf8(raw).map_err(|_| "utf8")?;
        return Ok(OracleField { text, line: start_line, offset: start, quoted: true });
    }
    let mut raw = Vec::new();
    while *index < input.len()
        && input[*index] != sep
        && input[*index] != b'\n'
        && input[*index] != b'\r'
    {
        if input[*index] == b'"' {
            return Err("quote");
        }
        raw.push(input[*index]);
        *index += 1;
    }
    let text = String::from_utf8(raw).map_err(|_| "utf8")?;
    Ok(OracleField { text, line: start_line, offset: start, quoted: false })
}

fn dump_oracle(name: &str, bytes: &[u8], preimage: &str, digest: [u8; 32], lineage: &str) {
    let Ok(dir) = std::env::var("OO_A192_ORACLE_DIR") else {
        return;
    };
    fs::create_dir_all(&dir).unwrap();
    fs::write(format!("{dir}/{name}.bin"), bytes).unwrap();
    fs::write(format!("{dir}/{name}.preimage.txt"), preimage).unwrap();
    fs::write(format!("{dir}/{name}.sha256.txt"), hex(&digest)).unwrap();
    fs::write(format!("{dir}/{name}.lineage.txt"), lineage).unwrap();
}

fn assert_receipt(done: &DelimitedComplete, rows: u64, batches: u64, digest: [u8; 32]) {
    assert_eq!(done.protocol, DELIMITED_PROTOCOL);
    assert_eq!(done.profile_version, DELIMITED_PROFILE_VERSION);
    assert_eq!(done.lineage_version, DELIMITED_LINEAGE_VERSION);
    assert_eq!(done.receipt_version, DELIMITED_RECEIPT_VERSION);
    assert_eq!(done.rows, rows);
    assert_eq!(done.batches, batches);
    assert_eq!(done.object_sha256, digest);
    assert!(!done.admits_import);
    assert!(!done.published);
}

fn field_at<'a>(row: &'a DelimitedRowLineage, name: &str) -> &'a DelimitedFieldLineage {
    row.fields.iter().find(|field| field.name == name).unwrap()
}

#[test]
fn sha256_oracle_matches_the_empty_and_abc_vectors() {
    assert_eq!(hex(&trusted_sha256(b"")), EMPTY_SHA);
    assert_eq!(hex(&trusted_sha256(b"abc")), ABC_SHA);
    dump_oracle("sha256-self-check", b"abc", "abc", trusted_sha256(b"abc"), EMPTY_SHA);
}

#[test]
fn source_is_a_bounded_state_machine() {
    let src = include_str!("../src/delimited.rs");
    assert!(!src.contains("unsafe"));
    assert!(!src.contains("read_to_end"));
    assert!(!src.contains("read_line"));
    assert!(!src.contains("read_to_string"));
    assert!(!src.contains(".split("));
    let contract = include_str!("../../../contracts/connectors/delimited-reader-v1.md");
    for line in [
        "oo.source.delimited/v1",
        "oo-delimited-profile/v1",
        "oo-delimited-config-identity/v1",
        "quote=double",
        "record=lf-or-crlf",
        "escape=double-quote",
        "null_token=",
        "expected_sha256=",
        "ambiguous_dialect",
        "malformed_quote",
    ] {
        assert!(contract.contains(line), "{line}");
    }
}

#[test]
fn profile_preimage_and_budget_identity_are_exact() {
    let profile = enterprise_profile();
    assert_eq!(profile.profile_preimage(), ENTERPRISE_PREIMAGE);
    let digest = trusted_sha256(b"object");
    assert_eq!(profile.profile_digest(), trusted_sha256(ENTERPRISE_PREIMAGE.as_bytes()));
    let expected = format!(
        "oo-delimited-config-identity/v1\n{ENTERPRISE_PREIMAGE}expected_sha256={}\n",
        hex(&digest)
    );
    assert_eq!(profile.config_preimage(digest), expected);
    assert_eq!(profile.config_identity(digest), trusted_sha256(expected.as_bytes()));
    let left = budget_with(1, 10, 3, 3, 3, 1);
    let right = budget_with(8, 100, 64, 16, 1000, 20);
    assert_eq!(profile.config_identity(digest), {
        let same = enterprise_profile();
        same.config_identity(digest)
    });
    assert_eq!(left.queue_batches(), 1);
    assert_eq!(right.queue_batches(), 1);
    assert_ne!(left, right);
}

#[test]
fn profile_and_budget_reject_ambiguous_input() {
    for byte in [b';', b'|', b' ', b'"', b'\n', b'\r', b'\\'] {
        let err = DelimitedDialect::try_from_separator(byte).unwrap_err();
        assert_eq!(err.kind(), DelimitedErrorKind::Config);
        assert_eq!(err.fact(), "ambiguous_dialect");
    }
    assert_eq!(DelimitedDialect::try_from_separator(b',').unwrap(), DelimitedDialect::Csv);
    assert_eq!(DelimitedDialect::try_from_separator(b'\t').unwrap(), DelimitedDialect::Tsv);
    let err = DelimitedProfile::try_new(
        "sample",
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        vec![],
    )
    .unwrap_err();
    assert_eq!(err.fact(), "empty_selection");
    let err = DelimitedProfile::try_new(
        "sample",
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        vec![col("a", DelimitedType::Utf8, true), col("a", DelimitedType::Utf8, true)],
    )
    .unwrap_err();
    assert_eq!(err.fact(), "duplicate_column");
    assert_eq!(DelimitedColumn::try_new(" a", DelimitedType::Utf8, true).unwrap_err().fact(), "column_name");
    assert_eq!(DelimitedColumn::try_new("", DelimitedType::Utf8, true).unwrap_err().fact(), "column_name");
    let spaced = col("my col", DelimitedType::Utf8, true);
    assert_eq!(spaced.name(), "my col");
    assert!(DelimitedColumn::try_new("名称", DelimitedType::Utf8, true).is_ok());
    for (precision, scale, fact) in [(0, 0, "decimal_precision"), (39, 0, "decimal_precision"), (10, 39, "decimal_scale"), (10, -39, "decimal_scale"), (4, 5, "decimal_scale")] {
        let err = DelimitedColumn::try_new("amount", DelimitedType::Decimal { precision, scale }, false).unwrap_err();
        assert_eq!(err.fact(), fact, "{precision},{scale}");
    }
    assert!(DelimitedColumn::try_new("amount", DelimitedType::Decimal { precision: 10, scale: -2 }, false).is_ok());
    let err = DelimitedProfile::try_new(
        "sample",
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Token(",".to_owned()),
        vec![col("a", DelimitedType::Utf8, true)],
    )
    .unwrap_err();
    assert_eq!(err.fact(), "null_token");
    assert!(DelimitedProfile::try_new(
        "sample",
        DelimitedDialect::Tsv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Token(",".to_owned()),
        vec![col("a", DelimitedType::Utf8, true)],
    )
    .is_ok());
    assert_eq!(
        DelimitedBudget::try_new(0, 1, 3, 1, 3, 1, 1, 1, Instant::now()).unwrap_err().fact(),
        "batch_rows"
    );
    assert_eq!(
        DelimitedBudget::try_new(1, 1, 2, 1, 2, 1, 1, 1, Instant::now()).unwrap_err().fact(),
        "max_record"
    );
    assert_eq!(
        DelimitedBudget::try_new(1, 1, 8, 9, 8, 1, 1, 1, Instant::now()).unwrap_err().fact(),
        "max_field"
    );
}

#[test]
fn enterprise_csv_matches_the_hand_checked_oracle() {
    let bytes = enterprise_csv();
    assert_eq!(bytes.len(), 132);
    let note = "甲,\"\r\n乙";
    let oracle = oracle_rows(&bytes, b',', true).unwrap();
    assert_eq!(oracle.len(), 4);
    assert_eq!(oracle[0].line, 1);
    assert_eq!(oracle[0].offset, 3);
    assert_eq!(oracle[0].fields.iter().map(|field| field.text.as_str()).collect::<Vec<_>>(), ["名称", "ok", "n", "amount", "note"]);
    let expected = [
        (2, 28, 50, vec!["熊", "true", "-9223372036854775808", "1.20", note]),
        (4, 78, 38, vec!["猫", "false", "9223372036854775807", "1.2", ""]),
        (5, 116, 16, vec!["\\N", "true", "-0", "0.00", ""]),
    ];
    for (row, expect) in oracle.iter().skip(1).zip(expected) {
        assert_eq!(row.line, expect.0);
        assert_eq!(row.offset, expect.1);
        assert_eq!(row.raw_len, expect.2);
        let texts: Vec<&str> = row.fields.iter().map(|field| field.text.as_str()).collect();
        assert_eq!(texts, expect.3);
    }
    assert!(oracle[1].fields[4].quoted);
    assert!(oracle[2].fields[4].quoted);
    assert!(!oracle[3].fields[0].quoted);
    assert!(!oracle[3].fields[4].quoted);
    assert_eq!(oracle[1].fields[4].offset, 63);
    assert_eq!(oracle[1].fields[4].line, 2);

    let profile = enterprise_profile();
    let digest = trusted_sha256(&bytes);
    assert_eq!(profile.profile_preimage(), ENTERPRISE_PREIMAGE);
    let path = temp_file(&bytes, "csv");
    let file = File::open(&path).unwrap();
    let pulled = pull(DelimitedReader::open(file, profile.clone(), digest, budget()).unwrap());
    assert_enterprise(&pulled, &digest);
    let streamed = pull(DelimitedReader::open(OneByte { inner: Cursor::new(bytes.clone()) }, profile, digest, budget()).unwrap());
    assert_enterprise(&streamed, &digest);
    let lineage = lineage_text(&pulled.batches[0].lineage);
    dump_oracle("positive-csv", &bytes, ENTERPRISE_PREIMAGE, digest, &lineage);
}

fn assert_enterprise(pulled: &Pulled, digest: &[u8; 32]) {
    assert_eq!(pulled.batches.len(), 1);
    let batch = &pulled.batches[0];
    assert_eq!(batch.batch_index, 0);
    assert!(!batch.resumable);
    assert_eq!(batch.raw_bytes, 104);
    assert!(batch.arrow_bytes > 0);
    assert_eq!(batch.record_batch.num_rows(), 3);
    assert!(!batch.header.admits_import);
    assert!(!batch.header.published);
    assert!(!batch.header.arrow_ipc);
    assert!(!batch.header.executes_native);
    assert_eq!(batch.header.protocol, DELIMITED_PROTOCOL);
    assert_eq!(batch.header.dialect, DelimitedDialect::Csv);
    let note = "甲,\"\r\n乙";
    assert_eq!(utf8_col(&batch.record_batch, 0), vec![Some("熊".to_owned()), Some("猫".to_owned()), None]);
    assert_eq!(bool_col(&batch.record_batch, 1), vec![Some(true), Some(false), Some(true)]);
    assert_eq!(i64_col(&batch.record_batch, 2), vec![Some(i64::MIN), Some(i64::MAX), Some(0)]);
    assert_eq!(dec_col(&batch.record_batch, 3), vec![Some(120), Some(120), Some(0)]);
    assert_eq!(utf8_col(&batch.record_batch, 4), vec![Some(note.to_owned()), Some(String::new()), Some(String::new())]);
    assert_eq!(batch.record_batch.schema().field(3).data_type(), &DataType::Decimal128(10, 2));
    assert_eq!(batch.record_batch.schema().field(0).metadata().get("oo.delimited.logical").unwrap(), "utf8");
    assert_eq!(batch.record_batch.schema().field(1).metadata().get("oo.delimited.logical").unwrap(), "bool");
    assert_eq!(batch.record_batch.schema().field(2).metadata().get("oo.delimited.logical").unwrap(), "int64");
    assert_eq!(batch.record_batch.schema().field(3).metadata().get("oo.delimited.logical").unwrap(), "decimal");
    assert_eq!(batch.record_batch.schema().field(3).metadata().get("oo.delimited.precision").unwrap(), "10");
    assert_eq!(batch.record_batch.schema().field(3).metadata().get("oo.delimited.scale").unwrap(), "2");
    assert_eq!(batch.record_batch.schema().field(0).metadata().get("oo.delimited.protocol").unwrap(), DELIMITED_PROTOCOL);
    let lines = [2_u64, 4, 5];
    let offsets = [28_u64, 78, 116];
    let note_at = [63_u64, 112, 132];
    let presence = [DelimitedFieldPresence::Value, DelimitedFieldPresence::Empty, DelimitedFieldPresence::Empty];
    assert_eq!(batch.lineage.len(), 3);
    for (index, row) in batch.lineage.iter().enumerate() {
        assert_eq!(row.row_index, u64::try_from(index).unwrap());
        assert_eq!(row.line, lines[index]);
        assert_eq!(row.byte_offset, offsets[index]);
        assert_eq!(row.fields.len(), 5);
        assert_eq!(field_at(row, "名称").line, lines[index]);
        assert_eq!(field_at(row, "note").byte_offset, note_at[index]);
        assert_eq!(field_at(row, "note").presence, presence[index]);
    }
    assert_eq!(field_at(&batch.lineage[2], "名称").presence, DelimitedFieldPresence::Null);
    assert_eq!(field_at(&batch.lineage[0], "note").line, 2);
    assert_eq!(pulled.exhausted.rows, 3);
    assert_eq!(pulled.exhausted.object_sha256, *digest);
    assert_receipt(&pulled.done, 3, 1, *digest);
    assert_eq!(pulled.done.columns, 5);
    assert_eq!(pulled.done.profile_id, "enterprise-import");
}

fn lineage_text(rows: &[DelimitedRowLineage]) -> String {
    let mut out = String::new();
    for row in rows {
        out.push_str(&format!("row={} line={} offset={}\n", row.row_index, row.line, row.byte_offset));
        for field in &row.fields {
            out.push_str(&format!(
                "  {} line={} offset={} presence={:?}\n",
                field.name, field.line, field.byte_offset, field.presence
            ));
        }
    }
    out
}

struct OneByte<R> {
    inner: R,
}

impl<R: Read> Read for OneByte<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut one = [0u8; 1];
        let read = self.inner.read(&mut one)?;
        if read == 1 {
            buf[0] = one[0];
        }
        Ok(read)
    }
}

#[test]
fn tsv_keeps_quoted_tabs_and_unquoted_commas() {
    let bytes = sample_tsv();
    let oracle = oracle_rows(&bytes, b'\t', false).unwrap();
    assert_eq!(oracle.len(), 2);
    assert_eq!(oracle[0].offset, 0);
    assert_eq!(oracle[0].raw_len, 12);
    assert_eq!(oracle[0].fields[0].text, "熊");
    assert_eq!(oracle[0].fields[1].text, "a\tb");
    assert!(oracle[0].fields[1].quoted);
    assert_eq!(oracle[0].fields[1].offset, 4);
    assert_eq!(oracle[0].fields[2].text, "7");
    assert_eq!(oracle[0].fields[2].offset, 10);
    assert_eq!(oracle[1].line, 2);
    assert_eq!(oracle[1].offset, 12);
    assert_eq!(oracle[1].raw_len, 17);
    assert_eq!(oracle[1].fields[0].text, "猫,鼠");
    assert_eq!(oracle[1].fields[1].text, "上海");
    assert_eq!(oracle[1].fields[1].offset, 20);
    assert_eq!(oracle[1].fields[2].text, "-0");
    assert_eq!(oracle[1].fields[2].offset, 27);
    let profile = profile(
        DelimitedDialect::Tsv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        vec![
            col("name", DelimitedType::Utf8, false),
            col("raw", DelimitedType::Utf8, false),
            col("n", DelimitedType::Int64, false),
        ],
    );
    let digest = trusted_sha256(&bytes);
    let pulled = pull(open_bytes(&bytes, profile.clone(), budget()));
    let batch = &pulled.batches[0];
    assert_eq!(utf8_col(&batch.record_batch, 0), vec![Some("熊".to_owned()), Some("猫,鼠".to_owned())]);
    assert_eq!(utf8_col(&batch.record_batch, 1), vec![Some("a\tb".to_owned()), Some("上海".to_owned())]);
    assert_eq!(i64_col(&batch.record_batch, 2), vec![Some(7), Some(0)]);
    assert_eq!(batch.lineage[0].byte_offset, 0);
    assert_eq!(batch.lineage[1].line, 2);
    assert_eq!(batch.lineage[1].byte_offset, 12);
    assert_eq!(field_at(&batch.lineage[0], "raw").byte_offset, 4);
    assert_receipt(&pulled.done, 2, 1, digest);
    dump_oracle("positive-tsv", &bytes, &profile.profile_preimage(), digest, &lineage_text(&batch.lineage));
}

#[test]
fn empty_null_policy_distinguishes_quoted_empty_from_null() {
    let bytes = b"\"\",\r\n";
    let profile = profile(
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        vec![col("text", DelimitedType::Utf8, true), col("flag", DelimitedType::Bool, true)],
    );
    let pulled = pull(open_bytes(bytes, profile, budget()));
    let batch = &pulled.batches[0];
    assert_eq!(utf8_col(&batch.record_batch, 0), vec![Some(String::new())]);
    assert_eq!(bool_col(&batch.record_batch, 1), vec![None]);
    assert_eq!(field_at(&batch.lineage[0], "text").presence, DelimitedFieldPresence::Empty);
    assert_eq!(field_at(&batch.lineage[0], "flag").presence, DelimitedFieldPresence::Null);
}

#[test]
fn quoted_null_token_stays_literal_text() {
    let bytes = b"\"\\N\"\n";
    let profile = profile(
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Token("\\N".to_owned()),
        vec![col("text", DelimitedType::Utf8, true)],
    );
    let pulled = pull(open_bytes(bytes, profile.clone(), budget()));
    assert_eq!(utf8_col(&pulled.batches[0].record_batch, 0), vec![Some("\\N".to_owned())]);
    assert_eq!(field_at(&pulled.batches[0].lineage[0], "text").presence, DelimitedFieldPresence::Value);
    let empty = pull(open_bytes(b"\n", profile, budget()));
    assert_eq!(utf8_col(&empty.batches[0].record_batch, 0), vec![Some(String::new())]);
    assert_eq!(field_at(&empty.batches[0].lineage[0], "text").presence, DelimitedFieldPresence::Empty);
}

#[test]
fn negative_scale_decimal_strips_zeros_only() {
    let profile = profile(
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        vec![col("amount", DelimitedType::Decimal { precision: 10, scale: -2 }, false)],
    );
    let bytes = b"1200\n";
    let pulled = pull(open_bytes(bytes, profile.clone(), budget()));
    assert_eq!(dec_col(&pulled.batches[0].record_batch, 0), vec![Some(12)]);
    assert_eq!(
        pulled.batches[0].record_batch.schema().field(0).data_type(),
        &DataType::Decimal128(10, -2)
    );
    assert_eq!(
        pulled.batches[0].record_batch.schema().field(0).metadata().get("oo.delimited.scale").unwrap(),
        "-2"
    );
    let err = refuse(b"12\n", profile, budget());
    assert_eq!(err.kind(), DelimitedErrorKind::Protocol);
    assert_eq!(err.fact(), "inexact_decimal");
}

#[test]
fn signed_decimal_and_lexical_rejections_drop_the_cell() {
    let decimal = profile(
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        vec![col("amount", DelimitedType::Decimal { precision: 10, scale: 2 }, false)],
    );
    let pulled = pull(open_bytes(b"-1.20\n", decimal.clone(), budget()));
    assert_eq!(dec_col(&pulled.batches[0].record_batch, 0), vec![Some(-120)]);
    for (text, fact) in [
        ("+1.20\n", "not_decimal"),
        (" 1.20\n", "not_decimal"),
        ("1.\n", "not_decimal"),
        ("1e2\n", "not_decimal"),
        ("1.251\n", "inexact_decimal"),
        ("100000000.00\n", "decimal_precision"),
    ] {
        let err = refuse(text.as_bytes(), decimal.clone(), budget());
        assert_eq!(err.fact(), fact, "{text}");
        assert_closed(&err, &[text.trim()]);
    }
    let integer = profile(
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        vec![col("n", DelimitedType::Int64, false)],
    );
    for (text, fact) in [
        ("+1\n", "not_int64"),
        ("01\n", "not_int64"),
        ("1.0\n", "not_int64"),
        (" 1\n", "not_int64"),
        ("1e2\n", "not_int64"),
        ("9223372036854775808\n", "int64_overflow"),
        ("-9223372036854775809\n", "int64_overflow"),
    ] {
        let err = refuse(text.as_bytes(), integer.clone(), budget());
        assert_eq!(err.kind(), DelimitedErrorKind::Protocol);
        assert_eq!(err.fact(), fact, "{text}");
        assert_closed(&err, &[text.trim()]);
    }
    let flags = profile(
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        vec![col("ok", DelimitedType::Bool, false)],
    );
    for text in ["TRUE\n", "yes\n", "1\n", "true \n"] {
        let err = refuse(text.as_bytes(), flags.clone(), budget());
        assert_eq!(err.fact(), "not_bool");
        assert_closed(&err, &[text.trim()]);
    }
}

#[test]
fn number_budget_is_closed() {
    let mut text = "9".repeat(8193);
    text.push('\n');
    let profile = profile(
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        vec![col("n", DelimitedType::Int64, false)],
    );
    let wide = budget_with(1, 1024 * 1024, 16_384, 16_384, 16_384, 1);
    let err = refuse(text.as_bytes(), profile, wide);
    assert_eq!(err.kind(), DelimitedErrorKind::Budget);
    assert_eq!(err.fact(), "number_budget");
    assert_closed(&err, &["999999"]);
}

#[test]
fn malformed_quotes_encoding_and_controls_hide_the_bytes() {
    let profile = text_profile(HeaderPolicy::Absent, BomPolicy::Refuse);
    let err = refuse(b"\"MARKER_SECRET_a192", profile.clone(), budget());
    assert_eq!(err.kind(), DelimitedErrorKind::Protocol);
    assert_eq!(err.fact(), "malformed_quote");
    assert_closed(&err, &["MARKER_SECRET_a192"]);
    let err = refuse(b"\"a\"MARKER_SECRET_a192\n", profile.clone(), budget());
    assert_eq!(err.fact(), "malformed_quote");
    assert_closed(&err, &["MARKER_SECRET_a192"]);
    let err = refuse(b"\xff\n", profile.clone(), budget());
    assert_eq!(err.kind(), DelimitedErrorKind::Encoding);
    assert_eq!(err.fact(), "utf8");
    let err = refuse(b"a\x00b\n", profile.clone(), budget());
    assert_eq!(err.fact(), "control_char");
    let err = refuse(b"a\x01b\n", profile.clone(), budget());
    assert_eq!(err.fact(), "control_char");
    let err = refuse(b"hello\r", profile, budget());
    assert_eq!(err.fact(), "bare_cr");
}

#[test]
fn quoted_controls_that_are_legal_stay_in_the_field() {
    let profile = text_profile(HeaderPolicy::Absent, BomPolicy::Refuse);
    let pulled = pull(open_bytes(b"\"a\rb\"\n", profile.clone(), budget()));
    assert_eq!(utf8_col(&pulled.batches[0].record_batch, 0), vec![Some("a\rb".to_owned())]);
    assert_eq!(pulled.batches[0].lineage[0].line, 1);
    let pulled = pull(open_bytes("a\tb\n".as_bytes(), profile, budget()));
    assert_eq!(utf8_col(&pulled.batches[0].record_batch, 0), vec![Some("a\tb".to_owned())]);
}

#[test]
fn width_header_and_bom_failures_are_closed() {
    let one = text_profile(HeaderPolicy::Absent, BomPolicy::Refuse);
    let err = refuse(b"a,b\n", one, budget());
    assert_eq!(err.fact(), "width_mismatch");
    let exact = text_profile(HeaderPolicy::Exact, BomPolicy::Refuse);
    let err = refuse(b"wrong\n", exact.clone(), budget());
    assert_eq!(err.fact(), "header_mismatch");
    let err = refuse(b"a,b\n", exact.clone(), budget());
    assert_eq!(err.fact(), "width_mismatch");
    let mut reader = DelimitedReader::open(Cursor::new(Vec::<u8>::new()), exact, [0u8; 32], budget()).unwrap();
    let err = reader.next_batch().unwrap_err();
    assert_eq!(err.fact(), "header_required");
    assert_eq!(reader.finish().unwrap_err().fact(), "header_required");

    let bom = [0xEF, 0xBB, 0xBF];
    let mut refused = bom.to_vec();
    refused.extend_from_slice(b"a\n");
    let err = refuse(&refused, text_profile(HeaderPolicy::Absent, BomPolicy::Refuse), budget());
    assert_eq!(err.kind(), DelimitedErrorKind::Encoding);
    assert_eq!(err.fact(), "bom_refused");
    let err = refuse(b"a\n", text_profile(HeaderPolicy::Absent, BomPolicy::Require), budget());
    assert_eq!(err.fact(), "bom_required");
}

#[test]
fn bom_require_and_exact_header_shift_lineage() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice("\"名称\"\n熊\n".as_bytes());
    let profile = profile(
        DelimitedDialect::Csv,
        BomPolicy::Require,
        HeaderPolicy::Exact,
        NullPolicy::Empty,
        vec![col("名称", DelimitedType::Utf8, false)],
    );
    let pulled = pull(open_bytes(&bytes, profile, budget()));
    assert_eq!(utf8_col(&pulled.batches[0].record_batch, 0), vec![Some("熊".to_owned())]);
    assert_eq!(pulled.batches[0].lineage[0].line, 2);
    let header_len = u64::try_from("\"名称\"\n".len()).unwrap();
    assert_eq!(pulled.batches[0].lineage[0].byte_offset, 3 + header_len);
    assert_eq!(pulled.done.rows, 1);
}

#[test]
fn empty_absent_header_completes_and_a_bad_hash_does_not() {
    let profile = text_profile(HeaderPolicy::Absent, BomPolicy::Refuse);
    let digest = trusted_sha256(b"");
    let pulled = pull(DelimitedReader::open(Cursor::new(Vec::<u8>::new()), profile.clone(), digest, budget()).unwrap());
    assert!(pulled.batches.is_empty());
    assert_receipt(&pulled.done, 0, 0, digest);
    let mut reader = DelimitedReader::open(Cursor::new(Vec::<u8>::new()), profile, [0u8; 32], budget()).unwrap();
    let err = reader.next_batch().unwrap_err();
    assert_eq!(err.kind(), DelimitedErrorKind::Hash);
    assert_eq!(err.fact(), "hash_mismatch");
    assert_eq!(reader.finish().unwrap_err().fact(), "hash_mismatch");
}

#[test]
fn header_only_completes_with_zero_data_rows() {
    let bytes = "name\n".as_bytes();
    let profile = text_profile(HeaderPolicy::Exact, BomPolicy::Refuse);
    let pulled = pull(open_bytes(bytes, profile, budget()));
    assert!(pulled.batches.is_empty());
    assert_eq!(pulled.done.rows, 0);
    assert_eq!(pulled.done.object_sha256, trusted_sha256(bytes));
}

#[test]
fn hash_mismatch_returns_the_batch_then_refuses_completion() {
    let bytes = "熊\n".as_bytes();
    let profile = text_profile(HeaderPolicy::Absent, BomPolicy::Refuse);
    let mut reader = DelimitedReader::open(Cursor::new(bytes.to_vec()), profile, [9u8; 32], budget()).unwrap();
    let item = reader.next_batch().unwrap();
    let DelimitedItem::Batch(batch) = item else {
        panic!("pending batch was dropped");
    };
    assert_eq!(utf8_col(&batch.record_batch, 0), vec![Some("熊".to_owned())]);
    let err = reader.next_batch().unwrap_err();
    assert_eq!(err.kind(), DelimitedErrorKind::Hash);
    assert_eq!(err.fact(), "hash_mismatch");
    assert_closed(&err, &["熊"]);
    let finished = reader.finish().unwrap_err();
    assert_eq!(finished.fact(), "hash_mismatch");
}

#[test]
fn early_finish_does_not_mint_completion() {
    let profile = text_profile(HeaderPolicy::Absent, BomPolicy::Refuse);
    let reader = open_bytes(b"a\nb\n", profile.clone(), budget());
    assert_eq!(reader.finish().unwrap_err().fact(), "not_exhausted");
    let mut reader = open_bytes(b"a\nb\n", profile, budget());
    assert!(matches!(reader.next_batch().unwrap(), DelimitedItem::Batch(_)));
    assert_eq!(reader.finish().unwrap_err().fact(), "not_exhausted");
}

#[test]
fn record_field_and_total_budgets_stop_before_completion() {
    let profile = text_profile(HeaderPolicy::Absent, BomPolicy::Refuse);
    let tight = budget_with(1, 1024, 8, 8, 64, 10);
    let err = refuse(b"\"abcdef\"\n", profile.clone(), tight);
    assert_eq!(err.kind(), DelimitedErrorKind::Budget);
    assert_eq!(err.fact(), "max_record");
    assert_closed(&err, &["abcdef"]);
    let fields = budget_with(1, 1024, 32, 4, 64, 10);
    let err = refuse(b"abcdef\n", profile.clone(), fields);
    assert_eq!(err.fact(), "max_field");
    assert_closed(&err, &["abcdef"]);
    let tape_budget = budget_with(4, 1024, 4, 4, 4, 10);
    let err = refuse(b"ab\nXY", profile, tape_budget);
    assert_eq!(err.fact(), "max_total");
}

#[test]
fn batch_caps_rewind_or_refuse() {
    let profile = text_profile(HeaderPolicy::Absent, BomPolicy::Refuse);
    let split = budget_with(1, 1024, 64, 64, 1024, 10);
    let pulled = pull(open_bytes(b"a\nb\n", profile.clone(), split));
    assert_eq!(pulled.batches.len(), 2);
    assert_eq!(pulled.batches[0].lineage[0].row_index, 0);
    assert_eq!(pulled.batches[1].lineage[0].row_index, 1);
    assert_eq!(utf8_col(&pulled.batches[1].record_batch, 0), vec![Some("b".to_owned())]);
    let bytes = budget_with(8, 3, 64, 64, 1024, 10);
    let pulled = pull(open_bytes(b"aa\nb\n", profile.clone(), bytes));
    assert_eq!(pulled.batches.len(), 2);
    assert_eq!(pulled.batches[0].raw_bytes, 3);
    assert_eq!(utf8_col(&pulled.batches[0].record_batch, 0), vec![Some("aa".to_owned())]);
    let tiny = budget_with(8, 1, 64, 64, 1024, 10);
    let err = refuse(b"aa\n", profile.clone(), tiny);
    assert_eq!(err.fact(), "batch_bytes");
    let rows = budget_with(8, 1024, 64, 64, 1024, 1);
    let mut reader = open_bytes(b"a\nb\n", profile.clone(), rows);
    assert!(matches!(reader.next_batch().unwrap(), DelimitedItem::Batch(_)));
    assert_eq!(reader.next_batch().unwrap_err().fact(), "max_rows");
    assert_eq!(reader.finish().unwrap_err().fact(), "max_rows");
    let narrow = DelimitedBudget::try_new(
        4,
        1024,
        64,
        64,
        1024,
        4,
        1,
        1,
        Instant::now() + Duration::from_secs(60),
    )
    .unwrap();
    let err = refuse(b"a,b\n", profile, narrow);
    assert_eq!(err.fact(), "max_fields");
}

#[test]
fn cancel_before_a_read_and_a_blocked_read_refuse_completion() {
    let reads = Arc::new(AtomicUsize::new(0));
    let profile = text_profile(HeaderPolicy::Absent, BomPolicy::Refuse);
    let counted = Counted {
        data: b"a\n".to_vec(),
        pos: 0,
        reads: Arc::clone(&reads),
    };
    let digest = trusted_sha256(b"a\n");
    let mut reader = DelimitedReader::open(counted, profile.clone(), digest, budget()).unwrap();
    reader.cancel_handle().request();
    let err = reader.next_batch().unwrap_err();
    assert_eq!(err.fact(), "cancelled");
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(reader.finish().unwrap_err().fact(), "cancelled");

    // try_new accepts a deadline that is already due. open rejects it before a read.
    let due = DelimitedBudget::try_new(1, 16, 8, 8, 16, 1, 1, 1, Instant::now()).unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    let err = DelimitedReader::open(
        Counted { data: b"a\n".to_vec(), pos: 0, reads: Arc::clone(&reads) },
        text_profile(HeaderPolicy::Absent, BomPolicy::Refuse),
        digest,
        due,
    )
    .unwrap_err();
    assert_eq!(err.kind(), DelimitedErrorKind::Budget);
    assert_eq!(err.fact(), "deadline");
    assert_eq!(reads.load(Ordering::SeqCst), 0);

    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let reads = Arc::new(AtomicUsize::new(0));
    let hold = Hold {
        data: b"a\n".to_vec(),
        pos: 0,
        blocked: false,
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
        reads: Arc::clone(&reads),
    };
    let mut reader = DelimitedReader::open(hold, profile, digest, budget()).unwrap();
    let cancel = reader.cancel_handle();
    let (tx, rx) = mpsc::channel();
    let done_flag = Arc::clone(&done);
    thread::spawn(move || {
        let pulled = match reader.next_batch() {
            Ok(_) => "returned",
            Err(err) => err.fact(),
        };
        let finished = match reader.finish() {
            Ok(_) => "complete",
            Err(err) => err.fact(),
        };
        done_flag.store(true, Ordering::SeqCst);
        tx.send((pulled, finished)).unwrap();
    });
    let start = Instant::now();
    while !entered.load(Ordering::SeqCst) {
        assert!(start.elapsed() < Duration::from_secs(5), "read did not start");
        thread::sleep(Duration::from_millis(1));
    }
    cancel.request();
    thread::sleep(Duration::from_millis(50));
    assert!(!done.load(Ordering::SeqCst), "next_batch returned while the read was blocked");
    release.store(true, Ordering::SeqCst);
    let (pulled, finished) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(pulled, "cancelled");
    assert_eq!(finished, "cancelled");
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[test]
fn drop_releases_the_reader_and_debug_hides_cells() {
    let flag = Arc::new(AtomicBool::new(false));
    let profile = text_profile(HeaderPolicy::Absent, BomPolicy::Refuse);
    let reader = DelimitedReader::open(
        FlagRead { flag: Arc::clone(&flag), data: b"\xe7\x86\x8a\n".to_vec(), pos: 0 },
        profile,
        trusted_sha256("熊\n".as_bytes()),
        budget(),
    )
    .unwrap();
    let shown = format!("{reader:?}");
    assert!(!shown.contains("熊"));
    drop(reader);
    assert!(flag.load(Ordering::SeqCst));
}

#[test]
fn io_errors_drop_the_os_message() {
    let profile = text_profile(HeaderPolicy::Absent, BomPolicy::Refuse);
    let mut reader = DelimitedReader::open(
        SecretRead,
        profile,
        trusted_sha256(b""),
        budget(),
    )
    .unwrap();
    let err = reader.next_batch().unwrap_err();
    assert_eq!(err.kind(), DelimitedErrorKind::Io);
    assert_eq!(err.fact(), "read");
    assert_closed(&err, &["SECRET_OS_a192"]);
    assert_closed(&reader.finish().unwrap_err(), &["SECRET_OS_a192"]);
}

#[test]
fn open_does_not_read() {
    let reads = Arc::new(AtomicUsize::new(0));
    let profile = text_profile(HeaderPolicy::Absent, BomPolicy::Refuse);
    let reader = DelimitedReader::open(
        Counted { data: b"a\n".to_vec(), pos: 0, reads: Arc::clone(&reads) },
        profile,
        trusted_sha256(b"a\n"),
        budget(),
    )
    .unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(reader.header().columns, vec!["name".to_owned()]);
    drop(reader);
}

struct Counted {
    data: Vec<u8>,
    pos: usize,
    reads: Arc<AtomicUsize>,
}

impl Read for Counted {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if buf.is_empty() || self.pos >= self.data.len() {
            return Ok(0);
        }
        let count = buf.len().min(self.data.len() - self.pos);
        buf[..count].copy_from_slice(&self.data[self.pos..self.pos + count]);
        self.pos += count;
        Ok(count)
    }
}

struct Hold {
    data: Vec<u8>,
    pos: usize,
    blocked: bool,
    entered: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
    reads: Arc<AtomicUsize>,
}

impl Read for Hold {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if !self.blocked {
            self.blocked = true;
            self.entered.store(true, Ordering::SeqCst);
            while !self.release.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(1));
            }
        }
        if buf.is_empty() || self.pos >= self.data.len() {
            return Ok(0);
        }
        buf[0] = self.data[self.pos];
        self.pos += 1;
        Ok(1)
    }
}

struct FlagRead {
    flag: Arc<AtomicBool>,
    data: Vec<u8>,
    pos: usize,
}

impl Read for FlagRead {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() || self.pos >= self.data.len() {
            return Ok(0);
        }
        buf[0] = self.data[self.pos];
        self.pos += 1;
        Ok(1)
    }
}

impl Drop for FlagRead {
    fn drop(&mut self) {
        self.flag.store(true, Ordering::SeqCst);
    }
}

struct SecretRead;

impl Read for SecretRead {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("SECRET_OS_a192"))
    }
}
