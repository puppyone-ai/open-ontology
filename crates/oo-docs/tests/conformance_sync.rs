//! FM3 (fs) conformance (80 §4, card V-S3-02): replay Quint ITF traces of
//! `formal/quint/sync_reconcile.qnt` against the real reconcile
//! (`ingest::reconcile`), a real PostgreSQL and a real directory.
//!
//! ```text
//! OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test conformance_sync -- --ignored --nocapture
//! OO_FM3_TRACES_DIR=/tmp/fm3-traces OO_FM3_PAR=4 ... (bulk: formal/scripts/formal-traces sync_reconcile sync_reconcile 1000 40 0x464d /tmp/fm3-traces)
//! ```
//!
//! How a model step maps to the system (`mbt::actionTaken`, picks in
//! `mbt::nondetPicks`; key `k` is the file `k<k>.md`, content `c` a text whose
//! size is unique per content, so equal size and equal BLAKE3 mean equal `c`):
//! * `write`, `remove`, `move`: the directory changes (`fs::write`, `remove_file`, `rename`).
//! * `startSync`: the real listing of the directory is taken now and kept (a
//!   partial listing keeps only the keys below `cut` and says `complete =
//!   false`), because a full sync is two steps and writes can fall between.
//! * `finishSync`: `run_sync_source` over a source that serves the kept
//!   listing and the live directory for everything else (stat, read, hash),
//!   with `confirm` as the model says.
//! * `targeted`: `targeted_sync` of that one path.
//! * `hashAll`: every due hash job runs to the end.
//!
//! After every step each key is compared: row present / live / deleted,
//! `last_seen_generation`, the content hash of the current version, whether a
//! hash job is pending; also `drive.sync_generation`, the status of the last
//! sync and whether it is held.
//!
//! Not covered: object stores (s3 versionId and sequencer, V-S4-01), pages
//! (listings are one page), symlinks, sniffing and sizes, parallel syncs and
//! the job scheduler (jobs are driven by hand).

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use common::*;
use oo_connectors::objectstore::fs::FsSource;
use oo_connectors::objectstore::hash::DualDigest;
use oo_connectors::objectstore::{
    DocBackend, DocCapability, DocSourceError, DocumentSource, ListCursor, ListPage, ObjectMeta,
    ReadExpect,
};
use oo_docs::ingest::reconcile::{SyncStatus, load_drive, run_sync_source, targeted_sync};
use oo_docs::ingest::{
    FsFactory, HashableSource, IngestLimits, SourceFactory, SyncFlavor, Trigger, hashjob,
};
use oo_docs::jobs::report_failure;
use oo_docs::jobs::scheduler::{JobCtx, Outcome};
use oo_docs::store::jobs as store;
use serde_json::Value;
use sqlx::PgPool;

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

fn map(v: &Value) -> Vec<(i64, Value)> {
    v["#map"]
        .as_array()
        .expect("#map")
        .iter()
        .map(|p| (int(&p[0]), p[1].clone()))
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
struct MRow {
    st: i64,
    seen: i64,
    c: i64,
    hp: bool,
}

struct Model {
    src: BTreeMap<i64, i64>,
    rows: BTreeMap<i64, MRow>,
    gen_: i64,
    held: bool,
    snap: BTreeMap<i64, i64>,
    ls_confirm: bool,
    ls_active: bool,
    ls_g: i64,
    kind: String,
}

fn parse_state(s: &Value) -> Model {
    Model {
        src: map(&s["src"])
            .into_iter()
            .map(|(k, v)| (k, int(&v)))
            .collect(),
        rows: map(&s["rows"])
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    MRow {
                        st: int(&v["st"]),
                        seen: int(&v["seen"]),
                        c: int(&v["c"]),
                        hp: v["hp"].as_bool().unwrap(),
                    },
                )
            })
            .collect(),
        gen_: int(&s["gen"]),
        held: s["held"].as_bool().unwrap(),
        snap: map(&s["ls"]["snap"])
            .into_iter()
            .map(|(k, v)| (k, int(&v)))
            .collect(),
        ls_confirm: s["ls"]["confirm"].as_bool().unwrap(),
        ls_active: s["ls"]["active"].as_bool().unwrap(),
        ls_g: int(&s["ls"]["g"]),
        kind: s["lastKind"].as_str().unwrap().to_owned(),
    }
}

fn body(c: i64) -> String {
    format!("content {c}\n{}\n", "x".repeat(c as usize * 7))
}

fn b3_of(c: i64) -> String {
    blake3::hash(body(c).as_bytes()).to_hex().to_string()
}

fn name(k: i64) -> String {
    format!("k{k}.md")
}

/// Serves a kept listing; everything else is the live directory.
struct Kept {
    inner: FsSource,
    entries: Vec<ObjectMeta>,
    complete: bool,
    /// The listing is served when the test opens the gate (after the sync has
    /// started, as in `run_sync_source`: start, list, apply).
    gate: Arc<tokio::sync::Notify>,
}

impl DocumentSource for Kept {
    fn backend(&self) -> DocBackend {
        self.inner.backend()
    }
    fn capabilities(&self) -> std::collections::BTreeSet<DocCapability> {
        self.inner.capabilities()
    }
    async fn list(
        &self,
        _cursor: Option<&ListCursor>,
        _page_size: u32,
    ) -> Result<ListPage, DocSourceError> {
        self.gate.notified().await;
        Ok(ListPage {
            entries: self.entries.clone(),
            next: None,
            complete: self.complete,
        })
    }
    async fn stat(&self, raw_key: &[u8]) -> Result<Option<ObjectMeta>, DocSourceError> {
        self.inner.stat(raw_key).await
    }
    async fn read(
        &self,
        raw_key: &[u8],
        offset: u64,
        len: u64,
        expect: &ReadExpect,
    ) -> Result<Vec<u8>, DocSourceError> {
        self.inner.read(raw_key, offset, len, expect).await
    }
}

impl HashableSource for Kept {
    fn hash_object(
        &self,
        raw_key: &[u8],
    ) -> impl std::future::Future<Output = Result<(DualDigest, ObjectMeta), DocSourceError>> + Send
    {
        self.inner.hash_object(raw_key)
    }
}

struct Rig {
    pool: PgPool,
    drive: String,
    root: PathBuf,
    limits: IngestLimits,
    n: usize,
}

impl Rig {
    async fn start(name: &str) -> Self {
        {
            let _g = CLUSTER_LOCK.lock().await;
            installed_db(name).await;
        }
        let pool = pool_runtime(name, 8).await;
        let root = std::env::temp_dir().join(format!("oo-docs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let config = serde_json::json!({ "root": root.to_str().unwrap() }).to_string();
        let drive: String = sqlx::query_scalar(
            "INSERT INTO _oo_docs.drive (id, name, backend, config, max_files) \
             VALUES (gen_random_uuid(), 'cs-start', 'fs', $1::jsonb, 1000) RETURNING id::text",
        )
        .bind(config)
        .fetch_one(&pool)
        .await
        .unwrap();
        let limits = IngestLimits {
            settle: Duration::ZERO,
            page_size: 100,
            mass_delete_min: 1,
            ..IngestLimits::default()
        };
        Self {
            pool,
            drive,
            root,
            limits,
            n: 0,
        }
    }

    /// Back to an empty drive: the model's `init`.
    async fn reset(&mut self) {
        for sql in [
            "DELETE FROM _oo_docs.job WHERE drive_id = $1::uuid",
            "DELETE FROM _oo_docs.drive_sync WHERE drive_id = $1::uuid",
        ] {
            let _ = sqlx::query(sql).bind(&self.drive).execute(&self.pool).await;
        }
        // Files are never deleted from a live system; a fresh drive per trace
        // is the simple reset.
        let config = serde_json::json!({ "root": self.root.to_str().unwrap() }).to_string();
        self.drive = sqlx::query_scalar(
            "INSERT INTO _oo_docs.drive (id, name, backend, config, max_files) \
             VALUES (gen_random_uuid(), $2, 'fs', $1::jsonb, 1000) RETURNING id::text",
        )
        .bind(config)
        .bind(format!("cs-{}", self.n))
        .fetch_one(&self.pool)
        .await
        .unwrap();
        for e in std::fs::read_dir(&self.root).unwrap().flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }

    fn inner(&self) -> FsSource {
        FsSource::open(&self.root, Default::default()).unwrap()
    }

    async fn listing(&self) -> Vec<ObjectMeta> {
        let src = self.inner();
        let mut out = Vec::new();
        let mut cursor: Option<ListCursor> = None;
        loop {
            let page = src.list(cursor.as_ref(), 100).await.unwrap();
            out.extend(page.entries);
            match page.next {
                Some(n) => cursor = Some(n),
                None => return out,
            }
        }
    }

    /// Run every due hash job to the end, as the scheduler would.
    async fn hash_all(&self) {
        let drive = load_drive(&self.pool, &self.drive).await.unwrap().unwrap();
        let source = FsFactory.open(&drive).unwrap();
        loop {
            let mut conn = self.pool.acquire().await.unwrap();
            let jobs = store::claim(&mut conn, &["hash"], "cs/1/0", 8)
                .await
                .unwrap();
            drop(conn);
            if jobs.is_empty() {
                return;
            }
            for job in jobs {
                let ctx = JobCtx::detached(job.clone(), "cs/1/0", self.pool.clone());
                let out = hashjob::run_hash(&ctx, &source, &self.limits).await;
                let mut conn = self.pool.acquire().await.unwrap();
                match out {
                    Outcome::Done => {
                        store::complete(&mut conn, &job.id, "cs/1/0", job.generation)
                            .await
                            .unwrap();
                    }
                    Outcome::Completed => {}
                    Outcome::Failed { class, code } => {
                        report_failure(&mut conn, &job, "cs/1/0", class, &code)
                            .await
                            .unwrap();
                    }
                    Outcome::Cancelled => panic!("cancelled"),
                }
            }
        }
    }

    async fn compare(&self, i: usize, m: &Model) -> Result<(), String> {
        for (k, mr) in &m.rows {
            let row: Option<(bool, i64, Option<String>, i64)> = sqlx::query_as(
                "SELECT f.deleted_at IS NULL, f.last_seen_generation, cv.b3, \
                 (SELECT count(*) FROM _oo_docs.job j WHERE j.kind = 'hash' AND j.file_id = f.id \
                    AND j.state IN ('queued', 'running')) \
                 FROM _oo_docs.file f LEFT JOIN _oo_docs.file_version cv ON cv.id = f.current_version_id \
                 WHERE f.drive_id = $1::uuid AND f.path = $2",
            )
            .bind(&self.drive)
            .bind(name(*k))
            .fetch_optional(&self.pool)
            .await
            .unwrap();
            let got = match row {
                None => MRow {
                    st: 0,
                    seen: 0,
                    c: 0,
                    hp: false,
                },
                Some((live, seen, b3, pending)) => {
                    let c = match b3 {
                        None => 0,
                        Some(h) => (1..=3)
                            .find(|c| b3_of(*c) == h)
                            .ok_or_else(|| format!("step {i}: key {k}: unknown content hash"))?,
                    };
                    MRow {
                        st: if live { 1 } else { 2 },
                        seen,
                        c,
                        hp: pending > 0,
                    }
                }
            };
            if got != *mr {
                return Err(format!("step {i}: key {k}: db {got:?} model {mr:?}"));
            }
        }
        let gen_: i64 =
            sqlx::query_scalar("SELECT sync_generation FROM _oo_docs.drive WHERE id = $1::uuid")
                .bind(&self.drive)
                .fetch_one(&self.pool)
                .await
                .unwrap();
        if gen_ != m.gen_ {
            return Err(format!("step {i}: generation db {gen_} model {}", m.gen_));
        }
        let held: Option<String> = sqlx::query_scalar(
            "SELECT status FROM _oo_docs.drive_sync WHERE drive_id = $1::uuid AND generation = $2",
        )
        .bind(&self.drive)
        .bind(m.gen_ + 1)
        .fetch_optional(&self.pool)
        .await
        .unwrap();
        // While a run of the held generation is in progress its row says `running`.
        if !m.ls_active && (held.as_deref() == Some("held")) != m.held {
            return Err(format!("step {i}: held db {held:?} model {}", m.held));
        }
        Ok(())
    }
}

async fn replay(rig: &mut Rig, trace: &Value) -> Result<usize, String> {
    let mut running = None;
    let out = replay_steps(rig, trace, &mut running).await;
    // A listing that never finished must not keep its drive lock and connection.
    if let Some((handle, gate, _)) = running.take() {
        gate.notify_one();
        let _ = handle.await;
    }
    out
}

type Running = (
    tokio::task::JoinHandle<
        Result<oo_docs::ingest::reconcile::SyncReport, oo_docs::ingest::IngestError>,
    >,
    Arc<tokio::sync::Notify>,
    i64,
);

async fn replay_steps(
    rig: &mut Rig,
    trace: &Value,
    running: &mut Option<Running>,
) -> Result<usize, String> {
    let states = trace["states"].as_array().ok_or("states")?;
    rig.reset().await;
    rig.n += 1;
    for (i, st) in states.iter().enumerate() {
        let m = parse_state(st);
        if i > 0 {
            let action = st["mbt::actionTaken"].as_str().ok_or("actionTaken")?;
            let pick = |n: &str| -> &Value { &st["mbt::nondetPicks"][n]["value"] };
            match action {
                "write" => {
                    let (k, c) = (int(pick("k")), int(pick("c")));
                    std::fs::write(rig.root.join(name(k)), body(c)).unwrap();
                }
                "remove" => {
                    std::fs::remove_file(rig.root.join(name(int(pick("k"))))).unwrap();
                }
                "move" => {
                    let (a, b) = (int(pick("a")), int(pick("b")));
                    std::fs::rename(rig.root.join(name(a)), rig.root.join(name(b))).unwrap();
                }
                "startSync" => {
                    let partial = pick("p").as_bool().unwrap();
                    let cut = int(pick("cut"));
                    let mut entries = rig.listing().await;
                    if partial {
                        entries.retain(|e| {
                            let p = e.path.as_ref().map(|p| p.as_str().to_owned());
                            p.and_then(|p| {
                                p.strip_prefix('k')
                                    .and_then(|r| r.strip_suffix(".md").map(str::to_owned))
                            })
                            .and_then(|n| n.parse::<i64>().ok())
                            .is_some_and(|k| k < cut)
                        });
                    }
                    let seen: BTreeMap<i64, i64> = (0..4)
                        .map(|k| {
                            let present = entries.iter().any(|e| {
                                e.path.as_ref().map(|p| p.as_str()) == Some(name(k).as_str())
                            });
                            (k, if present { m.src[&k] } else { 0 })
                        })
                        .collect();
                    if seen != m.snap {
                        return Err(format!("step {i}: listing {seen:?} model {:?}", m.snap));
                    }
                    let gate = Arc::new(tokio::sync::Notify::new());
                    let drive = load_drive(&rig.pool, &rig.drive).await.unwrap().unwrap();
                    let g = drive.sync_generation + 1;
                    if g != m.ls_g {
                        return Err(format!("step {i}: generation {g} model {}", m.ls_g));
                    }
                    let src = Kept {
                        inner: rig.inner(),
                        entries,
                        complete: !partial,
                        gate: gate.clone(),
                    };
                    let flavor = SyncFlavor {
                        trigger: Trigger::Poll,
                        deep: false,
                        confirm: if m.ls_confirm { Some(g) } else { None },
                    };
                    let (pool, limits) = (rig.pool.clone(), rig.limits.clone());
                    let handle = tokio::spawn(async move {
                        run_sync_source(
                            &pool,
                            &drive,
                            &src,
                            flavor,
                            &limits,
                            &AtomicBool::new(false),
                        )
                        .await
                    });
                    // The sync has started (its drive_sync row is there) and waits for its listing.
                    let mut started = false;
                    for _ in 0..200 {
                        let n: i64 = sqlx::query_scalar(
                            "SELECT count(*) FROM _oo_docs.drive_sync WHERE drive_id = $1::uuid \
                             AND generation = $2 AND status = 'running'",
                        )
                        .bind(&rig.drive)
                        .bind(g)
                        .fetch_one(&rig.pool)
                        .await
                        .unwrap();
                        if n > 0 {
                            started = true;
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    if !started {
                        return Err(format!("step {i}: the sync did not start"));
                    }
                    *running = Some((handle, gate, g));
                }
                "finishSync" => {
                    let (handle, gate, g) = running.take().ok_or("finish without start")?;
                    gate.notify_one();
                    let r = handle
                        .await
                        .map_err(|e| format!("step {i}: sync task: {e}"))?
                        .map_err(|e| format!("step {i}: sync: {e:?}"))?;
                    let want = match m.kind.as_str() {
                        "partial" => SyncStatus::Failed,
                        "held" => SyncStatus::Held,
                        "sweep" => SyncStatus::Complete,
                        other => return Err(format!("step {i}: finish with kind {other}")),
                    };
                    if r.status != want {
                        return Err(format!(
                            "step {i}: sync status {:?} model {want:?} ({r:?})",
                            r.status
                        ));
                    }
                    if r.generation != g {
                        return Err(format!("step {i}: generation {} model {g}", r.generation));
                    }
                }
                "targeted" => {
                    let t = int(pick("t"));
                    let drive = load_drive(&rig.pool, &rig.drive).await.unwrap().unwrap();
                    targeted_sync(&rig.pool, &drive, &rig.inner(), &[name(t)], &rig.limits)
                        .await
                        .map_err(|e| format!("step {i}: targeted: {e:?}"))?;
                }
                "hashAll" => rig.hash_all().await,
                other => return Err(format!("step {i}: unknown action {other}")),
            }
        }
        rig.compare(i, &m).await?;
        // the folder itself
        for (k, c) in &m.src {
            let p = rig.root.join(name(*k));
            match std::fs::read_to_string(&p) {
                Ok(t) if *c != 0 && t == body(*c) => {}
                Err(_) if *c == 0 => {}
                other => return Err(format!("step {i}: folder key {k}: {other:?} model {c}")),
            }
        }
    }
    Ok(states.len() - 1)
}

fn trace_dir() -> PathBuf {
    std::env::var("OO_FM3_TRACES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../formal/traces/sync_reconcile")
        })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn fm3_itf_traces_conform_to_the_reconcile() {
    let dir = trace_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(".itf.json"))
        .collect();
    files.sort();
    if let Some(max) = std::env::var("OO_FM3_MAX")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        files.truncate(max);
    }
    assert!(!files.is_empty(), "no *.itf.json in {}", dir.display());
    let par: usize = std::env::var("OO_FM3_PAR")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    let files = Arc::new(files);
    let next = Arc::new(AtomicUsize::new(0));
    let mut tasks = Vec::new();
    for n in 0..par {
        let (files, next) = (files.clone(), next.clone());
        tasks.push(tokio::spawn(async move {
            let name = format!("oo_docs_csy_{n}");
            let mut rig = Rig::start(&name).await;
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
            rig.pool.close().await;
            let _ = std::fs::remove_dir_all(&rig.root);
            drop_db(&name).await;
            outcome.map(|()| (traces, steps))
        }));
    }
    let (mut traces, mut steps) = (0, 0);
    let mut errors = Vec::new();
    for t in tasks {
        match t.await.unwrap() {
            Ok((a, b)) => {
                traces += a;
                steps += b;
            }
            Err(e) => errors.push(e),
        }
    }
    eprintln!(
        "conformance_sync: {traces} traces, {steps} steps replayed from {}",
        dir.display()
    );
    assert!(errors.is_empty(), "divergence:\n{}", errors.join("\n"));
    assert!(traces > 0);
}
