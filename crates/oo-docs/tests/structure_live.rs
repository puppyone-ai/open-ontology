//! Live measurement of the structure layer (D93) through the real pipeline: a real `oo-docs-engine`, the real Docling sidecar and, with
//! `OO_STRUCTURE_MODEL=1`, the real model egress. A corpus is dropped into a Drive and settles with the generic rules (before); the
//! structure is induced; a person accepts every class (the test plays that person through `structure:decide`); the files of the classes go
//! through extraction again, now against their templates (after). Each phase writes the structure (`structure-<phase>.json`), the facts
//! (`facts-<phase>.jsonl`) and the file states; with `OO_STRUCTURE_ACCEPTANCE=<score.py>` the cross-file relations of the acceptance folder are scored
//! by the same harness before and after.
//!
//! ```text
//! OO_DOCS_TEST_PG_PORT=<port> OO_STRUCTURE_OUT=<dir> [OO_STRUCTURE_PROJECT=<dir>] [OO_STRUCTURE_ACCEPTANCE=<abs score.py>] \
//!   [OO_STRUCTURE_MODEL=1 OO_EVAL_ENDPOINT_FILE=<path> OO_EVAL_KEY_FILE=<path> OO_EVAL_MODEL_ID=<id>] \
//!   cargo test -p oo-docs --locked --test structure_live -- --ignored --nocapture
//! ```
//!
//! The endpoint and key are only ever paths here; nothing reads or prints them.

mod common;
mod sidecar_support;
#[path = "sidecar_support/stack.rs"]
mod stack;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sidecar_support::testing_root;
use sqlx::Row;
use stack::{Stack, copy_tree};

async fn settle(
    s: &Stack,
    n: usize,
    limit: Duration,
) -> std::collections::BTreeMap<String, (String, Option<String>)> {
    let t0 = Instant::now();
    let mut quiet = 0;
    let mut st = s.states().await;
    while quiet < 5 && t0.elapsed() < limit {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        st = s.states().await;
        let open: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo_docs.job WHERE state IN ('queued', 'running') AND kind IN ('parse', 'extract_rules', 'extract_llm', 'publish')")
            .fetch_one(&s.pool)
            .await
            .unwrap();
        let moving = st.values().any(|(state, _)| {
            matches!(
                state.as_str(),
                "stored" | "queued" | "parsing" | "parsed" | "extracting" | "outdated"
            )
        });
        quiet = if st.len() >= n && open == 0 && !moving {
            quiet + 1
        } else {
            0
        };
    }
    st
}

async fn facts(s: &Stack, out: &Path, tag: &str) -> usize {
    let rows = sqlx::query(
        "SELECT fv.path_at_version AS path, es.type AS st, es.canonical_name AS sn, f.predicate AS p, eo.type AS ot, eo.canonical_name AS onm, \
                f.value::text AS value, f.rule_id AS rule, f.origin AS origin \
           FROM _oo_docs.fact f JOIN _oo_docs.entity es ON es.id = f.subject_id LEFT JOIN _oo_docs.entity eo ON eo.id = f.object_id \
           JOIN _oo_docs.evidence ev ON ev.target_kind = 'fact' AND ev.target_id = f.id AND ev.withdrawn_at IS NULL \
           JOIN _oo_docs.file_version fv ON fv.id = ev.version_id \
          WHERE f.status IN ('draft', 'published') GROUP BY 1, 2, 3, 4, 5, 6, 7, 8, 9 ORDER BY 1, 2, 3, 4, 6, 7",
    )
    .fetch_all(&s.pool)
    .await
    .unwrap();
    let mut text = String::new();
    for r in &rows {
        text.push_str(&format!(
            "{}\n",
            json!({"file": r.get::<String, _>("path"), "subject": {"type": r.get::<String, _>("st"), "name": r.get::<String, _>("sn")}, "predicate": r.get::<String, _>("p"),
                   "object": r.get::<Option<String>, _>("onm").map(|n| json!({"type": r.get::<Option<String>, _>("ot"), "name": n})), "value": r.get::<Option<String>, _>("value"),
                   "rule": r.get::<Option<String>, _>("rule"), "origin": r.get::<String, _>("origin")})
        ));
    }
    std::fs::write(out.join(format!("facts-{tag}.jsonl")), text).unwrap();
    rows.len()
}

async fn snapshot(
    s: &Stack,
    drive_id: &str,
    out: &Path,
    tag: &str,
    st: &std::collections::BTreeMap<String, (String, Option<String>)>,
) -> Value {
    let r = s
        .call(
            "POST",
            &format!("/v1/docs/drives/{drive_id}/structure:induce"),
            None,
        )
        .await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let schema = r.json();
    let members: BTreeSet<String> = sqlx::query_scalar("SELECT f.path FROM _oo_docs.structure_member m JOIN _oo_docs.file f ON f.id = m.file_id WHERE m.drive_id = $1::uuid")
        .bind(drive_id)
        .fetch_all(&s.pool)
        .await
        .unwrap()
        .into_iter()
        .collect();
    let record_paths: Vec<String> = sqlx::query_scalar(
        "SELECT path FROM _oo_docs.file WHERE drive_id = $1::uuid AND deleted_at IS NULL AND COALESCE(profile, 'business') IN ('business', 'meeting', 'table') ORDER BY path",
    )
    .bind(drive_id)
    .fetch_all(&s.pool)
    .await
    .unwrap();
    let unclustered: Vec<&String> = record_paths
        .iter()
        .filter(|p| !members.contains(*p))
        .collect();
    let doc = json!({"classes": schema["classes"], "unclustered": unclustered, "coverage": schema["coverage"], "version": schema["version"], "induced": schema["induced"]});
    std::fs::write(
        out.join(format!("structure-{tag}.json")),
        serde_json::to_string_pretty(&doc).unwrap(),
    )
    .unwrap();
    let y = s
        .call(
            "GET",
            &format!("/v1/docs/drives/{drive_id}/structure/yaml"),
            None,
        )
        .await;
    assert_eq!(y.status, 200);
    std::fs::write(
        out.join(format!("structure-{tag}.yaml")),
        y.json()["text"].as_str().unwrap(),
    )
    .unwrap();
    let n = facts(s, out, tag).await;
    let files: serde_json::Map<String, Value> = st
        .iter()
        .map(|(p, (state, code))| {
            (
                format!("project/{p}"),
                json!({"state": state, "detail": {"state_code": code, "size": 1}}),
            )
        })
        .collect();
    std::fs::write(
        out.join(format!("run-{tag}.json")),
        json!({"files": files}).to_string(),
    )
    .unwrap();
    eprintln!(
        "{tag}: {} classes, {} facts, coverage {}",
        doc["classes"].as_array().map_or(0, Vec::len),
        n,
        doc["coverage"]
    );
    doc
}

fn score_acceptance(script: &str, s: &Stack, out: &Path, tag: &str) {
    let token = out.join("token.txt");
    std::fs::write(&token, common::engine::TOKEN).unwrap();
    let gold = testing_root().join("acceptance/gold");
    let status = std::process::Command::new("python3")
        .args([
            "-B",
            script,
            "--endpoint",
            &format!("http://{}", s.addr()),
            "--token-file",
            token.to_str().unwrap(),
            "--run",
        ])
        .arg(out.join(format!("run-{tag}.json")))
        .arg("--gold")
        .arg(&gold)
        .arg("--out")
        .arg(out.join(format!("acceptance-{tag}")))
        .status()
        .expect("score.py");
    assert!(status.success(), "the acceptance scorer failed");
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT, OO_STRUCTURE_OUT and docker (oo-docs-parser:dev)"]
async fn the_structure_layer_through_the_real_pipeline() {
    let out = PathBuf::from(std::env::var("OO_STRUCTURE_OUT").expect("OO_STRUCTURE_OUT"));
    let with_model = std::env::var("OO_STRUCTURE_MODEL").is_ok_and(|v| v == "1");
    let project = std::env::var("OO_STRUCTURE_PROJECT")
        .map_or_else(|_| testing_root().join("structure/project"), PathBuf::from);
    let acceptance = std::env::var("OO_STRUCTURE_ACCEPTANCE").ok();
    let mut s = Stack::start(
        if with_model {
            "structure_on"
        } else {
            "structure_off"
        },
        2,
    )
    .await;
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/model-egress/model_egress.py")
        .canonicalize()
        .unwrap();
    let script_s = script.to_string_lossy().into_owned();
    drop(s.engine.take());
    let pass: Vec<(String, String)> = ["OO_DOCS_MODEL_TIMEOUT_SECS", "OO_DOCS_MODEL_LATENCY_LOG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| ((*k).to_owned(), v)))
        .collect();
    let mut env: Vec<(&str, &str)> = vec![("OO_DOCS_MODEL_EGRESS", script_s.as_str())];
    env.extend(pass.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    s.spawn_engine(&env);
    let n = copy_tree(&project, &s.root);
    assert!(n >= 20, "{n} files");
    if with_model {
        let endpoint = std::env::var("OO_EVAL_ENDPOINT_FILE").expect("OO_EVAL_ENDPOINT_FILE");
        let key = std::env::var("OO_EVAL_KEY_FILE").expect("OO_EVAL_KEY_FILE");
        let model = std::env::var("OO_EVAL_MODEL_ID").expect("OO_EVAL_MODEL_ID");
        let body = json!({"provider": "openai-compatible", "base_url": format!("file:{endpoint}"), "model": model, "key_ref": format!("file:{key}")});
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
        assert_eq!(
            put.json()["status"].as_str(),
            Some("ok"),
            "the model egress did not start"
        );
    }
    let r = s.call("POST", "/v1/docs/drives", Some(&json!({"name": "structure", "backend": "fs", "model_enabled": with_model, "config": {"root": s.root.to_str().unwrap()}}))).await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let drive_id = r.json()["id"].as_str().unwrap().to_owned();
    let limit = Duration::from_secs(if with_model { 3000 } else { 900 });
    std::fs::create_dir_all(&out).unwrap();

    // Before: the generic rules, then the structure is induced.
    let st = settle(&s, n, limit).await;
    let before = snapshot(&s, &drive_id, &out, "before", &st).await;
    let classes = before["classes"].as_array().unwrap().clone();
    assert!(!classes.is_empty(), "no class was induced");
    assert!(
        classes.iter().all(|c| c["status"] == "proposed"),
        "a class is proposed until a person confirms it"
    );
    assert_eq!(before["coverage"]["confirmed"], 0);
    if let Some(script) = &acceptance {
        score_acceptance(script, &s, &out, "before");
    }
    let slot_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo_docs.fact WHERE rule_id = 'structure.slot'")
            .fetch_one(&s.pool)
            .await
            .unwrap();
    assert_eq!(
        slot_before, 0,
        "nothing is read against a template before a person confirms the structure"
    );

    // A person confirms every class (renaming one property first, to see the decision survive the next induction).
    let first = classes[0]["class_key"].as_str().unwrap().to_owned();
    let prop0 = classes[0]["properties"][0]["slug"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = s.call("POST", &format!("/v1/docs/drives/{drive_id}/structure:decide"), Some(&json!({"op": "rename", "class_key": first, "property": prop0, "name": "Renamed by a person"}))).await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    for c in &classes {
        let r = s
            .call(
                "POST",
                &format!("/v1/docs/drives/{drive_id}/structure:decide"),
                Some(&json!({"op": "accept", "class_key": c["class_key"], "property": null})),
            )
            .await;
        assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    let st = settle(&s, n, limit).await;
    let after = snapshot(&s, &drive_id, &out, "after", &st).await;
    if let Some(script) = &acceptance {
        score_acceptance(script, &s, &out, "after");
    }

    // After: the structure is confirmed and effective.
    let cov = &after["coverage"];
    assert!(cov["confirmed"].as_i64().unwrap() > 0, "{cov}");
    let renamed = after["classes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["class_key"] == first.as_str())
        .unwrap()["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["slug"] == prop0.as_str())
        .unwrap()
        .clone();
    assert_eq!(
        renamed["name"], "Renamed by a person",
        "a person's decision survives the next induction"
    );
    let slots: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo_docs.fact WHERE rule_id = 'structure.slot' AND status IN ('draft', 'published')").fetch_one(&s.pool).await.unwrap();
    let held: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo_docs.review_item WHERE kind = 'reprocess_diff' AND status = 'pending'").fetch_one(&s.pool).await.unwrap();
    let versions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.structure_version WHERE drive_id = $1::uuid",
    )
    .bind(&drive_id)
    .fetch_one(&s.pool)
    .await
    .unwrap();
    if held != 0 {
        let items: Vec<(String, String)> = sqlx::query_as("SELECT reason_code, subject::text FROM _oo_docs.review_item WHERE kind = 'reprocess_diff' AND status = 'pending' LIMIT 4").fetch_all(&s.pool).await.unwrap();
        for (reason, subject) in items {
            let v: Value = serde_json::from_str(&subject).unwrap();
            for side in ["added", "removed"] {
                let keys: Vec<String> = v[side]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|k| k.as_str().map(str::to_owned))
                    .collect();
                let rows: Vec<(String, Option<String>, Option<String>, String)> = sqlx::query_as("SELECT DISTINCT predicate, rule_id, value::text, status FROM _oo_docs.fact WHERE fact_key = ANY($1::text[])").bind(&keys).fetch_all(&s.pool).await.unwrap();
                eprintln!(
                    "held ({reason}) prior {} {side} {}: {rows:?}",
                    v["prior"],
                    keys.len()
                );
            }
        }
    }
    assert_eq!(
        held, 0,
        "a decision about the structure is not held for review by the reprocess gate"
    );
    let bad: Vec<_> = st
        .iter()
        .filter(|(_, (state, _))| {
            !matches!(
                state.as_str(),
                "published" | "extract_partial" | "unsupported" | "duplicate" | "indexed"
            )
        })
        .collect();
    assert!(bad.is_empty(), "files that did not settle: {bad:?}");

    // A property that recurs in a confirmed class is a proposal (type promotion), not a silent change.
    let mut proposed = 0i64;
    if acceptance.is_none() {
        for i in 0..4 {
            let dir = s.root.join("meetings/星河");
            std::fs::write(dir.join(format!("2026-09-0{}-预算会.md", i + 1)), format!("---\n日期: 2026-09-0{}\n项目: 星河\n主持人: 周若溪\n---\n\n# 预算会\n\n## 参会人\n\n- 陈子墨\n- 周若溪\n\n## 决定\n\n- 追加预算\n\n## 待办\n\n- 提交预算\n\n## 预算\n\n- 追加{}万\n", i + 1, 10 + i)).unwrap();
        }
        let sync = s
            .call(
                "POST",
                &format!("/v1/docs/drives/{drive_id}:sync"),
                Some(&json!({})),
            )
            .await;
        assert!(
            sync.status == 200 || sync.status == 202,
            "{}",
            String::from_utf8_lossy(&sync.body)
        );
        let st2 = settle(&s, n + 4, limit).await;
        let _ = st2;
        let _ = snapshot(&s, &drive_id, &out, "proposal", &st2).await;
        proposed = sqlx::query_scalar("SELECT count(*) FROM _oo_docs.review_item WHERE kind = 'type_promotion' AND reason_code = 'structure_property' AND status = 'pending'").fetch_one(&s.pool).await.unwrap();
        assert!(
            proposed >= 1,
            "a section that recurs in four files of a confirmed class is proposed"
        );
        let item: (String, i64) = sqlx::query_as("SELECT id::text, revision::bigint FROM _oo_docs.review_item WHERE kind = 'type_promotion' AND reason_code = 'structure_property' AND status = 'pending' LIMIT 1").fetch_one(&s.pool).await.unwrap();
        let d = s
            .call(
                "POST",
                "/v1/docs/review:decide",
                Some(&json!({"items": [{"id": item.0, "revision": item.1, "decision": "accept"}]})),
            )
            .await;
        assert!(d.status == 200, "{}", String::from_utf8_lossy(&d.body));
        let (accepted,): (i64,) = sqlx::query_as("SELECT count(*) FROM _oo_docs.structure_class c, jsonb_array_elements(c.doc->'properties') p WHERE c.drive_id = $1::uuid AND p->>'status' = 'confirmed' AND p->>'slug' = 'budget'").bind(&drive_id).fetch_one(&s.pool).await.unwrap();
        assert!(
            accepted >= 1,
            "accepting the proposal confirms the property"
        );
    }
    let summary = json!({"files": n, "model": with_model, "classes": classes.len(), "coverage_before": before["coverage"], "coverage_after": cov, "slot_facts": slots, "structure_versions": versions, "proposals": proposed});
    std::fs::write(
        out.join("run.json"),
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .unwrap();
    eprintln!("{summary}");
    s.finish().await;
}
