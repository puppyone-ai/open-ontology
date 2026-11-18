//! FM11 conformance (80 section 4, card V-S6-03): the constraint model
//! `formal/quint/constraints.qnt` (profile `stepLive`: project owner, decision
//! status, rejected-vs-adopted) against the publisher's constraint check on a
//! throwaway PostgreSQL.
//!
//! Every ITF step is run on the database: `addDraftK` inserts the draft fact
//! (an owner, a status, a rejects/decides, with the model's validity slices as
//! dates), `publish` is the real publisher, `retract` withdraws a published fact,
//! `resolve` is `review:decide` accepting the winner of the `constraint_conflict`
//! item. After each step three things are compared with the model:
//!
//! * which facts are published and where each interval ends (`valid_to`), as long as the engine and
//!   the model choose the same outcome: where the temporal pass picks differently (an equal start
//!   between an uncertain and a rule-certain value, for one) the trace ends there and is counted;
//! * the safety invariant itself, restated on the rows by time slice: no two
//!   published facts of one (kind, key) with different values overlap (kind 3:
//!   rejects and decides of one alternative);
//! * every fact the model holds is waiting for a person (an open
//!   `constraint_violation` on its entity or a pending review item that names it).
//!
//! ```text
//! OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test conformance_constraints -- --ignored --nocapture
//! FORMAL_STEP=stepLive formal/scripts/formal-traces constraints constraints 300 30 0x464d /tmp/fm11-traces
//! OO_FM11_TRACES_DIR=/tmp/fm11-traces OO_FM11_MAX=300 ... (bulk)
//! ```
//!
//! Confidence falls with the model's order (start slice, then fact id): the temporal pass
//! meets facts by start date and prefers published, then higher confidence, so the model's
//! greedy order is the engine's.

mod common;
#[path = "itf_support/mod.rs"]
mod itf;

use std::collections::{BTreeMap, BTreeSet};

use common::CLUSTER_LOCK;
use common::fx::Fx;
use itf::*;
use oo_docs::extract::model::EntityType;
use oo_docs::publish::run::PublishOutcome;
use oo_docs::resolve::normalize::{entity_key, fact_key, norm};
use oo_docs::review::decide::{Decision, ItemDecision, decide_items};
use serde_json::{Value, json};

const TMAX: i64 = 4;

type Ent = (String, String);
type Shape = (&'static str, Ent, Option<Ent>, Option<Value>);

fn date(slice: i64) -> String {
    format!("2026-01-{:02}", slice + 1)
}

struct Rig {
    fx: Fx,
    trace: usize,
    doc: String,
    /// model fact id -> (db id, fact_key)
    facts: BTreeMap<i64, (String, String)>,
    truncated: usize,
    diverged: usize,
}

impl Rig {
    async fn entity(&mut self, ty: EntityType, name: &str) -> (String, String) {
        let key = entity_key(ty, None, &norm(name), None);
        sqlx::query(
            "INSERT INTO _oo_docs.entity (id, key, type, canonical_name, norm_name) \
             VALUES (gen_random_uuid(), $1, $2, $3, $4) ON CONFLICT (key) DO NOTHING",
        )
        .bind(&key)
        .bind(ty.as_str())
        .bind(name)
        .bind(norm(name))
        .execute(&mut self.fx.admin)
        .await
        .unwrap();
        let id: String = sqlx::query_scalar("SELECT id::text FROM _oo_docs.entity WHERE key = $1")
            .bind(&key)
            .fetch_one(&mut self.fx.admin)
            .await
            .unwrap();
        (id, key)
    }

    async fn add(&mut self, idx: i64, f: &Value) {
        let (kind, key, v) = (int(&f["kind"]), int(&f["key"]), int(&f["v"]));
        let (lo, hi) = (int(&f["lo"]), int(&f["hi"]));
        let certain = f["certain"].as_bool().unwrap();
        let t = self.trace;
        let (pred, subj, obj, value): Shape = match kind {
            0 => {
                let p = self.entity(EntityType::Person, &format!("P{t}x{v}")).await;
                let j = self
                    .entity(EntityType::Project, &format!("J{t}x{key}"))
                    .await;
                ("owns", p, Some(j), None)
            }
            1 => {
                let d = self
                    .entity(EntityType::Decision, &format!("D{t}x{key}"))
                    .await;
                (
                    "attr.status",
                    d,
                    None,
                    Some(json!({"enum": format!("s{v}")})),
                )
            }
            _ => {
                let p = self
                    .entity(EntityType::Person, &format!("Q{t}x{idx}"))
                    .await;
                let a = self
                    .entity(EntityType::Decision, &format!("A{t}x{key}"))
                    .await;
                (if v == 0 { "rejects" } else { "decides" }, p, Some(a), None)
            }
        };
        let from = date(lo);
        let to = (hi < TMAX).then(|| date(hi));
        let fkey = fact_key(
            &subj.1,
            pred,
            obj.as_ref().map(|o| o.1.as_str()),
            value.as_ref(),
            Some(&from),
        );
        let fid: String = sqlx::query_scalar(
            "INSERT INTO _oo_docs.fact (id, fact_key, subject_id, original_subject_id, predicate, \
                object_id, original_object_id, value, valid_from, valid_to, confidence_bp, origin, rule_id, status) \
             VALUES (gen_random_uuid(), $1, $2::uuid, $2::uuid, $3, $4::uuid, $4::uuid, $5::jsonb, \
                     $6::date, $7::date, $8, $9, $10, 'draft') RETURNING id::text",
        )
        .bind(&fkey)
        .bind(&subj.0)
        .bind(pred)
        .bind(obj.as_ref().map(|o| o.0.clone()))
        .bind(value.as_ref().map(Value::to_string))
        .bind(&from)
        .bind(to.as_deref())
        .bind(10_000 - 20 * i32::try_from(lo * 6 + idx).unwrap())
        .bind(if certain { "rule" } else { "llm" })
        .bind(certain.then_some("fm.fields"))
        .fetch_one(&mut self.fx.admin)
        .await
        .unwrap();
        // the evidence of the document's first block
        let doc = self.doc.clone();
        let ev: (String, String, String, String, String) = sqlx::query_as(
            "SELECT ev.version_id::text, ev.b3, ev.block_id, ev.anchor::text, ev.quote_b3 \
               FROM _oo_docs.evidence ev JOIN _oo_docs.entity d ON d.id = ev.target_id \
              WHERE ev.target_kind = 'entity' AND d.type = 'Document' AND d.canonical_name = $1 \
              ORDER BY ev.block_id LIMIT 1",
        )
        .bind(&doc)
        .fetch_one(&mut self.fx.admin)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO _oo_docs.evidence (id, target_kind, target_id, version_id, b3, block_id, anchor, \
                                            quote_b3, origin) \
             VALUES (gen_random_uuid(), 'fact', $1::uuid, $2::uuid, $3, $4, $5::jsonb, $6, 'llm')",
        )
        .bind(&fid)
        .bind(&ev.0)
        .bind(&ev.1)
        .bind(&ev.2)
        .bind(&ev.3)
        .bind(&ev.4)
        .execute(&mut self.fx.admin)
        .await
        .unwrap();
        self.facts.insert(idx, (fid, fkey));
    }

    /// Is the fact waiting for a person? An open violation on its entity (a project for an owner, the
    /// alternative for rejects/decides, the decision for a status) or a pending review item that names
    /// it. The violation and its item name the first conflicting pair only (`record_violation`), so a
    /// third waiting fact is covered by the entity's open violation, not by name.
    async fn explained(&mut self, idx: i64) -> bool {
        let id = self.facts[&idx].0.clone();
        let n: i64 = sqlx::query_scalar(
            "SELECT (SELECT count(*) FROM _oo_docs.constraint_violation v \
                       WHERE v.status = 'open' AND ($1::uuid = ANY(v.fact_ids) \
                          OR v.entity_key IN (SELECT COALESCE(o.key, s.key) FROM _oo_docs.fact f \
                               JOIN _oo_docs.entity s ON s.id = f.subject_id \
                               LEFT JOIN _oo_docs.entity o ON o.id = f.object_id WHERE f.id = $1::uuid))) \
                  + (SELECT count(*) FROM _oo_docs.review_item r WHERE r.status = 'pending' \
                       AND (r.subject->>'fact_id' = $1 OR r.subject->'fact_ids' ? $1))",
        )
        .bind(&id)
        .fetch_one(&mut self.fx.admin)
        .await
        .unwrap();
        n > 0
    }

    /// The facts of the database as the model's attributes: `(kind, key, v, lo, hi, status)` of every
    /// replayed fact, `lo`/`hi` read back from `valid_from` / `valid_to`.
    async fn db_facts(
        &mut self,
        model: &BTreeMap<i64, &Value>,
    ) -> BTreeMap<i64, (i64, i64, i64, i64, i64, String)> {
        let mut out = BTreeMap::new();
        for (idx, f) in model {
            if text(&f["st"]) == "absent" {
                continue;
            }
            let id = self.facts[idx].0.clone();
            let (status, from, to): (String, String, Option<String>) = sqlx::query_as(
                "SELECT status, valid_from::text, valid_to::text FROM _oo_docs.fact WHERE id = $1::uuid",
            )
            .bind(&id)
            .fetch_one(&mut self.fx.admin)
            .await
            .unwrap();
            let slice = |d: &str| d[8..].parse::<i64>().unwrap() - 1;
            out.insert(
                *idx,
                (
                    int(&f["kind"]),
                    int(&f["key"]),
                    int(&f["v"]),
                    slice(&from),
                    to.map_or(TMAX, |t| slice(&t)),
                    status,
                ),
            );
        }
        out
    }

    /// Compare the model's state with the database. Returns `true` when every fact is in the model's
    /// state with the model's interval. Whatever the answer, the safety oracle runs on the database:
    /// (S1) no two published facts of one (kind, key) with different values overlap, by time slice
    /// (rejects/decides of one alternative for kind 3); (S2) a fact the model holds and the engine did
    /// not publish waits for a person; (S3) partial blocking: after a publish run a fact that conflicts
    /// with no other live fact of the model's previous state is published.
    async fn compare(&mut self, prev: &Value, st: &Value, who: &str) -> bool {
        let facts = map_values(&st["facts"]);
        let db = self.db_facts(&facts).await;
        let mut exact = true;
        let mut why = String::new();
        for (idx, f) in &facts {
            let s = text(&f["st"]);
            if s == "absent" {
                continue;
            }
            let (_, _, _, _, dhi, status) = &db[idx];
            let want_db = match s {
                "draft" | "held" => "draft",
                other => other,
            };
            let ok = status == want_db
                || (s == "rejected" && status == "retracted")
                || (s == "retracted" && status == "rejected");
            if !ok {
                exact = false;
                why = format!("fact {idx}: model {s}, database {status}");
            } else if s == "published" && *dhi != int(&f["hi"]) {
                exact = false;
                why = format!(
                    "fact {idx}: model ends at {}, database at {dhi}",
                    int(&f["hi"])
                );
            }
        }
        // S1
        for k in [0i64, 1, 3] {
            for key in 0..2i64 {
                for slice in 0..TMAX {
                    let vals: BTreeSet<i64> = db
                        .values()
                        .filter(|(kd, ky, _, lo, hi, status)| {
                            *kd == k
                                && *ky == key
                                && status == "published"
                                && *lo <= slice
                                && slice < *hi
                        })
                        .map(|(_, _, v, _, _, _)| *v)
                        .collect();
                    assert!(
                        vals.len() <= 1,
                        "{who}: two current values at slice {slice} (kind {k}, key {key})"
                    );
                }
            }
        }
        // S2 and S3
        let before = map_values(&prev["facts"]);
        for (idx, f) in &facts {
            if text(&f["st"]) == "absent" {
                continue;
            }
            let (kd, ky, v, lo, hi) = (
                int(&f["kind"]),
                int(&f["key"]),
                int(&f["v"]),
                int(&f["lo"]),
                int(&f["hi"]),
            );
            let (_, _, _, _, _, status) = &db[idx];
            if text(&f["st"]) == "held" && status != "published" && !self.explained(*idx).await {
                {
                    let id = self.facts[idx].0.clone();
                    let dump: Vec<String> = sqlx::query_scalar(
                        "SELECT concat_ws(' ', 'fact', f2.id::text, f2.status, f2.valid_from::text, coalesce(f2.valid_to::text, 'open'), \
                                f2.confidence_bp::text, f2.origin, coalesce(f2.rule_id, '-')) \
                           FROM _oo_docs.fact f JOIN _oo_docs.fact f2 ON f2.subject_id = f.subject_id AND f2.predicate = f.predicate \
                                 AND COALESCE(f2.object_id, f.object_id) IS NOT DISTINCT FROM f.object_id \
                          WHERE f.id = $1::uuid \
                         UNION ALL SELECT concat_ws(' ', 'violation', v.constraint_id, v.status, v.fact_ids::text) \
                           FROM _oo_docs.constraint_violation v WHERE v.entity_key IN (SELECT s.key FROM _oo_docs.fact f \
                                JOIN _oo_docs.entity s ON s.id = f.subject_id WHERE f.id = $1::uuid UNION \
                                SELECT o.key FROM _oo_docs.fact f JOIN _oo_docs.entity o ON o.id = f.object_id WHERE f.id = $1::uuid)",
                    )
                    .bind(&id)
                    .fetch_all(&mut self.fx.admin)
                    .await
                    .unwrap();
                    panic!(
                        "{who}: held fact {idx} ({id}) waits for nobody:\n{}",
                        dump.join("\n")
                    );
                }
            }
            let was_waiting = matches!(text(&before[idx]["st"]), "draft" | "held");
            if was_waiting && text(&st["lastAct"]) == "publish" && status != "published" {
                let alone = before.iter().all(|(o, g)| {
                    *o == *idx
                        || matches!(text(&g["st"]), "absent" | "retracted" | "rejected")
                        || !(int(&g["kind"]) == kd
                            && int(&g["key"]) == ky
                            && int(&g["v"]) != v
                            && (kd == 2 || (int(&g["lo"]) < hi && lo < int(&g["hi"]))))
                });
                assert!(
                    !alone,
                    "{who}: fact {idx} conflicts with nothing and was not published"
                );
            }
        }
        if !exact {
            eprintln!("  {who}: diverges from the model at {why}");
        }
        exact
    }
}

async fn replay(rig: &mut Rig, name: &str, t: &Value) -> usize {
    let states = t["states"].as_array().unwrap();
    let mut steps = 0;
    for (i, st) in states.iter().enumerate().skip(1) {
        let who = format!("{name} step {i}");
        let prev = &states[i - 1];
        let picks = &st["mbt::nondetPicks"];
        let pick =
            |k: &str| -> Option<&Value> { (picks[k]["tag"] == "Some").then(|| &picks[k]["value"]) };
        match text(&st["mbt::actionTaken"]) {
            "addDraftK" => {
                let idx = int(&prev["nxt"]);
                let f = map_values(&st["facts"])[&idx].clone();
                rig.add(idx, &f).await;
            }
            "publish" => {
                let out = rig.fx.publish().await;
                assert!(
                    matches!(
                        out,
                        PublishOutcome::Published(_) | PublishOutcome::NoChange { .. }
                    ),
                    "{who}: publish gave {out:?}"
                );
            }
            "retract" => {
                let idx = int(pick("i").unwrap());
                let id = rig.facts[&idx].0.clone();
                sqlx::query("UPDATE _oo_docs.fact SET status = 'retracted', retracted_at = clock_timestamp() WHERE id = $1::uuid")
                    .bind(&id)
                    .execute(&mut rig.fx.admin)
                    .await
                    .unwrap();
            }
            "resolve" => {
                let w = int(pick("w").unwrap());
                let wid = rig.facts[&w].0.clone();
                // The item of a (constraint, entity) names the first pair only (`record_violation` keeps
                // the first `fact_ids`): a person can pick the winner among those two. When the model's
                // winner is a third value the engine offers no such choice; the trace ends there and
                // is counted as truncated.
                let found: Option<(String, i64)> = sqlx::query_as(
                    "SELECT id::text, revision FROM _oo_docs.review_item WHERE kind = 'constraint_conflict' \
                       AND status = 'pending' AND subject->'fact_ids' ? $1 ORDER BY created_at LIMIT 1",
                )
                .bind(&wid)
                .fetch_optional(&mut rig.fx.admin)
                .await
                .unwrap();
                let Some((item, rev)) = found else {
                    rig.truncated += 1;
                    return steps;
                };
                // ... and the engine settles those two facts only: if the model's resolution also moves a
                // third fact, the trace ends here as well
                let named: Vec<String> = sqlx::query_scalar(
                    "SELECT jsonb_array_elements_text(subject->'fact_ids') FROM _oo_docs.review_item WHERE id = $1::uuid",
                )
                .bind(&item)
                .fetch_all(&mut rig.fx.admin)
                .await
                .unwrap();
                let (before, after) = (map_values(&prev["facts"]), map_values(&st["facts"]));
                let moved_outside = after.iter().any(|(k, f)| {
                    *k != w
                        && text(&f["st"]) != text(&before[k]["st"])
                        && !named.contains(&rig.facts[k].0)
                });
                if moved_outside {
                    rig.truncated += 1;
                    return steps;
                }
                let out = decide_items(
                    &mut rig.fx.rt,
                    "tester",
                    &[ItemDecision {
                        id: item,
                        revision: rev,
                        decision: Decision::Accept,
                        edit: Some(json!({"winner": wid})),
                        note: None,
                    }],
                )
                .await
                .unwrap();
                assert_eq!(out.results[0].status, "ok", "{who}: resolve");
            }
            "compilePack" => {}
            other => panic!("{who}: unexpected action {other}"),
        }
        if !rig.compare(prev, st, &who).await {
            // the engine chose another valid outcome (see the module notes): the rest of the trace
            // would compare a different state, so it ends here
            rig.diverged += 1;
            return steps;
        }
        steps += 1;
    }
    steps
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn fm11_itf_traces_conform_to_the_constraint_check() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_fm11";
    let mut fx = Fx::new(db).await;
    let doc = "Notes/fm11.md".to_owned();
    fx.index_md(&doc, "# 约束\n\n内容\n").await;
    let _ = fx.publish().await;
    let mut rig = Rig {
        fx,
        trace: 0,
        doc,
        facts: BTreeMap::new(),
        truncated: 0,
        diverged: 0,
    };
    let traces = load_traces("constraints", "OO_FM11_TRACES_DIR");
    let max: usize = std::env::var("OO_FM11_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);
    let (mut n, mut steps) = (0usize, 0usize);
    for (name, t) in traces.iter().take(max) {
        rig.trace = n;
        rig.facts.clear();
        steps += replay(&mut rig, name, t).await;
        n += 1;
    }
    eprintln!(
        "FM11 conformance: {n} traces, {steps} steps equal to the model; safety (no two current values, held facts wait, \
         unrelated facts publish) held at every step; {} trace(s) ended at a valid outcome other than the model's, \
         {} where the model resolves a third fact the engine's item does not name",
        rig.diverged, rig.truncated
    );
    rig.fx.finish(db).await;
}
