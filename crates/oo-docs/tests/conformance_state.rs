//! FM2 conformance (80 §4, cards V-S2-02 and V-S3-01): the file state machine
//! model `formal/quint/file_lifecycle.qnt` against `state.rs`.
//!
//! Two parts:
//!
//! 1. `fm2_transition_table_equals_is_legal` (no database): the `EDGES` table
//!    of the model is read from the `.qnt` text and compared with
//!    `state::is_legal` for every (from, to) pair of the 16 states (and for
//!    "no row"), so the table, KP7 (`state/proofs.rs`) and 10 §4.2 are one
//!    thing. A new edge in either place fails this test.
//! 2. `fm2_itf_traces_conform_to_the_transition_function` (container,
//!    `--ignored`): every ITF step that changes the state of a file is run on
//!    the real `state::transition` against a real database row; before it
//!    every target the model calls illegal is tried and must be refused with
//!    `Illegal` and leave the row alone; after it state and `state_code` of all
//!    files must equal the model.
//!
//! ```text
//! OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test conformance_state -- --ignored --nocapture
//! OO_FM2_TRACES_DIR=/tmp/fm2-traces OO_FM2_PAR=4 ... (bulk: formal/scripts/formal-traces file_lifecycle file_lifecycle 2000 40 0x464d /tmp/fm2-traces)
//! ```
//!
//! Not covered: what the stages do between the transitions (parse and extract
//! results, versions, blobs, evidence); the model's guards for those
//! (`hasBlob`, `jobVer`, `resVer`) are checked by the pipeline tests, not here.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use oo_docs::ingest::files::insert_file;
use oo_docs::state::{self, FileState, Transition, TransitionError, is_legal};
use serde_json::Value;
use sqlx::PgPool;

fn model_text() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../formal/quint/file_lifecycle.qnt");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The `EDGES` pairs of the model.
fn model_edges(text: &str) -> BTreeSet<(String, String)> {
    let start = text.find("pure val EDGES").expect("EDGES in the model");
    let end = start
        + text[start..]
            .find("pure def legal")
            .expect("legal after EDGES");
    let block = &text[start..end];
    let mut out = BTreeSet::new();
    for piece in block.split('(').skip(2) {
        // `"a", "b")` after the opening `Set(`
        let Some(close) = piece.find(')') else {
            continue;
        };
        let parts: Vec<&str> = piece[..close].split(',').map(str::trim).collect();
        if parts.len() == 2 && parts.iter().all(|p| p.starts_with('"') && p.ends_with('"')) {
            out.insert((
                parts[0].trim_matches('"').to_owned(),
                parts[1].trim_matches('"').to_owned(),
            ));
        }
    }
    out
}

/// `legal` of the model, written the same way.
fn model_legal(edges: &BTreeSet<(String, String)>, from: &str, to: &str) -> bool {
    if from == "absent" {
        to == "stored"
    } else if to == "superseded" || to == "deleted" {
        (from != "superseded" && from != "deleted") || (from == "superseded" && to == "deleted")
    } else {
        edges.contains(&(from.to_owned(), to.to_owned()))
    }
}

#[test]
fn fm2_transition_table_equals_is_legal() {
    let edges = model_edges(&model_text());
    assert!(edges.len() >= 20, "table not read: {edges:?}");
    // every name in the table is a real state
    for (a, b) in &edges {
        assert!(FileState::parse(a).is_some(), "unknown state {a}");
        assert!(FileState::parse(b).is_some(), "unknown state {b}");
    }
    let mut checked = 0;
    for to in FileState::ALL {
        assert_eq!(
            is_legal(None, to),
            model_legal(&edges, "absent", to.as_str()),
            "creation -> {}",
            to.as_str()
        );
        checked += 1;
        for from in FileState::ALL {
            assert_eq!(
                is_legal(Some(from), to),
                model_legal(&edges, from.as_str(), to.as_str()),
                "{} -> {}",
                from.as_str(),
                to.as_str()
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 16 * 17);
}

#[derive(Debug)]
struct ModelFile {
    st: String,
    pfc: String,
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

fn parse_files(s: &Value) -> BTreeMap<i64, ModelFile> {
    s["files"]["#map"]
        .as_array()
        .expect("#map")
        .iter()
        .map(|p| {
            (
                int(&p[0]),
                ModelFile {
                    st: p[1]["st"].as_str().unwrap().to_owned(),
                    pfc: p[1]["pfc"].as_str().unwrap().to_owned(),
                },
            )
        })
        .collect()
}

fn code_of(pfc: &str) -> Option<&'static str> {
    match pfc {
        "quarantined" => Some("DOCS_PARSE_QUARANTINED"),
        "exhausted" => Some("DOCS_PARSE_ATTEMPTS_EXHAUSTED"),
        "unreadable" => Some("DOCS_SOURCE_PERMISSION_DENIED"),
        _ => None,
    }
}

struct Rig {
    pool: PgPool,
    drive: String,
    n: usize,
}

async fn db_state(pool: &PgPool, id: &str) -> (String, Option<String>) {
    sqlx::query_as("SELECT state, state_code FROM _oo_docs.file WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn replay(rig: &mut Rig, trace: &Value) -> Result<(usize, usize), String> {
    let edges = model_edges(&model_text());
    let states = trace["states"].as_array().ok_or("states")?;
    let trace_no = rig.n;
    rig.n += 1;
    let mut ids: BTreeMap<i64, String> = BTreeMap::new();
    let mut prev = parse_files(&states[0]);
    let mut probes = 0usize;
    for (i, st) in states.iter().enumerate().skip(1) {
        let cur = parse_files(st);
        let changed: Vec<i64> = cur
            .iter()
            .filter(|(k, f)| prev[k].st != f.st)
            .map(|(k, _)| *k)
            .collect();
        if changed.len() > 1 {
            return Err(format!("step {i}: model changed {} files", changed.len()));
        }
        if let Some(&f) = changed.first() {
            let (from, to) = (&prev[&f].st, &cur[&f].st);
            if from == "absent" {
                // listing finds a new path
                let key = format!("t{trace_no}-f{f}");
                let mut conn = rig.pool.acquire().await.unwrap();
                let id = insert_file(&mut conn, &rig.drive, key.as_bytes(), &key, &key, 1)
                    .await
                    .map_err(|e| format!("step {i}: insert: {e}"))?
                    .ok_or("file exists")?;
                ids.insert(f, id);
            } else {
                let id = ids.get(&f).ok_or("file never created")?.clone();
                let from_state = FileState::parse(from).ok_or("from state")?;
                // refused: every target the model calls illegal
                for target in FileState::ALL {
                    if model_legal(&edges, from, target.as_str()) {
                        continue;
                    }
                    let err = state::transition(
                        &rig.pool,
                        &Transition {
                            file_id: &id,
                            expect_from: Some(from_state),
                            to: target,
                            code: None,
                            retryable: false,
                            version_id: None,
                            attempt: None,
                            detail_json: None,
                        },
                    )
                    .await;
                    probes += 1;
                    match err {
                        Err(TransitionError::Illegal { .. }) => {}
                        other => {
                            return Err(format!(
                                "step {i}: {from} -> {} must be illegal, got {other:?}",
                                target.as_str()
                            ));
                        }
                    }
                    let (state_now, _) = db_state(&rig.pool, &id).await;
                    if state_now != *from {
                        return Err(format!("step {i}: refused transition moved {from}"));
                    }
                }
                // the model's move
                let to_state = FileState::parse(to).ok_or("to state")?;
                let code = if to == "parse_failed" {
                    code_of(&cur[&f].pfc)
                } else {
                    None
                };
                state::transition(
                    &rig.pool,
                    &Transition {
                        file_id: &id,
                        expect_from: Some(from_state),
                        to: to_state,
                        code,
                        retryable: false,
                        version_id: None,
                        attempt: None,
                        detail_json: None,
                    },
                )
                .await
                .map_err(|e| format!("step {i}: {from} -> {to} refused: {e:?}"))?;
            }
        }
        // compare every file
        for (k, mf) in &cur {
            match ids.get(k) {
                None => {
                    if mf.st != "absent" {
                        return Err(format!("step {i}: file {k} is {} in the model", mf.st));
                    }
                }
                Some(id) => {
                    let (db, code) = db_state(&rig.pool, id).await;
                    if db != mf.st {
                        return Err(format!("step {i}: file {k} db {db} model {}", mf.st));
                    }
                    let want = if mf.st == "parse_failed" {
                        code_of(&mf.pfc).map(str::to_owned)
                    } else {
                        None
                    };
                    if code != want {
                        return Err(format!(
                            "step {i}: file {k} code db {code:?} model {want:?}"
                        ));
                    }
                }
            }
        }
        prev = cur;
    }
    Ok((states.len() - 1, probes))
}

fn trace_dir() -> PathBuf {
    std::env::var("OO_FM2_TRACES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../formal/traces/file_lifecycle")
        })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn fm2_itf_traces_conform_to_the_transition_function() {
    let dir = trace_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(".itf.json"))
        .collect();
    files.sort();
    if let Some(max) = std::env::var("OO_FM2_MAX")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        files.truncate(max);
    }
    assert!(!files.is_empty(), "no *.itf.json in {}", dir.display());
    let par: usize = std::env::var("OO_FM2_PAR")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    let files = Arc::new(files);
    let next = Arc::new(AtomicUsize::new(0));
    let mut tasks = Vec::new();
    for n in 0..par {
        let (files, next) = (files.clone(), next.clone());
        tasks.push(tokio::spawn(async move {
            let name = format!("oo_docs_cst_{n}");
            {
                let _g = CLUSTER_LOCK.lock().await;
                installed_db(&name).await;
            }
            let pool = pool_runtime(&name, 4).await;
            let drive: String = sqlx::query_scalar(
                "INSERT INTO _oo_docs.drive (id, name, backend, config, max_files) \
                 VALUES (gen_random_uuid(), 'cst', 'fs', '{\"root\":\"/nonexistent\"}'::jsonb, 1000) \
                 RETURNING id::text",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            let mut rig = Rig { pool, drive, n: 0 };
            let (mut traces, mut steps, mut probes) = (0usize, 0usize, 0usize);
            let mut outcome = Ok(());
            loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(path) = files.get(i) else { break };
                let text = std::fs::read_to_string(path).unwrap();
                let trace: Value = serde_json::from_str(&text).unwrap();
                match replay(&mut rig, &trace).await {
                    Ok((s, p)) => {
                        traces += 1;
                        steps += s;
                        probes += p;
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
            drop_db(&name).await;
            outcome.map(|()| (traces, steps, probes))
        }));
    }
    let (mut traces, mut steps, mut probes) = (0, 0, 0);
    let mut errors = Vec::new();
    for t in tasks {
        match t.await.unwrap() {
            Ok((a, b, c)) => {
                traces += a;
                steps += b;
                probes += c;
            }
            Err(e) => errors.push(e),
        }
    }
    eprintln!(
        "conformance_state: {traces} traces, {steps} steps, {probes} refused-transition probes, from {}",
        dir.display()
    );
    assert!(errors.is_empty(), "divergence:\n{}", errors.join("\n"));
    assert!(traces > 0);
}
