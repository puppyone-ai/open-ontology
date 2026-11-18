//! FM4 conformance (80 §4, card V-S2-02): replay Quint ITF traces of
//! `formal/quint/event_stream.qnt` against the real event log and the real
//! `GET /v1/docs/events` server sent events.
//!
//! ```text
//! OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test conformance_events -- --ignored --nocapture
//! ```
//!
//! Default input is the committed sample set `formal/traces/event_stream/`
//! (ITF with `--mbt` metadata: the action of each step is `mbt::actionTaken`,
//! its arguments are in `mbt::nondetPicks`). Bulk run:
//!
//! ```text
//! formal/scripts/formal-traces event_stream event_stream 2000 30 0x464d /tmp/fm4-traces
//! OO_FM4_TRACES_DIR=/tmp/fm4-traces OO_FM4_PAR=4 cargo test ... (as above)
//! ```
//!
//! How a model step maps to the real system:
//! * `appendEvent(k, v)`: `store::events::append_event` of a `drive.updated`
//!   event whose payload is `{key, value}`; the test also keeps the
//!   authoritative state `truth` (what a list endpoint would return).
//! * `deliver`: read the next durable frame from the open stream; its `seq`
//!   must be `cursor + 1`; the client applies it.
//! * `duplicate(s)`: an already applied `seq` arrives again. The frame comes
//!   from a real replay (`?since=s-1`) when `s` is still retained, else from
//!   the copy the client kept; the client drops it because `s <= cursor`.
//! * `transient`: `file.progress` published on the progress bus; the frame has
//!   no `id`/`seq` and the client only counts it.
//! * `disconnect`: the stream is dropped (frames in flight are lost).
//! * `purge(n)`: retention deletes `seq <= n` (admin, the runtime role cannot).
//! * `reconnect`: `GET ?since=cursor`; `410 DOCS_ENGINE_CURSOR_EXPIRED` makes
//!   the client refetch `truth` and restart from the head, otherwise the
//!   replay resumes after the cursor.
//!
//! After each step the real state must equal the model: event rows retained,
//! head, the client's cursor, view, transient count, duplicate and refetch
//! counters, connection state and the result of the step.

mod common;

use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::engine::{Opts, TOKEN, TestEngine};
use common::http::{Sse, open_sse};
use common::*;
use oo_docs::api::progress::Progress;
use oo_docs::store::events::append_event;
use serde_json::{Value, json};
use sqlx::{Connection, Executor, PgConnection, Row};

const WAIT: Duration = Duration::from_secs(8);

fn int(v: &Value) -> i64 {
    match v {
        Value::Number(n) => n.as_i64().expect("int"),
        Value::Object(o) => o["#bigint"]
            .as_str()
            .expect("bigint")
            .parse()
            .expect("bigint"),
        other => panic!("not an int: {other}"),
    }
}

fn pairs(v: &Value) -> Vec<(i64, Value)> {
    v["#map"]
        .as_array()
        .expect("#map")
        .iter()
        .map(|p| (int(&p[0]), p[1].clone()))
        .collect()
}

/// The model variables this test compares.
#[derive(Debug, PartialEq, Eq)]
struct Model {
    hist: BTreeMap<i64, (i64, i64)>,
    head: i64,
    floor: i64,
    connected: bool,
    cursor: i64,
    view: BTreeMap<i64, i64>,
    transients: i64,
    dup_dropped: i64,
    refetches: i64,
    res: String,
}

fn parse_state(s: &Value) -> Model {
    Model {
        hist: pairs(&s["hist"])
            .into_iter()
            .map(|(k, v)| (k, (int(&v["key"]), int(&v["value"]))))
            .collect(),
        head: int(&s["headSeq"]),
        floor: int(&s["floor"]),
        connected: s["connected"].as_bool().expect("connected"),
        cursor: int(&s["cursor"]),
        view: pairs(&s["view"])
            .into_iter()
            .map(|(k, v)| (k, int(&v)))
            .collect(),
        transients: int(&s["transients"]),
        dup_dropped: int(&s["dupDropped"]),
        refetches: int(&s["refetches"]),
        res: s["lastRes"].as_str().expect("lastRes").to_owned(),
    }
}

struct Frame {
    /// `None` for a transient event.
    seq: Option<i64>,
    event: Value,
}

/// The client under test: applies durable events by seq, counts transients.
struct Client {
    addr: SocketAddr,
    stream: Option<Sse>,
    buf: VecDeque<Frame>,
    /// Copies of the durable events the client received, by seq.
    seen: BTreeMap<i64, Value>,
    cursor: i64,
    view: BTreeMap<i64, i64>,
    transients: i64,
    dup_dropped: i64,
    refetches: i64,
}

fn auth() -> [(&'static str, String); 1] {
    [("Authorization", format!("Bearer {TOKEN}"))]
}

async fn open(addr: SocketAddr, since: i64) -> Sse {
    let a = auth();
    let h = [(a[0].0, a[0].1.as_str())];
    open_sse(addr, &format!("/v1/docs/events?since={since}"), &h).await
}

impl Client {
    fn new(addr: SocketAddr, keys: &[i64]) -> Self {
        Self {
            addr,
            stream: None,
            buf: VecDeque::new(),
            seen: BTreeMap::new(),
            cursor: 0,
            view: keys.iter().map(|k| (*k, 0)).collect(),
            transients: 0,
            dup_dropped: 0,
            refetches: 0,
        }
    }

    /// Open the stream; reading the `retry:` frame proves the server has
    /// subscribed to the progress bus.
    async fn connect(&mut self, since: i64) -> Result<u16, String> {
        let mut s = open(self.addr, since).await;
        if s.status != 200 {
            let status = s.status;
            if status == 410 {
                let body = s.error_json();
                if body["code"] != "DOCS_ENGINE_CURSOR_EXPIRED" {
                    return Err(format!("410 with code {}", body["code"]));
                }
            }
            return Ok(status);
        }
        let first = s.frame(WAIT).await.ok_or("no retry frame")?;
        if !first.starts_with("retry:") {
            return Err(format!("first frame {first:?}"));
        }
        self.stream = Some(s);
        self.buf.clear();
        Ok(200)
    }

    /// Pull frames from the socket until `want` holds for one in the buffer.
    async fn pump(&mut self, want: impl Fn(&Frame) -> bool) -> Result<(), String> {
        loop {
            if self.buf.iter().any(&want) {
                return Ok(());
            }
            let s = self.stream.as_mut().ok_or("not connected")?;
            let event = s
                .event(WAIT)
                .await
                .ok_or("stream stalled while waiting for a frame")?;
            let seq = event.get("seq").and_then(Value::as_i64);
            self.buf.push_back(Frame { seq, event });
        }
    }

    /// The only apply rule: drop `seq <= cursor`, otherwise it must be the
    /// next one (gap free) and sets the key.
    fn apply(&mut self, event: &Value) -> Result<bool, String> {
        let seq = event["seq"].as_i64().ok_or("durable event without seq")?;
        if seq <= self.cursor {
            self.dup_dropped += 1;
            return Ok(false);
        }
        if seq != self.cursor + 1 {
            return Err(format!("gap: cursor {} then seq {seq}", self.cursor));
        }
        let k = event["data"]["key"].as_i64().ok_or("payload key")?;
        let v = event["data"]["value"].as_i64().ok_or("payload value")?;
        self.view.insert(k, v);
        self.cursor = seq;
        self.seen.insert(seq, event.clone());
        Ok(true)
    }

    async fn deliver(&mut self) -> Result<(), String> {
        self.pump(|f| f.seq.is_some()).await?;
        let at = self.buf.iter().position(|f| f.seq.is_some()).unwrap();
        let f = self.buf.remove(at).unwrap();
        // The live stream is in order: the next durable frame is cursor + 1.
        if f.seq != Some(self.cursor + 1) {
            return Err(format!(
                "deliver: got seq {:?}, cursor {}",
                f.seq, self.cursor
            ));
        }
        self.apply(&f.event)?;
        Ok(())
    }

    async fn transient(&mut self) -> Result<(), String> {
        self.pump(|f| f.seq.is_none()).await?;
        let at = self.buf.iter().position(|f| f.seq.is_none()).unwrap();
        let f = self.buf.remove(at).unwrap();
        if f.event["type"] != "file.progress" {
            return Err(format!("transient frame of type {}", f.event["type"]));
        }
        self.transients += 1;
        Ok(())
    }
}

struct Rig {
    engine: TestEngine,
    admin: PgConnection,
    rt: PgConnection,
    n: usize,
}

impl Rig {
    async fn start(name: &str) -> Self {
        // Short pings make the server notice closed streams quickly, so
        // dropped test connections give their slots back.
        let engine = TestEngine::start(
            name,
            Opts {
                tweak: Some(Box::new(|app| {
                    app.sse.ping = Duration::from_millis(150);
                    // Dropped streams are reaped on the next ping, which is later
                    // than a fast replay opens the next one.
                    app.sse_slots = Arc::new(tokio::sync::Semaphore::new(4096));
                })),
                ..Opts::default()
            },
        )
        .await;
        let admin = connect(name).await;
        let rt = connect_runtime(name).await;
        Self {
            engine,
            admin,
            rt,
            n: 0,
        }
    }

    /// Empty log, head 0, no stream state: the model's `init`.
    async fn reset(&mut self) {
        let a = &mut self.admin;
        a.execute("ALTER TABLE _oo_docs.event DISABLE TRIGGER event_insert_only")
            .await
            .unwrap();
        a.execute("DELETE FROM _oo_docs.event").await.unwrap();
        a.execute("ALTER TABLE _oo_docs.event ENABLE TRIGGER event_insert_only")
            .await
            .unwrap();
        a.execute("UPDATE _oo_docs.event_head SET seq = 0")
            .await
            .unwrap();
    }

    async fn head(&mut self) -> i64 {
        sqlx::query_scalar("SELECT seq FROM _oo_docs.event_head")
            .fetch_one(&mut self.admin)
            .await
            .unwrap()
    }

    async fn rows(&mut self) -> BTreeMap<i64, (i64, i64)> {
        sqlx::query(
            "SELECT seq, (payload->>'key')::bigint AS k, (payload->>'value')::bigint AS v \
             FROM _oo_docs.event ORDER BY seq",
        )
        .fetch_all(&mut self.admin)
        .await
        .unwrap()
        .iter()
        .map(|r| {
            (
                r.get::<i64, _>("seq"),
                (r.get::<i64, _>("k"), r.get::<i64, _>("v")),
            )
        })
        .collect()
    }
}

async fn replay(rig: &mut Rig, trace: &Value) -> Result<usize, String> {
    let states = trace["states"].as_array().ok_or("states")?;
    rig.reset().await;
    let addr = rig.engine.addr;
    let first = parse_state(&states[0]);
    let keys: Vec<i64> = first.view.keys().copied().collect();
    let mut client = Client::new(addr, &keys);
    // truth: what a list endpoint would return (the application state).
    let mut truth: BTreeMap<i64, i64> = keys.iter().map(|k| (*k, 0)).collect();
    let mut floor = 0i64;
    let mut connected = true;
    let mut res = "init".to_owned();
    if client.connect(0).await? != 200 {
        return Err("initial subscribe failed".into());
    }

    for (i, st) in states.iter().enumerate() {
        let m = parse_state(st);
        if i > 0 {
            let action = st["mbt::actionTaken"].as_str().ok_or("actionTaken")?;
            let pick = |name: &str| -> i64 { int(&st["mbt::nondetPicks"][name]["value"]) };
            match action {
                "appendEvent" => {
                    let (k, v) = (pick("k"), pick("v"));
                    let payload = json!({ "key": k, "value": v }).to_string();
                    let seq = append_event(
                        &mut rig.rt,
                        "drive.updated",
                        None,
                        Some(&format!("k{k}")),
                        &payload,
                    )
                    .await
                    .map_err(|e| format!("step {i}: append: {e}"))?;
                    if seq != m.head {
                        return Err(format!("step {i}: seq {seq}, model head {}", m.head));
                    }
                    truth.insert(k, v);
                    res = "appendEvent".into();
                }
                "deliver" => {
                    client
                        .deliver()
                        .await
                        .map_err(|e| format!("step {i}: {e}"))?;
                    res = "applied".into();
                }
                "duplicate" => {
                    let s = pick("s");
                    let event = if s > floor {
                        // A real replay of the retained event.
                        let mut r = open(addr, s - 1).await;
                        if r.status != 200 {
                            return Err(format!(
                                "step {i}: replay since={} -> {}",
                                s - 1,
                                r.status
                            ));
                        }
                        let e = loop {
                            let e = r
                                .event(WAIT)
                                .await
                                .ok_or_else(|| format!("step {i}: replay stalled"))?;
                            if e.get("seq").is_some() {
                                break e;
                            }
                        };
                        if e["seq"] != s {
                            return Err(format!("step {i}: replay gave seq {}", e["seq"]));
                        }
                        if let Some(old) = client.seen.get(&s)
                            && old != &e
                        {
                            return Err(format!("step {i}: replayed event differs: {old} vs {e}"));
                        }
                        e
                    } else {
                        // Purged: only the copy already on the wire is left.
                        client
                            .seen
                            .get(&s)
                            .cloned()
                            .unwrap_or_else(|| json!({ "seq": s, "data": {} }))
                    };
                    let applied = client.apply(&event).map_err(|e| format!("step {i}: {e}"))?;
                    if applied {
                        return Err(format!("step {i}: duplicate seq {s} was applied"));
                    }
                    res = "dup_dropped".into();
                }
                "transient" => {
                    let n = rig.n;
                    rig.n += 1;
                    let sent = rig.engine.app.progress.emit(Progress {
                        file_id: format!("fm4-file-{n}"),
                        drive_id: None,
                        unit: "line",
                        done: 1,
                        total: Some(2),
                    });
                    if !sent {
                        return Err(format!("step {i}: progress rate limited"));
                    }
                    client
                        .transient()
                        .await
                        .map_err(|e| format!("step {i}: {e}"))?;
                    res = "transient".into();
                }
                "disconnect" => {
                    client.stream = None;
                    client.buf.clear();
                    connected = false;
                    res = "disconnected".into();
                }
                "purge" => {
                    let n = pick("n");
                    let a = &mut rig.admin;
                    a.execute("ALTER TABLE _oo_docs.event DISABLE TRIGGER event_insert_only")
                        .await
                        .unwrap();
                    sqlx::query("DELETE FROM _oo_docs.event WHERE seq <= $1")
                        .bind(n)
                        .execute(&mut *a)
                        .await
                        .unwrap();
                    a.execute("ALTER TABLE _oo_docs.event ENABLE TRIGGER event_insert_only")
                        .await
                        .unwrap();
                    floor = n;
                    // a purge that leaves the connected client below the floor closes its stream (the model's
                    // rule; the live `run` loop has no such check, see FM4.md: no retention job exists yet)
                    if connected && client.cursor < n {
                        client.stream = None;
                        client.buf.clear();
                        connected = false;
                    }
                    res = "purged".into();
                }
                "reconnect" => {
                    let status = client
                        .connect(client.cursor)
                        .await
                        .map_err(|e| format!("step {i}: {e}"))?;
                    match status {
                        200 => res = "resumed".into(),
                        410 => {
                            // Refetch the list, then subscribe from its head.
                            client.view = truth.clone();
                            client.cursor = rig.head().await;
                            client.refetches += 1;
                            let again = client
                                .connect(client.cursor)
                                .await
                                .map_err(|e| format!("step {i}: {e}"))?;
                            if again != 200 {
                                return Err(format!("step {i}: subscribe after refetch: {again}"));
                            }
                            res = "gone_410_refetch".into();
                        }
                        other => return Err(format!("step {i}: reconnect status {other}")),
                    }
                    connected = true;
                }
                other => return Err(format!("step {i}: unknown action {other}")),
            }
        }
        // ---- compare ----
        let head = rig.head().await;
        if head != m.head {
            return Err(format!("step {i}: head db {head} model {}", m.head));
        }
        let rows = rig.rows().await;
        let want: BTreeMap<i64, (i64, i64)> = m
            .hist
            .iter()
            .filter(|(s, _)| **s > m.floor)
            .map(|(s, v)| (*s, *v))
            .collect();
        if rows != want {
            return Err(format!("step {i}: event rows db {rows:?} model {want:?}"));
        }
        let got = Model {
            hist: m.hist.clone(),
            head,
            floor,
            connected,
            cursor: client.cursor,
            view: client.view.clone(),
            transients: client.transients,
            dup_dropped: client.dup_dropped,
            refetches: client.refetches,
            res: res.clone(),
        };
        if got != m {
            return Err(format!("step {i}: client/real {got:?}\n model {m:?}"));
        }
    }
    client.stream = None;
    Ok(states.len() - 1)
}

fn trace_dir() -> PathBuf {
    std::env::var("OO_FM4_TRACES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../formal/traces/event_stream")
        })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn fm4_itf_traces_conform_to_the_event_log_and_sse() {
    let dir = trace_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(".itf.json"))
        .collect();
    files.sort();
    if let Some(max) = std::env::var("OO_FM4_MAX")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        files.truncate(max);
    }
    assert!(!files.is_empty(), "no *.itf.json in {}", dir.display());
    let par: usize = std::env::var("OO_FM4_PAR")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    let files = Arc::new(files);
    let next = Arc::new(AtomicUsize::new(0));
    // `Opts` (with its boxed tweak) is not `Send`: run the workers on a LocalSet.
    let local = tokio::task::LocalSet::new();
    let mut tasks = Vec::new();
    for n in 0..par {
        let (files, next) = (files.clone(), next.clone());
        tasks.push(local.spawn_local(async move {
            let mut rig = Rig::start(&format!("oo_docs_cev_{n}")).await;
            let (mut traces, mut steps) = (0usize, 0usize);
            let mut outcome = Ok(());
            loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(path) = files.get(i) else { break };
                let text = std::fs::read_to_string(path).unwrap();
                let trace: Value = serde_json::from_str(&text).unwrap();
                match replay(&mut rig, &trace).await {
                    Ok(s) => {
                        traces += 1;
                        steps += s;
                    }
                    Err(e) => {
                        let keep = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                            .join("../../target/conformance-fail");
                        let _ = std::fs::create_dir_all(&keep);
                        let _ = std::fs::copy(path, keep.join(path.file_name().unwrap()));
                        outcome = Err(format!("{}: {e}", path.display()));
                        break;
                    }
                }
            }
            let _ = rig.admin.close().await;
            let _ = rig.rt.close().await;
            rig.engine.finish().await;
            outcome.map(|()| (traces, steps))
        }));
    }
    let (mut traces, mut steps) = (0, 0);
    let mut errors = Vec::new();
    let results = local
        .run_until(async {
            let mut out = Vec::new();
            for t in tasks {
                out.push(t.await.unwrap());
            }
            out
        })
        .await;
    for r in results {
        match r {
            Ok((a, b)) => {
                traces += a;
                steps += b;
            }
            Err(e) => errors.push(e),
        }
    }
    eprintln!(
        "conformance_events: {traces} traces, {steps} steps replayed from {}",
        dir.display()
    );
    assert!(errors.is_empty(), "divergence:\n{}", errors.join("\n"));
    assert!(traces > 0);
}
