//! Rule output, resolve and the publisher against one throwaway PostgreSQL
//! (B-S2-07). The tests run as a login member of `oo_docs_runtime`; the
//! superuser connection only plants drives, files and versions.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test publish_live -- --ignored`

mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use common::*;
use oo_docs::cas::{Cas, CasKind};
use oo_docs::parse::markdown::parse_markdown;
use oo_docs::publish::run::{PUBLISH_LOCK, PublishConfig, PublishOutcome, publish};
use oo_docs::publish::write::{DocumentInput, index_document};
use sqlx::{Executor, PgConnection};

const DRIVE: &str = "00000000-0000-4000-8000-0000000000d1";
static SEQ: AtomicU64 = AtomicU64::new(1);

fn uuid() -> String {
    format!(
        "00000000-0000-4000-8000-{:012x}",
        SEQ.fetch_add(1, Ordering::SeqCst)
    )
}

fn golden() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testing/super-ontology/golden")
}

struct Fx {
    cas: Cas,
    dir: PathBuf,
    admin: PgConnection,
    rt: PgConnection,
}

impl Fx {
    async fn new(db: &str) -> Self {
        installed_db(db).await;
        let mut admin = connect(db).await;
        admin
            .execute(sqlx::AssertSqlSafe(format!(
                "INSERT INTO _oo_docs.drive (id, name, backend, config) VALUES ('{DRIVE}', 'd', 'fs', '{{}}')"
            )))
            .await
            .unwrap();
        let dir = std::env::temp_dir().join(format!("oo-docs-pub-{}-{db}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cas = Cas::open(&dir).expect("cas");
        let rt = connect_runtime(db).await;
        Self {
            cas,
            dir,
            admin,
            rt,
        }
    }

    /// Plant blob, file (state `indexed`), version and a complete parse.
    async fn plant(&mut self, path: &str, bytes: &[u8]) -> (String, String, oo_core::docir::DocIr) {
        let n = parse_markdown(bytes).expect("parse");
        let b3 = n.ir.blob_b3.clone();
        let put = self
            .cas
            .put(CasKind::Ir, n.canonical.as_bytes())
            .expect("cas put");
        assert_eq!(put.b3, n.ir_b3);
        let (file_id, version_id) = (uuid(), uuid());
        let a = &mut self.admin;
        sqlx::query(
            "INSERT INTO _oo_docs.blob (b3, sha256, size, format) VALUES ($1, $2, $3, 'md') ON CONFLICT DO NOTHING",
        )
        .bind(&b3)
        .bind("0".repeat(64))
        .bind(i64::try_from(bytes.len()).unwrap())
        .execute(&mut *a)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO _oo_docs.file (id, drive_id, path, raw_key, path_fold, state, last_seen_generation) \
             VALUES ($1::uuid, $2::uuid, $3, convert_to($3, 'UTF8'), lower($3), 'indexed', 1)",
        )
        .bind(&file_id)
        .bind(DRIVE)
        .bind(path)
        .execute(&mut *a)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO _oo_docs.file_version (id, file_id, b3, path_at_version, fp) \
             VALUES ($1::uuid, $2::uuid, $3, $4, '{}'::jsonb)",
        )
        .bind(&version_id)
        .bind(&file_id)
        .bind(&b3)
        .bind(path)
        .execute(&mut *a)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO _oo_docs.parse_result (b3, parser_id, parser_version, profile_sha256, status, ir_b3, block_count, finished_at) \
             VALUES ($1, 'oo-md', '1', $2, 'complete', $3, $4, clock_timestamp()) ON CONFLICT DO NOTHING",
        )
        .bind(&b3)
        .bind("0".repeat(64))
        .bind(&n.ir_b3)
        .bind(i32::try_from(n.ir.blocks.len()).unwrap())
        .execute(&mut *a)
        .await
        .unwrap();
        (version_id, b3, n.ir)
    }

    async fn index(&mut self, path: &str, bytes: &[u8]) -> oo_docs::publish::write::IndexSummary {
        let (version_id, b3, ir) = self.plant(path, bytes).await;
        let doc = DocumentInput {
            version_id: &version_id,
            b3: &b3,
            path,
            ir: &ir,
        };
        index_document(&mut self.rt, &doc).await.expect("index")
    }

    async fn publish(&mut self) -> PublishOutcome {
        publish(&mut self.rt, &self.cas, &PublishConfig::default())
            .await
            .expect("publish")
    }

    async fn scalar(&mut self, sql: &'static str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(&mut self.admin)
            .await
            .unwrap()
    }

    async fn finish(self, db: &str) {
        let Fx { dir, admin, rt, .. } = self;
        drop(rt);
        drop(admin);
        let _ = std::fs::remove_dir_all(&dir);
        drop_db(db).await;
    }
}

fn md_files() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for dir in ["People", "Projects", "Decisions", "Preferences"] {
        let base = golden().join("docs").join(dir);
        let mut names: Vec<_> = std::fs::read_dir(&base)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        names.sort();
        for p in names {
            if p.extension().is_some_and(|e| e == "md") {
                let rel = format!("{dir}/{}", p.file_name().unwrap().to_string_lossy());
                out.push((rel, std::fs::read(&p).unwrap()));
            }
        }
    }
    out
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn publish_golden_md_subset_makes_v1() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_pub_golden";
    let mut fx = Fx::new(db).await;
    let files = md_files();
    assert_eq!(files.len(), 18);
    for (path, bytes) in &files {
        let s = fx.index(path, bytes).await;
        assert!(s.entities_created > 0 || s.evidence_added > 0, "{path}");
    }
    let drafts = fx
        .scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'draft'")
        .await;
    assert!(drafts > 50, "drafts {drafts}");
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE status <> 'draft'")
            .await,
        0
    );

    let PublishOutcome::Published(v) = fx.publish().await else {
        panic!("expected a version")
    };
    assert_eq!(v.n, 1);
    assert_eq!(i64::from(v.facts_added), drafts);
    assert_eq!((v.blocked_facts, v.blocked_aliases), (0, 0));
    assert_eq!(v.files_published, 18);
    let gates: Vec<(String, String)> = v
        .gates
        .iter()
        .map(|g| (g.gate_id.to_string(), g.disposition.to_string()))
        .collect();
    assert_eq!(
        gates,
        [
            ("DOC-G1".into(), "pass".into()),
            ("DOC-G2".into(), "pass".into()),
            ("DOC-G3".into(), "pass".into()),
            ("DOC-G4".into(), "pass".into()),
            ("DOC-G5".into(), "pass".into())
        ]
    );

    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.ontology_version")
            .await,
        1
    );
    assert_eq!(fx.scalar("SELECT count(*) FROM _oo_docs.ontology_version WHERE n = 1 AND parent IS NULL AND digest ~ '^[0-9a-f]{64}$' AND gate_report_sha256 ~ '^[0-9a-f]{64}$'").await, 1);
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.gate_report WHERE n = 1 AND disposition = 'pass'")
            .await,
        5
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact WHERE status = 'published' AND published_in = 1"
        )
        .await,
        drafts
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.alias WHERE status = 'draft'")
            .await,
        0
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.file WHERE state = 'published'")
            .await,
        18
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.event WHERE type = 'ontology.version_published'")
            .await,
        1
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id JOIN _oo_docs.entity o ON o.id = f.object_id \
                   WHERE s.canonical_name = '陈子墨' AND f.predicate = 'owns' AND o.canonical_name = '星图看板' AND f.valid_to = DATE '2026-05-31' AND f.status = 'published'").await,
        1
    );
    // Every published fact has evidence.
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact f WHERE NOT EXISTS (SELECT 1 FROM _oo_docs.evidence e WHERE e.target_kind = 'fact' AND e.target_id = f.id)").await,
        0
    );

    // Re-indexing the same versions adds nothing; publishing again is a no-op.
    let before = fx.scalar("SELECT count(*) FROM _oo_docs.evidence").await;
    let facts_before = fx.scalar("SELECT count(*) FROM _oo_docs.fact").await;
    for (path, bytes) in &files {
        let (version_id, b3, ir) = {
            let n = parse_markdown(bytes).unwrap();
            let vid: String = sqlx::query_scalar(
                "SELECT fv.id::text FROM _oo_docs.file_version fv WHERE fv.path_at_version = $1",
            )
            .bind(path)
            .fetch_one(&mut fx.admin)
            .await
            .unwrap();
            (vid, n.ir.blob_b3.clone(), n.ir)
        };
        let doc = DocumentInput {
            version_id: &version_id,
            b3: &b3,
            path,
            ir: &ir,
        };
        let s = index_document(&mut fx.rt, &doc).await.unwrap();
        assert_eq!(s, Default::default(), "{path}");
    }
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.evidence").await,
        before
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact").await,
        facts_before
    );
    assert!(matches!(
        fx.publish().await,
        PublishOutcome::NoChange { .. }
    ));
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.ontology_version")
            .await,
        1
    );
    fx.finish(db).await;
}

async fn plant_bad_fact(
    admin: &mut PgConnection,
    subject: &str,
    predicate: &str,
    object: &str,
    evidence: Option<(&str, &str, &str)>,
) -> String {
    let id = uuid();
    sqlx::query(
        "INSERT INTO _oo_docs.fact (id, fact_key, subject_id, original_subject_id, predicate, object_id, original_object_id, \
                                    confidence_bp, origin, rule_id, status) \
         SELECT $1::uuid, $2, s.id, s.id, $3, o.id, o.id, 10000, 'rule', 'test', 'draft' \
           FROM _oo_docs.entity s, _oo_docs.entity o WHERE s.canonical_name = $4 AND o.canonical_name = $5",
    )
    .bind(&id)
    .bind(format!("{:0>64}", id.replace('-', "")))
    .bind(predicate)
    .bind(subject)
    .bind(object)
    .execute(&mut *admin)
    .await
    .unwrap();
    if let Some((version_id, b3, block_id)) = evidence {
        // Copy a real anchor from an existing evidence row of that version, so
        // only the named defect is present.
        sqlx::query(
            "INSERT INTO _oo_docs.evidence (id, target_kind, target_id, version_id, b3, block_id, anchor, quote_b3, origin) \
             SELECT gen_random_uuid(), 'fact', $1::uuid, $2::uuid, $3, $4, e.anchor, e.quote_b3, 'rule' \
               FROM _oo_docs.evidence e WHERE e.version_id = $2::uuid LIMIT 1",
        )
        .bind(&id)
        .bind(version_id)
        .bind(b3)
        .bind(block_id)
        .execute(&mut *admin)
        .await
        .unwrap();
    }
    id
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn publish_blocks_bad_items_and_publishes_the_rest() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_pub_gates";
    let mut fx = Fx::new(db).await;
    let md = "---\ntype: project\n---\n# 星图\n\n成员：吴启明\n".as_bytes();
    fx.index("Projects/星图.md", md).await;
    let (version_id, b3): (String, String) =
        sqlx::query_as("SELECT fv.id::text, fv.b3 FROM _oo_docs.file_version fv LIMIT 1")
            .fetch_one(&mut fx.admin)
            .await
            .unwrap();
    let real_block: String =
        sqlx::query_scalar("SELECT block_id FROM _oo_docs.evidence WHERE version_id = $1::uuid ORDER BY block_id LIMIT 1")
            .bind(&version_id).fetch_one(&mut fx.admin).await.unwrap();
    // G1: Project owns Person (domain violation), with good evidence.
    let g1_bad = plant_bad_fact(
        &mut fx.admin,
        "星图",
        "owns",
        "吴启明",
        Some((&version_id, &b3, &real_block)),
    )
    .await;
    // G2: no evidence at all.
    let g2_none = plant_bad_fact(&mut fx.admin, "吴启明", "member_of", "星图", None).await;
    // G2: evidence names a block the IR does not have.
    let g2_block = plant_bad_fact(
        &mut fx.admin,
        "吴启明",
        "owns",
        "星图",
        Some((&version_id, &b3, "blk_00000000000000000000")),
    )
    .await;

    let PublishOutcome::Published(v) = fx.publish().await else {
        panic!("expected a version")
    };
    assert_eq!(v.n, 1);
    assert_eq!(v.blocked_facts, 3);
    let by_id: std::collections::BTreeMap<_, _> = v.gates.iter().map(|g| (g.gate_id, g)).collect();
    assert_eq!(by_id["DOC-G1"].disposition, "warn");
    assert_eq!(by_id["DOC-G1"].detail["reasons"]["schema_domain"], 1);
    assert_eq!(by_id["DOC-G2"].disposition, "warn");
    assert_eq!(by_id["DOC-G2"].detail["reasons"]["no_evidence"], 1);
    assert_eq!(by_id["DOC-G2"].detail["reasons"]["block_missing"], 1);
    for id in [&g1_bad, &g2_none, &g2_block] {
        let st: String = sqlx::query_scalar("SELECT status FROM _oo_docs.fact WHERE id = $1::uuid")
            .bind(id)
            .fetch_one(&mut fx.admin)
            .await
            .unwrap();
        assert_eq!(st, "draft", "blocked items stay draft");
    }
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'published'")
            .await,
        i64::from(v.facts_added)
    );
    assert!(v.facts_added > 0);
    let (kind, reason): (String, String) =
        sqlx::query_as("SELECT kind, reason_code FROM _oo_docs.review_item")
            .fetch_one(&mut fx.admin)
            .await
            .unwrap();
    assert_eq!(
        (kind.as_str(), reason.as_str()),
        ("low_confidence_fact", "schema")
    );
    // A second run neither republishes nor duplicates the review item.
    assert!(matches!(
        fx.publish().await,
        PublishOutcome::NoChange {
            blocked_facts: 3,
            ..
        }
    ));
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.review_item").await,
        1
    );
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn publish_is_single_writer_and_chains_versions() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_pub_chain";
    let mut fx = Fx::new(db).await;
    fx.index("People/甲.md", "# 甲\n".as_bytes()).await;
    // Another session holds the publisher lock.
    let mut other = connect(db).await;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(PUBLISH_LOCK)
        .execute(&mut other)
        .await
        .unwrap();
    assert_eq!(fx.publish().await, PublishOutcome::Busy);
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.ontology_version")
            .await,
        0
    );
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(PUBLISH_LOCK)
        .execute(&mut other)
        .await
        .unwrap();
    let PublishOutcome::Published(v1) = fx.publish().await else {
        panic!("v1")
    };
    assert_eq!(v1.n, 1);

    fx.index("People/乙.md", "# 乙\n".as_bytes()).await;
    let PublishOutcome::Published(v2) = fx.publish().await else {
        panic!("v2")
    };
    assert_eq!(v2.n, 2);
    assert_ne!(v1.digest, v2.digest);
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.ontology_version WHERE n = 2 AND parent = 1")
            .await,
        1
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.gate_report WHERE n = 2")
            .await,
        5
    );

    // A retraction alone makes a version and stamps retracted_in.
    sqlx::query("UPDATE _oo_docs.fact SET status = 'retracted', retracted_at = clock_timestamp() WHERE id = (SELECT id FROM _oo_docs.fact WHERE published_in = 1 ORDER BY fact_key LIMIT 1)")
        .execute(&mut fx.admin).await.unwrap();
    let PublishOutcome::Published(v3) = fx.publish().await else {
        panic!("v3")
    };
    assert_eq!((v3.n, v3.facts_retracted, v3.facts_added), (3, 1, 0));
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE retracted_in = 3")
            .await,
        1
    );
    // Versions are append-only for the runtime role.
    let err = sqlx::query("UPDATE _oo_docs.ontology_version SET facts_added = 0")
        .execute(&mut fx.rt)
        .await;
    assert!(err.is_err());
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn resolve_binds_published_aliases_and_queues_ties() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_pub_resolve";
    let mut fx = Fx::new(db).await;
    fx.index(
        "People/陈子墨.md",
        "---\naliases: [小陈]\n---\n# 陈子墨\n".as_bytes(),
    )
    .await;
    // Alias is still a draft: it must not bind.
    fx.index("Projects/甲.md", "# 甲\n\n成员：小陈\n".as_bytes())
        .await;
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.entity WHERE type = 'Person' AND canonical_name = '小陈'"
        )
        .await,
        1
    );
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("v1")
    };
    // A published alias binds a new mention to its owner.
    fx.index(
        "People/王五.md",
        "---\naliases: [五哥]\n---\n# 王五\n".as_bytes(),
    )
    .await;
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("v2")
    };
    let s = fx
        .index("Projects/乙.md", "# 乙\n\n成员：五哥\n".as_bytes())
        .await;
    assert_eq!(s.alias_conflicts, 0);
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id JOIN _oo_docs.entity o ON o.id = f.object_id \
                   WHERE f.predicate = 'member_of' AND s.canonical_name = '王五' AND o.canonical_name = '乙'").await,
        1
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.entity WHERE type = 'Person' AND canonical_name = '五哥'"
        )
        .await,
        0,
        "no new 五哥"
    );

    // Two people share a published alias: a third mention is a tie.
    fx.index(
        "People/李一.md",
        "---\naliases: [小李]\n---\n# 李一\n".as_bytes(),
    )
    .await;
    fx.index(
        "People/李二.md",
        "---\naliases: [小李]\n---\n# 李二\n".as_bytes(),
    )
    .await;
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("v3")
    };
    let s = fx
        .index("Projects/丙.md", "# 丙\n\n成员：小李\n".as_bytes())
        .await;
    assert_eq!(s.alias_conflicts, 1);
    let (kind, reason): (String, String) = sqlx::query_as(
        "SELECT kind, reason_code FROM _oo_docs.review_item WHERE kind = 'alias_conflict'",
    )
    .fetch_one(&mut fx.admin)
    .await
    .unwrap();
    assert_eq!(
        (kind.as_str(), reason.as_str()),
        ("alias_conflict", "alias_tie")
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.entity WHERE type = 'Person' AND canonical_name = '小李'"
        )
        .await,
        1
    );
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn compile_from_the_store_matches_the_memory_reader_and_respects_scope() {
    use oo_docs::compile::{CompileRequest, PackError, compile_from_store};
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_pub_compile";
    let mut fx = Fx::new(db).await;
    for (path, bytes) in &md_files() {
        fx.index(path, bytes).await;
    }
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("v1")
    };
    // An out-of-scope file that must never appear in a Projects-only pack.
    fx.index(
        "Secret/账本.md",
        "---\ntype: project\n---\n# 隐藏项目\n\n负责人：秘密人物\n".as_bytes(),
    )
    .await;
    let PublishOutcome::Published(v2) = fx.publish().await else {
        panic!("v2")
    };
    assert_eq!(v2.n, 2);
    let rq = |scope: &[&str]| CompileRequest {
        scope: scope.iter().map(|s| (*s).to_owned()).collect(),
        task: "星图看板".into(),
        ..CompileRequest::default()
    };
    let a = compile_from_store(&mut fx.rt, &fx.cas, DRIVE, &rq(&["Projects"]))
        .await
        .expect("compile");
    let b = compile_from_store(&mut fx.rt, &fx.cas, DRIVE, &rq(&["Projects"]))
        .await
        .expect("compile");
    assert_eq!(a.pack_digest, b.pack_digest);
    assert_eq!(a.version.n, 2);
    for f in &a.files {
        let t = f.as_str();
        assert!(
            !t.contains("隐藏项目") && !t.contains("秘密人物") && !t.contains("Secret/"),
            "{} leaks",
            f.path
        );
    }
    assert!(a.blocked.facts > 0 && a.blocked.entities > 0);
    let brief = a.file("BRIEF.md").unwrap().as_str();
    assert!(brief.contains("- 解析出的对象：星图看板（Project）"));
    // Widening the scope reveals it.
    let all = compile_from_store(&mut fx.rt, &fx.cas, DRIVE, &rq(&[]))
        .await
        .expect("compile");
    assert!(all.files.iter().any(|f| f.as_str().contains("隐藏项目")));
    // Pinning version 1 hides the later file even for the whole drive.
    let mut v1 = rq(&[]);
    v1.version = Some(1);
    let p1 = compile_from_store(&mut fx.rt, &fx.cas, DRIVE, &v1)
        .await
        .expect("compile");
    assert!(p1.files.iter().all(|f| !f.as_str().contains("隐藏项目")));
    // Errors keep their codes.
    assert_eq!(
        compile_from_store(&mut fx.rt, &fx.cas, DRIVE, &rq(&["Nope"]))
            .await
            .unwrap_err(),
        PackError::ScopeInvalid {
            paths: vec!["Nope".into()]
        }
    );
    let mut v9 = rq(&[]);
    v9.version = Some(9);
    assert_eq!(
        compile_from_store(&mut fx.rt, &fx.cas, DRIVE, &v9)
            .await
            .unwrap_err(),
        PackError::VersionNotFound
    );
    fx.finish(db).await;
}
