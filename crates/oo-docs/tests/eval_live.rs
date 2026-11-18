//! Live evaluation of the whole pipeline on the golden set (L4: precision and
//! recall per core type, model on and off; 30 L4 acceptance, 10 12.6).
//!
//! A real `oo-docs-engine`, the real Docling sidecar container and (with
//! `OO_EVAL_MODEL=1`) the real model egress process: the engine is configured
//! through `PUT /v1/docs/config/model` with `file:` references only. The test
//! writes the engine's own output (entities, aliases, facts, with anchors) in
//! the `oo.eval.docs-labels/v1` format for `oo-eval docs`.
//!
//! ```text
//! OO_DOCS_TEST_PG_PORT=<port> OO_EVAL_OUT=<dir> [OO_EVAL_MODEL=1 \
//!   OO_EVAL_ENDPOINT_FILE=<path> OO_EVAL_KEY_FILE=<path> OO_EVAL_MODEL_ID=<id>] \
//!   cargo test -p oo-docs --locked --test eval_live -- --ignored --nocapture
//! ```
//!
//! The endpoint and key are only ever paths here; nothing reads or prints them.

mod common;
mod sidecar_support;
#[path = "sidecar_support/stack.rs"]
mod stack;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sidecar_support::testing_root;
use sqlx::Row;
use stack::{Stack, copy_tree};

const CORE: [&str; 8] = [
    "Person",
    "Organization",
    "Project",
    "Decision",
    "Document",
    "Topic",
    "Event",
    "Term",
];
const PREDICATES: [&str; 8] = [
    "owns",
    "member_of",
    "decides",
    "supersedes",
    "rejects",
    "mentions",
    "about",
    "tried_and_failed",
];

/// The label fragment of an anchor (`oo.eval.docs-labels/v1`).
fn frag(a: &Value) -> Option<(String, Option<Value>)> {
    let n = |k: &str| a[k].as_i64();
    match a["kind"].as_str()? {
        "text" => Some((format!("#L{}-{}", n("line_start")?, n("line_end")?), None)),
        "docx" => match (a["table_index"].as_i64(), a["cell_range"].as_str()) {
            (Some(t), Some(r)) => Some((format!("#T{}!{r}", t + 1), None)),
            _ => Some((format!("#¶{}-{}", n("para_start")?, n("para_end")?), None)),
        },
        "slide" => Some((format!("#s{}", n("slide")?), None)),
        "pdf" => Some((format!("#p{}", n("page")?), Some(a["bbox"].clone()))),
        "cells" => {
            let range = a["range"].as_str()?;
            match a["sheet"].as_str() {
                Some(sheet) => Some((format!("#'{}'!{range}", sheet.replace('\'', "''")), None)),
                None => {
                    let row: String = range
                        .trim_start_matches(|c: char| c.is_ascii_alphabetic())
                        .chars()
                        .take_while(char::is_ascii_digit)
                        .collect();
                    Some((format!("#R{row}"), None))
                }
            }
        }
        _ => None,
    }
}

type Anchors = BTreeMap<String, Vec<(String, String, Option<Value>)>>;

fn anchor_json(list: &[(String, String, Option<Value>)]) -> Vec<Value> {
    let mut out: Vec<Value> = list
        .iter()
        .map(|(file, frag, bbox)| {
            let mut v = json!({"file": format!("docs/{file}"), "frag": frag});
            if let Some(b) = bbox {
                v["bbox"] = b.clone();
            }
            v
        })
        .collect();
    out.sort_by_key(Value::to_string);
    out.dedup();
    out
}

async fn export(s: &Stack, dir: &std::path::Path) -> (usize, usize) {
    // Anchors per target (fact, alias or entity), live evidence only.
    let rows = sqlx::query(
        "SELECT e.target_id::text AS t, fv.path_at_version AS p, e.anchor::text AS a \
           FROM _oo_docs.evidence e JOIN _oo_docs.file_version fv ON fv.id = e.version_id \
          WHERE e.withdrawn_at IS NULL",
    )
    .fetch_all(&s.pool)
    .await
    .unwrap();
    let mut by_target: Anchors = BTreeMap::new();
    for r in rows {
        let a: Value = serde_json::from_str(&r.get::<String, _>("a")).unwrap();
        if let Some((f, bbox)) = frag(&a) {
            by_target
                .entry(r.get("t"))
                .or_default()
                .push((r.get("p"), f, bbox));
        }
    }
    let ents = sqlx::query(
        "SELECT id::text AS id, type, subkind, canonical_name FROM _oo_docs.entity \
          WHERE status = 'active' AND type <> 'Thing' ORDER BY canonical_name, id",
    )
    .fetch_all(&s.pool)
    .await
    .unwrap();
    let aliases = sqlx::query(
        "SELECT entity_id::text AS e, alias, id::text AS id FROM _oo_docs.alias \
          WHERE status IN ('draft', 'published') ORDER BY alias",
    )
    .fetch_all(&s.pool)
    .await
    .unwrap();
    let facts = sqlx::query(
        "SELECT f.id::text AS id, COALESCE(es.merged_into, es.id)::text AS s, f.predicate, \
                COALESCE(eo.merged_into, eo.id)::text AS o, f.valid_from::text AS vf, \
                f.valid_to::text AS vt, f.superseded_by::text AS sb \
           FROM _oo_docs.fact f JOIN _oo_docs.entity es ON es.id = f.subject_id \
           JOIN _oo_docs.entity eo ON eo.id = f.object_id \
          WHERE f.status IN ('draft', 'published') AND f.object_id IS NOT NULL \
          ORDER BY f.id",
    )
    .fetch_all(&s.pool)
    .await
    .unwrap();
    let mut eid: BTreeMap<String, String> = BTreeMap::new();
    let mut lines = String::new();
    let mut anchors_of_entity: BTreeMap<String, Vec<(String, String, Option<Value>)>> =
        BTreeMap::new();
    for f in &facts {
        let (sj, ob): (String, String) = (f.get("s"), f.get("o"));
        for t in [sj, ob] {
            anchors_of_entity.entry(t).or_default().extend(
                by_target
                    .get(f.get::<String, _>("id").as_str())
                    .cloned()
                    .unwrap_or_default(),
            );
        }
    }
    let mut n_ent = 0;
    for (i, e) in ents.iter().enumerate() {
        let ty: String = e.get("type");
        if !CORE.contains(&ty.as_str()) {
            continue;
        }
        let id: String = e.get("id");
        let name = format!("E{:04}", i + 1);
        eid.insert(id.clone(), name.clone());
        let mut als: Vec<String> = aliases
            .iter()
            .filter(|a| a.get::<String, _>("e") == id)
            .map(|a| a.get("alias"))
            .collect();
        als.sort();
        als.dedup();
        let mut anchors = anchors_of_entity.get(&id).cloned().unwrap_or_default();
        for a in aliases.iter().filter(|a| a.get::<String, _>("e") == id) {
            anchors.extend(
                by_target
                    .get(a.get::<String, _>("id").as_str())
                    .cloned()
                    .unwrap_or_default(),
            );
        }
        let mut rec = json!({"kind": "entity", "id": name, "type": ty, "name": e.get::<String, _>("canonical_name"),
                             "aliases": als, "anchors": anchor_json(&anchors)});
        if e.get::<Option<String>, _>("subkind").is_some() {
            rec["subkind"] = json!("preference");
        }
        writeln!(lines, "{rec}").unwrap();
        n_ent += 1;
    }
    let ents_text = std::mem::take(&mut lines);
    let mut fid: BTreeMap<String, String> = BTreeMap::new();
    let mut n_fact = 0;
    for f in &facts {
        let pred: String = f.get("predicate");
        let (Some(sj), Some(ob)) = (
            eid.get(&f.get::<String, _>("s")),
            eid.get(&f.get::<String, _>("o")),
        ) else {
            continue;
        };
        if !PREDICATES.contains(&pred.as_str()) {
            continue;
        }
        let id: String = f.get("id");
        n_fact += 1;
        let name = format!("F{:04}", n_fact);
        fid.insert(id.clone(), name.clone());
        let mut rec = json!({"kind": "fact", "id": name, "subject": sj, "predicate": pred, "object": ob,
                             "anchors": anchor_json(&by_target.get(&id).cloned().unwrap_or_default())});
        if let Some(v) = f.get::<Option<String>, _>("vf") {
            rec["valid_from"] = json!(v);
        }
        if let Some(v) = f.get::<Option<String>, _>("vt") {
            rec["valid_to"] = json!(v);
        }
        writeln!(lines, "{rec}").unwrap();
    }
    for f in &facts {
        if let (Some(old), Some(Some(new))) = (
            fid.get(&f.get::<String, _>("id")),
            f.get::<Option<String>, _>("sb").map(|n| fid.get(&n)),
        ) {
            writeln!(
                lines,
                "{}",
                json!({"kind": "supersede", "old": old, "new": new})
            )
            .unwrap();
        }
    }
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("entities.jsonl"), ents_text).unwrap();
    std::fs::write(dir.join("facts.jsonl"), lines).unwrap();
    (n_ent, n_fact)
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT, OO_EVAL_OUT and docker (oo-docs-parser:dev)"]
async fn the_whole_golden_set_through_the_real_pipeline() {
    let out = PathBuf::from(std::env::var("OO_EVAL_OUT").expect("OO_EVAL_OUT"));
    let with_model = std::env::var("OO_EVAL_MODEL").is_ok_and(|v| v == "1");
    let mut s = Stack::start(if with_model { "eval_on" } else { "eval_off" }, 2).await;
    let script = testing_root()
        .join("../../../tools/model-egress/model_egress.py")
        .canonicalize()
        .or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tools/model-egress/model_egress.py")
                .canonicalize()
        })
        .unwrap();
    let script_s = script.to_string_lossy().into_owned();
    // Respawn the engine with the egress script known and, for a slow model, more
    // extract_llm workers (`docs.json` `concurrency.extract_llm`).
    drop(s.engine.take());
    if let Ok(n) = std::env::var("OO_EVAL_LLM_WORKERS") {
        std::fs::write(
            s.state.join("docs.json"),
            format!(r#"{{"version":"oo.docs/v1","concurrency":{{"extract_llm":{n}}}}}"#),
        )
        .unwrap();
    }
    s.spawn_engine(&[("OO_DOCS_MODEL_EGRESS", script_s.as_str())]);

    // OO_EVAL_SUBSET=a/b.md,c/d.docx limits the run to those golden files (debugging).
    let n = match std::env::var("OO_EVAL_SUBSET") {
        Ok(list) => {
            let mut n = 0;
            for rel in list.split(',').filter(|r| !r.is_empty()) {
                let dest = s.root.join(rel);
                std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
                std::fs::copy(testing_root().join("golden/docs").join(rel), dest).unwrap();
                n += 1;
            }
            n
        }
        Err(_) => {
            let n = copy_tree(&testing_root().join("golden/docs"), &s.root);
            assert!(n >= 50, "{n} golden files");
            n
        }
    };
    if with_model {
        let endpoint = std::env::var("OO_EVAL_ENDPOINT_FILE").expect("OO_EVAL_ENDPOINT_FILE");
        let key = std::env::var("OO_EVAL_KEY_FILE").expect("OO_EVAL_KEY_FILE");
        let model = std::env::var("OO_EVAL_MODEL_ID").expect("OO_EVAL_MODEL_ID");
        // References only: the paths of two 0600 files.
        let body = json!({"provider": "openai-compatible", "base_url": format!("file:{endpoint}"),
                          "model": model, "key_ref": format!("file:{key}")});
        let auth = format!("Bearer {}", common::engine::TOKEN);
        let put = common::http::request(
            s.addr(),
            "PUT",
            "/v1/docs/config/model",
            &[("Authorization", auth.as_str()), ("If-Match", "\"r1\"")],
            Some(&body.to_string()),
        )
        .await;
        assert_eq!(
            put.status,
            200,
            "model config refused: {}",
            put.json()["code"]
        );
        let status = put.json()["status"].as_str().unwrap_or("").to_owned();
        let code = put.json()["code"].as_str().unwrap_or("").to_owned();
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            out.join("model-status.json"),
            json!({"status": status, "code": code}).to_string(),
        )
        .unwrap();
        assert_eq!(status, "ok", "the model egress did not start: {code}");
    }
    let r = s
        .call(
            "POST",
            "/v1/docs/drives",
            Some(
                &json!({"name": "eval", "backend": "fs", "model_enabled": with_model,
                         "config": {"root": s.root.to_str().unwrap()}}),
            ),
        )
        .await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let t0 = Instant::now();
    // Settled: every file is past the pipeline (a file whose facts wait for a person
    // may stay `indexed`) and no model job is open, for three polls in a row.
    let limit = Duration::from_secs(if with_model { 3000 } else { 900 });
    let mut quiet = 0;
    let mut st = s.states().await;
    while quiet < 3 && t0.elapsed() < limit {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        st = s.states().await;
        let open: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM _oo_docs.job WHERE state IN ('queued', 'running') AND kind IN ('parse', 'extract_rules', 'extract_llm')",
        )
        .fetch_one(&s.pool)
        .await
        .unwrap();
        let moving = st.values().any(|(state, _)| {
            matches!(
                state.as_str(),
                "stored" | "queued" | "parsing" | "parsed" | "extracting"
            )
        });
        quiet = if st.len() >= n && open == 0 && !moving {
            quiet + 1
        } else {
            0
        };
    }
    let secs = t0.elapsed().as_secs_f64();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (state, _) in st.values() {
        *counts.entry(state.clone()).or_default() += 1;
    }
    std::fs::create_dir_all(&out).unwrap();
    let (e, f) = export(&s, &out.join("predictions")).await;
    let usage = s.call("GET", "/v1/docs/usage", None).await.json();
    let jobs: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT kind, state, count(*) FROM _oo_docs.job WHERE kind IN ('extract_llm', 'extract_rules') GROUP BY 1, 2 ORDER BY 1, 2",
    )
    .fetch_all(&s.pool)
    .await
    .unwrap();
    let origin: Vec<(String, i64)> =
        sqlx::query_as("SELECT origin, count(*) FROM _oo_docs.fact GROUP BY 1 ORDER BY 1")
            .fetch_all(&s.pool)
            .await
            .unwrap();
    let summary = json!({"files": n, "states": counts, "seconds": secs, "entities": e, "facts": f,
        "model": with_model, "usage": usage, "jobs": jobs, "facts_by_origin": origin});
    std::fs::write(
        out.join("run.json"),
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .unwrap();
    eprintln!("{summary}");
    if std::env::var("OO_EVAL_KEEP").is_ok() {
        eprintln!("kept: db {} state {}", s.db, s.state.display());
        std::mem::forget(s);
        return;
    }
    s.finish().await;
}
