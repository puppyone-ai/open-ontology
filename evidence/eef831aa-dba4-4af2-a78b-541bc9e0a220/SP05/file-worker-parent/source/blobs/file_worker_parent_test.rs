//! Parent supervision of the compiled file-parser worker and of small fixture executables.
//!
//! Hash oracles are `sha2`. The fixture compiler is `rustc`, not a shell and not Python.
//! No PostgreSQL is started.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use arrow_array::{Array, BooleanArray, Decimal128Array, Int64Array, StringArray};
use oo_connectors::{
    BomPolicy, ChildRequest, DelimitedColumn, DelimitedDialect, DelimitedProfile, DelimitedType,
    FailureKind, HeaderPolicy, JsonlColumn, JsonlProfile, JsonlType, NullPolicy, ParentConfig,
    ParentError, ParentParseComplete, ParentPull, PullBudget, SourceProfile, VerifiedBatch,
    FILE_WORKER_PARENT_PROTOCOL, KERNEL_RESTRICTION, PARENT_DEADLINE_MAX_MS, PARENT_DEADLINE_MIN_MS,
    KernelRestriction,
};
use sha2::{Digest, Sha256};

const UUID: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a220";

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oo-file-worker-parent-{UUID}"));
    fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("{UUID} temp {err}"));
    dir.join(format!("{name}-{}", std::process::id()))
}

fn write_file(name: &str, bytes: &[u8]) -> (PathBuf, [u8; 32]) {
    let path = scratch(name);
    fs::write(&path, bytes).unwrap_or_else(|err| panic!("{UUID} write {err}"));
    (path, sha256_bytes(bytes))
}

fn worker_bin() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_oo-file-parser-worker"))
}

fn parent(deadline_ms: u64, bin: &Path) -> ParentConfig {
    ParentConfig::try_new(bin, deadline_ms).unwrap_or_else(|err| panic!("{UUID} config {err}"))
}

fn jsonl_budget(batch_rows: u32) -> PullBudget {
    PullBudget::jsonl(
        batch_rows,
        1_048_576,
        65_536,
        1_048_576,
        1000,
        16,
        64,
        60_000,
    )
    .unwrap_or_else(|err| panic!("{UUID} jsonl budget {err}"))
}

fn delimited_budget() -> PullBudget {
    PullBudget::delimited(32, 1_048_576, 65_536, 8192, 1_048_576, 1000, 16, 64, 60_000)
        .unwrap_or_else(|err| panic!("{UUID} delimited budget {err}"))
}

fn people_profile() -> JsonlProfile {
    JsonlProfile::try_new(
        "people",
        vec![
            JsonlColumn::try_new("name", JsonlType::Utf8, true).unwrap(),
            JsonlColumn::try_new("ok", JsonlType::Bool, false).unwrap(),
            JsonlColumn::try_new("n", JsonlType::Int64, false).unwrap(),
        ],
    )
    .unwrap()
}

fn enterprise_profile() -> DelimitedProfile {
    DelimitedProfile::try_new(
        "enterprise",
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Exact,
        NullPolicy::Token("\\N".to_owned()),
        vec![
            DelimitedColumn::try_new("id", DelimitedType::Int64, false).unwrap(),
            DelimitedColumn::try_new("name", DelimitedType::Utf8, true).unwrap(),
            DelimitedColumn::try_new("ok", DelimitedType::Bool, false).unwrap(),
            DelimitedColumn::try_new(
                "amount",
                DelimitedType::Decimal {
                    precision: 10,
                    scale: 2,
                },
                false,
            )
            .unwrap(),
            DelimitedColumn::try_new("note", DelimitedType::Utf8, true).unwrap(),
        ],
    )
    .unwrap()
}

fn tsv_profile() -> DelimitedProfile {
    DelimitedProfile::try_new(
        "tsv-sample",
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
    .unwrap()
}

fn request_jsonl(path: &Path, sha: [u8; 32], batch_rows: u32) -> ChildRequest {
    ChildRequest::try_new(
        path,
        sha,
        SourceProfile::Jsonl(people_profile()),
        jsonl_budget(batch_rows),
    )
    .unwrap_or_else(|err| panic!("{UUID} jsonl request {err}"))
}

fn failure(err: ParentError) -> oo_connectors::Failure {
    match err {
        ParentError::Failed(failure) => failure,
        ParentError::Config(token) => panic!("{UUID} config {token}"),
    }
}

fn assert_not_complete(err: ParentError) -> oo_connectors::Failure {
    let text = err.to_string();
    assert!(
        !text.contains(FILE_WORKER_PARENT_PROTOCOL) || text.contains("file worker parent"),
        "{UUID} display {text}"
    );
    assert!(!text.contains('/'), "{UUID} display leaked a path {text}");
    failure(err)
}

fn pull_all(mut pull: ParentPull) -> Result<(Vec<VerifiedBatch>, ParentParseComplete), ParentError> {
    let mut batches = Vec::new();
    while let Some(batch) = pull.next_batch()? {
        batches.push(batch);
    }
    let done = pull.finish()?;
    Ok((batches, done))
}

fn i64_col(batch: &arrow_array::RecordBatch, index: usize) -> Vec<Option<i64>> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap_or_else(|| panic!("{UUID} int64 {index}"));
    (0..array.len())
        .map(|row| (!array.is_null(row)).then(|| array.value(row)))
        .collect()
}

fn utf8_col(batch: &arrow_array::RecordBatch, index: usize) -> Vec<Option<String>> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap_or_else(|| panic!("{UUID} utf8 {index}"));
    (0..array.len())
        .map(|row| {
            if array.is_null(row) {
                None
            } else {
                Some(array.value(row).to_owned())
            }
        })
        .collect()
}

fn bool_col(batch: &arrow_array::RecordBatch, index: usize) -> Vec<Option<bool>> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<BooleanArray>()
        .unwrap_or_else(|| panic!("{UUID} bool {index}"));
    (0..array.len())
        .map(|row| (!array.is_null(row)).then(|| array.value(row)))
        .collect()
}

fn decimal_col(batch: &arrow_array::RecordBatch, index: usize) -> Vec<Option<i128>> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<Decimal128Array>()
        .unwrap_or_else(|| panic!("{UUID} decimal {index}"));
    (0..array.len())
        .map(|row| (!array.is_null(row)).then(|| array.value(row)))
        .collect()
}

fn enterprise_csv() -> (Vec<u8>, [u64; 3], [u64; 3], [u64; 3]) {
    let mut bytes = Vec::new();
    let mut rows = [0u64; 3];
    let mut names = [0u64; 3];
    let mut notes = [0u64; 3];
    bytes.extend_from_slice(b"id,name,ok,amount,note\n");
    rows[0] = bytes.len() as u64;
    bytes.extend_from_slice(b"-9223372036854775808,");
    names[0] = bytes.len() as u64;
    bytes.extend_from_slice("中文".as_bytes());
    bytes.extend_from_slice(b",true,1.20,");
    notes[0] = bytes.len() as u64;
    bytes.extend_from_slice(b"\"\"\n");
    rows[1] = bytes.len() as u64;
    bytes.extend_from_slice(b"9223372036854775807,");
    names[1] = bytes.len() as u64;
    bytes.extend_from_slice("猫".as_bytes());
    bytes.extend_from_slice(b",false,1.2,");
    notes[1] = bytes.len() as u64;
    bytes.extend_from_slice(b"\\N\n");
    rows[2] = bytes.len() as u64;
    bytes.extend_from_slice(b"0,");
    names[2] = bytes.len() as u64;
    bytes.extend_from_slice(b",true,0.00,");
    notes[2] = bytes.len() as u64;
    bytes.extend_from_slice(b"\"line\ntwo\"\n");
    (bytes, rows, names, notes)
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
}

#[test]
fn deadline_bounds_and_paths_do_not_spawn() {
    assert_eq!(PARENT_DEADLINE_MIN_MS, 1);
    assert_eq!(PARENT_DEADLINE_MAX_MS, 120_000);
    assert!(matches!(
        ParentConfig::try_new(worker_bin(), 0),
        Err(ParentError::Config("deadline"))
    ));
    assert!(matches!(
        ParentConfig::try_new(worker_bin(), 120_001),
        Err(ParentError::Config("deadline"))
    ));
    assert!(ParentConfig::try_new(worker_bin(), 120_000).is_ok());
    assert!(matches!(
        ParentConfig::try_new(Path::new("relative-worker"), 1000),
        Err(ParentError::Config("worker_path"))
    ));
    let (path, sha) = write_file("bounds-empty", b"");
    let relative = ChildRequest::try_new(
        Path::new("relative-input"),
        sha,
        SourceProfile::Jsonl(people_profile()),
        jsonl_budget(1),
    );
    assert!(matches!(relative, Err(ParentError::Config("path_form"))));
    let dotted = ChildRequest::try_new(
        Path::new("/tmp/../etc/passwd"),
        sha,
        SourceProfile::Jsonl(people_profile()),
        jsonl_budget(1),
    );
    assert!(matches!(dotted, Err(ParentError::Config("path_form"))));
    let link = scratch("bounds-link");
    let _ = fs::remove_file(&link);
    symlink(&path, &link).unwrap_or_else(|err| panic!("{UUID} symlink {err}"));
    let linked = ChildRequest::try_new(
        &link,
        sha,
        SourceProfile::Jsonl(people_profile()),
        jsonl_budget(1),
    );
    assert!(matches!(linked, Err(ParentError::Config("not_regular"))));
}

#[test]
fn csv_tsv_and_jsonl_round_trip_exact_values() {
    let config = parent(20_000, worker_bin());
    let (csv_bytes, rows, names, notes) = enterprise_csv();
    let (csv_path, csv_sha) = write_file("enterprise", &csv_bytes);
    let csv_request = ChildRequest::try_new(
        &csv_path,
        csv_sha,
        SourceProfile::Delimited(enterprise_profile()),
        delimited_budget(),
    )
    .unwrap();
    let (csv_batches, csv_done) = pull_all(config.start(&csv_request).unwrap())
        .unwrap_or_else(|err| panic!("{UUID} csv {}", err));
    assert_eq!(csv_batches.len(), 1, "{UUID} csv batches");
    let batch = &csv_batches[0].record;
    assert_eq!(
        i64_col(batch, 0),
        vec![Some(i64::MIN), Some(i64::MAX), Some(0)]
    );
    assert_eq!(
        utf8_col(batch, 1),
        vec![
            Some("中文".to_owned()),
            Some("猫".to_owned()),
            Some(String::new())
        ]
    );
    assert_eq!(
        bool_col(batch, 2),
        vec![Some(true), Some(false), Some(true)]
    );
    assert_eq!(decimal_col(batch, 3), vec![Some(120), Some(120), Some(0)]);
    assert_eq!(utf8_col(batch, 4)[0], Some(String::new()));
    assert_eq!(utf8_col(batch, 4)[1], None);
    assert_eq!(utf8_col(batch, 4)[2].as_deref(), Some("line\ntwo"));
    let schema = batch.schema();
    let amount = schema.field(3);
    assert_eq!(
        amount.metadata().get("oo.delimited.logical").map(String::as_str),
        Some("decimal")
    );
    assert_eq!(
        amount.metadata().get("oo.delimited.scale").map(String::as_str),
        Some("2")
    );
    let profile = enterprise_profile();
    assert_eq!(csv_batches[0].profile_digest, profile.profile_digest());
    assert_eq!(
        csv_batches[0].config_identity,
        profile.config_identity(csv_sha)
    );
    assert_eq!(csv_batches[0].rows.len(), 3);
    assert_eq!(csv_batches[0].rows[0].byte_offset, rows[0]);
    assert_eq!(csv_batches[0].rows[0].fields[1].byte_offset, names[0]);
    assert_eq!(csv_batches[0].rows[0].fields[1].presence, "value");
    assert_eq!(csv_batches[0].rows[2].fields[1].presence, "empty");
    assert_eq!(csv_batches[0].rows[0].fields[4].byte_offset, notes[0]);
    assert_eq!(csv_batches[0].rows[0].fields[4].presence, "empty");
    assert_eq!(csv_batches[0].rows[1].fields[4].presence, "null");
    assert_eq!(csv_batches[0].rows[2].fields[4].presence, "value");
    assert_eq!(csv_batches[0].rows[2].fields[4].line, 4);
    assert_receipt(&csv_done, 3, 1, csv_sha);
    assert_eq!(csv_done.profile_id(), "enterprise");
    assert_eq!(csv_done.reader_protocol(), "oo.source.delimited/v1");
    assert_eq!(csv_done.columns(), 5);

    let mut tsv = Vec::new();
    tsv.extend_from_slice("熊".as_bytes());
    tsv.extend_from_slice(b"\t\"a\tb\"\t7\n");
    let second = tsv.len() as u64;
    tsv.extend_from_slice("猫,鼠".as_bytes());
    tsv.extend_from_slice(b"\t");
    tsv.extend_from_slice("上海".as_bytes());
    tsv.extend_from_slice(b"\t-0");
    let (tsv_path, tsv_sha) = write_file("tsv", &tsv);
    let tsv_request = ChildRequest::try_new(
        &tsv_path,
        tsv_sha,
        SourceProfile::Delimited(tsv_profile()),
        delimited_budget(),
    )
    .unwrap();
    let (tsv_batches, tsv_done) = pull_all(config.start(&tsv_request).unwrap())
        .unwrap_or_else(|err| panic!("{UUID} tsv {}", err));
    assert_eq!(tsv_batches.len(), 1);
    assert_eq!(
        utf8_col(&tsv_batches[0].record, 0),
        vec![Some("熊".to_owned()), Some("猫,鼠".to_owned())]
    );
    assert_eq!(
        utf8_col(&tsv_batches[0].record, 1),
        vec![Some("a\tb".to_owned()), Some("上海".to_owned())]
    );
    assert_eq!(i64_col(&tsv_batches[0].record, 2), vec![Some(7), Some(0)]);
    assert_eq!(tsv_batches[0].rows[1].byte_offset, second);
    assert_receipt(&tsv_done, 2, 1, tsv_sha);

    let mut jsonl = Vec::new();
    let mut starts = Vec::new();
    for record in [
        r#"{"name":"中文","ok":true,"n":-9223372036854775808}"#,
        r#"{"name":null,"ok":false,"n":0}"#,
        r#"{"ok":true,"n":9223372036854775807}"#,
    ] {
        starts.push(jsonl.len() as u64);
        jsonl.extend_from_slice(record.as_bytes());
        jsonl.push(b'\n');
    }
    let (jsonl_path, jsonl_sha) = write_file("jsonl", &jsonl);
    let jsonl_request = request_jsonl(&jsonl_path, jsonl_sha, 1);
    let mut pull = config.start(&jsonl_request).unwrap();
    let expected_n = [i64::MIN, 0, i64::MAX];
    let expected_bool = [true, false, true];
    let expected_presence = ["value", "null", "missing"];
    let profile = people_profile();
    for index in 0..3 {
        let batch = pull
            .next_batch()
            .unwrap_or_else(|err| panic!("{UUID} jsonl batch {index} {err}"))
            .unwrap_or_else(|| panic!("{UUID} jsonl ended at {index}"));
        assert_eq!(batch.batch_index, index as u64);
        assert_eq!(i64_col(&batch.record, 2), vec![Some(expected_n[index])]);
        assert_eq!(bool_col(&batch.record, 1), vec![Some(expected_bool[index])]);
        assert_eq!(batch.rows[0].fields[0].presence, expected_presence[index]);
        assert_eq!(batch.rows[0].byte_offset, starts[index]);
        assert_eq!(batch.profile_digest, profile.profile_digest());
        assert_eq!(batch.config_identity, profile.config_identity(jsonl_sha));
        if index == 0 {
            assert_eq!(utf8_col(&batch.record, 0), vec![Some("中文".to_owned())]);
            let name_key = jsonl
                .windows(6)
                .position(|window| window == b"\"name\"")
                .unwrap() as u64;
            assert_eq!(batch.rows[0].fields[0].byte_offset, name_key);
        }
        if index == 2 {
            assert_eq!(utf8_col(&batch.record, 0), vec![None]);
            assert_eq!(batch.rows[0].fields[0].byte_offset, starts[2]);
        }
    }
    assert!(pull.next_batch().unwrap().is_none(), "{UUID} jsonl tail");
    let done = pull.finish().unwrap_or_else(|err| panic!("{UUID} jsonl finish {err}"));
    assert_receipt(&done, 3, 3, jsonl_sha);
    assert_eq!(done.reader_protocol(), "oo.source.jsonl/v1");
    assert_eq!(done.profile_digest(), profile.profile_digest());
}

#[test]
fn empty_jsonl_completes_without_import() {
    let (path, sha) = write_file("empty-jsonl", b"");
    let config = parent(20_000, worker_bin());
    let pull = config
        .start(&request_jsonl(&path, sha, 8))
        .unwrap_or_else(|err| panic!("{UUID} empty start {err}"));
    let (batches, done) = pull_all(pull).unwrap_or_else(|err| panic!("{UUID} empty {err}"));
    assert!(batches.is_empty(), "{UUID} empty batches");
    assert_receipt(&done, 0, 0, sha);
    assert_eq!(done.columns(), 3);
}

#[test]
fn hash_mismatch_nonzero_and_overflow_are_not_complete() {
    let config = parent(20_000, worker_bin());
    let (path, sha) = write_file("hash-body", b"{\"name\":\"a\",\"ok\":true,\"n\":1}\n");
    let mut wrong = sha;
    wrong[0] ^= 0xff;
    let err = pull_all(config.start(&request_jsonl(&path, wrong, 8)).unwrap())
        .expect_err("hash mismatch");
    let failure = assert_not_complete(err);
    assert_eq!(failure.kind, FailureKind::HashMismatch, "{UUID} hash kind");
    assert_eq!(failure.token, Some("hash_mismatch"));
    assert_eq!(failure.teardown.exit_code, Some(6));
    assert!(failure.teardown.reaped, "{UUID} hash reaped");

    let (bad_path, bad_sha) = write_file("quote", b"id\n\"unterminated\n");
    let one = DelimitedProfile::try_new(
        "one",
        DelimitedDialect::Csv,
        BomPolicy::Refuse,
        HeaderPolicy::Exact,
        NullPolicy::Empty,
        vec![DelimitedColumn::try_new("id", DelimitedType::Utf8, false).unwrap()],
    )
    .unwrap();
    let bad_request = ChildRequest::try_new(
        &bad_path,
        bad_sha,
        SourceProfile::Delimited(one),
        delimited_budget(),
    )
    .unwrap();
    let err = pull_all(config.start(&bad_request).unwrap()).expect_err("malformed csv");
    let failure = assert_not_complete(err);
    assert_eq!(failure.teardown.exit_code, Some(4), "{UUID} quote exit");
    assert!(failure.teardown.reaped, "{UUID} quote reaped");
    assert_ne!(failure.kind, FailureKind::TimedOut);

    let (over_path, over_sha) = write_file("overflow", b"{\"n\":9223372036854775808}\n");
    let over_profile = JsonlProfile::try_new(
        "wide",
        vec![JsonlColumn::try_new("n", JsonlType::Int64, false).unwrap()],
    )
    .unwrap();
    let over_request = ChildRequest::try_new(
        &over_path,
        over_sha,
        SourceProfile::Jsonl(over_profile),
        jsonl_budget(1),
    )
    .unwrap();
    let err = pull_all(config.start(&over_request).unwrap()).expect_err("overflow");
    let failure = assert_not_complete(err);
    assert_eq!(failure.token, Some("protocol"), "{UUID} overflow token");
    assert_eq!(failure.teardown.exit_code, Some(4));
    assert!(failure.teardown.reaped, "{UUID} overflow reaped");
}

#[test]
fn early_finish_is_not_a_receipt_and_the_next_call_works() {
    let (path, sha) = write_file("early-finish", b"");
    let config = parent(20_000, worker_bin());
    let pull = config.start(&request_jsonl(&path, sha, 4)).unwrap();
    let err = pull.finish().expect_err("early finish");
    let failure = assert_not_complete(err);
    assert_eq!(failure.kind, FailureKind::Protocol);
    assert!(failure.teardown.reaped, "{UUID} early finish reaped");
    let again = config.start(&request_jsonl(&path, sha, 4)).unwrap();
    let (batches, done) = pull_all(again).unwrap_or_else(|err| panic!("{UUID} next {err}"));
    assert!(batches.is_empty());
    assert_receipt(&done, 0, 0, sha);
}

fn fixture_dir() -> PathBuf {
    let root = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(std::env::temp_dir);
    root.join(format!("oo-file-worker-parent-fixture-{UUID}"))
}

fn fixture_bin() -> PathBuf {
    static ONCE: OnceLock<PathBuf> = OnceLock::new();
    ONCE.get_or_init(|| {
        let dir = fixture_dir();
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("fixture.rs");
        fs::write(&source, FIXTURE).unwrap();
        let output = dir.join("fixture");
        let built = Command::new("rustc")
            .arg("--edition=2021")
            .arg("-O")
            .arg(&source)
            .arg("-o")
            .arg(&output)
            .output()
            .unwrap_or_else(|err| panic!("{UUID} rustc spawn {err}"));
        assert!(
            built.status.success(),
            "{UUID} rustc {:?}",
            String::from_utf8_lossy(&built.stderr)
        );
        output
    })
    .clone()
}

const FIXTURE: &str = r#"
use std::io::{self, Read, Write};
use std::thread;
use std::time::Duration;

fn main() {
    let mut input = Vec::new();
    let _ = io::stdin().read_to_end(&mut input);
    let text = String::from_utf8_lossy(&input);
    if text.contains("mode-stall") {
        loop {
            thread::sleep(Duration::from_secs(30));
        }
    } else if text.contains("mode-malformed") {
        let _ = io::stdout().write_all(&[9, 0, 0, 0, 1, 0, 0, 0, b'X']);
    } else if text.contains("mode-trailer") {
        let payload = b"early\n";
        let mut out = Vec::new();
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
        out.push(b'Z');
        let _ = io::stdout().write_all(&out);
    } else if text.contains("mode-early") {
        let payload = b"early\n";
        let mut out = Vec::new();
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
        let _ = io::stdout().write_all(&out);
    } else {
        std::process::exit(9);
    }
}
"#;

fn fixture_request(name: &str) -> (ParentConfig, ChildRequest) {
    let (path, sha) = write_file(name, b"not-a-real-table\n");
    let config = parent(20_000, &fixture_bin());
    let request = request_jsonl(&path, sha, 1);
    (config, request)
}

#[test]
fn fixture_malformed_trailer_and_early_complete_are_not_receipts() {
    let (config, request) = fixture_request("mode-malformed");
    let err = pull_all(config.start(&request).unwrap()).expect_err("malformed");
    let failure = assert_not_complete(err);
    assert_eq!(failure.kind, FailureKind::Malformed, "{UUID} malformed");
    assert!(failure.teardown.reaped, "{UUID} malformed reaped");

    let (config, request) = fixture_request("mode-trailer");
    let err = pull_all(config.start(&request).unwrap()).expect_err("trailer");
    let failure = assert_not_complete(err);
    assert_eq!(failure.kind, FailureKind::Trailer, "{UUID} trailer");
    assert!(failure.teardown.reaped, "{UUID} trailer reaped");

    let (config, request) = fixture_request("mode-early");
    let err = pull_all(config.start(&request).unwrap()).expect_err("early");
    let failure = assert_not_complete(err);
    assert_eq!(failure.kind, FailureKind::Protocol, "{UUID} early complete");
    assert_eq!(failure.teardown.exit_code, Some(0), "{UUID} early exit");
    assert!(failure.teardown.reaped, "{UUID} early reaped");
}

#[test]
fn stall_is_killed_and_the_next_call_still_completes() {
    let bin = fixture_bin();
    let (path, sha) = write_file("mode-stall", b"sleep\n");
    let config = parent(800, &bin);
    let started = std::time::Instant::now();
    let err = pull_all(config.start(&request_jsonl(&path, sha, 1)).unwrap()).expect_err("stall");
    let elapsed = started.elapsed();
    let failure = assert_not_complete(err);
    println!(
        "{UUID} stall_elapsed_ms={} kind={:?} exit={:?} signal={:?} group={:?} reaped={} alive={}",
        elapsed.as_millis(),
        failure.kind,
        failure.teardown.exit_code,
        failure.teardown.term_signal,
        failure.teardown.group_signal,
        failure.teardown.reaped,
        failure.teardown.pid_alive
    );
    assert_eq!(failure.kind, FailureKind::TimedOut, "{UUID} stall kind");
    assert!(failure.teardown.reaped, "{UUID} stall reaped");
    assert!(!failure.teardown.pid_alive, "{UUID} stall pid");
    assert!(
        elapsed < Duration::from_millis(5_000),
        "{UUID} stall elapsed {} ms",
        elapsed.as_millis()
    );
    assert!(
        failure.teardown.elapsed < Duration::from_millis(5_000),
        "{UUID} teardown elapsed {:?}",
        failure.teardown.elapsed
    );

    let (empty, empty_sha) = write_file("after-stall", b"");
    let next = parent(20_000, worker_bin());
    let (batches, done) = pull_all(next.start(&request_jsonl(&empty, empty_sha, 2)).unwrap())
        .unwrap_or_else(|err| panic!("{UUID} after stall {err}"));
    assert!(batches.is_empty());
    assert_receipt(&done, 0, 0, empty_sha);
}

#[test]
fn one_millisecond_cannot_complete() {
    let bin = fixture_bin();
    let (path, sha) = write_file("mode-stall-1ms", b"sleep\n");
    let config = parent(1, &bin);
    let started = config.start(&request_jsonl(&path, sha, 1));
    let err = match started {
        Err(err) => err,
        Ok(pull) => pull_all(pull).expect_err("1ms pull"),
    };
    let failure = assert_not_complete(err);
    assert_eq!(failure.kind, FailureKind::TimedOut, "{UUID} 1ms");
    assert!(
        failure.teardown.elapsed < Duration::from_millis(5_000),
        "{UUID} 1ms elapsed {:?}",
        failure.teardown.elapsed
    );
}
