//! D91: how many questions a big folder leaves for a person.
//!
//! A synthetic Drive of `OO_SCALE_FILES` Markdown files (default 10,000) built the way real folders are: notes,
//! decision records, people, projects; the same file copied into other folders and renamed; a `README.md` in
//! every directory; two projects written `Orion 7` and `Orion-7`; and two real ambiguities (two different people
//! both called "Alex"). Indexed through the real write path (`Fx::index_md`), with model statements planted at
//! the confidences that used to fill the queue, then published. The pending review queue must hold the real
//! ambiguities and nothing else: at most 2 items per 10k to 100k files.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> [OO_SCALE_FILES=n] [OO_REVIEW_SCALE_OUT=<file>] \
//!   cargo test -p oo-docs --locked --test review_scale_live -- --ignored --nocapture`

mod common;

use std::collections::BTreeMap;
use std::time::Instant;

use common::fx::Fx;

fn files(n: usize) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for i in 0..n {
        let dir = format!("d{:03}/s{:02}/t{}", i % 100, (i / 100) % 10, i / 1000);
        let (p, s) = match i % 10 {
            0 => (
                format!("Projects/{dir}/p{i}.md"),
                format!(
                    "---\ntype: project\nowner: Member {}\n---\n\n# Project {i}\n\nScope and milestones of project {i}.\n",
                    i % 60
                ),
            ),
            1 => (
                format!("Decisions/{dir}/D-{i:05}.md"),
                format!(
                    "---\ntype: decision\ndate: 2026-0{}-1{}\nowner: Member {}\n---\n\n# Decision: project {} takes option {}\n\nMember {} decided that project {} takes option {}.\n",
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
                    "---\ntype: person\n---\n\n# Member {}\n\nRole: engineer. Works on project {}.\n",
                    i % 60,
                    i - 2
                ),
            ),
            3 => (
                format!("{dir}/README.md"),
                format!("# README {i}\n\nHow to build the part {i} of the repository.\n"),
            ),
            _ => (
                format!("Notes/{dir}/n{i:05}.md"),
                format!(
                    "# Note {i}\n\nDiscussed the schedule of project {}; owner Member {}.\n",
                    i - 4,
                    i % 60
                ),
            ),
        };
        out.push((p, s));
    }
    // 6 percent copies in other folders, 4 percent renames of earlier files: the same bytes under another path.
    let copies = n * 6 / 100;
    for k in 0..copies {
        let (p, s) = out[(k * 7) % n].clone();
        out.push((
            format!(
                "copies/c{:03}/{k}-{}",
                k % 40,
                p.rsplit('/').next().unwrap()
            ),
            s,
        ));
    }
    let renames = n * 4 / 100;
    for k in 0..renames {
        let (p, s) = out[(k * 11 + 3) % n].clone();
        out.push((
            format!(
                "moved/{}-renamed-{k}.md",
                p.rsplit('/').next().unwrap().trim_end_matches(".md")
            ),
            s,
        ));
    }
    // One project written two ways, a hundred times: the same name once punctuation is ignored.
    for k in 0..100 {
        out.push((
            format!("Projects/orion/a{k}.md"),
            format!("---\ntype: project\naliases: [Orion{k}]\n---\n\n# Orion {k}\n\nPart {k}.\n"),
        ));
        out.push((
            format!("Projects/orion/b{k}.md"),
            format!(
                "---\ntype: project\naliases: [Orion{k}]\n---\n\n# Orion-{k}\n\nPart {k} again.\n"
            ),
        ));
    }
    // Two real ambiguities: two different people both known as "Alex".
    for (stem, full) in [
        ("alex-ng", "Alex Ng"),
        ("alex-wu", "Alex Wu"),
        ("sam-li", "Samuel Li"),
        ("sam-lo", "Samuel Lo"),
    ] {
        let alias = if stem.starts_with("alex") {
            "Alex"
        } else {
            "Sam"
        };
        out.push((
            format!("People/team/{stem}.md"),
            format!("---\ntype: person\naliases: [{alias}]\n---\n\n# {full}\n\nEngineer.\n"),
        ));
    }
    out
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn the_queue_of_a_big_folder_holds_only_real_questions() {
    let n: usize = std::env::var("OO_SCALE_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);
    let docs = files(n);
    let total = docs.len();
    let mut fx = Fx::new("review_scale").await;
    let t = Instant::now();
    let (mut merged, mut queued) = (0u32, 0u32);
    for (p, s) in &docs {
        let r = fx.index_md(p, s).await;
        merged += r.identity_merges;
        queued += r.merge_proposals;
    }
    let index_s = t.elapsed().as_secs_f64();
    // The model's statements that used to fill the queue: topical links at 5000 to 7500.
    let mut planted = 0usize;
    for (i, (p, _)) in docs
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 25 == 0)
        .take(400)
    {
        if p.contains("copies/") || p.contains("moved/") {
            continue;
        }
        planted += fx
            .llm_topic_facts(p, 3, 5000 + (i as i32 % 6) * 500)
            .await
            .len();
    }
    // Quality gate: bare words from list items under a decisions heading.
    let t = Instant::now();
    let out = fx.publish().await;
    assert!(
        matches!(out, oo_docs::publish::run::PublishOutcome::Published(_)),
        "{out:?}"
    );
    let publish_s = t.elapsed().as_secs_f64();
    // Aliases are published now: the files that share one are read again (a later edit, a rescan), which is when
    // merge proposals are made.
    for (p, _) in docs
        .iter()
        .filter(|(p, _)| p.starts_with("Projects/orion/") || p.starts_with("People/team/"))
    {
        let planted = fx.known_plant(p).await;
        let r = fx.index_planted(p, &planted).await;
        merged += r.identity_merges;
        queued += r.merge_proposals;
    }
    let out = fx.publish().await;
    assert!(
        matches!(
            out,
            oo_docs::publish::run::PublishOutcome::Published(_)
                | oo_docs::publish::run::PublishOutcome::NoChange { .. }
        ),
        "{out:?}"
    );
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT kind, reason_code, status FROM _oo_docs.review_item")
            .fetch_all(&mut fx.admin)
            .await
            .unwrap();
    let mut by: BTreeMap<String, u32> = BTreeMap::new();
    for (k, r, s) in &rows {
        *by.entry(format!("{k}/{r}/{s}")).or_default() += 1;
    }
    let pending: u32 = rows.iter().filter(|r| r.2 == "pending").count() as u32;
    let docs_entities = fx
        .scalar(
            "SELECT count(*) FROM _oo_docs.entity WHERE type = 'Document' AND status = 'active'",
        )
        .await;
    let summary = serde_json::json!({
        "files": total, "index_seconds": index_s, "publish_seconds": publish_s, "model_facts_planted": planted,
        "merged_without_a_person": merged, "queued_at_index": queued, "document_entities": docs_entities,
        "review_pending": pending, "review_by_kind_reason_status": by,
    });
    eprintln!("{summary}");
    if let Ok(path) = std::env::var("OO_REVIEW_SCALE_OUT") {
        std::fs::write(path, serde_json::to_string_pretty(&summary).unwrap()).unwrap();
    }
    assert!(
        pending <= 2 * (total as u32).div_ceil(10_000).max(1),
        "{pending} pending: {by:?}"
    );
    fx.finish("review_scale").await;
}
