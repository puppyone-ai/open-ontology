//! Latency of `GET /v1/docs/search` on a large Drive (WP3b, docs-api 1.9, migration 0153): the same queries before
//! and after the GIN indexes, plus the `EXPLAIN` of the fact and block queries.
//!
//! Synthetic Markdown only, indexed through the real write path (as `bench_scale`). `OO_SCALE_FILES=<n>` changes the
//! size (default 10000), `OO_BENCH_OUT=<file>` writes the numbers, `OO_SEARCH_EXPLAIN=1` prints the plans.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --release --test bench_search -- --ignored --nocapture`

mod common;

use std::time::Instant;

use common::engine::{Opts, TestEngine};
use common::fx::{DRIVE, Fx};
use common::http::request;

fn files() -> usize {
    std::env::var("OO_SCALE_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(if cfg!(debug_assertions) { 500 } else { 10_000 })
}

fn doc(i: usize) -> (String, String) {
    let dir = format!("d{:03}/s{:02}", i % 100, (i / 100) % 10);
    match i % 5 {
        0 => (
            format!("Projects/{dir}/project-{i}.md"),
            format!(
                "---\ntype: project\nname: Project {i}\nowner: Member {}\nstatus: active\n---\n\n# Project {i}\n\nOwner: Member {}\n\nScope, milestones and risks of project {i}.\n",
                i % 60,
                i % 60
            ),
        ),
        1 => (
            format!("Decisions/{dir}/D-{i:05}.md"),
            format!(
                "---\ntype: decision\nstatus: confirmed\ndate: 2026-0{}-1{}\nowner: Member {}\n---\n\n# Decision: project {} takes option {}\n\nMember {} decided that project {} takes option {}.\n",
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
            format!("People/{dir}/member-{}-{i}.md", i % 60),
            format!(
                "---\ntype: person\nname: Member {}\nrole: engineer\n---\n\n# Member {}\n\nRole: engineer\n\nWorks on project {}\n",
                i % 60,
                i % 60,
                i - 2
            ),
        ),
        3 => (
            format!("Contracts/{dir}/c{i:05}.md"),
            format!(
                "# 合同 {i}\n\n合同编号：HT-2026-{i:05}\n\n甲方负责项目 {} 的交付，每年三月续约。\n",
                i - 3
            ),
        ),
        _ => (
            format!("Notes/{dir}/n{i:05}.md"),
            format!(
                "# Note {i}\n\nDiscussed the schedule of project {}; owner Member {}. Sentinel{} is pending.\n",
                i - 4,
                i % 60,
                i % 211
            ),
        ),
    }
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((v.len() as f64) * p).ceil() as usize;
    v[idx.saturating_sub(1).min(v.len() - 1)]
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn search_latency_on_a_large_drive() {
    let n = files();
    let e = TestEngine::start(
        "bench_search",
        Opts {
            stages: true,
            ..Opts::default()
        },
    )
    .await;
    let mut fx = Fx::attach("bench_search", &e.cas).await;
    let t = Instant::now();
    for i in 0..n {
        let (path, src) = doc(i);
        fx.index_md(&path, &src).await;
    }
    fx.publish().await;
    let index_s = t.elapsed().as_secs_f64();
    for t in ["fact", "entity", "evidence", "file_version", "parse_result"] {
        fx.admin_sql(&format!("ANALYZE _oo_docs.{t}")).await;
    }
    let facts = fx
        .scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'published'")
        .await;

    let queries = [
        "sentinel42",
        "合同编号",
        "member 7 decided",
        "project",
        "ht-2026-00123",
        "每年三月",
    ];
    // One warm-up call per query (it also builds the block vectors of the scope, once).
    let mut out = String::new();
    let auth = format!("Bearer {}", common::engine::TOKEN);
    let mut all = Vec::new();
    for q in queries {
        let path = format!("/v1/docs/search?q={}&drive_id={DRIVE}", enc(q));
        let r = request(e.addr, "GET", &path, &[("Authorization", &auth)], None).await;
        assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
        let hits = r.json()["items"].as_array().map_or(0, Vec::len);
        let mut times = Vec::new();
        for _ in 0..30 {
            let t = Instant::now();
            let r = request(e.addr, "GET", &path, &[("Authorization", &auth)], None).await;
            times.push(t.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(r.status, 200);
        }
        all.extend(times.iter().copied());
        let line = format!(
            "q={q:?} hits={hits} p50={:.1}ms p95={:.1}ms max={:.1}ms",
            pct(&mut times.clone(), 0.5),
            pct(&mut times.clone(), 0.95),
            times.iter().cloned().fold(0.0, f64::max)
        );
        println!("{line}");
        out.push_str(&line);
        out.push('\n');
    }
    let line = format!(
        "files={n} published_facts={facts} index_s={index_s:.0} overall_p95={:.1}ms",
        pct(&mut all, 0.95)
    );
    println!("{line}");
    out.push_str(&line);
    out.push('\n');

    if std::env::var("OO_SEARCH_EXPLAIN").is_ok() {
        let fact_plan = fx
            .text(
                "EXPLAIN (ANALYZE, COSTS OFF, TIMING OFF) \
                 SELECT ft.id, ts_rank_cd(ft.fts, query.q, 32) \
                   FROM _oo_docs.fact ft, (SELECT to_tsquery('simple', $$'project':* & '4242':*$$) AS q) query \
                  WHERE ft.status = 'published' AND ft.retracted_in IS NULL AND ft.fts @@ query.q",
            )
            .await;
        let block_plan = fx
            .text(
                "EXPLAIN (ANALYZE, COSTS OFF, TIMING OFF) \
                 SELECT bf.ir_b3, bf.ord, ts_rank_cd(bf.tsv, query.q, 32) \
                   FROM _oo_docs.block_fts bf, (SELECT to_tsquery('simple', $$'sentinel42':*$$) AS q) query \
                  WHERE bf.tsv @@ query.q AND bf.ord < 20000 \
                    AND bf.ir_b3 = ANY(ARRAY(SELECT ir_b3 FROM _oo_docs.block_fts_done LIMIT 300))",
            )
            .await;
        let old_fact = fx
            .text(
                "EXPLAIN (ANALYZE, COSTS OFF, TIMING OFF) \
                 SELECT ft.id FROM _oo_docs.fact ft JOIN _oo_docs.entity s ON s.id = ft.subject_id \
                   LEFT JOIN _oo_docs.entity o ON o.id = ft.object_id \
                  WHERE ft.status = 'published' AND ft.retracted_in IS NULL \
                    AND to_tsvector('simple', _oo_docs.fts_fold(concat_ws(' ', s.canonical_name, \
                        replace(replace(ft.predicate, '.', ' '), '_', ' '), ft.value->>'text', ft.value->>'enum', \
                        ft.value->>'date', o.canonical_name))) @@ to_tsquery('simple', $$'project':* & '4242':*$$)",
            )
            .await;
        for (name, plan) in [
            ("FACT (indexed)", fact_plan),
            ("BLOCK (indexed)", block_plan),
            ("FACT (old expression, scan)", old_fact),
        ] {
            println!("--- {name}\n{}", plan.join("\n"));
            out.push_str(&format!("--- {name}\n{}\n", plan.join("\n")));
        }
    }
    if let Ok(p) = std::env::var("OO_BENCH_OUT") {
        std::fs::write(p, out).unwrap();
    }
}
