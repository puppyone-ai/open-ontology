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

    pub async fn finish(self, db: &str) {
        let Fx { dir, admin, rt, .. } = self;
        drop(rt);
        drop(admin);
        let _ = std::fs::remove_dir_all(&dir);
        drop_db(db).await;
    }
}
