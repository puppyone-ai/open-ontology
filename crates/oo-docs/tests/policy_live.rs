//! Drive access policy in both places against one throwaway PostgreSQL (10 §11.3, B-S11-03): ingest never lists a
//! denied path into the Drive (full sync and targeted sync), a file that was there before the policy is swept, and
//! the visible graph drops denied paths even before any sweep.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test policy_live -- --ignored --test-threads=1`

mod common;

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use common::fx::Fx;
use common::*;
use oo_docs::compile::pg::PgReader;
use oo_docs::compile::reader::OntologyReader;
use oo_docs::compile::visible::VisibleGraph;
use oo_docs::ingest::reconcile::{load_drive, run_sync, targeted_sync};
use oo_docs::ingest::{FsFactory, IngestLimits, SourceFactory, SyncFlavor};
use oo_docs::publish::run::PublishOutcome;
use serde_json::json;

fn limits() -> IngestLimits {
    IngestLimits {
        settle: Duration::ZERO,
        page_size: 3,
        ..IngestLimits::default()
    }
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn ingest_never_lists_a_denied_path_and_sweeps_one_that_became_denied() {
    let name = "oo_docs_policy_ingest";
    {
        let _g = CLUSTER_LOCK.lock().await;
        installed_db(name).await;
    }
    let pool = pool_runtime(name, 8).await;
    let root = std::env::temp_dir().join(format!("oo-docs-policy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (rel, text) in [
        ("Projects/a.md", "# a\n"),
        ("Projects/Payroll/salaries.md", "# secret\n"),
        ("Shared/b.md", "# b\n"),
    ] {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let drive: String = sqlx::query_scalar(
        "INSERT INTO _oo_docs.drive (id, name, backend, config) VALUES (gen_random_uuid(), 'p', 'fs', $1::jsonb) \
         RETURNING id::text",
    )
    .bind(json!({"root": root.to_str().unwrap(), "policy": {"allow": ["Projects"], "deny": ["Projects/Payroll"]}}).to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    let sync = |flavor| {
        let (pool, drive) = (pool.clone(), drive.clone());
        async move {
            run_sync(
                &pool,
                &FsFactory,
                &drive,
                flavor,
                &limits(),
                &AtomicBool::new(false),
            )
            .await
            .unwrap()
        }
    };
    let paths = |pool: &sqlx::PgPool, drive: &str| {
        let (pool, drive) = (pool.clone(), drive.to_owned());
        async move {
            sqlx::query_scalar::<_, String>(
                "SELECT path FROM _oo_docs.file WHERE drive_id = $1::uuid AND deleted_at IS NULL ORDER BY path",
            )
            .bind(drive)
            .fetch_all(&pool)
            .await
            .unwrap()
        }
    };
    let r = sync(SyncFlavor::POLL).await;
    assert_eq!(r.listed, 1, "only Projects/a.md is listed: {r:?}");
    assert_eq!(paths(&pool, &drive).await, ["Projects/a.md"]);

    // A targeted sync (an event, a refresh) cannot reach in either.
    let d = load_drive(&pool, &drive).await.unwrap().unwrap();
    let source = FsFactory.open(&d).unwrap();
    let t = targeted_sync(
        &pool,
        &d,
        &source,
        &[
            "Projects/Payroll/salaries.md".to_owned(),
            "Shared/b.md".to_owned(),
        ],
        &limits(),
    )
    .await
    .unwrap();
    assert_eq!(t.added, 0, "{t:?}");
    assert_eq!(paths(&pool, &drive).await, ["Projects/a.md"]);

    // Open the policy, sync, then close it again: the file that became denied is swept like a deleted one.
    sqlx::query("UPDATE _oo_docs.drive SET config = config - 'policy' WHERE id = $1::uuid")
        .bind(&drive)
        .execute(&pool)
        .await
        .unwrap();
    sync(SyncFlavor::POLL).await;
    assert_eq!(paths(&pool, &drive).await.len(), 3);
    sqlx::query("UPDATE _oo_docs.drive SET config = config || $2::jsonb WHERE id = $1::uuid")
        .bind(&drive)
        .bind(json!({"policy": {"deny": ["Projects/Payroll"]}}).to_string())
        .execute(&pool)
        .await
        .unwrap();
    let r = sync(SyncFlavor::POLL).await;
    assert_eq!(r.removed, 1, "{r:?}");
    assert_eq!(paths(&pool, &drive).await, ["Projects/a.md", "Shared/b.md"]);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn the_visible_graph_drops_a_denied_path_before_any_sweep() {
    let _g = CLUSTER_LOCK.lock().await;
    let mut fx = Fx::new("oo_docs_policy_graph").await;
    fx.index_md(
        "Projects/星火计划.md",
        "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n",
    )
    .await;
    fx.index_md(
        "Payroll/薪酬方案.md",
        "---\ntype: project\nname: 薪酬方案\nowner: 周远\n---\n# 薪酬方案\n",
    )
    .await;
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("publish")
    };
    let (names, files) = load(&mut fx).await;
    assert!(
        names.iter().any(|n| n == "薪酬方案") && files.iter().any(|f| f == "Payroll/薪酬方案.md")
    );
    fx.admin_sql(&format!(
        "UPDATE _oo_docs.drive SET config = '{{\"policy\": {{\"deny\": [\"Payroll\"]}}}}'::jsonb WHERE id = '{}'",
        oo_docs_drive()
    ))
    .await;
    let (names, files) = load(&mut fx).await;
    assert!(
        names.iter().any(|n| n == "星火计划"),
        "the allowed side stays: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n == "薪酬方案" || n == "周远"),
        "nothing of the denied file leaks: {names:?}"
    );
    assert_eq!(files, ["Projects/星火计划.md"]);
}

async fn load(fx: &mut Fx) -> (Vec<String>, Vec<String>) {
    let reader = PgReader::load(&mut fx.rt, &fx.cas, oo_docs_drive(), None, &[])
        .await
        .unwrap();
    let v = reader.latest_version().unwrap();
    let (g, _) = VisibleGraph::load(&reader, &v, &[]).unwrap();
    let names: Vec<String> = g.entities().map(|e| e.name.clone()).collect();
    (names, g.snapshot().iter().map(|f| f.path.clone()).collect())
}

fn oo_docs_drive() -> &'static str {
    common::fx::DRIVE
}
