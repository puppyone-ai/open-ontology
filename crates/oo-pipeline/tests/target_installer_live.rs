//! Production target installer against one private PostgreSQL 17.
//!
//! The ignored test uses the caller-owned `PgConnection` API. Fixture
//! databases are sequential on that server. This is not a Loader and it
//! does not admit a source reader.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use oo_pipeline::target_installer::{
    compiled_digests, compiled_full_import_digest, install, install_full_import, sha256_hex,
    verify_full_import_readiness, verify_readiness, FullImportInstallOutcome, InstallError,
    InstallOutcome, ReadinessError, HISTORICAL_V2_SHA256, VERSION_BOOTSTRAP, VERSION_FENCE,
    VERSION_FULL_IMPORT, VERSION_MAPPING, VERSION_PREFLIGHT, VERSION_SEGMENT_V2,
};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::Connection;

const BOOTSTRAP: &str = include_str!("../migrations/0000_bootstrap_roles.sql");
const FENCE: &str = include_str!("../migrations/0001_target_fence_expand.sql");
const SEGMENT: &str = include_str!("../migrations/0002_target_segment_null_expand.sql");
const INSTALLER: &str = include_str!("../src/target_installer.rs");
const LIE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

#[test]
fn installer_embeds_production_scripts_and_not_v2() {
    assert!(INSTALLER.contains("include_str!(\"../migrations/0000_bootstrap_roles.sql\")"));
    assert!(INSTALLER.contains("include_str!(\"../migrations/0001_target_fence_expand.sql\")"));
    assert!(INSTALLER.contains("include_str!(\"../migrations/0003_target_segment_preflight_expand.sql\")"));
    assert!(INSTALLER.contains("include_str!(\"../migrations/0004_physical_mapping.sql\")"));
    assert!(!INSTALLER.contains("include_str!(\"../migrations/0002_target_segment_null_expand.sql\")"));
    assert_eq!(sha256_hex(SEGMENT.as_bytes()), HISTORICAL_V2_SHA256);
    let digests = compiled_digests();
    assert_ne!(digests.preflight(), HISTORICAL_V2_SHA256);
    assert_ne!(digests.mapping(), HISTORICAL_V2_SHA256);
}

#[tokio::test]
#[ignore = "one private PostgreSQL 17.10; OO_TARGET_INSTALLER_HOST must be set by the deciding run"]
async fn production_installer_on_one_postgres() {
    let host = env::var("OO_TARGET_INSTALLER_HOST").expect("OO_TARGET_INSTALLER_HOST");
    let port: u16 = env::var("OO_TARGET_INSTALLER_PORT")
        .expect("OO_TARGET_INSTALLER_PORT")
        .parse()
        .expect("port");
    let user = env::var("OO_TARGET_INSTALLER_USER").expect("OO_TARGET_INSTALLER_USER");
    let password = fs::read_to_string(
        env::var("OO_TARGET_INSTALLER_PASSWORD_FILE").expect("OO_TARGET_INSTALLER_PASSWORD_FILE"),
    )
    .expect("password file")
    .trim()
    .to_string();
    let evidence = PathBuf::from(
        env::var("OO_TARGET_INSTALLER_EVIDENCE").expect("OO_TARGET_INSTALLER_EVIDENCE"),
    );
    fs::create_dir_all(&evidence).expect("evidence dir");
    let digests = compiled_digests();
    let mut report = String::new();
    let mut admin = connect(&host, port, &user, &password, "postgres").await;
    let version = scalar(&mut admin, "SELECT version()").await;
    assert!(version.contains("17.10"), "{version}");
    report.push_str(&format!("server={version}\n"));

    reset(&mut admin).await;
    let mut empty = created(&host, port, &user, &password, &mut admin, "oo_a150_empty").await;
    match verify_readiness(&mut empty).await {
        Err(ReadinessError::NotInstalled) => {}
        other => panic!("empty before install: {other:?}"),
    }
    let first = install(&mut empty).await.expect("fresh install");
    assert_eq!(first, InstallOutcome::Bootstrapped);
    let fresh_rows = migration_text(&mut empty).await;
    assert_installed(&fresh_rows, &digests, false);
    assert_eq!(count_version(&mut empty, VERSION_SEGMENT_V2).await, 0);
    assert_eq!(scalar(&mut empty, "SELECT count(*)::text FROM pg_catalog.pg_publication").await, "0");
    assert_eq!(
        scalar(&mut empty, "SELECT count(*)::text FROM _oo.apply_receipts").await,
        "0"
    );
    assert_eq!(roles(&mut empty).await, expected_roles());
    assert_eq!(logged(&mut empty).await, expected_logged());
    assert_eq!(functions(&mut empty).await, expected_functions());
    let ready = verify_readiness(&mut empty).await.expect("fresh readiness");
    assert_eq!(ready.digests().bootstrap(), digests.bootstrap());
    assert_eq!(ready.digests().fence(), digests.fence());
    assert_eq!(ready.digests().preflight(), digests.preflight());
    assert_eq!(ready.digests().mapping(), digests.mapping());
    assert!(!ready.retains_historical_v2());
    tokio::time::sleep(Duration::from_millis(50)).await;
    let replay = install(&mut empty).await.expect("replay");
    assert_eq!(replay, InstallOutcome::Replay);
    let replay_rows = migration_text(&mut empty).await;
    assert_eq!(replay_rows, fresh_rows);
    report.push_str("empty=bootstrapped\nempty_replay=replay\nempty_applied_at_same=1\nempty_v2_rows=0\n");

    let runtime_password = hex_bytes(&read_random(16));
    exec(
        &mut empty,
        "CREATE ROLE oo_a150_runtime LOGIN PASSWORD '",
        &runtime_password,
        "' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION",
    )
    .await;
    raw(&mut empty, "GRANT oo_pipeline_runtime TO oo_a150_runtime").await;
    let mut runtime = connect(&host, port, "oo_a150_runtime", &runtime_password, "oo_a150_empty").await;
    let runtime_ready = verify_readiness(&mut runtime).await.expect("runtime readiness");
    assert_eq!(runtime_ready.digests().mapping(), digests.mapping());
    assert!(!runtime_ready.retains_historical_v2());
    match install(&mut runtime).await {
        Err(InstallError::NotAdministrator) => {}
        other => panic!("runtime install: {other:?}"),
    }
    drop(runtime);
    assert_eq!(migration_text(&mut empty).await, fresh_rows);
    report.push_str("runtime_verify=ok\nruntime_install=not_administrator\n");
    drop(empty);

    reset(&mut admin).await;
    let mut v1 = created(&host, port, &user, &password, &mut admin, "oo_a150_v1").await;
    harness_v1(&mut v1, &digests).await;
    let v1_before = migration_text(&mut v1).await;
    assert_eq!(count_version(&mut v1, VERSION_PREFLIGHT).await, 0);
    let upgraded = install(&mut v1).await.expect("v1 upgrade");
    assert_eq!(upgraded, InstallOutcome::UpgradedFromV1);
    let v1_after = migration_text(&mut v1).await;
    assert_eq!(applied_at_of(&v1_before, VERSION_BOOTSTRAP), applied_at_of(&v1_after, VERSION_BOOTSTRAP));
    assert_eq!(applied_at_of(&v1_before, VERSION_FENCE), applied_at_of(&v1_after, VERSION_FENCE));
    assert_eq!(count_version(&mut v1, VERSION_SEGMENT_V2).await, 0);
    assert_installed(&v1_after, &digests, false);
    let v1_ready = verify_readiness(&mut v1).await.expect("v1 readiness");
    assert!(!v1_ready.retains_historical_v2());
    report.push_str("v1=upgraded_from_v1\nv1_v2_rows=0\nv1_applied_at_same=1\n");
    drop(v1);

    reset(&mut admin).await;
    let mut v2 = created(&host, port, &user, &password, &mut admin, "oo_a150_v2").await;
    harness_v1(&mut v2, &digests).await;
    harness_v2(&mut v2).await;
    raw(&mut v2, LEGAL_RECEIPT).await;
    let receipt_before = scalar(&mut v2, RECEIPT_SQL).await;
    let v2_before = migration_text(&mut v2).await;
    let forward = install(&mut v2).await.expect("v2 upgrade");
    assert_eq!(forward, InstallOutcome::UpgradedFromV2);
    let v2_after = migration_text(&mut v2).await;
    assert_eq!(applied_at_of(&v2_before, VERSION_BOOTSTRAP), applied_at_of(&v2_after, VERSION_BOOTSTRAP));
    assert_eq!(applied_at_of(&v2_before, VERSION_FENCE), applied_at_of(&v2_after, VERSION_FENCE));
    assert_eq!(applied_at_of(&v2_before, VERSION_SEGMENT_V2), applied_at_of(&v2_after, VERSION_SEGMENT_V2));
    assert_eq!(checksum_of(&v2_after, VERSION_SEGMENT_V2), HISTORICAL_V2_SHA256);
    assert_eq!(scalar(&mut v2, RECEIPT_SQL).await, receipt_before);
    assert_installed(&v2_after, &digests, true);
    let v2_ready = verify_readiness(&mut v2).await.expect("v2 readiness");
    assert!(v2_ready.retains_historical_v2());
    report.push_str("v2=upgraded_from_v2\nv2_history_same=1\nv2_receipt_same=1\n");
    drop(v2);

    reset(&mut admin).await;
    let mut missing = created(&host, port, &user, &password, &mut admin, "oo_a150_miss").await;
    harness_v1(&mut missing, &digests).await;
    let miss_before = migration_text(&mut missing).await;
    raw(
        &mut missing,
        "ALTER TABLE _oo.apply_receipts DROP CONSTRAINT apply_receipts_segment_check",
    )
    .await;
    match install(&mut missing).await {
        Err(err) => {
            assert_eq!(err.sqlstate(), Some("OO002"), "{err}");
            assert!(err.to_string().contains("segment relation is absent"), "{err}");
        }
        Ok(outcome) => panic!("missing relation installed: {outcome}"),
    }
    assert_eq!(migration_text(&mut missing).await, miss_before);
    assert_eq!(count_version(&mut missing, VERSION_PREFLIGHT).await, 0);
    match verify_readiness(&mut missing).await {
        Err(ReadinessError::Incomplete) => {}
        other => panic!("missing readiness: {other:?}"),
    }
    report.push_str("oo002=OO002\noo002_unchanged=1\noo002_ready=incomplete\n");
    drop(missing);

    reset(&mut admin).await;
    let mut bad = created(&host, port, &user, &password, &mut admin, "oo_a150_bad").await;
    harness_v1(&mut bad, &digests).await;
    raw(
        &mut bad,
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES ('target-fence-receipts/3', 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', 'expand', '0003_target_segment_preflight_expand.sql')",
    )
    .await;
    let bad_before = migration_text(&mut bad).await;
    match install(&mut bad).await {
        Err(err) => {
            assert_eq!(err.sqlstate(), Some("OO001"), "{err}");
            assert_eq!(err.version(), Some(VERSION_PREFLIGHT));
        }
        Ok(outcome) => panic!("mismatch installed: {outcome}"),
    }
    let bad_after = migration_text(&mut bad).await;
    assert_eq!(bad_after, bad_before);
    assert_eq!(checksum_of(&bad_after, VERSION_PREFLIGHT), LIE);
    assert_eq!(count_version(&mut bad, VERSION_MAPPING).await, 0);
    match verify_readiness(&mut bad).await {
        Err(ReadinessError::ChecksumMismatch { version, stored }) => {
            assert_eq!(version, VERSION_PREFLIGHT);
            assert_eq!(stored, LIE);
        }
        other => panic!("mismatch readiness: {other:?}"),
    }
    report.push_str("oo001=OO001\noo001_unchanged=1\n");
    drop(bad);

    reset(&mut admin).await;
    let mut failed = created(&host, port, &user, &password, &mut admin, "oo_a150_fail").await;
    harness_v1(&mut failed, &digests).await;
    raw(&mut failed, ILLEGAL_RECEIPT).await;
    let fail_before = migration_text(&mut failed).await;
    let illegal_before = scalar(&mut failed, RECEIPT_SQL).await;
    match install(&mut failed).await {
        Err(err) => assert_eq!(err.sqlstate(), Some("OO021"), "{err}"),
        Ok(outcome) => panic!("illegal receipt installed: {outcome}"),
    }
    assert_eq!(migration_text(&mut failed).await, fail_before);
    assert_eq!(scalar(&mut failed, RECEIPT_SQL).await, illegal_before);
    assert_eq!(count_version(&mut failed, VERSION_PREFLIGHT).await, 0);
    match verify_readiness(&mut failed).await {
        Err(ReadinessError::Incomplete) => {}
        other => panic!("failed readiness: {other:?}"),
    }
    report.push_str("fail=OO021\nfail_unchanged=1\nfail_ready=incomplete\n");
    drop(failed);

    reset(&mut admin).await;
    let mut bad_full = created(&host, port, &user, &password, &mut admin, "oo_a154_bad").await;
    assert_eq!(
        install(&mut bad_full).await.expect("0005 base"),
        InstallOutcome::Bootstrapped
    );
    raw(
        &mut bad_full,
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES ('full-import/v1', 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', 'expand', '0005_full_import_expand.sql')",
    )
    .await;
    let bad_full_before = migration_text(&mut bad_full).await;
    match install_full_import(&mut bad_full).await {
        Err(err) => {
            assert_eq!(err.sqlstate(), Some("OO001"), "{err}");
            assert_eq!(err.version(), Some(VERSION_FULL_IMPORT));
        }
        Ok(outcome) => panic!("bad 0005 installed: {outcome}"),
    }
    assert_eq!(migration_text(&mut bad_full).await, bad_full_before);
    assert_eq!(checksum_of(&bad_full_before, VERSION_FULL_IMPORT), LIE);
    verify_readiness(&mut bad_full).await.expect("0004 readiness ignores 0005");
    match verify_full_import_readiness(&mut bad_full).await {
        Err(ReadinessError::ChecksumMismatch { version, stored }) => {
            assert_eq!(version, VERSION_FULL_IMPORT);
            assert_eq!(stored, LIE);
        }
        other => panic!("bad 0005 readiness: {other:?}"),
    }
    report.push_str("full_import_oo001=OO001\nfull_import_oo001_unchanged=1\n");
    drop(bad_full);

    reset(&mut admin).await;
    let mut full = created(&host, port, &user, &password, &mut admin, "oo_a154_ok").await;
    assert_eq!(
        install(&mut full).await.expect("0005 fresh"),
        InstallOutcome::Bootstrapped
    );
    match verify_full_import_readiness(&mut full).await {
        Err(ReadinessError::Incomplete) => {}
        other => panic!("0005 before install: {other:?}"),
    }
    let stamped = migration_text(&mut full).await;
    assert_eq!(
        install_full_import(&mut full).await.expect("0005 install"),
        FullImportInstallOutcome::Installed
    );
    assert_eq!(
        install_full_import(&mut full).await.expect("0005 replay"),
        FullImportInstallOutcome::Replay
    );
    let expanded = migration_text(&mut full).await;
    for version in [VERSION_BOOTSTRAP, VERSION_FENCE, VERSION_PREFLIGHT, VERSION_MAPPING] {
        assert_eq!(applied_at_of(&stamped, version), applied_at_of(&expanded, version));
    }
    assert_eq!(checksum_of(&expanded, VERSION_FULL_IMPORT), compiled_full_import_digest());
    assert_eq!(install(&mut full).await.expect("0004 replay"), InstallOutcome::Replay);
    assert_eq!(migration_text(&mut full).await, expanded);
    let full_ready = verify_full_import_readiness(&mut full).await.expect("0005 ready");
    assert_eq!(full_ready.digest(), compiled_full_import_digest());
    let runtime_password = hex_bytes(&read_random(16));
    exec(
        &mut full,
        "CREATE ROLE oo_a154_runtime LOGIN PASSWORD '",
        &runtime_password,
        "' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION",
    )
    .await;
    raw(&mut full, "GRANT oo_pipeline_runtime TO oo_a154_runtime").await;
    let mut full_runtime = connect(&host, port, "oo_a154_runtime", &runtime_password, "oo_a154_ok").await;
    verify_full_import_readiness(&mut full_runtime)
        .await
        .expect("runtime 0005 readiness");
    match install_full_import(&mut full_runtime).await {
        Err(InstallError::NotAdministrator) => {}
        other => panic!("runtime 0005 install: {other:?}"),
    }
    drop(full_runtime);
    assert_eq!(migration_text(&mut full).await, expanded);
    assert!(logged_full(&mut full).await.contains("full_import_run=p"));
    assert!(logged_full(&mut full).await.contains("full_import_quality=p"));
    report.push_str("full_import_installed=1\nfull_import_replay=1\nfull_import_runtime_install=not_administrator\n");
    drop(full);
    reset(&mut admin).await;
    report.push_str("installer_live_ok\n");
    fs::write(evidence.join("outcome.txt"), &report).expect("outcome");
    print!("{report}");
}

fn expected_roles() -> String {
    "oo_pipeline_runtime|false|false|false|false|false|false|false\noo_target_owner|false|false|false|false|false|false|false\n"
        .to_string()
}

fn expected_logged() -> String {
    "apply_receipts=p\nmigration_lock=p\nphysical_mappings=p\nschema_migrations=p\ntarget_generations=p\n"
        .to_string()
}

fn expected_functions() -> String {
    "adopt_target_generation,commit_apply_receipt,lookup_physical_mapping,register_physical_mapping,schema_ready,segment_preflight_ready"
        .to_string()
}

fn assert_installed(rows: &str, digests: &oo_pipeline::target_installer::CompiledDigests, historical_v2: bool) {
    assert_eq!(checksum_of(rows, VERSION_BOOTSTRAP), digests.bootstrap());
    assert_eq!(checksum_of(rows, VERSION_FENCE), digests.fence());
    assert_eq!(checksum_of(rows, VERSION_PREFLIGHT), digests.preflight());
    assert_eq!(checksum_of(rows, VERSION_MAPPING), digests.mapping());
    assert!(rows.contains("expand|0000_bootstrap_roles.sql|"));
    assert!(rows.contains("expand|0004_physical_mapping.sql|"));
    assert!(rows.contains("|oo_target_owner"));
    if historical_v2 {
        assert_eq!(checksum_of(rows, VERSION_SEGMENT_V2), HISTORICAL_V2_SHA256);
    } else {
        assert!(!rows.contains(VERSION_SEGMENT_V2));
    }
}

fn checksum_of(rows: &str, version: &str) -> String {
    field(rows, version, 1)
}

fn applied_at_of(rows: &str, version: &str) -> String {
    field(rows, version, 4)
}

fn field(rows: &str, version: &str, index: usize) -> String {
    let line = rows
        .lines()
        .find(|line| line.starts_with(&format!("{version}|")))
        .unwrap_or_else(|| panic!("missing {version} in {rows}"));
    line.split('|')
        .nth(index)
        .unwrap_or_else(|| panic!("field {index} in {line}"))
        .to_string()
}

async fn created(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    admin: &mut PgConnection,
    name: &str,
) -> PgConnection {
    raw(
        admin,
        &format!(
            "CREATE DATABASE {name} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'"
        ),
    )
    .await;
    connect(host, port, user, password, name).await
}

async fn reset(admin: &mut PgConnection) {
    for name in [
        "oo_a150_empty",
        "oo_a150_v1",
        "oo_a150_v2",
        "oo_a150_miss",
        "oo_a150_bad",
        "oo_a150_fail",
        "oo_a154_bad",
        "oo_a154_ok",
        "oo_a154_src",
        "oo_a154_dst",
    ] {
        raw(admin, &format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await;
    }
    raw(admin, "DROP ROLE IF EXISTS oo_a154_runtime").await;
    raw(admin, "DROP ROLE IF EXISTS oo_a154_reader").await;
    raw(admin, "DROP ROLE IF EXISTS oo_a154_writer").await;
    raw(admin, "DROP ROLE IF EXISTS oo_a154_agent").await;
    raw(admin, "DROP ROLE IF EXISTS oo_a150_runtime").await;
    raw(admin, "DROP ROLE IF EXISTS oo_pipeline_runtime").await;
    raw(admin, "DROP ROLE IF EXISTS oo_target_owner").await;
}

async fn harness_v1(conn: &mut PgConnection, digests: &oo_pipeline::target_installer::CompiledDigests) {
    let mut tx = conn.begin().await.expect("v1 begin");
    script(&mut tx, BOOTSTRAP).await;
    script(&mut tx, "SET LOCAL ROLE oo_target_owner").await;
    script(&mut tx, FENCE).await;
    script(&mut tx, "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE").await;
    let sql = format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES \
         ('{VERSION_BOOTSTRAP}', '{}', 'expand', '0000_bootstrap_roles.sql'), \
         ('{VERSION_FENCE}', '{}', 'expand', '0001_target_fence_expand.sql')",
        digests.bootstrap(),
        digests.fence()
    );
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *tx)
        .await
        .expect("v1 markers");
    tx.commit().await.expect("v1 commit");
}

async fn harness_v2(conn: &mut PgConnection) {
    let mut tx = conn.begin().await.expect("v2 begin");
    script(&mut tx, "SELECT pg_catalog.pg_advisory_xact_lock(6404202605)").await;
    script(&mut tx, "SET LOCAL ROLE oo_target_owner").await;
    script(&mut tx, "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE").await;
    script(&mut tx, SEGMENT).await;
    let sql = format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES \
         ('{VERSION_SEGMENT_V2}', '{HISTORICAL_V2_SHA256}', 'expand', '0002_target_segment_null_expand.sql')"
    );
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *tx)
        .await
        .expect("v2 marker");
    tx.commit().await.expect("v2 commit");
}

async fn migration_text(conn: &mut PgConnection) -> String {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT version || '|' || checksum || '|' || phase || '|' || filename || '|' || applied_at::text || '|' || applied_by \
         FROM _oo.schema_migrations ORDER BY version",
    )
    .fetch_all(&mut *conn)
    .await
    .expect("migration rows");
    let mut text = String::new();
    for (row,) in rows {
        text.push_str(&row);
        text.push('\n');
    }
    text
}

async fn count_version(conn: &mut PgConnection, version: &str) -> i64 {
    let sql = format!(
        "SELECT count(*) FROM _oo.schema_migrations WHERE version = '{version}'"
    );
    assert!(version.chars().all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '-'));
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(&mut *conn)
        .await
        .expect("count")
}

async fn roles(conn: &mut PgConnection) -> String {
    scalar(
        conn,
        "SELECT string_agg(rolname || '|' || rolcanlogin::text || '|' || rolsuper::text || '|' || rolcreatedb::text || '|' || rolcreaterole::text || '|' || rolinherit::text || '|' || rolbypassrls::text || '|' || rolreplication::text, E'\\n' ORDER BY rolname) || E'\\n' FROM pg_catalog.pg_roles WHERE rolname IN ('oo_target_owner', 'oo_pipeline_runtime')",
    )
    .await
}

async fn logged(conn: &mut PgConnection) -> String {
    scalar(
        conn,
        "SELECT string_agg(relation.relname || '=' || relation.relpersistence::text, E'\\n' ORDER BY relation.relname) || E'\\n' FROM pg_catalog.pg_class AS relation JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace WHERE namespace.nspname = '_oo' AND relation.relname IN ('schema_migrations', 'migration_lock', 'target_generations', 'apply_receipts', 'physical_mappings')",
    )
    .await
}

async fn logged_full(conn: &mut PgConnection) -> String {
    scalar(
        conn,
        "SELECT string_agg(relation.relname || '=' || relation.relpersistence::text, E'\\n' ORDER BY relation.relname) || E'\\n' FROM pg_catalog.pg_class AS relation JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace WHERE namespace.nspname = '_oo' AND relation.relname IN ('full_import_session', 'full_import_run', 'full_import_table', 'full_import_quality')",
    )
    .await
}

async fn functions(conn: &mut PgConnection) -> String {
    scalar(
        conn,
        "SELECT string_agg(proc.proname, ',' ORDER BY proc.proname) FROM pg_catalog.pg_proc AS proc JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = proc.pronamespace WHERE namespace.nspname = '_oo' AND proc.proname IN ('adopt_target_generation', 'commit_apply_receipt', 'schema_ready', 'segment_preflight_ready', 'register_physical_mapping', 'lookup_physical_mapping')",
    )
    .await
}

async fn scalar(conn: &mut PgConnection, sql: &str) -> String {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(conn)
        .await
        .expect("scalar")
}

async fn raw(conn: &mut PgConnection, sql: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("sql failed: {err}"));
}

async fn exec(conn: &mut PgConnection, prefix: &str, secret: &str, suffix: &str) {
    assert!(secret.chars().all(|c| c.is_ascii_hexdigit()));
    let sql = format!("{prefix}{secret}{suffix}");
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *conn)
        .await
        .expect("role");
}

async fn script(conn: &mut PgConnection, sql: &'static str) {
    sqlx::raw_sql(sql)
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("script failed: {err}"));
}

async fn connect(host: &str, port: u16, user: &str, password: &str, database: &str) -> PgConnection {
    let options = PgConnectOptions::new()
        .host(host)
        .port(port)
        .username(user)
        .password(password)
        .database(database)
        .ssl_mode(PgSslMode::Disable);
    PgConnection::connect_with(&options)
        .await
        .expect("connect")
}

fn read_random(len: usize) -> Vec<u8> {
    let mut bytes = vec![0_u8; len];
    let mut file = fs::File::open("/dev/urandom").expect("urandom");
    std::io::Read::read_exact(&mut file, &mut bytes).expect("urandom read");
    bytes
}

fn hex_bytes(bytes: &[u8]) -> String {
    sha256_hex(bytes)[..bytes.len().saturating_mul(2).min(64)].to_string()
}

const RECEIPT_SQL: &str = "SELECT coalesce(string_agg(source || '|' || txn_or_batch_id || '|' || payload_digest || '|' || target_commit_at::text || '|' || encode(segment_bitmap, 'hex') || '|' || coalesce(segment_index::text, 'null') || '|' || coalesce(segment_count::text, 'null'), E'\\n' ORDER BY txn_or_batch_id), '') FROM _oo.apply_receipts";

const LEGAL_RECEIPT: &str = "\
BEGIN;\n\
SET LOCAL ROLE oo_target_owner;\n\
SELECT pg_catalog.set_config('_oo.writer', 'apply-receipt', true);\n\
INSERT INTO _oo.apply_receipts (\n\
  source, table_generation, txn_or_batch_id,\n\
  config_version, config_digest, type_policy_version, type_digest,\n\
  hook_digest, schema_digest, payload_digest,\n\
  protocol_version, position_codec, position_lineage, source_epoch, applied_pos,\n\
  partial_txn, cross_table_consistent, txn_complete,\n\
  segment_index, segment_count, segment_bitmap,\n\
  fence_shard, fence_generation\n\
) VALUES (\n\
  'warehouse', 'orders_g1', 'kept-batch',\n\
  'config/v1', repeat('a', 64), 'type-policy/v1', repeat('b', 64),\n\
  repeat('c', 64), repeat('d', 64), repeat('e', 64),\n\
  'batch/v1', 'pg-commit-lsn/v1', 'src_main', 1, 0,\n\
  false, true, true,\n\
  NULL, NULL, ''::bytea,\n\
  'shard_keep', 1\n\
);\n\
COMMIT;";

const ILLEGAL_RECEIPT: &str = "\
BEGIN;\n\
SET LOCAL ROLE oo_target_owner;\n\
SELECT pg_catalog.set_config('_oo.writer', 'apply-receipt', true);\n\
INSERT INTO _oo.apply_receipts (\n\
  source, table_generation, txn_or_batch_id,\n\
  config_version, config_digest, type_policy_version, type_digest,\n\
  hook_digest, schema_digest, payload_digest,\n\
  protocol_version, position_codec, position_lineage, source_epoch, applied_pos,\n\
  partial_txn, cross_table_consistent, txn_complete,\n\
  segment_index, segment_count, segment_bitmap,\n\
  fence_shard, fence_generation\n\
) VALUES (\n\
  'warehouse', 'orders_g1', 'bad-batch',\n\
  'config/v1', repeat('a', 64), 'type-policy/v1', repeat('b', 64),\n\
  repeat('c', 64), repeat('d', 64), repeat('f', 64),\n\
  'batch/v1', 'pg-commit-lsn/v1', 'src_main', 1, 0,\n\
  false, true, true,\n\
  NULL, 1, '\\x01'::bytea,\n\
  'shard_bad', 1\n\
);\n\
COMMIT;";
