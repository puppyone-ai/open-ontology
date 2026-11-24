//! FM5 conformance (80 section 4, card V-S5-01): the review model
//! `formal/quint/review_decide.qnt` (profile `stepFacts`) against the real
//! queue, `review:decide`, accept by source, overlays and the publisher on a
//! throwaway PostgreSQL.
//!
//! Every ITF step is run on the database and, after it, every model slot is
//! compared with the rows: the fact side (`draft` split into eligible / waiting /
//! held by [`classify_drafts`], `published`, `rejected`, `retracted`), the item
//! side (status and `revision`, with the model's rev + 1 because rows start at
//! 1), and the overlay (`fact_assert`, `fact_reject`, `orphaned`, `withdrawn`).
//! `decide` is compared by result (`ok`, `conflict`, `already_decided`,
//! `not_found`), accept by source with the event the caller names as `as_of_seq`.
//!
//! ```text
//! OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test conformance_review -- --ignored --nocapture
//! FORMAL_STEP=stepFacts formal/scripts/formal-traces review_decide review_decide 500 40 0x464d /tmp/fm5-traces
//! OO_FM5_TRACES_DIR=/tmp/fm5-traces OO_FM5_MAX=500 ... (bulk)
//! ```
//!
//! Where the model reads the rows differently from the code: a model step `markWrong`
//! is the SQL of `routes_correct.rs` (the handler is HTTP-only), and `evidenceGone`
//! is the real `withdraw_evidence` followed by the real `sweep_orphans`.
//! Merges and supersessions (kinds 1 and 2 of the model) are not replayed here:
//! they run through the resolver and the temporal pass (`resolve_live.rs`,
//! `temporal_live.rs`); see `fm5_in_use_supersession_leaves_one_notice` for D51.

mod common;
#[path = "itf_support/mod.rs"]
mod itf;

use std::collections::BTreeMap;

use common::CLUSTER_LOCK;
use common::fx::Fx;
use itf::*;
use oo_docs::ingest::files::withdraw_evidence;
use oo_docs::publish::run::PublishOutcome;
use oo_docs::review::decide::{Decision, ItemDecision, decide_by_source, decide_items};
use oo_docs::review::overlay::{insert_rev, sweep_orphans};
use oo_docs::review::queue::{Class, Thresholds, classify_drafts, enqueue};
use oo_docs::store::events::append_event;
use oo_docs::store::rev::bump_drive_rev;
use serde_json::{Value, json};

const FACT: i64 = 0;
const ORPHAN: i64 = 3;

#[derive(Clone, Debug)]
struct Db {
    fact_id: String,
    fact_key: String,
}

struct Rig {
    fx: Fx,
    trace: usize,
    files: [String; 2],
    db: BTreeMap<i64, Db>,
    /// model slot of an orphan item -> the review item id
    orphan_items: BTreeMap<i64, String>,
    seq: BTreeMap<i64, i64>,
}

fn bp_of(conf: i64) -> i32 {
    match conf {
        2 => 9500,
        1 => 6000,
        _ => 3000,
    }
}

impl Rig {
    async fn item_of(&mut self, slot: i64, kind: i64) -> Option<(String, String, i64)> {
        if kind == ORPHAN {
            let id = self.orphan_items.get(&slot)?.clone();
            return sqlx::query_as::<_, (String, String, i64)>(
                "SELECT id::text, status, revision FROM _oo_docs.review_item WHERE id = $1::uuid",
            )
            .bind(id)
            .fetch_optional(&mut self.fx.admin)
            .await
            .unwrap();
        }
        let d = self.db.get(&slot)?;
        sqlx::query_as::<_, (String, String, i64)>(
            "SELECT id::text, status, revision FROM _oo_docs.review_item \
              WHERE kind = 'low_confidence_fact' AND subject->>'fact_id' = $1",
        )
        .bind(&d.fact_id)
        .fetch_optional(&mut self.fx.admin)
        .await
        .unwrap()
    }

    async fn event(&mut self, model_seq: i64) {
        tokio::time::sleep(std::time::Duration::from_millis(3)).await;
        let s = append_event(
            &mut self.fx.rt,
            "review.changed",
            Some(common::fx::DRIVE),
            None,
            "{}",
        )
        .await
        .unwrap();
        self.seq.insert(model_seq, s);
        tokio::time::sleep(std::time::Duration::from_millis(3)).await;
    }

    async fn appear(&mut self, idx: i64, slot: &Value) {
        let src = usize::try_from(int(&slot["src"])).unwrap();
        let conf = int(&slot["conf"]);
        let origin = text(&slot["origin"]).to_owned();
        let path = self.files[src].clone();
        let ids = self
            .fx
            .llm_facts_named(&path, &format!("t{}x{idx}", self.trace), 1, bp_of(conf))
            .await;
        let fid = ids[0].clone();
        let key: String =
            sqlx::query_scalar("SELECT fact_key FROM _oo_docs.fact WHERE id = $1::uuid")
                .bind(&fid)
                .fetch_one(&mut self.fx.admin)
                .await
                .unwrap();
        if origin == "human" {
            // a person's fact: origin human and the overlay that made it (as `edit_fact` writes them)
            sqlx::query("UPDATE _oo_docs.fact SET origin = 'human' WHERE id = $1::uuid")
                .bind(&fid)
                .execute(&mut self.fx.admin)
                .await
                .unwrap();
            insert_rev(
                &mut self.fx.rt,
                "fact_assert",
                &key,
                json!({"fact_id": fid, "human": true}),
                "tester",
                None,
                "active",
                None,
                None,
            )
            .await
            .unwrap();
        }
        self.db.insert(
            idx,
            Db {
                fact_id: fid,
                fact_key: key,
            },
        );
        enqueue(&mut self.fx.rt, Thresholds::default())
            .await
            .unwrap();
        self.event(int(&slot["seq"])).await;
    }

    /// Compare every model slot with the rows.
    async fn compare(&mut self, st: &Value, who: &str) {
        let slots = map_values(&st["slots"]);
        let classes: BTreeMap<String, Class> =
            classify_drafts(&mut self.fx.rt, Thresholds::default())
                .await
                .unwrap()
                .into_iter()
                .map(|c| (c.fact_id, c.class))
                .collect();
        for (idx, s) in &slots {
            let kind = int(&s["kind"]);
            let ist = text(&s["ist"]);
            if kind == FACT && text(&s["fst"]) != "none" {
                let d = self.db[idx].clone();
                let (status, origin): (String, String) =
                    sqlx::query_as("SELECT status, origin FROM _oo_docs.fact WHERE id = $1::uuid")
                        .bind(&d.fact_id)
                        .fetch_one(&mut self.fx.admin)
                        .await
                        .unwrap();
                let class = classes.get(&d.fact_id);
                let fact_side = match (status.as_str(), class) {
                    ("draft", Some(Class::Eligible)) => "eligible",
                    ("draft", _) => "draft",
                    (other, _) => other,
                };
                assert_eq!(
                    fact_side,
                    text(&s["fst"]),
                    "{who}: slot {idx} fact side ({origin})"
                );
                // overlay
                let ov: Vec<(String, String)> = sqlx::query_as(
                    "SELECT kind, status FROM _oo_docs.overlay_current WHERE binds = $1 \
                       AND kind IN ('fact_assert', 'fact_reject') ORDER BY created_at DESC, id DESC",
                )
                .bind(&d.fact_key)
                .fetch_all(&mut self.fx.admin)
                .await
                .unwrap();
                let ov_side = match ov.first() {
                    None => "none",
                    Some((k, s)) if k == "fact_reject" && s == "active" => "reject",
                    Some((_, s)) if s == "orphaned" => "orphaned",
                    Some((_, s)) if s == "withdrawn" => "withdrawn",
                    Some((k, s)) if k == "fact_assert" && s == "active" => "assert",
                    other => panic!("{who}: unexpected overlay {other:?}"),
                };
                let want = match text(&s["ov"]) {
                    "active" => "assert",
                    o => o,
                };
                assert_eq!(ov_side, want, "{who}: slot {idx} overlay");
            }
            if kind == FACT || kind == ORPHAN {
                let item = self.item_of(*idx, kind).await;
                match (ist, item) {
                    ("none", None) => {}
                    (m, Some((_, status, rev))) => {
                        assert_eq!(status, m, "{who}: slot {idx} item status");
                        assert_eq!(rev, int(&s["rev"]) + 1, "{who}: slot {idx} item revision");
                    }
                    (m, None) => panic!("{who}: slot {idx}: model item {m}, no row"),
                }
            }
        }
    }
}

async fn replay(rig: &mut Rig, name: &str, t: &Value) -> usize {
    let states = t["states"].as_array().unwrap();
    let mut steps = 0;
    for (i, st) in states.iter().enumerate().skip(1) {
        let who = format!("{name} step {i}");
        let prev = &states[i - 1];
        let act = text(&st["mbt::actionTaken"]).to_owned();
        let picks = &st["mbt::nondetPicks"];
        let pick =
            |k: &str| -> Option<&Value> { (picks[k]["tag"] == "Some").then(|| &picks[k]["value"]) };
        match act.as_str() {
            "appearKind" => {
                let idx = int(&prev["nxt"]);
                let slot = map_values(&st["slots"])[&idx].clone();
                assert_eq!(int(&slot["kind"]), FACT, "profile stepFacts");
                rig.appear(idx, &slot).await;
            }
            "rescore" => {
                let idx = int(pick("i").unwrap());
                let kind = int(&map_values(&prev["slots"])[&idx]["kind"]);
                let (id, _, _) = rig.item_of(idx, kind).await.expect("item");
                sqlx::query(
                    "UPDATE _oo_docs.review_item SET revision = revision + 1 WHERE id = $1::uuid",
                )
                .bind(&id)
                .execute(&mut rig.fx.admin)
                .await
                .unwrap();
            }
            "decide" => {
                let idx = int(pick("i").unwrap());
                let rv = int(pick("rv").unwrap());
                let acc = pick("acc").unwrap().as_bool().unwrap();
                let slot = map_values(&prev["slots"])[&idx].clone();
                let kind = int(&slot["kind"]);
                let id = match rig.item_of(idx, kind).await {
                    Some((id, _, _)) => id,
                    None => common::fx::uuid(),
                };
                let out = decide_items(
                    &mut rig.fx.rt,
                    "tester",
                    &[ItemDecision {
                        id,
                        revision: rv + 1,
                        decision: if acc {
                            Decision::Accept
                        } else {
                            Decision::Reject
                        },
                        edit: None,
                        note: None,
                    }],
                )
                .await
                .unwrap();
                assert_eq!(
                    out.results[0].status,
                    text(&st["last"]["ok"]),
                    "{who}: decide result"
                );
            }
            "bySource" => {
                let sr = usize::try_from(int(pick("sr").unwrap())).unwrap();
                let ao = int(pick("ao").unwrap());
                let acc = pick("acc").unwrap().as_bool().unwrap();
                let file_id: String =
                    sqlx::query_scalar("SELECT id::text FROM _oo_docs.file WHERE path = $1")
                        .bind(&rig.files[sr])
                        .fetch_one(&mut rig.fx.admin)
                        .await
                        .unwrap();
                let as_of = if ao == 0 { 0 } else { rig.seq[&ao] };
                decide_by_source(&mut rig.fx.rt, "tester", &file_id, &[], acc, Some(as_of))
                    .await
                    .unwrap();
            }
            "publish" => {
                let out = rig.fx.publish().await;
                assert!(
                    matches!(out, PublishOutcome::Published(_)),
                    "{who}: publish with an eligible fact gave {out:?}"
                );
            }
            "evidenceGone" => {
                // the source file goes away: its version's evidence is withdrawn (real code), overlays
                // that lost their evidence become orphans, and the publisher's classify pass retires the rest
                let sr = usize::try_from(int(pick("sr").unwrap())).unwrap();
                let vid: String = sqlx::query_scalar(
                    "SELECT current_version_id::text FROM _oo_docs.file WHERE path = $1",
                )
                .bind(&rig.files[sr])
                .fetch_one(&mut rig.fx.admin)
                .await
                .unwrap();
                withdraw_evidence(&mut rig.fx.rt, &vid, "file_deleted")
                    .await
                    .unwrap();
                let queued = sweep_orphans(&mut rig.fx.rt).await.unwrap();
                enqueue(&mut rig.fx.rt, Thresholds::default())
                    .await
                    .unwrap();
                let first = int(&prev["nxt"]);
                let made = int(&st["nxt"]) - first;
                assert_eq!(i64::from(queued), made, "{who}: orphan items");
                let now = map_values(&st["slots"]);
                for slot in first..int(&st["nxt"]) {
                    let tgt = int(&now[&slot]["tgt"]);
                    let key = rig.db[&tgt].fact_key.clone();
                    let id: String = sqlx::query_scalar(
                        "SELECT id::text FROM _oo_docs.review_item WHERE dedupe_key = $1",
                    )
                    .bind(format!("orphaned_overlay:{key}"))
                    .fetch_one(&mut rig.fx.admin)
                    .await
                    .unwrap();
                    rig.orphan_items.insert(slot, id);
                }
                if made > 0 {
                    rig.event(int(&st["evSeq"])).await;
                }
            }
            "markWrong" => {
                // the SQL of `facts/{id}:correct` mark_wrong (routes_correct.rs)
                let t = int(pick("t").unwrap());
                let d = rig.db[&t].clone();
                let mut tx = sqlx::Connection::begin(&mut rig.fx.rt).await.unwrap();
                let rev = bump_drive_rev(&mut tx, common::fx::DRIVE).await.unwrap();
                insert_rev(
                    &mut tx,
                    "fact_reject",
                    &d.fact_key,
                    json!({"fact_id": d.fact_id, "human": true}),
                    "tester",
                    None,
                    "active",
                    None,
                    Some(rev),
                )
                .await
                .unwrap();
                sqlx::query(
                    "UPDATE _oo_docs.fact SET status = CASE WHEN status = 'published' THEN 'retracted' ELSE 'rejected' END, \
                            retracted_at = CASE WHEN status = 'published' THEN clock_timestamp() ELSE retracted_at END, \
                            revision = revision + 1 WHERE id = $1::uuid",
                )
                .bind(&d.fact_id)
                .execute(&mut *tx)
                .await
                .unwrap();
                tx.commit().await.unwrap();
                // the debounced publish run classifies and retires the item (4.8)
                enqueue(&mut rig.fx.rt, Thresholds::default())
                    .await
                    .unwrap();
            }
            other => panic!("{who}: unexpected action {other}"),
        }
        rig.compare(st, &who).await;
        steps += 1;
    }
    steps
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn fm5_itf_traces_conform_to_review_and_publish() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_fm5";
    let fx = Fx::new(db).await;
    let rig_fx = fx;
    let mut rig = Rig {
        fx: rig_fx,
        trace: 0,
        files: [String::new(), String::new()],
        db: BTreeMap::new(),
        orphan_items: BTreeMap::new(),
        seq: BTreeMap::new(),
    };
    let traces = load_traces("review_decide", "OO_FM5_TRACES_DIR");
    let max: usize = std::env::var("OO_FM5_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);
    let (mut n, mut steps) = (0usize, 0usize);
    for (name, t) in traces.iter().take(max) {
        rig.trace = n;
        // two source files per trace: accept by source must not see the items of earlier traces
        for k in 0..2 {
            let f = format!("Notes/fm5-t{n}-s{k}.md");
            rig.fx.index_md(&f, "# 议题清单\n\n内容\n").await;
            rig.files[k] = f;
        }
        rig.db.clear();
        rig.orphan_items.clear();
        rig.seq.clear();
        steps += replay(&mut rig, name, t).await;
        n += 1;
    }
    eprintln!("FM5 conformance: {n} traces, {steps} steps, all states equal");
    rig.fx.finish(db).await;
}
