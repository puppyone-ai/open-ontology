//! Live measurement of the document profiles (D89) through the real pipeline: a real `oo-docs-engine`, the real Docling sidecar and, with
//! `OO_PROFILES_MODEL=1`, the real model egress. The synthetic engineering repository and the three papers of
//! `testing/super-ontology/profiles/project` are dropped into a Drive; the test asserts what must hold without a model (every file settles, the
//! profile of each file, knowledge search finds AGENTS.md, a code file and a plain log) and writes what the engine found as `entities.jsonl`
//! (`{"file","type","label","name","published"}`) and `files.jsonl` for `tools/super-ontology/profiles/score.py`. With `OO_PROFILES_ASK=<dir>`
//! the Ask loop of the app (`ask_headless.mjs`) then answers the frozen questions against the same engine.
//!
//! ```text
//! OO_DOCS_TEST_PG_PORT=<port> OO_PROFILES_OUT=<dir> [OO_PROFILES_MODEL=1 OO_EVAL_ENDPOINT_FILE=<path> OO_EVAL_KEY_FILE=<path> OO_EVAL_MODEL_ID=<id>] \
//!   cargo test -p oo-docs --locked --test profiles_live -- --ignored --nocapture
//! ```
//!
//! The endpoint and key are only ever paths here; nothing reads or prints them.

mod common;
mod sidecar_support;
#[path = "sidecar_support/stack.rs"]
mod stack;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::json;
use sidecar_support::testing_root;
use sqlx::Row;
use stack::{Stack, copy_tree};

fn expected_profile(path: &str) -> &'static str {
    match path {
        "harborq/AGENTS.md"
        | "harborq/README.md"
        | "harborq/CONTRIBUTING.md"
        | "harborq/docs/STYLE_GUIDE.md" => "instruction",
        p if p.starts_with("harborq/docs/adr/") => "instruction",
        p if p.starts_with("papers/") => "paper",
        "harborq/CHANGELOG.md" | "harborq/LICENSE" | "harborq/ops/worker.log" => "other",
        _ => "code",
    }
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT, OO_PROFILES_OUT and docker (oo-docs-parser:dev)"]
async fn the_profile_set_through_the_real_pipeline() {
    let out = PathBuf::from(std::env::var("OO_PROFILES_OUT").expect("OO_PROFILES_OUT"));
    let with_model = std::env::var("OO_PROFILES_MODEL").is_ok_and(|v| v == "1");
    let mut s = Stack::start(
        if with_model {
            "profiles_on"
        } else {
            "profiles_off"
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
    let n = copy_tree(&testing_root().join("profiles/project"), &s.root);
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
    let r = s
        .call("POST", "/v1/docs/drives", Some(&json!({"name": "profiles", "backend": "fs", "model_enabled": with_model, "config": {"root": s.root.to_str().unwrap()}})))
        .await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let drive_id = r.json()["id"].as_str().unwrap().to_owned();
    let t0 = Instant::now();
    let limit = Duration::from_secs(if with_model { 2400 } else { 600 });
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
                "stored" | "queued" | "parsing" | "parsed" | "extracting"
            )
        });
        quiet = if st.len() >= n && open == 0 && !moving {
            quiet + 1
        } else {
            0
        };
    }
    std::fs::create_dir_all(&out).unwrap();
    let secs = t0.elapsed().as_secs_f64();

    // Every file settles; a file with nothing structured in it is published, not failed. With the model on, a paper may wait `indexed` for the
    // person who reviews what the model proposed (that is the model's, not the profile's).
    let bad: Vec<_> = st
        .iter()
        .filter(|(path, (state, _))| {
            !(matches!(state.as_str(), "published" | "extract_partial")
                || with_model && state == "indexed" && path.starts_with("papers/"))
        })
        .collect();
    assert!(bad.is_empty(), "files that did not publish: {bad:?}");

    // The file list carries the profile of each file.
    let list = s
        .call(
            "GET",
            &format!("/v1/docs/drives/{drive_id}/files?recursive=true&limit=200"),
            None,
        )
        .await
        .json();
    // The blocks of the papers as the parser delivered them (evidence of what the extractors read).
    for f in list["items"].as_array().unwrap() {
        let path = f["path"].as_str().unwrap();
        if path.starts_with("papers/") {
            let id = f["id"].as_str().unwrap();
            let blocks = s
                .call(
                    "GET",
                    &format!("/v1/docs/files/{id}/blocks?limit=200"),
                    None,
                )
                .await
                .json();
            std::fs::write(
                out.join(format!("blocks-{}.json", path.replace('/', "_"))),
                serde_json::to_string_pretty(&blocks).unwrap(),
            )
            .unwrap();
        }
    }
    let mut files = String::new();
    let mut by_profile: BTreeMap<String, usize> = BTreeMap::new();
    for f in list["items"].as_array().unwrap() {
        let path = f["path"].as_str().unwrap();
        let profile = f["profile"].as_str().unwrap_or("none");
        assert_eq!(profile, expected_profile(path), "profile of {path}");
        *by_profile.entry(profile.to_owned()).or_default() += 1;
        files.push_str(&format!(
            "{}\n",
            json!({"file": path, "profile": profile, "state": f["state"], "format": f["format"]})
        ));
    }
    std::fs::write(out.join("files.jsonl"), files).unwrap();
    // The profile is stored on the file, not only derived.
    let stored: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.file WHERE profile IS NOT NULL AND deleted_at IS NULL",
    )
    .fetch_one(&s.pool)
    .await
    .unwrap();
    assert!(
        stored as usize >= n - 2,
        "{stored} files have a stored profile"
    );

    // Knowledge search finds what the extractors did not turn into entities: the rules in AGENTS.md, a code file, a log and a licence.
    for (q, path) in [
        ("tests", "harborq/AGENTS.md"),
        ("Exporter", "harborq/tools/export.py"),
        ("attempt", "harborq/ops/worker.log"),
        ("Apache", "harborq/LICENSE"),
    ] {
        let hits = s
            .call("GET", &format!("/v1/docs/search?q={q}&limit=20"), None)
            .await
            .json();
        let found = hits["items"].as_array().unwrap().iter().any(|h| {
            h["evidence"]
                .as_array()
                .is_some_and(|e| e.iter().any(|x| x["path"].as_str() == Some(path)))
        });
        assert!(
            found,
            "knowledge search for {q:?} does not cite {path}: {hits}"
        );
    }

    // No model call for code, instruction and other files.
    let llm_files: Vec<(String,)> = sqlx::query_as("SELECT DISTINCT f.path FROM _oo_docs.job j JOIN _oo_docs.file f ON f.id = j.file_id WHERE j.kind = 'extract_llm' ORDER BY 1")
        .fetch_all(&s.pool)
        .await
        .unwrap();
    for (p,) in &llm_files {
        assert_eq!(expected_profile(p), "paper", "extract_llm ran for {p}");
    }
    if !with_model {
        assert!(llm_files.is_empty());
    }

    // What the engine found, per file: the entity evidence and the evidence of the facts that touch the entity.
    let rows = sqlx::query(
        "SELECT e.type AS ty, e.thing_label AS label, e.canonical_name AS name, fv.path_at_version AS path, \
                bool_or(src.published) AS published \
           FROM _oo_docs.entity e \
           JOIN LATERAL ( \
                SELECT ev.version_id, false AS published FROM _oo_docs.evidence ev \
                 WHERE ev.target_kind = 'entity' AND ev.target_id = e.id AND ev.withdrawn_at IS NULL \
                UNION ALL \
                SELECT ev.version_id, (f.status = 'published') FROM _oo_docs.fact f \
                  JOIN _oo_docs.evidence ev ON ev.target_kind = 'fact' AND ev.target_id = f.id AND ev.withdrawn_at IS NULL \
                 WHERE (f.subject_id = e.id OR f.object_id = e.id) AND f.status IN ('draft', 'published') \
           ) src ON true \
           JOIN _oo_docs.file_version fv ON fv.id = src.version_id \
          WHERE e.status = 'active' AND e.type <> 'Document' \
          GROUP BY 1, 2, 3, 4 ORDER BY 4, 1, 2, 3",
    )
    .fetch_all(&s.pool)
    .await
    .unwrap();
    let mut lines = String::new();
    for r in &rows {
        lines.push_str(&format!(
            "{}\n",
            json!({"file": r.get::<String, _>("path"), "type": r.get::<String, _>("ty"), "label": r.get::<Option<String>, _>("label"),
                   "name": r.get::<String, _>("name"), "published": r.get::<bool, _>("published")})
        ));
    }
    std::fs::write(out.join("entities.jsonl"), lines).unwrap();
    // A Thing of a profile label is never queued for promotion (a label the model made up may be).
    let promos: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.review_item WHERE kind = 'type_promotion' AND subject->>'label' = ANY($1::text[])",
    )
    .bind(oo_docs::profile::LABELS.iter().map(|l| (*l).to_owned()).collect::<Vec<_>>())
    .fetch_one(&s.pool)
    .await
    .unwrap();
    let pending: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo_docs.review_item WHERE status = 'pending'")
            .fetch_one(&s.pool)
            .await
            .unwrap_or(-1);
    let summary = json!({"files": n, "seconds": secs, "by_profile": by_profile, "entities": rows.len(), "type_promotions": promos, "pending_review_items": pending,
        "model": with_model, "llm_files": llm_files.iter().map(|x| x.0.clone()).collect::<Vec<_>>()});
    std::fs::write(
        out.join("run.json"),
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .unwrap();
    eprintln!("{summary}");
    assert_eq!(promos, 0, "a profile label was proposed as a type");

    // Ask: the app's own loop against this engine (the frozen questions), when asked for.
    if let Ok(dir) = std::env::var("OO_PROFILES_ASK") {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/super-ontology/profiles/ask_headless.mjs")
            .canonicalize()
            .unwrap();
        let questions = testing_root().join("profiles/gold/questions.jsonl");
        let status = std::process::Command::new("node")
            .arg(&script)
            .args([
                "--base-url",
                &format!("http://{}", s.addr()),
                "--token",
                common::engine::TOKEN,
                "--questions",
            ])
            .arg(&questions)
            .args(["--out", &dir])
            .status()
            .expect("node");
        assert!(status.success(), "ask_headless failed");
    }
    if std::env::var("OO_PROFILES_KEEP").is_ok() {
        std::mem::forget(s);
        return;
    }
    s.finish().await;
}
