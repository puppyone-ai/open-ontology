//! `install-retained-target` prepares one lite Agent DB for retained oo-load
//! generations, the retained Builder and `release-coordinator publish-retained`.
//!
//! It runs inside the deployed lite runtime image (D16), so a clean install
//! needs no repository checkout or test harness. Stdin is one bounded
//! `oo.pipeline.install-retained-target/v1` document naming the administrator,
//! a password file and a CA file. The connection verifies the server
//! certificate and hostname. The sequence is the one the retained Gateway
//! fixtures install (`retained_gateway_install`), followed by the publisher
//! fence schema. A completed install is detected and never repeated. Stdout is
//! a fixed JSON outcome; errors are fixed codes without database text.

#![forbid(unsafe_code)]

use std::io::{self, Read};
use std::process::ExitCode;
use std::time::Duration;

use oo_pipeline::target_installer as ti;
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Executor, PgConnection};

const PROTOCOL: &str = "oo.pipeline.install-retained-target/v1";
const JOBS_SHIM: &str = "CREATE SCHEMA IF NOT EXISTS jobs;
CREATE TABLE IF NOT EXISTS jobs.tasks(id uuid PRIMARY KEY,state text,generation bigint,lease_until timestamptz,project_id uuid);
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_jobs_owner') THEN CREATE ROLE oo_jobs_owner NOLOGIN; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_jobs_runtime') THEN CREATE ROLE oo_jobs_runtime NOLOGIN; END IF;
END $$;
GRANT USAGE ON SCHEMA jobs TO oo_target_owner,oo_pipeline_runtime;
GRANT SELECT ON jobs.tasks TO oo_target_owner;";
const PUBLISH_TARGET: &str =
    include_str!("../../../../tools/release-coordinator/migrations/oo_publish_target_v1.sql");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol: String,
    host: String,
    port: u16,
    database: String,
    user: String,
    password_file: String,
    ca_file: String,
}

fn main() -> ExitCode {
    if std::env::args_os().len() != 1 {
        eprintln!("install-retained-target: usage");
        return ExitCode::from(2);
    }
    match execute() {
        Ok(outcome) => {
            println!("{{\"protocol\":\"{PROTOCOL}\",\"outcome\":\"{outcome}\"}}");
            ExitCode::SUCCESS
        }
        Err(code) => {
            eprintln!("install-retained-target: {code}");
            ExitCode::from(1)
        }
    }
}

fn read_file(path: &str, max: u64) -> Result<Vec<u8>, &'static str> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| "file")?;
    if !meta.is_file() || meta.len() > max {
        return Err("file");
    }
    std::fs::read(path).map_err(|_| "file")
}

#[tokio::main(flavor = "current_thread")]
async fn execute() -> Result<&'static str, &'static str> {
    let mut text = String::new();
    io::stdin()
        .take(65_537)
        .read_to_string(&mut text)
        .map_err(|_| "input")?;
    let request: Request = serde_json::from_str(&text).map_err(|_| "input")?;
    if request.protocol != PROTOCOL || request.port == 0 {
        return Err("input");
    }
    let password = String::from_utf8(read_file(&request.password_file, 1024)?)
        .map_err(|_| "password")?
        .trim_end()
        .to_owned();
    let ca = read_file(&request.ca_file, 1_048_576)?;
    if password.is_empty() {
        return Err("password");
    }
    let options = PgConnectOptions::new()
        .host(&request.host)
        .port(request.port)
        .username(&request.user)
        .password(&password)
        .database(&request.database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca)
        .disable_statement_logging();
    let mut db = tokio::time::timeout(Duration::from_secs(30), options.connect())
        .await
        .map_err(|_| "connect")?
        .map_err(|_| "connect")?;
    let installed: bool =
        sqlx::query_scalar("SELECT to_regclass('oo_publish_target.fence') IS NOT NULL")
            .fetch_one(&mut db)
            .await
            .map_err(|_| "database")?;
    if installed {
        install_logical_identity(&mut db).await?;
        return Ok("already_installed");
    }
    install(&mut db).await?;
    install_logical_identity(&mut db).await?;
    Ok("installed")
}

async fn install_logical_identity(db: &mut PgConnection) -> Result<(), &'static str> {
    let installed: bool =
        sqlx::query_scalar("SELECT to_regclass('_oo.publication_identity') IS NOT NULL")
            .fetch_one(&mut *db)
            .await
            .map_err(|_| "logical_publication")?;
    if !installed {
        db.execute(include_str!(
            "../../migrations/0135_logical_publication_identity.sql"
        ))
        .await
        .map_err(|_| "logical_publication")?;
    }
    Ok(())
}

async fn install(db: &mut PgConnection) -> Result<(), &'static str> {
    // Each step is the library's own checked installer; the order matches the
    // retained Gateway fixtures so no step is exercised here for the first time.
    macro_rules! step {
        ($($f:ident),* $(,)?) => {$(
            ti::$f(&mut *db).await.map_err(|_| stringify!($f))?;
        )*};
    }
    step!(install);
    db.execute(JOBS_SHIM).await.map_err(|_| "jobs_shim")?;
    step!(
        install_full_import,
        install_catalog_before_empty,
        install_jsonb_document,
        install_native_update,
        install_native_partition,
        install_native_partition_rebind,
        install_native_interval,
        install_native_olap_partition,
        install_native_pg_transaction_group,
        install_native_empty_catalog,
        install_native_pg_group_receipt,
        install_native_pg_group_prior,
        install_native_pg_group_generation,
        install_native_pg_single_transaction,
        install_native_sqlserver_ct,
        install_native_gtid_transaction,
        install_native_c1_source_epoch,
        install_native_c1_source_epoch_runtime,
        install_native_c1_baseline_stage,
        install_native_c1_gtid_stage_identity,
        install_native_c1_gtid_measured_identity,
        install_native_c1_baseline_typed_bridge,
        install_native_c1_baseline_lease_rebind,
        install_native_c1_baseline_date_quality,
        install_native_c1_baseline_key_join,
        install_native_c1_ack_seed,
        install_native_publication,
        install_native_c1_collection_publication,
        install_native_c1_timed_receipt,
        install_native_c1_quiet_baseline_time,
        install_native_per_table_publication,
        install_native_source_progress_read,
        install_native_partition_observation_read,
        install_native_c1_chunked_baseline,
        install_native_c1_chunked_baseline_final,
        install_native_source_instance_grant,
        install_native_c1_ack_seed_format,
        install_native_c5_range,
        install_native_c5_range_apply,
        install_native_c5_scan,
        install_native_c5_replay_guard,
        install_native_c5_full_initialize,
        install_native_c5_closed_observation,
        install_native_c5_multipart,
        install_masked_rejects,
        install_multipart_cells,
        install_native_c5_publication,
        install_native_c5_single_baseline,
        install_native_c5_multipart_baseline_guard,
        install_native_c5_wide_columns,
        install_native_c5_cells_array,
        install_native_library_publication,
        install_native_library_closed_pointer,
        install_native_library_closed_receipt,
        install_native_library_existing_receipt_fields,
    );
    db.execute(include_str!("../../migrations/0089_native_library_c1.sql"))
        .await
        .map_err(|_| "native_library_c1")?;
    // Last, so a partial install is never mistaken for a completed one.
    let mut tx = sqlx::Connection::begin(&mut *db)
        .await
        .map_err(|_| "publish_target")?;
    tx.execute(PUBLISH_TARGET)
        .await
        .map_err(|_| "publish_target")?;
    tx.execute(
        "GRANT USAGE ON SCHEMA oo_publish_target TO oo_target_owner;
         GRANT SELECT ON oo_publish_target.fence TO oo_target_owner;",
    )
    .await
    .map_err(|_| "publish_target")?;
    tx.commit().await.map_err(|_| "publish_target")
}
