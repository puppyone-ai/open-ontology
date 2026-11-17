//! `_oo_docs` installer against one throwaway PostgreSQL (B-S2-01).
//!
//! Run with a labelled container and
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test install -- --ignored`.

mod common;

use common::*;
use oo_docs::store::install::{
    InstallError, InstallOutcome, ReadinessError, SCHEMA_MIN, install, migrations, sha256_hex,
    verify_readiness,
};
use sqlx::{Connection, Executor, Row};

#[test]
fn compiled_files_have_stable_shape() {
    let list = migrations();
    assert_eq!(list.len(), 4);
    let mut seen = std::collections::HashSet::new();
    for (m, min) in list.iter().zip(SCHEMA_MIN) {
        assert_eq!(m.version, min);
        assert_eq!(m.sha256().len(), 64);
        assert!(seen.insert(m.sha256()), "duplicate digest");
        assert!(m.file.starts_with("01") && m.file.ends_with(".sql"));
    }
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn empty_db_install_then_readiness() {
    let _g = CLUSTER_LOCK.lock().await;
    installed_db("oo_docs_it_empty").await;
    let mut admin = connect("oo_docs_it_empty").await;
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT version, sha256 FROM _oo_docs.schema_migrations ORDER BY applied_at",
    )
    .fetch_all(&mut admin)
    .await
    .unwrap();
    assert_eq!(rows.len(), 4);
    for (row, m) in rows.iter().zip(migrations()) {
        assert_eq!(row.0, m.version);
        assert_eq!(row.1, m.sha256());
    }
    let tables: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pg_tables WHERE schemaname = '_oo_docs'")
            .fetch_one(&mut admin)
            .await
            .unwrap();
    assert_eq!(tables, 27);

    // The runtime principal can verify readiness without installing.
    let mut rt = connect_runtime("oo_docs_it_empty").await;
    let verified = verify_readiness(&mut rt).await.expect("ready");
    assert_eq!(verified.known, SCHEMA_MIN.to_vec());
    assert!(verified.unknown_additive.is_empty());
    // ...and cannot install.
    match install(&mut rt).await {
        Err(InstallError::NotAdministrator) => {}
        other => panic!("{other:?}"),
    }
    // Functions are not executable by PUBLIC.
    let public_exec: bool = sqlx::query_scalar(
        "SELECT has_function_privilege('public', '_oo_docs.append_event(text, uuid, text, jsonb)', 'EXECUTE')",
    )
    .fetch_one(&mut admin)
    .await
    .unwrap();
    assert!(!public_exec);
    drop_db("oo_docs_it_empty").await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn repeat_install_is_a_noop() {
    let _g = CLUSTER_LOCK.lock().await;
    installed_db("oo_docs_it_repeat").await;
    let mut admin = connect("oo_docs_it_repeat").await;
    let before: Vec<(String, String)> = sqlx::query_as(
        "SELECT version, applied_at::text FROM _oo_docs.schema_migrations ORDER BY version",
    )
    .fetch_all(&mut admin)
    .await
    .unwrap();
    let xmin_before: Vec<String> =
        sqlx::query_scalar("SELECT xmin::text FROM _oo_docs.schema_migrations ORDER BY version")
            .fetch_all(&mut admin)
            .await
            .unwrap();
    for _ in 0..2 {
        assert_eq!(install(&mut admin).await.unwrap(), InstallOutcome::Replay);
    }
    let after: Vec<(String, String)> = sqlx::query_as(
        "SELECT version, applied_at::text FROM _oo_docs.schema_migrations ORDER BY version",
    )
    .fetch_all(&mut admin)
    .await
    .unwrap();
    let xmin_after: Vec<String> =
        sqlx::query_scalar("SELECT xmin::text FROM _oo_docs.schema_migrations ORDER BY version")
            .fetch_all(&mut admin)
            .await
            .unwrap();
    assert_eq!(before, after, "applied_at changed");
    assert_eq!(xmin_before, xmin_after, "rows were rewritten");
    drop_db("oo_docs_it_repeat").await;
}

async fn tamper(admin: &mut sqlx::PgConnection, sql: &'static str) {
    admin
        .execute(
            "ALTER TABLE _oo_docs.schema_migrations DISABLE TRIGGER schema_migrations_insert_only",
        )
        .await
        .unwrap();
    admin.execute(sql).await.unwrap();
    admin
        .execute(
            "ALTER TABLE _oo_docs.schema_migrations ENABLE TRIGGER schema_migrations_insert_only",
        )
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn checksum_mismatch_raises_oo001_and_changes_nothing() {
    let _g = CLUSTER_LOCK.lock().await;
    installed_db("oo_docs_it_sum").await;
    let mut admin = connect("oo_docs_it_sum").await;
    let lie = "e".repeat(64);
    tamper(
        &mut admin,
        "UPDATE _oo_docs.schema_migrations SET sha256 = repeat('e', 64) WHERE version = 'docs-index/1'",
    )
    .await;
    let err = install(&mut admin).await.unwrap_err();
    assert_eq!(err.sqlstate(), Some("OO001"), "{err}");
    match &err {
        InstallError::ChecksumMismatch { version, stored } => {
            assert_eq!(version, "docs-index/1");
            assert_eq!(stored, &lie);
        }
        other => panic!("{other:?}"),
    }
    let stored: String = sqlx::query_scalar(
        "SELECT sha256 FROM _oo_docs.schema_migrations WHERE version = 'docs-index/1'",
    )
    .fetch_one(&mut admin)
    .await
    .unwrap();
    assert_eq!(stored, lie, "row must not be repaired");
    match verify_readiness(&mut admin).await {
        Err(ReadinessError::ChecksumMismatch { version, .. }) => {
            assert_eq!(version, "docs-index/1")
        }
        other => panic!("{other:?}"),
    }
    drop_db("oo_docs_it_sum").await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn missing_prerequisite_raises_oo002_and_runs_nothing() {
    let _g = CLUSTER_LOCK.lock().await;
    installed_db("oo_docs_it_pre").await;
    let mut admin = connect("oo_docs_it_pre").await;
    tamper(
        &mut admin,
        "DELETE FROM _oo_docs.schema_migrations WHERE version = 'docs-sources-jobs/1'",
    )
    .await;
    let err = install(&mut admin).await.unwrap_err();
    assert_eq!(err.sqlstate(), Some("OO002"), "{err}");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo_docs.schema_migrations")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    assert_eq!(n, 3);
    match verify_readiness(&mut admin).await {
        Err(ReadinessError::Incomplete { missing }) => assert_eq!(missing, "docs-sources-jobs/1"),
        other => panic!("{other:?}"),
    }
    drop_db("oo_docs_it_pre").await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn unknown_versions_follow_compat() {
    let _g = CLUSTER_LOCK.lock().await;
    installed_db("oo_docs_it_compat").await;
    let mut admin = connect("oo_docs_it_compat").await;
    // A later additive migration from a newer binary does not stop an older one.
    admin
        .execute("INSERT INTO _oo_docs.schema_migrations (version, sha256) VALUES ('docs-llm/1', repeat('a', 64))")
        .await
        .unwrap();
    assert_eq!(install(&mut admin).await.unwrap(), InstallOutcome::Replay);
    let v = verify_readiness(&mut admin).await.unwrap();
    assert_eq!(v.unknown_additive, vec!["docs-llm/1".to_string()]);
    admin
        .execute("INSERT INTO _oo_docs.schema_migrations (version, sha256, compat) VALUES ('docs-vectors/1', repeat('b', 64), 'breaking')")
        .await
        .unwrap();
    match verify_readiness(&mut admin).await {
        Err(ReadinessError::SchemaMismatch { version }) => assert_eq!(version, "docs-vectors/1"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        install(&mut admin).await,
        Err(InstallError::Refused(_))
    ));
    drop_db("oo_docs_it_compat").await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn runtime_role_has_no_direct_dml_on_job_and_event_tables() {
    let _g = CLUSTER_LOCK.lock().await;
    installed_db("oo_docs_it_dml").await;
    let mut rt = connect_runtime("oo_docs_it_dml").await;
    for sql in [
        "INSERT INTO _oo_docs.job (id, kind, dedupe_key, state, max_attempts) VALUES (gen_random_uuid(), 'sync', 'k', 'queued', 3)",
        "UPDATE _oo_docs.job SET priority = 1",
        "DELETE FROM _oo_docs.job",
        "TRUNCATE _oo_docs.job",
        "INSERT INTO _oo_docs.event (seq, type, payload) VALUES (1, 'a.b', '{}')",
        "UPDATE _oo_docs.event SET seq = seq",
        "DELETE FROM _oo_docs.event",
        "UPDATE _oo_docs.event_head SET seq = 99",
        "SELECT * FROM _oo_docs.event_head",
        "UPDATE _oo_docs.schema_migrations SET compat = 'additive'",
        "INSERT INTO _oo_docs.schema_migrations (version, sha256) VALUES ('docs-x/1', repeat('a', 64))",
    ] {
        assert_eq!(sqlstate(rt.execute(sql).await), "42501", "{sql}");
    }
    // The sanctioned path works and reads are allowed.
    let id: String = sqlx::query_scalar(
        "SELECT job_id::text FROM _oo_docs.enqueue(NULL, 'sync', NULL, NULL, NULL, 'it:1', 100::smallint, 3, NULL)",
    )
    .fetch_one(&mut rt)
    .await
    .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo_docs.job WHERE id = $1::uuid")
        .bind(&id)
        .fetch_one(&mut rt)
        .await
        .unwrap();
    assert_eq!(n, 1);
    let seq: i64 =
        sqlx::query_scalar("SELECT _oo_docs.append_event('job.enqueued', NULL, NULL, '{}')")
            .fetch_one(&mut rt)
            .await
            .unwrap();
    assert_eq!(seq, 1);
    // Internal helpers are not callable.
    assert_eq!(
        sqlstate(
            rt.execute("SELECT _oo_docs._finish(NULL, NULL, NULL, 'succeeded', NULL)")
                .await
        ),
        "42501"
    );
    // The runtime cannot assume the owner role.
    assert_eq!(
        sqlstate(rt.execute("SET ROLE oo_docs_owner").await),
        "42501"
    );
    drop_db("oo_docs_it_dml").await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn insert_only_tables_reject_mutation_even_for_the_owner() {
    let _g = CLUSTER_LOCK.lock().await;
    installed_db("oo_docs_it_ins").await;
    let mut admin = connect("oo_docs_it_ins").await;
    admin
        .execute(
            "INSERT INTO _oo_docs.drive (id, name, backend, config) \
             VALUES ('00000000-0000-0000-0000-000000000001', 'd', 'fs', '{}'); \
             INSERT INTO _oo_docs.file (id, drive_id, path, raw_key, path_fold, state, last_seen_generation) \
             VALUES ('00000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000001', 'a.md', '\\x61', 'a.md', 'stored', 1); \
             INSERT INTO _oo_docs.file_state_history (file_id, seq, to_state) \
             VALUES ('00000000-0000-0000-0000-000000000002', 1, 'stored'); \
             SELECT _oo_docs.append_event('file.state_changed', NULL, NULL, '{}')",
        )
        .await
        .unwrap();
    // superuser and owner both hit the trigger
    for role in [None, Some("oo_docs_owner")] {
        let mut tx = admin.begin().await.unwrap();
        if let Some(r) = role {
            tx.execute(sqlx::AssertSqlSafe(format!("SET LOCAL ROLE {r}")))
                .await
                .unwrap();
        }
        for sql in [
            "UPDATE _oo_docs.event SET type = 'x.y'",
            "DELETE FROM _oo_docs.event",
            "TRUNCATE _oo_docs.event",
            "UPDATE _oo_docs.file_state_history SET code = 'x'",
            "DELETE FROM _oo_docs.file_state_history",
            "TRUNCATE _oo_docs.file_state_history",
            "UPDATE _oo_docs.schema_migrations SET compat = 'additive'",
            "DELETE FROM _oo_docs.schema_migrations",
        ] {
            let mut sp = tx.begin().await.unwrap();
            assert_eq!(sqlstate(sp.execute(sql).await), "OO030", "{role:?} {sql}");
            sp.rollback().await.unwrap();
        }
        tx.rollback().await.unwrap();
    }
    // The tables with later insert-only triggers too.
    let row = admin
        .fetch_one("SELECT count(*) AS n FROM pg_trigger WHERE tgfoid = '_oo_docs.reject_mutation'::regproc")
        .await
        .unwrap();
    assert!(row.get::<i64, _>("n") >= 14);
    drop_db("oo_docs_it_ins").await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn non_superuser_cannot_bootstrap() {
    let _g = CLUSTER_LOCK.lock().await;
    installed_db("oo_docs_it_noadmin").await; // creates the login role
    create_db("oo_docs_it_noadmin2").await;
    let mut rt = connect_runtime("oo_docs_it_noadmin2").await;
    assert!(matches!(
        install(&mut rt).await,
        Err(InstallError::NotAdministrator)
    ));
    drop_db("oo_docs_it_noadmin").await;
    drop_db("oo_docs_it_noadmin2").await;
}
