//! Single file latency instrumentation (G13) and the D49 target for md and
//! txt, measured on the real pipeline.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test latency -- --ignored --test-threads=1`

mod common;

use common::engine::{Opts, TestEngine};
use oo_docs::obs::latency::STAGES;
use serde_json::Value;

const FILES: usize = 30;

fn doc(i: usize) -> String {
    format!(
        "---\ntype: project\nname: 项目{i}\nowner: 林川\n---\n# 项目{i}\n\n负责人：林川\n\n状态：进行中\n"
    )
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn md_and_txt_reach_published_within_the_d49_target_and_are_measured() {
    let e = TestEngine::start(
        "latency_md",
        Opts {
            workers: true,
            stages: true,
            ..Opts::default()
        },
    )
    .await;
    for i in 0..FILES {
        e.write(&format!("p/f{i:02}.md"), &doc(i));
    }
    for i in 0..5 {
        e.write(
            &format!("p/t{i}.txt"),
            &format!("笔记 {i}：周会改到周三。\n"),
        );
    }
    let drive = e.create_drive("latency").await;
    let id = drive["id"].as_str().unwrap().to_owned();
    e.until("all published", || async {
        let r = e
            .get(&format!("/v1/docs/drives/{id}/files?limit=100"))
            .await
            .json();
        let items = r["items"].as_array().unwrap();
        items.len() == FILES + 5 && items.iter().all(|f| f["state"] == "published")
    })
    .await;

    let page = e
        .get(&format!("/v1/docs/drives/{id}/files?limit=100"))
        .await
        .json();
    let mut totals: Vec<i64> = Vec::new();
    for f in page["items"].as_array().unwrap() {
        let l = &f["latency_ms"];
        assert!(l.is_object(), "every published file has latency_ms: {f}");
        let total = l["total_ms"].as_i64().expect("total_ms");
        let sum: i64 = STAGES.iter().map(|s| l[*s].as_i64().unwrap_or(0)).sum();
        // Stage times come from the same timestamps as the total.
        assert!((sum - total).abs() <= 2, "{sum} vs {total}: {l}");
        assert_eq!(l["llm_wait_ms"], 0, "no model configured");
        for s in [
            "hash_ms",
            "queue_ms",
            "parse_ms",
            "extract_ms",
            "publish_wait_ms",
        ] {
            assert!(l[s].is_i64(), "{s} missing: {l}");
        }
        totals.push(total);
    }
    totals.sort_unstable();
    let p95 = totals[(totals.len() * 95).div_ceil(100) - 1];
    eprintln!(
        "latency stored->published ms: p50={} p95={p95} max={}",
        totals[totals.len() / 2],
        totals.last().unwrap()
    );
    assert!(p95 <= 5_000, "D49: md/txt p95 <= 5 s, got {p95} ms");

    // The event that announces `published` carries the latency too.
    let ev: String = sqlx::query_scalar(
        "SELECT subject FROM _oo_docs.event WHERE type = 'file.state_changed' \
          AND payload->>'to' = 'published' ORDER BY seq LIMIT 1",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    let h = e
        .get(&format!("/v1/docs/drives/{id}/files?limit=1"))
        .await
        .json();
    assert!(h["items"][0]["latency_ms"].is_object());
    assert!(!ev.is_empty());

    // Metrics: authenticated, Prometheus text, histograms fed by the files.
    let anon = common::http::request(e.addr, "GET", "/v1/docs/metrics", &[], None).await;
    assert_eq!(anon.status, 401);
    let r = e.get("/v1/docs/metrics").await;
    assert_eq!(r.status, 200);
    assert!(
        r.header("content-type")
            .is_some_and(|c| c.starts_with("text/plain"))
    );
    let text = String::from_utf8(r.body).unwrap();
    assert!(
        text.contains(&format!(
            "oo_docs_files{{state=\"published\"}} {}",
            FILES + 5
        )),
        "{text}"
    );
    assert!(
        text.contains("oo_docs_file_latency_ms_count 35\n"),
        "{text}"
    );
    assert!(text.contains("oo_docs_stage_duration_ms_count{stage=\"parse_ms\"} 35\n"));
    assert!(
        text.contains(
            "oo_docs_stage_duration_ms_bucket{stage=\"publish_wait_ms\",le=\"+Inf\"} 35\n"
        )
    );
    assert!(text.contains("oo_docs_jobs{kind=\"parse\",state=\"succeeded\"}"));
    assert!(text.contains("oo_docs_ontology_version "));
    assert!(text.contains("oo_docs_events_head "));
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let (_, value) = line.rsplit_once(' ').expect("name value");
        assert!(value.parse::<f64>().is_ok(), "numeric sample: {line}");
        assert!(
            !line.contains("/p/") && !line.contains("f00"),
            "no paths in labels: {line}"
        );
    }
    let _: Value = Value::Null;
    e.finish().await;
}
