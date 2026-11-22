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
    assert_eq!(list.len(), 18);
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

/// Whether the server ships pgvector (the optional `docs-vectors/1` step, B-S13-01).
async fn has_vector(conn: &mut sqlx::PgConnection) -> bool {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_available_extensions WHERE name = 'vector')",
    )
    .fetch_one(conn)
    .await
    .unwrap()
}

#[test]
fn vectors_migration_is_optional_and_not_required() {
    use oo_docs::store::install::{VECTORS, VERSION_VECTORS};
    assert_eq!(VECTORS.version, VERSION_VECTORS);
    assert_eq!(VECTORS.file, "0144_docs_vectors.sql");
    assert!(!SCHEMA_MIN.contains(&VERSION_VECTORS));
    assert!(!migrations().iter().any(|m| m.version == VERSION_VECTORS));
}

/// On a pgvector server the table exists, is usable by the runtime role and ranks like the Rust
/// cosine; on a stock server nothing is installed and readiness still passes.
#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container; pgvector image for the full check)"]
async fn vectors_table_follows_the_server_image() {
    let _g = CLUSTER_LOCK.lock().await;
    installed_db("oo_docs_it_vec").await;
    let mut admin = connect("oo_docs_it_vec").await;
    let table: bool = sqlx::query_scalar("SELECT to_regclass('_oo_docs.block_vector') IS NOT NULL")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    assert_eq!(table, has_vector(&mut admin).await);
    if table {
        // Second install is still a replay and readiness accepts the optional row.
        assert_eq!(install(&mut admin).await.unwrap(), InstallOutcome::Replay);
        let mut rt = connect_runtime("oo_docs_it_vec").await;
        let v = verify_readiness(&mut rt).await.expect("ready");
        assert!(v.unknown_additive.is_empty());
        let drive: String = sqlx::query_scalar(
            "INSERT INTO _oo_docs.drive (id, name, backend, config) VALUES (gen_random_uuid(), 'v', 'fs', '{}') RETURNING id::text",
        )
        .fetch_one(&mut admin)
        .await
        .unwrap();
        {
            let q = oo_docs::vector::embed("quarterly budget review");
            let lit = |v: &oo_docs::vector::Vector| {
                format!(
                    "[{}]",
                    v.iter().map(i16::to_string).collect::<Vec<_>>().join(",")
                )
            };
            for (id, t) in [("b1", "quarterly budget review"), ("b2", "zebra paint")] {
                let e = lit(&oo_docs::vector::embed(t));
                sqlx::query("INSERT INTO _oo_docs.block_vector (drive_id, block_id, model_version, dim, embedding) VALUES ($1::uuid, $2, $3, 256, $4::vector)")
                    .bind(&drive).bind(id).bind(oo_docs::vector::MODEL_VERSION).bind(e)
                    .execute(&mut admin).await.unwrap();
            }
            let top: String = sqlx::query_scalar("SELECT block_id FROM _oo_docs.block_vector WHERE drive_id = $1::uuid ORDER BY embedding <=> $2::vector, block_id LIMIT 1")
                .bind(&drive).bind(lit(&q)).fetch_one(&mut admin).await.unwrap();
            assert_eq!(top, "b1");
        }
    }
    drop_db("oo_docs_it_vec").await;
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
    let extra = usize::from(has_vector(&mut admin).await);
    assert_eq!(rows.len(), 18 + extra);
    for (row, m) in rows.iter().zip(migrations()) {
        assert_eq!(row.0, m.version);
        assert_eq!(row.1, m.sha256());
    }
    let tables: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pg_tables WHERE schemaname = '_oo_docs'")
            .fetch_one(&mut admin)
            .await
            .unwrap();
    assert_eq!(tables, 44 + i64::try_from(extra).unwrap());

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
    assert_eq!(n, 16 + i64::from(has_vector(&mut admin).await));
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
        .execute("INSERT INTO _oo_docs.schema_migrations (version, sha256) VALUES ('docs-future/1', repeat('a', 64))")
        .await
        .unwrap();
    assert_eq!(install(&mut admin).await.unwrap(), InstallOutcome::Replay);
    let v = verify_readiness(&mut admin).await.unwrap();
    assert_eq!(v.unknown_additive, vec!["docs-future/1".to_string()]);
    admin
        .execute("INSERT INTO _oo_docs.schema_migrations (version, sha256, compat) VALUES ('docs-future/2', repeat('b', 64), 'breaking')")
        .await
        .unwrap();
    match verify_readiness(&mut admin).await {
        Err(ReadinessError::SchemaMismatch { version }) => assert_eq!(version, "docs-future/2"),
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

/// A database at 0153 that holds a Drive on the old default (max_files 10000) upgrades to 0154 in place: the cap moves to the new
/// default, the new bound and the job_file index exist, and the Drive's own data is untouched.
#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_database_at_0153_with_a_capped_drive_upgrades_to_0154() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_it_upgrade_0154";
    installed_db(db).await;
    let mut admin = connect(db).await;
    // Put the database back to the state 0153 leaves: old CHECK and default, no index, no 0154 row.
    for sql in [
        "ALTER TABLE _oo_docs.drive DROP CONSTRAINT drive_max_files_check",
        "ALTER TABLE _oo_docs.drive ADD CONSTRAINT drive_max_files_check CHECK (max_files BETWEEN 1 AND 10000)",
        "ALTER TABLE _oo_docs.drive ALTER COLUMN max_files SET DEFAULT 10000",
        "DROP INDEX _oo_docs.job_file",
    ] {
        admin.execute(sql).await.unwrap();
    }
    admin
        .execute("INSERT INTO _oo_docs.drive (id, name, backend, config, max_files) VALUES ('00000000-0000-4000-8000-0000000000a1', 'old', 'fs', '{}', 10000), ('00000000-0000-4000-8000-0000000000a2', 'custom', 'fs', '{}', 500)")
        .await
        .unwrap();
    // The state 0153 leaves has neither the media class table (0157) nor the profile column (0159).
    admin
        .execute("DROP TABLE _oo_docs.media_profile")
        .await
        .unwrap();
    admin
        .execute("ALTER TABLE _oo_docs.file DROP COLUMN profile, DROP COLUMN profile_reason")
        .await
        .unwrap();
    admin
        .execute("DROP TABLE _oo_docs.profile_override")
        .await
        .unwrap();
    tamper(
        &mut admin,
        "DELETE FROM _oo_docs.schema_migrations WHERE version IN ('docs-drive-scale/1', 'docs-media-profile/1', 'docs-file-profile/1')",
    )
    .await;
    match install(&mut admin).await.expect("upgrade") {
        InstallOutcome::Applied(versions) => {
            assert_eq!(
                versions,
                vec![
                    "docs-drive-scale/1",
                    "docs-media-profile/1",
                    "docs-file-profile/1"
                ]
            )
        }
        other => panic!("{other:?}"),
    }
    let caps: Vec<(String, i32)> =
        sqlx::query_as("SELECT name, max_files FROM _oo_docs.drive ORDER BY name")
            .fetch_all(&mut admin)
            .await
            .unwrap();
    assert_eq!(
        caps,
        vec![("custom".to_owned(), 500), ("old".to_owned(), 1_000_000)]
    );
    let index: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_indexes WHERE schemaname = '_oo_docs' AND indexname = 'job_file'",
    )
    .fetch_one(&mut admin)
    .await
    .unwrap();
    assert_eq!(index, 1);
    admin
        .execute("UPDATE _oo_docs.drive SET max_files = 5000000 WHERE name = 'custom'")
        .await
        .expect("the new bound allows a larger cap");
    drop_db(db).await;
}

/// A database at 0154 upgrades to 0157 in place: the recording class table appears and takes a row per blob.
#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_database_at_0154_upgrades_to_0157_with_the_media_profile_table() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_it_upgrade_0157";
    installed_db(db).await;
    let mut admin = connect(db).await;
    // 0159 needs 0157: it comes off first.
    admin
        .execute("ALTER TABLE _oo_docs.file DROP COLUMN profile, DROP COLUMN profile_reason")
        .await
        .unwrap();
    admin
        .execute("DROP TABLE _oo_docs.profile_override")
        .await
        .unwrap();
    admin
        .execute("DROP TABLE _oo_docs.media_profile")
        .await
        .unwrap();
    tamper(
        &mut admin,
        "DELETE FROM _oo_docs.schema_migrations WHERE version IN ('docs-media-profile/1', 'docs-file-profile/1')",
    )
    .await;
    match install(&mut admin).await.expect("upgrade") {
        InstallOutcome::Applied(versions) => assert_eq!(
            versions,
            vec!["docs-media-profile/1", "docs-file-profile/1"]
        ),
        other => panic!("{other:?}"),
    }
    admin
        .execute(sqlx::AssertSqlSafe(format!(
            "INSERT INTO _oo_docs.media_profile (b3, class, kind, confidence_bp, source) \
             VALUES ('{}', 'conversation', 'conversation', 9000, 'rules')",
            "a".repeat(64)
        )))
        .await
        .expect("a row per blob");
    assert!(
        admin
            .execute("INSERT INTO _oo_docs.media_profile (b3, class, kind, confidence_bp, source) VALUES ('x', 'meeting', 'k', 1, 'rules')")
            .await
            .is_err(),
        "the class is closed"
    );
    drop_db(db).await;
}

/// A database at 0157 upgrades to 0159 in place: the nullable `profile` and `profile_reason` columns and the override table exist, rows that were there
/// before keep a NULL profile, the column takes only the eight profiles and the predicate check knows `cites` and `authored`.
#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_database_at_0157_upgrades_to_0159_with_the_file_class() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_it_upgrade_0159";
    installed_db(db).await;
    let mut admin = connect(db).await;
    admin
        .execute("ALTER TABLE _oo_docs.file DROP COLUMN profile, DROP COLUMN profile_reason")
        .await
        .unwrap();
    admin
        .execute("DROP TABLE _oo_docs.profile_override")
        .await
        .unwrap();
    tamper(
        &mut admin,
        "DELETE FROM _oo_docs.schema_migrations WHERE version = 'docs-file-profile/1'",
    )
    .await;
    match install(&mut admin).await.expect("upgrade") {
        InstallOutcome::Applied(versions) => assert_eq!(versions, vec!["docs-file-profile/1"]),
        other => panic!("{other:?}"),
    }
    let nullable: String = sqlx::query_scalar(
        "SELECT is_nullable FROM information_schema.columns \
          WHERE table_schema = '_oo_docs' AND table_name = 'file' AND column_name = 'profile'",
    )
    .fetch_one(&mut admin)
    .await
    .unwrap();
    assert_eq!(nullable, "YES");
    let check: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_constraint c JOIN pg_class t ON t.oid = c.conrelid \
          WHERE t.relname = 'file' AND c.contype = 'c' AND pg_get_constraintdef(c.oid) LIKE '%instruction%'",
    )
    .fetch_one(&mut admin)
    .await
    .unwrap();
    assert_eq!(check, 1);
    admin
        .execute("INSERT INTO _oo_docs.profile_override (drive_id, scope, path, profile) SELECT id, 'dir', 'notes', 'instruction' FROM _oo_docs.drive LIMIT 0")
        .await
        .expect("the override table exists");
    let cites: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_constraint WHERE conname = 'fact_predicate_check' AND pg_get_constraintdef(oid) LIKE '%cites%' AND pg_get_constraintdef(oid) LIKE '%has_participant%'",
    )
    .fetch_one(&mut admin)
    .await
    .unwrap();
    assert_eq!(
        cites, 1,
        "the check keeps the media relations and adds cites and authored"
    );
    drop_db(db).await;
}

/// A database at 0159 upgrades to 0160 in place: the four structure tables (D93) exist, empty, and the class table takes
/// only the three statuses.
#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_database_at_0159_upgrades_to_0160_with_the_structure_layer() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_it_upgrade_0160";
    installed_db(db).await;
    let mut admin = connect(db).await;
    admin
        .execute(
            "DROP TABLE _oo_docs.structure_member, _oo_docs.structure_class, _oo_docs.structure_version, _oo_docs.structure_observation",
        )
        .await
        .unwrap();
    tamper(
        &mut admin,
        "DELETE FROM _oo_docs.schema_migrations WHERE version = 'docs-structure/1'",
    )
    .await;
    match install(&mut admin).await.expect("upgrade") {
        InstallOutcome::Applied(versions) => assert_eq!(versions, vec!["docs-structure/1"]),
        other => panic!("{other:?}"),
    }
    let tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.tables WHERE table_schema = '_oo_docs' AND table_name LIKE 'structure\\_%'",
    )
    .fetch_one(&mut admin)
    .await
    .unwrap();
    assert_eq!(tables, 4);
    let bad = admin
        .execute("INSERT INTO _oo_docs.structure_version (drive_id, n, reason, actor, doc) VALUES (gen_random_uuid(), 1, 'whatever', 'engine', '{}')")
        .await;
    assert!(bad.is_err(), "the version reason is a closed list");
}
