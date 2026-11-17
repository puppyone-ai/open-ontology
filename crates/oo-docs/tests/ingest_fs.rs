//! Ingest against a real PostgreSQL and a real directory (B-S2-05): the fs
//! reconcile, hash jobs, rename, delete, partial listings, mass delete and
//! targeted sync.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test ingest_fs -- --ignored`

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use common::*;
use oo_connectors::objectstore::fs::FsSource;
use oo_connectors::objectstore::hash::DualDigest;
use oo_connectors::objectstore::{
    DocBackend, DocCapability, DocSourceError, DocumentSource, ListCursor, ListPage, ObjectMeta,
    ReadExpect,
};
use oo_docs::ingest::reconcile::{
    EnqueuedSync, SyncReport, SyncStatus, enqueue_sync, load_drive, run_sync, run_sync_source,
    targeted_sync,
};
use oo_docs::ingest::{
    FsFactory, HashableSource, IngestError, IngestLimits, SourceFactory, SyncFlavor, Trigger,
    handlers, hashjob,
};
use oo_docs::jobs::scheduler::{JobCtx, Outcome, Scheduler, SchedulerConfig};
use oo_docs::jobs::{FailureClass, JobKind, report_failure};
use oo_docs::store::jobs as store;
use sqlx::PgPool;

struct Env {
    name: String,
    pool: PgPool,
    drive: String,
    root: PathBuf,
    limits: IngestLimits,
}

async fn env(name: &str, max_files: i32) -> Env {
    {
        let _g = CLUSTER_LOCK.lock().await;
        installed_db(name).await;
    }
    let pool = pool_runtime(name, 12).await;
    let root = std::env::temp_dir().join(format!("oo-docs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let config = serde_json::json!({ "root": root.to_str().unwrap() }).to_string();
    let drive: String = sqlx::query_scalar(
        "INSERT INTO _oo_docs.drive (id, name, backend, config, max_files) \
         VALUES (gen_random_uuid(), 'it', 'fs', $1::jsonb, $2) RETURNING id::text",
    )
    .bind(config)
    .bind(max_files)
    .fetch_one(&pool)
    .await
    .unwrap();
    let limits = IngestLimits {
        settle: Duration::ZERO,
        page_size: 3,
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

    fn rm(&self, rel: &str) {
        std::fs::remove_file(self.root.join(rel)).unwrap();
    }

    async fn sync(&self) -> SyncReport {
        self.sync_with(SyncFlavor::POLL).await
    }

    async fn sync_with(&self, flavor: SyncFlavor) -> SyncReport {
        run_sync(
            &self.pool,
            &FsFactory,
            &self.drive,
            flavor,
            &self.limits,
            &AtomicBool::new(false),
        )
        .await
        .unwrap()
    }

    /// Run every due hash job to the end, as the scheduler would.
    async fn hash_all(&self) -> usize {
        let drive = load_drive(&self.pool, &self.drive).await.unwrap().unwrap();
        let source = FsFactory.open(&drive).unwrap();
        let mut n = 0;
        loop {
            let mut conn = self.pool.acquire().await.unwrap();
            let jobs = store::claim(&mut conn, &["hash"], "it/1/0", 8)
                .await
                .unwrap();
            drop(conn);
            if jobs.is_empty() {
                return n;
            }
            for job in jobs {
                n += 1;
                let ctx = JobCtx::detached(job.clone(), "it/1/0", self.pool.clone());
                let out = hashjob::run_hash(&ctx, &source, &self.limits).await;
                let mut conn = self.pool.acquire().await.unwrap();
                match out {
                    Outcome::Done => {
                        store::complete(&mut conn, &job.id, "it/1/0", job.generation)
                            .await
                            .unwrap();
                    }
                    Outcome::Completed => {}
                    Outcome::Failed { class, code } => {
                        report_failure(&mut conn, &job, "it/1/0", class, &code)
                            .await
                            .unwrap();
                    }
                    Outcome::Cancelled => panic!("cancelled"),
                }
            }
        }
    }

    async fn state(&self, path: &str) -> (String, Option<String>) {
        sqlx::query_as(
            "SELECT state, state_code FROM _oo_docs.file WHERE drive_id = $1::uuid AND path = $2 \
             ORDER BY (deleted_at IS NULL) DESC LIMIT 1",
        )
        .bind(&self.drive)
        .bind(path)
        .fetch_one(&self.pool)
        .await
        .unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    async fn scalar(&self, sql: &'static str) -> i64 {
        sqlx::query_scalar(sql)
            .bind(&self.drive)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn live(&self) -> i64 {
        self.scalar(
            "SELECT count(*) FROM _oo_docs.file WHERE drive_id = $1::uuid AND deleted_at IS NULL",
        )
        .await
    }

    async fn events(&self, kind: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM _oo_docs.event WHERE type = $1")
            .bind(kind)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn generation(&self) -> i64 {
        self.scalar("SELECT sync_generation FROM _oo_docs.drive WHERE id = $1::uuid")
            .await
    }

    async fn finish(self) {
        self.pool.close().await;
        let _ = std::fs::remove_dir_all(&self.root);
        drop_db(&self.name).await;
    }
}

fn b3_of(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn add_modify_delete() {
    let e = env("ingest_add_modify_delete", 100).await;
    e.write("a.md", "# alpha\n");
    e.write("sub/deep/b.txt", "bravo\n");
    e.write("c.md", "charlie\n");
    e.write(".hidden/x.md", "never listed\n");

    let r = e.sync().await;
    assert_eq!(
        (r.status, r.added, r.changed, r.removed),
        (SyncStatus::Complete, 3, 0, 0)
    );
    assert_eq!(r.generation, 1);
    assert_eq!(e.generation().await, 1);
    assert_eq!(e.state("a.md").await.0, "stored");
    assert_eq!(e.hash_all().await, 3);
    for p in ["a.md", "sub/deep/b.txt", "c.md"] {
        assert_eq!(e.state(p).await.0, "queued", "{p}");
    }
    let b3: String = sqlx::query_scalar(
        "SELECT cv.b3 FROM _oo_docs.file f JOIN _oo_docs.file_version cv ON cv.id = f.current_version_id \
         WHERE f.drive_id = $1::uuid AND f.path = 'a.md'",
    )
    .bind(&e.drive)
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(b3, b3_of("# alpha\n"));
    let (fmt, size): (String, i64) =
        sqlx::query_as("SELECT format, size FROM _oo_docs.blob WHERE b3 = $1")
            .bind(&b3)
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert_eq!((fmt.as_str(), size), ("md", 8));

    // Nothing changed: a quiet generation.
    let r = e.sync().await;
    assert_eq!(
        (r.status, r.added, r.changed, r.removed, r.hash_enqueued),
        (SyncStatus::Complete, 0, 0, 0, 0)
    );
    assert_eq!(r.generation, 2);

    // Modify: new version, old one superseded, file back to queued through
    // superseded -> stored (10 4.2).
    e.write("a.md", "# alpha, edited\n");
    let r = e.sync().await;
    assert_eq!((r.changed, r.hash_enqueued), (1, 1));
    assert_eq!(e.hash_all().await, 1);
    assert_eq!(e.state("a.md").await.0, "queued");
    let versions: Vec<(String, bool)> = sqlx::query_as(
        "SELECT cv.b3, cv.superseded_at IS NOT NULL FROM _oo_docs.file_version cv \
         JOIN _oo_docs.file f ON f.id = cv.file_id WHERE f.drive_id = $1::uuid AND f.path = 'a.md' \
         ORDER BY cv.created_at",
    )
    .bind(&e.drive)
    .fetch_all(&e.pool)
    .await
    .unwrap();
    assert_eq!(
        versions,
        vec![
            (b3_of("# alpha\n"), true),
            (b3_of("# alpha, edited\n"), false)
        ]
    );
    let hist: Vec<String> = sqlx::query_scalar(
        "SELECT h.to_state FROM _oo_docs.file_state_history h JOIN _oo_docs.file f ON f.id = h.file_id \
         WHERE f.drive_id = $1::uuid AND f.path = 'a.md' ORDER BY h.seq",
    )
    .bind(&e.drive)
    .fetch_all(&e.pool)
    .await
    .unwrap();
    assert_eq!(hist, ["stored", "queued", "superseded", "stored", "queued"]);

    // Same content, new mtime: only the fingerprint moves.
    e.write("c.md", "charlie\n");
    let r = e.sync().await;
    assert_eq!(r.status, SyncStatus::Complete);
    e.hash_all().await;
    let versions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.file_version cv JOIN _oo_docs.file f ON f.id = cv.file_id \
         WHERE f.drive_id = $1::uuid AND f.path = 'c.md'",
    )
    .bind(&e.drive)
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(versions, 1);

    // Delete.
    e.rm("sub/deep/b.txt");
    let r = e.sync().await;
    assert_eq!((r.status, r.removed, r.added), (SyncStatus::Complete, 1, 0));
    assert_eq!(e.state("sub/deep/b.txt").await.0, "deleted");
    assert_eq!(e.live().await, 2);
    let vdel: bool = sqlx::query_scalar(
        "SELECT cv.deleted_at IS NOT NULL FROM _oo_docs.file f \
         JOIN _oo_docs.file_version cv ON cv.id = f.current_version_id \
         WHERE f.drive_id = $1::uuid AND f.path = 'sub/deep/b.txt'",
    )
    .bind(&e.drive)
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert!(vdel);

    // The deleted path comes back: its row is revived.
    e.write("sub/deep/b.txt", "bravo again\n");
    let r = e.sync().await;
    assert_eq!(r.status, SyncStatus::Complete);
    e.hash_all().await;
    assert_eq!(e.state("sub/deep/b.txt").await.0, "queued");
    assert_eq!(e.live().await, 3);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn rename_is_detected_by_content() {
    let e = env("ingest_rename", 100).await;
    e.write("old/plan.md", "the plan, version one\n");
    e.write("keep.md", "stays put\n");
    e.sync().await;
    e.hash_all().await;
    // Pretend the plan was fully processed.
    sqlx::query("UPDATE _oo_docs.file SET state = 'published' WHERE path = 'old/plan.md'")
        .execute(&e.pool)
        .await
        .unwrap();
    let old_vid: String = sqlx::query_scalar(
        "SELECT current_version_id::text FROM _oo_docs.file WHERE path = 'old/plan.md'",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();

    std::fs::create_dir_all(e.root.join("new")).unwrap();
    std::fs::rename(e.root.join("old/plan.md"), e.root.join("new/plan-v1.md")).unwrap();
    let r = e.sync().await;
    assert_eq!(
        (r.status, r.renamed, r.removed, r.added, r.hash_enqueued),
        (SyncStatus::Complete, 1, 0, 0, 0),
        "{r:?}"
    );
    assert_eq!(e.state("old/plan.md").await.0, "deleted");
    // The renamed file carries the finished result of the old version.
    assert_eq!(e.state("new/plan-v1.md").await.0, "duplicate");
    let dup: Option<String> = sqlx::query_scalar(
        "SELECT cv.duplicate_of::text FROM _oo_docs.file f \
         JOIN _oo_docs.file_version cv ON cv.id = f.current_version_id WHERE f.path = 'new/plan-v1.md'",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(dup.as_deref(), Some(old_vid.as_str()));
    let (old_path, new_path): (String, String) = sqlx::query_as(
        "SELECT payload->>'old_path', payload->>'new_path' FROM _oo_docs.event WHERE type = 'file.renamed'",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(
        (old_path.as_str(), new_path.as_str()),
        ("old/plan.md", "new/plan-v1.md")
    );
    // The renamed file needs no hash job of its own.
    assert_eq!(e.hash_all().await, 0);
    assert_eq!(e.live().await, 2);

    // A rename of an unfinished file keeps it simply queued.
    std::fs::rename(e.root.join("keep.md"), e.root.join("kept.md")).unwrap();
    let r = e.sync().await;
    assert_eq!((r.renamed, r.removed, r.added), (1, 0, 0));
    assert_eq!(e.state("kept.md").await.0, "queued");
    e.finish().await;
}

/// A source whose listing misbehaves, to prove a partial listing never sweeps.
struct Flaky {
    inner: FsSource,
    /// Fail with `Unavailable` when asked for this page (0-based).
    fail_page: Option<usize>,
    /// Report the last page as `complete = false`.
    never_complete: bool,
    pages: std::sync::atomic::AtomicUsize,
}

impl DocumentSource for Flaky {
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
        let seen = if cursor.is_none() {
            self.pages.store(1, std::sync::atomic::Ordering::SeqCst);
            0
        } else {
            self.pages.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        };
        if self.fail_page == Some(seen) {
            return Err(DocSourceError::Unavailable);
        }
        let mut page = self.inner.list(cursor, page_size).await?;
        if self.never_complete && page.next.is_none() {
            page.complete = false;
        }
        Ok(page)
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

impl HashableSource for Flaky {
    fn hash_object(
        &self,
        raw_key: &[u8],
    ) -> impl std::future::Future<Output = Result<(DualDigest, ObjectMeta), DocSourceError>> + Send
    {
        self.inner.hash_object(raw_key)
    }
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn partial_listing_never_sweeps() {
    let e = env("ingest_partial", 100).await;
    for i in 0..8 {
        e.write(&format!("f{i}.md"), &format!("file number {i}\n"));
    }
    e.sync().await;
    e.hash_all().await;
    assert_eq!(e.live().await, 8);

    e.rm("f0.md");
    e.rm("f7.md");
    e.write("fresh.md", "appeared meanwhile\n");
    let drive = load_drive(&e.pool, &e.drive).await.unwrap().unwrap();
    let inner = FsSource::open(&e.root, Default::default()).unwrap();

    // The listing dies on its second page.
    let flaky = Flaky {
        inner: inner.clone(),
        fail_page: Some(1),
        never_complete: false,
        pages: Default::default(),
    };
    let r = run_sync_source(
        &e.pool,
        &drive,
        &flaky,
        SyncFlavor::POLL,
        &e.limits,
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert_eq!((r.status, r.removed), (SyncStatus::Failed, 0), "{r:?}");
    assert_eq!(r.error_code.as_deref(), Some("DOCS_SOURCE_UNAVAILABLE"));
    assert_eq!(
        e.live().await,
        8 + (r.added as i64),
        "no file may disappear"
    );
    assert_eq!(
        e.generation().await,
        1,
        "a failed generation is not committed"
    );
    assert_eq!(e.events("drive.sync_failed").await, 1);

    // The last page claims `complete = false`.
    let flaky = Flaky {
        inner: inner.clone(),
        fail_page: None,
        never_complete: true,
        pages: Default::default(),
    };
    let live_before = e.live().await;
    let r = run_sync_source(
        &e.pool,
        &drive,
        &flaky,
        SyncFlavor::POLL,
        &e.limits,
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert_eq!((r.status, r.removed), (SyncStatus::Failed, 0));
    assert_eq!(r.error_code.as_deref(), Some("DOCS_SOURCE_LIST_INCOMPLETE"));
    assert_eq!(
        e.live().await,
        live_before + r.added as i64,
        "only additions, no deletions"
    );
    assert_eq!(e.state("f0.md").await.0, "queued");
    assert_eq!(e.generation().await, 1);

    // A complete listing sweeps exactly what is gone.
    let r = e.sync().await;
    assert_eq!((r.status, r.removed), (SyncStatus::Complete, 2));
    assert_eq!(e.state("f0.md").await.0, "deleted");
    assert_eq!(e.state("f7.md").await.0, "deleted");
    assert_eq!(e.state("f3.md").await.0, "queued");
    assert_eq!(e.generation().await, 2);
    e.finish().await;
}

/// FM3-001 (formal/traces/regressions/FM3-001.itf.json): a generation that is
/// run again must not inherit the marks of the failed attempt, or a file that
/// attempt listed and that was deleted afterwards is never swept by the retry.
#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_retried_generation_forgets_the_marks_of_the_failed_attempt() {
    let e = env("ingest_retry_marks", 100).await;
    for i in 0..4 {
        e.write(&format!("f{i}.md"), &format!("file number {i}\n"));
    }
    e.sync().await;
    e.hash_all().await;
    assert_eq!((e.live().await, e.generation().await), (4, 1));

    // Generation 2 fails after listing everything: every row is marked 2.
    let drive = load_drive(&e.pool, &e.drive).await.unwrap().unwrap();
    let flaky = Flaky {
        inner: FsSource::open(&e.root, Default::default()).unwrap(),
        fail_page: None,
        never_complete: true,
        pages: Default::default(),
    };
    let r = run_sync_source(
        &e.pool,
        &drive,
        &flaky,
        SyncFlavor::POLL,
        &e.limits,
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert_eq!(
        (r.status, r.generation, r.removed),
        (SyncStatus::Failed, 2, 0)
    );

    // The file goes away, then generation 2 runs again.
    e.rm("f2.md");
    let r = e.sync().await;
    assert_eq!((r.status, r.generation), (SyncStatus::Complete, 2));
    assert_eq!(
        r.removed, 1,
        "the retry must sweep what the failed attempt had marked"
    );
    assert_eq!(e.state("f2.md").await.0, "deleted");
    assert_eq!(e.live().await, 3);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn missing_root_fails_without_sweeping() {
    let e = env("ingest_root_missing", 100).await;
    e.write("a.md", "a\n");
    e.sync().await;
    e.hash_all().await;
    let moved = e.root.with_extension("moved");
    std::fs::rename(&e.root, &moved).unwrap();
    let r = e.sync().await;
    assert_eq!(r.status, SyncStatus::Failed);
    assert_eq!(r.error_code.as_deref(), Some("DOCS_SOURCE_ROOT_MISSING"));
    assert_eq!(e.live().await, 1);
    std::fs::rename(&moved, &e.root).unwrap();
    let r = e.sync().await;
    assert_eq!((r.status, r.removed), (SyncStatus::Complete, 0));
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn mass_delete_is_held_until_confirmed() {
    let e = env("ingest_mass_delete", 1000).await;
    for i in 0..150 {
        e.write(&format!("bulk/f{i:03}.md"), &format!("content {i}\n"));
    }
    e.sync().await;
    e.hash_all().await;
    assert_eq!(e.live().await, 150);
    for i in 0..120 {
        e.rm(&format!("bulk/f{i:03}.md"));
    }
    let r = e.sync().await;
    assert_eq!(r.status, SyncStatus::Held, "{r:?}");
    assert_eq!(r.would_delete, Some(120));
    assert_eq!(
        r.error_code.as_deref(),
        Some("DOCS_SOURCE_MASS_DELETE_HELD")
    );
    assert_eq!(e.live().await, 150, "nothing is deleted while held");
    assert_eq!(e.generation().await, 1);
    assert_eq!(e.events("drive.sync_held").await, 1);

    // Enqueueing another sync is refused while held, naming the generation.
    match enqueue_sync(&e.pool, &e.drive, SyncFlavor::POLL)
        .await
        .unwrap()
    {
        EnqueuedSync::Held {
            generation,
            would_delete,
        } => assert_eq!((generation, would_delete), (2, 120)),
        other => panic!("{other:?}"),
    }
    // A confirmation for another generation is refused.
    let wrong = SyncFlavor {
        trigger: Trigger::Manual,
        deep: false,
        confirm: Some(9),
    };
    assert!(matches!(
        enqueue_sync(&e.pool, &e.drive, wrong).await,
        Err(IngestError::ConfirmMismatch { generation: 2 })
    ));
    // The right one sweeps.
    let ok = SyncFlavor {
        trigger: Trigger::Manual,
        deep: false,
        confirm: Some(2),
    };
    assert!(matches!(
        enqueue_sync(&e.pool, &e.drive, ok).await.unwrap(),
        EnqueuedSync::Enqueued { generation: 2, .. }
    ));
    let r = e.sync_with(ok).await;
    assert_eq!((r.status, r.removed), (SyncStatus::Complete, 120), "{r:?}");
    assert_eq!(e.live().await, 30);
    assert_eq!(e.generation().await, 2);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn emptied_root_counts_as_mass_delete() {
    let e = env("ingest_empty_root", 100).await;
    e.write("a.md", "a\n");
    e.write("b.md", "b\n");
    e.sync().await;
    e.hash_all().await;
    e.rm("a.md");
    e.rm("b.md");
    let r = e.sync().await;
    assert_eq!((r.status, r.would_delete), (SyncStatus::Held, Some(2)));
    assert_eq!(e.live().await, 2);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn targeted_sync_never_sweeps_or_bumps_generation() {
    let e = env("ingest_targeted", 100).await;
    e.write("a.md", "alpha\n");
    e.write("b.md", "bravo\n");
    e.write("c.md", "charlie\n");
    e.sync().await;
    e.hash_all().await;
    assert_eq!(e.generation().await, 1);
    let syncs_before = e
        .scalar("SELECT count(*) FROM _oo_docs.drive_sync WHERE drive_id = $1::uuid")
        .await;

    e.write("new/t.md", "targeted new\n");
    e.write("a.md", "alpha, but longer now\n");
    e.rm("b.md");
    let drive = load_drive(&e.pool, &e.drive).await.unwrap().unwrap();
    let source = FsFactory.open(&drive).unwrap();
    let paths: Vec<String> = [
        "new/t.md",
        "a.md",
        "b.md",
        "c.md",
        "../etc/passwd",
        ".git/config",
        "ghost.md",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let r = targeted_sync(&e.pool, &drive, &source, &paths, &e.limits)
        .await
        .unwrap();
    assert_eq!((r.added, r.changed, r.unchanged), (1, 1, 1), "{r:?}");
    assert_eq!(r.maybe_deleted, ["b.md", "ghost.md"]);
    assert_eq!(r.skipped, ["../etc/passwd", ".git/config"]);
    assert_eq!(r.hash_enqueued, 2);

    assert_eq!(
        e.generation().await,
        1,
        "targeted sync does not move the generation"
    );
    assert_eq!(
        e.scalar("SELECT count(*) FROM _oo_docs.drive_sync WHERE drive_id = $1::uuid")
            .await,
        syncs_before
    );
    assert_eq!(
        e.state("b.md").await.0,
        "queued",
        "a missing path is only a hint"
    );
    assert_eq!(e.live().await, 4);
    // Targeted work runs at the manual priority.
    let prio: i16 = sqlx::query_scalar(
        "SELECT priority FROM _oo_docs.job WHERE kind = 'hash' AND state = 'queued' ORDER BY id LIMIT 1",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(prio, 50);
    assert_eq!(e.hash_all().await, 2);
    assert_eq!(e.state("new/t.md").await.0, "queued");

    // The next full reconcile confirms the deletion and sees nothing new.
    let r = e.sync().await;
    assert_eq!(
        (r.status, r.removed, r.added, r.generation),
        (SyncStatus::Complete, 1, 0, 2)
    );
    assert_eq!(e.state("b.md").await.0, "deleted");
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn duplicate_content_is_detected() {
    let e = env("ingest_duplicate", 100).await;
    e.write("one.md", "identical bytes\n");
    e.sync().await;
    e.hash_all().await;
    // A twin while the first is still in the queue is not a duplicate yet.
    e.write("two.md", "identical bytes\n");
    e.sync().await;
    e.hash_all().await;
    assert_eq!(e.state("two.md").await.0, "queued");
    // Once the first one has a finished result, a third copy is.
    sqlx::query("UPDATE _oo_docs.file SET state = 'published' WHERE path = 'one.md'")
        .execute(&e.pool)
        .await
        .unwrap();
    e.write("three.md", "identical bytes\n");
    e.sync().await;
    e.hash_all().await;
    assert_eq!(e.state("three.md").await.0, "duplicate");
    let dup: String = sqlx::query_scalar(
        "SELECT cv.duplicate_of::text FROM _oo_docs.file f \
         JOIN _oo_docs.file_version cv ON cv.id = f.current_version_id WHERE f.path = 'three.md'",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    let first: String = sqlx::query_scalar(
        "SELECT current_version_id::text FROM _oo_docs.file WHERE path = 'one.md'",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(dup, first);
    // One blob row for all three.
    let blobs: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo_docs.blob")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(blobs, 1);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn size_limits_symlinks_and_invalid_names() {
    let mut e = env("ingest_special", 100).await;
    e.limits.max_text_file_bytes = 64;
    e.write("ok.md", "fine\n");
    e.write("huge.md", &"x".repeat(200));
    #[cfg(unix)]
    std::os::unix::fs::symlink("ok.md", e.root.join("link.md")).unwrap();
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::OsStr::from_bytes(&[b'b', b'a', b'd', 0xFF, b'.', b'm', b'd']);
        std::fs::write(e.root.join(name), "latin-1 name\n").unwrap();
    }
    let r = e.sync().await;
    assert_eq!(r.status, SyncStatus::Complete);
    // Over the md limit: too_large, never hashed (no hash job for it).
    assert_eq!(
        e.state("huge.md").await,
        (
            "too_large".to_owned(),
            Some("DOCS_SOURCE_TOO_LARGE".to_owned())
        )
    );
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo_docs.job WHERE kind = 'hash'")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(
        jobs,
        1 + cfg!(target_os = "linux") as i64,
        "ok.md (and the odd name) only"
    );
    #[cfg(unix)]
    assert_eq!(
        e.state("link.md").await,
        (
            "unsupported".to_owned(),
            Some("DOCS_SOURCE_SYMLINK_SKIPPED".to_owned())
        )
    );
    #[cfg(target_os = "linux")]
    {
        let invalid: i64 = e
            .scalar("SELECT count(*) FROM _oo_docs.file WHERE drive_id = $1::uuid AND state_code = 'DOCS_SOURCE_PATH_INVALID'")
            .await;
        assert_eq!(invalid, 1);
    }
    e.hash_all().await;
    assert_eq!(e.state("ok.md").await.0, "queued");

    // The same oversize file, still too large: a quiet generation.
    let r = e.sync().await;
    assert_eq!((r.changed, r.hash_enqueued), (0, 0), "{r:?}");
    // It shrinks below the limit and is processed.
    e.write("huge.md", "small now\n");
    let r = e.sync().await;
    assert_eq!((r.changed, r.hash_enqueued), (1, 1));
    e.hash_all().await;
    assert_eq!(e.state("huge.md").await.0, "queued");
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn drive_file_limit_keeps_the_first_paths() {
    let e = env("ingest_file_limit", 3).await;
    for name in ["e.md", "a.md", "d.md", "b.md", "c.md"] {
        e.write(name, name);
    }
    let r = e.sync().await;
    assert_eq!((r.status, r.added), (SyncStatus::Complete, 5));
    for ok in ["a.md", "b.md", "c.md"] {
        assert_eq!(e.state(ok).await.0, "stored", "{ok}");
    }
    for over in ["d.md", "e.md"] {
        assert_eq!(
            e.state(over).await,
            (
                "unsupported".to_owned(),
                Some("DOCS_SOURCE_DRIVE_FILE_LIMIT".to_owned())
            ),
            "{over}"
        );
    }
    assert_eq!(e.hash_all().await, 3);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn sniff_verdicts_reach_the_state_machine() {
    let e = env("ingest_sniff_states", 100).await;
    std::fs::write(e.root.join("pic.png"), [0x89, b'P', b'N', b'G', 1, 2, 3]).unwrap();
    std::fs::write(e.root.join("doc.pdf"), b"%PDF-1.7\n%%EOF\n").unwrap();
    std::fs::write(
        e.root.join("legacy.doc"),
        [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0, 0, 0],
    )
    .unwrap();
    let mut enc = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    enc.extend("EncryptionInfo".encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(e.root.join("secret.docx"), enc).unwrap();
    std::fs::write(e.root.join("latin.txt"), [0xC4, 0xE3, 0xBA, 0xC3]).unwrap();
    e.write("ok.md", "plain\n");
    e.sync().await;
    e.hash_all().await;
    let want = [
        ("pic.png", "unsupported", "DOCS_PARSE_IMAGE_OCR_M2"),
        ("doc.pdf", "unsupported", "DOCS_PARSE_UNSUPPORTED_FORMAT"),
        ("legacy.doc", "unsupported", "DOCS_PARSE_LEGACY_OFFICE"),
        ("secret.docx", "encrypted", "DOCS_PARSE_ENCRYPTED"),
        ("latin.txt", "unsupported", "DOCS_PARSE_ENCODING"),
    ];
    for (path, state, code) in want {
        assert_eq!(
            e.state(path).await,
            (state.to_owned(), Some(code.to_owned())),
            "{path}"
        );
    }
    assert_eq!(e.state("ok.md").await.0, "queued");
    // Every verdict went through `queued`, as the table says.
    let hist: Vec<String> = sqlx::query_scalar(
        "SELECT h.to_state FROM _oo_docs.file_state_history h JOIN _oo_docs.file f ON f.id = h.file_id \
         WHERE f.path = 'secret.docx' ORDER BY h.seq",
    )
    .fetch_all(&e.pool)
    .await
    .unwrap();
    assert_eq!(hist, ["stored", "queued", "encrypted"]);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn unstable_files_retry_without_counting_attempts() {
    let e = env("ingest_unstable", 100).await;
    e.write("busy.md", "being written\n");
    e.sync().await;
    let mut conn = e.pool.acquire().await.unwrap();
    let mut jobs = store::claim(&mut conn, &["hash"], "it/1/0", 1)
        .await
        .unwrap();
    drop(conn);
    let job = jobs.remove(0);
    assert_eq!(job.attempt, 1);

    struct Unstable(FsSource);
    impl DocumentSource for Unstable {
        fn backend(&self) -> DocBackend {
            self.0.backend()
        }
        fn capabilities(&self) -> std::collections::BTreeSet<DocCapability> {
            self.0.capabilities()
        }
        async fn list(&self, c: Option<&ListCursor>, n: u32) -> Result<ListPage, DocSourceError> {
            self.0.list(c, n).await
        }
        async fn stat(&self, k: &[u8]) -> Result<Option<ObjectMeta>, DocSourceError> {
            self.0.stat(k).await
        }
        async fn read(
            &self,
            k: &[u8],
            o: u64,
            l: u64,
            x: &ReadExpect,
        ) -> Result<Vec<u8>, DocSourceError> {
            self.0.read(k, o, l, x).await
        }
    }
    impl HashableSource for Unstable {
        async fn hash_object(&self, _k: &[u8]) -> Result<(DualDigest, ObjectMeta), DocSourceError> {
            Err(DocSourceError::Changed)
        }
    }
    let drive = load_drive(&e.pool, &e.drive).await.unwrap().unwrap();
    let src = Unstable(FsFactory.open(&drive).unwrap());
    let ctx = JobCtx::detached(job.clone(), "it/1/0", e.pool.clone());
    let out = hashjob::run_hash(&ctx, &src, &e.limits).await;
    assert_eq!(
        out,
        Outcome::failed(FailureClass::Busy, "DOCS_SOURCE_UNSTABLE")
    );
    let mut conn = e.pool.acquire().await.unwrap();
    report_failure(
        &mut conn,
        &job,
        "it/1/0",
        FailureClass::Busy,
        "DOCS_SOURCE_UNSTABLE",
    )
    .await
    .unwrap();
    let after = store::get(&mut conn, &job.id).await.unwrap().unwrap();
    assert_eq!(
        (after.state.as_str(), after.attempt),
        ("queued", 0),
        "Busy gives the attempt back"
    );
    assert_eq!(after.checkpoint.as_deref(), Some("{\"unstable\": 1}"));
    drop(conn);

    // The tenth consecutive unstable round counts as one failure.
    let mut late = job.clone();
    late.checkpoint = Some("{\"unstable\":9}".into());
    let ctx = JobCtx::detached(late, "it/1/0", e.pool.clone());
    let out = hashjob::run_hash(&ctx, &src, &e.limits).await;
    assert_eq!(
        out,
        Outcome::failed(FailureClass::Transient, "DOCS_SOURCE_UNSTABLE")
    );
    assert_eq!(e.state("busy.md").await.0, "stored");
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn scheduler_runs_sync_and_hash_end_to_end() {
    let e = env("ingest_scheduler", 100).await;
    for i in 0..10 {
        e.write(&format!("d{}/f{i}.md", i % 3), &format!("scheduled {i}\n"));
    }
    let limits = std::sync::Arc::new(e.limits.clone());
    let map = handlers::handlers(&e.pool, std::sync::Arc::new(FsFactory), limits);
    let mut cfg = SchedulerConfig::new("it-host/1/00000000000000aa");
    cfg.poll_interval = Duration::from_millis(50);
    cfg.concurrency.insert(JobKind::Hash, 3);
    cfg.concurrency.insert(JobKind::Sync, 1);
    let sched = Scheduler::new(e.pool.clone(), cfg, map).spawn();

    let first = enqueue_sync(&e.pool, &e.drive, SyncFlavor::POLL)
        .await
        .unwrap();
    assert!(matches!(
        first,
        EnqueuedSync::Enqueued { generation: 1, .. }
    ));
    // A second request while the first is live is a no-op.
    assert!(matches!(
        enqueue_sync(&e.pool, &e.drive, SyncFlavor::POLL)
            .await
            .unwrap(),
        EnqueuedSync::Running { generation: 1 }
    ));
    sched.wake();
    let mut done = false;
    for _ in 0..200 {
        let queued: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM _oo_docs.file WHERE drive_id = $1::uuid AND state = 'queued'",
        )
        .bind(&e.drive)
        .fetch_one(&e.pool)
        .await
        .unwrap();
        if queued == 10 {
            done = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(done, "all ten files should reach queued");
    assert_eq!(e.generation().await, 1);
    let unfinished: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo_docs.job WHERE state NOT IN ('succeeded')")
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert_eq!(unfinished, 0, "every job succeeded");
    sched.shutdown().await;
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn deleting_a_file_withdraws_its_evidence() {
    let e = env("ingest_evidence", 100).await;
    e.write("src.md", "the evidence lives here\n");
    e.write("other.md", "and here\n");
    e.sync().await;
    e.hash_all().await;
    let vid: String = sqlx::query_scalar(
        "SELECT current_version_id::text FROM _oo_docs.file WHERE path = 'src.md'",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    let b3 = b3_of("the evidence lives here\n");
    // Fixtures as the extraction stage would write them.
    let key = "a".repeat(64);
    sqlx::query(
        "INSERT INTO _oo_docs.entity (id, key, type, canonical_name, norm_name) VALUES \
         ('00000000-0000-0000-0000-00000000e001', 'project:aaaaaaaaaaaaaaaaaaaaaaaa', 'Project', 'P', 'p')",
    )
    .execute(&e.pool)
    .await
    .unwrap();
    for (fid, origin, status) in [
        ("00000000-0000-0000-0000-00000000f001", "rule", "published"),
        ("00000000-0000-0000-0000-00000000f002", "human", "published"),
    ] {
        sqlx::query(
            "INSERT INTO _oo_docs.fact (id, fact_key, subject_id, original_subject_id, predicate, value, \
             confidence_bp, origin, status) VALUES ($1::uuid, $2, '00000000-0000-0000-0000-00000000e001', \
             '00000000-0000-0000-0000-00000000e001', 'attr.note', '{\"text\":\"x\"}', 9000, $3, $4)",
        )
        .bind(fid)
        .bind(&key)
        .bind(origin)
        .bind(status)
        .execute(&e.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO _oo_docs.evidence (id, target_kind, target_id, version_id, b3, block_id, anchor, quote_b3, origin) \
             VALUES (gen_random_uuid(), 'fact', $1::uuid, $2::uuid, $3, 'blk_00000000000000000000', '{}', $3, $4)",
        )
        .bind(fid)
        .bind(&vid)
        .bind(&b3)
        .bind(origin)
        .execute(&e.pool)
        .await
        .unwrap();
    }
    e.rm("src.md");
    let r = e.sync().await;
    assert_eq!((r.status, r.removed), (SyncStatus::Complete, 1));
    let withdrawn: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.evidence WHERE withdrawn_at IS NOT NULL AND withdrawn_reason = 'file_deleted'",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(withdrawn, 2);
    let rule_fact: String = sqlx::query_scalar(
        "SELECT status FROM _oo_docs.fact WHERE id = '00000000-0000-0000-0000-00000000f001'",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    let human_fact: String = sqlx::query_scalar(
        "SELECT status FROM _oo_docs.fact WHERE id = '00000000-0000-0000-0000-00000000f002'",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(rule_fact, "retracted");
    assert_eq!(
        human_fact, "published",
        "human overlay facts are not retracted by a delete"
    );
    e.finish().await;
}

#[allow(dead_code)]
fn _paths(_: &Path) {}
