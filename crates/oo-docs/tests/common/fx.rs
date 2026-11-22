//! Fixture for the ontology tests (B-S5-04 and later): a throwaway database
//! with one Drive, files planted as `indexed` with a complete parse, and the
//! rule output indexed through the engine's own write path. The tests run as
//! a login member of `oo_docs_runtime`; the superuser connection only plants.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use oo_core::docir::DocFormat;
use oo_docs::cas::{Cas, CasKind};
use oo_docs::normalize::Normalized;
use oo_docs::publish::run::{PublishConfig, PublishOutcome, publish};
use oo_docs::publish::write::{DocumentInput, IndexSummary, index_document_with};
use sqlx::{Executor, PgConnection};

use super::{connect, connect_runtime, drop_db, installed_db};

pub const DRIVE: &str = "00000000-0000-4000-8000-0000000000d1";
static SEQ: AtomicU64 = AtomicU64::new(1);

pub fn uuid() -> String {
    format!(
        "00000000-0000-4000-8000-{:012x}",
        SEQ.fetch_add(1, Ordering::SeqCst)
    )
}

pub fn golden() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testing/super-ontology/golden")
}

pub struct Planted {
    pub file_id: String,
    pub version_id: String,
    pub b3: String,
    pub ir: oo_core::docir::DocIr,
}

pub struct Fx {
    pub cas: Cas,
    pub dir: PathBuf,
    pub admin: PgConnection,
    pub rt: PgConnection,
    pub siblings: Vec<String>,
}

impl Fx {
    pub async fn new(db: &str) -> Self {
        installed_db(db).await;
        let mut admin = connect(db).await;
        admin
            .execute(sqlx::AssertSqlSafe(format!(
                "INSERT INTO _oo_docs.drive (id, name, backend, config) VALUES ('{DRIVE}', 'd', 'fs', '{{}}')"
            )))
            .await
            .unwrap();
        let dir = std::env::temp_dir().join(format!("oo-docs-fx-{}-{db}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cas = Cas::open(&dir).expect("cas");
        let rt = connect_runtime(db).await;
        Self {
            cas,
            dir,
            admin,
            rt,
            siblings: Vec::new(),
        }
    }

    /// Attach to a database a `TestEngine` already installed: inserts the Drive
    /// and uses the engine's content store directory.
    pub async fn attach(db: &str, cas_dir: &std::path::Path) -> Self {
        let mut admin = connect(db).await;
        admin
            .execute(sqlx::AssertSqlSafe(format!(
                "INSERT INTO _oo_docs.drive (id, name, backend, config) VALUES ('{DRIVE}', 'd', 'fs', '{{}}')"
            )))
            .await
            .unwrap();
        let cas = Cas::open(cas_dir).expect("cas");
        let rt = connect_runtime(db).await;
        Self {
            cas,
            dir: cas_dir.to_path_buf(),
            admin,
            rt,
            siblings: Vec::new(),
        }
    }

    /// Plant blob, file (state `indexed`), version and a complete parse.
    pub async fn plant_normalized(&mut self, path: &str, n: Normalized, size: usize) -> Planted {
        let b3 = n.ir.blob_b3.clone();
        let format = match n.ir.format {
            DocFormat::Md => "md",
            DocFormat::Txt => "txt",
            DocFormat::Csv => "csv",
            DocFormat::Xlsx => "xlsx",
            DocFormat::Docx => "docx",
            DocFormat::Pptx => "pptx",
            DocFormat::Pdf => "pdf",
            DocFormat::Audio => "audio",
            DocFormat::Video => "video",
            DocFormat::Image => "image",
            DocFormat::PdfScanned => "pdf_scanned",
        };
        let put = self
            .cas
            .put(CasKind::Ir, n.canonical.as_bytes())
            .expect("cas put");
        assert_eq!(put.b3, n.ir_b3);
        let (file_id, version_id) = (uuid(), uuid());
        let a = &mut self.admin;
        sqlx::query(
            "INSERT INTO _oo_docs.blob (b3, sha256, size, format) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
        )
        .bind(&b3)
        .bind("0".repeat(64))
        .bind(i64::try_from(size).unwrap())
        .bind(format)
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
        sqlx::query("UPDATE _oo_docs.file SET current_version_id = $2::uuid WHERE id = $1::uuid")
            .bind(&file_id)
            .bind(&version_id)
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
        self.siblings.push(path.to_owned());
        Planted {
            file_id,
            version_id,
            b3,
            ir: n.ir,
        }
    }

    /// Plant a markdown document.
    pub async fn plant_md(&mut self, path: &str, src: &str) -> Planted {
        let n = oo_docs::parse::markdown::parse_markdown(src.as_bytes()).expect("parse");
        self.plant_normalized(path, n, src.len()).await
    }

    /// A second version of an already planted markdown file (new bytes, same
    /// file row); returns the plant to index.
    pub async fn plant_version(&mut self, path: &str, src: &str) -> Planted {
        let n = oo_docs::parse::markdown::parse_markdown(src.as_bytes()).expect("parse");
        let b3 = n.ir.blob_b3.clone();
        let put = self
            .cas
            .put(CasKind::Ir, n.canonical.as_bytes())
            .expect("cas put");
        assert_eq!(put.b3, n.ir_b3);
        let file_id: String =
            sqlx::query_scalar("SELECT id::text FROM _oo_docs.file WHERE path = $1")
                .bind(path)
                .fetch_one(&mut self.admin)
                .await
                .unwrap();
        let version_id = uuid();
        let a = &mut self.admin;
        for sql in [
            "INSERT INTO _oo_docs.blob (b3, sha256, size, format) VALUES ($1, $2, $3, 'md') ON CONFLICT DO NOTHING",
        ] {
            sqlx::query(sql)
                .bind(&b3)
                .bind("0".repeat(64))
                .bind(i64::try_from(src.len()).unwrap())
                .execute(&mut *a)
                .await
                .unwrap();
        }
        sqlx::query("UPDATE _oo_docs.file_version SET superseded_at = now() WHERE file_id = $1::uuid AND superseded_at IS NULL")
            .bind(&file_id)
            .execute(&mut *a)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO _oo_docs.file_version (id, file_id, b3, path_at_version, fp) VALUES ($1::uuid, $2::uuid, $3, $4, '{}'::jsonb)",
        )
        .bind(&version_id)
        .bind(&file_id)
        .bind(&b3)
        .bind(path)
        .execute(&mut *a)
        .await
        .unwrap();
        sqlx::query("UPDATE _oo_docs.file SET current_version_id = $2::uuid WHERE id = $1::uuid")
            .bind(&file_id)
            .bind(&version_id)
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
        Planted {
            file_id,
            version_id,
            b3,
            ir: n.ir,
        }
    }

    /// Plant and index a markdown document.
    pub async fn index_md(&mut self, path: &str, src: &str) -> IndexSummary {
        let p = self.plant_md(path, src).await;
        self.index_planted(path, &p).await
    }

    /// Plant and index a csv document.
    pub async fn index_csv(&mut self, path: &str, src: &str) -> IndexSummary {
        let n = oo_docs::parse::csv::parse_csv(src.as_bytes()).expect("parse");
        let p = self.plant_normalized(path, n, src.len()).await;
        self.index_planted(path, &p).await
    }

    pub async fn index_planted(&mut self, path: &str, p: &Planted) -> IndexSummary {
        let doc = DocumentInput {
            version_id: &p.version_id,
            b3: &p.b3,
            path,
            ir: &p.ir,
        };
        let sibs = self.siblings.clone();
        index_document_with(&mut self.rt, &doc, &sibs)
            .await
            .expect("index")
    }

    pub async fn publish(&mut self) -> PublishOutcome {
        publish(&mut self.rt, &self.cas, &PublishConfig::default())
            .await
            .expect("publish")
    }

    /// Run one statement as the administrator.
    pub async fn admin_sql(&mut self, sql: &str) {
        self.admin
            .execute(sqlx::AssertSqlSafe(sql.to_owned()))
            .await
            .unwrap();
    }

    pub async fn scalar(&mut self, sql: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .fetch_one(&mut self.admin)
            .await
            .unwrap()
    }

    pub async fn text(&mut self, sql: &str) -> Vec<String> {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .fetch_all(&mut self.admin)
            .await
            .unwrap()
    }

    /// The plant of a path that was planted before (to index it again): the
    /// IR is read back from the CAS through the stored parse result.
    pub async fn known_plant(&mut self, path: &str) -> Planted {
        let (file_id, version_id, b3, ir_b3): (String, String, String, String) = sqlx::query_as(
            "SELECT f.id::text, fv.id::text, fv.b3, pr.ir_b3 FROM _oo_docs.file f \
               JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
               JOIN _oo_docs.parse_result pr ON pr.b3 = fv.b3 WHERE f.path = $1",
        )
        .bind(path)
        .fetch_one(&mut self.admin)
        .await
        .unwrap();
        let bytes = self.cas.get(CasKind::Ir, &ir_b3).expect("ir");
        Planted {
            file_id,
            version_id,
            b3,
            ir: serde_json::from_slice(&bytes).unwrap(),
        }
    }

    /// `n` LLM facts `Organization decides Decision` of the file at `path` (an organisation `<prefix>负责人`), each with
    /// its own decision and the evidence of the document's first block. Another organisation (`<prefix>对立`) decides
    /// each of the decisions by a rule: the facts contradict, which is what D91 asks about when their score is in
    /// the middle band. Returns the ids of the model's facts.
    pub async fn llm_facts(&mut self, path: &str, n: usize, bp: i32) -> Vec<String> {
        self.llm_facts_named(path, "议题", n, bp).await
    }

    /// [`Self::llm_facts`] with decisions named `<prefix>NNN`.
    pub async fn llm_facts_named(
        &mut self,
        path: &str,
        prefix: &str,
        n: usize,
        bp: i32,
    ) -> Vec<String> {
        use oo_docs::extract::model::EntityType;
        use oo_docs::resolve::normalize::{entity_key, fact_key, norm};
        let doc_id: String = sqlx::query_scalar(
            "SELECT id::text FROM _oo_docs.entity WHERE type = 'Document' AND canonical_name = $1",
        )
        .bind(path)
        .fetch_one(&mut self.admin)
        .await
        .unwrap();
        let ev: (String, String, String, String, String) = sqlx::query_as(
            "SELECT ev.version_id::text, ev.b3, ev.block_id, ev.anchor::text, ev.quote_b3 \
               FROM _oo_docs.evidence ev WHERE ev.target_kind = 'entity' AND ev.target_id = $1::uuid \
              ORDER BY ev.block_id LIMIT 1",
        )
        .bind(&doc_id)
        .fetch_one(&mut self.admin)
        .await
        .unwrap();
        let who = format!("{prefix}负责人");
        let skey = entity_key(EntityType::Organization, None, &norm(&who), None);
        sqlx::query(
            "INSERT INTO _oo_docs.entity (id, key, type, canonical_name, norm_name) \
             VALUES (gen_random_uuid(), $1, 'Organization', $2, $3) ON CONFLICT (key) DO NOTHING",
        )
        .bind(&skey)
        .bind(&who)
        .bind(norm(&who))
        .execute(&mut self.admin)
        .await
        .unwrap();
        let other = format!("{prefix}对立");
        let okey = entity_key(EntityType::Organization, None, &norm(&other), None);
        sqlx::query(
            "INSERT INTO _oo_docs.entity (id, key, type, canonical_name, norm_name) \
             VALUES (gen_random_uuid(), $1, 'Organization', $2, $3) ON CONFLICT (key) DO NOTHING",
        )
        .bind(&okey)
        .bind(&other)
        .bind(norm(&other))
        .execute(&mut self.admin)
        .await
        .unwrap();
        let mut ids = Vec::new();
        for i in 0..n {
            let name = format!("{prefix}{i:03}");
            let tkey = entity_key(EntityType::Decision, None, &norm(&name), None);
            sqlx::query(
                "INSERT INTO _oo_docs.entity (id, key, type, canonical_name, norm_name) \
                 VALUES (gen_random_uuid(), $1, 'Decision', $2, $3) ON CONFLICT (key) DO NOTHING",
            )
            .bind(&tkey)
            .bind(&name)
            .bind(norm(&name))
            .execute(&mut self.admin)
            .await
            .unwrap();
            let fkey = fact_key(&skey, "decides", Some(&tkey), None, None);
            let fid: String = sqlx::query_scalar(
                "INSERT INTO _oo_docs.fact (id, fact_key, subject_id, original_subject_id, predicate, \
                    object_id, original_object_id, confidence_bp, origin, status) \
                 SELECT gen_random_uuid(), $1, s.id, s.id, 'decides', t.id, t.id, $3, 'llm', 'draft' \
                   FROM _oo_docs.entity t, _oo_docs.entity s WHERE t.key = $4 AND s.key = $2 RETURNING id::text",
            )
            .bind(&fkey)
            .bind(&skey)
            .bind(bp)
            .bind(&tkey)
            .fetch_one(&mut self.admin)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO _oo_docs.evidence (id, target_kind, target_id, version_id, b3, block_id, anchor, \
                                                quote_b3, origin) \
                 VALUES (gen_random_uuid(), 'fact', $1::uuid, $2::uuid, $3, $4, $5::jsonb, $6, 'llm')",
            )
            .bind(&fid)
            .bind(&ev.0)
            .bind(&ev.1)
            .bind(&ev.2)
            .bind(&ev.3)
            .bind(&ev.4)
            .execute(&mut self.admin)
            .await
            .unwrap();
            // The rule's claim for the same decision by someone else, with the same evidence.
            let rkey = fact_key(&okey, "decides", Some(&tkey), None, None);
            let rid: String = sqlx::query_scalar(
                "INSERT INTO _oo_docs.fact (id, fact_key, subject_id, original_subject_id, predicate, \
                    object_id, original_object_id, confidence_bp, origin, rule_id, status) \
                 SELECT gen_random_uuid(), $1, s.id, s.id, 'decides', t.id, t.id, 9800, 'rule', 'fm.fields', 'draft' \
                   FROM _oo_docs.entity t, _oo_docs.entity s WHERE t.key = $3 AND s.key = $2 RETURNING id::text",
            )
            .bind(&rkey)
            .bind(&okey)
            .bind(&tkey)
            .fetch_one(&mut self.admin)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO _oo_docs.evidence (id, target_kind, target_id, version_id, b3, block_id, anchor, \
                                                quote_b3, origin) \
                 VALUES (gen_random_uuid(), 'fact', $1::uuid, $2::uuid, $3, $4, $5::jsonb, $6, 'rule')",
            )
            .bind(&rid)
            .bind(&ev.0)
            .bind(&ev.1)
            .bind(&ev.2)
            .bind(&ev.3)
            .bind(&ev.4)
            .execute(&mut self.admin)
            .await
            .unwrap();
            ids.push(fid);
        }
        ids
    }

    /// `n` LLM facts `Document about Topic` of the file at `path`, each with its
    /// own topic (a topical link: D91 holds it as a candidate, never asks) and the evidence of the document's first block. Returns the
    /// fact ids.
    pub async fn llm_topic_facts(&mut self, path: &str, n: usize, bp: i32) -> Vec<String> {
        self.llm_topic_facts_named(path, "议题", n, bp).await
    }

    /// [`Self::llm_topic_facts`] with topics named `<prefix>NNN`.
    pub async fn llm_topic_facts_named(
        &mut self,
        path: &str,
        prefix: &str,
        n: usize,
        bp: i32,
    ) -> Vec<String> {
        use oo_docs::extract::model::EntityType;
        use oo_docs::resolve::normalize::{entity_key, fact_key, norm};
        let (doc_id, doc_key): (String, String) = sqlx::query_as(
            "SELECT id::text, key FROM _oo_docs.entity WHERE type = 'Document' AND canonical_name = $1",
        )
        .bind(path)
        .fetch_one(&mut self.admin)
        .await
        .unwrap();
        let ev: (String, String, String, String, String) = sqlx::query_as(
            "SELECT ev.version_id::text, ev.b3, ev.block_id, ev.anchor::text, ev.quote_b3 \
               FROM _oo_docs.evidence ev WHERE ev.target_kind = 'entity' AND ev.target_id = $1::uuid \
              ORDER BY ev.block_id LIMIT 1",
        )
        .bind(&doc_id)
        .fetch_one(&mut self.admin)
        .await
        .unwrap();
        let mut ids = Vec::new();
        for i in 0..n {
            let name = format!("{prefix}{i:03}");
            let tkey = entity_key(EntityType::Topic, None, &norm(&name), None);
            sqlx::query(
                "INSERT INTO _oo_docs.entity (id, key, type, canonical_name, norm_name) \
                 VALUES (gen_random_uuid(), $1, 'Topic', $2, $3) ON CONFLICT (key) DO NOTHING",
            )
            .bind(&tkey)
            .bind(&name)
            .bind(norm(&name))
            .execute(&mut self.admin)
            .await
            .unwrap();
            let fkey = fact_key(&doc_key, "about", Some(&tkey), None, None);
            let fid: String = sqlx::query_scalar(
                "INSERT INTO _oo_docs.fact (id, fact_key, subject_id, original_subject_id, predicate, \
                    object_id, original_object_id, confidence_bp, origin, status) \
                 SELECT gen_random_uuid(), $1, $2::uuid, $2::uuid, 'about', t.id, t.id, $3, 'llm', 'draft' \
                   FROM _oo_docs.entity t WHERE t.key = $4 RETURNING id::text",
            )
            .bind(&fkey)
            .bind(&doc_id)
            .bind(bp)
            .bind(&tkey)
            .fetch_one(&mut self.admin)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO _oo_docs.evidence (id, target_kind, target_id, version_id, b3, block_id, anchor, \
                                                quote_b3, origin) \
                 VALUES (gen_random_uuid(), 'fact', $1::uuid, $2::uuid, $3, $4, $5::jsonb, $6, 'llm')",
            )
            .bind(&fid)
            .bind(&ev.0)
            .bind(&ev.1)
            .bind(&ev.2)
            .bind(&ev.3)
            .bind(&ev.4)
            .execute(&mut self.admin)
            .await
            .unwrap();
            ids.push(fid);
        }
        ids
    }

    pub async fn finish(self, db: &str) {
        let Fx { dir, admin, rt, .. } = self;
        drop(rt);
        drop(admin);
        let _ = std::fs::remove_dir_all(&dir);
        drop_db(db).await;
    }
}
