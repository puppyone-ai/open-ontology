//! FM10 conformance (80 section 4, card V-S6-03): the freshness model
//! `formal/quint/freshness_token.qnt` (profile `stepLive`) against `drive.rev`,
//! `covers_drive_rev`, the publisher and the compile-time coverage
//! (`share/coverage.rs`) on a throwaway PostgreSQL.
//!
//! A trace gets its own database. Model steps:
//! * `upload(f)` plants a new file version (the trigger takes the next `drive_rev`), the
//!   file is `stored` (pending);
//! * `process(i)` indexes the version through the real indexer (`indexed`, ready) or
//!   fails it (`parse_failed`);
//! * `correct` is a person's correction (the SQL of `facts/{id}:correct`: `drive_rev`
//!   taken in the transaction that writes the overlay and retracts a published fact);
//! * `beginPublish` / `commitPublish`: the real publisher. A file that finished after
//!   `beginPublish` is held back (`extracting`) during the run so it is not in the batch,
//!   as in the model; `abortPublish` does nothing;
//! * `compile` calls `coverage::prepare` with `min_drive_rev`, the scope and
//!   `require_complete` of the model and no wait (the model's timeout flag is "the wait
//!   is over").
//!
//! After every step the Drive's rev, the stored `covers_drive_rev` of the latest version
//! and (after `compile`) the coverage (`covered_rev`, pending, failed, processed, in
//! scope) or the 409 are compared with the model; `rev` and `covers` are shifted by the
//! rev the Drive had before the trace.
//!
//! ```text
//! OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test conformance_freshness -- --ignored --nocapture
//! FORMAL_STEP=stepLive formal/scripts/formal-traces freshness_token freshness_token 150 40 0x464d /tmp/fm10-traces
//! OO_FM10_TRACES_DIR=/tmp/fm10-traces OO_FM10_MAX=150 ... (bulk)
//! ```

mod common;
#[path = "itf_support/mod.rs"]
mod itf;

use std::collections::{BTreeMap, BTreeSet};

use common::CLUSTER_LOCK;
use common::fx::{DRIVE, Fx};
use itf::*;
use oo_docs::api::error::code;
use oo_docs::publish::run::PublishOutcome;
use oo_docs::review::overlay::insert_rev;
use oo_docs::share::coverage::{Freshness, prepare};
use oo_docs::store::rev::bump_drive_rev;
use serde_json::{Value, json};

fn path(f: i64) -> String {
    format!("Notes/F{f}.md")
}

fn src(f: i64, v: usize) -> String {
    format!("# 文件 {f} 版本 {v}\n\n负责人：陈子墨\n\n内容 {f} {v}\n")
}

struct Rig {
    fx: Fx,
    db: String,
    /// rev the Drive had before the trace
    r0: i64,
    /// model file -> (path, plant of the latest version, versions so far)
    planted: BTreeMap<i64, (common::fx::Planted, usize)>,
    /// published base facts, one per correction
    base: Vec<String>,
    used_base: usize,
    batch: BTreeSet<i64>,
    lagging: usize,
}

impl Rig {
    async fn drive_rev(&mut self) -> i64 {
        self.fx
            .scalar(&format!(
                "SELECT rev FROM _oo_docs.drive WHERE id = '{DRIVE}'"
            ))
            .await
    }

    async fn set_state(&mut self, f: i64, state: &str) {
        sqlx::query("UPDATE _oo_docs.file SET state = $2 WHERE path = $1")
            .bind(path(f))
            .bind(state)
            .execute(&mut self.fx.admin)
            .await
            .unwrap();
    }
}

/// What the engine's publish handler does after a run (`PublishHandler::promote`, jobs/stages.rs): an
/// `indexed` file with no draft fact left goes to `published`, whether or not the run made a version.
async fn promote(pool: &sqlx::PgPool, n: Option<i64>) {
    use oo_docs::state::{FileState, Transition, transition};
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT f.id::text, fv.id::text FROM _oo_docs.file f \
           JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
          WHERE f.state = 'indexed' AND f.deleted_at IS NULL \
            AND NOT EXISTS (SELECT 1 FROM _oo_docs.evidence e JOIN _oo_docs.fact ft ON ft.id = e.target_id \
                             WHERE e.target_kind = 'fact' AND e.version_id = fv.id \
                               AND e.withdrawn_at IS NULL AND ft.status = 'draft') \
            AND NOT EXISTS (SELECT 1 FROM _oo_docs.evidence e JOIN _oo_docs.alias a ON a.id = e.target_id \
                             WHERE e.target_kind = 'alias' AND e.version_id = fv.id \
                               AND e.withdrawn_at IS NULL AND a.status = 'draft') ORDER BY 1",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let detail = json!({ "ontology_version": n }).to_string();
    for (file_id, version_id) in rows {
        let _ = transition(
            pool,
            &Transition {
                file_id: &file_id,
                expect_from: Some(FileState::Indexed),
                to: FileState::Published,
                code: None,
                retryable: false,
                version_id: Some(&version_id),
                attempt: None,
                detail_json: Some(&detail),
            },
        )
        .await;
    }
}

async fn replay(rig: &mut Rig, name: &str, t: &Value) -> usize {
    let states = t["states"].as_array().unwrap();
    let mut steps = 0;
    let pool = common::pool_runtime(&rig.db, 2).await;
    for (i, st) in states.iter().enumerate().skip(1) {
        let who = format!("{name} step {i}");
        let prev = &states[i - 1];
        let picks = &st["mbt::nondetPicks"];
        let pick =
            |k: &str| -> Option<&Value> { (picks[k]["tag"] == "Some").then(|| &picks[k]["value"]) };
        let chg_prev = map_values(&prev["chg"]);
        match text(&st["mbt::actionTaken"]) {
            "upload" => {
                let f = int(pick("f").unwrap());
                let n = rig.planted.get(&f).map_or(0, |(_, n)| *n) + 1;
                let p = if n == 1 {
                    rig.fx.plant_md(&path(f), &src(f, n)).await
                } else {
                    rig.fx.plant_version(&path(f), &src(f, n)).await
                };
                rig.planted.insert(f, (p, n));
                rig.set_state(f, "stored").await;
            }
            "process" => {
                let i = int(pick("i").unwrap());
                let bad = pick("bad").unwrap().as_bool().unwrap();
                let f = int(&chg_prev[&i]["file"]);
                if bad {
                    rig.set_state(f, "parse_failed").await;
                } else {
                    let p = &rig.planted[&f].0;
                    let planted = common::fx::Planted {
                        file_id: p.file_id.clone(),
                        version_id: p.version_id.clone(),
                        b3: p.b3.clone(),
                        ir: p.ir.clone(),
                    };
                    rig.fx.index_planted(&path(f), &planted).await;
                    rig.set_state(f, "indexed").await;
                }
            }
            "correct" => {
                // the SQL of `facts/{id}:correct` mark_wrong (routes_correct.rs)
                let fid = rig.base[rig.used_base].clone();
                rig.used_base += 1;
                let key: String =
                    sqlx::query_scalar("SELECT fact_key FROM _oo_docs.fact WHERE id = $1::uuid")
                        .bind(&fid)
                        .fetch_one(&mut rig.fx.admin)
                        .await
                        .unwrap();
                let mut tx = sqlx::Connection::begin(&mut rig.fx.rt).await.unwrap();
                let rev = bump_drive_rev(&mut tx, DRIVE).await.unwrap();
                insert_rev(
                    &mut tx,
                    "fact_reject",
                    &key,
                    json!({"fact_id": fid, "human": true}),
                    "tester",
                    None,
                    "active",
                    None,
                    Some(rev),
                )
                .await
                .unwrap();
                sqlx::query(
                    "UPDATE _oo_docs.fact SET status = 'retracted', retracted_at = clock_timestamp(), \
                            revision = revision + 1 WHERE id = $1::uuid",
                )
                .bind(&fid)
                .execute(&mut *tx)
                .await
                .unwrap();
                tx.commit().await.unwrap();
            }
            "beginPublish" => {
                rig.batch = set_ints(&st["batch"]);
            }
            "abortPublish" => rig.batch.clear(),
            "commitPublish" => {
                // files that became ready after beginPublish are not in the batch: hold them back
                let chg = map_values(&prev["chg"]);
                let mut held: Vec<i64> = Vec::new();
                for (i, c) in &chg {
                    if text(&c["st"]) == "ready" && int(&c["file"]) >= 0 && !rig.batch.contains(i) {
                        let f = int(&c["file"]);
                        rig.set_state(f, "extracting").await;
                        held.push(f);
                    }
                }
                let out = rig.fx.publish().await;
                let n = match &out {
                    PublishOutcome::Published(v) => Some(v.n),
                    PublishOutcome::NoChange { .. } => None,
                    PublishOutcome::Busy => panic!("{who}: publisher busy"),
                };
                promote(&pool, n).await;
                for f in held {
                    rig.set_state(f, "indexed").await;
                }
                rig.batch.clear();
            }
            "compile" => {
                let mr = int(pick("mr").unwrap());
                let sc = set_ints(pick("sc").unwrap());
                let rc = pick("rc").unwrap().as_bool().unwrap();
                let prefixes: Vec<String> = if sc.is_empty() {
                    vec!["Notes/none".to_owned()]
                } else {
                    sc.iter().map(|f| path(*f)).collect()
                };
                let got = prepare(
                    &pool,
                    DRIVE,
                    &prefixes,
                    &Freshness {
                        min_drive_rev: Some(mr + rig.r0),
                        wait_ms: 0,
                        require_complete: rc,
                    },
                )
                .await;
                let rec = &st["rec"];
                let want = text(&rec["res"]);
                match got {
                    Ok(c) => {
                        assert_eq!(want, "ok", "{who}: engine compiled, model says {want}");
                        assert_eq!(
                            c.covered_rev - rig.r0,
                            int(&rec["covered"]),
                            "{who}: covered_rev"
                        );
                        let by = |l: &Vec<(String, String)>| -> BTreeSet<i64> {
                            l.iter()
                                .map(|(p, _)| {
                                    p.trim_start_matches("Notes/F")
                                        .trim_end_matches(".md")
                                        .parse()
                                        .unwrap()
                                })
                                .collect()
                        };
                        assert_eq!(by(&c.pending), set_ints(&rec["pending"]), "{who}: pending");
                        assert_eq!(by(&c.failed), set_ints(&rec["failed"]), "{who}: failed");
                        assert_eq!(
                            c.processed,
                            u64::try_from(int(&rec["processed"])).unwrap(),
                            "{who}: processed"
                        );
                        assert_eq!(
                            c.in_scope,
                            u64::try_from(int(&rec["inScope"])).unwrap(),
                            "{who}: in scope"
                        );
                        // nothing outside the scope is named (D26)
                        for (p, _) in c.pending.iter().chain(c.failed.iter()) {
                            assert!(prefixes.contains(p), "{who}: {p} is out of scope");
                        }
                    }
                    Err(e) => {
                        assert_eq!(e.code, code::PACK_COVERAGE_INCOMPLETE, "{who}: {e:?}");
                        assert_eq!(
                            want, "incomplete409",
                            "{who}: engine refused, model says {want}"
                        );
                    }
                }
            }
            other => panic!("{who}: unexpected action {other}"),
        }
        // the token, after every step
        assert_eq!(
            rig.drive_rev().await - rig.r0,
            int(&st["rev"]),
            "{who}: drive.rev"
        );
        let stored: Option<i64> = sqlx::query_scalar(
            "SELECT covers_drive_rev FROM _oo_docs.ontology_version ORDER BY n DESC LIMIT 1",
        )
        .fetch_optional(&mut rig.fx.admin)
        .await
        .unwrap();
        // The stored number is a lower bound of the model's: files with nothing left to publish are
        // promoted after the run (and a run without a version stores nothing), the live measure the
        // compile uses is exact and is compared with the model at every `compile` step above.
        let stored = stored.unwrap_or(rig.r0) - rig.r0;
        assert!(
            stored <= int(&st["covers"]),
            "{who}: stored covers_drive_rev {stored} is ahead of the model's {}",
            int(&st["covers"])
        );
        if stored < int(&st["covers"]) {
            rig.lagging += 1;
        }
        steps += 1;
    }
    steps
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn fm10_itf_traces_conform_to_drive_rev_and_coverage() {
    let _g = CLUSTER_LOCK.lock().await;
    let traces = load_traces("freshness_token", "OO_FM10_TRACES_DIR");
    let max: usize = std::env::var("OO_FM10_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);
    let (mut n, mut steps, mut lagging) = (0usize, 0usize, 0usize);
    for (name, t) in traces.iter().take(max) {
        let db = format!("oo_docs_fm10_{n}");
        let mut fx = Fx::new(&db).await;
        // a published base: one version, and six facts a correction can retract
        fx.index_md("Notes/base.md", "# 基础\n\n内容\n").await;
        let base = fx.llm_facts("Notes/base.md", 6, 9500).await;
        let _ = fx.publish().await;
        let mut rig = Rig {
            fx,
            db: db.clone(),
            r0: 0,
            planted: BTreeMap::new(),
            base,
            used_base: 0,
            batch: BTreeSet::new(),
            lagging: 0,
        };
        rig.r0 = rig.drive_rev().await;
        steps += replay(&mut rig, name, t).await;
        lagging += rig.lagging;
        rig.fx.finish(&db).await;
        n += 1;
    }
    eprintln!(
        "FM10 conformance: {n} traces, {steps} steps; drive.rev and the live coverage equal at every step, \
         stored covers_drive_rev never ahead ({lagging} steps behind it)"
    );
}
