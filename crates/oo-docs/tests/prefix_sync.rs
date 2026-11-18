//! Parallel listing per first level prefix (10 11.3, B-S11-03) against a real PostgreSQL and a real
//! directory: each prefix lists on its own, has its own row and generation counter, and a prefix that fails
//! to list neither blocks nor loses the others.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test prefix_sync -- --ignored`

mod common;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use common::*;
use oo_connectors::objectstore::fs::FsSource;
use oo_connectors::objectstore::hash::DualDigest;
use oo_connectors::objectstore::{
    DocBackend, DocCapability, DocSourceError, DocumentSource, ListCursor, ListPage, ObjectMeta,
    ReadExpect,
};
use oo_docs::ingest::reconcile::{SyncReport, SyncStatus, load_drive, run_sync, run_sync_source};
use oo_docs::ingest::{FsFactory, HashableSource, IngestLimits, SyncFlavor};
use sqlx::PgPool;

struct Env {
    name: String,
    pool: PgPool,
    drive: String,
    root: PathBuf,
    limits: IngestLimits,
}

async fn env(name: &str, parallel: u64) -> Env {
    {
        let _g = CLUSTER_LOCK.lock().await;
        installed_db(name).await;
    }
    let pool = pool_runtime(name, 12).await;
    let root = std::env::temp_dir().join(format!("oo-docs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let config = serde_json::json!({
        "root": root.to_str().unwrap(),
        "listing": { "parallel": parallel },
    })
    .to_string();
    let drive: String = sqlx::query_scalar(
        "INSERT INTO _oo_docs.drive (id, name, backend, config, max_files) \
         VALUES (gen_random_uuid(), 'it', 'fs', $1::jsonb, 1000) RETURNING id::text",
    )
    .bind(config)
    .fetch_one(&pool)
    .await
    .unwrap();
    let limits = IngestLimits {
        settle: Duration::ZERO,
        page_size: 2,
        ..IngestLimits::default()
    };
    Env {
        name: name.to_owned(),
        pool,
        drive,
        root,
        limits,
    }
}

impl Env {
    fn write(&self, rel: &str, content: &str) {
        let p = self.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    async fn sync(&self) -> SyncReport {
        run_sync(
            &self.pool,
            &FsFactory,
            &self.drive,
            SyncFlavor::POLL,
            &self.limits,
            &AtomicBool::new(false),
        )
        .await
        .unwrap()
    }

    async fn sync_with<S: HashableSource>(&self, source: &S) -> SyncReport {
        let drive = load_drive(&self.pool, &self.drive).await.unwrap().unwrap();
        run_sync_source(
            &self.pool,
            &drive,
            source,
            SyncFlavor::POLL,
            &self.limits,
            &AtomicBool::new(false),
        )
        .await
        .unwrap()
    }

    async fn live(&self) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM _oo_docs.file WHERE drive_id = $1::uuid AND deleted_at IS NULL",
        )
        .bind(&self.drive)
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }

    async fn is_live(&self, path: &str) -> bool {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM _oo_docs.file WHERE drive_id = $1::uuid AND path = $2 \
             AND deleted_at IS NULL)",
        )
        .bind(&self.drive)
        .bind(path)
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }

    async fn generation(&self) -> i64 {
        sqlx::query_scalar("SELECT sync_generation FROM _oo_docs.drive WHERE id = $1::uuid")
            .bind(&self.drive)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    /// `(prefix, prefix_generation, status, listed, error_code)` of one run, by prefix.
    async fn scopes(&self, generation: i64) -> Vec<(String, i64, String, i64, Option<String>)> {
        sqlx::query_as(
            "SELECT prefix, prefix_generation, status, listed, error_code \
             FROM _oo_docs.drive_sync_prefix WHERE drive_id = $1::uuid AND generation = $2 \
             ORDER BY prefix",
        )
        .bind(&self.drive)
        .bind(generation)
        .fetch_all(&self.pool)
        .await
        .unwrap()
    }

    async fn finish(self) {
        self.pool.close().await;
        let _ = std::fs::remove_dir_all(&self.root);
        drop_db(&self.name).await;
    }
}

/// An fs source whose scoped listing of `bad` fails.
struct ScopeFlaky {
    inner: FsSource,
    bad: &'static str,
}

impl DocumentSource for ScopeFlaky {
    fn backend(&self) -> DocBackend {
        self.inner.backend()
    }
    fn capabilities(&self) -> std::collections::BTreeSet<DocCapability> {
        self.inner.capabilities()
    }
    async fn list(
        &self,
        cursor: Option<&ListCursor>,
        page_size: u32,
    ) -> Result<ListPage, DocSourceError> {
        self.inner.list(cursor, page_size).await
    }
    async fn stat(&self, raw_key: &[u8]) -> Result<Option<ObjectMeta>, DocSourceError> {
        self.inner.stat(raw_key).await
    }
    async fn read(
        &self,
        raw_key: &[u8],
        offset: u64,
        len: u64,
        expect: &ReadExpect,
    ) -> Result<Vec<u8>, DocSourceError> {
        self.inner.read(raw_key, offset, len, expect).await
    }
}

impl HashableSource for ScopeFlaky {
    fn hash_object(
        &self,
        raw_key: &[u8],
    ) -> impl std::future::Future<Output = Result<(DualDigest, ObjectMeta), DocSourceError>> + Send
    {
        self.inner.hash_object(raw_key)
    }

    fn list_scopes(
        &self,
    ) -> impl std::future::Future<Output = Result<Option<Vec<String>>, DocSourceError>> + Send {
        self.inner.list_scopes()
    }

    async fn list_scope(
        &self,
        scope: &str,
        cursor: Option<&ListCursor>,
        page_size: u32,
    ) -> Result<ListPage, DocSourceError> {
        if scope == self.bad {
            return Err(DocSourceError::Unavailable);
        }
        self.inner.list_scope(scope, cursor, page_size).await
    }
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn scopes_list_in_parallel_and_count_their_own_generations() {
    let e = env("prefix_parallel", 4).await;
    e.write("root.md", "at the root\n");
    for i in 0..5 {
        e.write(&format!("a/f{i}.md"), &format!("a {i}\n"));
        e.write(&format!("b/deep/g{i}.md"), &format!("b {i}\n"));
    }
    e.write("c/one.md", "c\n");
    let r = e.sync().await;
    assert_eq!(
        (r.status, r.listed, r.removed),
        (SyncStatus::Complete, 12, 0),
        "{r:?}"
    );
    assert_eq!(e.live().await, 12);
    assert_eq!(e.generation().await, 1);
    let s = e.scopes(1).await;
    let names: Vec<&str> = s.iter().map(|x| x.0.as_str()).collect();
    assert_eq!(
        names,
        ["", "a", "b", "c"],
        "the root files form the scope \"\""
    );
    assert!(
        s.iter()
            .all(|x| x.1 == 1 && x.2 == "complete" && x.4.is_none()),
        "{s:?}"
    );
    assert_eq!(s.iter().map(|x| x.3).sum::<i64>(), 12);

    // Same Drive, same listing, one scope less: the second run counts 2 for the scopes that remain.
    std::fs::remove_dir_all(e.root.join("c")).unwrap();
    let r = e.sync().await;
    assert_eq!(
        (r.status, r.removed),
        (SyncStatus::Complete, 1),
        "a vanished scope lists as gone: {r:?}"
    );
    assert!(!e.is_live("c/one.md").await);
    let s = e.scopes(2).await;
    assert_eq!(
        s.iter().map(|x| (x.0.as_str(), x.1)).collect::<Vec<_>>(),
        [("", 2), ("a", 2), ("b", 2)]
    );
    assert_eq!(e.generation().await, 2);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_failed_scope_is_not_swept_and_does_not_hold_the_others() {
    let e = env("prefix_failed", 4).await;
    e.write("root.md", "r\n");
    e.write("a/x.md", "x\n");
    e.write("a/y.md", "y\n");
    e.write("b/p.md", "p\n");
    e.write("b/q.md", "q\n");
    assert_eq!(e.sync().await.status, SyncStatus::Complete);
    assert_eq!(e.live().await, 5);

    std::fs::remove_file(e.root.join("a/x.md")).unwrap();
    std::fs::remove_file(e.root.join("b/p.md")).unwrap();
    e.write("a/new.md", "new\n");
    let flaky = ScopeFlaky {
        inner: FsSource::open(&e.root, Default::default()).unwrap(),
        bad: "b",
    };
    let r = e.sync_with(&flaky).await;
    assert_eq!(r.status, SyncStatus::Failed, "{r:?}");
    assert_eq!(r.error_code.as_deref(), Some("DOCS_SOURCE_UNAVAILABLE"));
    assert_eq!(r.removed, 1, "the complete scope a is swept");
    assert!(!e.is_live("a/x.md").await);
    assert!(
        e.is_live("a/new.md").await,
        "what a scope lists is still taken"
    );
    assert!(
        e.is_live("b/p.md").await,
        "nothing under the failed scope may disappear"
    );
    assert_eq!(
        e.generation().await,
        1,
        "the Drive generation stays uncommitted"
    );
    let s = e.scopes(2).await;
    let b = s.iter().find(|x| x.0 == "b").unwrap();
    assert_eq!(
        (b.2.as_str(), b.4.as_deref()),
        ("failed", Some("DOCS_SOURCE_UNAVAILABLE"))
    );
    assert_eq!(
        b.1, 2,
        "the counter of a failed scope does not move past the last complete one"
    );
    let a = s.iter().find(|x| x.0 == "a").unwrap();
    assert_eq!((a.1, a.2.as_str()), (2, "complete"));

    // The retry of the same generation: b lists now and its deletion is swept.
    let r = e.sync().await;
    assert_eq!((r.status, r.removed), (SyncStatus::Complete, 1), "{r:?}");
    assert!(!e.is_live("b/p.md").await);
    assert_eq!(e.generation().await, 2);
    assert!(e.scopes(2).await.iter().all(|x| x.2 == "complete"));
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn one_scope_or_no_parallelism_lists_the_drive_whole() {
    let e = env("prefix_whole", 1).await;
    e.write("a/x.md", "x\n");
    e.write("b/y.md", "y\n");
    let r = e.sync().await;
    assert_eq!((r.status, r.listed), (SyncStatus::Complete, 2));
    assert!(
        e.scopes(1).await.is_empty(),
        "parallel 1 writes no scope rows"
    );
    e.finish().await;
}
