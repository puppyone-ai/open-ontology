//! 10,000 file Drive (B-S13-03, 10 11.3): listing and stat, cold index without an LLM, the file
//! list API and engine memory.
//!
//! Synthetic Markdown only. The Drive is indexed through the real write path (`Fx::index_md`: the
//! stage chain without the parser sidecar, which handles the binary formats and is measured by the
//! 1000 file compose bench), then published. Limits (11.3): list + stat <= 60 s; cold index
//! <= 5 h (the extrapolation from 1000 files, 30 min per 1000); file list p95 < 100 ms; engine
//! memory <= 1.5 GiB. `OO_BENCH_OUT=<file>` writes the numbers, `OO_SCALE_FILES=<n>` changes the
//! size (default 10000).
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --release --test bench_scale -- --ignored --nocapture`

mod common;

use std::time::Instant;

use common::fx::{DRIVE, Fx};
use oo_connectors::objectstore::fs::FsSource;
use oo_connectors::objectstore::{DocumentSource, ListCursor};
use oo_docs::api::files::{FileQuery, Sort, list_files};
use sqlx::Executor;

const LIST_P95_MS: f64 = 100.0;
const LIST_STAT_S: f64 = 60.0;
const INDEX_LIMIT_S: f64 = 5.0 * 3600.0;
const ENGINE_RSS_MIB: f64 = 1536.0;

fn files() -> usize {
    std::env::var("OO_SCALE_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        // A debug build only smoke-tests the bench; the product size is for the release build.
        .unwrap_or(if cfg!(debug_assertions) { 500 } else { 10_000 })
}

fn doc(i: usize) -> (String, String) {
    let dir = format!("d{:03}/s{:02}", i % 100, (i / 100) % 10);
    match i % 4 {
        0 => (
            format!("Projects/{dir}/project-{i}.md"),
            format!(
                "---\ntype: project\nname: Project {i}\nowner: Member {}\nstatus: active\n---\n\n# Project {i}\n\nOwner: Member {}\n\nStatus: active\n\nScope, milestones and risks of project {i}.\n",
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
        _ => (
            format!("Notes/{dir}/n{i:05}.md"),
            format!(
                "# Note {i}\n\nDiscussed the schedule of project {}; owner Member {}.\n",
                i - 3,
                i % 60
            ),
        ),
    }
}

fn rss_mib() -> f64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<f64>()
        .unwrap_or(0.0)
        / 1024.0
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((v.len() as f64) * p).ceil() as usize;
    v[idx.saturating_sub(1).min(v.len() - 1)]
}

fn query(path: &str, recursive: bool, states: &[&str], q: Option<&str>, sort: Sort) -> FileQuery {
    FileQuery {
        drive_id: DRIVE.to_owned(),
        path: path.to_owned(),
        recursive,
        states: states.iter().map(|s| (*s).to_owned()).collect(),
        q: q.map(str::to_owned),
        sort,
        limit: 100,
        cursor: None,
    }
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn ten_thousand_file_drive() {
    let n = files();
    // 1. list + stat of a real directory tree through the fs source.
    let root = std::env::temp_dir().join(format!("oo-scale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for i in 0..n {
        let (p, s) = doc(i);
        let full = root.join(&p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, s).unwrap();
    }
    let t = Instant::now();
    let src = FsSource::open(&root, Default::default()).unwrap();
    let mut cursor: Option<ListCursor> = None;
    let mut listed = 0usize;
    loop {
        let page = src.list(cursor.as_ref(), 1000).await.unwrap();
        for e in &page.entries {
            let st = src.stat(&e.raw_key).await.unwrap();
            assert!(st.is_some());
            listed += 1;
        }
        match page.next {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    let list_stat_s = t.elapsed().as_secs_f64();
    assert_eq!(listed, n);
    let _ = std::fs::remove_dir_all(&root);

    // 2. cold index of the whole Drive, no LLM, then publish.
    let mut fx = Fx::new("bench_scale").await;
    let t = Instant::now();
    for i in 0..n {
        let (path, src) = doc(i);
        fx.index_md(&path, &src).await;
    }
    let index_s = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let out = fx.publish().await;
    assert!(
        matches!(out, oo_docs::publish::run::PublishOutcome::Published(_)),
        "{out:?}"
    );
    let publish_s = t.elapsed().as_secs_f64();
    let rss = rss_mib();
    let published = fx
        .scalar("SELECT count(*) FROM _oo_docs.file WHERE state = 'published'")
        .await;
    assert_eq!(published as usize, n);
    // planner statistics are what a running engine has after autovacuum
    fx.admin
        .execute("ANALYZE _oo_docs.file")
        .await
        .expect("analyze");

    // 3. the file list API: first pages of several shapes, then a full keyset walk.
    let mut times = Vec::new();
    let shapes = [
        query("", true, &[], None, Sort::Path),
        query("", true, &["published"], None, Sort::StateSince),
        query("", true, &[], None, Sort::ModifiedAt),
        query("Projects/d000/s00", false, &[], None, Sort::Path),
        query("Decisions/d005", true, &[], None, Sort::Path),
        query("", true, &[], Some("member-7"), Sort::Path),
    ];
    for _ in 0..20 {
        for s in &shapes {
            let t = Instant::now();
            let v = list_files(&mut fx.rt, s).await.expect("list");
            times.push(t.elapsed().as_secs_f64() * 1000.0);
            assert!(
                v["items"]
                    .as_array()
                    .is_some_and(|a| !a.is_empty() || s.q.is_some())
            );
        }
    }
    let mut walk = Vec::new();
    let mut q = query("", true, &[], None, Sort::Path);
    let mut seen = 0usize;
    loop {
        let t = Instant::now();
        let v = list_files(&mut fx.rt, &q).await.expect("walk");
        walk.push(t.elapsed().as_secs_f64() * 1000.0);
        seen += v["items"].as_array().unwrap().len();
        match v["next_cursor"].as_str() {
            Some(c) => q.cursor = Some(c.to_owned()),
            None => break,
        }
    }
    assert_eq!(seen, n);
    times.extend(walk.iter().copied());
    let (p50, p95, max) = (
        pct(&mut times.clone(), 0.5),
        pct(&mut times.clone(), 0.95),
        pct(&mut times.clone(), 1.0),
    );
    let deep = pct(&mut walk.clone(), 0.95);
    eprintln!(
        "scale bench: {n} files; list+stat {list_stat_s:.1}s; index {index_s:.1}s publish {publish_s:.1}s; engine rss {rss:.0} MiB; list p50 {p50:.1} p95 {p95:.1} max {max:.1} ms (walk p95 {deep:.1} ms, {} pages)",
        walk.len()
    );
    let facts = fx
        .scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'published'")
        .await;
    if let Ok(path) = std::env::var("OO_BENCH_OUT") {
        let v = serde_json::json!({
            "protocol": "oo.bench.scale/v1", "files": n, "facts": facts,
            "list_stat_s": list_stat_s, "list_stat_limit_s": LIST_STAT_S,
            "index_s": index_s, "publish_s": publish_s, "index_limit_s": INDEX_LIMIT_S,
            "index_files_per_s": n as f64 / index_s,
            "engine_rss_mib": rss, "engine_rss_limit_mib": ENGINE_RSS_MIB,
            "list_ms": {"p50": p50, "p95": p95, "max": max, "walk_p95": deep, "samples": times.len()},
            "list_p95_limit_ms": LIST_P95_MS,
            "build": if cfg!(debug_assertions) { "debug" } else { "release" },
        });
        std::fs::write(path, serde_json::to_string_pretty(&v).unwrap() + "\n").unwrap();
    }
    if !cfg!(debug_assertions) {
        assert!(list_stat_s <= LIST_STAT_S, "list + stat {list_stat_s:.1}s");
        assert!(
            index_s + publish_s <= INDEX_LIMIT_S,
            "cold index {index_s:.0}s"
        );
        assert!(rss <= ENGINE_RSS_MIB, "engine rss {rss:.0} MiB");
        assert!(p95 < LIST_P95_MS, "list p95 {p95:.1} ms");
    }
    fx.finish("bench_scale").await;
}
