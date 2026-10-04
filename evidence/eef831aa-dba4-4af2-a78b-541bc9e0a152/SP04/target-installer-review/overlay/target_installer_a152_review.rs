//! Independent oracle for `oo_pipeline::target_installer`.
//!
//! The product entries are `install` and `verify_readiness` on one
//! caller-owned `PgConnection`. Fixture SQL builds a known pre-state.
//! It is not a reference `psql` applicator, not an arbitrary SQL adapter,
//! and not the Loader. No statement copies user rows or writes `public`.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use oo_pipeline::target_installer::{
    compiled_digests, install, sha256_hex, verify_readiness, InstallError, InstallOutcome,
    ReadinessError, HISTORICAL_V2_SHA256, MIGRATION_LOCK_KEY, VERSION_BOOTSTRAP, VERSION_FENCE,
    VERSION_MAPPING, VERSION_PREFLIGHT, VERSION_SEGMENT_V2,
};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::Connection;

const BOOTSTRAP: &str = include_str!("../migrations/0000_bootstrap_roles.sql");
const FENCE: &str = include_str!("../migrations/0001_target_fence_expand.sql");
const SEGMENT: &str = include_str!("../migrations/0002_target_segment_null_expand.sql");
const PREFLIGHT: &str = include_str!("../migrations/0003_target_segment_preflight_expand.sql");
const MAPPING: &str = include_str!("../migrations/0004_physical_mapping.sql");
const INSTALLER: &str = include_str!("../src/target_installer.rs");

const PRIOR_BOOTSTRAP: &str = "37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208";
const PRIOR_FENCE: &str = "97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9";
const PRIOR_PREFLIGHT: &str = "d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3";
const PRIOR_MAPPING: &str = "844de6f645f8f1ebf29d05118ae25d4669d8dc870eed1fa7697d4b98456eaf95";
const LIE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

#[test]
fn embedded_bytes_match_previous_independent_evidence() {
    assert_eq!(MIGRATION_LOCK_KEY, 6_404_202_605);
    assert_eq!(sha256_hex(BOOTSTRAP.as_bytes()), PRIOR_BOOTSTRAP);
    assert_eq!(sha256_hex(FENCE.as_bytes()), PRIOR_FENCE);
    assert_eq!(sha256_hex(SEGMENT.as_bytes()), HISTORICAL_V2_SHA256);
    assert_eq!(sha256_hex(PREFLIGHT.as_bytes()), PRIOR_PREFLIGHT);
    assert_eq!(sha256_hex(MAPPING.as_bytes()), PRIOR_MAPPING);
    let digests = compiled_digests();
    assert_eq!(digests.bootstrap(), PRIOR_BOOTSTRAP);
    assert_eq!(digests.fence(), PRIOR_FENCE);
    assert_eq!(digests.preflight(), PRIOR_PREFLIGHT);
    assert_eq!(digests.mapping(), PRIOR_MAPPING);
    assert!(INSTALLER.contains("include_str!(\"../migrations/0000_bootstrap_roles.sql\")"));
    assert!(INSTALLER.contains("include_str!(\"../migrations/0001_target_fence_expand.sql\")"));
    assert!(INSTALLER.contains("include_str!(\"../migrations/0003_target_segment_preflight_expand.sql\")"));
    assert!(INSTALLER.contains("include_str!(\"../migrations/0004_physical_mapping.sql\")"));
    assert!(!INSTALLER.contains("include_str!(\"../migrations/0002_target_segment_null_expand.sql\")"));
    assert!(INSTALLER.contains("SELECT pg_catalog.pg_advisory_xact_lock(6404202605)"));
    assert!(INSTALLER.contains("pub async fn install(conn: &mut PgConnection)"));
    assert!(INSTALLER.contains("pub async fn verify_readiness(\n    conn: &mut PgConnection,"));
    assert!(!INSTALLER.contains("PgPool"));
    assert!(!MAPPING.contains("COPY "));
    assert!(!BOOTSTRAP.contains("GRANT oo_target_owner TO oo_pipeline_runtime"));
}

#[tokio::test]
#[ignore = "one private PostgreSQL 17.10; OO_A152_HOST is set only by the a152 probe"]
async fn production_api_on_one_postgres() {
    let host = env::var("OO_A152_HOST").expect("OO_A152_HOST");
    let port: u16 = env::var("OO_A152_PORT").expect("OO_A152_PORT").parse().expect("port");
    let user = env::var("OO_A152_USER").expect("OO_A152_USER");
    let password = fs::read_to_string(env::var("OO_A152_PASSWORD_FILE").expect("OO_A152_PASSWORD_FILE"))
        .expect("password file")
        .trim()
        .to_string();
    let evidence = PathBuf::from(env::var("OO_A152_EVIDENCE").expect("OO_A152_EVIDENCE"));
    fs::create_dir_all(&evidence).expect("evidence dir");
    let digests = compiled_digests();
    let mut report = String::new();
    let mut admin = connect(&host, port, &user, &password, "postgres").await;
    let version = scalar(&mut admin, "SELECT version()").await;
    assert!(version.contains("17.10"), "{version}");
    note(&evidence, &mut report, &format!("server={version}"));

    reset(&mut admin).await;
    let mut empty = created(&host, port, &user, &password, &mut admin, "oo_a152_empty").await;
    match verify_readiness(&mut empty).await {
        Err(ReadinessError::NotInstalled) => {}
        other => panic!("empty before install: {other:?}"),
    }

    let mut holder = connect(&host, port, &user, &password, "oo_a152_empty").await;
    let mut held = holder.begin().await.expect("hold begin");
    script(&mut held, "SELECT pg_catalog.pg_advisory_xact_lock(6404202605)").await;
    let lock_row = scalar(
        &mut held,
        "SELECT classid::text || '|' || objid::text FROM pg_catalog.pg_locks WHERE locktype = 'advisory' AND granted AND pid = pg_catalog.pg_backend_pid()",
    )
    .await;
    assert_eq!(lock_row, "1|2109235309", "{lock_row}");
    note(&evidence, &mut report, "lock_held=1|2109235309");
    let started = Instant::now();
    let blocked = install(&mut empty).await;
    let waited = started.elapsed();
    match blocked {
        Err(err) => {
            assert_eq!(
                err.sqlstate(),
                Some("55P03"),
                "{err} after {} ms",
                waited.as_millis()
            );
            assert!(
                waited >= Duration::from_secs(10),
                "lock wait was only {}ms",
                waited.as_millis()
            );
        }
        Ok(outcome) => panic!("install ignored the held migration lock: {outcome}"),
    }
    held.rollback().await.expect("release lock");
    drop(holder);
    drop(empty);
    let mut empty = connect(&host, port, &user, &password, "oo_a152_empty").await;
    match verify_readiness(&mut empty).await {
        Err(ReadinessError::NotInstalled) => {}
        other => panic!("readiness after lock timeout: {other:?}"),
    }
    assert_eq!(schema_count(&mut empty).await, 0);
    note(&evidence, &mut report, "lock=55P03");
    note(&evidence, &mut report, "lock_ready=not_installed");

    let first = install(&mut empty).await.expect("fresh install");
    assert_eq!(first, InstallOutcome::Bootstrapped);
    let fresh_rows = migration_text(&mut empty).await;
    assert_installed(&fresh_rows, false);
    assert_eq!(count_version(&mut empty, VERSION_SEGMENT_V2).await, 0);
    assert_eq!(scalar(&mut empty, "SELECT count(*)::text FROM pg_catalog.pg_publication").await, "0");
    assert_eq!(scalar(&mut empty, "SELECT count(*)::text FROM _oo.apply_receipts").await, "0");
    assert_eq!(roles(&mut empty).await, expected_roles());
    let ready = verify_readiness(&mut empty).await.expect("fresh readiness");
    assert_eq!(ready.digests().bootstrap(), digests.bootstrap());
    assert_eq!(ready.digests().fence(), digests.fence());
    assert_eq!(ready.digests().preflight(), digests.preflight());
    assert_eq!(ready.digests().mapping(), digests.mapping());
    assert!(!ready.retains_historical_v2());
    tokio::time::sleep(Duration::from_millis(50)).await;
    let replay = install(&mut empty).await.expect("replay");
    assert_eq!(replay, InstallOutcome::Replay);
    assert_eq!(migration_text(&mut empty).await, fresh_rows);
    note(&evidence, &mut report, "empty=bootstrapped");
    note(&evidence, &mut report, "empty_replay=replay");
    note(&evidence, &mut report, "empty_applied_at_same=1");
    note(&evidence, &mut report, "empty_v2_rows=0");
    note(&evidence, &mut report, "empty_publication=0");
    note(&evidence, &mut report, "empty_receipts=0");

    let runtime_password = hex_bytes(&read_random(16));
    exec(
        &mut empty,
        "CREATE ROLE oo_a152_runtime LOGIN PASSWORD '",
        &runtime_password,
        "' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION",
    )
    .await;
    raw(&mut empty, "GRANT oo_pipeline_runtime TO oo_a152_runtime").await;
    let mut runtime = connect(&host, port, "oo_a152_runtime", &runtime_password, "oo_a152_empty").await;
    let runtime_ready = verify_readiness(&mut runtime).await.expect("runtime readiness");
    assert_eq!(runtime_ready.digests().mapping(), digests.mapping());
    assert!(!runtime_ready.retains_historical_v2());
    match install(&mut runtime).await {
        Err(InstallError::NotAdministrator) => {}
        other => panic!("runtime install: {other:?}"),
    }
    drop(runtime);
    assert_eq!(migration_text(&mut empty).await, fresh_rows);
    note(&evidence, &mut report, "runtime_verify=ok");
    note(&evidence, &mut report, "runtime_install=not_administrator");

    let mut second = created(&host, port, &user, &password, &mut admin, "oo_a152_second").await;
    let second_result = install(&mut second).await;
    match second_result {
        Err(err) => {
            assert_eq!(err.sqlstate(), Some("OO002"), "{err}");
            assert!(err.to_string().contains("segment relation is absent"), "{err}");
        }
        Ok(outcome) => panic!("second database installed despite existing roles: {outcome}"),
    }
    match verify_readiness(&mut second).await {
        Err(ReadinessError::NotInstalled) => {}
        other => panic!("second readiness: {other:?}"),
    }
    assert_eq!(schema_count(&mut second).await, 0);
    assert_eq!(migration_text(&mut empty).await, fresh_rows);
    let role_count = scalar(
        &mut admin,
        "SELECT count(*)::text FROM pg_catalog.pg_roles WHERE rolname IN ('oo_target_owner', 'oo_pipeline_runtime')",
    )
    .await;
    assert_eq!(role_count, "2");
    note(&evidence, &mut report, "second=OO002");
    note(&evidence, &mut report, "second_ready=not_installed");
    note(&evidence, &mut report, "second_schema=0");
    note(&evidence, &mut report, "first_unchanged=1");
    drop(second);
    drop(empty);

    reset(&mut admin).await;
    let mut v1 = created(&host, port, &user, &password, &mut admin, "oo_a152_v1").await;
    harness_v1(&mut v1, &digests).await;
    let v1_before = migration_text(&mut v1).await;
    assert_eq!(count_version(&mut v1, VERSION_PREFLIGHT).await, 0);
    assert_eq!(count_version(&mut v1, VERSION_SEGMENT_V2).await, 0);
    let upgraded = install(&mut v1).await.expect("v1 upgrade");
    assert_eq!(upgraded, InstallOutcome::UpgradedFromV1);
    let v1_after = migration_text(&mut v1).await;
    assert_eq!(applied_at_of(&v1_before, VERSION_BOOTSTRAP), applied_at_of(&v1_after, VERSION_BOOTSTRAP));
    assert_eq!(applied_at_of(&v1_before, VERSION_FENCE), applied_at_of(&v1_after, VERSION_FENCE));
    assert_eq!(count_version(&mut v1, VERSION_SEGMENT_V2).await, 0);
    assert_installed(&v1_after, false);
    let v1_ready = verify_readiness(&mut v1).await.expect("v1 readiness");
    assert!(!v1_ready.retains_historical_v2());
    note(&evidence, &mut report, "v1=upgraded_from_v1");
    note(&evidence, &mut report, "v1_v2_rows=0");
    note(&evidence, &mut report, "v1_applied_at_same=1");
    drop(v1);

    reset(&mut admin).await;
    let mut v2 = created(&host, port, &user, &password, &mut admin, "oo_a152_v2").await;
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
    assert_installed(&v2_after, true);
    let v2_ready = verify_readiness(&mut v2).await.expect("v2 readiness");
    assert!(v2_ready.retains_historical_v2());
    note(&evidence, &mut report, "v2=upgraded_from_v2");
    note(&evidence, &mut report, "v2_history_same=1");
    note(&evidence, &mut report, "v2_receipt_same=1");
    drop(v2);

    reset(&mut admin).await;
    let mut mapped = created(&host, port, &user, &password, &mut admin, "oo_a152_map").await;
    harness_v3_without_mapping(&mut mapped, &digests).await;
    assert_eq!(count_version(&mut mapped, VERSION_MAPPING).await, 0);
    assert_eq!(count_version(&mut mapped, VERSION_SEGMENT_V2).await, 0);
    assert_eq!(
        scalar(
            &mut mapped,
            "SELECT (pg_catalog.to_regclass('_oo.physical_mappings') IS NULL)::text",
        )
        .await,
        "true"
    );
    let map_before = migration_text(&mut mapped).await;
    assert_eq!(count_version(&mut mapped, VERSION_PREFLIGHT).await, 1);
    let mapping_only = install(&mut mapped).await.expect("mapping install");
    assert_eq!(mapping_only, InstallOutcome::InstalledMapping);
    let map_after = migration_text(&mut mapped).await;
    assert_eq!(applied_at_of(&map_before, VERSION_BOOTSTRAP), applied_at_of(&map_after, VERSION_BOOTSTRAP));
    assert_eq!(applied_at_of(&map_before, VERSION_FENCE), applied_at_of(&map_after, VERSION_FENCE));
    assert_eq!(applied_at_of(&map_before, VERSION_PREFLIGHT), applied_at_of(&map_after, VERSION_PREFLIGHT));
    assert_eq!(checksum_of(&map_before, VERSION_PREFLIGHT), digests.preflight());
    assert_eq!(count_version(&mut mapped, VERSION_SEGMENT_V2).await, 0);
    assert_eq!(count_version(&mut mapped, VERSION_MAPPING).await, 1);
    assert_eq!(checksum_of(&map_after, VERSION_MAPPING), digests.mapping());
    assert_eq!(
        scalar(
            &mut mapped,
            "SELECT relation.relpersistence::text FROM pg_catalog.pg_class AS relation JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace WHERE namespace.nspname = '_oo' AND relation.relname = 'physical_mappings'",
        )
        .await,
        "p"
    );
    assert_eq!(scalar(&mut mapped, "SELECT count(*)::text FROM _oo.apply_receipts").await, "0");
    assert_eq!(scalar(&mut mapped, "SELECT count(*)::text FROM pg_catalog.pg_publication").await, "0");
    let map_ready = verify_readiness(&mut mapped).await.expect("mapping readiness");
    assert!(!map_ready.retains_historical_v2());
    assert_eq!(map_ready.digests().mapping(), digests.mapping());
    tokio::time::sleep(Duration::from_millis(50)).await;
    let map_replay = install(&mut mapped).await.expect("mapping replay");
    assert_eq!(map_replay, InstallOutcome::Replay);
    assert_eq!(migration_text(&mut mapped).await, map_after);
    note(&evidence, &mut report, "map=installed_mapping");
    note(&evidence, &mut report, "map_prior_applied_at_same=1");
    note(&evidence, &mut report, "map_v2_rows=0");
    note(&evidence, &mut report, "map_mapping_rows=1");
    note(&evidence, &mut report, "map_objects=1");
    note(&evidence, &mut report, "map_replay=replay");
    note(&evidence, &mut report, "map_replay_applied_at_same=1");
    note(&evidence, &mut report, "map_ready=1");
    drop(mapped);

    reset(&mut admin).await;
    let mut missing = created(&host, port, &user, &password, &mut admin, "oo_a152_miss").await;
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
    note(&evidence, &mut report, "oo002=OO002");
    note(&evidence, &mut report, "oo002_unchanged=1");
    note(&evidence, &mut report, "oo002_ready=incomplete");
    drop(missing);

    reset(&mut admin).await;
    let mut bad = created(&host, port, &user, &password, &mut admin, "oo_a152_bad").await;
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
    note(&evidence, &mut report, "oo001=OO001");
    note(&evidence, &mut report, "oo001_unchanged=1");
    note(&evidence, &mut report, "oo001_ready=checksum_mismatch");
    drop(bad);

    reset(&mut admin).await;
    let mut failed = created(&host, port, &user, &password, &mut admin, "oo_a152_fail").await;
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
    note(&evidence, &mut report, "fail=OO021");
    note(&evidence, &mut report, "fail_unchanged=1");
    note(&evidence, &mut report, "fail_ready=incomplete");
    drop(failed);
    reset(&mut admin).await;
    note(&evidence, &mut report, "digests_match_files=1");
    note(&evidence, &mut report, "installer_review_ok");
}

fn note(evidence: &std::path::Path, report: &mut String, line: &str) {
    report.push_str(line);
    report.push('\n');
    println!("{line}");
    fs::write(evidence.join("outcome.txt"), report.as_bytes()).expect("outcome");
}

fn expected_roles() -> String {
    "oo_pipeline_runtime|false|false|false|false|false|false|false\noo_target_owner|false|false|false|false|false|false|false\n"
        .to_string()
}

fn assert_installed(rows: &str, historical_v2: bool) {
    let digests = compiled_digests();
    assert_eq!(checksum_of(rows, VERSION_BOOTSTRAP), digests.bootstrap());
    assert_eq!(checksum_of(rows, VERSION_FENCE), digests.fence());
    assert_eq!(checksum_of(rows, VERSION_PREFLIGHT), digests.preflight());
    assert_eq!(checksum_of(rows, VERSION_MAPPING), digests.mapping());
    assert!(rows.contains("expand|0000_bootstrap_roles.sql|"));
    assert!(rows.contains("expand|0004_physical_mapping.sql|"));
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
        .unwrap_or_else(|| panic!("missing {version}"));
    line.split('|')
        .nth(index)
        .unwrap_or_else(|| panic!("field {index}"))
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
    assert_name(name);
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
        "oo_a152_empty",
        "oo_a152_second",
        "oo_a152_v1",
        "oo_a152_v2",
        "oo_a152_map",
        "oo_a152_miss",
        "oo_a152_bad",
        "oo_a152_fail",
    ] {
        raw(admin, &format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await;
    }
    raw(admin, "DROP ROLE IF EXISTS oo_a152_runtime").await;
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
    exec_owned(&mut tx, &sql).await;
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
    exec_owned(&mut tx, &sql).await;
    tx.commit().await.expect("v2 commit");
}

async fn harness_v3_without_mapping(
    conn: &mut PgConnection,
    digests: &oo_pipeline::target_installer::CompiledDigests,
) {
    harness_v1(conn, digests).await;
    let mut tx = conn.begin().await.expect("v3 begin");
    script(&mut tx, "SELECT pg_catalog.pg_advisory_xact_lock(6404202605)").await;
    script(&mut tx, "SET LOCAL ROLE oo_target_owner").await;
    script(&mut tx, "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE").await;
    script(&mut tx, PREFLIGHT).await;
    let sql = format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES \
         ('{VERSION_PREFLIGHT}', '{}', 'expand', '0003_target_segment_preflight_expand.sql')",
        digests.preflight()
    );
    exec_owned(&mut tx, &sql).await;
    tx.commit().await.expect("v3 commit");
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
    assert!(version.chars().all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '-'));
    let sql = format!("SELECT count(*) FROM _oo.schema_migrations WHERE version = '{version}'");
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(&mut *conn)
        .await
        .expect("count")
}

async fn schema_count(conn: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM pg_catalog.pg_namespace WHERE nspname = '_oo'")
        .fetch_one(&mut *conn)
        .await
        .expect("schema count")
}

async fn roles(conn: &mut PgConnection) -> String {
    scalar(
        conn,
        "SELECT string_agg(rolname || '|' || rolcanlogin::text || '|' || rolsuper::text || '|' || rolcreatedb::text || '|' || rolcreaterole::text || '|' || rolinherit::text || '|' || rolbypassrls::text || '|' || rolreplication::text, E'\\n' ORDER BY rolname) || E'\\n' FROM pg_catalog.pg_roles WHERE rolname IN ('oo_target_owner', 'oo_pipeline_runtime')",
    )
    .await
}

async fn scalar(conn: &mut PgConnection, sql: &str) -> String {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .expect("scalar")
}

async fn raw(conn: &mut PgConnection, sql: &str) {
    exec_owned(conn, sql).await;
}

async fn exec_owned(conn: &mut PgConnection, sql: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("sql failed: {err}"));
}

async fn exec(conn: &mut PgConnection, prefix: &str, secret: &str, suffix: &str) {
    assert!(secret.chars().all(|c| c.is_ascii_hexdigit()));
    let sql = format!("{prefix}{secret}{suffix}");
    exec_owned(conn, &sql).await;
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
    PgConnection::connect_with(&options).await.expect("connect")
}

fn assert_name(name: &str) {
    assert!(name.starts_with("oo_a152_"));
    assert!(name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
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
