//! F2: `native_source_instance_register` (0024) takes `pg_advisory_xact_lock(integer,
//! integer)`. Bootstrap 0000 grants `oo_target_owner` only the bigint overload, so on a
//! fresh chain the call fails with 42501. Migration 0044 grants the integer overload.
//!
//! The static tests run without a server. The live test is ignored. It needs
//! OO_C1V2_HOST, OO_C1V2_PORT and OO_C1V2_PASSWORD_FILE (superuser `postgres`), and it
//! drops and recreates `oo_cb_t13_grant`. Run with `--ignored --test-threads=1`.

use std::{env, fs};

use oo_pipeline::target_installer::{self as ti, sha256_hex};
use sqlx::Connection;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};

const MIGRATION_0044: &str = include_str!("../migrations/0044_native_source_instance_advisory_grant.sql");
const INSTALLER: &str = include_str!("../src/target_installer.rs");
const NATIVE_UPDATE: &str = include_str!("../src/native_update.rs");
const REGISTER_0024: &str = include_str!("../migrations/0024_native_gtid_transaction.sql");
const DB: &str = "oo_cb_t13_grant";

#[test]
fn register_takes_the_integer_overload_that_0044_grants() {
    assert!(REGISTER_0024.contains("pg_advisory_xact_lock(hashtext('oo-source-instance'), hashtext(p_source))"));
    assert!(MIGRATION_0044.contains(
        "GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_xact_lock(integer, integer) TO oo_target_owner"
    ));
    // Only the grant. No PUBLIC grant, no table grant, no role switch.
    assert!(!MIGRATION_0044.contains("TO PUBLIC"));
    assert!(!MIGRATION_0044.contains("ON TABLE"));
    assert!(!MIGRATION_0044.contains("SET ROLE"));
}

#[test]
fn installer_runs_0044_after_0043_with_its_compiled_digest() {
    assert!(INSTALLER.contains("0044_native_source_instance_advisory_grant.sql"));
    assert_eq!(
        ti::compiled_native_source_instance_grant_digest(),
        sha256_hex(MIGRATION_0044.as_bytes())
    );
    let at_0043 = NATIVE_UPDATE
        .find("install_native_c1_chunked_baseline_final(&mut conn)")
        .expect("0043 step");
    let at_0044 = NATIVE_UPDATE
        .find("install_native_source_instance_grant(&mut conn)")
        .expect("0044 step");
    assert!(at_0044 > at_0043, "0044 must follow 0043 in the install chain");
    assert!(NATIVE_UPDATE.contains("\"step\": \"0044\""));
    assert!(NATIVE_UPDATE.contains("\"native_source_instance_grant_digest\""));
}

fn env_var(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{name} must be set for the live test"))
}

async fn connect(database: &str) -> PgConnection {
    let password = fs::read_to_string(env_var("OO_C1V2_PASSWORD_FILE")).expect("password file");
    let options = PgConnectOptions::new()
        .host(&env_var("OO_C1V2_HOST"))
        .port(env_var("OO_C1V2_PORT").parse().expect("port"))
        .username("postgres")
        .password(password.trim())
        .database(database)
        .ssl_mode(PgSslMode::Disable);
    PgConnection::connect_with(&options).await.expect("connect")
}

async fn flag(conn: &mut PgConnection) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT has_function_privilege('oo_target_owner', 'pg_catalog.pg_advisory_xact_lock(integer, integer)', 'EXECUTE')",
    )
    .fetch_one(&mut *conn)
    .await
    .expect("privilege")
}

#[tokio::test]
#[ignore]
async fn fresh_bootstrap_lacks_the_integer_overload_and_0044_grants_it() {
    let mut admin = connect("postgres").await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP DATABASE IF EXISTS {DB} WITH (FORCE)")))
        .execute(&mut admin)
        .await
        .expect("drop");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE DATABASE {DB}")))
        .execute(&mut admin)
        .await
        .expect("create");
    let mut db = connect(DB).await;
    // 0000-0004: bootstrap roles and the fence. The bigint overload is granted, the integer one is not.
    ti::install(&mut db).await.expect("bootstrap chain");
    assert!(!flag(&mut db).await, "F2: the integer overload is not granted by 0000");
    sqlx::raw_sql(sqlx::AssertSqlSafe(MIGRATION_0044.to_owned()))
        .execute(&mut db)
        .await
        .expect("0044");
    assert!(flag(&mut db).await, "0044 grants the integer overload");
    // The owner can take the lock. This is the call 0024 makes inside its definer function.
    let mut tx = db.begin().await.expect("begin");
    sqlx::raw_sql("SET LOCAL ROLE oo_target_owner")
        .execute(&mut *tx)
        .await
        .expect("set role");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('oo-source-instance'), hashtext('s1'))")
        .execute(&mut *tx)
        .await
        .expect("owner takes the integer lock");
    tx.rollback().await.expect("rollback");
    // Replaying the grant is harmless.
    sqlx::raw_sql(sqlx::AssertSqlSafe(MIGRATION_0044.to_owned()))
        .execute(&mut db)
        .await
        .expect("0044 replay");
    // The bootstrap roles are cluster-wide. Drop this database, so that the next live test
    // can drop and recreate them.
    drop(db);
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP DATABASE {DB} WITH (FORCE)")))
        .execute(&mut admin)
        .await
        .expect("cleanup");
}
