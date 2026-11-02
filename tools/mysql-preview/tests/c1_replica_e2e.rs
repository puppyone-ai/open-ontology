//! Live tests for the replica read source (DESIGN §5 R1–R5, §6 T14). They run
//! `c1-baseline-stream-v2` with `read_source: replica` against an owned T12 fixture
//! (primary and replica, `testing/chunked-baseline/fixtures/up.sh --variant 57|84`).
//! They are `#[ignore]` by default. Run one variant at a time:
//!
//!   1. On the host: `testing/chunked-baseline/data/seed.sh --variant 57 --profile small --reset`
//!   2. On the host: `tools/mysql-preview/tests/replica_fault_agent.sh --variant 57 --dir $FAULTS`
//!      (faults need docker, which only the host has; this agent runs whitelisted actions)
//!   3. In the container, with the same paths mounted, set the environment below and run
//!      `cargo test --test c1_replica_e2e -- --ignored --test-threads=1`
//!
//! Environment: OO_CB_T4_VARIANT (57|84), OO_CB_T4_HOST (a name the container can reach,
//! for example host.docker.internal), OO_CB_T4_TLS_NAME (a SAN of the T12 certificate,
//! for example localhost), OO_CB_T4_PRIMARY_PORT, OO_CB_T4_REPLICA_PORT, OO_CB_T4_CA,
//! OO_CB_T4_CDC_PW_FILE (oo_cdc), OO_CB_T4_LOAD_PW_FILE (oo_load),
//! OO_CB_T4_READER_PW_FILE (oo_reader), OO_CB_T4_FAULT_DIR (the agent's directory),
//! OO_CB_T4_WORK (a writable scratch directory).
//!
//! The fake worker follows FRAMES-V2.md. It reads each part back through
//! `oo_pipeline::c1_parts` before it acknowledges, and it keeps the state a real worker
//! would keep. At the end, that state must equal the primary.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, SslOpts};
use oo_pipeline::c1_parts::{PartEntry, PartKind, PartManifest, read_part};
use oo_pipeline::native_arrow::NativeCell;
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_mysql-preview");
const DB: &str = "oo_c1";
const HDR: &str = "oo_c1.cb_hdr";
const LINE: &str = "oo_c1.cb_line";
/// Seeded keys (T12 small profile): cb_hdr 1000000..1001999, cb_line 2000000..2007999.
const HDR_SEED: u64 = 1_000_000;
const LINE_SEED: u64 = 2_000_000;
const CHUNK_ROWS: u64 = 100;

#[derive(Clone)]
struct Env {
    fixture: &'static str,
    server_id: u32,
    host: String,
    tls_name: String,
    primary_port: u16,
    replica_port: u16,
    ca: PathBuf,
    cdc_pw: PathBuf,
    load_pw: PathBuf,
    reader_pw: PathBuf,
    faults: PathBuf,
    work: PathBuf,
}

fn env() -> Env {
    let get = |name: &str| {
        std::env::var(name).unwrap_or_else(|_| panic!("set {name} (see the file header)"))
    };
    let variant = get("OO_CB_T4_VARIANT");
    let (fixture, server_id) = match variant.as_str() {
        "57" => ("owned-mysql-57", 5799),
        "84" => ("owned-mysql-8", 8499),
        other => panic!("OO_CB_T4_VARIANT must be 57 or 84, got {other}"),
    };
    let work = PathBuf::from(get("OO_CB_T4_WORK"));
    fs::create_dir_all(&work).unwrap();
    Env {
        fixture,
        server_id,
        host: get("OO_CB_T4_HOST"),
        tls_name: get("OO_CB_T4_TLS_NAME"),
        primary_port: get("OO_CB_T4_PRIMARY_PORT").parse().unwrap(),
        replica_port: get("OO_CB_T4_REPLICA_PORT").parse().unwrap(),
        ca: PathBuf::from(get("OO_CB_T4_CA")),
        cdc_pw: PathBuf::from(get("OO_CB_T4_CDC_PW_FILE")),
        load_pw: PathBuf::from(get("OO_CB_T4_LOAD_PW_FILE")),
        reader_pw: PathBuf::from(get("OO_CB_T4_READER_PW_FILE")),
        faults: PathBuf::from(get("OO_CB_T4_FAULT_DIR")),
        work,
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn read_pw(path: &Path) -> String {
    fs::read_to_string(path).unwrap().trim().to_owned()
}

fn tls(env: &Env) -> SslOpts {
    SslOpts::default()
        .with_root_certs(vec![env.ca.clone().into()])
        .with_danger_accept_invalid_certs(false)
        .with_danger_skip_domain_validation(false)
        .with_danger_tls_hostname_override(Some(env.tls_name.clone()))
        .with_disable_built_in_roots(true)
}

async fn open(env: &Env, port: u16, user: &str, password_file: &Path) -> Conn {
    let opts = OptsBuilder::default()
        .ip_or_hostname(env.host.clone())
        .tcp_port(port)
        .user(Some(user.to_owned()))
        .pass(Some(read_pw(password_file)))
        .db_name(Some(DB.to_owned()))
        .prefer_socket(false)
        .ssl_opts(tls(env));
    Conn::new(opts).await.unwrap()
}

async fn primary(env: &Env) -> Conn {
    open(env, env.primary_port, "oo_load", &env.load_pw).await
}

async fn replica_reader(env: &Env) -> Conn {
    open(env, env.replica_port, "oo_reader", &env.reader_pw).await
}

async fn server_uuid(conn: &mut Conn) -> String {
    conn.query_first::<String, _>("SELECT @@GLOBAL.server_uuid")
        .await
        .unwrap()
        .unwrap()
}

/// Full state of one table, keyed by its integer primary key.
async fn table_state(conn: &mut Conn, table: &str) -> BTreeMap<i128, String> {
    let sql = if table == HDR {
        "SELECT id, note FROM cb_hdr"
    } else {
        "SELECT id, sku FROM cb_line"
    };
    let rows: Vec<(i64, String)> = conn.query(sql).await.unwrap();
    rows.into_iter()
        .map(|(id, text)| (i128::from(id), text))
        .collect()
}

/// Config for the producer. `read_replica` is the operator block (credentials stay in files).
fn write_config(env: &Env, dir: &Path, replica: bool) -> PathBuf {
    let mut config = json!({
        "database": DB,
        "host": env.host,
        "port": env.primary_port,
        "user": "oo_cdc",
        "password_file": env.cdc_pw,
        "dba_user": "oo_cdc",
        "dba_password_file": env.cdc_pw,
        "ca_file": env.ca,
        "server_id": env.server_id,
        "health_every_ms": 500,
        "reconcile_every_ms": 1000,
        "preserve_dir": dir,
        "fixture": env.fixture,
        "tables": [HDR, LINE],
        "columns": {HDR: ["id", "note", "status", "updated_at"], LINE: ["id", "hdr_id", "qty", "sku"]},
        "secret_ref": "secret://mysql-owned-fixture",
        "tls_server_name": env.tls_name,
    });
    if replica {
        config["read_replica"] = json!({
            "host": env.host,
            "port": env.replica_port,
            "user": "oo_reader",
            "password_file": env.reader_pw,
            "ca_file": env.ca,
            "tls_server_name": env.tls_name,
            "wait_timeout_s": 2,
            "retry_budget_s": 600,
        });
    }
    fs::create_dir_all(dir).unwrap();
    let path = dir.join("producer-config.json");
    fs::write(&path, config.to_string()).unwrap();
    path
}

/// Bootstrap with a replica read source. `wait` is the WAIT timeout in seconds, and the
/// replica retry budget comes from the producer config (`write_config`).
fn bootstrap(identity: Option<&str>, wait: u64) -> Value {
    let mut read_source = json!({"kind": "replica", "wait_timeout_s": wait});
    if let Some(identity) = identity {
        read_source["replica_identity"] = json!(identity);
    }
    json!({
        "protocol": "oo.jobs.mysql-c1-bootstrap/v2",
        "source_id": "mysql_t4e2e",
        "source_generation": 1,
        "baseline": {"format": "chunked-typed/v2", "chunk_rows": CHUNK_ROWS, "catchup_stall_ms": 120000},
        "read_source": read_source,
        "resume": null,
        "previous": null,
    })
}

/// A producer child process. Stdout frames arrive on a channel. Stderr goes to a file.
struct Run {
    child: Child,
    stdin: ChildStdin,
    frames: mpsc::Receiver<Result<String, String>>,
    stderr: PathBuf,
}

fn spawn(config: &Path, out: &Path, stderr: &Path) -> Run {
    let mut child = Command::new(BIN)
        .arg("c1-baseline-stream-v2")
        .arg("--config")
        .arg(config)
        .arg("--out")
        .arg(out)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(fs::File::create(stderr).unwrap())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let item = line.map_err(|err| err.to_string());
            let stop = item.is_err();
            if tx.send(item).is_err() || stop {
                return;
            }
        }
    });
    Run {
        child,
        stdin,
        frames: rx,
        stderr: stderr.to_path_buf(),
    }
}

impl Run {
    fn send(&mut self, value: &Value) {
        writeln!(self.stdin, "{value}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Next frame, or `None` when the producer closed stdout. A silent producer fails.
    fn next(&mut self) -> Option<Value> {
        match self.frames.recv_timeout(Duration::from_secs(240)) {
            Ok(Ok(line)) => Some(serde_json::from_str(&line).expect("frame is JSON")),
            Ok(Err(err)) => panic!("stdout read failed: {err}"),
            Err(mpsc::RecvTimeoutError::Disconnected) => None,
            Err(mpsc::RecvTimeoutError::Timeout) => panic!("producer silent for 240 s"),
        }
    }

    fn ack(&mut self, stage: &str, extra: Value) {
        let mut ack = json!({"protocol": "oo.jobs.mysql-c1-baseline-ack/v2", "stage": stage, "durable": true});
        for (key, value) in extra.as_object().cloned().unwrap_or_default() {
            ack[key] = value;
        }
        self.send(&ack);
    }

    fn finish(mut self) -> (ExitStatus, String) {
        let status = self.child.wait().unwrap();
        (status, self.stderr_text())
    }

    fn stderr_text(&self) -> String {
        fs::read_to_string(&self.stderr).unwrap_or_default()
    }
}

/// What the fake worker has acknowledged, per table, plus the frames it saw.
#[derive(Default, Clone)]
struct TableModel {
    fill: BTreeMap<i128, String>,
    events: Vec<(u64, i128, bool, Option<String>)>,
    last_chunk_no: u64,
    last_key_hi: Option<i128>,
    terminal: bool,
}

#[derive(Default)]
struct Model {
    l0: String,
    l0_pos: u64,
    applied: u64,
    tables: BTreeMap<String, TableModel>,
    chunks: Vec<Value>,
    batches: u64,
    cdc_rows: u64,
}

impl Model {
    /// DESIGN §3.1 applied to the frames: the last event by position wins. Fill rows
    /// have position 0.
    fn state(&self, table: &str) -> BTreeMap<i128, String> {
        let table_model = &self.tables[table];
        let mut out = table_model.fill.clone();
        let mut events = table_model.events.clone();
        events.sort_by_key(|event| event.0);
        for (_, key, delete, payload) in events {
            if delete {
                out.remove(&key);
            } else {
                out.insert(key, payload.expect("upsert has a payload"));
            }
        }
        out
    }
}

fn key_of(cell: &NativeCell) -> i128 {
    match cell {
        NativeCell::Int { value, .. } => i128::from(*value),
        NativeCell::UInt64(v) => i128::from(*v),
        NativeCell::Decimal { coefficient, .. } => *coefficient,
        other => panic!("unexpected key cell {other:?}"),
    }
}

fn text_of(cell: &NativeCell) -> Option<String> {
    match cell {
        NativeCell::Null => None,
        NativeCell::Utf8(text) => Some(text.clone()),
        other => panic!("unexpected payload cell {other:?}"),
    }
}

fn part_entry(frame: &Value, kind: PartKind, part_no: u32) -> PartEntry {
    let key = |name: &str| frame[name].as_str().map(|s| s.parse::<i128>().unwrap());
    PartEntry {
        part_no,
        kind,
        key_lo: key("key_lo"),
        key_hi: key("key_hi"),
        rows: frame["rows"].as_u64().unwrap(),
        bytes: frame["part_bytes"].as_u64().unwrap(),
        sha256: frame["part_sha256"].as_str().unwrap().to_owned(),
    }
}

/// Reads one part exactly as a worker would before it acknowledges the frame.
fn check_part(
    out: &Path,
    table: &str,
    entry: &PartEntry,
) -> oo_pipeline::native_arrow::DecodedStream {
    let dir = out.join("parts").join(table);
    read_part(&dir, entry)
        .unwrap_or_else(|err| panic!("part {table} {} rejected: {err}", entry.part_no))
}

fn payload_column(table: &str) -> &'static str {
    if table == HDR { "note" } else { "sku" }
}

/// Plays the worker until the producer closes stdout. `hook` runs after each chunk
/// acknowledgement, so a test can act between chunks.
fn drive(run: &mut Run, model: &mut Model, out: &Path, hook: &mut dyn FnMut(&Value)) {
    while let Some(frame) = run.next() {
        match frame["kind"].as_str().unwrap_or("") {
            "open" => {
                model.l0 = frame["l0"].as_str().unwrap().to_owned();
                model.l0_pos = frame["l0_pos"].as_str().unwrap().parse().unwrap();
                model.applied = model.l0_pos;
                run.ack("stage_open", json!({"source_epoch": 1}));
            }
            "stream_started" => run.ack("stream_started", json!({})),
            "chunk" => {
                let table = format!(
                    "{}.{}",
                    frame["schema"].as_str().unwrap(),
                    frame["table"].as_str().unwrap()
                );
                let chunk_no = frame["chunk_no"].as_u64().unwrap() as u32;
                let entry = part_entry(&frame, PartKind::Fill, chunk_no);
                let stream = check_part(out, &table, &entry);
                assert_eq!(stream.rows as u64, entry.rows, "row count in part");
                let keycol = stream.fields.iter().position(|f| f.name == "id").unwrap();
                let payload = stream
                    .fields
                    .iter()
                    .position(|f| f.name == payload_column(&table))
                    .unwrap();
                let st = model.tables.entry(table.clone()).or_default();
                assert_eq!(
                    u64::from(chunk_no),
                    st.last_chunk_no + 1,
                    "chunk numbers are contiguous"
                );
                if !frame["terminal"].as_bool().unwrap() {
                    let lo = frame["key_lo"].as_str().unwrap().parse::<i128>().unwrap();
                    let hi = frame["key_hi"].as_str().unwrap().parse::<i128>().unwrap();
                    if let Some(prev) = st.last_key_hi {
                        assert!(lo > prev, "chunk {chunk_no} overlaps the previous one");
                    }
                    for row in 0..stream.rows {
                        let key = key_of(&stream.columns[keycol][row]);
                        assert!(key >= lo && key <= hi, "key {key} outside the chunk range");
                        st.fill
                            .insert(key, text_of(&stream.columns[payload][row]).unwrap());
                    }
                    st.last_key_hi = Some(hi);
                }
                st.last_chunk_no = u64::from(chunk_no);
                st.terminal = frame["terminal"].as_bool().unwrap();
                model.chunks.push(frame.clone());
                run.ack(
                    "chunk_apply",
                    json!({"chunk_no": chunk_no, "outcome": "loaded",
                    "schema": frame["schema"], "table": frame["table"]}),
                );
                hook(&frame);
            }
            "txn_batch" => {
                let batch_no = frame["batch_no"].as_u64().unwrap();
                let first: u64 = frame["first_pos"].as_str().unwrap().parse().unwrap();
                let last: u64 = frame["last_pos"].as_str().unwrap().parse().unwrap();
                assert_eq!(first, model.applied + 1, "batch chain has no gap");
                for part in frame["parts"].as_array().unwrap() {
                    let table = format!(
                        "{}.{}",
                        part["schema"].as_str().unwrap(),
                        part["table"].as_str().unwrap()
                    );
                    let entry = part_entry(
                        part,
                        PartKind::Cdc,
                        part["part_no"].as_u64().unwrap() as u32,
                    );
                    let stream = check_part(out, &table, &entry);
                    let n = stream.fields.len();
                    let keycol = stream.fields.iter().position(|f| f.name == "id").unwrap();
                    let payload = stream
                        .fields
                        .iter()
                        .position(|f| f.name == payload_column(&table))
                        .unwrap();
                    let st = model.tables.entry(table.clone()).or_default();
                    for row in 0..stream.rows {
                        let pos = match &stream.columns[n - 2][row] {
                            NativeCell::Decimal { coefficient, .. } => *coefficient as u64,
                            other => panic!("pos cell {other:?}"),
                        };
                        assert!(
                            pos >= first && pos <= last,
                            "row position {pos} outside the batch"
                        );
                        let op = match &stream.columns[n - 1][row] {
                            NativeCell::Utf8(t) => t.clone(),
                            other => panic!("op cell {other:?}"),
                        };
                        let key = key_of(&stream.columns[keycol][row]);
                        let text = if op == "delete" {
                            None
                        } else {
                            text_of(&stream.columns[payload][row])
                        };
                        st.events.push((pos, key, op == "delete", text));
                    }
                    model.cdc_rows += entry.rows;
                }
                model.applied = last;
                model.batches += 1;
                run.ack("txn_batch", json!({"batch_no": batch_no, "last_pos": last.to_string(), "outcome": "applied"}));
            }
            "end" => {
                let manifest = out.join(frame["parts_manifest"].as_str().unwrap());
                let bytes = fs::read(&manifest).unwrap();
                assert_eq!(
                    sha256_hex(&bytes),
                    frame["parts_manifest_sha256"].as_str().unwrap()
                );
                let doc: Value = serde_json::from_slice(&bytes).unwrap();
                for table in doc["tables"].as_array().unwrap() {
                    PartManifest::from_json(&table["manifest"])
                        .expect("per-table manifest validates");
                }
                assert_eq!(
                    frame["applied_pos"].as_str().unwrap(),
                    model.applied.to_string()
                );
                run.ack("end", json!({"outcome": "applied"}));
            }
            other => panic!("unexpected frame kind {other:?}: {frame}"),
        }
    }
}

fn receipt(out: &Path) -> Value {
    serde_json::from_slice(&fs::read(out.join("receipt.json")).unwrap()).unwrap()
}

/// Waits until `needle` appears in the producer's stderr.
fn wait_for_stderr(path: &Path, needle: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if fs::read_to_string(path)
            .unwrap_or_default()
            .contains(needle)
        {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// Runs `action` once the producer's stderr shows `needle`. It runs on a helper thread,
/// so the test keeps driving the worker and the producer is never left waiting for an ack.
/// The helper returns whether the marker was seen.
fn when_stderr(
    path: PathBuf,
    needle: &'static str,
    action: impl FnOnce() + Send + 'static,
) -> JoinHandle<bool> {
    thread::spawn(move || {
        let seen = wait_for_stderr(&path, needle, Duration::from_secs(120));
        if seen {
            action();
        }
        seen
    })
}

// ---- fault agent client -----------------------------------------------------

/// Runs one whitelisted action on the host through the fault agent and waits for it.
fn fault(env: &Env, action: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let id = format!("{nanos:020}-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let tmp = env.faults.join(format!("req-{id}.tmp"));
    fs::write(&tmp, action).unwrap();
    fs::rename(&tmp, env.faults.join(format!("req-{id}"))).unwrap();
    let res = env.faults.join(format!("res-{id}"));
    let deadline = Instant::now() + Duration::from_secs(900);
    loop {
        if let Ok(text) = fs::read_to_string(&res) {
            let rc = text.lines().next().unwrap_or("");
            assert_eq!(rc, "0", "fault {action:?} failed: {text}");
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "fault agent did not answer {action:?}"
        );
        thread::sleep(Duration::from_millis(200));
    }
}

/// Holds a row lock on the replica, so the replica's applier blocks on that row. This
/// makes the replica lag deterministically: the update below is in L0, not yet applied.
struct ReplicaLock {
    release: Option<mpsc::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl ReplicaLock {
    fn acquire(env: &Env, id: u64) -> Self {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let env = env.clone();
        let handle = thread::spawn(move || {
            runtime().block_on(async move {
                let mut conn = replica_reader(&env).await;
                conn.query_drop("START TRANSACTION").await.unwrap();
                conn.query_drop(format!(
                    "SELECT id FROM cb_hdr WHERE id = {id} LOCK IN SHARE MODE"
                ))
                .await
                .unwrap();
                ready_tx.send(()).unwrap();
                let _ = tokio::task::spawn_blocking(move || release_rx.recv()).await;
                conn.query_drop("COMMIT").await.unwrap();
                conn.disconnect().await.unwrap();
            });
        });
        ready_rx.recv().unwrap();
        Self {
            release: Some(release_tx),
            handle: Some(handle),
        }
    }

    fn release(mut self) {
        if let Some(tx) = self.release.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap();
        }
    }
}

// ---- writer on the primary --------------------------------------------------

/// One round of source writes: inserts, primary-key changes, updates, deletes, and a
/// two-table transaction. Keys start at `base`, a per-run range (`run_base`) that never
/// collides with the seed or with an earlier run's rows.
async fn write_round(conn: &mut Conn, base: u64, i: u64) {
    let base = base + i;
    conn.query_drop(format!(
        "INSERT INTO cb_hdr (id, note, status, updated_at) VALUES ({base}, 'new-{i}', 1, NOW(6))"
    ))
    .await
    .unwrap();
    conn.query_drop(format!(
        "INSERT INTO cb_line (id, hdr_id, qty, sku) VALUES ({base}, {base}, 1, 'sku-{i}')"
    ))
    .await
    .unwrap();
    if i >= 2 && i % 3 == 0 {
        // Primary-key change: the old key must disappear from the copy as well (P3).
        let old = base - 2;
        conn.query_drop(format!(
            "UPDATE cb_hdr SET id = id + 1000000 WHERE id = {old}"
        ))
        .await
        .unwrap();
    }
    if i % 5 == 0 {
        conn.query_drop(format!(
            "UPDATE cb_hdr SET note = CONCAT(note, '-u{i}') WHERE id = {}",
            HDR_SEED + i % 2000
        ))
        .await
        .unwrap();
    }
    if i >= 4 && i % 7 == 0 {
        conn.query_drop(format!("DELETE FROM cb_hdr WHERE id = {}", base - 4))
            .await
            .unwrap();
    }
    if i % 4 == 0 {
        conn.query_drop("START TRANSACTION").await.unwrap();
        conn.query_drop(format!(
            "UPDATE cb_line SET sku = CONCAT(sku, '-t') WHERE id = {}",
            LINE_SEED + i % 8000
        ))
        .await
        .unwrap();
        conn.query_drop(format!(
            "UPDATE cb_hdr SET status = status + 1 WHERE id = {}",
            HDR_SEED + i % 2000
        ))
        .await
        .unwrap();
        conn.query_drop("COMMIT").await.unwrap();
    }
}

/// Source writes in the background until stopped.
struct Writer {
    stop: Arc<std::sync::atomic::AtomicBool>,
    handle: JoinHandle<u64>,
}

/// Key range of one run. Each run gets its own range, 1e7 apart, and a key change moves a
/// row by 1e6, so runs never collide with each other.
fn run_base() -> u64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    3_000_000_000 + (secs % 100_000) * 10_000_000
}

fn start_writer(env: &Env, base: u64) -> Writer {
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let env = env.clone();
    let handle = thread::spawn(move || {
        runtime().block_on(async move {
            let mut conn = primary(&env).await;
            let mut i = 0u64;
            while !flag.load(Ordering::Relaxed) {
                write_round(&mut conn, base, i).await;
                i += 1;
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            conn.disconnect().await.unwrap();
            i
        })
    });
    Writer { stop, handle }
}

impl Writer {
    /// Stops writing and waits for the writer to finish its round. Returns the rounds.
    fn stop(self) -> u64 {
        self.stop.store(true, Ordering::Relaxed);
        self.handle.join().unwrap()
    }
}

/// Current primary and replica identities, used to pin the replica.
fn identities(env: &Env) -> (String, String) {
    runtime().block_on(async {
        let mut p = primary(env).await;
        let mut r = replica_reader(env).await;
        (server_uuid(&mut p).await, server_uuid(&mut r).await)
    })
}

/// Restores the replica after a test: SQL thread running and no filter. Best effort.
fn restore_replica(env: &Env) {
    fault(env, "start-sql");
    fault(env, "filter-clear");
}

fn assert_primary_equal(model: &Model, env: &Env) {
    runtime().block_on(async {
        let mut p = primary(env).await;
        for table in [HDR, LINE] {
            let source = table_state(&mut p, table).await;
            let copy = model.state(table);
            assert_eq!(
                copy.len(),
                source.len(),
                "{table}: row count equals the primary"
            );
            assert!(
                copy == source,
                "{table}: final state equals the primary at L_end"
            );
        }
    });
}

fn out_dir(env: &Env, name: &str) -> (PathBuf, PathBuf) {
    let dir = env.work.join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    (dir.clone(), dir.join("capture"))
}

// ---- R1: reads from the replica under concurrent PK-changing writes ---------

/// Runs the concurrent-writes scenario with one read source. Writes run during the cb_hdr
/// chunk reads and stop before the cb_line chunks, so the primary's final state is the
/// state at L_end. Returns the model, the receipt, the rounds written and the run's base key.
fn concurrent_writes_run(env: &Env, name: &str, replica: bool) -> (Model, Value, u64, u64) {
    let (dir, out) = out_dir(env, name);
    let replica_id = if replica {
        Some(identities(env).1)
    } else {
        None
    };
    let config = write_config(env, &dir, replica);
    let mut run = spawn(&config, &out, &dir.join("stderr.txt"));
    let boot = match &replica_id {
        Some(id) => bootstrap(Some(id), 2),
        None => bootstrap_primary(),
    };
    run.send(&boot);
    let mut model = Model::default();
    let base = run_base();
    let mut writer = Some(start_writer(env, base));
    let mut rounds = 0;
    let mut hook = |frame: &Value| {
        if frame["table"] == "cb_line" && frame["chunk_no"] == 1 {
            rounds = writer.take().map(Writer::stop).unwrap_or(0);
        }
    };
    drive(&mut run, &mut model, &out, &mut hook);
    let (status, stderr) = run.finish();
    assert!(status.success(), "producer failed: {stderr}");
    assert!(
        rounds >= 5,
        "writes ran during the chunk reads: {rounds} rounds"
    );
    assert!(
        model
            .chunks
            .iter()
            .any(|c| c["table"] == "cb_hdr" && c["chunk_no"].as_u64() > Some(1)),
        "several cb_hdr chunks"
    );
    let expected_source = if replica { "replica" } else { "primary" };
    for chunk in &model.chunks {
        assert_eq!(
            chunk["read"]["source"], expected_source,
            "every chunk names its read source"
        );
        match &replica_id {
            Some(id) => assert_eq!(
                chunk["read"]["replica_identity"],
                id.as_str(),
                "the pinned identity"
            ),
            None => assert!(
                chunk["read"]["replica_identity"].is_null(),
                "a primary read has no identity"
            ),
        }
    }
    assert!(
        model.tables[HDR]
            .events
            .iter()
            .any(|e| e.1 >= (base + 1_000_000) as i128),
        "primary-key changes reached the producer"
    );
    assert_primary_equal(&model, env);
    let receipt = receipt(&out);
    assert_eq!(receipt["read_source"], expected_source);
    match &replica_id {
        Some(id) => assert_eq!(receipt["replica"]["identity"], id.as_str()),
        None => assert!(
            receipt["replica"].is_null(),
            "no replica block for a primary read"
        ),
    }
    println!(
        "{name} rounds={rounds} chunks={} batches={} cdc_rows={} waits={}",
        model.chunks.len(),
        model.batches,
        model.cdc_rows,
        receipt["replica"]["wait_count"],
    );
    fs::remove_dir_all(&dir).unwrap();
    (model, receipt, rounds, base)
}

/// Bootstrap for the primary read source. No replica block is sent.
fn bootstrap_primary() -> Value {
    json!({
        "protocol": "oo.jobs.mysql-c1-bootstrap/v2",
        "source_id": "mysql_t4e2e",
        "source_generation": 1,
        "baseline": {"format": "chunked-typed/v2", "chunk_rows": CHUNK_ROWS, "catchup_stall_ms": 120000},
        "read_source": {"kind": "primary"},
        "resume": null,
        "previous": null,
    })
}

/// DESIGN R1. The chunks come from the replica while the primary takes inserts, key
/// changes, updates, deletes and multi-table transactions. The final copy equals the
/// primary, and every chunk names the replica identity it was read from.
#[test]
#[ignore = "needs the owned T12 fixture and the fault agent; see the file header"]
fn r1_chunks_from_the_replica_under_pk_changing_writes_equal_the_primary() {
    let env = env();
    concurrent_writes_run(&env, "r1", true);
}

/// Regression for the primary path, which shares the catch-up watchdog and the chunk
/// source wrapper with the replica path. The same writes and the same final-state check.
#[test]
#[ignore = "needs the owned T12 fixture and the fault agent; see the file header"]
fn primary_read_source_under_pk_changing_writes_equal_the_primary() {
    let env = env();
    concurrent_writes_run(&env, "primary-reads", false);
}

// ---- R2: lag -----------------------------------------------------------------

/// DESIGN R2 (lag clears). A row lock on the replica holds its applier while the primary
/// commits an update. The update is inside L0, so WAIT(L0) cannot pass. The producer
/// retries with `replica_lag`. The lock is released, and the run completes.
#[test]
#[ignore = "needs the owned T12 fixture and the fault agent; see the file header"]
fn r2_lag_retries_with_replica_lag_and_resumes_when_it_clears() {
    let env = env();
    let (dir, out) = out_dir(&env, "r2-clears");
    let (_, replica_id) = identities(&env);
    let base = run_base();
    let lock = ReplicaLock::acquire(&env, HDR_SEED);
    runtime().block_on(async {
        let mut p = primary(&env).await;
        p.query_drop(format!(
            "UPDATE cb_hdr SET note = 'lagged-{base}' WHERE id = {HDR_SEED}"
        ))
        .await
        .unwrap();
    });
    let config = write_config(&env, &dir, true);
    let stderr = dir.join("stderr.txt");
    let mut run = spawn(&config, &out, &stderr);
    run.send(&bootstrap(Some(&replica_id), 2));
    // Once the producer is retrying with replica_lag, clear the lag.
    let helper = when_stderr(stderr.clone(), "reason=replica_lag", move || lock.release());
    let mut model = Model::default();
    drive(&mut run, &mut model, &out, &mut |_| {});
    let retrying = helper.join().unwrap();
    let (status, text) = run.finish();
    assert!(retrying, "the producer waits with replica_lag: {text}");
    assert!(
        status.success(),
        "producer failed after the lag cleared: {text}"
    );
    assert_primary_equal(&model, &env);
    assert!(
        receipt(&out)["replica"]["wait_count"].as_u64().unwrap() >= 1,
        "the waits are recorded"
    );
    fs::remove_dir_all(&dir).unwrap();
}

/// DESIGN R2 (budget spent). The same lag lasts past the retry budget. The failure is
/// retryable: `replica_lag` (the threads are running and L0 is delivered, so not a stall).
#[test]
#[ignore = "needs the owned T12 fixture and the fault agent; see the file header"]
fn r2_lag_past_the_budget_fails_retryably_with_replica_lag() {
    let env = env();
    let (dir, out) = out_dir(&env, "r2-budget");
    let (_, replica_id) = identities(&env);
    let base = run_base();
    let lock = ReplicaLock::acquire(&env, HDR_SEED);
    runtime().block_on(async {
        let mut p = primary(&env).await;
        p.query_drop(format!(
            "UPDATE cb_hdr SET note = 'lagged-{base}' WHERE id = {HDR_SEED}"
        ))
        .await
        .unwrap();
    });
    let mut config: Value =
        serde_json::from_slice(&fs::read(write_config(&env, &dir, true)).unwrap()).unwrap();
    config["read_replica"]["retry_budget_s"] = json!(6);
    let config_path = dir.join("producer-config.json");
    fs::write(&config_path, config.to_string()).unwrap();
    let mut run = spawn(&config_path, &out, &dir.join("stderr.txt"));
    run.send(&bootstrap(Some(&replica_id), 2));
    let mut model = Model::default();
    drive(&mut run, &mut model, &out, &mut |_| {});
    let (status, text) = run.finish();
    lock.release();
    assert_eq!(status.code(), Some(1), "{text}");
    assert!(text.contains("c1 replica_lag"), "{text}");
    assert!(
        !text.contains("replica_stalled"),
        "a delivered L0 is a lag, not a stall: {text}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

/// DESIGN R2 (SQL thread stopped, budget spent). `replica_stopped` is retryable.
#[test]
#[ignore = "needs the owned T12 fixture and the fault agent; see the file header"]
fn r2_stopped_sql_thread_fails_retryably_when_the_budget_ends() {
    let env = env();
    let (dir, out) = out_dir(&env, "r2-stopped");
    let (_, replica_id) = identities(&env);
    fault(&env, "stop-sql");
    let mut config: Value =
        serde_json::from_slice(&fs::read(write_config(&env, &dir, true)).unwrap()).unwrap();
    config["read_replica"]["retry_budget_s"] = json!(5);
    let config_path = dir.join("producer-config.json");
    fs::write(&config_path, config.to_string()).unwrap();
    let mut run = spawn(&config_path, &out, &dir.join("stderr.txt"));
    run.send(&bootstrap(Some(&replica_id), 2));
    let mut model = Model::default();
    drive(&mut run, &mut model, &out, &mut |_| {});
    let (status, text) = run.finish();
    restore_replica(&env);
    assert_eq!(status.code(), Some(1), "{text}");
    assert!(text.contains("c1 replica_stopped"), "{text}");
    fs::remove_dir_all(&dir).unwrap();
}

/// DESIGN R2 (SQL thread stopped, then started). The producer waits, the thread starts,
/// and the run completes with the final state equal to the primary.
#[test]
#[ignore = "needs the owned T12 fixture and the fault agent; see the file header"]
fn r2_stopped_sql_thread_resumes_after_start() {
    let env = env();
    let (dir, out) = out_dir(&env, "r2-resume");
    let (_, replica_id) = identities(&env);
    fault(&env, "stop-sql");
    let config = write_config(&env, &dir, true);
    let stderr = dir.join("stderr.txt");
    let mut run = spawn(&config, &out, &stderr);
    run.send(&bootstrap(Some(&replica_id), 2));
    let start = env.clone();
    let helper = when_stderr(stderr.clone(), "reason=replica_stopped", move || {
        fault(&start, "start-sql");
    });
    let mut model = Model::default();
    drive(&mut run, &mut model, &out, &mut |_| {});
    let waiting = helper.join().unwrap();
    let (status, text) = run.finish();
    assert!(
        waiting,
        "the producer waits while the thread is stopped: {text}"
    );
    assert!(status.success(), "producer failed after start: {text}");
    assert_primary_equal(&model, &env);
    fs::remove_dir_all(&dir).unwrap();
}

// ---- R3: replication interruption and replica restart ----------------------

/// DESIGN R3. The replica container restarts while the producer is reading. The
/// connection drops, the producer reconnects within the budget, and the identity is
/// the same after the restart, so the run completes.
#[test]
#[ignore = "needs the owned T12 fixture and the fault agent; see the file header"]
fn r3_replica_restart_mid_run_is_retried_and_completes() {
    let env = env();
    let (dir, out) = out_dir(&env, "r3-restart");
    let (_, replica_id) = identities(&env);
    let config = write_config(&env, &dir, true);
    let stderr = dir.join("stderr.txt");
    let mut run = spawn(&config, &out, &stderr);
    run.send(&bootstrap(Some(&replica_id), 2));
    let mut model = Model::default();
    let mut restarted = false;
    let mut hook = |frame: &Value| {
        if !restarted && frame["table"] == "cb_hdr" && frame["chunk_no"] == 2 {
            restarted = true;
            fault(&env, "restart");
        }
    };
    drive(&mut run, &mut model, &out, &mut hook);
    let (status, text) = run.finish();
    assert!(restarted, "the restart happened mid-run");
    assert!(
        status.success(),
        "producer failed across the restart: {text}"
    );
    assert!(
        text.contains("replica retry"),
        "the producer retried while the replica was down: {text}"
    );
    assert_primary_equal(&model, &env);
    fs::remove_dir_all(&dir).unwrap();
}

/// DESIGN R3 (replication interrupted). The IO thread stops and starts again.
#[test]
#[ignore = "needs the owned T12 fixture and the fault agent; see the file header"]
fn r3_stopped_io_thread_resumes_after_start() {
    let env = env();
    let (dir, out) = out_dir(&env, "r3-io");
    let (_, replica_id) = identities(&env);
    fault(&env, "stop-io");
    let config = write_config(&env, &dir, true);
    let stderr = dir.join("stderr.txt");
    let mut run = spawn(&config, &out, &stderr);
    run.send(&bootstrap(Some(&replica_id), 2));
    let start = env.clone();
    let helper = when_stderr(stderr.clone(), "reason=replica_stopped", move || {
        fault(&start, "start-io");
    });
    let mut model = Model::default();
    drive(&mut run, &mut model, &out, &mut |_| {});
    let waiting = helper.join().unwrap();
    let (status, text) = run.finish();
    assert!(
        waiting,
        "the producer waits while the IO thread is stopped: {text}"
    );
    assert!(
        status.success(),
        "producer failed after the IO thread restarted: {text}"
    );
    assert_primary_equal(&model, &env);
    fs::remove_dir_all(&dir).unwrap();
}

// ---- R4: replication filters -----------------------------------------------

/// DESIGN R4. A filter that skips a selected table is refused before any chunk read.
/// Nothing is written.
#[test]
#[ignore = "needs the owned T12 fixture and the fault agent; see the file header"]
fn r4_ignored_selected_table_is_refused_before_any_read() {
    let env = env();
    let (dir, out) = out_dir(&env, "r4-preflight");
    let (_, replica_id) = identities(&env);
    fault(&env, "filter-ignore oo_c1.cb_hdr");
    let config = write_config(&env, &dir, true);
    let mut run = spawn(&config, &out, &dir.join("stderr.txt"));
    run.send(&bootstrap(Some(&replica_id), 2));
    let (status, text) = {
        let mut model = Model::default();
        drive(&mut run, &mut model, &out, &mut |_| {});
        run.finish()
    };
    restore_replica(&env);
    assert_eq!(status.code(), Some(1), "{text}");
    assert!(text.contains("c1 replica_filter"), "{text}");
    assert!(
        !out.exists(),
        "nothing is written when the source is refused"
    );
    fs::remove_dir_all(&dir).unwrap();
}

/// DESIGN R4 (mid-run). A filter added during the run is refused at the next chunk.
#[test]
#[ignore = "needs the owned T12 fixture and the fault agent; see the file header"]
fn r4_filter_added_mid_run_is_refused_at_the_next_chunk() {
    let env = env();
    let (dir, out) = out_dir(&env, "r4-midrun");
    let (_, replica_id) = identities(&env);
    let config = write_config(&env, &dir, true);
    let mut run = spawn(&config, &out, &dir.join("stderr.txt"));
    run.send(&bootstrap(Some(&replica_id), 2));
    let mut model = Model::default();
    let mut filtered = false;
    let mut hook = |frame: &Value| {
        if !filtered && frame["table"] == "cb_hdr" && frame["chunk_no"] == 2 {
            filtered = true;
            fault(&env, "filter-ignore oo_c1.cb_hdr");
        }
    };
    drive(&mut run, &mut model, &out, &mut hook);
    let (status, text) = run.finish();
    restore_replica(&env);
    assert!(filtered, "the filter was added mid-run");
    assert_eq!(status.code(), Some(1), "{text}");
    assert!(text.contains("c1 replica_filter"), "{text}");
    fs::remove_dir_all(&dir).unwrap();
}

// ---- R5: identity ------------------------------------------------------------

/// DESIGN R5 (identity pinned at open). A bootstrap that pins another replica is refused
/// before any chunk read.
#[test]
#[ignore = "needs the owned T12 fixture and the fault agent; see the file header"]
fn r5_pinned_identity_that_does_not_match_is_refused_at_open() {
    let env = env();
    let (dir, out) = out_dir(&env, "r5-pin");
    let config = write_config(&env, &dir, true);
    let mut run = spawn(&config, &out, &dir.join("stderr.txt"));
    run.send(&bootstrap(Some("11111111-2222-3333-4444-555555555555"), 2));
    let mut model = Model::default();
    drive(&mut run, &mut model, &out, &mut |_| {});
    let (status, text) = run.finish();
    assert_eq!(status.code(), Some(1), "{text}");
    assert!(text.contains("c1 replica_identity"), "{text}");
    assert!(
        !out.exists(),
        "nothing is written when the identity is refused"
    );
    fs::remove_dir_all(&dir).unwrap();
}

/// DESIGN R5 (replica replaced mid-run). The replica gets a new server_uuid while the
/// producer reads. The next attempt refuses with `replica_identity`.
#[test]
#[ignore = "needs the owned T12 fixture and the fault agent; see the file header"]
fn r5_replica_identity_change_mid_run_is_refused() {
    let env = env();
    let (dir, out) = out_dir(&env, "r5-reset");
    let (_, replica_id) = identities(&env);
    let config = write_config(&env, &dir, true);
    let mut run = spawn(&config, &out, &dir.join("stderr.txt"));
    run.send(&bootstrap(Some(&replica_id), 2));
    let mut model = Model::default();
    let mut reset = false;
    let mut hook = |frame: &Value| {
        if !reset && frame["table"] == "cb_hdr" && frame["chunk_no"] == 2 {
            reset = true;
            fault(&env, "reset-identity");
        }
    };
    drive(&mut run, &mut model, &out, &mut hook);
    let (status, text) = run.finish();
    assert!(reset, "the identity was reset mid-run");
    assert_eq!(status.code(), Some(1), "{text}");
    assert!(text.contains("c1 replica_identity"), "{text}");
    fs::remove_dir_all(&dir).unwrap();
}
