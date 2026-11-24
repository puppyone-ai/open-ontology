//! Shared helpers for the container tests. A throwaway PostgreSQL 17 must be
//! listening; point the tests at it with `OO_DOCS_TEST_PG_PORT` (and
//! optionally `OO_DOCS_TEST_PG_HOST`, `OO_DOCS_TEST_PG_USER`; the container
//! uses trust authentication, so there is no password).
#![allow(dead_code)]

use std::env;

pub mod engine;
pub mod fx;
pub mod http;

use oo_docs::store::install::{self, InstallOutcome};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions, PgSslMode};
use sqlx::{Connection, Executor};

/// Roles are cluster wide and `CREATE ROLE` inside `0137` is not serialized
/// across databases, so every fresh install in a test binary takes this.
pub static CLUSTER_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub const RUNTIME_LOGIN: &str = "oo_docs_it_runtime_login";

fn options(db: &str, user: Option<&str>) -> PgConnectOptions {
    let host = env::var("OO_DOCS_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = env::var("OO_DOCS_TEST_PG_PORT")
        .expect("OO_DOCS_TEST_PG_PORT")
        .parse()
        .expect("port");
    let admin = env::var("OO_DOCS_TEST_PG_USER").unwrap_or_else(|_| "postgres".into());
    let mut o = PgConnectOptions::new();
    if let Ok(pw) = env::var("OO_DOCS_TEST_PG_PASSWORD") {
        o = o.password(&pw);
    }
    o.host(&host)
        .port(port)
        .username(user.unwrap_or(&admin))
        .database(db)
        .ssl_mode(PgSslMode::Disable)
}

pub async fn connect(db: &str) -> PgConnection {
    PgConnection::connect_with(&options(db, None))
        .await
        .expect("admin connect")
}

pub async fn connect_runtime(db: &str) -> PgConnection {
    PgConnection::connect_with(&options(db, Some(RUNTIME_LOGIN)))
        .await
        .expect("runtime connect")
}

pub async fn pool_runtime(db: &str, max: u32) -> PgPool {
    PgPoolOptions::new()
        .max_connections(max)
        .connect_with(options(db, Some(RUNTIME_LOGIN)))
        .await
        .expect("runtime pool")
}

pub async fn pool_admin(db: &str, max: u32) -> PgPool {
    PgPoolOptions::new()
        .max_connections(max)
        .connect_with(options(db, None))
        .await
        .expect("admin pool")
}

/// Create an empty database named `name` (dropped first if present).
pub async fn create_db(name: &str) {
    let mut admin = connect("postgres").await;
    admin
        .execute(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS {name} WITH (FORCE)"
        )))
        .await
        .expect("drop");
    admin
        .execute(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .await
        .expect("create");
}

pub async fn drop_db(name: &str) {
    let mut admin = connect("postgres").await;
    let _ = admin
        .execute(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS {name} WITH (FORCE)"
        )))
        .await;
}

/// Empty database, installed, with a login member of `oo_docs_runtime`.
pub async fn installed_db(name: &str) {
    create_db(name).await;
    let mut admin = connect(name).await;
    let outcome = install::install(&mut admin).await.expect("install");
    assert!(matches!(outcome, InstallOutcome::Applied(_)), "{outcome:?}");
    admin
        .execute(sqlx::AssertSqlSafe(format!(
            "DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{RUNTIME_LOGIN}') \
             THEN CREATE ROLE {RUNTIME_LOGIN} LOGIN IN ROLE oo_docs_runtime; END IF; END $$"
        )))
        .await
        .expect("login role");
}

/// SQLSTATE of a failed statement.
pub fn sqlstate<T: std::fmt::Debug>(r: Result<T, sqlx::Error>) -> String {
    match r {
        Ok(v) => panic!("expected an error, got {v:?}"),
        Err(e) => e
            .as_database_error()
            .and_then(|d| d.code().map(|c| c.into_owned()))
            .unwrap_or_else(|| format!("no sqlstate: {e}")),
    }
}
