//! Independent parent for the frozen `oo-file-parser-worker`.
//!
//! Session eef831aa-dba4-4af2-a78b-541bc9e0a224. The child environment is
//! empty. SHA-256 is FIPS 180-4 in this file. Profile and schema preimages
//! are the contract text, not a call into the library.

#![forbid(unsafe_code)]

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use arrow_array::{Array, BooleanArray, Decimal128Array, Int64Array, RecordBatch, StringArray};
use arrow_ipc::reader::StreamReader;
use arrow_schema::DataType;

const UUID: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a224";
const PROTOCOL: &str = "oo.source.file-parser-worker/v1";
const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const TAG_IPC: u32 = 1;
const TAG_LINEAGE: u32 = 2;
const TAG_COMPLETE: u32 = 3;
const MAX_PAYLOAD: usize = 2_097_152;
const MAX_FRAMES: usize = 64;

const LOOSE_BUDGET: &str = r#"{"batch_rows":32,"batch_bytes":1048576,"max_record_bytes":65536,"max_field_bytes":8192,"max_total_bytes":1048576,"max_rows":1000,"max_columns":16,"max_fields":64,"deadline_ms":60000}"#;
const JSONL_BUDGET: &str = r#"{"batch_rows":1,"batch_bytes":1048576,"max_record_bytes":65536,"max_total_bytes":1048576,"max_rows":1000,"max_columns":16,"max_fields":64,"deadline_ms":60000}"#;
const CEILING_BUDGET: &str = r#"{"batch_rows":256,"batch_bytes":16384,"max_record_bytes":256,"max_field_bytes":8,"max_total_bytes":65536,"max_rows":256,"max_columns":40,"max_fields":40,"deadline_ms":60000}"#;

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

struct Spawned {
    reap: Reap,
    stdin: ChildStdin,
    stdout: thread::JoinHandle<Result<Vec<Frame>, String>>,
    stderr: thread::JoinHandle<Result<Vec<u8>, String>>,
}

struct FieldMark {
    name: String,
    line: u64,
    offset: u64,
    presence: String,
}

struct Tape {
    bytes: Vec<u8>,
    line: u64,
}

impl Tape {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            line: 1,
        }
    }

    fn mark(&self) -> (u64, u64) {
        (self.line, self.bytes.len() as u64)
    }

    fn push(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.bytes.push(*byte);
            if *byte == b'\n' {
                self.line += 1;
            }
        }
    }
}

fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("oo-file-parser-{UUID}"));
    fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("{UUID} temp dir {err}"));
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    dir.join(format!("{name}-{pid}-{n}"))
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

fn sha256_hex(bytes: &[u8]) -> String {
    let empty = hex(&sha256(b""));
    let abc = hex(&sha256(b"abc"));
    assert_eq!(empty, EMPTY_SHA, "{UUID} empty sha256");
    assert_eq!(abc, ABC_SHA, "{UUID} abc sha256");
    hex(&sha256(bytes))
}

fn save_oracle(name: &str, bytes: &[u8]) {
    let dir = std::env::var("OO_A224_ORACLE_DIR").unwrap_or_else(|_| panic!("{UUID} oracle dir"));
    let path = Path::new(&dir).join(name);
    fs::write(&path, bytes).unwrap_or_else(|err| panic!("{UUID} oracle {name} {err}"));
    let digest = sha256_hex(bytes);
    let side = Path::new(&dir).join(format!("{name}.sha256"));
    fs::write(&side, format!("{digest}\n")).unwrap_or_else(|err| panic!("{UUID} oracle sha {name} {err}"));
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

fn join_thread<T>(handle: thread::JoinHandle<T>, what: &str) -> T {
    match handle.join() {
        Ok(value) => value,
        Err(_) => panic!("{UUID} {what} thread panicked"),
    }
}

fn finish(
    mut reap: Reap,
    stdout_thread: thread::JoinHandle<Result<Vec<Frame>, String>>,
    stderr_thread: thread::JoinHandle<Result<Vec<u8>, String>>,
    limit: Duration,
) -> Outcome {
    let pid = reap
        .child
        .as_ref()
        .unwrap_or_else(|| panic!("{UUID} pid"))
        .id();
    let status = wait_for(
        reap.child
            .as_mut()
            .unwrap_or_else(|| panic!("{UUID} child")),
        limit,
    );
    let frames = join_thread(stdout_thread, "stdout").unwrap_or_else(|err| panic!("{err}"));
    let stderr = join_thread(stderr_thread, "stderr").unwrap_or_else(|err| panic!("{err}"));
    reap.child.take();
    let proc_path = format!("/proc/{pid}");
    assert!(
        !Path::new(&proc_path).exists(),
        "{UUID} child {pid} was still alive"
    );
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
    match stdin.write_all(request) {
        Ok(()) => {}
        Err(err) => panic!("{UUID} stdin write {err}"),
    }
    drop(stdin);
    finish(reap, stdout, stderr, Duration::from_secs(20))
}

fn request(path: &Path, sha: &str, profile: &str, budget: &str) -> String {
    let shown = path.display().to_string();
    assert!(
        !shown.contains(['"', '\\', '\n']),
        "{UUID} path cannot be embedded"
    );
    format!(
        r#"{{"protocol":"{PROTOCOL}","input":{{"class":"local_file","admission":"caller_admitted_immutable","path":"{shown}"}},"object_sha256":"{sha}","profile":{profile},"budget":{budget}}}"#
    )
}

fn write_input(name: &str, bytes: &[u8]) -> (PathBuf, String) {
    let path = scratch(name);
    fs::write(&path, bytes).unwrap_or_else(|err| panic!("{UUID} write input {err}"));
    let sha = sha256_hex(bytes);
    (path, sha)
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
    let last = outcome.frames.last().map(|frame| frame.tag);
    assert_eq!(last, Some(TAG_COMPLETE), "{UUID} completion was not last");
    text_lines(&completes[0].payload)
}

fn pairs(outcome: &Outcome) -> Vec<(&Frame, &Frame)> {
    let last = outcome.frames.last().map(|frame| frame.tag);
    assert_eq!(last, Some(TAG_COMPLETE), "{UUID} missing completion");
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
    assert_eq!(payload.len() % 8, 0, "{UUID} ipc alignment");
    assert!(payload.len() >= 8, "{UUID} ipc short");
    assert_eq!(
        &payload[..4],
        &[0xff, 0xff, 0xff, 0xff],
        "{UUID} ipc continuation"
    );
    let cursor = io::Cursor::new(payload);
    let mut reader =
        StreamReader::try_new(cursor, None).unwrap_or_else(|err| panic!("{UUID} ipc schema {err}"));
    let batch = reader
        .next()
        .unwrap_or_else(|| panic!("{UUID} ipc had no batch"))
        .unwrap_or_else(|err| panic!("{UUID} ipc batch {err}"));
    assert!(reader.next().is_none(), "{UUID} ipc stream had a second batch");
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
            let index = parts
                .next()
                .unwrap_or_else(|| panic!("{UUID} row index"))
                .parse()
                .unwrap_or_else(|_| panic!("{UUID} row index number"));
            let line_no = parts
                .next()
                .unwrap_or_else(|| panic!("{UUID} row line"))
                .parse()
                .unwrap_or_else(|_| panic!("{UUID} row line number"));
            let offset = parts
                .next()
                .unwrap_or_else(|| panic!("{UUID} row offset"))
                .parse()
                .unwrap_or_else(|_| panic!("{UUID} row offset number"));
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

fn assert_limits(lines: &[String]) {
    let text = lines.join("\n");
    for banned in ["PgRunComplete", "oo.source.pgsql", "sandbox", "landlock", "seccomp"] {
        assert!(!text.contains(banned), "{UUID} receipt contained {banned}");
    }
    for (key, value) in [
        ("admits_import", "false"),
        ("published", "false"),
        ("executes_native", "false"),
        ("kernel_network", "unsupported"),
        ("kernel_filesystem", "unsupported"),
        ("kernel_memory", "unsupported"),
        ("parent_kill", "caller"),
        ("parent_reap", "caller"),
        ("launcher", "not_this_process"),
        ("pg_run_complete", "false"),
        ("resumable", "false"),
    ] {
        let got = one_value(lines, key);
        assert_eq!(got, value, "{UUID} receipt {key}");
    }
}

fn assert_ok(outcome: &Outcome, label: &str) {
    let code = outcome.code;
    assert_eq!(code, Some(0), "{UUID} {label} exit {code:?}");
    assert!(
        outcome.stderr.is_empty(),
        "{UUID} {label} stderr was not empty"
    );
}

fn assert_no_cell(payload: &[u8], cell: &str) {
    let text = std::str::from_utf8(payload).unwrap_or_else(|_| panic!("{UUID} lineage utf8"));
    assert!(!text.contains(cell), "{UUID} lineage copied a cell");
}

fn config_preimage(prefix: &str, profile: &str, sha: &str) -> String {
    format!("{prefix}{profile}expected_sha256={sha}\n")
}

fn shelf_profile() -> String {
    r#"{"format":"delimited","id":"a224-shelf","dialect":"csv","bom":"refuse","header":"exact","null":{"policy":"token","text":"~"},"columns":[{"name":"sku","type":"utf8","nullable":true},{"name":"qty","type":"int64","nullable":false},{"name":"price","type":"decimal","nullable":false,"precision":8,"scale":3},{"name":"note","type":"utf8","nullable":true}]}"#.to_owned()
}

fn shelf_profile_preimage() -> String {
    "oo-delimited-profile/v1\n\
id=a224-shelf\n\
dialect=csv\n\
bom=refuse\n\
header=exact\n\
null=token\n\
null_token=~\n\
quote=double\n\
record=lf-or-crlf\n\
escape=double-quote\n\
column=sku\tutf8\t1\t-\t-\n\
column=qty\tint64\t0\t-\t-\n\
column=price\tdecimal\t0\t8\t3\n\
column=note\tutf8\t1\t-\t-\n"
        .to_owned()
}

fn shelf_schema_preimage() -> String {
    "oo-file-parser-schema/v1\n\
field=sku\tutf8\t1\n\
meta=sku\too.delimited.logical\tutf8\n\
meta=sku\too.delimited.protocol\too.source.delimited/v1\n\
field=qty\tint64\t0\n\
meta=qty\too.delimited.logical\tint64\n\
meta=qty\too.delimited.protocol\too.source.delimited/v1\n\
field=price\tdecimal128:8:3\t0\n\
meta=price\too.delimited.logical\tdecimal\n\
meta=price\too.delimited.precision\t8\n\
meta=price\too.delimited.protocol\too.source.delimited/v1\n\
meta=price\too.delimited.scale\t3\n\
field=note\tutf8\t1\n\
meta=note\too.delimited.logical\tutf8\n\
meta=note\too.delimited.protocol\too.source.delimited/v1\n"
        .to_owned()
}

fn notes_profile_preimage() -> String {
    "oo-jsonl-profile/v1\n\
id=a224-notes\n\
column=label\tutf8\t1\n\
column=n\tint64\t0\n\
column=flag\tbool\t0\n"
        .to_owned()
}

fn notes_schema_preimage() -> String {
    "oo-file-parser-schema/v1\n\
field=label\tutf8\t1\n\
meta=label\too.jsonl.logical\tutf8\n\
meta=label\too.jsonl.protocol\too.source.jsonl/v1\n\
field=n\tint64\t0\n\
meta=n\too.jsonl.logical\tint64\n\
meta=n\too.jsonl.protocol\too.source.jsonl/v1\n\
field=flag\tbool\t0\n\
meta=flag\too.jsonl.logical\tbool\n\
meta=flag\too.jsonl.protocol\too.source.jsonl/v1\n"
        .to_owned()
}

struct ShelfMarks {
    rows: [u64; 3],
    sku: [u64; 3],
    note: [u64; 3],
}

fn shelf_bytes() -> (Vec<u8>, ShelfMarks) {
    let mut file = Tape::new();
    file.push(b"sku,qty,price,note\n");
    let (row_line, row0) = file.mark();
    let (sku_line, sku0) = file.mark();
    file.push("甲".as_bytes());
    file.push(b",-9223372036854775808,3.140,");
    let (note_line, note0) = file.mark();
    file.push(b"\"\"\n");
    let (row_line1, row1) = file.mark();
    let (sku_line1, sku1) = file.mark();
    file.push(b",9223372036854775807,3.14,");
    let (note_line1, note1) = file.mark();
    file.push(b"~\n");
    let (row_line2, row2) = file.mark();
    let (sku_line2, sku2) = file.mark();
    file.push(b"\"ab\nc\",0,-0.010,");
    let (note_line2, note2) = file.mark();
    file.push(b"\"x,y\"\n");
    assert_eq!([row_line, row_line1, row_line2], [2u64, 3, 4], "{UUID} shelf rows");
    assert_eq!([sku_line, sku_line1, sku_line2], [2u64, 3, 4], "{UUID} shelf sku lines");
    assert_eq!(
        [note_line, note_line1, note_line2],
        [2u64, 3, 5],
        "{UUID} shelf note lines"
    );
    (
        file.bytes,
        ShelfMarks {
            rows: [row0, row1, row2],
            sku: [sku0, sku1, sku2],
            note: [note0, note1, note2],
        },
    )
}

fn key_offset(bytes: &[u8], key: &str, nth: usize) -> u64 {
    let needle = format!("\"{key}\"");
    let raw = needle.as_bytes();
    let mut seen = 0usize;
    let mut index = 0usize;
    while index + raw.len() <= bytes.len() {
        if &bytes[index..index + raw.len()] == raw {
            if seen == nth {
                return index as u64;
            }
            seen += 1;
        }
        index += 1;
    }
    panic!("{UUID} missing key {key} #{nth}")
}

fn check_silent(body: &[u8], token: &str, exit: i32, absent: &[&str]) -> Result<(), String> {
    let outcome = run_worker(body);
    let code = outcome.code;
    if code != Some(exit) {
        return Err(format!("{token} exit {code:?}"));
    }
    let text = String::from_utf8(outcome.stderr).map_err(|_| format!("{token} stderr utf8"))?;
    let expected = format!("oo-file-parser-worker: {token}\n");
    if text != expected {
        return Err(format!("{token} stderr mismatched"));
    }
    if !outcome.frames.is_empty() {
        let count = outcome.frames.len();
        return Err(format!("{token} wrote {count} frames"));
    }
    for item in absent {
        if text.contains(item) {
            return Err(format!("{token} stderr leaked"));
        }
    }
    Ok(())
}

#[test]
fn sha256_matches_the_empty_and_abc_vectors() {
    assert_eq!(sha256_hex(b""), EMPTY_SHA, "{UUID} empty vector");
    assert_eq!(sha256_hex(b"abc"), ABC_SHA, "{UUID} abc vector");
}

#[test]
fn csv_decodes_exact_values_lineage_and_receipt() {
    let (bytes, marks) = shelf_bytes();
    let (path, sha) = write_input("shelf", &bytes);
    let meta = fs::metadata(&path).unwrap_or_else(|err| panic!("{UUID} metadata {err}"));
    assert!(meta.is_file(), "{UUID} shelf input is a regular file");
    let profile = shelf_profile();
    let profile_text = shelf_profile_preimage();
    let schema_text = shelf_schema_preimage();
    save_oracle("shelf.csv", &bytes);
    save_oracle("shelf-profile.txt", profile_text.as_bytes());
    save_oracle("shelf-schema.txt", schema_text.as_bytes());
    let outcome = run_worker(request(&path, &sha, &profile, LOOSE_BUDGET).as_bytes());
    assert_ok(&outcome, "csv");
    let paired = pairs(&outcome);
    assert_eq!(paired.len(), 1, "{UUID} csv batches");
    let batch = decode_one(&paired[0].0.payload);
    assert_eq!(
        i64_col(&batch, 1),
        vec![Some(i64::MIN), Some(i64::MAX), Some(0)],
        "{UUID} int64"
    );
    assert_eq!(
        decimal_col(&batch, 2),
        vec![Some(3140), Some(3140), Some(-10)],
        "{UUID} decimal coefficients"
    );
    assert_eq!(
        utf8_col(&batch, 0),
        vec![
            Some("甲".to_owned()),
            Some(String::new()),
            Some("ab\nc".to_owned())
        ],
        "{UUID} sku"
    );
    assert_eq!(utf8_col(&batch, 3)[0], Some(String::new()), "{UUID} quoted empty");
    assert_eq!(utf8_col(&batch, 3)[1], None, "{UUID} null token");
    assert_eq!(utf8_col(&batch, 3)[2].as_deref(), Some("x,y"), "{UUID} quoted comma");
    let price = batch.schema().field(2);
    assert_eq!(
        price.data_type(),
        &DataType::Decimal128(8, 3),
        "{UUID} decimal type"
    );
    let scale = price.metadata().get("oo.delimited.scale").map(String::as_str);
    assert_eq!(scale, Some("3"), "{UUID} decimal scale");
    let lineage = text_lines(&paired[0].1.payload);
    assert_eq!(lineage[0], "oo-file-parser-lineage/v1", "{UUID} lineage prefix");
    for cell in ["甲", "3.140", "3.14", "-9223372036854775808", "9223372036854775807", "x,y"] {
        assert_no_cell(&paired[0].1.payload, cell);
    }
    let profile_sha = sha256_hex(profile_text.as_bytes());
    let config_sha = sha256_hex(
        config_preimage(
            "oo-delimited-config-identity/v1\n",
            &profile_text,
            &sha,
        )
        .as_bytes(),
    );
    let schema_sha = sha256_hex(schema_text.as_bytes());
    assert_eq!(one_value(&lineage, "batch_index"), "0", "{UUID} batch index");
    assert_eq!(one_value(&lineage, "reader_protocol"), "oo.source.delimited/v1");
    assert_eq!(one_value(&lineage, "profile_digest"), profile_sha, "{UUID} profile");
    assert_eq!(one_value(&lineage, "config_identity"), config_sha, "{UUID} config");
    assert_eq!(one_value(&lineage, "schema_identity"), schema_sha, "{UUID} schema");
    let _ = one_value(&lineage, "raw_bytes");
    let _ = one_value(&lineage, "arrow_bytes");
    let marked = row_marks(&lineage);
    assert_eq!(
        marked,
        vec![
            (0, 2, marks.rows[0]),
            (1, 3, marks.rows[1]),
            (2, 4, marks.rows[2])
        ],
        "{UUID} csv rows"
    );
    let fields = field_marks(&lineage);
    assert_eq!(find_field(&fields, "sku", 0).offset, marks.sku[0]);
    assert_eq!(find_field(&fields, "sku", 0).presence, "value");
    assert_eq!(find_field(&fields, "sku", 1).offset, marks.sku[1]);
    assert_eq!(find_field(&fields, "sku", 1).presence, "empty", "{UUID} unquoted empty");
    assert_eq!(find_field(&fields, "sku", 2).offset, marks.sku[2]);
    assert_eq!(find_field(&fields, "sku", 2).line, 4);
    assert_eq!(find_field(&fields, "note", 0).offset, marks.note[0]);
    assert_eq!(find_field(&fields, "note", 0).presence, "empty", "{UUID} quoted empty presence");
    assert_eq!(find_field(&fields, "note", 1).offset, marks.note[1]);
    assert_eq!(find_field(&fields, "note", 1).presence, "null", "{UUID} token null");
    assert_eq!(find_field(&fields, "note", 2).offset, marks.note[2]);
    assert_eq!(find_field(&fields, "note", 2).line, 5, "{UUID} note after quoted newline");
    assert_eq!(find_field(&fields, "note", 2).presence, "value");
    let receipt = complete_lines(&outcome);
    assert_eq!(receipt[0], "oo-file-parser-complete/v1", "{UUID} receipt prefix");
    assert_limits(&receipt);
    for (key, value) in [
        ("protocol", PROTOCOL),
        ("reader_protocol", "oo.source.delimited/v1"),
        ("reader_profile_version", "oo.source.delimited.profile/v1"),
        ("reader_lineage_version", "oo.source.delimited.lineage/v1"),
        ("reader_receipt_version", "oo.source.delimited.receipt/v1"),
        ("profile_id", "a224-shelf"),
        ("profile_digest", profile_sha.as_str()),
        ("config_identity", config_sha.as_str()),
        ("schema_identity", schema_sha.as_str()),
        ("object_sha256", sha.as_str()),
        ("rows", "3"),
        ("columns", "4"),
        ("batches", "1"),
        ("ipc_frames", "1"),
        ("lineage_frames", "1"),
        ("arrow_ipc", "true"),
    ] {
        let got = one_value(&receipt, key);
        assert_eq!(got, value, "{UUID} csv receipt {key}");
    }
}

#[test]
fn tsv_keeps_a_quoted_tab_a_comma_and_negative_zero() {
    let mut file = Tape::new();
    let (row_line, _row0) = file.mark();
    file.push("雀".as_bytes());
    file.push(b"\t");
    let (raw_line, raw0) = file.mark();
    file.push(b"\"q\tt\"\t-0\n");
    let (row_line1, row1) = file.mark();
    file.push(b"\"a,b\"\t");
    let (raw_line1, raw1) = file.mark();
    file.push(b"\t1\n");
    assert_eq!(row_line, 1u64, "{UUID} tsv first line");
    assert_eq!(raw_line, 1u64, "{UUID} tsv quoted tab line");
    assert_eq!(row_line1, 2u64, "{UUID} tsv second line");
    assert_eq!(raw_line1, 2u64, "{UUID} tsv empty line");
    let bytes = file.bytes;
    save_oracle("tab.tsv", &bytes);
    let profile = r#"{"format":"delimited","id":"a224-tab","dialect":"tsv","bom":"refuse","header":"absent","null":{"policy":"empty"},"columns":[{"name":"name","type":"utf8","nullable":false},{"name":"raw","type":"utf8","nullable":true},{"name":"n","type":"int64","nullable":false}]}"#;
    let (path, sha) = write_input("tab", &bytes);
    let outcome = run_worker(request(&path, &sha, profile, LOOSE_BUDGET).as_bytes());
    assert_ok(&outcome, "tsv");
    let paired = pairs(&outcome);
    assert_eq!(paired.len(), 1, "{UUID} tsv batches");
    let batch = decode_one(&paired[0].0.payload);
    assert_eq!(
        utf8_col(&batch, 0),
        vec![Some("雀".to_owned()), Some("a,b".to_owned())],
        "{UUID} tsv name"
    );
    assert_eq!(utf8_col(&batch, 1)[0].as_deref(), Some("q\tt"), "{UUID} quoted tab");
    assert_eq!(utf8_col(&batch, 1)[1], None, "{UUID} unquoted empty null");
    assert_eq!(i64_col(&batch, 2), vec![Some(0), Some(1)], "{UUID} tsv -0");
    let lineage = text_lines(&paired[0].1.payload);
    assert_no_cell(&paired[0].1.payload, "雀");
    assert_no_cell(&paired[0].1.payload, "a,b");
    let marked = row_marks(&lineage);
    assert_eq!(marked[1], (1, 2, row1), "{UUID} tsv second row");
    let fields = field_marks(&lineage);
    assert_eq!(find_field(&fields, "raw", 0).offset, raw0);
    assert_eq!(find_field(&fields, "raw", 0).presence, "value");
    assert_eq!(find_field(&fields, "raw", 1).offset, raw1);
    assert_eq!(find_field(&fields, "raw", 1).presence, "null");
    let receipt = complete_lines(&outcome);
    assert_limits(&receipt);
    assert_eq!(one_value(&receipt, "object_sha256"), sha);
    assert_eq!(one_value(&receipt, "rows"), "2");
    assert_eq!(one_value(&receipt, "profile_id"), "a224-tab");
    assert_eq!(one_value(&receipt, "arrow_ipc"), "true");
}

#[test]
fn jsonl_separates_null_from_missing_across_three_frames() {
    let mut bytes = Vec::new();
    let mut starts = [0u64; 3];
    let records = [
        "{\"label\":\"乙\",\"n\":-1,\"flag\":false}\n",
        "{\"label\":null,\"n\":-0,\"flag\":true}\n",
        "{\"flag\":false,\"n\":7}\n",
    ];
    for (index, record) in records.iter().enumerate() {
        starts[index] = bytes.len() as u64;
        bytes.extend_from_slice(record.as_bytes());
        assert_eq!(bytes[starts[index] as usize], b'{', "{UUID} jsonl object");
    }
    let profile_text = notes_profile_preimage();
    let schema_text = notes_schema_preimage();
    save_oracle("notes.jsonl", &bytes);
    save_oracle("notes-profile.txt", profile_text.as_bytes());
    save_oracle("notes-schema.txt", schema_text.as_bytes());
    let profile = r#"{"format":"jsonl","id":"a224-notes","columns":[{"name":"label","type":"utf8","nullable":true},{"name":"n","type":"int64","nullable":false},{"name":"flag","type":"bool","nullable":false}]}"#;
    let (path, sha) = write_input("notes", &bytes);
    let outcome = run_worker(request(&path, &sha, profile, JSONL_BUDGET).as_bytes());
    assert_ok(&outcome, "jsonl");
    let paired = pairs(&outcome);
    assert_eq!(paired.len(), 3, "{UUID} jsonl frames");
    let profile_sha = sha256_hex(profile_text.as_bytes());
    let config_sha = sha256_hex(
        config_preimage("oo-jsonl-config-identity/v1\n", &profile_text, &sha).as_bytes(),
    );
    let schema_sha = sha256_hex(schema_text.as_bytes());
    let expected_n = [Some(-1_i64), Some(0), Some(7)];
    let expected_flag = [Some(false), Some(true), Some(false)];
    let expected_presence = ["value", "null", "missing"];
    let label_offsets = [key_offset(&bytes, "label", 0), key_offset(&bytes, "label", 1), starts[2]];
    for (index, (ipc, lineage_frame)) in paired.iter().enumerate() {
        let batch = decode_one(&ipc.payload);
        assert_eq!(i64_col(&batch, 1), vec![expected_n[index]], "{UUID} jsonl int");
        assert_eq!(bool_col(&batch, 2), vec![expected_flag[index]], "{UUID} jsonl flag");
        let lineage = text_lines(&lineage_frame.payload);
        assert_eq!(one_value(&lineage, "batch_index"), index.to_string());
        assert_eq!(one_value(&lineage, "reader_protocol"), "oo.source.jsonl/v1");
        assert_eq!(one_value(&lineage, "profile_digest"), profile_sha);
        assert_eq!(one_value(&lineage, "config_identity"), config_sha);
        assert_eq!(one_value(&lineage, "schema_identity"), schema_sha);
        assert_no_cell(&lineage_frame.payload, "乙");
        let marked = row_marks(&lineage);
        let row_index = index as u64;
        let row_line = row_index + 1;
        assert_eq!(marked, vec![(row_index, row_line, starts[index])]);
        let fields = field_marks(&lineage);
        let label = find_field(&fields, "label", 0);
        assert_eq!(label.presence, expected_presence[index], "{UUID} label presence");
        assert_eq!(label.offset, label_offsets[index], "{UUID} label offset");
    }
    assert_eq!(utf8_col(&decode_one(&paired[0].0.payload), 0), vec![Some("乙".to_owned())]);
    assert_eq!(utf8_col(&decode_one(&paired[1].0.payload), 0), vec![None]);
    assert_eq!(utf8_col(&decode_one(&paired[2].0.payload), 0), vec![None]);
    let first = decode_one(&paired[0].0.payload);
    let logical = first
        .schema()
        .field(0)
        .metadata()
        .get("oo.jsonl.logical")
        .map(String::as_str);
    assert_eq!(logical, Some("utf8"), "{UUID} jsonl logical");
    assert_eq!(first.schema().field(1).data_type(), &DataType::Int64);
    assert_eq!(first.schema().field(2).data_type(), &DataType::Boolean);
    let receipt = complete_lines(&outcome);
    assert_limits(&receipt);
    for (key, value) in [
        ("reader_protocol", "oo.source.jsonl/v1"),
        ("reader_profile_version", "oo.source.jsonl.profile/v1"),
        ("reader_lineage_version", "oo.source.jsonl.lineage/v1"),
        ("reader_receipt_version", "oo.source.jsonl.receipt/v1"),
        ("profile_id", "a224-notes"),
        ("object_sha256", sha.as_str()),
        ("rows", "3"),
        ("columns", "3"),
        ("batches", "3"),
        ("ipc_frames", "3"),
        ("lineage_frames", "3"),
        ("arrow_ipc", "true"),
        ("profile_digest", profile_sha.as_str()),
        ("schema_identity", schema_sha.as_str()),
    ] {
        let got = one_value(&receipt, key);
        assert_eq!(got, value, "{UUID} jsonl receipt {key}");
    }
}

#[test]
fn empty_eof_header_policy_and_header_only_bind_completion() {
    let (empty_path, empty_sha) = write_input("empty", b"");
    assert_eq!(empty_sha, EMPTY_SHA, "{UUID} empty file");
    save_oracle("empty.bin", b"");
    let absent = r#"{"format":"delimited","id":"a224-blank","dialect":"csv","bom":"refuse","header":"absent","null":{"policy":"empty"},"columns":[{"name":"sku","type":"utf8","nullable":true}]}"#;
    let absent_out = run_worker(request(&empty_path, &empty_sha, absent, LOOSE_BUDGET).as_bytes());
    assert_ok(&absent_out, "empty absent");
    assert!(pairs(&absent_out).is_empty(), "{UUID} empty absent emitted a batch");
    let absent_receipt = complete_lines(&absent_out);
    assert_limits(&absent_receipt);
    assert_eq!(one_value(&absent_receipt, "rows"), "0");
    assert_eq!(one_value(&absent_receipt, "batches"), "0");
    assert_eq!(one_value(&absent_receipt, "ipc_frames"), "0");
    assert_eq!(one_value(&absent_receipt, "arrow_ipc"), "false");
    assert_eq!(one_value(&absent_receipt, "object_sha256"), EMPTY_SHA);
    assert_eq!(absent_out.frames.len(), 1, "{UUID} empty absent frames");

    let exact = r#"{"format":"delimited","id":"a224-blank-exact","dialect":"csv","bom":"refuse","header":"exact","null":{"policy":"empty"},"columns":[{"name":"sku","type":"utf8","nullable":true}]}"#;
    let exact_out = run_worker(request(&empty_path, &empty_sha, exact, LOOSE_BUDGET).as_bytes());
    let exact_code = exact_out.code;
    assert_eq!(exact_code, Some(4), "{UUID} header_required exit {exact_code:?}");
    let exact_err = String::from_utf8(exact_out.stderr.clone()).unwrap_or_else(|_| panic!("{UUID} stderr"));
    assert_eq!(exact_err, "oo-file-parser-worker: header_required\n");
    assert!(exact_out.frames.is_empty(), "{UUID} header_required wrote a frame");

    let header_bytes = b"sku,qty,price,note\n";
    let (header_path, header_sha) = write_input("header-only", header_bytes);
    let header_out = run_worker(
        request(&header_path, &header_sha, &shelf_profile(), LOOSE_BUDGET).as_bytes(),
    );
    assert_ok(&header_out, "header only");
    assert!(pairs(&header_out).is_empty(), "{UUID} header only emitted data");
    let profile_sha = sha256_hex(shelf_profile_preimage().as_bytes());
    let schema_sha = sha256_hex(shelf_schema_preimage().as_bytes());
    let config_sha = sha256_hex(
        config_preimage(
            "oo-delimited-config-identity/v1\n",
            &shelf_profile_preimage(),
            &header_sha,
        )
        .as_bytes(),
    );
    let header_receipt = complete_lines(&header_out);
    assert_limits(&header_receipt);
    assert_eq!(one_value(&header_receipt, "rows"), "0");
    assert_eq!(one_value(&header_receipt, "batches"), "0");
    assert_eq!(one_value(&header_receipt, "arrow_ipc"), "false");
    assert_eq!(one_value(&header_receipt, "object_sha256"), header_sha);
    assert_eq!(one_value(&header_receipt, "profile_digest"), profile_sha);
    assert_eq!(one_value(&header_receipt, "config_identity"), config_sha);
    assert_eq!(one_value(&header_receipt, "schema_identity"), schema_sha);

    let jsonl = r#"{"format":"jsonl","id":"a224-blank-jsonl","columns":[{"name":"n","type":"int64","nullable":false}]}"#;
    let jsonl_budget = r#"{"batch_rows":8,"batch_bytes":1048576,"max_record_bytes":65536,"max_total_bytes":1048576,"max_rows":1000,"max_columns":16,"max_fields":64,"deadline_ms":60000}"#;
    let jsonl_out = run_worker(request(&empty_path, &empty_sha, jsonl, jsonl_budget).as_bytes());
    assert_ok(&jsonl_out, "empty jsonl");
    let jsonl_receipt = complete_lines(&jsonl_out);
    assert_limits(&jsonl_receipt);
    assert_eq!(one_value(&jsonl_receipt, "rows"), "0");
    assert_eq!(one_value(&jsonl_receipt, "reader_protocol"), "oo.source.jsonl/v1");
    assert_eq!(one_value(&jsonl_receipt, "arrow_ipc"), "false");
    let jsonl_profile = "oo-jsonl-profile/v1\nid=a224-blank-jsonl\ncolumn=n\tint64\t0\n";
    let jsonl_schema = "oo-file-parser-schema/v1\nfield=n\tint64\t0\nmeta=n\too.jsonl.logical\tint64\nmeta=n\too.jsonl.protocol\too.source.jsonl/v1\n";
    assert_eq!(
        one_value(&jsonl_receipt, "profile_digest"),
        sha256_hex(jsonl_profile.as_bytes())
    );
    assert_eq!(
        one_value(&jsonl_receipt, "schema_identity"),
        sha256_hex(jsonl_schema.as_bytes())
    );
}

#[test]
fn late_hash_mismatch_keeps_the_batch_and_omits_completion() {
    let bytes = "丁\n".as_bytes();
    let (path, sha) = write_input("late", bytes);
    let mut wrong = sha.clone();
    let last = wrong.len() - 1;
    let flipped = other_hex(wrong.as_bytes()[last]);
    wrong.replace_range(last..last + 1, &flipped.to_string());
    assert_ne!(wrong, sha, "{UUID} flipped hash");
    assert_eq!(wrong.len(), 64, "{UUID} flipped length");
    let profile = r#"{"format":"delimited","id":"a224-late","dialect":"csv","bom":"refuse","header":"absent","null":{"policy":"empty"},"columns":[{"name":"name","type":"utf8","nullable":false}]}"#;
    let profile_text = "oo-delimited-profile/v1\n\
id=a224-late\n\
dialect=csv\n\
bom=refuse\n\
header=absent\n\
null=empty\n\
quote=double\n\
record=lf-or-crlf\n\
escape=double-quote\n\
column=name\tutf8\t0\t-\t-\n";
    let outcome = run_worker(request(&path, &wrong, profile, LOOSE_BUDGET).as_bytes());
    let code = outcome.code;
    assert_eq!(code, Some(6), "{UUID} hash exit {code:?}");
    let stderr = String::from_utf8(outcome.stderr.clone()).unwrap_or_else(|_| panic!("{UUID} stderr"));
    assert_eq!(stderr, "oo-file-parser-worker: hash_mismatch\n");
    let shown = path.display().to_string();
    assert!(!stderr.contains(&shown), "{UUID} hash stderr leaked the path");
    assert!(!stderr.contains('丁'), "{UUID} hash stderr copied the cell");
    assert!(
        outcome.frames.iter().all(|frame| frame.tag != TAG_COMPLETE),
        "{UUID} hash mismatch completed"
    );
    assert_eq!(outcome.frames.len(), 2, "{UUID} hash frames");
    assert_eq!(outcome.frames[0].tag, TAG_IPC, "{UUID} hash ipc");
    assert_eq!(outcome.frames[1].tag, TAG_LINEAGE, "{UUID} hash lineage");
    assert_eq!(
        utf8_col(&decode_one(&outcome.frames[0].payload), 0),
        vec![Some("丁".to_owned())],
        "{UUID} hash batch"
    );
    assert_no_cell(&outcome.frames[1].payload, "丁");
    let lineage = text_lines(&outcome.frames[1].payload);
    let config_sha = sha256_hex(
        config_preimage("oo-delimited-config-identity/v1\n", profile_text, &wrong).as_bytes(),
    );
    assert_eq!(one_value(&lineage, "config_identity"), config_sha, "{UUID} late config");
    assert_eq!(
        one_value(&lineage, "profile_digest"),
        sha256_hex(profile_text.as_bytes())
    );
}

fn other_hex(byte: u8) -> char {
    if byte == b'0' { '1' } else { '0' }
}

#[test]
fn unknown_profile_request_cap_and_path_class_refuse() {
    let mut problems = Vec::new();
    let absent = scratch("parquet-absent");
    let absent_shown = absent.display().to_string();
    let parquet = format!(
        r#"{{"format":"parquet","id":"a224-nope"}}"#
    );
    let body = request(&absent, EMPTY_SHA, &parquet, JSONL_BUDGET);
    if let Err(err) = check_silent(body.as_bytes(), "config", 2, &["parquet", &absent_shown]) {
        problems.push(err);
    }
    let mut huge = vec![b'{'; 1_048_577];
    huge[0] = b'{';
    if let Err(err) = check_silent(&huge, "request_too_large", 2, &[]) {
        problems.push(err);
    }
    let (_path, sha) = write_input("tiny", b"a\n");
    let profile = r#"{"format":"delimited","id":"a224-one","dialect":"csv","bom":"refuse","header":"absent","null":{"policy":"empty"},"columns":[{"name":"name","type":"utf8","nullable":true}]}"#;
    let relative = request(Path::new("relative.csv"), &sha, profile, LOOSE_BUDGET);
    if let Err(err) = check_silent(relative.as_bytes(), "path_form", 2, &["relative.csv"]) {
        problems.push(err);
    }
    let dir = scratch("dir");
    fs::create_dir(&dir).unwrap_or_else(|err| panic!("{UUID} mkdir {err}"));
    let dir_body = request(&dir, &sha, profile, LOOSE_BUDGET);
    let dir_shown = dir.display().to_string();
    if let Err(err) = check_silent(dir_body.as_bytes(), "not_regular", 2, &[&dir_shown]) {
        problems.push(err);
    }
    let target = scratch("target");
    fs::write(&target, b"a\n").unwrap_or_else(|err| panic!("{UUID} target {err}"));
    let link = scratch("link");
    std::os::unix::fs::symlink(&target, &link).unwrap_or_else(|err| panic!("{UUID} symlink {err}"));
    let link_body = request(&link, &sha, profile, LOOSE_BUDGET);
    let link_shown = link.display().to_string();
    if let Err(err) = check_silent(link_body.as_bytes(), "not_regular", 2, &[&link_shown]) {
        problems.push(err);
    }
    let fifo = scratch("fifo");
    let made = Command::new("/usr/bin/mkfifo")
        .arg(&fifo)
        .status()
        .unwrap_or_else(|err| panic!("{UUID} mkfifo {err}"));
    assert!(made.success(), "{UUID} mkfifo status");
    let fifo_body = request(&fifo, &sha, profile, LOOSE_BUDGET);
    let fifo_shown = fifo.display().to_string();
    if let Err(err) = check_silent(fifo_body.as_bytes(), "not_regular", 2, &[&fifo_shown]) {
        problems.push(err);
    }
    if !problems.is_empty() {
        let joined = problems.join("; ");
        panic!("{UUID} refusals {joined}");
    }
}

#[test]
fn ipc_ceiling_writes_no_frame() {
    let width = 40usize;
    let rows = 204usize;
    let mut row = Vec::new();
    for index in 0..width {
        if index > 0 {
            row.push(b',');
        }
        row.push(b'0');
    }
    row.push(b'\n');
    assert_eq!(row.len(), 80, "{UUID} ceiling row width");
    let mut bytes = Vec::new();
    for _ in 0..rows {
        bytes.extend_from_slice(&row);
    }
    assert_eq!(bytes.len(), 204 * 80, "{UUID} ceiling file length");
    let mut columns = String::from("[");
    for index in 0..width {
        if index > 0 {
            columns.push(',');
        }
        let name = format!("c{index:02}");
        columns.push_str(&format!(
            r#"{{"name":"{name}","type":"decimal","nullable":false,"precision":2,"scale":0}}"#
        ));
    }
    columns.push(']');
    let profile = format!(
        r#"{{"format":"delimited","id":"a224-cap","dialect":"csv","bom":"refuse","header":"absent","null":{{"policy":"empty"}},"columns":{columns}}}"#
    );
    let (path, sha) = write_input("ceiling", &bytes);
    let outcome = run_worker(request(&path, &sha, &profile, CEILING_BUDGET).as_bytes());
    let code = outcome.code;
    assert_eq!(code, Some(8), "{UUID} ipc ceiling exit {code:?}");
    let stderr = String::from_utf8(outcome.stderr).unwrap_or_else(|_| panic!("{UUID} stderr"));
    assert_eq!(stderr, "oo-file-parser-worker: ipc_ceiling\n");
    assert!(outcome.frames.is_empty(), "{UUID} ipc ceiling wrote a frame");
}

#[test]
fn parent_kills_a_blocked_read_and_the_drop_guard_reaps() {
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
    assert_eq!(outcome.code, None, "{UUID} kill must be a signal, not an exit code");
    assert!(
        outcome.frames.iter().all(|frame| frame.tag != TAG_COMPLETE),
        "{UUID} killed child completed"
    );
    let proc_path = format!("/proc/{pid}");
    assert!(!Path::new(&proc_path).exists(), "{UUID} child {pid} was still alive");

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
    match stdout.join() {
        Ok(_) => {}
        Err(_) => panic!("{UUID} drop stdout thread panicked"),
    }
    match stderr.join() {
        Ok(_) => {}
        Err(_) => panic!("{UUID} drop stderr thread panicked"),
    }
    drop(stdin);
    let proc_path = format!("/proc/{pid}");
    assert!(
        !Path::new(&proc_path).exists(),
        "{UUID} drop guard left {pid} alive"
    );
}
