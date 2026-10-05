//! Focused pull tests for the JSONL reader. Inputs are real temporary files.
//! The object hash is checked against `sha2` at this call site and against
//! coreutils `sha256sum` on the file and on stdin.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use arrow_array::{Array, BooleanArray, Int64Array, RecordBatch, StringArray};
use arrow_schema::DataType;
use oo_connectors::{
    FieldPresence, JsonlBudget, JsonlColumn, JsonlError, JsonlErrorKind, JsonlExhausted,
    JsonlItem, JsonlProfile, JsonlReader, JsonlType, JSONL_LINEAGE_VERSION, JSONL_PROFILE_VERSION,
    JSONL_PROTOCOL, JSONL_RECEIPT_VERSION,
};
use sha2::{Digest, Sha256};

fn temp_path() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("oo-jsonl-a182-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{}.jsonl", NEXT.fetch_add(1, Ordering::Relaxed)))
}

fn rust_sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn parse_hex64(text: &str) -> [u8; 32] {
    let hex = text.split_whitespace().next().expect("sha256sum hex");
    assert_eq!(hex.len(), 64, "sha256sum hex width");
    let mut out = [0u8; 32];
    for (index, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).expect("hex digit");
    }
    out
}

fn sha256sum_file(path: &Path) -> [u8; 32] {
    let output = Command::new("sha256sum").arg(path).output().expect("sha256sum");
    assert!(output.status.success(), "sha256sum file status");
    parse_hex64(&String::from_utf8(output.stdout).unwrap())
}

fn sha256sum_stdin(bytes: &[u8]) -> [u8; 32] {
    let mut child = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("sha256sum");
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "sha256sum stdin status");
    parse_hex64(&String::from_utf8(output.stdout).unwrap())
}

fn oracle(bytes: &[u8], path: &Path) -> [u8; 32] {
    let rust = rust_sha256(bytes);
    assert_eq!(rust, sha256sum_file(path), "file oracle");
    assert_eq!(rust, sha256sum_stdin(bytes), "stdin oracle");
    rust
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

struct OneByte<R> {
    inner: R,
}

impl<R: Read> Read for OneByte<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.inner.read(&mut buf[..1])
    }
}

struct Prepared {
    input: Counted<OneByte<File>>,
    hash: [u8; 32],
    reads: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
}

fn prepare(bytes: &[u8]) -> Prepared {
    let path = temp_path();
    fs::write(&path, bytes).unwrap();
    let hash = oracle(bytes, &path);
    let reads = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicBool::new(false));
    let input = Counted {
        inner: OneByte {
            inner: File::open(&path).unwrap(),
        },
        reads: Arc::clone(&reads),
        dropped: Arc::clone(&dropped),
    };
    Prepared {
        input,
        hash,
        reads,
        dropped,
    }
}

fn loose() -> JsonlBudget {
    JsonlBudget::try_new(
        8,
        1024 * 1024,
        4096,
        1024 * 1024,
        1000,
        16,
        16,
        Instant::now() + Duration::from_secs(60),
    )
    .unwrap()
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
    .unwrap()
}

fn column(name: &str, ty: JsonlType, nullable: bool) -> JsonlColumn {
    JsonlColumn::try_new(name, ty, nullable).unwrap()
}

fn profile(id: &str, columns: Vec<JsonlColumn>) -> JsonlProfile {
    JsonlProfile::try_new(id, columns).unwrap()
}

fn int_profile() -> JsonlProfile {
    profile("nums", vec![column("n", JsonlType::Int64, false)])
}

fn people() -> JsonlProfile {
    profile(
        "people",
        vec![
            column("name", JsonlType::Utf8, true),
            column("ok", JsonlType::Bool, false),
            column("n", JsonlType::Int64, false),
        ],
    )
}

fn fixture() -> String {
    let mut text = String::new();
    text.push_str("{\n");
    text.push_str("  \"name\": \"熊猫 café 🦀\",\n");
    text.push_str("  \"ok\": true,\n");
    text.push_str("  \"n\": -9223372036854775808\n");
    text.push_str("}\n");
    text.push_str("{\n");
    text.push_str("  \"name\": null,\n");
    text.push_str("  \"ok\": false,\n");
    text.push_str("  \"n\": 9223372036854775807\n");
    text.push_str("}\n");
    text.push_str("{\n");
    text.push_str("  \"ok\": true,\n");
    text.push_str("  \"n\": -0\n");
    text.push_str("}\n");
    text
}

fn nth(bytes: &[u8], needle: &[u8], occurrence: usize) -> (u64, u64) {
    let mut start = 0usize;
    let mut seen = 0usize;
    while start + needle.len() <= bytes.len() {
        if &bytes[start..start + needle.len()] == needle {
            if seen == occurrence {
                let line = bytes[..start].iter().filter(|byte| **byte == b'\n').count() as u64 + 1;
                return (line, start as u64);
            }
            seen += 1;
        }
        start += 1;
    }
    panic!("needle missing");
}

fn kind_label(kind: JsonlErrorKind) -> &'static str {
    match kind {
        JsonlErrorKind::Config => "config",
        JsonlErrorKind::Budget => "budget",
        JsonlErrorKind::Protocol => "protocol",
        JsonlErrorKind::Encoding => "encoding",
        JsonlErrorKind::Hash => "hash",
        JsonlErrorKind::Cancelled => "cancelled",
        JsonlErrorKind::Io => "io",
    }
}

fn profile_preimage() -> String {
    let mut body = String::from("oo-jsonl-profile/v1\n");
    body.push_str("id=people\n");
    body.push_str("column=name\tutf8\t1\n");
    body.push_str("column=ok\tbool\t0\n");
    body.push_str("column=n\tint64\t0\n");
    body
}

fn publish(name: &str, bytes: &[u8]) {
    let Ok(dir) = std::env::var("OO_JSONL_ORACLE_DIR") else {
        return;
    };
    let path = Path::new(&dir).join(name);
    fs::write(path, bytes).unwrap();
}

fn string_col(batch: &RecordBatch, index: usize) -> &StringArray {
    batch
        .column(index)
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("utf8 column")
}

fn bool_col(batch: &RecordBatch, index: usize) -> &BooleanArray {
    batch
        .column(index)
        .as_any()
        .downcast_ref::<BooleanArray>()
        .expect("bool column")
}

fn int_col(batch: &RecordBatch, index: usize) -> &Int64Array {
    batch
        .column(index)
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("int64 column")
}

#[test]
fn profile_rejects_duplicate_empty_and_ambiguous_names() {
    let duplicate = JsonlProfile::try_new(
        "p",
        vec![
            column("n", JsonlType::Int64, true),
            column("n", JsonlType::Int64, true),
        ],
    )
    .unwrap_err();
    assert_eq!(duplicate.kind(), JsonlErrorKind::Config);
    assert_eq!(duplicate.fact(), "duplicate_column");

    let empty = JsonlProfile::try_new("p", vec![]).unwrap_err();
    assert_eq!(empty.fact(), "empty_selection");

    assert_eq!(
        JsonlColumn::try_new(" n", JsonlType::Utf8, true)
            .unwrap_err()
            .fact(),
        "column_name"
    );
    assert_eq!(
        JsonlColumn::try_new("n ", JsonlType::Utf8, true)
            .unwrap_err()
            .fact(),
        "column_name"
    );
    assert_eq!(
        JsonlColumn::try_new("n\t", JsonlType::Utf8, true)
            .unwrap_err()
            .fact(),
        "column_name"
    );
    assert_eq!(
        JsonlProfile::try_new("", vec![column("n", JsonlType::Int64, true)])
            .unwrap_err()
            .fact(),
        "profile_id"
    );
    let spaced = column("full name", JsonlType::Utf8, true);
    assert_eq!(spaced.name(), "full name");
    assert_eq!(loose().queue_batches(), 1);
}

#[test]
fn explicit_profile_multiline_file_projects_arrow_and_lineage() {
    let bytes = fixture().into_bytes();
    let prepared = prepare(&bytes);
    let hash = prepared.hash;
    let reads = Arc::clone(&prepared.reads);
    let dropped = Arc::clone(&prepared.dropped);
    let spec = people();
    let body = profile_preimage();
    assert_eq!(rust_sha256(body.as_bytes()), spec.profile_digest());
    let mut config = String::from("oo-jsonl-config-identity/v1\n");
    config.push_str(&body);
    config.push_str("expected_sha256=");
    config.push_str(&hex(&hash));
    config.push('\n');
    assert_eq!(rust_sha256(config.as_bytes()), spec.config_identity(hash));
    publish("fixture.jsonl", &bytes);
    publish("fixture.sha256", format!("{}\n", hex(&hash)).as_bytes());
    publish("profile-preimage.txt", body.as_bytes());
    publish("config-preimage.txt", config.as_bytes());
    println!("oracle sha256={}", hex(&hash));

    let mut reader = JsonlReader::open(
        prepared.input,
        spec,
        hash,
        budget(
            1,
            1024 * 1024,
            4096,
            1024 * 1024,
            1000,
            16,
            16,
            Instant::now() + Duration::from_secs(60),
        ),
    )
    .unwrap();
    let rendered = format!("{reader:?}");
    assert!(rendered.contains("people"));
    assert!(!rendered.contains("熊猫"));
    assert!(!rendered.contains("café"));

    let header = reader.header();
    assert_eq!(header.protocol, JSONL_PROTOCOL);
    assert_eq!(header.profile_version, JSONL_PROFILE_VERSION);
    assert_eq!(header.lineage_version, JSONL_LINEAGE_VERSION);
    assert_eq!(header.receipt_version, JSONL_RECEIPT_VERSION);
    assert_eq!(header.columns, vec!["name", "ok", "n"]);
    assert_eq!(
        header.types,
        vec![JsonlType::Utf8, JsonlType::Bool, JsonlType::Int64]
    );
    assert_eq!(header.nullability, vec![true, false, false]);
    assert!(!header.admits_import);
    assert!(!header.published);
    assert!(!header.arrow_ipc);
    assert!(!header.executes_native);
    assert_eq!(header.profile_digest, rust_sha256(body.as_bytes()));
    assert_eq!(header.config_identity, rust_sha256(config.as_bytes()));

    let mut batches = Vec::new();
    for _ in 0..3 {
        match reader.next_batch().unwrap() {
            JsonlItem::Batch(batch) => batches.push(batch),
            JsonlItem::Exhausted(_) => panic!("exhausted before three rows"),
        }
    }
    let first = reader.next_batch().unwrap();
    let second = reader.next_batch().unwrap();
    let JsonlItem::Exhausted(done) = first else {
        panic!("expected exhaustion");
    };
    let JsonlItem::Exhausted(again) = second else {
        panic!("exhaustion is repeatable");
    };
    assert_eq!(done.rows, 3);
    assert_eq!(done.batches, 3);
    assert_eq!(done.object_sha256, hash);
    assert_eq!(again.rows, done.rows);
    assert_eq!(again.object_sha256, hash);
    let rendered_end = format!("{done:?}");
    assert!(!rendered_end.contains("is_eof"));
    assert!(!rendered_end.contains("is_complete"));

    assert!(reads.load(Ordering::SeqCst) >= bytes.len());
    check_batch(&batches[0], &bytes, 0);
    check_batch(&batches[1], &bytes, 1);
    check_batch(&batches[2], &bytes, 2);

    let token = reader.finish().unwrap();
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(token.protocol, JSONL_PROTOCOL);
    assert_eq!(token.profile_id, "people");
    assert_eq!(token.object_sha256, hash);
    assert_eq!(token.rows, 3);
    assert_eq!(token.columns, 3);
    assert_eq!(token.batches, 3);
    assert!(!token.admits_import);
    assert!(!token.published);
    let cloned = token.clone();
    assert!(!cloned.admits_import);
    assert_eq!(cloned.object_sha256, hash);
}

fn check_batch(batch: &oo_connectors::JsonlBatch, bytes: &[u8], index: usize) {
    assert_eq!(batch.batch_index, index as u64);
    assert!(!batch.resumable);
    assert!(!batch.header.admits_import);
    assert!(!batch.header.arrow_ipc);
    assert!(!batch.header.executes_native);
    let record = &batch.record_batch;
    let schema = record.schema();
    assert_eq!(schema.fields().len(), 3);
    assert_eq!(schema.field(0).name(), "name");
    assert_eq!(schema.field(0).data_type(), &DataType::Utf8);
    assert!(schema.field(0).is_nullable());
    assert_eq!(
        schema.field(0).metadata().get("oo.jsonl.logical").map(String::as_str),
        Some("utf8")
    );
    assert_eq!(
        schema.field(0).metadata().get("oo.jsonl.protocol").map(String::as_str),
        Some(JSONL_PROTOCOL)
    );
    assert_eq!(schema.field(1).data_type(), &DataType::Boolean);
    assert!(!schema.field(1).is_nullable());
    assert_eq!(
        schema.field(1).metadata().get("oo.jsonl.logical").map(String::as_str),
        Some("bool")
    );
    assert_eq!(schema.field(2).data_type(), &DataType::Int64);
    assert!(!schema.field(2).is_nullable());
    assert_eq!(
        schema.field(2).metadata().get("oo.jsonl.logical").map(String::as_str),
        Some("int64")
    );
    assert!(batch.arrow_bytes > 0);
    assert_eq!(record.num_rows(), 1);
    assert_eq!(batch.lineage.len(), 1);
    let row = &batch.lineage[0];
    assert_eq!(row.row_index, index as u64);
    let (line, offset) = nth(bytes, b"{", index);
    assert_eq!(row.line, line);
    assert_eq!(row.byte_offset, offset);
    assert_eq!(line, [1, 6, 11][index]);
    let end = nth(bytes, b"}", index).1 + 1;
    assert_eq!(batch.raw_bytes, end - offset);
    assert_eq!(row.fields.len(), 3);
    assert_eq!(row.fields[0].name, "name");
    assert_eq!(row.fields[1].name, "ok");
    assert_eq!(row.fields[2].name, "n");
    let names = string_col(record, 0);
    let flags = bool_col(record, 1);
    let numbers = int_col(record, 2);
    match index {
        0 => {
            assert_eq!(names.value(0), "熊猫 café 🦀");
            assert!(flags.value(0));
            assert_eq!(numbers.value(0), i64::MIN);
            let key = nth(bytes, b"\"name\"", 0);
            assert_eq!(row.fields[0].presence, FieldPresence::Value);
            assert_eq!((row.fields[0].line, row.fields[0].byte_offset), key);
            let ok = nth(bytes, b"\"ok\"", 0);
            assert_eq!(row.fields[1].presence, FieldPresence::Value);
            assert_eq!((row.fields[1].line, row.fields[1].byte_offset), ok);
            let number = nth(bytes, b"\"n\"", 0);
            assert_eq!(row.fields[2].presence, FieldPresence::Value);
            assert_eq!((row.fields[2].line, row.fields[2].byte_offset), number);
        }
        1 => {
            assert!(names.is_null(0));
            assert!(!flags.value(0));
            assert_eq!(numbers.value(0), i64::MAX);
            let key = nth(bytes, b"\"name\"", 1);
            assert_eq!(row.fields[0].presence, FieldPresence::Null);
            assert_eq!((row.fields[0].line, row.fields[0].byte_offset), key);
        }
        2 => {
            assert!(names.is_null(0));
            assert!(flags.value(0));
            assert_eq!(numbers.value(0), 0);
            assert_eq!(row.fields[0].presence, FieldPresence::Missing);
            assert_eq!(row.fields[0].line, line);
            assert_eq!(row.fields[0].byte_offset, offset);
            let ok = nth(bytes, b"\"ok\"", 2);
            assert_eq!(row.fields[1].presence, FieldPresence::Value);
            assert_eq!((row.fields[1].line, row.fields[1].byte_offset), ok);
        }
        _ => panic!("row"),
    }
}

#[test]
fn hash_mismatch_returns_a_batch_then_refuses_the_token() {
    let bytes = fixture().into_bytes();
    let prepared = prepare(&bytes);
    let mut wrong = prepared.hash;
    wrong[31] ^= 0xff;
    let mut reader = JsonlReader::open(prepared.input, people(), wrong, loose()).unwrap();
    let mut delivered = 0u64;
    let err = loop {
        match reader.next_batch() {
            Ok(JsonlItem::Batch(_)) => delivered += 1,
            Ok(JsonlItem::Exhausted(_)) => panic!("mismatch minted exhaustion"),
            Err(err) => break err,
        }
    };
    assert!(delivered >= 1);
    assert_eq!(err.kind(), JsonlErrorKind::Hash);
    assert_eq!(err.fact(), "hash_mismatch");
    assert_eq!(err.to_string(), "jsonl error: hash fact=hash_mismatch");
    assert!(!err.to_string().contains("熊猫"));
    let finished = reader.finish().unwrap_err();
    assert_eq!(finished, err);

    let prepared = prepare(&bytes);
    let mut wrong = prepared.hash;
    wrong[0] ^= 0x01;
    let wide = budget(
        16,
        1024 * 1024,
        4096,
        1024 * 1024,
        1000,
        16,
        16,
        Instant::now() + Duration::from_secs(60),
    );
    let mut reader = JsonlReader::open(prepared.input, people(), wrong, wide).unwrap();
    match reader.next_batch().unwrap() {
        JsonlItem::Batch(batch) => assert_eq!(batch.record_batch.num_rows(), 3),
        JsonlItem::Exhausted(_) => panic!("wide pull exhausted on a bad hash"),
    }
    let err = reader.next_batch().unwrap_err();
    assert_eq!(err.fact(), "hash_mismatch");
    assert_eq!(reader.finish().unwrap_err().fact(), "hash_mismatch");
}

#[test]
fn partial_finish_before_exhaustion_refuses_a_token() {
    let bytes = fixture().into_bytes();
    let prepared = prepare(&bytes);
    let rows = budget(
        1,
        1024 * 1024,
        4096,
        1024 * 1024,
        1000,
        16,
        16,
        Instant::now() + Duration::from_secs(60),
    );
    let mut reader = JsonlReader::open(prepared.input, people(), prepared.hash, rows).unwrap();
    for _ in 0..3 {
        match reader.next_batch().unwrap() {
            JsonlItem::Batch(_) => {}
            JsonlItem::Exhausted(_) => panic!("partial pull returned exhaustion"),
        }
    }
    let err = reader.finish().unwrap_err();
    assert_eq!(err.kind(), JsonlErrorKind::Protocol);
    assert_eq!(err.fact(), "not_exhausted");
    assert_eq!(err.to_string(), "jsonl error: protocol fact=not_exhausted");
}

#[test]
fn cancel_and_drop_release_the_reader_without_a_token() {
    let bytes = fixture().into_bytes();
    let prepared = prepare(&bytes);
    let reads = Arc::clone(&prepared.reads);
    let dropped = Arc::clone(&prepared.dropped);
    let mut reader = JsonlReader::open(prepared.input, people(), prepared.hash, loose()).unwrap();
    reader.cancel_handle().request();
    let err = reader.next_batch().unwrap_err();
    assert_eq!(err.kind(), JsonlErrorKind::Cancelled);
    assert_eq!(err.fact(), "cancelled");
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(reader.finish().unwrap_err().fact(), "cancelled");
    assert!(dropped.load(Ordering::SeqCst));

    let prepared = prepare(&bytes);
    let dropped = Arc::clone(&prepared.dropped);
    let one_row = budget(
        1,
        1024 * 1024,
        4096,
        1024 * 1024,
        1000,
        16,
        16,
        Instant::now() + Duration::from_secs(60),
    );
    let mut reader = JsonlReader::open(prepared.input, people(), prepared.hash, one_row).unwrap();
    match reader.next_batch().unwrap() {
        JsonlItem::Batch(batch) => {
            assert_eq!(batch.record_batch.num_rows(), 1);
            assert_eq!(batch.lineage[0].row_index, 0);
        }
        JsonlItem::Exhausted(_) => panic!("cancel setup"),
    }
    let cancelled = reader.cancel();
    assert!(cancelled.released);
    assert_eq!(cancelled.rows_emitted, 1);
    assert_eq!(cancelled.profile_id, "people");
    assert!(dropped.load(Ordering::SeqCst));

    let prepared = prepare(&bytes);
    let dropped = Arc::clone(&prepared.dropped);
    let reads = Arc::clone(&prepared.reads);
    let reader = JsonlReader::open(prepared.input, people(), prepared.hash, loose()).unwrap();
    drop(reader);
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[test]
fn deadline_and_column_cap_fail_before_a_read() {
    let bytes = b"{\"n\":1}\n";
    let prepared = prepare(bytes);
    let reads = Arc::clone(&prepared.reads);
    let dropped = Arc::clone(&prepared.dropped);
    let past = budget(
        1,
        1024,
        64,
        1024,
        10,
        4,
        4,
        Instant::now() - Duration::from_secs(5),
    );
    let err = JsonlReader::open(prepared.input, int_profile(), prepared.hash, past).unwrap_err();
    assert_eq!(err.kind(), JsonlErrorKind::Budget);
    assert_eq!(err.fact(), "deadline");
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert!(dropped.load(Ordering::SeqCst));

    let prepared = prepare(bytes);
    let reads = Arc::clone(&prepared.reads);
    let dropped = Arc::clone(&prepared.dropped);
    let narrow = budget(
        1,
        1024,
        64,
        1024,
        10,
        1,
        4,
        Instant::now() + Duration::from_secs(60),
    );
    let wide = profile(
        "two",
        vec![
            column("n", JsonlType::Int64, false),
            column("ok", JsonlType::Bool, true),
        ],
    );
    let err = JsonlReader::open(prepared.input, wide, prepared.hash, narrow).unwrap_err();
    assert_eq!(err.kind(), JsonlErrorKind::Config);
    assert_eq!(err.fact(), "max_columns");
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert!(dropped.load(Ordering::SeqCst));
}

#[test]
fn bounds_stop_oversized_records_and_extra_bytes() {
    let exact = b"{\"n\":1}";
    assert_eq!(exact.len(), 7);
    let prepared = prepare(exact);
    let fit = budget(
        1,
        1024,
        7,
        7,
        10,
        4,
        4,
        Instant::now() + Duration::from_secs(60),
    );
    let mut reader = JsonlReader::open(prepared.input, int_profile(), prepared.hash, fit).unwrap();
    match reader.next_batch().unwrap() {
        JsonlItem::Batch(batch) => assert_eq!(int_col(&batch.record_batch, 0).value(0), 1),
        JsonlItem::Exhausted(_) => panic!("exact record"),
    }
    match reader.next_batch().unwrap() {
        JsonlItem::Exhausted(done) => assert_eq!(done.object_sha256, prepared.hash),
        JsonlItem::Batch(_) => panic!("exact record leftover"),
    }
    let token = reader.finish().unwrap();
    assert_eq!(token.rows, 1);
    assert!(!token.admits_import);

    let over = b"{\"n\":12345678901234567890}";
    assert!(over.len() > 16);
    let prepared = prepare(over);
    let cap = budget(
        1,
        1024,
        16,
        1024,
        10,
        4,
        4,
        Instant::now() + Duration::from_secs(60),
    );
    let (err, batches) = pull_err(prepared, int_profile(), cap);
    assert_eq!(batches, 0);
    assert_eq!(err.kind(), JsonlErrorKind::Budget);
    assert_eq!(err.fact(), "max_record");

    let mut long = Vec::new();
    while long.len() <= 32 {
        long.extend_from_slice(b"{\"n\":1}\n");
    }
    assert!(long.len() > 32);
    let prepared = prepare(&long);
    let total = budget(
        1,
        1024,
        32,
        32,
        10,
        4,
        4,
        Instant::now() + Duration::from_secs(60),
    );
    let (err, batches) = pull_err(prepared, int_profile(), total);
    assert!(batches >= 1);
    assert_eq!(err.fact(), "max_total");
    assert_eq!(err.to_string(), "jsonl error: budget fact=max_total");

    let edge = b"{\"n\":1}\n";
    assert_eq!(edge.len(), 8);
    let prepared = prepare(edge);
    let edge_budget = budget(
        4,
        1024,
        8,
        8,
        10,
        4,
        4,
        Instant::now() + Duration::from_secs(60),
    );
    let mut reader =
        JsonlReader::open(prepared.input, int_profile(), prepared.hash, edge_budget).unwrap();
    assert!(matches!(reader.next_batch().unwrap(), JsonlItem::Batch(_)));
    match reader.next_batch().unwrap() {
        JsonlItem::Exhausted(done) => assert_eq!(done.rows, 1),
        JsonlItem::Batch(_) => panic!("boundary leftover"),
    }
    assert!(reader.finish().is_ok());
}

fn pull_err(
    prepared: Prepared,
    profile: JsonlProfile,
    budget: JsonlBudget,
) -> (JsonlError, u64) {
    let mut reader = JsonlReader::open(prepared.input, profile, prepared.hash, budget).unwrap();
    let mut batches = 0u64;
    let err = loop {
        match reader.next_batch() {
            Ok(JsonlItem::Batch(_)) => batches += 1,
            Ok(JsonlItem::Exhausted(_)) => panic!("limit minted exhaustion"),
            Err(err) => break err,
        }
    };
    assert_eq!(reader.finish().unwrap_err(), err);
    (err, batches)
}

#[test]
fn refusals_are_exact_and_do_not_skip_the_row() {
    refuse(
        b"{\"n\":\"x\"}\n",
        int_profile(),
        JsonlErrorKind::Protocol,
        "type_mismatch",
    );
    refuse(
        b"{\"n\":+1}\n",
        int_profile(),
        JsonlErrorKind::Protocol,
        "type_mismatch",
    );
    refuse(
        b"{\"a\":1,\"a\":2}\n",
        profile("d", vec![column("a", JsonlType::Int64, false)]),
        JsonlErrorKind::Protocol,
        "duplicate_key",
    );
    refuse(
        b"{\"a\":1,\"\\u0061\":2}\n",
        profile("d", vec![column("a", JsonlType::Int64, false)]),
        JsonlErrorKind::Protocol,
        "duplicate_key",
    );
    refuse(
        b"{\"j\":{\"a\":1,\"a\":2}}\n",
        profile("d", vec![column("j", JsonlType::JsonText, false)]),
        JsonlErrorKind::Protocol,
        "duplicate_key",
    );
    refuse(b"{\"n\":1.0}\n", int_profile(), JsonlErrorKind::Protocol, "not_int64");
    refuse(b"{\"n\":1.5}\n", int_profile(), JsonlErrorKind::Protocol, "not_int64");
    refuse(b"{\"n\":1e2}\n", int_profile(), JsonlErrorKind::Protocol, "not_int64");
    refuse(
        b"{\"n\":9223372036854775808}\n",
        int_profile(),
        JsonlErrorKind::Protocol,
        "int64_overflow",
    );
    refuse(
        b"{\"n\":-9223372036854775809}\n",
        int_profile(),
        JsonlErrorKind::Protocol,
        "int64_overflow",
    );
    refuse(b"{\"n\":01}\n", int_profile(), JsonlErrorKind::Protocol, "invalid_number");
    let nested = refuse(
        b"{\"n\":{\"password\":\"secret\"}}\n",
        int_profile(),
        JsonlErrorKind::Protocol,
        "nested_refused",
    );
    assert!(!nested.contains("password"));
    assert!(!nested.contains("secret"));
    refuse(
        b"{\"n\":1,\"extra\":true}\n",
        int_profile(),
        JsonlErrorKind::Protocol,
        "unknown_key",
    );
    refuse(
        b"{\"ok\":true}\n",
        profile(
            "need",
            vec![
                column("ok", JsonlType::Bool, false),
                column("n", JsonlType::Int64, false),
            ],
        ),
        JsonlErrorKind::Protocol,
        "missing_key",
    );
    refuse(
        b"{\"n\":null}\n",
        int_profile(),
        JsonlErrorKind::Protocol,
        "null_not_allowed",
    );
    refuse(b"[1]\n", int_profile(), JsonlErrorKind::Protocol, "not_object");
    refuse(b"{\"n\":1", int_profile(), JsonlErrorKind::Protocol, "truncated");
    let mut bad = b"{\"name\":\"".to_vec();
    bad.push(0xff);
    bad.extend_from_slice(b"\"}\n");
    refuse(
        &bad,
        profile("t", vec![column("name", JsonlType::Utf8, true)]),
        JsonlErrorKind::Encoding,
        "utf8",
    );
    let mut bom = vec![0xef, 0xbb, 0xbf];
    bom.extend_from_slice(b"{\"n\":1}\n");
    refuse(&bom, int_profile(), JsonlErrorKind::Protocol, "not_object");
    let mut control = b"{\"name\":\"a".to_vec();
    control.push(b'\n');
    control.extend_from_slice(b"\"}\n");
    refuse(
        &control,
        profile("t", vec![column("name", JsonlType::Utf8, true)]),
        JsonlErrorKind::Protocol,
        "control_char",
    );
}

fn refuse(bytes: &[u8], profile: JsonlProfile, kind: JsonlErrorKind, fact: &str) -> String {
    let prepared = prepare(bytes);
    let mut reader = JsonlReader::open(prepared.input, profile, prepared.hash, loose()).unwrap();
    let err = match reader.next_batch() {
        Ok(JsonlItem::Batch(_)) => panic!("{fact} delivered a batch"),
        Ok(JsonlItem::Exhausted(_)) => panic!("{fact} exhausted"),
        Err(err) => err,
    };
    assert_eq!(err.kind(), kind, "{fact}");
    assert_eq!(err.fact(), fact);
    let rendered = err.to_string();
    assert_eq!(
        rendered,
        format!("jsonl error: {} fact={fact}", kind_label(kind))
    );
    assert_eq!(reader.finish().unwrap_err(), err);
    assert!(prepared.dropped.load(Ordering::SeqCst));
    rendered
}

#[test]
fn json_text_keeps_raw_json_and_null_stays_null() {
    let raw = "{\"a\": 1, \"b\": [true, null]}";
    let body = format!("{{\"j\":{raw}}}\n");
    let prepared = prepare(body.as_bytes());
    let spec = profile("raw", vec![column("j", JsonlType::JsonText, true)]);
    let mut reader = JsonlReader::open(prepared.input, spec, prepared.hash, loose()).unwrap();
    match reader.next_batch().unwrap() {
        JsonlItem::Batch(batch) => {
            let column = string_col(&batch.record_batch, 0);
            assert_eq!(column.value(0), raw);
            assert_eq!(
                batch
                    .record_batch
                    .schema()
                    .field(0)
                    .metadata()
                    .get("oo.jsonl.logical")
                    .map(String::as_str),
                Some("json_text")
            );
            assert_eq!(
                batch
                    .record_batch
                    .schema()
                    .field(0)
                    .metadata()
                    .get("oo.jsonl.encoding")
                    .map(String::as_str),
                Some("raw-json-utf8")
            );
            assert_eq!(batch.lineage[0].fields[0].presence, FieldPresence::Value);
        }
        JsonlItem::Exhausted(_) => panic!("raw json"),
    }
    assert!(matches!(
        reader.next_batch().unwrap(),
        JsonlItem::Exhausted(_)
    ));
    assert!(!reader.finish().unwrap().published);

    let prepared = prepare(b"{\"j\":\"a\\\"b\"}\n");
    let spec = profile("raw", vec![column("j", JsonlType::JsonText, true)]);
    let mut reader = JsonlReader::open(prepared.input, spec, prepared.hash, loose()).unwrap();
    match reader.next_batch().unwrap() {
        JsonlItem::Batch(batch) => {
            assert_eq!(string_col(&batch.record_batch, 0).value(0), "\"a\\\"b\"");
        }
        JsonlItem::Exhausted(_) => panic!("escaped raw"),
    }
    assert_eq!(reader.finish().unwrap_err().fact(), "not_exhausted");

    let prepared = prepare(b"{\"j\":null}\n");
    let spec = profile("raw", vec![column("j", JsonlType::JsonText, true)]);
    let mut reader = JsonlReader::open(prepared.input, spec, prepared.hash, loose()).unwrap();
    match reader.next_batch().unwrap() {
        JsonlItem::Batch(batch) => {
            assert!(string_col(&batch.record_batch, 0).is_null(0));
            assert_eq!(batch.lineage[0].fields[0].presence, FieldPresence::Null);
        }
        JsonlItem::Exhausted(_) => panic!("json null"),
    }
    assert!(matches!(
        reader.next_batch().unwrap(),
        JsonlItem::Exhausted(_)
    ));
    let token = reader.finish().unwrap();
    assert!(!token.admits_import);
}

#[test]
fn one_line_holds_two_records_and_batch_bytes_splits() {
    let bytes = b"{\"n\":1}{\"n\":2}\n";
    let prepared = prepare(bytes);
    let rows = budget(
        1,
        1024,
        64,
        1024,
        10,
        4,
        4,
        Instant::now() + Duration::from_secs(60),
    );
    let mut reader = JsonlReader::open(prepared.input, int_profile(), prepared.hash, rows).unwrap();
    let mut lines = Vec::new();
    let mut values = Vec::new();
    for _ in 0..2 {
        match reader.next_batch().unwrap() {
            JsonlItem::Batch(batch) => {
                lines.push(batch.lineage[0].line);
                values.push(int_col(&batch.record_batch, 0).value(0));
                assert_eq!(batch.lineage[0].line, 1);
            }
            JsonlItem::Exhausted(_) => panic!("one line"),
        }
    }
    assert_eq!(values, vec![1, 2]);
    assert_eq!(lines, vec![1, 1]);
    assert!(matches!(
        reader.next_batch().unwrap(),
        JsonlItem::Exhausted(_)
    ));
    assert_eq!(reader.finish().unwrap().rows, 2);

    let decoded = b"{\"\\u006e\":1}\n";
    let prepared = prepare(decoded);
    let mut reader =
        JsonlReader::open(prepared.input, int_profile(), prepared.hash, loose()).unwrap();
    match reader.next_batch().unwrap() {
        JsonlItem::Batch(batch) => assert_eq!(int_col(&batch.record_batch, 0).value(0), 1),
        JsonlItem::Exhausted(_) => panic!("decoded key"),
    }
    assert!(matches!(
        reader.next_batch().unwrap(),
        JsonlItem::Exhausted(_)
    ));
    assert!(reader.finish().is_ok());

    let split = b"{\"n\":1}\n{\"n\":22}\n";
    let prepared = prepare(split);
    let tight = budget(
        8,
        10,
        64,
        1024,
        10,
        4,
        4,
        Instant::now() + Duration::from_secs(60),
    );
    let mut reader =
        JsonlReader::open(prepared.input, int_profile(), prepared.hash, tight).unwrap();
    match reader.next_batch().unwrap() {
        JsonlItem::Batch(batch) => {
            assert_eq!(batch.record_batch.num_rows(), 1);
            assert_eq!(int_col(&batch.record_batch, 0).value(0), 1);
            assert_eq!(batch.raw_bytes, 7);
        }
        JsonlItem::Exhausted(_) => panic!("split"),
    }
    match reader.next_batch().unwrap() {
        JsonlItem::Batch(batch) => {
            assert_eq!(int_col(&batch.record_batch, 0).value(0), 22);
            assert_eq!(batch.raw_bytes, 8);
        }
        JsonlItem::Exhausted(_) => panic!("second split"),
    }
    assert!(matches!(
        reader.next_batch().unwrap(),
        JsonlItem::Exhausted(_)
    ));
    assert_eq!(reader.finish().unwrap().batches, 2);
}

#[test]
fn empty_object_stream_completes_with_zero_rows() {
    let prepared = prepare(b"");
    let mut reader =
        JsonlReader::open(prepared.input, int_profile(), prepared.hash, loose()).unwrap();
    match reader.next_batch().unwrap() {
        JsonlItem::Exhausted(JsonlExhausted {
            rows,
            batches,
            object_sha256,
            ..
        }) => {
            assert_eq!(rows, 0);
            assert_eq!(batches, 0);
            assert_eq!(object_sha256, prepared.hash);
        }
        JsonlItem::Batch(_) => panic!("empty file"),
    }
    let token = reader.finish().unwrap();
    assert_eq!(token.rows, 0);
    assert_eq!(token.batches, 0);
    assert!(!token.admits_import);
    assert!(!token.published);

    let prepared = prepare(b"\n\n");
    let mut reader =
        JsonlReader::open(prepared.input, int_profile(), prepared.hash, loose()).unwrap();
    assert!(matches!(
        reader.next_batch().unwrap(),
        JsonlItem::Exhausted(_)
    ));
    assert_eq!(reader.finish().unwrap().rows, 0);
}
