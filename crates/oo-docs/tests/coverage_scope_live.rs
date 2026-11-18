//! A pack of a scope does not wait for a long recording elsewhere in the Drive (D77, B-L6rem): the freshness floor of a
//! share or a compile is measured over the files of its scope. The whole Drive still waits for everything.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test coverage_scope_live -- --ignored --test-threads=1`

mod common;

use std::time::{Duration, Instant};

use common::fx::{DRIVE, Fx};
use common::*;
use oo_docs::publish::run::PublishOutcome;
use oo_docs::share::coverage::{Freshness, prepare};

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_scope_does_not_wait_for_a_file_outside_it_that_is_still_being_parsed() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_cov_scope";
    let mut fx = Fx::new(db).await;
    fx.index_md(
        "Projects/a.md",
        "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n",
    )
    .await;
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("publish")
    };
    // A recording elsewhere in the Drive, still being parsed, with the newest change of the Drive.
    fx.plant_md("Meetings/long.md", "# 例会\n").await;
    fx.admin_sql(&format!(
        "UPDATE _oo_docs.file SET state = 'parsing' WHERE path = 'Meetings/long.md'; \
         UPDATE _oo_docs.drive SET rev = rev + 10 WHERE id = '{DRIVE}'; \
         UPDATE _oo_docs.file_version SET drive_rev = (SELECT rev FROM _oo_docs.drive WHERE id = '{DRIVE}') \
          WHERE path_at_version = 'Meetings/long.md'; \
         UPDATE _oo_docs.file_version SET drive_rev = 1 WHERE path_at_version = 'Projects/a.md'"
    ))
    .await;
    let pool = pool_runtime(db, 2).await;
    let f = Freshness {
        wait_ms: 1500,
        ..Freshness::default()
    };

    let t = Instant::now();
    let scoped = prepare(&pool, DRIVE, &["Projects".to_owned()], &f)
        .await
        .unwrap();
    assert!(
        t.elapsed() < Duration::from_millis(700),
        "a scope without the recording does not wait: {:?}",
        t.elapsed()
    );
    // The number reported stays the Drive's (FM10): the recording holds it. The wait did not.
    assert!(scoped.covered_rev < scoped.drive_rev, "{scoped:?}");
    assert!(scoped.pending.is_empty() && scoped.in_scope == 1);

    let t = Instant::now();
    let whole = prepare(&pool, DRIVE, &[], &f).await.unwrap();
    assert!(
        t.elapsed() >= Duration::from_millis(1400),
        "the whole Drive waits for the recording: {:?}",
        t.elapsed()
    );
    assert!(whole.covered_rev < whole.drive_rev, "{whole:?}");
    assert_eq!(whole.pending.len(), 1);

    // A scope that contains the recording waits for it too.
    let t = Instant::now();
    let with = prepare(&pool, DRIVE, &["Meetings".to_owned()], &f)
        .await
        .unwrap();
    assert!(t.elapsed() >= Duration::from_millis(1400));
    assert!(with.covered_rev < with.drive_rev);
}
