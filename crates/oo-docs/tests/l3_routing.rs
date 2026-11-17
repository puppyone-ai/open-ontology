//! Pipeline routing by sniffed format against the real engine binary and the
//! real Docling sidecar container (B-S3-05, 30 L3 acceptance 1 and 4).
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test l3_routing -- --ignored --nocapture --test-threads=1`
//!
//! Needs docker and the `oo-docs-parser:dev` image (or `OO_SIDECAR_ADDR` and
//! `OO_SIDECAR_TOKEN` for a running sidecar).

mod common;
mod sidecar_support;
#[path = "sidecar_support/stack.rs"]
mod stack;

use std::time::{Duration, Instant};

use sidecar_support::testing_root;
use stack::{Stack, copy_tree};

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT and docker (oo-docs-parser:dev)"]
async fn every_format_is_routed_and_hostile_files_get_their_verdict() {
    let s = Stack::start("l3_routing", 2).await;
    let golden = testing_root().join("golden/docs");
    let n = copy_tree(&golden, &s.root);
    let fx = testing_root().join("fixtures/files");
    let hostile = [
        "docx-encrypted.docx",
        "pdf-encrypted.pdf",
        "legacy.doc",
        "zip-bomb-ratio.docx",
        "zip-bomb-size.xlsx",
        "zip-plain.zip",
        "zip-traversal.docx",
        "gb18030.txt",
        "pdf-table.pdf",
        "xlsx-merged-regions.xlsx",
        "comma.csv",
    ];
    for h in hostile {
        std::fs::copy(fx.join(h), s.root.join(format!("fx-{h}"))).unwrap();
    }
    s.register_drive().await;
    let t0 = Instant::now();
    let st = s.settled(n + hostile.len(), Duration::from_secs(300)).await;
    eprintln!(
        "settled {} files in {:.1}s",
        st.len(),
        t0.elapsed().as_secs_f64()
    );
    for (p, (state, code)) in &st {
        if p.starts_with("fx-") || state != "published" {
            eprintln!("{p}: {state} {code:?}");
        }
    }
    let golden_bad: Vec<_> = st
        .iter()
        .filter(|(p, _)| !p.starts_with("fx-"))
        .filter(|(_, (s, _))| s != "published")
        .collect();
    assert!(
        golden_bad.is_empty(),
        "golden files not published: {golden_bad:?}"
    );
    let want = |p: &str, state: &str, code: Option<&str>| {
        let got = st
            .get(&format!("fx-{p}"))
            .unwrap_or_else(|| panic!("{p} missing"));
        assert_eq!((got.0.as_str(), got.1.as_deref()), (state, code), "{p}");
    };
    want(
        "docx-encrypted.docx",
        "encrypted",
        Some("DOCS_PARSE_ENCRYPTED"),
    );
    want(
        "pdf-encrypted.pdf",
        "encrypted",
        Some("DOCS_PARSE_ENCRYPTED"),
    );
    want(
        "legacy.doc",
        "unsupported",
        Some("DOCS_PARSE_LEGACY_OFFICE"),
    );
    want(
        "zip-bomb-ratio.docx",
        "unsupported",
        Some("DOCS_PARSE_ZIP_BOMB"),
    );
    want(
        "zip-bomb-size.xlsx",
        "unsupported",
        Some("DOCS_PARSE_ZIP_BOMB"),
    );
    want(
        "zip-plain.zip",
        "unsupported",
        Some("DOCS_PARSE_UNSUPPORTED_FORMAT"),
    );
    want("gb18030.txt", "unsupported", Some("DOCS_PARSE_ENCODING"));
    want("pdf-table.pdf", "published", None);
    want("xlsx-merged-regions.xlsx", "published", None);
    want("comma.csv", "published", None);
    s.finish().await;
}

async fn count(s: &Stack, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&s.pool).await.unwrap()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT and docker (oo-docs-parser:dev)"]
async fn ten_copies_of_one_content_parse_once_and_nine_are_duplicates() {
    let s = Stack::start("l3_dups", 2).await;
    let golden = testing_root().join("golden/docs");
    let pdf = std::fs::read_dir(golden.join("Proposals"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "pdf"))
        .or_else(|| {
            walk(&golden)
                .into_iter()
                .find(|p| p.extension().is_some_and(|e| e == "pdf"))
        })
        .expect("a golden pdf");
    let md = walk(&golden)
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e == "md"))
        .expect("a golden md");
    for i in 0..10 {
        std::fs::copy(&pdf, s.root.join(format!("copy-{i}.pdf"))).unwrap();
        std::fs::copy(&md, s.root.join(format!("copy-{i}.md"))).unwrap();
    }
    s.register_drive().await;
    let st = s.settled(20, Duration::from_secs(240)).await;
    let by_state = |ext: &str| {
        let mut m = std::collections::BTreeMap::<String, usize>::new();
        for (p, (state, _)) in &st {
            if p.ends_with(ext) {
                *m.entry(state.clone()).or_default() += 1;
            }
        }
        m
    };
    for ext in [".pdf", ".md"] {
        let m = by_state(ext);
        eprintln!("{ext}: {m:?}");
        assert_eq!(m.get("published"), Some(&1), "{ext}: one file leads {m:?}");
        assert_eq!(m.get("duplicate"), Some(&9), "{ext}: nine duplicates {m:?}");
    }
    let parses = count(
        &s,
        "SELECT count(*) FROM _oo_docs.job WHERE kind = 'parse' AND state = 'succeeded'",
    )
    .await;
    assert_eq!(parses, 2, "one parse job per content, not one per path");
    let results = count(
        &s,
        "SELECT count(*) FROM _oo_docs.parse_result WHERE status = 'complete'",
    )
    .await;
    assert_eq!(results, 2, "one parse result per content");
    // Every path keeps its own evidence (4.7.1).
    let per_path: Vec<(String, i64)> = sqlx::query_as(
        "SELECT f.path, count(e.id) FROM _oo_docs.file f \
           JOIN _oo_docs.evidence e ON e.version_id = f.current_version_id AND e.withdrawn_at IS NULL \
          GROUP BY f.path ORDER BY f.path",
    )
    .fetch_all(&s.pool)
    .await
    .unwrap();
    let pdfs: Vec<_> = per_path
        .iter()
        .filter(|(p, _)| p.ends_with(".pdf"))
        .collect();
    let mds: Vec<_> = per_path
        .iter()
        .filter(|(p, _)| p.ends_with(".md"))
        .collect();
    eprintln!("evidence per path: pdf {pdfs:?} md {mds:?}");
    assert_eq!(pdfs.len(), 10, "every pdf path has evidence");
    assert_eq!(mds.len(), 10, "every md path has evidence");
    assert!(pdfs.iter().all(|(_, n)| *n == pdfs[0].1));
    assert!(mds.iter().all(|(_, n)| *n == mds[0].1));
    s.finish().await;
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.file_type().unwrap().is_dir() {
            out.extend(walk(&e.path()));
        } else {
            out.push(e.path());
        }
    }
    out.sort();
    out
}
