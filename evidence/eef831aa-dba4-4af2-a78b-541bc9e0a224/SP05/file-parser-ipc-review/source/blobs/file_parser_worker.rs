//! Spawns the compiled `oo-file-parser-worker` binary and decodes its Arrow IPC.
//!
//! The harness is the parent. It clears the child environment, reads bounded
//! frames, and kills and reaps the child on a timeout or a test panic.
//! SHA-256 oracles come from `sha256sum`, not from the worker's `sha2` crate.
//! No PostgreSQL is started. Exit and stderr failures name this session.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use arrow_array::{Array, BooleanArray, Decimal128Array, Int64Array, RecordBatch, StringArray};
use arrow_ipc::reader::StreamReader;

const UUID: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a207";
const PROTOCOL: &str = "oo.source.file-parser-worker/v1";
const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const TAG_IPC: u32 = 1;
const TAG_LINEAGE: u32 = 2;
const TAG_COMPLETE: u32 = 3;
const MAX_PAYLOAD: usize = 2_097_152;
const MAX_FRAMES: usize = 64;

struct Frame {
    tag: u32,
    payload: Vec<u8>,
}

struct Outcome {
    code: Option<i32>,
    stderr: Vec<u8>,
    frames: Vec<Frame>,
}

struct Reap {
    child: Option<Child>,
}

impl Drop for Reap {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("oo-file-parser-{UUID}"));
    fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("{UUID} temp dir {err}"));
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    dir.join(format!("{name}-{}-{n}", std::process::id()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let path = scratch("oracle");
    fs::write(&path, bytes).unwrap_or_else(|err| panic!("{UUID} oracle write {err}"));
    let output = Command::new("/usr/bin/sha256sum")
        .arg(&path)
        .env_clear()
        .output()
        .unwrap_or_else(|err| panic!("{UUID} sha256sum {err}"));
    assert!(
        output.status.success(),
        "{UUID} sha256sum exit {:?}",
        output.status.code()
    );
    let text = String::from_utf8(output.stdout).unwrap_or_else(|_| panic!("{UUID} sha256sum utf8"));
    let hex = text
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("{UUID} sha256sum parse"));
    assert_eq!(hex.len(), 64, "{UUID} sha256sum length");
    let _ = fs::remove_file(&path);
    hex.to_owned()
}

fn read_full(input: &mut impl Read, buf: &mut [u8]) -> Result<bool, String> {
    let mut filled = 0;
    while filled < buf.len() {
        match input.read(&mut buf[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => return Err(format!("{UUID} short frame")),
            Ok(read) => filled += read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(format!("{UUID} frame read {err}")),
        }
    }
    Ok(true)
}

fn read_frames(mut input: impl Read) -> Result<Vec<Frame>, String> {
    let mut frames = Vec::new();
    loop {
        let mut head = [0u8; 8];
        if !read_full(&mut input, &mut head)? {
            return Ok(frames);
        }
        let tag = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        if len == 0 || len > MAX_PAYLOAD {
            return Err(format!("{UUID} payload length {len}"));
        }
        if frames.len() == MAX_FRAMES {
            return Err(format!("{UUID} too many frames"));
        }
        let mut payload = vec![0u8; len];
        if !read_full(&mut input, &mut payload)? {
            return Err(format!("{UUID} short payload"));
        }
        frames.push(Frame { tag, payload });
    }
}

fn read_capped(mut input: impl Read, cap: usize) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        let read = match input.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(format!("{UUID} stderr {err}")),
        };
        if body.len() + read > cap {
            return Err(format!("{UUID} stderr exceeded {cap}"));
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Ok(body)
}

fn wait_for(child: &mut Child, limit: Duration) -> ExitStatus {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status,
            Ok(None) if start.elapsed() > limit => {
                let _ = child.kill();
                return child
                    .wait()
                    .unwrap_or_else(|err| panic!("{UUID} reap after kill {err}"));
            }
            Ok(None) => thread::sleep(Duration::from_millis(15)),
            Err(err) => panic!("{UUID} wait {err}"),
        }
    }
}

struct Spawned {
    reap: Reap,
    stdin: ChildStdin,
    stdout: thread::JoinHandle<Result<Vec<Frame>, String>>,
    stderr: thread::JoinHandle<Result<Vec<u8>, String>>,
}

fn spawn_worker() -> Spawned {
    let bin = env!("CARGO_BIN_EXE_oo-file-parser-worker");
    let mut command = Command::new(bin);
    command
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    assert!(
        command.get_envs().next().is_none(),
        "{UUID} child environment must stay empty"
    );
    let mut child = command
        .spawn()
        .unwrap_or_else(|err| panic!("{UUID} spawn {err}"));
    let stdin = child.stdin.take().unwrap_or_else(|| panic!("{UUID} stdin"));
    let stdout = child
        .stdout
        .take()
        .unwrap_or_else(|| panic!("{UUID} stdout"));
    let stderr = child
        .stderr
        .take()
        .unwrap_or_else(|| panic!("{UUID} stderr"));
    Spawned {
        reap: Reap { child: Some(child) },
        stdin,
        stdout: thread::spawn(move || read_frames(stdout)),
        stderr: thread::spawn(move || read_capped(stderr, 1024)),
    }
}

fn finish(
    mut reap: Reap,
    stdout_thread: thread::JoinHandle<Result<Vec<Frame>, String>>,
    stderr_thread: thread::JoinHandle<Result<Vec<u8>, String>>,
    limit: Duration,
) -> Outcome {
    let status = wait_for(
        reap.child
            .as_mut()
            .unwrap_or_else(|| panic!("{UUID} child")),
        limit,
    );
    let frames = stdout_thread
        .join()
        .unwrap_or_else(|_| panic!("{UUID} stdout thread"))
        .unwrap_or_else(|err| panic!("{err}"));
    let stderr = stderr_thread
        .join()
        .unwrap_or_else(|_| panic!("{UUID} stderr thread"))
        .unwrap_or_else(|err| panic!("{err}"));
    reap.child.take();
    Outcome {
        code: status.code(),
        stderr,
        frames,
    }
}

fn run_worker(request: &[u8]) -> Outcome {
    let Spawned {
        reap,
        mut stdin,
        stdout,
        stderr,
    } = spawn_worker();
    let _ = stdin.write_all(request);
    drop(stdin);
    finish(reap, stdout, stderr, Duration::from_secs(20))
}

fn assert_closed(outcome: &Outcome, token: &str, exit: i32) {
    assert_eq!(
        outcome.code,
        Some(exit),
        "{UUID} exit for {token} was {:?}",
        outcome.code
    );
    let text = String::from_utf8(outcome.stderr.clone())
        .unwrap_or_else(|_| panic!("{UUID} stderr utf8 for {token}"));
    assert_eq!(
        text,
        format!("oo-file-parser-worker: {token}\n"),
        "{UUID} stderr for {token}"
    );
    assert!(
        outcome.frames.iter().all(|frame| frame.tag != TAG_COMPLETE),
        "{UUID} {token} wrote a completion frame"
    );
}

fn text_lines(payload: &[u8]) -> Vec<String> {
    let text = std::str::from_utf8(payload).unwrap_or_else(|_| panic!("{UUID} frame utf8"));
    text.lines().map(str::to_owned).collect()
}

fn one_value(lines: &[String], key: &str) -> String {
    let prefix = format!("{key}=");
    let mut hits = lines.iter().filter(|line| line.starts_with(&prefix));
    let found = hits
        .next()
        .unwrap_or_else(|| panic!("{UUID} missing {key}"))
        .clone();
    assert!(hits.next().is_none(), "{UUID} duplicate {key}");
    found[prefix.len()..].to_owned()
}

fn complete_lines(outcome: &Outcome) -> Vec<String> {
    let completes: Vec<_> = outcome
        .frames
        .iter()
        .filter(|frame| frame.tag == TAG_COMPLETE)
        .collect();
    assert_eq!(completes.len(), 1, "{UUID} completion count");
    assert_eq!(
        outcome.frames.last().map(|frame| frame.tag),
        Some(TAG_COMPLETE),
        "{UUID} completion was not the last frame"
    );
    text_lines(&completes[0].payload)
}

fn pairs(outcome: &Outcome) -> Vec<(&Frame, &Frame)> {
    assert!(
        outcome
            .frames
            .last()
            .is_some_and(|frame| frame.tag == TAG_COMPLETE),
        "{UUID} missing completion"
    );
    let body = &outcome.frames[..outcome.frames.len() - 1];
    assert_eq!(body.len() % 2, 0, "{UUID} ipc and lineage must pair");
    body.chunks(2)
        .map(|chunk| {
            assert_eq!(chunk[0].tag, TAG_IPC, "{UUID} ipc tag");
            assert_eq!(chunk[1].tag, TAG_LINEAGE, "{UUID} lineage tag");
            (&chunk[0], &chunk[1])
        })
        .collect()
}

fn decode_one(payload: &[u8]) -> RecordBatch {
    let cursor = io::Cursor::new(payload);
    let mut reader =
        StreamReader::try_new(cursor, None).unwrap_or_else(|err| panic!("{UUID} ipc schema {err}"));
    let batch = reader
        .next()
        .unwrap_or_else(|| panic!("{UUID} ipc had no batch"))
        .unwrap_or_else(|err| panic!("{UUID} ipc batch {err}"));
    assert!(
        reader.next().is_none(),
        "{UUID} ipc stream had a second batch"
    );
    batch
}

fn utf8_col(batch: &RecordBatch, index: usize) -> Vec<Option<String>> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap_or_else(|| panic!("{UUID} utf8 column {index}"));
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

fn bool_col(batch: &RecordBatch, index: usize) -> Vec<Option<bool>> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<BooleanArray>()
        .unwrap_or_else(|| panic!("{UUID} bool column {index}"));
    (0..array.len())
        .map(|row| (!array.is_null(row)).then(|| array.value(row)))
        .collect()
}

fn i64_col(batch: &RecordBatch, index: usize) -> Vec<Option<i64>> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap_or_else(|| panic!("{UUID} int64 column {index}"));
    (0..array.len())
        .map(|row| (!array.is_null(row)).then(|| array.value(row)))
        .collect()
}

fn decimal_col(batch: &RecordBatch, index: usize) -> Vec<Option<i128>> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<Decimal128Array>()
        .unwrap_or_else(|| panic!("{UUID} decimal column {index}"));
    (0..array.len())
        .map(|row| (!array.is_null(row)).then(|| array.value(row)))
        .collect()
}

struct FieldMark {
    name: String,
    line: u64,
    offset: u64,
    presence: String,
}

fn field_marks(lines: &[String]) -> Vec<FieldMark> {
    lines
        .iter()
        .filter(|line| line.starts_with("field="))
        .map(|line| {
            let mut parts = line["field=".len()..].split('\t');
            let name = parts
                .next()
                .unwrap_or_else(|| panic!("{UUID} field name"))
                .to_owned();
            let line_no = parts
                .next()
                .unwrap_or_else(|| panic!("{UUID} field line"))
                .parse()
                .unwrap_or_else(|_| panic!("{UUID} field line number"));
            let offset = parts
                .next()
                .unwrap_or_else(|| panic!("{UUID} field offset"))
                .parse()
                .unwrap_or_else(|_| panic!("{UUID} field offset number"));
            let presence = parts
                .next()
                .unwrap_or_else(|| panic!("{UUID} field presence"))
                .to_owned();
            assert!(parts.next().is_none(), "{UUID} extra field column");
            FieldMark {
                name,
                line: line_no,
                offset,
                presence,
            }
        })
        .collect()
}

fn row_marks(lines: &[String]) -> Vec<(u64, u64, u64)> {
    lines
        .iter()
        .filter(|line| line.starts_with("row="))
        .map(|line| {
            let mut parts = line["row=".len()..].split('\t');
            let index = parts.next().unwrap().parse().unwrap();
            let line_no = parts.next().unwrap().parse().unwrap();
            let offset = parts.next().unwrap().parse().unwrap();
            (index, line_no, offset)
        })
        .collect()
}

fn find_field<'a>(marks: &'a [FieldMark], name: &str, nth: usize) -> &'a FieldMark {
    marks
        .iter()
        .filter(|mark| mark.name == name)
        .nth(nth)
        .unwrap_or_else(|| panic!("{UUID} missing field {name} #{nth}"))
}

fn request(path: &Path, sha: &str, profile: &str, budget: &str) -> String {
    format!(
        r#"{{"protocol":"{PROTOCOL}","input":{{"class":"local_file","admission":"caller_admitted_immutable","path":"{}"}},"object_sha256":"{sha}","profile":{profile},"budget":{budget}}}"#,
        path.display()
    )
}

fn loose_delimited_budget() -> String {
    r#"{"batch_rows":32,"batch_bytes":1048576,"max_record_bytes":65536,"max_field_bytes":8192,"max_total_bytes":1048576,"max_rows":1000,"max_columns":16,"max_fields":64,"deadline_ms":60000}"#.to_owned()
}

fn jsonl_budget(batch_rows: u32) -> String {
    format!(
        r#"{{"batch_rows":{batch_rows},"batch_bytes":1048576,"max_record_bytes":65536,"max_total_bytes":1048576,"max_rows":1000,"max_columns":16,"max_fields":64,"deadline_ms":60000}}"#
    )
}

fn write_input(name: &str, bytes: &[u8]) -> (PathBuf, String) {
    let path = scratch(name);
    fs::write(&path, bytes).unwrap_or_else(|err| panic!("{UUID} write input {err}"));
    let sha = sha256_bytes(bytes);
    (path, sha)
}

fn assert_receipt(lines: &[String], expect: &[(&str, &str)]) {
    assert_eq!(
        lines[0], "oo-file-parser-complete/v1",
        "{UUID} receipt prefix"
    );
    for (key, value) in expect {
        assert_eq!(one_value(lines, key), *value, "{UUID} receipt {key}");
    }
    let text = lines.join("\n");
    assert!(
        !text.contains("PgRunComplete"),
        "{UUID} receipt named PgRunComplete"
    );
    assert!(
        !text.contains("oo.source.pgsql"),
        "{UUID} receipt named the postgres protocol"
    );
    assert!(
        !text.contains("sandbox"),
        "{UUID} receipt claimed a sandbox"
    );
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

fn enterprise_profile() -> String {
    r#"{"format":"delimited","id":"enterprise","dialect":"csv","bom":"refuse","header":"exact","null":{"policy":"token","text":"\\N"},"columns":[{"name":"id","type":"int64","nullable":false},{"name":"name","type":"utf8","nullable":true},{"name":"ok","type":"bool","nullable":false},{"name":"amount","type":"decimal","nullable":false,"precision":10,"scale":2},{"name":"note","type":"utf8","nullable":true}]}"#.to_owned()
}

fn enterprise_profile_preimage() -> String {
    "oo-delimited-profile/v1\n\
id=enterprise\n\
dialect=csv\n\
bom=refuse\n\
header=exact\n\
null=token\n\
null_token=\\N\n\
quote=double\n\
record=lf-or-crlf\n\
escape=double-quote\n\
column=id\tint64\t0\t-\t-\n\
column=name\tutf8\t1\t-\t-\n\
column=ok\tbool\t0\t-\t-\n\
column=amount\tdecimal\t0\t10\t2\n\
column=note\tutf8\t1\t-\t-\n"
        .to_owned()
}

fn enterprise_schema_preimage() -> String {
    "oo-file-parser-schema/v1\n\
field=id\tint64\t0\n\
meta=id\too.delimited.logical\tint64\n\
meta=id\too.delimited.protocol\too.source.delimited/v1\n\
field=name\tutf8\t1\n\
meta=name\too.delimited.logical\tutf8\n\
meta=name\too.delimited.protocol\too.source.delimited/v1\n\
field=ok\tbool\t0\n\
meta=ok\too.delimited.logical\tbool\n\
meta=ok\too.delimited.protocol\too.source.delimited/v1\n\
field=amount\tdecimal128:10:2\t0\n\
meta=amount\too.delimited.logical\tdecimal\n\
meta=amount\too.delimited.precision\t10\n\
meta=amount\too.delimited.protocol\too.source.delimited/v1\n\
meta=amount\too.delimited.scale\t2\n\
field=note\tutf8\t1\n\
meta=note\too.delimited.logical\tutf8\n\
meta=note\too.delimited.protocol\too.source.delimited/v1\n"
        .to_owned()
}

fn config_preimage(prefix: &str, profile: &str, sha: &str) -> String {
    format!("{prefix}{profile}expected_sha256={sha}\n")
}

#[test]
fn sha256sum_matches_the_empty_vector() {
    assert_eq!(sha256_bytes(b""), EMPTY_SHA, "{UUID} empty sha256sum");
}

#[test]
fn csv_round_trip_decodes_exact_values_and_lineage() {
    let (bytes, rows, names, notes) = enterprise_csv();
    let (path, sha) = write_input("enterprise", &bytes);
    let outcome = run_worker(
        request(
            &path,
            &sha,
            &enterprise_profile(),
            &loose_delimited_budget(),
        )
        .as_bytes(),
    );
    assert_eq!(
        outcome.code,
        Some(0),
        "{UUID} csv exit {:?} stderr {:?}",
        outcome.code,
        outcome.stderr
    );
    assert!(
        outcome.stderr.is_empty(),
        "{UUID} csv stderr {:?}",
        outcome.stderr
    );
    let paired = pairs(&outcome);
    assert_eq!(paired.len(), 1, "{UUID} csv batches");
    let batch = decode_one(&paired[0].0.payload);
    assert_eq!(
        i64_col(&batch, 0),
        vec![Some(i64::MIN), Some(i64::MAX), Some(0)],
        "{UUID} int64"
    );
    assert_eq!(
        utf8_col(&batch, 1),
        vec![
            Some("中文".to_owned()),
            Some("猫".to_owned()),
            Some(String::new())
        ],
        "{UUID} utf8"
    );
    assert_eq!(
        bool_col(&batch, 2),
        vec![Some(true), Some(false), Some(true)],
        "{UUID} bool"
    );
    assert_eq!(
        decimal_col(&batch, 3),
        vec![Some(120), Some(120), Some(0)],
        "{UUID} decimal coefficients"
    );
    assert_eq!(
        utf8_col(&batch, 4)[0],
        Some(String::new()),
        "{UUID} quoted empty"
    );
    assert_eq!(utf8_col(&batch, 4)[1], None, "{UUID} null token");
    assert_eq!(
        utf8_col(&batch, 4)[2].as_deref(),
        Some("line\ntwo"),
        "{UUID} quoted newline"
    );
    let schema = batch.schema();
    let amount = schema.field(3);
    assert_eq!(
        amount
            .metadata()
            .get("oo.delimited.logical")
            .map(String::as_str),
        Some("decimal"),
        "{UUID} decimal logical metadata"
    );
    assert_eq!(
        amount
            .metadata()
            .get("oo.delimited.scale")
            .map(String::as_str),
        Some("2"),
        "{UUID} decimal scale metadata"
    );
    let lineage = text_lines(&paired[0].1.payload);
    assert_eq!(
        lineage[0], "oo-file-parser-lineage/v1",
        "{UUID} lineage prefix"
    );
    let profile_sha = sha256_bytes(enterprise_profile_preimage().as_bytes());
    let config_sha = sha256_bytes(
        config_preimage(
            "oo-delimited-config-identity/v1\n",
            &enterprise_profile_preimage(),
            &sha,
        )
        .as_bytes(),
    );
    let schema_sha = sha256_bytes(enterprise_schema_preimage().as_bytes());
    assert_eq!(
        one_value(&lineage, "batch_index"),
        "0",
        "{UUID} batch index"
    );
    assert_eq!(
        one_value(&lineage, "reader_protocol"),
        "oo.source.delimited/v1"
    );
    assert_eq!(
        one_value(&lineage, "profile_digest"),
        profile_sha,
        "{UUID} profile"
    );
    assert_eq!(
        one_value(&lineage, "config_identity"),
        config_sha,
        "{UUID} config"
    );
    assert_eq!(
        one_value(&lineage, "schema_identity"),
        schema_sha,
        "{UUID} schema"
    );
    let marked = row_marks(&lineage);
    assert_eq!(
        marked,
        vec![(0, 2, rows[0]), (1, 3, rows[1]), (2, 4, rows[2])],
        "{UUID} rows"
    );
    let fields = field_marks(&lineage);
    assert_eq!(
        find_field(&fields, "name", 0).offset,
        names[0],
        "{UUID} name offset"
    );
    assert_eq!(find_field(&fields, "name", 0).presence, "value");
    assert_eq!(find_field(&fields, "name", 2).offset, names[2]);
    assert_eq!(
        find_field(&fields, "name", 2).presence,
        "empty",
        "{UUID} unquoted empty"
    );
    assert_eq!(find_field(&fields, "note", 0).offset, notes[0]);
    assert_eq!(
        find_field(&fields, "note", 0).presence,
        "empty",
        "{UUID} quoted empty presence"
    );
    assert_eq!(find_field(&fields, "note", 1).offset, notes[1]);
    assert_eq!(
        find_field(&fields, "note", 1).presence,
        "null",
        "{UUID} token null"
    );
    assert_eq!(find_field(&fields, "note", 2).offset, notes[2]);
    assert_eq!(
        find_field(&fields, "note", 2).line,
        4,
        "{UUID} quoted newline line"
    );
    assert_eq!(find_field(&fields, "note", 2).presence, "value");
    let receipt = complete_lines(&outcome);
    assert_receipt(
        &receipt,
        &[
            ("protocol", PROTOCOL),
            ("reader_protocol", "oo.source.delimited/v1"),
            ("reader_profile_version", "oo.source.delimited.profile/v1"),
            ("reader_lineage_version", "oo.source.delimited.lineage/v1"),
            ("reader_receipt_version", "oo.source.delimited.receipt/v1"),
            ("profile_id", "enterprise"),
            ("profile_digest", profile_sha.as_str()),
            ("config_identity", config_sha.as_str()),
            ("schema_identity", schema_sha.as_str()),
            ("object_sha256", sha.as_str()),
            ("rows", "3"),
            ("columns", "5"),
            ("batches", "1"),
            ("ipc_frames", "1"),
            ("lineage_frames", "1"),
            ("admits_import", "false"),
            ("published", "false"),
            ("arrow_ipc", "true"),
            ("executes_native", "false"),
            ("kernel_network", "unsupported"),
            ("kernel_filesystem", "unsupported"),
            ("kernel_memory", "unsupported"),
            ("parent_kill", "caller"),
            ("parent_reap", "caller"),
            ("launcher", "not_this_process"),
            ("pg_run_complete", "false"),
            ("resumable", "false"),
        ],
    );
}

#[test]
fn tsv_keeps_a_quoted_tab_and_an_unquoted_comma() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice("熊".as_bytes());
    bytes.extend_from_slice(b"\t\"a\tb\"\t7\n");
    let second = bytes.len() as u64;
    bytes.extend_from_slice("猫,鼠".as_bytes());
    bytes.extend_from_slice(b"\t");
    bytes.extend_from_slice("上海".as_bytes());
    bytes.extend_from_slice(b"\t-0");
    let profile = r#"{"format":"delimited","id":"tsv-sample","dialect":"tsv","bom":"refuse","header":"absent","null":{"policy":"empty"},"columns":[{"name":"name","type":"utf8","nullable":false},{"name":"raw","type":"utf8","nullable":false},{"name":"n","type":"int64","nullable":false}]}"#;
    let (path, sha) = write_input("tsv", &bytes);
    let outcome = run_worker(request(&path, &sha, profile, &loose_delimited_budget()).as_bytes());
    assert_eq!(
        outcome.code,
        Some(0),
        "{UUID} tsv exit {:?}",
        outcome.stderr
    );
    let paired = pairs(&outcome);
    let batch = decode_one(&paired[0].0.payload);
    assert_eq!(
        utf8_col(&batch, 0),
        vec![Some("熊".to_owned()), Some("猫,鼠".to_owned())]
    );
    assert_eq!(
        utf8_col(&batch, 1),
        vec![Some("a\tb".to_owned()), Some("上海".to_owned())]
    );
    assert_eq!(i64_col(&batch, 2), vec![Some(7), Some(0)], "{UUID} tsv -0");
    let lineage = text_lines(&paired[0].1.payload);
    let marked = row_marks(&lineage);
    assert_eq!(marked[1], (1, 2, second), "{UUID} tsv second row");
    assert_eq!(one_value(&complete_lines(&outcome), "object_sha256"), sha);
    assert_eq!(one_value(&complete_lines(&outcome), "rows"), "2");
}

#[test]
fn jsonl_splits_batches_and_distinguishes_null_from_missing() {
    let mut bytes = Vec::new();
    let mut starts = Vec::new();
    for record in [
        r#"{"name":"中文","ok":true,"n":-9223372036854775808}"#,
        r#"{"name":null,"ok":false,"n":0}"#,
        r#"{"ok":true,"n":9223372036854775807}"#,
    ] {
        starts.push(bytes.len() as u64);
        bytes.extend_from_slice(record.as_bytes());
        bytes.push(b'\n');
    }
    let profile = r#"{"format":"jsonl","id":"people","columns":[{"name":"name","type":"utf8","nullable":true},{"name":"ok","type":"bool","nullable":false},{"name":"n","type":"int64","nullable":false}]}"#;
    let (path, sha) = write_input("jsonl", &bytes);
    let outcome = run_worker(request(&path, &sha, profile, &jsonl_budget(1)).as_bytes());
    assert_eq!(
        outcome.code,
        Some(0),
        "{UUID} jsonl exit {:?}",
        outcome.stderr
    );
    let paired = pairs(&outcome);
    assert_eq!(paired.len(), 3, "{UUID} jsonl batches");
    let profile_preimage = "oo-jsonl-profile/v1\nid=people\ncolumn=name\tutf8\t1\ncolumn=ok\tbool\t0\ncolumn=n\tint64\t0\n";
    let profile_sha = sha256_bytes(profile_preimage.as_bytes());
    let expected_n = [i64::MIN, 0, i64::MAX];
    let expected_bool = [true, false, true];
    let expected_presence = ["value", "null", "missing"];
    for (index, (ipc, lineage_frame)) in paired.iter().enumerate() {
        let batch = decode_one(&ipc.payload);
        assert_eq!(
            i64_col(&batch, 2),
            vec![Some(expected_n[index])],
            "{UUID} jsonl int"
        );
        assert_eq!(bool_col(&batch, 1), vec![Some(expected_bool[index])]);
        let lineage = text_lines(&lineage_frame.payload);
        assert_eq!(one_value(&lineage, "batch_index"), index.to_string());
        assert_eq!(one_value(&lineage, "reader_protocol"), "oo.source.jsonl/v1");
        assert_eq!(
            one_value(&lineage, "profile_digest"),
            profile_sha,
            "{UUID} jsonl profile"
        );
        let marked = row_marks(&lineage);
        assert_eq!(
            marked,
            vec![(index as u64, index as u64 + 1, starts[index])]
        );
        let fields = field_marks(&lineage);
        assert_eq!(
            find_field(&fields, "name", 0).presence,
            expected_presence[index]
        );
    }
    assert_eq!(
        utf8_col(&decode_one(&paired[0].0.payload), 0),
        vec![Some("中文".to_owned())]
    );
    assert_eq!(utf8_col(&decode_one(&paired[1].0.payload), 0), vec![None]);
    assert_eq!(utf8_col(&decode_one(&paired[2].0.payload), 0), vec![None]);
    let name_fields = field_marks(&text_lines(&paired[0].1.payload));
    let name_key = bytes
        .windows(6)
        .position(|window| window == b"\"name\"")
        .unwrap_or_else(|| panic!("{UUID} name key")) as u64;
    assert_eq!(
        find_field(&name_fields, "name", 0).offset,
        name_key,
        "{UUID} jsonl key offset"
    );
    let missing = field_marks(&text_lines(&paired[2].1.payload));
    assert_eq!(
        find_field(&missing, "name", 0).offset,
        starts[2],
        "{UUID} missing offset"
    );
    let receipt = complete_lines(&outcome);
    assert_eq!(one_value(&receipt, "rows"), "3");
    assert_eq!(one_value(&receipt, "batches"), "3");
    assert_eq!(one_value(&receipt, "ipc_frames"), "3");
    assert_eq!(one_value(&receipt, "object_sha256"), sha);
    assert_eq!(one_value(&receipt, "reader_protocol"), "oo.source.jsonl/v1");
    assert_eq!(one_value(&receipt, "admits_import"), "false");
    assert_eq!(one_value(&receipt, "published"), "false");
    assert_eq!(one_value(&receipt, "arrow_ipc"), "true");
    assert_eq!(one_value(&receipt, "pg_run_complete"), "false");
}

#[test]
fn bom_strip_shifts_the_first_record_and_keeps_the_bom_in_the_hash() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    let header = "\"名称\"\n";
    bytes.extend_from_slice(header.as_bytes());
    let data_at = bytes.len() as u64;
    bytes.extend_from_slice("熊\n".as_bytes());
    let profile = r#"{"format":"delimited","id":"bom","dialect":"csv","bom":"strip","header":"exact","null":{"policy":"empty"},"columns":[{"name":"名称","type":"utf8","nullable":false}]}"#;
    let (path, sha) = write_input("bom", &bytes);
    let outcome = run_worker(request(&path, &sha, profile, &loose_delimited_budget()).as_bytes());
    assert_eq!(
        outcome.code,
        Some(0),
        "{UUID} bom exit {:?}",
        outcome.stderr
    );
    let paired = pairs(&outcome);
    assert_eq!(
        utf8_col(&decode_one(&paired[0].0.payload), 0),
        vec![Some("熊".to_owned())]
    );
    let marked = row_marks(&text_lines(&paired[0].1.payload));
    assert_eq!(marked, vec![(0, 2, data_at)], "{UUID} bom offset");
    assert_eq!(data_at, 3 + header.len() as u64, "{UUID} bom arithmetic");
    assert_eq!(one_value(&complete_lines(&outcome), "object_sha256"), sha);
}

#[test]
fn empty_files_follow_the_header_policy() {
    let absent = r#"{"format":"delimited","id":"blank","dialect":"csv","bom":"refuse","header":"absent","null":{"policy":"empty"},"columns":[{"name":"name","type":"utf8","nullable":true}]}"#;
    let exact = r#"{"format":"delimited","id":"blank","dialect":"csv","bom":"refuse","header":"exact","null":{"policy":"empty"},"columns":[{"name":"name","type":"utf8","nullable":true}]}"#;
    let (empty_path, empty_sha) = write_input("empty", b"");
    assert_eq!(empty_sha, EMPTY_SHA, "{UUID} empty file oracle");
    let absent_out =
        run_worker(request(&empty_path, &empty_sha, absent, &loose_delimited_budget()).as_bytes());
    assert_eq!(
        absent_out.code,
        Some(0),
        "{UUID} empty absent {:?}",
        absent_out.stderr
    );
    assert!(
        pairs(&absent_out).is_empty(),
        "{UUID} empty absent emitted a batch"
    );
    let absent_receipt = complete_lines(&absent_out);
    assert_eq!(one_value(&absent_receipt, "rows"), "0");
    assert_eq!(one_value(&absent_receipt, "batches"), "0");
    assert_eq!(one_value(&absent_receipt, "ipc_frames"), "0");
    assert_eq!(one_value(&absent_receipt, "arrow_ipc"), "false");
    assert_eq!(one_value(&absent_receipt, "object_sha256"), EMPTY_SHA);
    assert_eq!(one_value(&absent_receipt, "admits_import"), "false");

    let exact_out =
        run_worker(request(&empty_path, &empty_sha, exact, &loose_delimited_budget()).as_bytes());
    assert_closed(&exact_out, "header_required", 4);

    let (header_path, header_sha) = write_input("header-only", b"name\n");
    let header_out =
        run_worker(request(&header_path, &header_sha, exact, &loose_delimited_budget()).as_bytes());
    assert_eq!(
        header_out.code,
        Some(0),
        "{UUID} header only {:?}",
        header_out.stderr
    );
    assert!(
        pairs(&header_out).is_empty(),
        "{UUID} header only emitted data"
    );
    let header_receipt = complete_lines(&header_out);
    assert_eq!(one_value(&header_receipt, "rows"), "0");
    assert_eq!(one_value(&header_receipt, "object_sha256"), header_sha);
    assert_eq!(one_value(&header_receipt, "arrow_ipc"), "false");

    let jsonl = r#"{"format":"jsonl","id":"blank","columns":[{"name":"n","type":"int64","nullable":false}]}"#;
    let jsonl_out =
        run_worker(request(&empty_path, &empty_sha, jsonl, &jsonl_budget(8)).as_bytes());
    assert_eq!(
        jsonl_out.code,
        Some(0),
        "{UUID} empty jsonl {:?}",
        jsonl_out.stderr
    );
    assert_eq!(one_value(&complete_lines(&jsonl_out), "rows"), "0");
    assert_eq!(
        one_value(&complete_lines(&jsonl_out), "reader_protocol"),
        "oo.source.jsonl/v1"
    );
    assert_eq!(one_value(&complete_lines(&jsonl_out), "arrow_ipc"), "false");
}

#[test]
fn rejected_requests_exit_nonzero_without_completion() {
    let (path, sha) = write_input("tiny", b"a\n");
    let profile = r#"{"format":"delimited","id":"one","dialect":"csv","bom":"refuse","header":"absent","null":{"policy":"empty"},"columns":[{"name":"name","type":"utf8","nullable":true}]}"#;
    let cases = [
        ("{", "request_syntax", 2),
        (
            r#"{"protocol":"oo.source.file-parser-worker/v1","protocol":"oo.source.file-parser-worker/v1"}"#,
            "duplicate_key",
            2,
        ),
        (
            &format!(
                r#"{{"protocol":"{PROTOCOL}","sniff":false,"input":{{"class":"local_file","admission":"caller_admitted_immutable","path":"{}"}},"object_sha256":"{sha}","profile":{profile},"budget":{}}}"#,
                path.display(),
                loose_delimited_budget()
            ),
            "unknown_key",
            2,
        ),
        (r#"{"password":"blocked"}"#, "credentials", 2),
        (
            r#"{"protocol":"oo.source.pgsql/v1"}"#,
            "unknown_protocol",
            2,
        ),
        (
            &request(
                Path::new("relative.csv"),
                &sha,
                profile,
                &loose_delimited_budget(),
            ),
            "path_form",
            2,
        ),
        (
            &request(
                Path::new("/tmp/a/../b"),
                &sha,
                profile,
                &loose_delimited_budget(),
            ),
            "path_form",
            2,
        ),
    ];
    for (body, token, exit) in cases {
        let outcome = run_worker(body.as_bytes());
        assert_closed(&outcome, token, exit);
        let stderr = String::from_utf8(outcome.stderr).unwrap();
        assert!(!stderr.contains("blocked"), "{UUID} credential leaked");
        assert!(
            !stderr.contains(&path.display().to_string()),
            "{UUID} path leaked"
        );
        assert!(!stderr.contains("PgRunComplete"), "{UUID} pg token");
    }

    let missing = scratch("missing");
    let outcome =
        run_worker(request(&missing, &sha, profile, &loose_delimited_budget()).as_bytes());
    assert_closed(&outcome, "io", 7);
    let stderr = String::from_utf8(outcome.stderr).unwrap();
    assert!(
        !stderr.contains(&missing.display().to_string()),
        "{UUID} missing path leaked"
    );

    let dir = scratch("dir");
    fs::create_dir(&dir).unwrap();
    assert_closed(
        &run_worker(request(&dir, &sha, profile, &loose_delimited_budget()).as_bytes()),
        "not_regular",
        2,
    );
    let target = scratch("target");
    fs::write(&target, b"a\n").unwrap();
    let link = scratch("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert_closed(
        &run_worker(request(&link, &sha, profile, &loose_delimited_budget()).as_bytes()),
        "not_regular",
        2,
    );
    let fifo = scratch("fifo");
    let made = Command::new("/usr/bin/mkfifo")
        .arg(&fifo)
        .status()
        .unwrap_or_else(|err| panic!("{UUID} mkfifo {err}"));
    assert!(made.success(), "{UUID} mkfifo status");
    assert_closed(
        &run_worker(request(&fifo, &sha, profile, &loose_delimited_budget()).as_bytes()),
        "not_regular",
        2,
    );

    let empty_profile = r#"{"format":"jsonl","id":"none","columns":[]}"#;
    assert_closed(
        &run_worker(request(&path, &sha, empty_profile, &jsonl_budget(1)).as_bytes()),
        "config",
        2,
    );

    let mut huge = vec![b' '; 1_048_577];
    huge[0] = b'{';
    assert_closed(&run_worker(&huge), "request_too_large", 2);
}

#[test]
fn hash_mismatch_can_emit_a_batch_and_still_omits_completion() {
    let bytes = "熊\n".as_bytes();
    let (path, _sha) = write_input("hash", bytes);
    let wrong = "0".repeat(64);
    let profile = r#"{"format":"delimited","id":"one","dialect":"csv","bom":"refuse","header":"absent","null":{"policy":"empty"},"columns":[{"name":"name","type":"utf8","nullable":false}]}"#;
    let outcome = run_worker(request(&path, &wrong, profile, &loose_delimited_budget()).as_bytes());
    assert_closed(&outcome, "hash_mismatch", 6);
    assert!(
        outcome.frames.iter().any(|frame| frame.tag == TAG_IPC),
        "{UUID} hash mismatch dropped the delivered batch"
    );
    let ipc = outcome
        .frames
        .iter()
        .find(|frame| frame.tag == TAG_IPC)
        .unwrap();
    assert_eq!(
        utf8_col(&decode_one(&ipc.payload), 0),
        vec![Some("熊".to_owned())]
    );
    let stderr = String::from_utf8(outcome.stderr.clone()).unwrap();
    assert!(!stderr.contains('熊'), "{UUID} stderr copied the cell");
}

#[test]
fn truncated_jsonl_and_record_budget_omit_completion() {
    let truncated = br#"{"n":1"#;
    let (path, sha) = write_input("cut", truncated);
    let profile =
        r#"{"format":"jsonl","id":"cut","columns":[{"name":"n","type":"int64","nullable":false}]}"#;
    let outcome = run_worker(request(&path, &sha, profile, &jsonl_budget(4)).as_bytes());
    assert_closed(&outcome, "truncated", 4);
    assert!(outcome.frames.is_empty(), "{UUID} truncated wrote a frame");

    let (wide, wide_sha) = write_input("wide", b"abcd\n");
    let budget = r#"{"batch_rows":4,"batch_bytes":1024,"max_record_bytes":3,"max_field_bytes":3,"max_total_bytes":64,"max_rows":4,"max_columns":1,"max_fields":1,"deadline_ms":60000}"#;
    let one = r#"{"format":"delimited","id":"wide","dialect":"csv","bom":"refuse","header":"absent","null":{"policy":"empty"},"columns":[{"name":"name","type":"utf8","nullable":false}]}"#;
    let outcome = run_worker(request(&wide, &wide_sha, one, budget).as_bytes());
    assert_closed(&outcome, "budget", 3);
    assert!(outcome.frames.is_empty(), "{UUID} budget wrote a frame");

    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(b"a\n");
    let (bom_path, bom_sha) = write_input("bom-no", &bom);
    let outcome =
        run_worker(request(&bom_path, &bom_sha, one, &loose_delimited_budget()).as_bytes());
    assert_closed(&outcome, "encoding", 5);
    assert!(
        outcome.frames.is_empty(),
        "{UUID} bom refusal wrote a frame"
    );
}

#[test]
fn ipc_ceiling_rejects_before_a_frame_is_written() {
    let width = 64usize;
    let rows = 128usize;
    let mut row = Vec::new();
    for index in 0..width {
        if index > 0 {
            row.push(b',');
        }
        row.push(b'0');
    }
    row.push(b'\n');
    assert_eq!(row.len(), 128, "{UUID} ceiling row width");
    let mut bytes = Vec::new();
    for _ in 0..rows {
        bytes.extend_from_slice(&row);
    }
    let mut columns = String::from("[");
    for index in 0..width {
        if index > 0 {
            columns.push(',');
        }
        columns.push_str(&format!(
            r#"{{"name":"c{index:02}","type":"decimal","nullable":false,"precision":2,"scale":0}}"#
        ));
    }
    columns.push(']');
    let profile = format!(
        r#"{{"format":"delimited","id":"ceiling","dialect":"csv","bom":"refuse","header":"absent","null":{{"policy":"empty"}},"columns":{columns}}}"#
    );
    let budget = r#"{"batch_rows":128,"batch_bytes":16384,"max_record_bytes":256,"max_field_bytes":8,"max_total_bytes":65536,"max_rows":128,"max_columns":64,"max_fields":64,"deadline_ms":60000}"#;
    let (path, sha) = write_input("ceiling", &bytes);
    let outcome = run_worker(request(&path, &sha, &profile, budget).as_bytes());
    assert_closed(&outcome, "ipc_ceiling", 8);
    assert!(
        outcome.frames.is_empty(),
        "{UUID} ipc ceiling wrote {} frames",
        outcome.frames.len()
    );
}

#[test]
fn parent_kills_and_reaps_a_child_blocked_on_stdin() {
    let Spawned {
        reap,
        stdin,
        stdout,
        stderr,
    } = spawn_worker();
    let pid = reap
        .child
        .as_ref()
        .unwrap_or_else(|| panic!("{UUID} pid"))
        .id();
    let outcome = finish(reap, stdout, stderr, Duration::from_millis(400));
    drop(stdin);
    assert_eq!(
        outcome.code, None,
        "{UUID} kill must be a signal, not an exit code"
    );
    assert!(
        outcome.frames.iter().all(|frame| frame.tag != TAG_COMPLETE),
        "{UUID} killed child completed"
    );
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "{UUID} child {pid} was still alive"
    );
}

#[test]
fn drop_guard_reaps_when_the_test_does_not_wait() {
    let Spawned {
        reap,
        stdin,
        stdout,
        stderr,
    } = spawn_worker();
    let pid = reap
        .child
        .as_ref()
        .unwrap_or_else(|| panic!("{UUID} pid"))
        .id();
    drop(reap);
    let _ = stdout.join();
    let _ = stderr.join();
    drop(stdin);
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "{UUID} drop guard left {pid} alive"
    );
}
