use tokio_postgres::Client;

use crate::api::{ADAPT_SHA, BOOT_SHA, CONTROL_SHA, DATABASE, EVENT_SHA, LEASE_SHA, RUNTIME_LOGIN};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::sha256::sha256_hex;
use crate::tls;

const ADAPT_SQL: &str = include_str!("../adapt.sql");

pub async fn reset(config: &Config) -> Result<()> {
    let client = admin(config, "postgres").await?;
    client
        .batch_execute(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = 'oo_a390_control' AND pid <> pg_backend_pid()",
        )
        .await
        .map_err(Error::sql)?;
    for statement in [
        "DROP DATABASE IF EXISTS oo_a390_control",
        "DROP ROLE IF EXISTS oo_a390_runtime_login",
        "DROP ROLE IF EXISTS oo_jobs_runtime",
        "DROP ROLE IF EXISTS oo_jobs_owner",
    ] {
        client.batch_execute(statement).await.map_err(Error::sql)?;
    }
    Ok(())
}

pub async fn apply(config: &Config) -> Result<()> {
    if sha256_hex(ADAPT_SQL.as_bytes()) != ADAPT_SHA {
        return Err(Error::new("hash"));
    }
    let bootstrap = read_sql(&config.migrations, "0000_bootstrap_roles.sql", BOOT_SHA)?;
    let control = read_sql(&config.migrations, "0001_jobs_control_expand.sql", CONTROL_SHA)?;
    let event = read_sql(
        &config.migrations,
        "0002_event_project_binding_expand.sql",
        EVENT_SHA,
    )?;
    let lease = read_sql(
        &config.migrations,
        "0003_job_lease_operations_expand.sql",
        LEASE_SHA,
    )?;
    {
        let client = admin(config, "postgres").await?;
        let exists: i64 = client
            .query_one(
                "SELECT count(*)::bigint FROM pg_database WHERE datname = $1",
                &[&DATABASE],
            )
            .await
            .map_err(Error::sql)?
            .get(0);
        if exists == 0 {
            client
                .batch_execute(
                    "CREATE DATABASE oo_a390_control ENCODING 'UTF8' TEMPLATE template0",
                )
                .await
                .map_err(Error::sql)?;
        }
    }
    let mut client = admin(config, DATABASE).await?;
    install_migrations(&mut client, &bootstrap, &control, &event, &lease).await?;
    install_adapt(&mut client).await?;
    install_login(&client, &config.runtime_password).await?;
    require_ready(&client).await?;
    Ok(())
}

async fn install_migrations(
    client: &mut Client,
    bootstrap: &str,
    control: &str,
    event: &str,
    lease: &str,
) -> Result<()> {
    let tx = client.transaction().await.map_err(Error::sql)?;
    tx.batch_execute("SET LOCAL lock_timeout = '15s'")
        .await
        .map_err(Error::sql)?;
    tx.batch_execute("SET LOCAL statement_timeout = '120s'")
        .await
        .map_err(Error::sql)?;
    tx.batch_execute("SELECT pg_advisory_xact_lock(6401202601)")
        .await
        .map_err(Error::sql)?;
    let present: Option<String> = tx
        .query_one("SELECT to_regclass('jobs.schema_migrations')::text", &[])
        .await
        .map_err(Error::sql)?
        .get(0);
    if present.is_some() {
        let rows = tx
            .query(
                "SELECT version, checksum FROM jobs.schema_migrations ORDER BY version",
                &[],
            )
            .await
            .map_err(Error::sql)?;
        let mut seen = std::collections::BTreeMap::new();
        for row in rows {
            let version: String = row.try_get(0).map_err(Error::sql)?;
            let checksum: String = row.try_get(1).map_err(Error::sql)?;
            seen.insert(version, checksum);
        }
        let expected = [
            ("jobs-bootstrap/1", BOOT_SHA),
            ("jobs-control/1", CONTROL_SHA),
            ("jobs-event-project/1", EVENT_SHA),
            ("jobs-lease-operations/1", LEASE_SHA),
        ];
        if expected
            .iter()
            .all(|(version, sha)| seen.get(*version).map(String::as_str) == Some(*sha))
        {
            tx.commit().await.map_err(Error::sql)?;
            return Ok(());
        }
        return Err(Error::new("hash"));
    }
    tx.batch_execute(bootstrap).await.map_err(Error::sql)?;
    tx.batch_execute("SET LOCAL ROLE oo_jobs_owner")
        .await
        .map_err(Error::sql)?;
    tx.batch_execute(control).await.map_err(Error::sql)?;
    tx.batch_execute(event).await.map_err(Error::sql)?;
    tx.batch_execute(lease).await.map_err(Error::sql)?;
    tx.execute(
        "INSERT INTO jobs.schema_migrations (version, checksum, phase, filename) VALUES
         ('jobs-bootstrap/1', $1, 'expand', '0000_bootstrap_roles.sql'),
         ('jobs-control/1', $2, 'expand', '0001_jobs_control_expand.sql'),
         ('jobs-event-project/1', $3, 'expand', '0002_event_project_binding_expand.sql'),
         ('jobs-lease-operations/1', $4, 'expand', '0003_job_lease_operations_expand.sql')",
        &[&BOOT_SHA, &CONTROL_SHA, &EVENT_SHA, &LEASE_SHA],
    )
    .await
    .map_err(Error::sql)?;
    tx.commit().await.map_err(Error::sql)?;
    Ok(())
}

async fn install_adapt(client: &mut Client) -> Result<()> {
    let tx = client.transaction().await.map_err(Error::sql)?;
    tx.batch_execute("SET LOCAL ROLE oo_jobs_owner")
        .await
        .map_err(Error::sql)?;
    tx.batch_execute(ADAPT_SQL).await.map_err(Error::sql)?;
    tx.execute(
        "INSERT INTO jw.install (id, checksum) VALUES (1, $1)
         ON CONFLICT (id) DO UPDATE SET checksum = EXCLUDED.checksum
         WHERE jw.install.checksum = EXCLUDED.checksum",
        &[&ADAPT_SHA],
    )
    .await
    .map_err(Error::sql)?;
    let stored: String = tx
        .query_one("SELECT checksum FROM jw.install WHERE id = 1", &[])
        .await
        .map_err(Error::sql)?
        .get(0);
    if stored != ADAPT_SHA {
        return Err(Error::new("hash"));
    }
    tx.commit().await.map_err(Error::sql)?;
    Ok(())
}

async fn install_login(client: &Client, password: &str) -> Result<()> {
    client
        .batch_execute(
            "DO $$ BEGIN
               IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'oo_a390_runtime_login') THEN
                 CREATE ROLE oo_a390_runtime_login
                   LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION INHERIT;
               END IF;
             END $$;
             GRANT oo_jobs_runtime TO oo_a390_runtime_login;
             GRANT CONNECT ON DATABASE oo_a390_control TO oo_a390_runtime_login;",
        )
        .await
        .map_err(Error::sql)?;
    let statement: String = client
        .query_one(
            "SELECT format('ALTER ROLE oo_a390_runtime_login PASSWORD %L', $1::text)",
            &[&password],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    client.batch_execute(&statement).await.map_err(Error::sql)?;
    let who: String = client
        .query_one("SELECT $1::text", &[&RUNTIME_LOGIN])
        .await
        .map_err(Error::sql)?
        .get(0);
    if who != RUNTIME_LOGIN {
        return Err(Error::new("login"));
    }
    Ok(())
}

async fn require_ready(client: &Client) -> Result<()> {
    let ready: bool = client
        .query_one(
            "SELECT COALESCE(bool_and(schema_version_present), false)
             FROM jobs.lease_operations_readiness",
            &[],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if !ready {
        return Err(Error::new("ready"));
    }
    let rows = client
        .query(
            "SELECT version, checksum FROM jobs.schema_migrations",
            &[],
        )
        .await
        .map_err(Error::sql)?;
    let mut seen = std::collections::BTreeMap::new();
    for row in rows {
        let version: String = row.try_get(0).map_err(Error::sql)?;
        let checksum: String = row.try_get(1).map_err(Error::sql)?;
        seen.insert(version, checksum);
    }
    let expected = [
        ("jobs-bootstrap/1", BOOT_SHA),
        ("jobs-control/1", CONTROL_SHA),
        ("jobs-event-project/1", EVENT_SHA),
        ("jobs-lease-operations/1", LEASE_SHA),
    ];
    if expected
        .iter()
        .any(|(version, sha)| seen.get(*version).map(String::as_str) != Some(*sha))
    {
        return Err(Error::new("hash"));
    }
    Ok(())
}

fn read_sql(dir: &std::path::Path, name: &str, expected: &str) -> Result<String> {
    if name.contains('/') || name.contains("..") {
        return Err(Error::new("input"));
    }
    let bytes = std::fs::read(dir.join(name)).map_err(|_| Error::new("hash"))?;
    let digest = sha256_hex(&bytes);
    if digest != expected {
        return Err(Error::new("hash"));
    }
    String::from_utf8(bytes).map_err(|_| Error::new("hash"))
}

async fn admin(config: &Config, database: &str) -> Result<Client> {
    tls::connect(
        &config.host,
        config.port,
        database,
        &config.admin_user,
        &config.admin_password,
        &config.ca_pem,
    )
    .await
}

pub fn adapt_sha() -> &'static str {
    ADAPT_SHA
}
