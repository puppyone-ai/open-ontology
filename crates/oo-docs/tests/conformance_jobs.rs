//! FM1 conformance (80 §4, card V-S2-01): replay Quint ITF traces of
//! `formal/quint/jobs.qnt` against the real `_oo_docs` job functions and
//! compare the observable job rows after every step.
//!
//! ```text
//! OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test conformance_jobs -- --ignored --nocapture
//! ```
//!
//! Default input is the committed sample set `formal/traces/jobs/`. For the
//! bulk run (80 asks 10,000 traces) generate more and point at them:
//!
//! ```text
//! formal/scripts/formal-traces jobs jobs 10000 30 0x464d /tmp/fm1-traces --no-mbt
//! OO_FM1_TRACES_DIR=/tmp/fm1-traces OO_FM1_PAR=8 OO_FM1_MAX=10000 cargo test ... (as above)
//! ```
//!
//! How a model step maps to the database (the model reads `lastOp` in each
//! state):
//! * `claim` picks the job the model chose by giving it priority 0 (others
//!   100) before calling `claim(.., limit 1)`; the SQL orders by priority.
//! * `expire` is `UPDATE .. lease_until = now - 1 s` (time passing).
//! * retry delays are 0, so a queued row is always due, as in the model.

mod common;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use oo_docs::jobs::{JobKind, JobStatus, NewJob};
use oo_docs::store::jobs as store;
use serde_json::Value;
use sqlx::{Connection, PgConnection, Row};

const ABSENT_ID: &str = "00000000-0000-0000-0000-000000000000";
const MAX_ATTEMPTS: i32 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
struct MJob {
    st: String,
    attempt: i64,
    generation: i64,
    owner: i64,
    live: bool,
    crash: i64,
    cancel: bool,
}

#[derive(Clone, Debug)]
struct Op {
    kind: String,
    w: i64,
    j: i64,
    g: i64,
    crash: bool,
    cnt: bool,
}

struct ModelState {
    jobs: BTreeMap<i64, MJob>,
    successes: BTreeMap<i64, i64>,
    rows: BTreeMap<i64, i64>,
    op: Op,
    res: String,
}

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

fn map(v: &Value) -> Vec<(&Value, &Value)> {
    v["#map"]
        .as_array()
        .expect("#map")
        .iter()
        .map(|p| (&p[0], &p[1]))
        .collect()
}

fn parse_state(s: &Value) -> ModelState {
    let jobs = map(&s["jobs"])
        .into_iter()
        .map(|(k, v)| {
            (
                int(k),
                MJob {
                    st: v["st"].as_str().unwrap().to_string(),
                    attempt: int(&v["attempt"]),
                    generation: int(&v["gen"]),
                    owner: int(&v["owner"]),
                    live: v["live"].as_bool().unwrap(),
                    crash: int(&v["crash"]),
                    cancel: v["cancel"].as_bool().unwrap(),
                },
            )
        })
        .collect();
    let ints = |name: &str| -> BTreeMap<i64, i64> {
        map(&s[name])
            .into_iter()
            .map(|(k, v)| (int(k), int(v)))
            .collect()
    };
    let o = &s["lastOp"];
    ModelState {
        jobs,
        successes: ints("successes"),
        rows: ints("rows"),
        op: Op {
            kind: o["kind"].as_str().unwrap().to_string(),
            w: int(&o["w"]),
            j: int(&o["j"]),
            g: int(&o["g"]),
            crash: o["crash"].as_bool().unwrap(),
            cnt: o["cnt"].as_bool().unwrap(),
        },
        res: s["lastRes"].as_str().unwrap().to_string(),
    }
}

fn status_name(s: &JobStatus) -> String {
    match s {
        JobStatus::Enqueued => "enqueued",
        JobStatus::Replayed => "replayed",
        JobStatus::AlreadyDone => "already_done",
        JobStatus::Terminal => "terminal",
        JobStatus::Claimed => "claimed",
        JobStatus::Extended => "extended",
        JobStatus::LostLease => "lost_lease",
        JobStatus::Succeeded => "succeeded",
        JobStatus::Failed => "failed",
        JobStatus::RetryScheduled => "retry_scheduled",
        JobStatus::Exhausted => "exhausted",
        JobStatus::Quarantined => "quarantined",
        JobStatus::Cancelled => "cancelled",
        JobStatus::CancelRequested => "cancel_requested",
        JobStatus::NotRequested => "not_requested",
        JobStatus::Saved => "saved",
        JobStatus::NotFound => "not_found",
        JobStatus::Other(x) => return format!("other:{x}"),
    }
    .to_string()
}

fn owner(w: i64) -> String {
    format!("w{w}/1/0000000000000000")
}

struct Rig {
    rt: PgConnection,
    admin: PgConnection,
    ids: HashMap<i64, String>,
    successes: BTreeMap<i64, i64>,
}

impl Rig {
    async fn reset(&mut self) {
        sqlx::query("DELETE FROM _oo_docs.job")
            .execute(&mut self.admin)
            .await
            .unwrap();
        self.ids.clear();
        self.successes.clear();
    }

    fn id(&self, j: i64) -> String {
        self.ids
            .get(&j)
            .cloned()
            .unwrap_or_else(|| ABSENT_ID.into())
    }

    /// Execute one model action on the real functions; returns the status text.
    async fn apply(&mut self, op: &Op) -> String {
        let id = self.id(op.j);
        let own = owner(op.w);
        let rt = &mut self.rt;
        match op.kind.as_str() {
            "enqueue" => {
                let key = format!("k{}", op.j);
                let mut job = NewJob::new(JobKind::Parse, &key);
                job.max_attempts = Some(MAX_ATTEMPTS);
                let r = store::enqueue(rt, &job).await.unwrap();
                if let Some(j) = &r.job {
                    self.ids.insert(op.j, j.id.clone());
                }
                status_name(&r.status)
            }
            "expire" => {
                sqlx::query(
                    "UPDATE _oo_docs.job SET lease_until = clock_timestamp() - interval '1 second' WHERE id = $1::uuid",
                )
                .bind(&id)
                .execute(&mut self.admin)
                .await
                .unwrap();
                "expired".into()
            }
            "claim" | "claim_none" => {
                // Steer the SQL ordering (priority, run_at, id) to the model's pick.
                let key = format!("k{}", op.j);
                sqlx::query(
                    "UPDATE _oo_docs.job SET priority = (CASE WHEN dedupe_key = $1 THEN 0 ELSE 100 END)::smallint",
                )
                .bind(&key)
                .execute(&mut self.admin)
                .await
                .unwrap();
                let got = store::claim(rt, &["parse"], &own, 1).await.unwrap();
                match got.len() {
                    0 => "empty".into(),
                    1 => {
                        assert_eq!(got[0].id, id, "claim returned another job than the model's");
                        "claimed".into()
                    }
                    n => panic!("claim(limit 1) returned {n} rows"),
                }
            }
            "heartbeat" => {
                status_name(&store::heartbeat(rt, &id, &own, op.g).await.unwrap().status)
            }
            "complete" => {
                let s = status_name(&store::complete(rt, &id, &own, op.g).await.unwrap().status);
                if s == "succeeded" {
                    *self.successes.entry(op.j).or_insert(0) += 1;
                }
                s
            }
            "fail" => status_name(
                &store::fail(rt, &id, &own, op.g, "E_FM1")
                    .await
                    .unwrap()
                    .status,
            ),
            "quarantine" => status_name(
                &store::quarantine(rt, &id, &own, op.g, "E_FM1")
                    .await
                    .unwrap()
                    .status,
            ),
            "retry" => status_name(
                &store::retry(rt, &id, &own, op.g, "E_FM1", 0, op.cnt, op.crash)
                    .await
                    .unwrap()
                    .status,
            ),
            "cancel" => status_name(&store::cancel(rt, &id).await.unwrap().status),
            "ack_cancel" => {
                status_name(&store::ack_cancel(rt, &id, &own, op.g).await.unwrap().status)
            }
            "checkpoint" => status_name(
                &store::save_checkpoint(rt, &id, &own, op.g, "{}")
                    .await
                    .unwrap()
                    .status,
            ),
            other => panic!("unknown model action {other}"),
        }
    }

    /// Observable rows keyed by dedupe key index.
    async fn snapshot(&mut self) -> BTreeMap<i64, MJob> {
        let rows = sqlx::query(
            "SELECT dedupe_key, state, attempt, generation, lease_owner, crash_count, cancel_requested, \
             COALESCE(lease_until > clock_timestamp(), false) AS live FROM _oo_docs.job",
        )
        .fetch_all(&mut self.admin)
        .await
        .unwrap();
        rows.iter()
            .map(|r| {
                let key: String = r.get("dedupe_key");
                let owner: Option<String> = r.get("lease_owner");
                (
                    key[1..].parse::<i64>().unwrap(),
                    MJob {
                        st: r.get("state"),
                        attempt: i64::from(r.get::<i32, _>("attempt")),
                        generation: r.get("generation"),
                        owner: owner
                            .map(|o| o[1..o.find('/').unwrap()].parse::<i64>().unwrap())
                            .unwrap_or(-1),
                        live: r.get("live"),
                        crash: i64::from(r.get::<i32, _>("crash_count")),
                        cancel: r.get("cancel_requested"),
                    },
                )
            })
            .collect()
    }
}

/// Replays one trace; returns the number of steps or a description of the first divergence.
async fn replay(rig: &mut Rig, trace: &Value) -> Result<usize, String> {
    let states = trace["states"].as_array().ok_or("no states")?;
    rig.reset().await;
    for (i, s) in states.iter().enumerate() {
        let m = parse_state(s);
        if i > 0 {
            let got = rig.apply(&m.op).await;
            if got != m.res {
                return Err(format!(
                    "step {i} {:?}: result {got:?}, model {:?}",
                    m.op, m.res
                ));
            }
        }
        let db = rig.snapshot().await;
        let model_present: BTreeMap<i64, MJob> = m
            .jobs
            .iter()
            .filter(|(_, v)| v.st != "absent")
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        if db != model_present {
            return Err(format!(
                "step {i} after {:?}:\n  db    {db:?}\n  model {model_present:?}",
                m.op
            ));
        }
        let rows_db: BTreeMap<i64, i64> = m
            .jobs
            .keys()
            .map(|k| (*k, i64::from(db.contains_key(k))))
            .collect();
        if rows_db != m.rows {
            return Err(format!("step {i}: rows db {rows_db:?} model {:?}", m.rows));
        }
        for (k, n) in &m.successes {
            let got = rig.successes.get(k).copied().unwrap_or(0);
            if got != *n {
                return Err(format!("step {i}: successes[{k}] db {got} model {n}"));
            }
        }
    }
    Ok(states.len() - 1)
}

fn trace_dir() -> PathBuf {
    std::env::var("OO_FM1_TRACES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../formal/traces/jobs")
        })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn fm1_itf_traces_conform_to_the_sql_functions() {
    let dir = trace_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(".itf.json"))
        .collect();
    files.sort();
    if let Some(max) = std::env::var("OO_FM1_MAX")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        files.truncate(max);
    }
    assert!(!files.is_empty(), "no *.itf.json in {}", dir.display());
    let par: usize = std::env::var("OO_FM1_PAR")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    let files = Arc::new(files);
    let next = Arc::new(AtomicUsize::new(0));

    let mut tasks = Vec::new();
    for n in 0..par {
        let (files, next) = (files.clone(), next.clone());
        tasks.push(tokio::spawn(async move {
            let name = format!("oo_docs_cj_{n}");
            let rig = {
                let _g = CLUSTER_LOCK.lock().await;
                installed_db(&name).await;
                Rig {
                    rt: connect_runtime(&name).await,
                    admin: connect(&name).await,
                    ids: HashMap::new(),
                    successes: BTreeMap::new(),
                }
            };
            let mut rig = rig;
            let (mut traces, mut steps) = (0usize, 0usize);
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
                        return Err(format!("{}: {e}", path.display()));
                    }
                }
            }
            let _ = rig.rt.close().await;
            let _ = rig.admin.close().await;
            drop_db(&name).await;
            Ok((traces, steps))
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
        "conformance_jobs: {traces} traces, {steps} steps replayed from {}",
        dir.display()
    );
    assert!(errors.is_empty(), "divergence:\n{}", errors.join("\n"));
    assert!(traces > 0);
}
