//! Independent review of `oo.source.jsonl/v1` at the fixed reader.
//!
//! Hashes, profile preimages, and lineage positions are computed in this
//! file with `sha2` and byte scans. They do not call a library hasher.
//! The success input is a real temporary file read in short chunks.

#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use arrow_array::{Array, BooleanArray, Int64Array, RecordBatch, StringArray};
use arrow_schema::DataType;
use oo_connectors::{
    FieldPresence, JsonlBudget, JsonlColumn, JsonlError, JsonlItem, JsonlProfile, JsonlReader,
    JsonlType, JSONL_LINEAGE_VERSION, JSONL_PROFILE_VERSION, JSONL_PROTOCOL, JSONL_RECEIPT_VERSION,
};
use sha2::{Digest, Sha256};

const ROW0: &str = r#"{"name":"ann","ok":true,"n":-9223372036854775808,"j":{"z":1, "b":[true, null]}}"#;
const ROW1: &str = r#"{"j":"kept","n":9223372036854775807,"ok":false,"name":"bob"}"#;
const ROW2: &str = r#"{"name":null,"ok":true,"n":9007199254740993}"#;
const NESTED: &str = r#"{"z":1, "b":[true, null]}"#;
const EXACT: i64 = 9_007_199_254_740_993;
const PROFILE_ID: &str = "review";

const PREIMAGE_COLUMNS: &[(&str, &str, char)] = &[
    ("name", "utf8", '1'),
    ("ok", "bool", '0'),
    ("n", "int64", '0'),
    ("j", "json_text", '1'),
];

fn rust_sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
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

fn parse_hex64(text: &str) -> [u8; 32] {
    let token = text.split_whitespace().next().expect("sha256 token");
    assert_eq!(token.len(), 64, "sha256 width");
    let mut out = [0u8; 32];
    for (index, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&token[index * 2..index * 2 + 2], 16).expect("hex");
    }
    out
}

fn sha256sum_file(path: &Path) -> [u8; 32] {
    let output = Command::new("sha256sum")
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .expect("sha256sum");
    assert!(output.status.success(), "sha256sum status {}", output.status);
    parse_hex64(&String::from_utf8(output.stdout).expect("sha256sum utf8"))
}

fn profile_preimage() -> String {
    let mut body = String::from("oo-jsonl-profile/v1\n");
    body.push_str("id=");
    body.push_str(PROFILE_ID);
    body.push('\n');
    for (name, ty, bit) in PREIMAGE_COLUMNS {
        body.push_str("column=");
        body.push_str(name);
        body.push('\t');
        body.push_str(ty);
        body.push('\t');
        body.push(*bit);
        body.push('\n');
    }
    body
}

fn config_preimage(profile_body: &str, expected: &[u8; 32]) -> String {
    let mut body = String::from("oo-jsonl-config-identity/v1\n");
    body.push_str(profile_body);
    body.push_str("expected_sha256=");
    body.push_str(&hex(expected));
    body.push('\n');
    body
}

fn success_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(ROW0.as_bytes());
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(ROW1.as_bytes());
    out.extend_from_slice(b"\n");
    out.extend_from_slice(ROW2.as_bytes());
    out
}

fn line_of(bytes: &[u8], offset: usize) -> u64 {
    let breaks = bytes[..offset].iter().filter(|byte| **byte == b'\n').count();
    u64::try_from(breaks).expect("line") + 1
}

fn record_starts(bytes: &[u8]) -> Vec<usize> {
    let mut starts = Vec::new();
    if bytes.first() == Some(&b'{') {
        starts.push(0);
    }
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'\n' && bytes[index + 1] == b'{' {
            starts.push(index + 1);
        }
        index += 1;
    }
    starts
}

fn find_nth(bytes: &[u8], needle: &[u8], occurrence: usize) -> usize {
    let mut from = 0;
    let mut seen = 0;
    while from + needle.len() <= bytes.len() {
        if &bytes[from..from + needle.len()] == needle {
            if seen == occurrence {
                return from;
            }
            seen += 1;
        }
        from += 1;
    }
    panic!("missing needle {}", String::from_utf8_lossy(needle));
}

fn publish(name: &str, bytes: &[u8]) {
    let Ok(dir) = std::env::var("OO_JSONL_REVIEW_ORACLE_DIR") else {
        return;
    };
    let path = PathBuf::from(dir);
    fs::create_dir_all(&path).expect("oracle dir");
    let mut file = File::create(path.join(name)).expect("oracle file");
    file.write_all(bytes).expect("oracle write");
}

struct Counted<R> {
    inner: R,
    reads: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
}

impl<R> Drop for Counted<R> {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.read(buf)
    }
}

struct Short<R> {
    inner: R,
    chunk: usize,
}

impl<R: Read> Read for Short<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf.len().min(self.chunk);
        if n == 0 {
            return Ok(0);
        }
        self.inner.read(&mut buf[..n])
    }
}

struct Prepared {
    input: Short<Counted<File>>,
    hash: [u8; 32],
    reads: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
    path: PathBuf,
}

fn prepare(bytes: &[u8]) -> Prepared {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("oo-jsonl-a185-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join(format!("{}.jsonl", NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, bytes).expect("temp file");
    let reads = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicBool::new(false));
    let input = Short {
        inner: Counted {
            inner: File::open(&path).expect("open temp"),
            reads: Arc::clone(&reads),
            dropped: Arc::clone(&dropped),
        },
        chunk: 3,
    };
    Prepared {
        input,
        hash: rust_sha256(bytes),
        reads,
        dropped,
        path,
    }
}

fn wide_deadline() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

#[allow(clippy::too_many_arguments)]
fn budget(
    batch_rows: u32,
    batch_bytes: u64,
    max_record: u32,
    max_total: u64,
    max_rows: u64,
    max_columns: u32,
    max_fields: u32,
    deadline: Instant,
) -> JsonlBudget {
    JsonlBudget::try_new(
        batch_rows,
        batch_bytes,
        max_record,
        max_total,
        max_rows,
        max_columns,
        max_fields,
        deadline,
    )
    .expect("budget")
}

fn loose() -> JsonlBudget {
    budget(8, 1 << 20, 4096, 1 << 20, 1000, 16, 16, wide_deadline())
}

fn column(name: &str, ty: JsonlType, nullable: bool) -> JsonlColumn {
    JsonlColumn::try_new(name, ty, nullable).expect("column")
}

fn review_profile() -> JsonlProfile {
    JsonlProfile::try_new(
        PROFILE_ID,
        vec![
            column("name", JsonlType::Utf8, true),
            column("ok", JsonlType::Bool, false),
            column("n", JsonlType::Int64, false),
            column("j", JsonlType::JsonText, true),
        ],
    )
    .expect("profile")
}

fn int_profile() -> JsonlProfile {
    JsonlProfile::try_new("nums", vec![column("n", JsonlType::Int64, false)]).expect("int profile")
}

fn strings(batch: &RecordBatch, index: usize) -> &StringArray {
    batch
        .column(index)
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("string array")
}

fn bools(batch: &RecordBatch, index: usize) -> &BooleanArray {
    batch
        .column(index)
        .as_any()
        .downcast_ref::<BooleanArray>()
        .expect("bool array")
}

fn ints(batch: &RecordBatch, index: usize) -> &Int64Array {
    batch
        .column(index)
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("int array")
}

fn refuse(bytes: &[u8], profile: JsonlProfile, fact: &str) -> JsonlError {
    let prepared = prepare(bytes);
    let mut reader = JsonlReader::open(prepared.input, profile, prepared.hash, loose()).expect("open");
    let err = loop {
        match reader.next_batch() {
            Ok(JsonlItem::Batch(_)) => {}
            Ok(JsonlItem::Exhausted(_)) => panic!("exhausted for {fact}"),
            Err(err) => break err,
        }
    };
    assert_eq!(err.fact(), fact, "{err}");
    let finished = reader.finish().expect_err("finish after refusal");
    assert_eq!(finished, err);
    assert!(!finished.to_string().contains("SHOULD_NOT_LEAK"));
    err
}

struct Hold {
    data: Vec<u8>,
    pos: usize,
    held: bool,
    entered: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
    slept_ms: Arc<AtomicU64>,
}

impl Read for Hold {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        if !self.held {
            self.held = true;
            self.entered.store(true, Ordering::SeqCst);
            let start = Instant::now();
            while !self.release.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(20));
            }
            let elapsed = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            self.slept_ms.store(elapsed, Ordering::SeqCst);
        }
        let n = buf.len().min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

struct Nap {
    data: Vec<u8>,
    pos: usize,
    napped: bool,
    nap: Duration,
    slept_ms: Arc<AtomicU64>,
}

impl Read for Nap {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        if !self.napped {
            self.napped = true;
            let start = Instant::now();
            thread::sleep(self.nap);
            let elapsed = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            self.slept_ms.store(elapsed, Ordering::SeqCst);
        }
        let n = buf.len().min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

struct Release(Arc<AtomicBool>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[test]
fn explicit_profile_projects_arrow_and_matches_oracle_only_at_eof() {
    let bytes = success_bytes();
    let starts = record_starts(&bytes);
    assert_eq!(starts.len(), 3, "oracle record starts");
    assert_eq!(&bytes[starts[0]..starts[0] + ROW0.len()], ROW0.as_bytes());
    assert_eq!(&bytes[starts[1] - 2..starts[1]], b"\r\n");
    assert_eq!(&bytes[starts[1]..starts[1] + ROW1.len()], ROW1.as_bytes());
    assert_eq!(&bytes[starts[2] - 1..starts[2]], b"\n");
    assert_eq!(&bytes[starts[2]..], ROW2.as_bytes());
    assert_eq!(line_of(&bytes, starts[0]), 1);
    assert_eq!(line_of(&bytes, starts[1]), 2);
    assert_eq!(line_of(&bytes, starts[2]), 3);
    assert!(bytes.ends_with(b"}"), "final record has no trailing newline");
    assert!(ROW0.contains(NESTED));

    let lossy = format!("{}", EXACT as f64);
    assert_ne!(lossy, EXACT.to_string(), "oracle f64 drops the exact integer");

    let prepared = prepare(&bytes);
    let file_sha = sha256sum_file(&prepared.path);
    assert_eq!(file_sha, prepared.hash, "raw file sha256");
    assert_eq!(file_sha, rust_sha256(&bytes));
    let prefix = ROW0.as_bytes();
    assert_ne!(rust_sha256(prefix), prepared.hash, "prefix is not the object");

    let profile_body = profile_preimage();
    let config_body = config_preimage(&profile_body, &prepared.hash);
    let profile_digest = rust_sha256(profile_body.as_bytes());
    let config_digest = rust_sha256(config_body.as_bytes());
    publish("fixture.jsonl", &bytes);
    publish("fixture.sha256", format!("{}\n", hex(&prepared.hash)).as_bytes());
    publish("profile-preimage.txt", profile_body.as_bytes());
    publish("config-preimage.txt", config_body.as_bytes());
    println!("oracle_file_sha256={}", hex(&prepared.hash));
    println!("oracle_profile_sha256={}", hex(&profile_digest));
    println!("oracle_config_sha256={}", hex(&config_digest));

    let profile = review_profile();
    for (index, (name, ty, bit)) in PREIMAGE_COLUMNS.iter().enumerate() {
        let column = &profile.columns()[index];
        assert_eq!(column.name(), *name);
        assert_eq!(column.ty().as_str(), *ty);
        assert_eq!(column.nullable(), *bit == '1');
    }
    assert_eq!(profile.profile_digest(), profile_digest);
    assert_eq!(profile.config_identity(prepared.hash), config_digest);
    let rendered = format!("{profile:?}").to_ascii_lowercase();
    assert!(!rendered.contains("pii"), "{rendered}");
    assert!(!rendered.contains("semantic"), "{rendered}");

    let other_budget = budget(1, 64, 128, 256, 4, 8, 8, wide_deadline());
    assert_eq!(other_budget.queue_batches(), 1);
    assert_eq!(profile.config_identity(prepared.hash), config_digest);

    let reads = Arc::clone(&prepared.reads);
    let dropped = Arc::clone(&prepared.dropped);
    let hash = prepared.hash;
    let mut reader = JsonlReader::open(prepared.input, profile, hash, loose()).expect("open");
    assert_eq!(reads.load(Ordering::SeqCst), 0, "open does not read");
    let header = reader.header();
    assert_eq!(header.protocol, JSONL_PROTOCOL);
    assert_eq!(header.profile_version, JSONL_PROFILE_VERSION);
    assert_eq!(header.lineage_version, JSONL_LINEAGE_VERSION);
    assert_eq!(header.receipt_version, JSONL_RECEIPT_VERSION);
    assert_eq!(header.profile_digest, profile_digest);
    assert_eq!(header.config_identity, config_digest);
    assert!(!header.admits_import);
    assert!(!header.published);
    assert!(!header.arrow_ipc);
    assert!(!header.executes_native);

    let batch = match reader.next_batch().expect("batch") {
        JsonlItem::Batch(batch) => batch,
        JsonlItem::Exhausted(_) => panic!("first pull exhausted"),
    };
    assert!(reads.load(Ordering::SeqCst) > 1, "short chunks issued more than one read");
    assert_eq!(batch.batch_index, 0);
    assert!(!batch.resumable);
    assert_eq!(batch.record_batch.num_rows(), 3);
    assert_eq!(batch.raw_bytes, (ROW0.len() + ROW1.len() + ROW2.len()) as u64);
    assert_eq!(batch.header.profile_digest, profile_digest);
    assert!(!batch.header.admits_import);
    assert!(!batch.header.arrow_ipc);
    assert!(!batch.header.executes_native);

    let schema = batch.record_batch.schema();
    assert_eq!(schema.field(0).name(), "name");
    assert_eq!(schema.field(0).data_type(), &DataType::Utf8);
    assert!(schema.field(0).is_nullable());
    assert_eq!(schema.field(1).data_type(), &DataType::Boolean);
    assert!(!schema.field(1).is_nullable());
    assert_eq!(schema.field(2).data_type(), &DataType::Int64);
    assert!(!schema.field(2).is_nullable());
    assert_eq!(schema.field(3).name(), "j");
    assert_eq!(schema.field(3).data_type(), &DataType::Utf8);
    assert!(schema.field(3).is_nullable());
    assert_eq!(
        schema.field(3).metadata().get("oo.jsonl.logical").map(String::as_str),
        Some("json_text")
    );
    assert_eq!(
        schema.field(3).metadata().get("oo.jsonl.encoding").map(String::as_str),
        Some("raw-json-utf8")
    );
    assert_ne!(schema.field(2).data_type(), &DataType::Float64);

    let names = strings(&batch.record_batch, 0);
    let flags = bools(&batch.record_batch, 1);
    let numbers = ints(&batch.record_batch, 2);
    let jsons = strings(&batch.record_batch, 3);
    assert_eq!(names.value(0), "ann");
    assert_eq!(names.value(1), "bob");
    assert!(names.is_null(2));
    assert!(flags.value(0));
    assert!(!flags.value(1));
    assert!(flags.value(2));
    assert_eq!(numbers.value(0), i64::MIN);
    assert_eq!(numbers.value(1), i64::MAX);
    assert_eq!(numbers.value(2), EXACT);
    assert_eq!(jsons.value(0), NESTED);
    assert_eq!(jsons.value(1), "\"kept\"");
    assert!(jsons.is_null(2));

    assert_eq!(batch.lineage.len(), 3);
    let name_key = |n| find_nth(&bytes, b"\"name\"", n);
    let ok_key = |n| find_nth(&bytes, b"\"ok\"", n);
    let n_key = |n| find_nth(&bytes, b"\"n\"", n);
    let j_key = |n| find_nth(&bytes, b"\"j\"", n);
    for (row_index, start) in starts.iter().enumerate() {
        let row = &batch.lineage[row_index];
        assert_eq!(row.row_index, row_index as u64);
        assert_eq!(row.line, line_of(&bytes, *start));
        assert_eq!(row.byte_offset, *start as u64);
        assert_eq!(row.fields.len(), 4);
    }
    let row0 = &batch.lineage[0];
    assert_eq!(row0.fields[0].presence, FieldPresence::Value);
    assert_eq!(row0.fields[0].byte_offset, name_key(0) as u64);
    assert_eq!(row0.fields[1].byte_offset, ok_key(0) as u64);
    assert_eq!(row0.fields[2].byte_offset, n_key(0) as u64);
    assert_eq!(row0.fields[3].presence, FieldPresence::Value);
    assert_eq!(row0.fields[3].byte_offset, j_key(0) as u64);
    assert_eq!(row0.fields[3].line, line_of(&bytes, j_key(0)));
    let row1 = &batch.lineage[1];
    assert_eq!(row1.line, 2, "CRLF is one line break");
    assert_eq!(row1.fields[0].byte_offset, name_key(1) as u64);
    assert_eq!(row1.fields[3].byte_offset, j_key(1) as u64);
    let row2 = &batch.lineage[2];
    assert_eq!(row2.line, 3);
    assert_eq!(row2.fields[0].presence, FieldPresence::Null);
    assert_eq!(row2.fields[0].byte_offset, name_key(2) as u64);
    assert_eq!(row2.fields[0].line, line_of(&bytes, name_key(2)));
    assert_eq!(row2.fields[3].presence, FieldPresence::Missing);
    assert_eq!(row2.fields[3].byte_offset, starts[2] as u64);
    assert_eq!(row2.fields[3].line, line_of(&bytes, starts[2]));
    assert_ne!(row2.fields[0].presence, row2.fields[3].presence);

    let early_prepared = prepare(&bytes);
    let early = JsonlReader::open(early_prepared.input, review_profile(), hash, loose()).expect("early");
    assert_eq!(early.finish().expect_err("finish before read").fact(), "not_exhausted");

    match reader.next_batch().expect("eof") {
        JsonlItem::Exhausted(done) => {
            assert_eq!(done.rows, 3);
            assert_eq!(done.batches, 1);
            assert_eq!(done.object_sha256, hash);
            assert_eq!(done.object_sha256, file_sha);
        }
        JsonlItem::Batch(_) => panic!("leftover batch"),
    }
    let token = reader.finish().expect("complete");
    assert_eq!(token.object_sha256, file_sha);
    assert_eq!(token.rows, 3);
    assert_eq!(token.columns, 4);
    assert_eq!(token.batches, 1);
    assert_eq!(token.profile_id, PROFILE_ID);
    assert_eq!(token.profile_digest, profile_digest);
    assert_eq!(token.config_identity, config_digest);
    assert_eq!(token.protocol, JSONL_PROTOCOL);
    assert!(!token.admits_import);
    assert!(!token.published);
    assert!(dropped.load(Ordering::SeqCst));
    println!("complete_sha256={}", hex(&token.object_sha256));
}

#[test]
fn prefix_and_mismatch_do_not_mint() {
    let bytes = success_bytes();
    let prefix_hash = rust_sha256(ROW0.as_bytes());
    assert_ne!(prefix_hash, rust_sha256(&bytes));
    let prepared = prepare(&bytes);
    let mut reader =
        JsonlReader::open(prepared.input, review_profile(), prefix_hash, loose()).expect("open");
    let mut saw_batch = false;
    let err = loop {
        match reader.next_batch() {
            Ok(JsonlItem::Batch(_)) => saw_batch = true,
            Ok(JsonlItem::Exhausted(_)) => panic!("prefix hash exhausted"),
            Err(err) => break err,
        }
    };
    assert!(saw_batch, "a batch can be delivered and still stay untrusted");
    assert_eq!(err.fact(), "hash_mismatch");
    assert_eq!(reader.finish().expect_err("prefix finish").fact(), "hash_mismatch");

    let prepared = prepare(&bytes);
    let mut wrong = prepared.hash;
    wrong[31] ^= 0xff;
    let mut reader = JsonlReader::open(prepared.input, review_profile(), wrong, loose()).expect("open");
    let err = loop {
        match reader.next_batch() {
            Ok(JsonlItem::Batch(_)) => {}
            Ok(JsonlItem::Exhausted(_)) => panic!("flipped hash exhausted"),
            Err(err) => break err,
        }
    };
    assert_eq!(err.fact(), "hash_mismatch");
    assert!(!err.to_string().contains("ann"));
    assert_eq!(reader.finish().expect_err("flipped finish"), err);
}

#[test]
fn partial_poison_and_cancel_do_not_mint() {
    let bytes = success_bytes();
    let prepared = prepare(&bytes);
    let one = budget(1, 1 << 20, 4096, 1 << 20, 1000, 16, 16, wide_deadline());
    let mut reader = JsonlReader::open(prepared.input, review_profile(), prepared.hash, one).expect("open");
    match reader.next_batch().expect("one row") {
        JsonlItem::Batch(batch) => assert_eq!(batch.record_batch.num_rows(), 1),
        JsonlItem::Exhausted(_) => panic!("partial exhausted"),
    }
    assert_eq!(reader.finish().expect_err("partial finish").fact(), "not_exhausted");

    let poison = b"{\"n\":1}\n{\"SHOULD_NOT_LEAK\":2}\n";
    let err = refuse(poison, int_profile(), "unknown_key");
    assert_eq!(err.fact(), "unknown_key");

    let prepared = prepare(&bytes);
    let reads = Arc::clone(&prepared.reads);
    let dropped = Arc::clone(&prepared.dropped);
    let mut reader = JsonlReader::open(prepared.input, review_profile(), prepared.hash, loose()).expect("open");
    reader.cancel_handle().request();
    let err = reader.next_batch().expect_err("cancel before read");
    assert_eq!(err.fact(), "cancelled");
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(reader.finish().expect_err("finish cancelled").fact(), "cancelled");
    assert!(dropped.load(Ordering::SeqCst));

    let prepared = prepare(b"{\"n\":1}\n");
    let dropped = Arc::clone(&prepared.dropped);
    let mut reader = JsonlReader::open(prepared.input, int_profile(), prepared.hash, loose()).expect("open");
    match reader.next_batch().expect("row") {
        JsonlItem::Batch(batch) => assert_eq!(ints(&batch.record_batch, 0).value(0), 1),
        JsonlItem::Exhausted(_) => panic!("cancel setup"),
    }
    let cancelled = reader.cancel();
    assert!(cancelled.released);
    assert_eq!(cancelled.rows_emitted, 1);
    assert_eq!(cancelled.profile_id, "nums");
    assert!(dropped.load(Ordering::SeqCst));

    let prepared = prepare(b"{\"n\":1}");
    let hash = prepared.hash;
    let mut reader = JsonlReader::open(prepared.input, int_profile(), hash, loose()).expect("open");
    match reader.next_batch().expect("batch") {
        JsonlItem::Batch(_) => {}
        JsonlItem::Exhausted(_) => panic!("expected the row first"),
    }
    match reader.next_batch().expect("exhausted") {
        JsonlItem::Exhausted(done) => assert_eq!(done.object_sha256, hash),
        JsonlItem::Batch(_) => panic!("expected eof"),
    }
    reader.cancel_handle().request();
    assert_eq!(reader.finish().expect_err("cancel after eof").fact(), "cancelled");

    let prepared = prepare(b"{\"n\":1}");
    let reads = Arc::clone(&prepared.reads);
    let dropped = Arc::clone(&prepared.dropped);
    let reader = JsonlReader::open(prepared.input, int_profile(), prepared.hash, loose()).expect("open");
    drop(reader);
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[test]
fn bounds_are_enforced_before_read_and_by_backpressure() {
    let prepared = prepare(b"{\"n\":1}\n");
    let reads = Arc::clone(&prepared.reads);
    let dropped = Arc::clone(&prepared.dropped);
    let past = budget(1, 1024, 64, 1024, 10, 4, 4, Instant::now() - Duration::from_secs(5));
    let err = JsonlReader::open(prepared.input, int_profile(), prepared.hash, past).expect_err("past deadline");
    assert_eq!(err.fact(), "deadline");
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert!(dropped.load(Ordering::SeqCst));

    let prepared = prepare(b"{\"n\":1}\n");
    let reads = Arc::clone(&prepared.reads);
    let narrow = budget(1, 1024, 64, 1024, 10, 1, 4, wide_deadline());
    let wide = JsonlProfile::try_new(
        "two",
        vec![column("n", JsonlType::Int64, false), column("ok", JsonlType::Bool, true)],
    )
    .expect("two");
    let err = JsonlReader::open(prepared.input, wide, prepared.hash, narrow).expect_err("columns");
    assert_eq!(err.fact(), "max_columns");
    assert_eq!(reads.load(Ordering::SeqCst), 0);

    let pair = b"{\"n\":1}\n{\"n\":2}";
    assert_eq!(&pair[..7], b"{\"n\":1}");
    let prepared = prepare(pair);
    let split = budget(8, 7, 64, 1024, 10, 4, 4, wide_deadline());
    let mut reader = JsonlReader::open(prepared.input, int_profile(), prepared.hash, split).expect("open");
    match reader.next_batch().expect("first") {
        JsonlItem::Batch(batch) => {
            assert_eq!(batch.record_batch.num_rows(), 1);
            assert_eq!(batch.raw_bytes, 7);
            assert_eq!(ints(&batch.record_batch, 0).value(0), 1);
        }
        JsonlItem::Exhausted(_) => panic!("split exhausted early"),
    }
    match reader.next_batch().expect("second") {
        JsonlItem::Batch(batch) => {
            assert_eq!(batch.record_batch.num_rows(), 1);
            assert_eq!(ints(&batch.record_batch, 0).value(0), 2);
        }
        JsonlItem::Exhausted(_) => panic!("second row was not held back"),
    }
    match reader.next_batch().expect("done") {
        JsonlItem::Exhausted(done) => assert_eq!(done.rows, 2),
        JsonlItem::Batch(_) => panic!("split leftover"),
    }
    let token = reader.finish().expect("split complete");
    assert_eq!(token.batches, 2);
    assert!(!token.admits_import);

    let prepared = prepare(pair);
    let capped = budget(8, 1024, 64, 1024, 1, 4, 4, wide_deadline());
    let mut reader = JsonlReader::open(prepared.input, int_profile(), prepared.hash, capped).expect("open");
    assert!(matches!(reader.next_batch().expect("capped row"), JsonlItem::Batch(_)));
    assert_eq!(reader.next_batch().expect_err("max rows").fact(), "max_rows");
    assert_eq!(reader.finish().expect_err("max rows finish").fact(), "max_rows");

    let prepared = prepare(b"{\"n\":1}");
    let tiny = budget(1, 1024, 4, 1024, 10, 4, 4, wide_deadline());
    let mut reader = JsonlReader::open(prepared.input, int_profile(), prepared.hash, tiny).expect("open");
    assert_eq!(reader.next_batch().expect_err("max record").fact(), "max_record");
    assert_eq!(reader.finish().expect_err("max record finish").fact(), "max_record");

    let long = b"{\"n\":1}\n{\"n\":2}\n";
    assert!(long.len() > 8);
    let prepared = prepare(long);
    let total = budget(4, 1024, 8, 8, 10, 4, 4, wide_deadline());
    let mut reader = JsonlReader::open(prepared.input, int_profile(), prepared.hash, total).expect("open");
    let err = loop {
        match reader.next_batch() {
            Ok(JsonlItem::Batch(_)) => {}
            Ok(JsonlItem::Exhausted(_)) => panic!("max_total exhausted"),
            Err(err) => break err,
        }
    };
    assert_eq!(err.fact(), "max_total");
    assert_eq!(reader.finish().expect_err("max total finish").fact(), "max_total");
}

#[test]
fn missing_unknown_duplicate_and_types_are_exact() {
    assert_eq!(
        JsonlProfile::try_new("id", Vec::new()).expect_err("empty").fact(),
        "empty_selection"
    );
    let duplicate = JsonlProfile::try_new(
        "id",
        vec![
            column("n", JsonlType::Int64, false),
            column("n", JsonlType::Int64, true),
        ],
    )
    .expect_err("duplicate column");
    assert_eq!(duplicate.fact(), "duplicate_column");
    assert_eq!(
        JsonlColumn::try_new(" n", JsonlType::Utf8, true).expect_err("pad").fact(),
        "column_name"
    );

    let err = refuse(b"{\"name\":1}\n", review_profile(), "type_mismatch");
    assert_eq!(err.fact(), "type_mismatch");
    refuse(b"{\"ok\":\"true\"}", review_profile(), "type_mismatch");
    refuse(b"{\"n\":{\"a\":1}}", int_profile(), "nested_refused");
    refuse(b"{\"n\":null}", int_profile(), "null_not_allowed");
    refuse(b"{}", int_profile(), "missing_key");
    refuse(b"{\"n\":1,\"z\":true}", int_profile(), "unknown_key");
    refuse(b"{\"n\":1,\"n\":2}", int_profile(), "duplicate_key");
    refuse(br#"{"n":1,"\u006e":2}"#, int_profile(), "duplicate_key");
    let json_profile = JsonlProfile::try_new("raw", vec![column("j", JsonlType::JsonText, false)]).expect("json");
    refuse(br#"{"j":{"a":1,"a":2}}"#, json_profile, "duplicate_key");

    refuse(b"{\"n\":1.0}", int_profile(), "not_int64");
    refuse(b"{\"n\":1.5}", int_profile(), "not_int64");
    refuse(b"{\"n\":1e2}", int_profile(), "not_int64");
    refuse(b"{\"n\":+1}", int_profile(), "type_mismatch");
    refuse(b"{\"n\":01}", int_profile(), "invalid_number");
    refuse(b"{\"n\":9223372036854775808}", int_profile(), "int64_overflow");
    refuse(b"{\"n\":-9223372036854775809}", int_profile(), "int64_overflow");

    let prepared = prepare(b"{\"n\":-0}");
    let hash = prepared.hash;
    let mut reader = JsonlReader::open(prepared.input, int_profile(), hash, loose()).expect("open");
    match reader.next_batch().expect("zero") {
        JsonlItem::Batch(batch) => assert_eq!(ints(&batch.record_batch, 0).value(0), 0),
        JsonlItem::Exhausted(_) => panic!("missing -0"),
    }
    match reader.next_batch().expect("eof") {
        JsonlItem::Exhausted(done) => assert_eq!(done.object_sha256, hash),
        JsonlItem::Batch(_) => panic!("-0 leftover"),
    }
    assert_eq!(reader.finish().expect("minus zero").rows, 1);

    let digits = b"{\"j\":9007199254740993}";
    let prepared = prepare(digits);
    let profile = JsonlProfile::try_new("raw", vec![column("j", JsonlType::JsonText, false)]).expect("digits");
    let mut reader = JsonlReader::open(prepared.input, profile, prepared.hash, loose()).expect("open");
    match reader.next_batch().expect("digits") {
        JsonlItem::Batch(batch) => {
            let text = strings(&batch.record_batch, 0).value(0);
            let via_f64 = format!("{}", "9007199254740993".parse::<f64>().expect("f64"));
            assert_ne!(via_f64, "9007199254740993");
            assert_eq!(text, "9007199254740993");
        }
        JsonlItem::Exhausted(_) => panic!("missing digits"),
    }
    assert!(matches!(reader.next_batch().expect("eof"), JsonlItem::Exhausted(_)));
    assert!(!reader.finish().expect("digits complete").published);
}

#[test]
fn eof_crlf_and_unterminated_final_record() {
    let prepared = prepare(b"{\"n\":1}");
    let hash = prepared.hash;
    assert_eq!(sha256sum_file(&prepared.path), hash);
    let mut reader = JsonlReader::open(prepared.input, int_profile(), hash, loose()).expect("open");
    assert!(matches!(reader.next_batch().expect("row"), JsonlItem::Batch(_)));
    match reader.next_batch().expect("eof") {
        JsonlItem::Exhausted(done) => assert_eq!(done.object_sha256, hash),
        JsonlItem::Batch(_) => panic!("unterminated?"),
    }
    assert_eq!(reader.finish().expect("no newline").object_sha256, hash);

    let crlf = b"{\"n\":1}\r\n";
    let prepared = prepare(crlf);
    let hash = prepared.hash;
    let mut reader = JsonlReader::open(prepared.input, int_profile(), hash, loose()).expect("open");
    match reader.next_batch().expect("row") {
        JsonlItem::Batch(batch) => {
            assert_eq!(batch.lineage[0].line, 1);
            assert_eq!(ints(&batch.record_batch, 0).value(0), 1);
        }
        JsonlItem::Exhausted(_) => panic!("crlf row"),
    }
    match reader.next_batch().expect("eof") {
        JsonlItem::Exhausted(done) => assert_eq!(done.object_sha256, hash),
        JsonlItem::Batch(_) => panic!("crlf was treated as another record"),
    }
    assert_eq!(reader.finish().expect("trailing crlf").rows, 1);

    let cut = b"{\"n\":1}\n{\"n\":";
    let prepared = prepare(cut);
    let one = budget(1, 1024, 64, 1024, 10, 4, 4, wide_deadline());
    let mut reader = JsonlReader::open(prepared.input, int_profile(), prepared.hash, one).expect("open");
    match reader.next_batch().expect("first") {
        JsonlItem::Batch(batch) => assert_eq!(ints(&batch.record_batch, 0).value(0), 1),
        JsonlItem::Exhausted(_) => panic!("cut file exhausted on the first row"),
    }
    assert_eq!(reader.next_batch().expect_err("cut").fact(), "truncated");
    assert_eq!(reader.finish().expect_err("cut finish").fact(), "truncated");

    let prepared = prepare(b"{\"n\":1");
    let mut reader = JsonlReader::open(prepared.input, int_profile(), prepared.hash, loose()).expect("open");
    assert_eq!(reader.next_batch().expect_err("bare cut").fact(), "truncated");
    assert_eq!(reader.finish().expect_err("bare cut finish").fact(), "truncated");
}

#[test]
fn blocking_read_is_not_preempted_by_cancel_or_deadline() {
    println!("blocking_cancel_begin");
    let data = b"{\"n\":1}\n".to_vec();
    let hash = rust_sha256(&data);
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let slept = Arc::new(AtomicU64::new(0));
    let hold = Hold {
        data,
        pos: 0,
        held: false,
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
        slept_ms: Arc::clone(&slept),
    };
    let mut reader = JsonlReader::open(hold, int_profile(), hash, loose()).expect("open");
    let cancel = reader.cancel_handle();
    let entered_worker = Arc::clone(&entered);
    let worker = thread::spawn(move || {
        let guard = Release(release);
        let start = Instant::now();
        while !entered_worker.load(Ordering::SeqCst) {
            if start.elapsed() > Duration::from_secs(10) {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        cancel.request();
        thread::sleep(Duration::from_millis(400));
        drop(guard);
    });
    let err = reader.next_batch().expect_err("cancel during read");
    worker.join().expect("worker");
    let slept_ms = slept.load(Ordering::SeqCst);
    println!("blocking_cancel_slept_ms={slept_ms}");
    assert!(slept_ms >= 350, "read returned before the release sleep: {slept_ms}");
    assert_eq!(err.fact(), "cancelled");
    assert_eq!(reader.finish().expect_err("blocked cancel finish").fact(), "cancelled");

    println!("blocking_deadline_begin");
    let data = b"{\"n\":1}\n".to_vec();
    let hash = rust_sha256(&data);
    let slept = Arc::new(AtomicU64::new(0));
    let nap = Nap {
        data,
        pos: 0,
        napped: false,
        nap: Duration::from_millis(1500),
        slept_ms: Arc::clone(&slept),
    };
    let soon = budget(1, 1024, 64, 1024, 10, 4, 4, Instant::now() + Duration::from_millis(400));
    let mut reader = JsonlReader::open(nap, int_profile(), hash, soon).expect("open");
    let err = reader.next_batch().expect_err("deadline during read");
    let slept_ms = slept.load(Ordering::SeqCst);
    println!("blocking_deadline_slept_ms={slept_ms}");
    assert!(slept_ms >= 1200, "deadline aborted the read: {slept_ms}");
    assert_eq!(err.fact(), "deadline");
    assert_eq!(reader.finish().expect_err("deadline finish").fact(), "deadline");
}

#[test]
fn profile_grants_no_inference_pii_import_or_native_execution() {
    let bytes = success_bytes();
    let prepared = prepare(&bytes);
    let hash = prepared.hash;
    let mut reader = JsonlReader::open(prepared.input, review_profile(), hash, loose()).expect("open");
    let batch = match reader.next_batch().expect("batch") {
        JsonlItem::Batch(batch) => batch,
        JsonlItem::Exhausted(_) => panic!("empty"),
    };
    assert!(!batch.header.admits_import);
    assert!(!batch.header.published);
    assert!(!batch.header.arrow_ipc);
    assert!(!batch.header.executes_native);
    assert_eq!(batch.header.types.len(), 4);
    match reader.next_batch().expect("eof") {
        JsonlItem::Exhausted(_) => {}
        JsonlItem::Batch(_) => panic!("leftover"),
    }
    let token = reader.finish().expect("token");
    assert!(!token.admits_import);
    assert!(!token.published);
    assert_eq!(token.object_sha256, hash);
    assert_eq!(loose().queue_batches(), 1);

    let inferred = refuse(b"{\"n\":1}\n", review_profile(), "missing_key");
    assert_eq!(inferred.fact(), "missing_key");
    let number_as_text = refuse(b"{\"name\":42}", review_profile(), "type_mismatch");
    assert_eq!(number_as_text.fact(), "type_mismatch");
}
