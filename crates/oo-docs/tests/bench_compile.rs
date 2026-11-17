//! Compile p95 on a 1000 file Drive, budget 800 (B-S6-07, 30 L4 acceptance 5:
//! p95 < 2 s; 20 percent over alarms, 50 percent over blocks).
//!
//! The Drive is 1000 synthetic Markdown files (projects with an owner and a
//! status, decisions, people) indexed and published through the real write
//! path; each timed compile is `compile_from_store` plus persisting the pack,
//! the work behind `POST /context-packs:compile`, with a fresh task each time so
//! the pack cache never answers. `OO_BENCH_OUT=<file>` writes the numbers.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --release --test bench_compile -- --ignored --nocapture`
//! (the limit is asserted in the release build only).

mod common;

use std::time::Instant;

use common::fx::{DRIVE, Fx};
use oo_docs::compile::{CompileRequest, persist};

const FILES: usize = 1000;
const RUNS: usize = 60;
const BUDGET_SECS: f64 = 2.0;

fn doc(i: usize) -> (String, String) {
    match i % 4 {
        0 => (
            format!("Projects/p{:03}/项目{i}.md", i % 40),
            format!(
                "---\ntype: project\nname: 项目{i}\nowner: 成员{}\nstatus: 进行中\n---\n\n# 项目{i}\n\n负责人：成员{}\n\n状态：进行中\n\n说明：第{i}号项目的范围、里程碑与风险。\n",
                i % 60,
                i % 60
            ),
        ),
        1 => (
            format!("Decisions/D-{i:04}.md"),
            format!(
                "---\ntype: decision\nstatus: confirmed\ndate: 2026-0{}-1{}\nowner: 成员{}\n---\n\n# 决定：项目{}采用方案{}\n\n成员{}决定项目{}采用方案{}。\n",
                1 + i % 9,
                i % 9,
                i % 60,
                i - 1,
                i % 7,
                i % 60,
                i - 1,
                i % 7
            ),
        ),
        2 => (
            format!("People/成员{}-{i}.md", i % 60),
            format!(
                "---\ntype: person\nname: 成员{}\nrole: 工程师\n---\n\n# 成员{}\n\n角色：工程师\n\n参与：项目{}\n",
                i % 60,
                i % 60,
                i - 2
            ),
        ),
        _ => (
            format!("Notes/n{i:04}.md"),
            format!(
                "# 笔记{i}\n\n今天讨论了项目{}的排期，负责人成员{}。\n",
                i - 3,
                i % 60
            ),
        ),
    }
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    let idx = ((sorted.len() as f64) * p).ceil() as usize;
    sorted[idx.saturating_sub(1).min(sorted.len() - 1)]
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn compile_p95_on_a_thousand_file_drive() {
    let mut fx = Fx::new("bench_compile").await;
    let t = Instant::now();
    for i in 0..FILES {
        let (path, src) = doc(i);
        fx.index_md(&path, &src).await;
    }
    let index_secs = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let out = fx.publish().await;
    assert!(
        matches!(out, oo_docs::publish::run::PublishOutcome::Published(_)),
        "{out:?}"
    );
    let publish_secs = t.elapsed().as_secs_f64();
    let files = fx
        .scalar("SELECT count(*) FROM _oo_docs.file WHERE state = 'published'")
        .await;
    let facts = fx
        .scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'published'")
        .await;
    assert_eq!(files as usize, FILES);

    let mut times = Vec::new();
    let mut tokens = 0;
    for i in 0..RUNS {
        let scope: Vec<String> = match i % 3 {
            0 => vec![],
            1 => vec!["Projects".into(), "Decisions".into()],
            _ => vec!["People".into(), "Projects/p004".into()],
        };
        let req = CompileRequest {
            scope,
            task: format!("准备项目{}的周报和决定回顾（第{i}次）", (i * 7) % FILES),
            budget: 800,
            ..CompileRequest::default()
        };
        let t = Instant::now();
        let norm = oo_docs::compile::normalize_scope(&req.scope);
        let reader =
            oo_docs::compile::pg::PgReader::load(&mut fx.rt, &fx.cas, DRIVE, req.version, &norm)
                .await
                .unwrap();
        let t_load = t.elapsed().as_secs_f64();
        let pack = oo_docs::compile::compile(&reader, &req).unwrap();
        let t_compile = t.elapsed().as_secs_f64() - t_load;
        let (rec, created) = persist::persist(&mut fx.rt, &fx.cas, &pack).await.unwrap();
        times.push(t.elapsed().as_secs_f64());
        if i < 6 {
            eprintln!(
                "    load {t_load:.3}s compile {t_compile:.3}s persist {:.3}s",
                times[i] - t_load - t_compile
            );
        }
        if i < 5 || i % 10 == 0 {
            eprintln!("  run {i}: {:.3}s", times[i]);
        }
        assert!(created, "run {i} was answered by the cache");
        assert!(pack.brief_tokens <= 800);
        tokens = rec.tokens_brief;
    }
    let mut sorted = times.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let (p50, p95, max) = (
        pct(&sorted, 0.5),
        pct(&sorted, 0.95),
        *sorted.last().unwrap(),
    );
    eprintln!(
        "compile bench: {FILES} files, {facts} facts, budget 800, {RUNS} runs: p50 {p50:.3}s p95 {p95:.3}s max {max:.3}s (index {index_secs:.1}s, publish {publish_secs:.1}s, brief {tokens} tokens)"
    );
    if let Ok(path) = std::env::var("OO_BENCH_OUT") {
        let v = serde_json::json!({
            "protocol": "oo.bench.compile/v1", "files": FILES, "facts": facts, "budget": 800,
            "runs": RUNS, "p50_s": p50, "p95_s": p95, "max_s": max,
            "limit_s": BUDGET_SECS, "alarm_s": BUDGET_SECS * 1.2, "block_s": BUDGET_SECS * 1.5,
            "index_s": index_secs, "publish_s": publish_secs,
            "build": if cfg!(debug_assertions) { "debug" } else { "release" },
        });
        std::fs::write(path, serde_json::to_string_pretty(&v).unwrap() + "\n").unwrap();
    }
    // The limit is for the release build (the product); a debug build only reports.
    if !cfg!(debug_assertions) {
        assert!(
            p95 < BUDGET_SECS * 1.5,
            "p95 {p95:.3}s blocks (limit {BUDGET_SECS}s +50%)"
        );
        assert!(
            p95 < BUDGET_SECS,
            "p95 {p95:.3}s over the {BUDGET_SECS}s limit"
        );
    }
    fx.finish("bench_compile").await;
}
