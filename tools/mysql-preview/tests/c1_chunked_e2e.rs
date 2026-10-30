//! End-to-end tests for `c1-baseline-stream-v2` against an owned MySQL 5.7 fixture.
//!
//! These tests need the fixture and are `#[ignore]` by default. They run with
//! `cargo test --test c1_chunked_e2e -- --ignored --test-threads=1`, with the
//! environment described in `fixture()`. The fake worker below plays the worker
//! side of FRAMES-V2.md. It acknowledges every frame only after it has read the
//! part back through `oo_pipeline::c1_parts`, and it keeps the model state that a
//! real worker would keep in the target.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder};
use oo_pipeline::c1_parts::{PartEntry, PartKind, PartManifest, read_part};
use oo_pipeline::native_arrow::NativeCell;
use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_mysql-preview");
const DB: &str = "oo_cb_t3";
const BIG_ROWS: u64 = 65_536;

struct Fixture {
    host: String,
    port: u16,
    ca: PathBuf,
    cdc_pw: PathBuf,
    writer_pw: PathBuf,
    tls_name: String,
    work: PathBuf,
}

fn fixture() -> Fixture {
    let get = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {name}"));
    let work = std::env::temp_dir().join(format!("oo-cb-T3-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    Fixture {
        host: get("OO_CB_T3_HOST"),
        port: get("OO_CB_T3_PORT").parse().unwrap(),
        ca: PathBuf::from(get("OO_CB_T3_CA")),
        cdc_pw: PathBuf::from(get("OO_CB_T3_CDC_PW_FILE")),
        writer_pw: PathBuf::from(get("OO_CB_T3_WRITER_PW_FILE")),
        tls_name: get("OO_CB_T3_TLS_NAME"),
        work,
    }
}

fn read_pw(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap().trim().to_owned()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()
}

/// Writer connection (plain TCP, as the owned fixture allows). Used only for the
/// fixture DDL, the source DML and the final source read.
async fn writer(fx: &Fixture) -> Conn {
    let opts = OptsBuilder::default()
        .ip_or_hostname(fx.host.clone())
        .tcp_port(fx.port)
        .user(Some("oo_writer".to_owned()))
        .pass(Some(read_pw(&fx.writer_pw)))
        .db_name(Some(DB.to_owned()))
        .prefer_socket(false);
    Conn::new(opts).await.unwrap()
}

fn payload_of(id: u64) -> String {
    format!("{id:012}{}{}", "é".repeat(600), "x".repeat(380))
}

/// A table a quarter the size of `big`, for the memory comparison.
async fn build_mid(conn: &mut Conn) {
    conn.query_drop(
        "CREATE TABLE mid (id BIGINT UNSIGNED NOT NULL PRIMARY KEY, payload VARCHAR(1000) CHARACTER SET utf8mb4 NOT NULL) ENGINE=InnoDB",
    )
    .await
    .unwrap();
    conn.query_drop(format!("INSERT INTO mid VALUES (1, '{}')", payload_of(1))).await.unwrap();
    let mut count = 1u64;
    while count < BIG_ROWS / 4 {
        conn.query_drop(format!(
            "INSERT INTO mid (id, payload) SELECT id + {count}, CONCAT(LPAD(id + {count}, 12, '0'), SUBSTRING(payload, 13)) FROM mid"
        ))
        .await
        .unwrap();
        count *= 2;
    }
}

/// Creates `big` with `BIG_ROWS` rows of about 1.6 KB, and `small` with 5 rows.
async fn build_fixture(conn: &mut Conn) {
    conn.query_drop(format!("DROP DATABASE IF EXISTS {DB}")).await.unwrap();
    conn.query_drop(format!("CREATE DATABASE {DB} CHARACTER SET utf8mb4 COLLATE utf8mb4_bin"))
        .await
        .unwrap();
    conn.query_drop(format!("USE {DB}")).await.unwrap();
    conn.query_drop(
        "CREATE TABLE big (id BIGINT UNSIGNED NOT NULL PRIMARY KEY, payload VARCHAR(1000) CHARACTER SET utf8mb4 NOT NULL) ENGINE=InnoDB",
    )
    .await
    .unwrap();
    conn.query_drop(format!("INSERT INTO big VALUES (1, '{}')", payload_of(1)))
        .await
        .unwrap();
    let mut count = 1u64;
    while count < BIG_ROWS {
        conn.query_drop(format!(
            "INSERT INTO big (id, payload) SELECT id + {count}, CONCAT(LPAD(id + {count}, 12, '0'), SUBSTRING(payload, 13)) FROM big"
        ))
        .await
        .unwrap();
        count *= 2;
    }
    conn.query_drop(
        "CREATE TABLE small (id INT NOT NULL PRIMARY KEY, note VARCHAR(64) CHARACTER SET utf8mb4 NOT NULL) ENGINE=InnoDB",
    )
    .await
    .unwrap();
    conn.query_drop("INSERT INTO small VALUES (1,'a'),(2,'é'),(3,'c'),(4,'d'),(5,'e')")
        .await
        .unwrap();
    build_mid(conn).await;
}

async fn count_big(conn: &mut Conn) -> u64 {
    conn.query_first::<u64, _>("SELECT COUNT(*) FROM big").await.unwrap().unwrap()
}

/// Producer config. `tls_server_name` is optional (ONBOARDING-API §5).
fn write_config(fx: &Fixture, dir: &Path, tables: &[&str], host: &str, tls: Option<&str>) -> PathBuf {
    let mut columns = serde_json::Map::new();
    columns.insert("oo_cb_t3.big".into(), json!(["id", "payload"]));
    columns.insert("oo_cb_t3.small".into(), json!(["id", "note"]));
    columns.insert("oo_cb_t3.mid".into(), json!(["id", "payload"]));
    let columns: serde_json::Map<String, Value> = columns
        .into_iter()
        .filter(|(k, _)| tables.contains(&k.as_str()))
        .collect();
    let mut config = json!({
        "database": DB,
        "host": host,
        "port": fx.port,
        "user": "oo_cdc",
        "password_file": fx.cdc_pw,
        "dba_user": "oo_cdc",
        "dba_password_file": fx.cdc_pw,
        "ca_file": fx.ca,
        "server_id": 4242,
        "health_every_ms": 500,
        "reconcile_every_ms": 1000,
        "preserve_dir": dir,
        "fixture": "owned-mysql-57",
        "tables": tables,
        "columns": columns,
        "secret_ref": "secret://mysql-owned-fixture",
    });
    if let Some(name) = tls {
        config["tls_server_name"] = json!(name);
    }
    let path = dir.join("producer-config.json");
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(&path, config.to_string()).unwrap();
    path
}

/// Peak RSS of the producer process, sampled from /proc while it runs.
struct PeakRss {
    peak_kb: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

impl PeakRss {
    fn start(pid: u32) -> Self {
        let peak_kb = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (p, s) = (Arc::clone(&peak_kb), Arc::clone(&stop));
        std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                if let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) {
                    for line in status.lines() {
                        if let Some(rest) = line.strip_prefix("VmHWM:") {
                            let kb: u64 = rest.trim().trim_end_matches("kB").trim().parse().unwrap_or(0);
                            p.fetch_max(kb, Ordering::Relaxed);
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        Self { peak_kb, stop }
    }

    fn finish(self) -> u64 {
        self.stop.store(true, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(40));
        self.peak_kb.load(Ordering::Relaxed)
    }
}

struct Run {
    child: Child,
    stdin: ChildStdin,
    frames: mpsc::Receiver<Result<String, String>>,
    rss: PeakRss,
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
        .stderr(std::fs::File::create(stderr).unwrap())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(text) => {
                    if tx.send(Ok(text)).is_err() {
                        return;
                    }
                }
                Err(err) => {
                    let _ = tx.send(Err(err.to_string()));
                    return;
                }
            }
        }
    });
    let rss = PeakRss::start(child.id());
    Run {
        child,
        stdin,
        frames: rx,
        rss,
        stderr: stderr.to_path_buf(),
    }
}

impl Run {
    fn send(&mut self, value: &Value) {
        writeln!(self.stdin, "{value}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Next frame. A producer that stays silent for 120 s fails the test.
    fn next(&mut self) -> Option<Value> {
        match self.frames.recv_timeout(Duration::from_secs(120)) {
            Ok(Ok(line)) => Some(serde_json::from_str(&line).expect("frame is JSON")),
            Ok(Err(err)) => panic!("stdout read failed: {err}"),
            Err(mpsc::RecvTimeoutError::Disconnected) => None,
            Err(mpsc::RecvTimeoutError::Timeout) => panic!("producer silent for 120 s"),
        }
    }

    fn ack(&mut self, stage: &str, extra: Value) {
        let mut ack = json!({"protocol": "oo.jobs.mysql-c1-baseline-ack/v2", "stage": stage, "durable": true});
        if let (Some(map), Some(extra)) = (ack.as_object_mut(), extra.as_object()) {
            for (k, v) in extra {
                map.insert(k.clone(), v.clone());
            }
        }
        self.send(&ack);
    }

    fn finish(mut self) -> (std::process::ExitStatus, u64, String) {
        let status = self.child.wait().unwrap();
        let peak = self.rss.finish();
        let stderr = std::fs::read_to_string(&self.stderr).unwrap_or_default();
        (status, peak, stderr)
    }
}

/// What the fake worker has durably acknowledged. It is the worker's state, and a
/// restart builds `resume` from it.
#[derive(Default)]
struct WorkerState {
    l0: String,
    l0_pos: u64,
    applied: u64,
    tables: BTreeMap<String, TableState>,
    /// Acknowledged fill rows: key -> payload.
    fill: BTreeMap<i128, String>,
    /// Acknowledged CDC events in position order: (pos, key, delete, payload).
    events: Vec<(u64, i128, bool, Option<String>)>,
    fill_frames: Vec<Value>,
    batch_frames: Vec<Value>,
    txn_count: u64,
    cdc_rows: u64,
}

#[derive(Default, Clone)]
struct TableState {
    last_chunk_no: u64,
    last_key_hi: Option<i128>,
    terminal: bool,
    rows: u64,
    avg: u64,
}

impl WorkerState {
    fn resume(&self) -> Value {
        let tables: Vec<Value> = self
            .tables
            .iter()
            .map(|(name, t)| {
                let (schema, table) = name.split_once('.').unwrap();
                json!({"schema": schema, "table": table, "last_chunk_no": t.last_chunk_no,
                    "last_key_hi": t.last_key_hi.map(|k| k.to_string()), "terminal": t.terminal,
                    "rows": t.rows, "avg_row_bytes": t.avg})
            })
            .collect();
        json!({"state": "open", "l0": self.l0, "l0_pos": self.l0_pos.to_string(),
            "applied_pos": self.applied.to_string(), "tables": tables})
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

/// Reads and checks one part exactly as the worker would before acknowledging it.
fn check_part(out: &Path, table: &str, entry: &PartEntry) -> oo_pipeline::native_arrow::DecodedStream {
    let dir = out.join("parts").join(table);
    read_part(&dir, entry).unwrap_or_else(|err| panic!("part {table} {} rejected: {err}", entry.part_no))
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

/// Plays the worker against one producer run. `hook` runs source writes at chosen
/// points. `kill_after` is the chunk number whose frame arrives next but is never
/// acknowledged: the producer is killed while that frame is in flight.
struct Playbook<'a> {
    fx: &'a Fixture,
    hook: &'a mut dyn FnMut(u32, &mut Conn, &tokio::runtime::Runtime),
    kill_when_sent: Option<u32>,
}

enum Ended {
    Completed,
    Killed,
}

fn play(run: &mut Run, state: &mut WorkerState, out: &Path, book: &mut Playbook, rt: &tokio::runtime::Runtime) -> Ended {
    let mut conn = rt.block_on(writer(book.fx));
    loop {
        let Some(frame) = run.next() else {
            return Ended::Completed;
        };
        match frame["kind"].as_str().unwrap_or("") {
            "open" => {
                state.l0 = frame["l0"].as_str().unwrap().to_owned();
                state.l0_pos = frame["l0_pos"].as_str().unwrap().parse().unwrap();
                if state.applied == 0 {
                    state.applied = state.l0_pos;
                }
                run.ack("stage_open", json!({"source_epoch": 1}));
            }
            "stream_started" => {
                assert_eq!(frame["applied_pos"].as_str().unwrap(), state.applied.to_string());
                run.ack("stream_started", json!({}));
            }
            "chunk" => {
                let chunk_no = frame["chunk_no"].as_u64().unwrap() as u32;
                let table = format!("{}.{}", frame["schema"].as_str().unwrap(), frame["table"].as_str().unwrap());
                let entry = part_entry(&frame, PartKind::Fill, chunk_no);
                if book.kill_when_sent == Some(chunk_no) {
                    // Frame received, never acknowledged. The producer is killed now.
                    let _ = run.child.kill();
                    return Ended::Killed;
                }
                let stream = check_part(out, &table, &entry);
                assert_eq!(stream.rows as u64, entry.rows, "row count in part");
                let st = state.tables.entry(table.clone()).or_default();
                assert_eq!(u64::from(chunk_no), st.last_chunk_no + 1, "chunk numbers are contiguous");
                if !frame["terminal"].as_bool().unwrap() {
                    let lo = frame["key_lo"].as_str().unwrap().parse::<i128>().unwrap();
                    let hi = frame["key_hi"].as_str().unwrap().parse::<i128>().unwrap();
                    assert!(lo <= hi);
                    if let Some(prev) = st.last_key_hi {
                        assert!(lo > prev, "chunk {chunk_no} overlaps the previous chunk: {lo} <= {prev}");
                    }
                    let keycol = stream.fields.iter().position(|f| f.name == "id").unwrap();
                    let payload_col = stream.fields.iter().position(|f| f.name == "payload" || f.name == "note").unwrap();
                    for row in 0..stream.rows {
                        let key = key_of(&stream.columns[keycol][row]);
                        assert!(key >= lo && key <= hi, "key {key} outside the chunk range");
                        state.fill.insert(key, text_of(&stream.columns[payload_col][row]).unwrap());
                    }
                    st.last_key_hi = Some(hi);
                }
                st.last_chunk_no = u64::from(chunk_no);
                st.terminal = frame["terminal"].as_bool().unwrap();
                st.rows += entry.rows;
                if entry.rows > 0 {
                    st.avg = (frame["bytes"].as_u64().unwrap() / entry.rows).max(1);
                }
                state.fill_frames.push(frame.clone());
                run.ack("chunk_apply", json!({"chunk_no": chunk_no, "outcome": "loaded",
                    "schema": frame["schema"], "table": frame["table"]}));
                (book.hook)(chunk_no, &mut conn, rt);
            }
            "txn_batch" => {
                let batch_no = frame["batch_no"].as_u64().unwrap();
                let first: u64 = frame["first_pos"].as_str().unwrap().parse().unwrap();
                let last: u64 = frame["last_pos"].as_str().unwrap().parse().unwrap();
                assert_eq!(first, state.applied + 1, "batch chain has no gap");
                let mut parts_rows = 0u64;
                for part in frame["parts"].as_array().unwrap() {
                    let table = format!("{}.{}", part["schema"].as_str().unwrap(), part["table"].as_str().unwrap());
                    let entry = part_entry(part, PartKind::Cdc, part["part_no"].as_u64().unwrap() as u32);
                    let stream = check_part(out, &table, &entry);
                    parts_rows += entry.rows;
                    let n = stream.fields.len();
                    let pos_col = n - 2;
                    let op_col = n - 1;
                    let keycol = stream.fields.iter().position(|f| f.name == "id").unwrap();
                    let payload_col = stream.fields.iter().position(|f| f.name == "payload" || f.name == "note").unwrap();
                    for row in 0..stream.rows {
                        let pos = match &stream.columns[pos_col][row] {
                            NativeCell::Decimal { coefficient, .. } => *coefficient as u64,
                            other => panic!("pos cell {other:?}"),
                        };
                        assert!(pos >= first && pos <= last, "row position {pos} outside the batch");
                        let op = match &stream.columns[op_col][row] {
                            NativeCell::Utf8(t) => t.clone(),
                            other => panic!("op cell {other:?}"),
                        };
                        let key = key_of(&stream.columns[keycol][row]);
                        let payload = if op == "delete" { None } else { text_of(&stream.columns[payload_col][row]) };
                        state.events.push((pos, key, op == "delete", payload));
                    }
                }
                state.cdc_rows += parts_rows;
                // Count transactions per batch from the batch record.
                let txns = frame["txns"].as_array().unwrap();
                assert_eq!(txns.len() as u64, last - first + 1, "every position has one transaction record");
                state.txn_count += txns.len() as u64;
                state.applied = last;
                state.batch_frames.push(frame.clone());
                run.ack("txn_batch", json!({"batch_no": batch_no, "last_pos": last.to_string(), "outcome": "applied"}));
            }
            "end" => {
                let manifest = out.join(frame["parts_manifest"].as_str().unwrap());
                let bytes = std::fs::read(&manifest).unwrap();
                let sha = frame["parts_manifest_sha256"].as_str().unwrap();
                assert_eq!(oo_pipeline::target_installer::sha256_hex(&bytes), sha);
                let doc: Value = serde_json::from_slice(&bytes).unwrap();
                for table in doc["tables"].as_array().unwrap() {
                    PartManifest::from_json(&table["manifest"]).expect("per-table manifest validates");
                }
                assert_eq!(frame["applied_pos"].as_str().unwrap(), state.applied.to_string());
                run.ack("end", json!({"outcome": "applied"}));
                return Ended::Completed;
            }
            other => panic!("unexpected frame kind {other:?}: {frame}"),
        }
    }
}

/// Source state after all writes, from the writer connection.
fn source_state(rt: &tokio::runtime::Runtime, fx: &Fixture) -> BTreeMap<i128, String> {
    rt.block_on(async {
        let mut conn = writer(fx).await;
        let rows: Vec<(u64, String)> = conn
            .query("SELECT id, payload FROM big")
            .await
            .unwrap();
        rows.into_iter().map(|(id, p)| (i128::from(id), p)).collect()
    })
}

/// DESIGN §3.1 applied to the frames: the last event by position wins. Fill rows
/// have position 0.
fn model(state: &WorkerState) -> BTreeMap<i128, String> {
    let mut events = state.events.clone();
    events.sort_by_key(|e| e.0);
    let mut out = state.fill.clone();
    for (_, key, delete, payload) in events {
        if delete {
            out.remove(&key);
        } else {
            out.insert(key, payload.expect("upsert has a payload"));
        }
    }
    out
}

fn source_writes_after_chunk(chunk: u32, conn: &mut Conn, rt: &tokio::runtime::Runtime) {
    rt.block_on(async {
        match chunk {
            3 => {
                conn.query_drop("INSERT INTO big VALUES (1000001,'new-1'),(1000002,'new-2'),(1000003,'new-3')").await.unwrap();
                conn.query_drop("UPDATE big SET payload = CONCAT('u', payload) WHERE id BETWEEN 100 AND 119").await.unwrap();
                conn.query_drop("DELETE FROM big WHERE id BETWEEN 200 AND 209").await.unwrap();
                // Key change: the old keys must be deleted, not left behind.
                conn.query_drop("UPDATE big SET id = id + 5000000 WHERE id BETWEEN 300 AND 304").await.unwrap();
                // One transaction with several statements, then a rollback.
                conn.query_drop("START TRANSACTION").await.unwrap();
                conn.query_drop("INSERT INTO big VALUES (1000010,'txn-a')").await.unwrap();
                conn.query_drop("UPDATE big SET payload = 'txn-b' WHERE id = 1000010").await.unwrap();
                conn.query_drop("COMMIT").await.unwrap();
                conn.query_drop("START TRANSACTION").await.unwrap();
                conn.query_drop("INSERT INTO big VALUES (1000099,'never')").await.unwrap();
                conn.query_drop("ROLLBACK").await.unwrap();
            }
            8 => {
                conn.query_drop("UPDATE big SET payload = CONCAT('v', payload) WHERE id BETWEEN 40000 AND 40029").await.unwrap();
                conn.query_drop("DELETE FROM big WHERE id BETWEEN 50000 AND 50004").await.unwrap();
                conn.query_drop("INSERT INTO big VALUES (1000020,'late-1'),(1000021,'late-2')").await.unwrap();
            }
            _ => {}
        }
    });
}

/// Full baseline of one table with no source writes. Returns the producer's peak RSS.
fn baseline_peak_rss(fx: &Fixture, rt: &tokio::runtime::Runtime, name: &str, table: &str) -> (u64, WorkerState) {
    let out = out_dir(fx, name);
    let dir = out.parent().unwrap().to_path_buf();
    let config = write_config(fx, &dir, &[table], &fx.host, Some(&fx.tls_name));
    let mut run = spawn(&config, &out, &dir.join("stderr.txt"));
    run.send(&bootstrap(None));
    let mut state = WorkerState::default();
    let mut none = |_: u32, _: &mut Conn, _: &tokio::runtime::Runtime| {};
    let mut book = Playbook { fx, hook: &mut none, kill_when_sent: None };
    assert!(matches!(play(&mut run, &mut state, &out, &mut book, rt), Ended::Completed));
    let (status, peak_kb, stderr) = run.finish();
    assert!(status.success(), "producer failed: {stderr}");
    std::fs::remove_dir_all(&dir).unwrap();
    (peak_kb, state)
}

/// Peak RSS must not grow with table size. A table four times larger does not use
/// four times the memory. Memory is set by the chunk budget (8 MiB), not the table.
#[test]
#[ignore = "needs the owned MySQL 5.7 fixture; see tools/mysql-preview/tests/c1_chunked_e2e.rs"]
fn peak_rss_does_not_scale_with_table_size() {
    let fx = fixture();
    let rt = runtime();
    rt.block_on(async { build_fixture(&mut writer(&fx).await).await });
    let (mid_kb, mid_state) = baseline_peak_rss(&fx, &rt, "rss-mid", "oo_cb_t3.mid");
    let (big_kb, big_state) = baseline_peak_rss(&fx, &rt, "rss-big", "oo_cb_t3.big");
    println!(
        "E5 peak_rss_mib mid(~25MiB)={:.1} big(~100MiB)={:.1} mid_chunks={} big_chunks={}",
        mid_kb as f64 / 1024.0,
        big_kb as f64 / 1024.0,
        mid_state.fill_frames.len(),
        big_state.fill_frames.len(),
    );
    assert!(
        big_kb < mid_kb * 2,
        "a 4x table must not need 2x the memory: mid {mid_kb} KiB, big {big_kb} KiB"
    );
}

fn bootstrap(resume: Option<Value>) -> Value {
    json!({
        "protocol": "oo.jobs.mysql-c1-bootstrap/v2",
        "source_id": "mysql_t3e2e",
        "source_generation": 1,
        "baseline": {"format": "chunked-typed/v2"},
        "read_source": {"kind": "primary"},
        "resume": resume,
        "previous": null,
    })
}

fn out_dir(fx: &Fixture, name: &str) -> PathBuf {
    let dir = fx.work.join(name);
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("capture")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// 100 MiB table, CDC during the baseline (inserts, updates, deletes, a key change,
/// a multi-statement transaction and a rollback). Bounded memory, and the final
/// state equals the source.
#[test]
#[ignore = "needs the owned MySQL 5.7 fixture; see tools/mysql-preview/tests/c1_chunked_e2e.rs"]
fn hundred_mib_table_streams_in_chunks_with_bounded_memory_and_matches_source() {
    let fx = fixture();
    let rt = runtime();
    rt.block_on(async { build_fixture(&mut writer(&fx).await).await });
    let total = rt.block_on(async { count_big(&mut writer(&fx).await).await });
    assert_eq!(total, BIG_ROWS);
    let out = out_dir(&fx, "big-run");
    let config = write_config(&fx, out.parent().unwrap(), &["oo_cb_t3.big"], &fx.host, Some(&fx.tls_name));
    let mut run = spawn(&config, &out, &out.parent().unwrap().join("stderr.txt"));
    run.send(&bootstrap(None));
    let mut state = WorkerState::default();
    let mut hook = |chunk: u32, conn: &mut Conn, rt: &tokio::runtime::Runtime| source_writes_after_chunk(chunk, conn, rt);
    let mut book = Playbook { fx: &fx, hook: &mut hook, kill_when_sent: None };
    let ended = play(&mut run, &mut state, &out, &mut book, &rt);
    assert!(matches!(ended, Ended::Completed));
    let (status, peak_kb, stderr) = run.finish();
    assert!(status.success(), "producer failed: {stderr}");

    let chunks = state.fill_frames.len();
    let table = &state.tables["oo_cb_t3.big"];
    assert!(table.terminal && chunks >= 10, "a 100 MiB table takes many chunks, got {chunks}");
    assert!(state.batch_frames.len() >= 1, "CDC batches were sent");
    let model = model(&state);
    let source = source_state(&rt, &fx);
    assert_eq!(model.len(), source.len(), "row count matches the source");
    assert!(model == source, "composed state equals the source state at L_end");
    let receipt: Value = serde_json::from_slice(&std::fs::read(out.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["chunks"].as_u64(), Some(chunks as u64));
    let mib = 1024.0 * 1024.0;
    println!(
        "E1 chunks={chunks} batches={} txns={} cdc_rows={} fill_rows={} peak_rss_mib={:.1} table_mib={:.1}",
        state.batch_frames.len(),
        state.txn_count,
        state.cdc_rows,
        table.rows,
        peak_kb as f64 / 1024.0,
        (total as f64 * 1592.0) / mib,
    );
    assert!(
        peak_kb < 100 * 1024,
        "producer peak RSS {} MiB must stay below the table size",
        peak_kb / 1024
    );
    std::fs::remove_dir_all(out.parent().unwrap()).unwrap();
}

/// Kill the producer while chunk 6's frame is in flight, then restart from the
/// worker's acknowledged state. The restart reads from chunk 6, not from the start.
#[test]
#[ignore = "needs the owned MySQL 5.7 fixture; see tools/mysql-preview/tests/c1_chunked_e2e.rs"]
fn kill_mid_table_resumes_from_the_last_acknowledged_chunk() {
    let fx = fixture();
    let rt = runtime();
    rt.block_on(async { build_fixture(&mut writer(&fx).await).await });
    let out = out_dir(&fx, "kill-run");
    let dir = out.parent().unwrap().to_path_buf();
    let config = write_config(&fx, &dir, &["oo_cb_t3.big"], &fx.host, Some(&fx.tls_name));
    let mut state = WorkerState::default();
    let mut none = |_: u32, _: &mut Conn, _: &tokio::runtime::Runtime| {};

    let mut run = spawn(&config, &out, &dir.join("stderr-1.txt"));
    run.send(&bootstrap(None));
    let mut book = Playbook { fx: &fx, hook: &mut none, kill_when_sent: Some(6) };
    assert!(matches!(play(&mut run, &mut state, &out, &mut book, &rt), Ended::Killed));
    let (status, _, _) = run.finish();
    assert!(!status.success(), "killed producer exits non-zero");
    let acked_chunks = state.tables["oo_cb_t3.big"].last_chunk_no;
    assert_eq!(acked_chunks, 5, "chunks 1..=5 were acknowledged before the kill");
    assert!(
        out.join("parts/oo_cb_t3.big/fill-00000006.arrow").exists(),
        "the in-flight chunk was journaled before its frame, so its part exists"
    );

    // Restart: the worker resumes from its own state, not from the producer's.
    let mut run = spawn(&config, &out, &dir.join("stderr-2.txt"));
    run.send(&bootstrap(Some(state.resume())));
    let mut none = |_: u32, _: &mut Conn, _: &tokio::runtime::Runtime| {};
    let mut book = Playbook { fx: &fx, hook: &mut none, kill_when_sent: None };
    let ended = play(&mut run, &mut state, &out, &mut book, &rt);
    assert!(matches!(ended, Ended::Completed));
    let (status, _, stderr) = run.finish();
    assert!(status.success(), "resumed producer failed: {stderr}");
    assert!(
        !out.join("parts/oo_cb_t3.big/fill-00000006.arrow.partial").exists(),
        "no partial file is left behind"
    );
    let source = source_state(&rt, &fx);
    assert!(model(&state) == source, "resumed state equals the source");
    let receipt: Value = serde_json::from_slice(&std::fs::read(out.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["resumed"], true);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A tls_server_name that the certificate does not carry is refused, and the
/// matching name connects. TCP goes to the same host and port both times.
#[test]
#[ignore = "needs the owned MySQL 5.7 fixture; see tools/mysql-preview/tests/c1_chunked_e2e.rs"]
fn tls_server_name_verifies_the_certificate_against_the_override() {
    let fx = fixture();
    let rt = runtime();
    rt.block_on(async { build_fixture(&mut writer(&fx).await).await });
    let dir = fx.work.join("tls-run");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Without the override, the certificate name does not match the connect host.
    let no_override = write_config(&fx, &dir.join("a"), &["oo_cb_t3.small"], &fx.host, None);
    let out_a = dir.join("a/capture");
    let mut run = spawn(&no_override, &out_a, &dir.join("a/stderr.txt"));
    run.send(&bootstrap(None));
    let (status, _, _) = run.finish();
    assert_eq!(status.code(), Some(4), "TLS verification fails with exit code 4 (connect)");

    // With the override, the same host and port verify and the baseline completes.
    let with_override = write_config(&fx, &dir.join("b"), &["oo_cb_t3.small"], &fx.host, Some(&fx.tls_name));
    let out_b = dir.join("b/capture");
    let mut run = spawn(&with_override, &out_b, &dir.join("b/stderr.txt"));
    run.send(&bootstrap(None));
    let mut state = WorkerState::default();
    let mut none = |_: u32, _: &mut Conn, _: &tokio::runtime::Runtime| {};
    let mut book = Playbook { fx: &fx, hook: &mut none, kill_when_sent: None };
    assert!(matches!(play(&mut run, &mut state, &out_b, &mut book, &rt), Ended::Completed));
    let (status, _, stderr) = run.finish();
    assert!(status.success(), "override connects: {stderr}");
    assert_eq!(state.tables["oo_cb_t3.small"].rows, 5);
    assert!(model(&state) == source_state_small(&rt, &fx));
    std::fs::remove_dir_all(&dir).unwrap();
}

fn source_state_small(rt: &tokio::runtime::Runtime, fx: &Fixture) -> BTreeMap<i128, String> {
    rt.block_on(async {
        let mut conn = writer(fx).await;
        let rows: Vec<(i32, String)> = conn.query("SELECT id, note FROM small").await.unwrap();
        rows.into_iter().map(|(id, n)| (i128::from(id), n)).collect()
    })
}

/// A replica read source needs the operator's `read_replica` block. Without it the
/// run is refused before anything is written, and nothing is read from the primary.
/// (The live replica behaviour is in `c1_replica_e2e.rs`.)
#[test]
#[ignore = "needs the owned MySQL 5.7 fixture; see tools/mysql-preview/tests/c1_chunked_e2e.rs"]
fn replica_read_source_without_operator_block_is_refused_before_any_read() {
    let fx = fixture();
    let dir = fx.work.join("replica-run");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config = write_config(&fx, &dir, &["oo_cb_t3.small"], &fx.host, Some(&fx.tls_name));
    let out = dir.join("capture");
    let mut run = spawn(&config, &out, &dir.join("stderr.txt"));
    let mut boot = bootstrap(None);
    boot["read_source"] = json!({"kind": "replica", "wait_timeout_s": 60});
    run.send(&boot);
    let (status, _, stderr) = run.finish();
    assert_eq!(status.code(), Some(3));
    assert!(stderr.contains("config read_source"), "{stderr}");
    assert!(!out.exists(), "nothing is written when the source is refused");
    std::fs::remove_dir_all(&dir).unwrap();
}
