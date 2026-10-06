//! Independent review of the frozen file-worker parent.
//!
//! The production cases spawn `oo-file-parser-worker`. The fixture executable is
//! used only for a missing completion, a trailer byte, a deadline stall, and one
//! `setsid` grandchild. Hash checks use `sha2` in-process and the std-only
//! oracle tool on the saved bytes. No PostgreSQL and no Python.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use arrow_array::{Array, BooleanArray, Decimal128Array, Int64Array, RecordBatch, StringArray};
use oo_connectors::{
    BomPolicy, ChildRequest, DelimitedColumn, DelimitedDialect, DelimitedProfile, DelimitedType,
    Failure, FailureKind, GroupSignal, HeaderPolicy, JsonlColumn, JsonlProfile, JsonlType,
    KernelRestriction, NullPolicy, ParentConfig, ParentError, ParentParseComplete, ParentPull,
    PullBudget, SourceProfile, VerifiedBatch, FILE_WORKER_PARENT_PROTOCOL, KERNEL_RESTRICTION,
    PARENT_DEADLINE_MAX_MS, PARENT_DEADLINE_MIN_MS,
};
use sha2::{Digest, Sha256};

const UUID: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a237";
const FRAME_CAP: u64 = 1_048_576 + 65_536;
const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
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

fn scratch(name: &str) -> PathBuf {
    let root = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(std::env::temp_dir);
    let dir = root.join(format!("oo-a237-parent-review-{UUID}"));
    fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("{UUID} temp {err}"));
    dir.join(format!("{name}-{}", std::process::id()))
}

fn write_file(name: &str, bytes: &[u8]) -> (PathBuf, [u8; 32]) {
    let path = scratch(name);
    fs::write(&path, bytes).unwrap_or_else(|err| panic!("{UUID} write {err}"));
    (path, sha256_bytes(bytes))
}

fn save_oracle(name: &str, bytes: &[u8], digest: [u8; 32]) {
    let dir = PathBuf::from(
        std::env::var("OO_A237_ORACLE_DIR").unwrap_or_else(|err| panic!("{UUID} oracle dir {err}")),
    );
    fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("{UUID} oracle mkdir {err}"));
    fs::write(dir.join(name), bytes).unwrap_or_else(|err| panic!("{UUID} oracle write {err}"));
    fs::write(dir.join(format!("{name}.sha256")), format!("{}\n", hex(&digest)))
        .unwrap_or_else(|err| panic!("{UUID} oracle hash {err}"));
}

fn worker_bin() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_oo-file-parser-worker"))
}

fn parent(deadline_ms: u64, bin: &Path) -> ParentConfig {
    ParentConfig::try_new(bin, deadline_ms).unwrap_or_else(|err| panic!("{UUID} config {err}"))
}

fn jsonl_budget(batch_rows: u32) -> PullBudget {
    PullBudget::jsonl(batch_rows, 1_048_576, 65_536, 1_048_576, 1000, 16, 64, 60_000)
        .unwrap_or_else(|err| panic!("{UUID} jsonl budget {err}"))
}

fn delimited_budget() -> PullBudget {
    PullBudget::delimited(32, 1_048_576, 65_536, 8192, 1_048_576, 1000, 16, 64, 60_000)
        .unwrap_or_else(|err| panic!("{UUID} delimited budget {err}"))
}

fn notes_profile() -> JsonlProfile {
    JsonlProfile::try_new(
        "a237-notes",
        vec![
            JsonlColumn::try_new("label", JsonlType::Utf8, true).unwrap(),
            JsonlColumn::try_new("flag", JsonlType::Bool, false).unwrap(),
            JsonlColumn::try_new("n", JsonlType::Int64, false).unwrap(),
        ],
    )
    .unwrap_or_else(|err| panic!("{UUID} notes profile {err}"))
}

fn lot_profile() -> DelimitedProfile {
    DelimitedProfile::try_new(
        "a237-lot",
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Exact,
        NullPolicy::Token("~".to_owned()),
        vec![
            DelimitedColumn::try_new("sku", DelimitedType::Utf8, true).unwrap(),
            DelimitedColumn::try_new("qty", DelimitedType::Int64, false).unwrap(),
            DelimitedColumn::try_new(
                "price",
                DelimitedType::Decimal {
                    precision: 8,
                    scale: 3,
                },
                false,
            )
            .unwrap(),
            DelimitedColumn::try_new("note", DelimitedType::Utf8, true).unwrap(),
        ],
    )
    .unwrap_or_else(|err| panic!("{UUID} lot profile {err}"))
}

fn tab_profile() -> DelimitedProfile {
    DelimitedProfile::try_new(
        "a237-tab",
        DelimitedDialect::Tsv,
        BomPolicy::Refuse,
        HeaderPolicy::Absent,
        NullPolicy::Empty,
        vec![
            DelimitedColumn::try_new("name", DelimitedType::Utf8, false).unwrap(),
            DelimitedColumn::try_new("raw", DelimitedType::Utf8, false).unwrap(),
            DelimitedColumn::try_new("n", DelimitedType::Int64, false).unwrap(),
        ],
    )
    .unwrap_or_else(|err| panic!("{UUID} tab profile {err}"))
}

fn request_notes(path: &Path, sha: [u8; 32], batch_rows: u32) -> ChildRequest {
    ChildRequest::try_new(
        path,
        sha,
        SourceProfile::Jsonl(notes_profile()),
        jsonl_budget(batch_rows),
    )
    .unwrap_or_else(|err| panic!("{UUID} notes request {err}"))
}

fn failure(err: ParentError) -> Failure {
    match err {
        ParentError::Failed(failure) => failure,
        ParentError::Config(token) => panic!("{UUID} config {token}"),
    }
}

fn assert_hidden(err: &ParentError, forbidden: &[&str]) {
    let text = err.to_string();
    for item in forbidden {
        assert!(!text.contains(item), "{UUID} display leaked {item} in {text}");
    }
}

fn pull_all(mut pull: ParentPull) -> Result<(Vec<VerifiedBatch>, ParentParseComplete), ParentError> {
    let mut batches = Vec::new();
    loop {
        match pull.next_batch()? {
            Some(batch) => batches.push(batch),
            None => break,
        }
    }
    let done = pull.finish()?;
    Ok((batches, done))
}

fn i64_col(batch: &RecordBatch, index: usize) -> Vec<Option<i64>> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap_or_else(|| panic!("{UUID} int {index}"));
    (0..array.len())
        .map(|row| (!array.is_null(row)).then(|| array.value(row)))
        .collect()
}

fn utf8_col(batch: &RecordBatch, index: usize) -> Vec<Option<String>> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap_or_else(|| panic!("{UUID} utf8 {index}"));
    (0..array.len())
        .map(|row| (!array.is_null(row)).then(|| array.value(row).to_owned()))
        .collect()
}

fn bool_col(batch: &RecordBatch, index: usize) -> Vec<Option<bool>> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<BooleanArray>()
        .unwrap_or_else(|| panic!("{UUID} bool {index}"));
    (0..array.len())
        .map(|row| (!array.is_null(row)).then(|| array.value(row)))
        .collect()
}

fn decimal_col(batch: &RecordBatch, index: usize) -> Vec<Option<i128>> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<Decimal128Array>()
        .unwrap_or_else(|| panic!("{UUID} decimal {index}"));
    (0..array.len())
        .map(|row| (!array.is_null(row)).then(|| array.value(row)))
        .collect()
}

fn assert_frame(batch: &VerifiedBatch) {
    let decoded = batch
        .record
        .columns()
        .iter()
        .map(|column| column.get_array_memory_size() as u64)
        .sum::<u64>();
    assert!(batch.raw_bytes > 0, "{UUID} raw");
    assert!(batch.arrow_bytes > 0 && batch.arrow_bytes <= FRAME_CAP, "{UUID} lineage arrow");
    assert!(decoded > 0 && decoded <= FRAME_CAP, "{UUID} decoded arrow {decoded}");
}

fn assert_receipt(done: &ParentParseComplete, rows: u64, batches: u64, sha: [u8; 32]) {
    assert_eq!(done.rows(), rows, "{UUID} rows");
    assert_eq!(done.batches(), batches, "{UUID} batches");
    assert_eq!(done.object_sha256(), sha, "{UUID} sha");
    assert!(!done.admits_import(), "{UUID} admits_import");
    assert!(!done.published(), "{UUID} published");
    assert!(!done.pg_run_complete(), "{UUID} pg_run_complete");
    assert_eq!(done.arrow_ipc(), batches > 0, "{UUID} arrow_ipc");
    assert_eq!(KERNEL_RESTRICTION, KernelRestriction::Unsupported);
    assert_eq!(FILE_WORKER_PARENT_PROTOCOL, "oo.source.file-worker-parent/v1");
}

fn hasher_self_check() {
    assert_eq!(hex(&sha256_bytes(b"")), EMPTY_SHA, "{UUID} empty sha");
    assert_eq!(hex(&sha256_bytes(b"abc")), ABC_SHA, "{UUID} abc sha");
}

struct LotMarks {
    header_len: u64,
    rows: [u64; 3],
    skus: [u64; 3],
    notes: [u64; 3],
}

fn lot_csv() -> (Vec<u8>, LotMarks) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"sku,qty,price,note\n");
    let header_len = bytes.len() as u64;
    let mut rows = [0u64; 3];
    let mut skus = [0u64; 3];
    let mut notes = [0u64; 3];
    rows[0] = bytes.len() as u64;
    skus[0] = rows[0];
    bytes.extend_from_slice("甲".as_bytes());
    bytes.extend_from_slice(b",-9223372036854775808,3.140,");
    notes[0] = bytes.len() as u64;
    bytes.extend_from_slice(b"\"\"\n");
    rows[1] = bytes.len() as u64;
    skus[1] = rows[1];
    bytes.extend_from_slice(b",9223372036854775807,3.14,");
    notes[1] = bytes.len() as u64;
    bytes.extend_from_slice(b"~\n");
    rows[2] = bytes.len() as u64;
    skus[2] = rows[2];
    bytes.extend_from_slice("鹿".as_bytes());
    bytes.extend_from_slice(b",0,-0.010,");
    notes[2] = bytes.len() as u64;
    bytes.extend_from_slice(b"\"x\ny\"\n");
    (
        bytes,
        LotMarks {
            header_len,
            rows,
            skus,
            notes,
        },
    )
}

fn tab_tsv() -> (Vec<u8>, u64, u64) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice("鹿".as_bytes());
    bytes.push(b'\t');
    let raw0 = bytes.len() as u64;
    bytes.extend_from_slice(b"\"q\tt\"\t-7\n");
    let second = bytes.len() as u64;
    bytes.extend_from_slice(b"b,c\t");
    bytes.extend_from_slice("北京".as_bytes());
    bytes.extend_from_slice(b"\t-0\n");
    (bytes, raw0, second)
}

fn notes_jsonl() -> (Vec<u8>, [u64; 3], [u64; 3], [u64; 3]) {
    let records = [
        "{\"label\":\"丙\",\"flag\":true,\"n\":-9223372036854775808}\n",
        "{\"label\":null,\"flag\":false,\"n\":1}\n",
        "{\"flag\":true,\"n\":4}\n",
    ];
    let mut bytes = Vec::new();
    let mut starts = [0u64; 3];
    let mut labels = [0u64; 3];
    let mut raws = [0u64; 3];
    for (index, record) in records.iter().enumerate() {
        starts[index] = bytes.len() as u64;
        bytes.extend_from_slice(record.as_bytes());
        let start = starts[index] as usize;
        let end = bytes.len();
        raws[index] = (end - start - 1) as u64;
        if index < 2 {
            let key = b"\"label\"";
            let rel = bytes[start..end]
                .windows(key.len())
                .position(|window| window == key)
                .unwrap_or_else(|| panic!("{UUID} label key {index}"));
            labels[index] = (start + rel) as u64;
        } else {
            labels[index] = starts[index];
        }
    }
    (bytes, starts, labels, raws)
}

#[test]
fn production_csv_tsv_jsonl_clean_eof_private_complete() {
    hasher_self_check();
    assert_eq!(PARENT_DEADLINE_MIN_MS, 1);
    assert_eq!(PARENT_DEADLINE_MAX_MS, 120_000);
    let config = parent(20_000, worker_bin());

    let (csv_bytes, marks) = lot_csv();
    let (csv_path, csv_sha) = write_file("a237-lot", &csv_bytes);
    save_oracle("lot.csv", &csv_bytes, csv_sha);
    let csv_request = ChildRequest::try_new(
        &csv_path,
        csv_sha,
        SourceProfile::Delimited(lot_profile()),
        delimited_budget(),
    )
    .unwrap_or_else(|err| panic!("{UUID} csv request {err}"));
    let (csv_batches, csv_done) = pull_all(config.start(&csv_request).unwrap_or_else(|err| {
        panic!("{UUID} csv start {err}")
    }))
    .unwrap_or_else(|err| panic!("{UUID} csv {}", err));
    assert_eq!(csv_batches.len(), 1, "{UUID} csv batches");
    let batch = &csv_batches[0];
    assert_frame(batch);
    assert_eq!(batch.raw_bytes, csv_bytes.len() as u64 - marks.header_len, "{UUID} csv raw");
    assert_eq!(i64_col(&batch.record, 1), vec![Some(i64::MIN), Some(i64::MAX), Some(0)]);
    assert_eq!(
        utf8_col(&batch.record, 0),
        vec![Some("甲".to_owned()), Some(String::new()), Some("鹿".to_owned())]
    );
    assert_eq!(decimal_col(&batch.record, 2), vec![Some(3140), Some(3140), Some(-10)]);
    assert_eq!(utf8_col(&batch.record, 3)[0], Some(String::new()));
    assert_eq!(utf8_col(&batch.record, 3)[1], None);
    assert_eq!(utf8_col(&batch.record, 3)[2].as_deref(), Some("x\ny"));
    let price = batch.record.schema().field(2);
    assert_eq!(price.metadata().get("oo.delimited.logical").map(String::as_str), Some("decimal"));
    assert_eq!(price.metadata().get("oo.delimited.scale").map(String::as_str), Some("3"));
    let profile = lot_profile();
    assert_eq!(batch.profile_digest, profile.profile_digest());
    assert_eq!(batch.config_identity, profile.config_identity(csv_sha));
    assert_eq!(batch.rows[0].byte_offset, marks.rows[0]);
    assert_eq!(batch.rows[0].fields[0].byte_offset, marks.skus[0]);
    assert_eq!(batch.rows[0].fields[0].presence, "value");
    assert_eq!(batch.rows[1].fields[0].presence, "empty");
    assert_eq!(batch.rows[1].fields[0].byte_offset, marks.skus[1]);
    assert_eq!(batch.rows[0].fields[3].byte_offset, marks.notes[0]);
    assert_eq!(batch.rows[0].fields[3].presence, "empty");
    assert_eq!(batch.rows[1].fields[3].presence, "null");
    assert_eq!(batch.rows[2].fields[3].presence, "value");
    assert_eq!(batch.rows[2].fields[3].byte_offset, marks.notes[2]);
    assert_eq!(batch.rows[2].fields[3].line, 4);
    assert_eq!(batch.schema_identity, csv_done.schema_identity());
    assert_receipt(&csv_done, 3, 1, csv_sha);
    assert_eq!(csv_done.profile_id(), "a237-lot");
    assert_eq!(csv_done.reader_protocol(), "oo.source.delimited/v1");
    assert_eq!(csv_done.columns(), 4);
    assert_eq!(csv_done.profile_digest(), profile.profile_digest());
    println!(
        "{UUID} csv rows=3 batches=1 sha={} admits=false published=false pg=false kernel=unsupported",
        hex(&csv_sha)
    );

    let (tsv_bytes, raw0, second) = tab_tsv();
    let (tsv_path, tsv_sha) = write_file("a237-tab", &tsv_bytes);
    save_oracle("tab.tsv", &tsv_bytes, tsv_sha);
    let tsv_request = ChildRequest::try_new(
        &tsv_path,
        tsv_sha,
        SourceProfile::Delimited(tab_profile()),
        delimited_budget(),
    )
    .unwrap_or_else(|err| panic!("{UUID} tsv request {err}"));
    let (tsv_batches, tsv_done) = pull_all(config.start(&tsv_request).unwrap_or_else(|err| {
        panic!("{UUID} tsv start {err}")
    }))
    .unwrap_or_else(|err| panic!("{UUID} tsv {}", err));
    assert_eq!(tsv_batches.len(), 1);
    assert_frame(&tsv_batches[0]);
    assert_eq!(tsv_batches[0].raw_bytes, tsv_bytes.len() as u64, "{UUID} tsv raw");
    assert_eq!(
        utf8_col(&tsv_batches[0].record, 0),
        vec![Some("鹿".to_owned()), Some("b,c".to_owned())]
    );
    assert_eq!(
        utf8_col(&tsv_batches[0].record, 1),
        vec![Some("q\tt".to_owned()), Some("北京".to_owned())]
    );
    assert_eq!(i64_col(&tsv_batches[0].record, 2), vec![Some(-7), Some(0)]);
    assert_eq!(tsv_batches[0].rows[0].fields[1].byte_offset, raw0);
    assert_eq!(tsv_batches[0].rows[1].byte_offset, second);
    let tab = tab_profile();
    assert_eq!(tsv_batches[0].profile_digest, tab.profile_digest());
    assert_eq!(tsv_batches[0].config_identity, tab.config_identity(tsv_sha));
    assert_receipt(&tsv_done, 2, 1, tsv_sha);
    assert_eq!(tsv_done.profile_id(), "a237-tab");
    println!("{UUID} tsv rows=2 batches=1 sha={}", hex(&tsv_sha));

    let (jsonl_bytes, starts, labels, raws) = notes_jsonl();
    let (jsonl_path, jsonl_sha) = write_file("a237-notes", &jsonl_bytes);
    save_oracle("notes.jsonl", &jsonl_bytes, jsonl_sha);
    let mut pull = config
        .start(&request_notes(&jsonl_path, jsonl_sha, 1))
        .unwrap_or_else(|err| panic!("{UUID} jsonl start {err}"));
    let expected_n = [i64::MIN, 1, 4];
    let expected_flag = [true, false, true];
    let expected_presence = ["value", "null", "missing"];
    let notes = notes_profile();
    let mut schema = None;
    for index in 0..3 {
        let batch = pull
            .next_batch()
            .unwrap_or_else(|err| panic!("{UUID} jsonl batch {index} {err}"))
            .unwrap_or_else(|| panic!("{UUID} jsonl ended at {index}"));
        assert_eq!(batch.batch_index, index as u64);
        assert_frame(&batch);
        assert_eq!(batch.raw_bytes, raws[index], "{UUID} jsonl raw {index}");
        assert_eq!(i64_col(&batch.record, 2), vec![Some(expected_n[index])]);
        assert_eq!(bool_col(&batch.record, 1), vec![Some(expected_flag[index])]);
        assert_eq!(batch.rows[0].byte_offset, starts[index]);
        assert_eq!(batch.rows[0].fields[0].presence, expected_presence[index]);
        assert_eq!(batch.rows[0].fields[0].byte_offset, labels[index]);
        assert_eq!(batch.profile_digest, notes.profile_digest());
        assert_eq!(batch.config_identity, notes.config_identity(jsonl_sha));
        let logical = batch.record.schema().field(0).metadata().get("oo.jsonl.logical").cloned();
        assert_eq!(logical.as_deref(), Some("utf8"));
        schema = Some(batch.schema_identity);
        if index == 0 {
            assert_eq!(utf8_col(&batch.record, 0), vec![Some("丙".to_owned())]);
        }
        if index == 1 {
            assert_eq!(utf8_col(&batch.record, 0), vec![None]);
        }
        if index == 2 {
            assert_eq!(utf8_col(&batch.record, 0), vec![None]);
        }
    }
    assert!(pull.next_batch().unwrap_or_else(|err| panic!("{UUID} jsonl tail {err}")).is_none());
    let done = pull.finish().unwrap_or_else(|err| panic!("{UUID} jsonl finish {err}"));
    assert_receipt(&done, 3, 3, jsonl_sha);
    assert_eq!(done.reader_protocol(), "oo.source.jsonl/v1");
    assert_eq!(done.profile_id(), "a237-notes");
    assert_eq!(done.profile_digest(), notes.profile_digest());
    assert_eq!(Some(done.schema_identity()), schema);
    println!("{UUID} jsonl rows=3 batches=3 sha={}", hex(&jsonl_sha));

    let (empty_path, empty_sha) = write_file("a237-empty", b"");
    save_oracle("empty.jsonl", b"", empty_sha);
    let (empty_batches, empty_done) = pull_all(
        config
            .start(&request_notes(&empty_path, empty_sha, 4))
            .unwrap_or_else(|err| panic!("{UUID} empty start {err}")),
    )
    .unwrap_or_else(|err| panic!("{UUID} empty {}", err));
    assert!(empty_batches.is_empty(), "{UUID} empty batches");
    assert_receipt(&empty_done, 0, 0, empty_sha);
    assert_eq!(empty_done.columns(), 3);
    println!("{UUID} empty rows=0 batches=0 sha={} arrow_ipc=false", hex(&empty_sha));
}

#[test]
fn refusal_does_not_complete() {
    assert!(matches!(
        ParentConfig::try_new(worker_bin(), 0),
        Err(ParentError::Config("deadline"))
    ));
    assert!(matches!(
        ParentConfig::try_new(worker_bin(), 120_001),
        Err(ParentError::Config("deadline"))
    ));
    let relative = ParentConfig::try_new(Path::new("relative-worker"), 1000);
    assert!(matches!(relative, Err(ParentError::Config("worker_path"))));
    assert_hidden(&relative.unwrap_err(), &["relative-worker"]);
    let link = scratch("a237-worker-link");
    let _ = fs::remove_file(&link);
    symlink(worker_bin(), &link).unwrap_or_else(|err| panic!("{UUID} worker symlink {err}"));
    assert!(matches!(
        ParentConfig::try_new(&link, 1000),
        Err(ParentError::Config("worker_path"))
    ));

    let (path, sha) = write_file("a237-refuse-body", b"{\"label\":\"a237-secret-cell\",\"flag\":true,\"n\":1}\n");
    let dotted = ChildRequest::try_new(
        Path::new("/tmp/../etc/passwd"),
        sha,
        SourceProfile::Jsonl(notes_profile()),
        jsonl_budget(1),
    );
    assert!(matches!(dotted, Err(ParentError::Config("path_form"))));
    let input_link = scratch("a237-input-link");
    let _ = fs::remove_file(&input_link);
    symlink(&path, &input_link).unwrap_or_else(|err| panic!("{UUID} input symlink {err}"));
    let linked = ChildRequest::try_new(
        &input_link,
        sha,
        SourceProfile::Jsonl(notes_profile()),
        jsonl_budget(1),
    );
    assert!(matches!(linked, Err(ParentError::Config("not_regular"))));
    let mixed = ChildRequest::try_new(
        &path,
        sha,
        SourceProfile::Jsonl(notes_profile()),
        delimited_budget(),
    );
    assert!(matches!(mixed, Err(ParentError::Config("profile_budget"))));

    let config = parent(20_000, worker_bin());
    let mut wrong = sha;
    wrong[31] ^= 0x01;
    let err = pull_all(config.start(&request_notes(&path, wrong, 4)).unwrap_or_else(|err| {
        panic!("{UUID} hash start {err}")
    }))
    .expect_err("hash mismatch");
    let failed = failure(err.clone());
    assert_eq!(failed.kind, FailureKind::HashMismatch, "{UUID} hash kind");
    assert_eq!(failed.token, Some("hash_mismatch"));
    assert_eq!(failed.teardown.exit_code, Some(6));
    assert!(failed.teardown.reaped, "{UUID} hash reaped");
    assert_hidden(&err, &["a237-secret-cell", &path.display().to_string()]);

    let (bad_path, bad_sha) = write_file("a237-quote", b"sku\n\"a237-open\n");
    let one = DelimitedProfile::try_new(
        "a237-one",
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Exact,
        NullPolicy::Empty,
        vec![DelimitedColumn::try_new("sku", DelimitedType::Utf8, false).unwrap()],
    )
    .unwrap_or_else(|err| panic!("{UUID} one profile {err}"));
    let bad_request = ChildRequest::try_new(
        &bad_path,
        bad_sha,
        SourceProfile::Delimited(one),
        delimited_budget(),
    )
    .unwrap_or_else(|err| panic!("{UUID} quote request {err}"));
    let err = pull_all(config.start(&bad_request).unwrap_or_else(|err| panic!("{UUID} quote start {err}")))
        .expect_err("bad quote");
    let failed = failure(err.clone());
    assert_eq!(failed.teardown.exit_code, Some(4), "{UUID} quote exit");
    assert!(failed.teardown.reaped, "{UUID} quote reaped");
    assert_ne!(failed.kind, FailureKind::TimedOut);
    assert_hidden(&err, &["a237-open", &bad_path.display().to_string()]);
    println!("{UUID} refusal hash_exit=6 quote_exit=4 symlink=not_regular deadline=config");
}

fn fixture_dir() -> PathBuf {
    let root = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(std::env::temp_dir);
    root.join(format!("oo-a237-fixture-{UUID}"))
}

fn compile_fixture(name: &str, source: &str) -> PathBuf {
    let dir = fixture_dir();
    fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("{UUID} fixture dir {err}"));
    let src = dir.join(format!("{name}.rs"));
    let output = dir.join(name);
    fs::write(&src, source).unwrap_or_else(|err| panic!("{UUID} fixture source {err}"));
    let built = Command::new("rustc")
        .arg("--edition=2021")
        .arg("-D")
        .arg("warnings")
        .arg("-O")
        .arg(&src)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap_or_else(|err| panic!("{UUID} rustc spawn {err}"));
    assert!(
        built.status.success(),
        "{UUID} rustc {name} {}",
        String::from_utf8_lossy(&built.stderr)
    );
    output
}

fn fixture_bin() -> PathBuf {
    static ONCE: OnceLock<PathBuf> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _hold = compile_fixture(
            "a237-hold",
            r#"
fn main() {
    let path = std::env::args().nth(1).unwrap();
    std::fs::write(&path, std::process::id().to_string()).unwrap();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
}
"#,
        );
        let fixture = compile_fixture(
            "a237-fixture",
            r#"
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn setsid_bin() -> &'static str {
    if std::path::Path::new("/usr/bin/setsid").is_file() {
        "/usr/bin/setsid"
    } else {
        "/bin/setsid"
    }
}

fn main() {
    let mut input = Vec::new();
    let _ = io::stdin().read_to_end(&mut input);
    let text = String::from_utf8_lossy(&input);
    if text.contains("a237-stall") {
        loop {
            thread::sleep(Duration::from_secs(30));
        }
    } else if text.contains("a237-missing") {
        std::process::exit(0);
    } else if text.contains("a237-trailer") {
        let payload = b"a237-not-a-receipt\n";
        let mut out = Vec::new();
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
        out.push(b'Q');
        let _ = io::stdout().write_all(&out);
    } else if text.contains("a237-setsid") {
        let key = "\"path\":\"";
        let start = text.find(key).unwrap() + key.len();
        let end = text[start..].find('"').unwrap();
        let input_path = &text[start..start + end];
        let pid_path = format!("{input_path}.holdpid");
        let hold = std::env::current_exe().unwrap().parent().unwrap().join("a237-hold");
        let _child = Command::new(setsid_bin())
            .arg(&hold)
            .arg(&pid_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let began = Instant::now();
        while !std::path::Path::new(&pid_path).is_file() {
            if began.elapsed() > Duration::from_secs(1) {
                std::process::exit(7);
            }
            thread::sleep(Duration::from_millis(10));
        }
        loop {
            thread::sleep(Duration::from_secs(30));
        }
    } else {
        std::process::exit(9);
    }
}
"#,
        );
        fixture
    })
    .clone()
}

fn fixture_request(name: &str) -> (ParentConfig, ChildRequest, PathBuf) {
    let (path, sha) = write_file(name, b"not-a-table\n");
    let config = parent(20_000, &fixture_bin());
    let request = request_notes(&path, sha, 1);
    (config, request, path)
}

#[test]
fn missing_completion_and_trailer_are_not_receipts() {
    let (config, request, path) = fixture_request("a237-missing");
    let err = pull_all(config.start(&request).unwrap_or_else(|err| panic!("{UUID} missing start {err}")))
        .expect_err("missing completion");
    let failed = failure(err.clone());
    assert_eq!(failed.kind, FailureKind::Ended, "{UUID} missing kind");
    assert_eq!(failed.teardown.exit_code, Some(0), "{UUID} missing exit");
    assert!(failed.teardown.reaped, "{UUID} missing reaped");
    assert_hidden(&err, &[&path.display().to_string()]);

    let (config, request, path) = fixture_request("a237-trailer");
    let err = pull_all(config.start(&request).unwrap_or_else(|err| panic!("{UUID} trailer start {err}")))
        .expect_err("trailer");
    let failed = failure(err.clone());
    assert_eq!(failed.kind, FailureKind::Trailer, "{UUID} trailer kind");
    assert!(failed.teardown.reaped, "{UUID} trailer reaped");
    assert_hidden(&err, &[&path.display().to_string(), "a237-not-a-receipt"]);
    println!("{UUID} missing=ended trailer=trailer");
}

#[test]
fn deadline_kills_waits_and_the_next_call_recovers() {
    let bin = fixture_bin();
    let (path, sha) = write_file("a237-stall", b"sleep\n");
    let config = parent(800, &bin);
    let started = Instant::now();
    let err = pull_all(
        config
            .start(&request_notes(&path, sha, 1))
            .unwrap_or_else(|err| panic!("{UUID} stall start {err}")),
    )
    .expect_err("stall");
    let elapsed = started.elapsed();
    let failed = failure(err);
    println!(
        "{UUID} stall_elapsed_ms={} kind={:?} exit={:?} signal={:?} group={:?} reaped={} alive={}",
        elapsed.as_millis(),
        failed.kind,
        failed.teardown.exit_code,
        failed.teardown.term_signal,
        failed.teardown.group_signal,
        failed.teardown.reaped,
        failed.teardown.pid_alive
    );
    assert_eq!(failed.kind, FailureKind::TimedOut, "{UUID} stall kind");
    assert!(failed.teardown.reaped, "{UUID} stall reaped");
    assert!(!failed.teardown.pid_alive, "{UUID} stall pid");
    assert_eq!(failed.teardown.term_signal, Some(9), "{UUID} stall signal");
    assert_eq!(failed.teardown.group_signal, GroupSignal::Sent, "{UUID} stall group");
    assert!(elapsed >= Duration::from_millis(700) && elapsed < Duration::from_millis(5_000));
    assert!(failed.teardown.elapsed < Duration::from_millis(5_000));

    let recover = b"{\"label\":\"\xe4\xb8\x81\",\"flag\":true,\"n\":2}\n";
    let (recover_path, recover_sha) = write_file("a237-recover", recover);
    save_oracle("recover.jsonl", recover, recover_sha);
    let next = parent(20_000, worker_bin());
    let (batches, done) = pull_all(
        next.start(&request_notes(&recover_path, recover_sha, 4))
            .unwrap_or_else(|err| panic!("{UUID} recover start {err}")),
    )
    .unwrap_or_else(|err| panic!("{UUID} recover {}", err));
    assert_eq!(batches.len(), 1);
    assert_eq!(utf8_col(&batches[0].record, 0), vec![Some("丁".to_owned())]);
    assert_eq!(i64_col(&batches[0].record, 2), vec![Some(2)]);
    assert_receipt(&done, 1, 1, recover_sha);
    println!("{UUID} recover rows=1 sha={}", hex(&recover_sha));
}

struct HoldGuard(Option<u32>);

impl Drop for HoldGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.0.take() {
            let _ = Command::new("/bin/kill")
                .arg("-KILL")
                .arg("--")
                .arg(pid.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

fn session_of(pid: u32) -> u32 {
    let text = fs::read_to_string(format!("/proc/{pid}/stat"))
        .unwrap_or_else(|err| panic!("{UUID} stat {pid} {err}"));
    let rest = text
        .rsplit_once(')')
        .unwrap_or_else(|| panic!("{UUID} stat form {text}"))
        .1;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    fields[3].parse().unwrap_or_else(|err| panic!("{UUID} session {err}"))
}

#[test]
fn setsid_grandchild_survives_group_kill() {
    let bin = fixture_bin();
    let (path, sha) = write_file("a237-setsid", b"escape\n");
    let pid_path = PathBuf::from(format!("{}.holdpid", path.display()));
    let _ = fs::remove_file(&pid_path);
    let config = parent(4_000, &bin);
    let err = pull_all(
        config
            .start(&request_notes(&path, sha, 1))
            .unwrap_or_else(|err| panic!("{UUID} setsid start {err}")),
    )
    .expect_err("setsid stall");
    let failed = failure(err);
    assert_eq!(failed.kind, FailureKind::TimedOut, "{UUID} setsid kind");
    assert!(failed.teardown.reaped, "{UUID} setsid reaped");
    assert!(!failed.teardown.pid_alive, "{UUID} setsid direct pid");
    assert_eq!(failed.teardown.group_signal, GroupSignal::Sent, "{UUID} setsid group");
    let pid_text = fs::read_to_string(&pid_path).unwrap_or_else(|err| panic!("{UUID} hold pid {err}"));
    let hold_pid: u32 = pid_text.trim().parse().unwrap_or_else(|err| panic!("{UUID} hold parse {err}"));
    let _guard = HoldGuard(Some(hold_pid));
    assert!(Path::new(&format!("/proc/{hold_pid}")).exists(), "{UUID} hold died with the group");
    let session = session_of(hold_pid);
    assert_eq!(session, hold_pid, "{UUID} hold session {session} pid {hold_pid}");
    println!(
        "{UUID} setsid hold_pid={hold_pid} session={session} direct_reaped=true direct_alive=false group=Sent kernel=unsupported"
    );
    assert_eq!(KERNEL_RESTRICTION, KernelRestriction::Unsupported);
    let killed = Command::new("/bin/kill")
        .arg("-KILL")
        .arg("--")
        .arg(hold_pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap_or_else(|err| panic!("{UUID} kill hold {err}"));
    assert!(killed.success(), "{UUID} kill hold status {killed}");
    let mut gone = false;
    for _ in 0..50 {
        if !Path::new(&format!("/proc/{hold_pid}")).exists() {
            gone = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(gone, "{UUID} hold still alive after the review cleanup");
    println!("{UUID} setsid cleanup hold_alive=false");
}
