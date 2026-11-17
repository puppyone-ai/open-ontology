//! parse -> extract -> publish -> packs -> shares over real HTTP and a real
//! PostgreSQL (B-S2-09 completion).
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test pipeline_live -- --ignored --test-threads=2`

mod common;

use std::time::Duration;

use common::engine::{Opts, TestEngine};
use serde_json::{Value, json};

const PROJECT: &str = "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n\n负责人：林川\n\n状态：进行中\n";
const PERSON: &str =
    "---\ntype: person\nname: 林川\nrole: 工程负责人\n---\n# 林川\n\n角色：工程负责人\n";
const DECISION: &str = "---\ntype: decision\nname: 采用本地优先\ndate: 2026-03-01\n---\n# 采用本地优先\n\n决定：数据只留在本机\n负责人：林川\n";

async fn states(e: &TestEngine, drive: &str) -> Vec<(String, String)> {
    let r = e
        .get(&format!("/v1/docs/drives/{drive}/files?limit=100"))
        .await;
    r.json()["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["path"].as_str().unwrap().to_owned(),
                f["state"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

async fn all_in(e: &TestEngine, drive: &str, want: &str, n: usize) -> bool {
    let s = states(e, drive).await;
    s.len() == n && s.iter().all(|(_, st)| st == want)
}

fn opts() -> Opts {
    Opts {
        workers: true,
        stages: true,
        ..Opts::default()
    }
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn files_reach_published_with_events_and_a_pack_is_served() {
    let e = TestEngine::start("pipe_all", opts()).await;
    e.write("Projects/a/星火计划.md", PROJECT);
    e.write("People/林川.md", PERSON);
    e.write("Decisions/本地优先.md", DECISION);
    e.write("notes.txt", "随手记：星火计划的周会改到周三。\n");
    let drive = e.create_drive("pipe-all").await;
    let did = drive["id"].as_str().unwrap().to_owned();

    e.until("all files published", || async {
        all_in(&e, &did, "published", 4).await
    })
    .await;

    // The state history reached published through every stage, with events.
    let ontology: i64 = sqlx::query_scalar("SELECT max(n) FROM _oo_docs.ontology_version")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert!(ontology >= 1);
    let tos: Vec<String> = sqlx::query_scalar(
        "SELECT to_state FROM _oo_docs.file_state_history WHERE file_id = \
         (SELECT id FROM _oo_docs.file WHERE path = 'People/林川.md') ORDER BY seq",
    )
    .fetch_all(&e.pool)
    .await
    .unwrap();
    assert_eq!(
        tos,
        [
            "stored",
            "queued",
            "parsing",
            "parsed",
            "extracting",
            "indexed",
            "published"
        ]
    );

    // Compile: 201 first, 200 (same pack) second.
    let body = json!({"drive_id": did, "scope": ["Projects/a"], "budget": 800});
    let r = e.post("/v1/docs/context-packs:compile", &body).await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let pack = r.json();
    let brief = pack["brief_preview"].as_str().unwrap();
    assert!(brief.contains("[src:"), "{brief}");
    let r2 = e.post("/v1/docs/context-packs:compile", &body).await;
    assert_eq!(r2.status, 200);
    assert_eq!(r2.json()["id"], pack["id"]);
    assert_eq!(r2.json()["pack_digest"], pack["pack_digest"]);

    // GET pack and every manifest file, bytes match the manifest.
    let pid = pack["id"].as_str().unwrap();
    let got = e.get(&format!("/v1/docs/context-packs/{pid}")).await;
    assert_eq!(got.status, 200);
    let files = pack["manifest"]["files"].as_array().unwrap();
    assert!(!files.is_empty());
    for f in files {
        let path = f["path"].as_str().unwrap();
        let enc: String = path
            .split('/')
            .map(|s| {
                s.bytes()
                    .map(|b| {
                        if b.is_ascii_alphanumeric() || b"-._".contains(&b) {
                            (b as char).to_string()
                        } else {
                            format!("%{b:02X}")
                        }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("/");
        let r = e
            .get(&format!("/v1/docs/context-packs/{pid}/files/{enc}"))
            .await;
        assert_eq!(r.status, 200, "{path}");
        assert_eq!(r.body.len() as u64, f["bytes"].as_u64().unwrap());
        assert_eq!(
            r.header("etag"),
            Some(format!("\"{}\"", f["sha256"].as_str().unwrap()).as_str())
        );
    }
    assert_eq!(
        e.get(&format!("/v1/docs/context-packs/{pid}/files/nope.md"))
            .await
            .status,
        404
    );
    assert_eq!(
        e.get("/v1/docs/context-packs/00000000-0000-4000-8000-000000000000")
            .await
            .json()["code"],
        "DOCS_PACK_NOT_FOUND"
    );

    // Shares.
    let sbody = json!({"compile": body, "mount": {"target_dir": "/tmp/oo-l2-unused"}});
    let s = e.post("/v1/docs/shares", &sbody).await;
    assert_eq!(s.status, 201, "{}", String::from_utf8_lossy(&s.body));
    let share: Value = s.json();
    assert_eq!(share["state"], "active");
    assert_eq!(share["pack_id"], pack["id"]);
    let sj: Value = serde_json::from_str(share["share_json"].as_str().unwrap()).unwrap();
    assert_eq!(sj["share_id"], share["id"]);
    assert_eq!(sj["pack_digest"], pack["pack_digest"]);
    assert!(
        sj["refs"]["credential"]
            .as_str()
            .unwrap()
            .starts_with("oodr_")
    );
    let got = e
        .get(&format!(
            "/v1/docs/shares/{}",
            share["id"].as_str().unwrap()
        ))
        .await;
    assert_eq!(got.status, 200);
    assert!(got.json().get("share_json").is_none());
    let uses: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo_docs.share_fact_use")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert!(uses > 0);
    let bad = e
        .post(
            "/v1/docs/shares",
            &json!({"compile": body, "mount": {"target_dir": "relative"}}),
        )
        .await;
    assert_eq!(bad.json()["code"], "DOCS_SHARE_TARGET_INVALID");
    let none = e
        .post(
            "/v1/docs/context-packs:compile",
            &json!({"drive_id": did, "scope": ["Projects/zzz"]}),
        )
        .await;
    assert_eq!(none.json()["code"], "DOCS_PACK_SCOPE_INVALID");
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn editing_a_file_runs_the_chain_again_and_the_brief_follows() {
    let e = TestEngine::start("pipe_edit", opts()).await;
    e.write("Projects/a/星火计划.md", PROJECT);
    e.write("People/林川.md", PERSON);
    let drive = e.create_drive("pipe-edit").await;
    let did = drive["id"].as_str().unwrap().to_owned();
    e.until("published", || async {
        all_in(&e, &did, "published", 2).await
    })
    .await;
    let body = json!({"drive_id": did, "scope": ["Projects/a"]});
    let first = e.post("/v1/docs/context-packs:compile", &body).await.json();
    assert!(first["brief_preview"].as_str().unwrap().contains("林川"));

    std::thread::sleep(Duration::from_millis(1100));
    e.write("Projects/a/星火计划.md", &PROJECT.replace("林川", "周远"));
    let r = e
        .post(&format!("/v1/docs/drives/{did}:sync"), &json!({}))
        .await;
    assert_eq!(r.status, 202, "{}", String::from_utf8_lossy(&r.body));
    e.until("republished with a new ontology version", || async {
        let n: i64 = sqlx::query_scalar("SELECT max(n) FROM _oo_docs.ontology_version")
            .fetch_one(&e.pool)
            .await
            .unwrap();
        n >= 2 && all_in(&e, &did, "published", 2).await
    })
    .await;
    let second = e.post("/v1/docs/context-packs:compile", &body).await.json();
    assert_ne!(first["pack_digest"], second["pack_digest"]);
    let brief = second["brief_preview"].as_str().unwrap();
    assert!(brief.contains("周远"), "{brief}");
    // The old owner is retracted, not merely hidden.
    let retracted: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.fact WHERE status = 'retracted' AND retracted_in IS NOT NULL",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert!(retracted >= 1);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_binary_or_empty_file_does_not_stall_the_rest() {
    let e = TestEngine::start("pipe_odd", opts()).await;
    e.write("Projects/a/星火计划.md", PROJECT);
    e.write("empty.md", "");
    std::fs::write(e.root.join("blob.txt"), [0u8, 1, 2, 0, 255]).unwrap();
    let drive = e.create_drive("pipe-odd").await;
    let did = drive["id"].as_str().unwrap().to_owned();
    e.until("settled", || async {
        let s = states(&e, &did).await;
        s.len() == 3
            && s.iter().all(|(p, st)| match p.as_str() {
                "blob.txt" => st == "parse_failed" || st == "unsupported",
                _ => st == "published" || st == "indexed",
            })
    })
    .await;
    e.finish().await;
}
