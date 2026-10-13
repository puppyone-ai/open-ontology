//! Capture window on the published jobs lease functions and the published
//! full-import SQL. This file does not create a jobs function. A binding with
//! a `jw.capture_sources` row execs the pinned `doris-reader capture-tick`
//! after claim. Other bindings still read the pinned artifact.

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::process::{Command, Stdio};
use std::time::Duration;

use bytes::Bytes;
use futures_util::SinkExt;
use tokio_postgres::Client;

use crate::api::{LeaseStatus, ADAPT_SHA, HEARTBEAT_SECONDS};
use crate::capture_plan::{
    applied_pos, batch_id, driver_pinned, encode_batch, fire_choice, hook_digest, is_physical_name,
    payload_digest, plan_mapping, quote_physical, read_capture, receipt_key, secret_shape, FireChoice,
    Level, MapRequest, CAPTURE_CONFIG, CAPTURE_KIND, CAPTURE_PROJECT, NAMESPACE,
    PIPE_BOOT_SHA, PIPE_FENCE_SHA, PIPE_FULL_SHA, PIPE_MAPPING_SHA, PIPE_PREFLIGHT_SHA, PIPE_V2_SHA,
    SOURCE_DORIS,
};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::lease::{self, Enqueue};
use crate::session;
use crate::sha256::sha256_hex;
use crate::sql;

const ADAPT_SQL: &str = include_str!("../adapt.sql");

pub async fn dispatch(config: &Config, command: &str, flags: &[(String, String)]) -> Result<()> {
    match command {
        "capture-apply" => install(config).await,
        "capture-bind" => bind(config, flags).await,
        "capture-arm" => arm(config, flags).await,
        "capture-period" => set_period(config, flags).await,
        "capture-repin" => repin(config, flags).await,
        "capture-fire" => fire(config, flags).await,
        "capture-step" => step(config, &required(flags, "worker")?).await,
        "capture-hold" => hold(config, &required(flags, "worker")?).await,
        "capture-beat" => beat(config, flags).await,
        "capture-finish-old" => finish_old(config, flags).await,
        "capture-reject" => reject_payloads(config).await,
        "capture-fence" => fence_role(config).await,
        "capture-show" => show(config, flags).await,
        "capture-source-apply" => apply_source_schema(config).await,
        "capture-source" => attach_source(config, flags).await,
        "capture-source-update" => update_source(config, flags).await,
        "capture-source-delete" => delete_source(config, flags).await,
        "capture-disable" => disable_schedule(config, flags).await,
        "capture-pause-fe" => pause_fe(config, flags).await,
        "capture-resume-fe" => resume_fe(config, flags).await,
        "capture-replay" => replay_receipt(config, flags).await,
        _ => Err(Error::new("usage")),
    }
}

async fn install(config: &Config) -> Result<()> {
    let capture = config.capture()?;
    owned_name(&config.database)?;
    owned_name(&capture.target_database)?;
    owned_name(&config.runtime_user)?;
    owned_name(&capture.target_runtime_user)?;
    if config.database == capture.target_database {
        return Err(Error::new("config"));
    }
    if sha256_hex(ADAPT_SQL.as_bytes()) != ADAPT_SHA {
        return Err(Error::new("hash"));
    }
    install_control(config).await?;
    install_target(config).await?;
    println!("control=ready");
    println!("target=ready");
    println!("databases=2");
    println!("segment_v2=not_executed sha={PIPE_V2_SHA}");
    println!("full_import_sha={PIPE_FULL_SHA}");
    Ok(())
}

async fn install_control(config: &Config) -> Result<()> {
    let bootstrap = read_hashed(&config.migrations, "0000_bootstrap_roles.sql", crate::api::BOOT_SHA)?;
    let control = read_hashed(&config.migrations, "0001_jobs_control_expand.sql", crate::api::CONTROL_SHA)?;
    let event = read_hashed(
        &config.migrations,
        "0002_event_project_binding_expand.sql",
        crate::api::EVENT_SHA,
    )?;
    let lease = read_hashed(
        &config.migrations,
        "0003_job_lease_operations_expand.sql",
        crate::api::LEASE_SHA,
    )?;
    {
        let admin = session::admin_db(config, "postgres").await?;
        let exists: i64 = admin
            .query_one(
                "SELECT count(*)::bigint FROM pg_database WHERE datname = $1",
                &[&config.database],
            )
            .await
            .map_err(Error::sql)?
            .get(0);
        if exists == 0 {
            let statement: String = admin
                .query_one(
                    "SELECT format('CREATE DATABASE %I ENCODING %L TEMPLATE template0', $1::text, 'UTF8')",
                    &[&config.database],
                )
                .await
                .map_err(Error::sql)?
                .get(0);
            admin.batch_execute(&statement).await.map_err(Error::sql)?;
        }
    }
    let mut client = session::admin_db(config, &config.database).await?;
    let present: Option<String> = client
        .query_one("SELECT to_regclass('jobs.schema_migrations')::text", &[])
        .await
        .map_err(Error::sql)?
        .get(0);
    if present.is_none() {
        let tx = client.transaction().await.map_err(Error::sql)?;
        tx.batch_execute("SET LOCAL lock_timeout = '15s'").await.map_err(Error::sql)?;
        tx.batch_execute("SET LOCAL statement_timeout = '120s'").await.map_err(Error::sql)?;
        tx.batch_execute("SELECT pg_advisory_xact_lock(6401202601)").await.map_err(Error::sql)?;
        tx.batch_execute(&bootstrap).await.map_err(Error::sql)?;
        tx.batch_execute("SET LOCAL ROLE oo_jobs_owner").await.map_err(Error::sql)?;
        tx.batch_execute(&control).await.map_err(Error::sql)?;
        tx.batch_execute(&event).await.map_err(Error::sql)?;
        tx.batch_execute(&lease).await.map_err(Error::sql)?;
        tx.execute(
            "INSERT INTO jobs.schema_migrations (version, checksum, phase, filename) VALUES
             ('jobs-bootstrap/1', $1, 'expand', '0000_bootstrap_roles.sql'),
             ('jobs-control/1', $2, 'expand', '0001_jobs_control_expand.sql'),
             ('jobs-event-project/1', $3, 'expand', '0002_event_project_binding_expand.sql'),
             ('jobs-lease-operations/1', $4, 'expand', '0003_job_lease_operations_expand.sql')",
            &[
                &crate::api::BOOT_SHA,
                &crate::api::CONTROL_SHA,
                &crate::api::EVENT_SHA,
                &crate::api::LEASE_SHA,
            ],
        )
        .await
        .map_err(Error::sql)?;
        tx.commit().await.map_err(Error::sql)?;
    }
    let mut client = session::admin_db(config, &config.database).await?;
    let tx = client.transaction().await.map_err(Error::sql)?;
    tx.batch_execute("SET LOCAL ROLE oo_jobs_owner").await.map_err(Error::sql)?;
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
    ensure_member(
        &client,
        &config.runtime_user,
        &config.runtime_password,
        "oo_jobs_runtime",
        &config.database,
    )
    .await?;
    Ok(())
}

async fn install_target(config: &Config) -> Result<()> {
    let capture = config.capture()?;
    let boot = read_hashed(&capture.pipeline_migrations, "0000_bootstrap_roles.sql", PIPE_BOOT_SHA)?;
    let fence = read_hashed(&capture.pipeline_migrations, "0001_target_fence_expand.sql", PIPE_FENCE_SHA)?;
    let preflight = read_hashed(
        &capture.pipeline_migrations,
        "0003_target_segment_preflight_expand.sql",
        PIPE_PREFLIGHT_SHA,
    )?;
    let mapping = read_hashed(&capture.pipeline_migrations, "0004_physical_mapping.sql", PIPE_MAPPING_SHA)?;
    let full = read_hashed(&capture.pipeline_migrations, "0005_full_import_expand.sql", PIPE_FULL_SHA)?;
    let _historical = read_hashed(
        &capture.pipeline_migrations,
        "0002_target_segment_null_expand.sql",
        PIPE_V2_SHA,
    )?;
    {
        let admin = session::admin_db(config, "postgres").await?;
        let exists: i64 = admin
            .query_one(
                "SELECT count(*)::bigint FROM pg_database WHERE datname = $1",
                &[&capture.target_database],
            )
            .await
            .map_err(Error::sql)?
            .get(0);
        if exists == 0 {
            let statement: String = admin
                .query_one(
                    "SELECT format('CREATE DATABASE %I ENCODING %L TEMPLATE template0', $1::text, 'UTF8')",
                    &[&capture.target_database],
                )
                .await
                .map_err(Error::sql)?
                .get(0);
            admin.batch_execute(&statement).await.map_err(Error::sql)?;
        }
    }
    let mut client = session::admin_db(config, &capture.target_database).await?;
    let ready = target_versions(&client).await?;
    if !ready.iter().any(|(version, _)| version == "target-bootstrap/1") {
        let tx = client.transaction().await.map_err(Error::sql)?;
        tx.batch_execute("SET LOCAL statement_timeout = '120s'").await.map_err(Error::sql)?;
        tx.batch_execute(&boot).await.map_err(Error::sql)?;
        tx.commit().await.map_err(Error::sql)?;
    }
    let mut client = session::admin_db(config, &capture.target_database).await?;
    let ready = target_versions(&client).await?;
    if !production_ready(&ready) {
        let tx = client.transaction().await.map_err(Error::sql)?;
        tx.batch_execute("SET LOCAL lock_timeout = '15s'").await.map_err(Error::sql)?;
        tx.batch_execute("SET LOCAL statement_timeout = '120s'").await.map_err(Error::sql)?;
        tx.batch_execute("SELECT pg_catalog.pg_advisory_xact_lock(6404202605)").await.map_err(Error::sql)?;
        tx.batch_execute("SET LOCAL ROLE oo_target_owner").await.map_err(Error::sql)?;
        tx.batch_execute(&fence).await.map_err(Error::sql)?;
        tx.batch_execute(&preflight).await.map_err(Error::sql)?;
        tx.batch_execute(&mapping).await.map_err(Error::sql)?;
        tx.execute(
            "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES
             ('target-bootstrap/1', $1, 'expand', '0000_bootstrap_roles.sql'),
             ('target-fence-receipts/1', $2, 'expand', '0001_target_fence_expand.sql'),
             ('target-fence-receipts/3', $3, 'expand', '0003_target_segment_preflight_expand.sql'),
             ('physical-mapping/v1', $4, 'expand', '0004_physical_mapping.sql')",
            &[&PIPE_BOOT_SHA, &PIPE_FENCE_SHA, &PIPE_PREFLIGHT_SHA, &PIPE_MAPPING_SHA],
        )
        .await
        .map_err(Error::sql)?;
        tx.commit().await.map_err(Error::sql)?;
    }
    let mut client = session::admin_db(config, &capture.target_database).await?;
    let ready = target_versions(&client).await?;
    if !ready.iter().any(|(version, sha)| version == "full-import/v1" && sha == PIPE_FULL_SHA) {
        if ready.iter().any(|(version, sha)| version == "full-import/v1" && sha != PIPE_FULL_SHA) {
            return Err(Error::new("hash"));
        }
        let tx = client.transaction().await.map_err(Error::sql)?;
        tx.batch_execute("SET LOCAL lock_timeout = '15s'").await.map_err(Error::sql)?;
        tx.batch_execute("SET LOCAL statement_timeout = '120s'").await.map_err(Error::sql)?;
        tx.batch_execute("SELECT pg_catalog.pg_advisory_xact_lock(6404202605)").await.map_err(Error::sql)?;
        tx.batch_execute("GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE")
            .await
            .map_err(Error::sql)?;
        tx.batch_execute("SET LOCAL ROLE oo_target_owner").await.map_err(Error::sql)?;
        tx.batch_execute(&full).await.map_err(Error::sql)?;
        tx.execute(
            "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES
             ('full-import/v1', $1, 'expand', '0005_full_import_expand.sql')",
            &[&PIPE_FULL_SHA],
        )
        .await
        .map_err(Error::sql)?;
        tx.commit().await.map_err(Error::sql)?;
    }
    let client = session::admin_db(config, &capture.target_database).await?;
    ensure_member(
        &client,
        &capture.target_runtime_user,
        &capture.target_runtime_password,
        "oo_pipeline_runtime",
        &capture.target_database,
    )
    .await?;
    Ok(())
}

fn production_ready(rows: &[(String, String)]) -> bool {
    let expect = [
        ("target-bootstrap/1", PIPE_BOOT_SHA),
        ("target-fence-receipts/1", PIPE_FENCE_SHA),
        ("target-fence-receipts/3", PIPE_PREFLIGHT_SHA),
        ("physical-mapping/v1", PIPE_MAPPING_SHA),
    ];
    expect.iter().all(|(version, sha)| {
        rows.iter().any(|(got_version, got_sha)| got_version == version && got_sha == sha)
    })
}

async fn target_versions(client: &Client) -> Result<Vec<(String, String)>> {
    let present: Option<String> = client
        .query_one("SELECT to_regclass('_oo.schema_migrations')::text", &[])
        .await
        .map_err(Error::sql)?
        .get(0);
    if present.is_none() {
        return Ok(Vec::new());
    }
    let rows = client
        .query("SELECT version, checksum FROM _oo.schema_migrations", &[])
        .await
        .map_err(Error::sql)?;
    let mut out = Vec::new();
    for row in rows {
        out.push((
            row.try_get(0).map_err(Error::sql)?,
            row.try_get(1).map_err(Error::sql)?,
        ));
    }
    Ok(out)
}

async fn ensure_member(
    client: &Client,
    login: &str,
    password: &str,
    member_of: &str,
    database: &str,
) -> Result<()> {
    owned_name(login)?;
    let exists: i64 = client
        .query_one("SELECT count(*)::bigint FROM pg_roles WHERE rolname = $1", &[&login])
        .await
        .map_err(Error::sql)?
        .get(0);
    if exists == 0 {
        let statement: String = client
            .query_one(
                "SELECT format('CREATE ROLE %I LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION INHERIT', $1::text)",
                &[&login],
            )
            .await
            .map_err(Error::sql)?
            .get(0);
        client.batch_execute(&statement).await.map_err(Error::sql)?;
    }
    let grant: String = client
        .query_one(
            "SELECT format('GRANT %I TO %I', $1::text, $2::text)",
            &[&member_of, &login],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    client.batch_execute(&grant).await.map_err(Error::sql)?;
    let connect: String = client
        .query_one(
            "SELECT format('GRANT CONNECT ON DATABASE %I TO %I', $1::text, $2::text)",
            &[&database, &login],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    client.batch_execute(&connect).await.map_err(Error::sql)?;
    let password_sql: String = client
        .query_one(
            "SELECT format('ALTER ROLE %I PASSWORD %L', $1::text, $2::text)",
            &[&login, &password],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    client.batch_execute(&password_sql).await.map_err(Error::sql)?;
    Ok(())
}

struct Binding {
    id: String,
    source: String,
    driver_sha: String,
    artifact_rel: String,
    secret_ref: String,
    table_name: String,
    shard: String,
    fence_owner: String,
    row_budget: i32,
    byte_budget: i32,
    artifact_sha: String,
    source_generation: i64,
}

async fn bind(config: &Config, flags: &[(String, String)]) -> Result<()> {
    let capture = config.capture()?;
    let source = required(flags, "source")?;
    let driver = required(flags, "driver")?;
    let artifact = required(flags, "artifact")?;
    let table = required(flags, "table")?;
    let period_ms = required(flags, "period-ms")?.parse::<i32>().map_err(|_| Error::new("input"))?;
    let rows = required(flags, "rows")?.parse::<i32>().map_err(|_| Error::new("input"))?;
    let bytes = required(flags, "bytes")?.parse::<i32>().map_err(|_| Error::new("input"))?;
    let shard = required(flags, "shard")?;
    let dedupe = required(flags, "dedupe")?;
    if !driver_pinned(&source, &driver) || !(1000..=86_400_000).contains(&period_ms) {
        return Err(Error::new("input"));
    }
    if !(1..=64).contains(&rows) || !(1..=2_000_000).contains(&bytes) {
        return Err(Error::new("input"));
    }
    if !shard_ok(&shard) || !table_ok(&table) || !(1..=200).contains(&dedupe.len()) || !dedupe.is_ascii() {
        return Err(Error::new("input"));
    }
    let path = resolve_artifact(&capture.capture_root, &artifact)?;
    let file = std::fs::read(&path).map_err(|_| Error::new("capture"))?;
    let digest = sha256_hex(&file);
    check_secret_ref(&capture.secret_ref)?;
    let secret = capture.secret_ref.display().to_string();
    let mut client = session::runtime(config).await?;
    let tx = client.transaction().await.map_err(Error::sql)?;
    let ids = tx
        .query_one("SELECT gen_random_uuid()::text, gen_random_uuid()::text", &[])
        .await
        .map_err(Error::sql)?;
    let id: String = ids.get(0);
    let owner: String = ids.get(1);
    tx.execute(
        "INSERT INTO jw.capture_bindings (
           id, project_id, source, driver_sha, artifact_rel, secret_ref, table_name,
           shard, fence_owner, period_ms, row_budget, byte_budget, current_pin
         ) VALUES (
           $1::text::uuid, $2::text::uuid, $3, $4, $5, $6, $7, $8, $9::text::uuid, $10, $11, $12, 1
         )",
        &[
            &id,
            &CAPTURE_PROJECT,
            &source,
            &driver,
            &artifact,
            &secret,
            &table,
            &shard,
            &owner,
            &period_ms,
            &rows,
            &bytes,
        ],
    )
    .await
    .map_err(Error::sql)?;
    tx.execute(
        "INSERT INTO jw.capture_pins (binding_id, pin, artifact_sha, source_generation)
         VALUES ($1::text::uuid, 1, $2, 1)",
        &[&id, &digest],
    )
    .await
    .map_err(Error::sql)?;
    let payload = crate::capture_plan::CapturePayload::text(&id, 1);
    let schedule_digest = sha256_hex(format!("capture-binding/v1\n{id}").as_bytes());
    let schedule_id: String = tx
        .query_one(
            "INSERT INTO jobs.schedules (
               project_id, kind, config_version, artifact_digest, dedupe_prefix, next_run_at
             ) VALUES ($1::text::uuid, $2, $3, $4, $5, clock_timestamp())
             RETURNING id::text",
            &[&CAPTURE_PROJECT, &CAPTURE_KIND, &CAPTURE_CONFIG, &schedule_digest, &dedupe],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    tx.execute(
        "UPDATE jw.capture_bindings
         SET schedule_id = $2::text::uuid, updated_at = clock_timestamp()
         WHERE id = $1::text::uuid",
        &[&id, &schedule_id],
    )
    .await
    .map_err(Error::sql)?;
    tx.execute(
        "INSERT INTO jw.schedule_payloads (
           schedule_id, project_id, payload, payload_version, config_version, artifact_digest, max_attempts
         ) VALUES ($1::text::uuid, $2::text::uuid, $3, 1, $4, $5, 8)",
        &[
            &schedule_id,
            &CAPTURE_PROJECT,
            &payload,
            &CAPTURE_CONFIG,
            &sha256_hex(payload.as_bytes()),
        ],
    )
    .await
    .map_err(Error::sql)?;
    tx.execute(
        "SELECT jw.schedule_put($1::text::uuid, $2::text::uuid, $3, $4::text::jsonb)",
        &[&schedule_id, &CAPTURE_PROJECT, &period_ms, &payload],
    )
    .await
    .map_err(Error::sql)?;
    tx.commit().await.map_err(Error::sql)?;
    println!("bind={id}");
    println!("schedule={schedule_id}");
    println!("artifact_sha={digest}");
    println!("period_ms={period_ms}");
    println!("source={source}");
    println!("driver={driver}");
    println!("table={table}");
    Ok(())
}

async fn arm(config: &Config, flags: &[(String, String)]) -> Result<()> {
    let binding = required(flags, "binding")?;
    let kind = required(flags, "kind")?;
    if !crate::api::is_uuid(&binding) || (kind != "apply_once" && kind != "mirror_once") {
        return Err(Error::new("input"));
    }
    let client = session::runtime(config).await?;
    client
        .execute(
            "INSERT INTO jw.capture_faults (binding_id, kind, armed) VALUES ($1::text::uuid, $2, true)
             ON CONFLICT (binding_id) DO UPDATE SET kind = EXCLUDED.kind, armed = true",
            &[&binding, &kind],
        )
        .await
        .map_err(Error::sql)?;
    println!("armed={kind}");
    println!("binding={binding}");
    Ok(())
}

async fn set_period(config: &Config, flags: &[(String, String)]) -> Result<()> {
    let binding = required(flags, "binding")?;
    let period_ms = required(flags, "period-ms")?.parse::<i32>().map_err(|_| Error::new("input"))?;
    if !crate::api::is_uuid(&binding) || !(1000..=86_400_000).contains(&period_ms) {
        return Err(Error::new("input"));
    }
    let client = session::runtime(config).await?;
    let before: String = client
        .query_one(
            "SELECT s.next_run_at::text
             FROM jw.capture_bindings AS b
             JOIN jobs.schedules AS s ON s.id = b.schedule_id
             WHERE b.id = $1::text::uuid AND b.project_id = $2::text::uuid",
            &[&binding, &CAPTURE_PROJECT],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    let updated: i32 = client
        .query_one(
            "SELECT jw.schedule_set_period($1::text::uuid, $2::text::uuid, $3::int)",
            &[&binding, &CAPTURE_PROJECT, &period_ms],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    let viewed = client
        .query_one(
            "SELECT next_run_at::text, period_ms
             FROM jw.schedule_api
             WHERE id = (
               SELECT schedule_id FROM jw.capture_bindings WHERE id = $1::text::uuid
             )",
            &[&binding],
        )
        .await
        .map_err(Error::sql)?;
    let after: String = viewed.get(0);
    let api_period: i32 = viewed.get(1);
    if before != after || api_period != period_ms {
        return Err(Error::new("schedule"));
    }
    println!("binding={binding}");
    println!("period_was_ms={updated}");
    println!("period_now_ms={period_ms}");
    println!("api_period_ms={api_period}");
    println!("schedule_api=jw.schedule_set_period");
    println!("next_before={before}");
    println!("next_after={after}");
    println!("inflight_payload=unchanged");
    Ok(())
}

async fn repin(config: &Config, flags: &[(String, String)]) -> Result<()> {
    let capture = config.capture()?;
    let binding = required(flags, "binding")?;
    let artifact = required(flags, "artifact")?;
    if !crate::api::is_uuid(&binding) {
        return Err(Error::new("input"));
    }
    let path = resolve_artifact(&capture.capture_root, &artifact)?;
    let digest = sha256_hex(&std::fs::read(&path).map_err(|_| Error::new("capture"))?);
    let mut client = session::runtime(config).await?;
    let tx = client.transaction().await.map_err(Error::sql)?;
    let current: i64 = tx
        .query_one(
            "SELECT current_pin FROM jw.capture_bindings WHERE id = $1::text::uuid AND project_id = $2::text::uuid FOR UPDATE",
            &[&binding, &CAPTURE_PROJECT],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    let pin = current + 1;
    tx.execute(
        "INSERT INTO jw.capture_pins (binding_id, pin, artifact_sha, source_generation)
         VALUES ($1::text::uuid, $2, $3, $2)",
        &[&binding, &pin, &digest],
    )
    .await
    .map_err(Error::sql)?;
    tx.execute(
        "UPDATE jw.capture_bindings
         SET artifact_rel = $2, current_pin = $3, updated_at = clock_timestamp()
         WHERE id = $1::text::uuid",
        &[&binding, &artifact, &pin],
    )
    .await
    .map_err(Error::sql)?;
    tx.execute(
        "SELECT jw.schedule_set_pin($1::text::uuid, $2::text::uuid, $3::bigint)",
        &[&binding, &CAPTURE_PROJECT, &pin],
    )
    .await
    .map_err(Error::sql)?;
    tx.commit().await.map_err(Error::sql)?;
    println!("binding={binding}");
    println!("pin={pin}");
    println!("artifact_sha={digest}");
    Ok(())
}

async fn fire(config: &Config, flags: &[(String, String)]) -> Result<()> {
    let mut client = session::runtime(config).await?;
    let schedules = if let Some(schedule) = optional(flags, "schedule") {
        if !crate::api::is_uuid(&schedule) {
            return Err(Error::new("input"));
        }
        vec![schedule]
    } else {
        let rows = client
            .query(
                "SELECT id::text FROM jobs.schedules
                 WHERE project_id = $1::text::uuid AND kind = $2 AND enabled
                   AND next_run_at <= clock_timestamp()
                 ORDER BY next_run_at, id",
                &[&CAPTURE_PROJECT, &CAPTURE_KIND],
            )
            .await
            .map_err(Error::sql)?;
        rows.into_iter().map(|row| row.get(0)).collect()
    };
    if schedules.is_empty() {
        println!("fire=idle");
        return Ok(());
    }
    for schedule in schedules {
        fire_one(&mut client, &schedule).await?;
    }
    Ok(())
}

async fn fire_one(client: &mut Client, schedule_id: &str) -> Result<()> {
    let tx = client.transaction().await.map_err(Error::sql)?;
    let row = tx
        .query_opt(
            "SELECT next_run_at::text, (next_run_at <= clock_timestamp())
             FROM jobs.schedules
             WHERE id = $1::text::uuid AND project_id = $2::text::uuid AND kind = $3 AND enabled
             FOR UPDATE",
            &[&schedule_id, &CAPTURE_PROJECT, &CAPTURE_KIND],
        )
        .await
        .map_err(Error::sql)?;
    let Some(row) = row else {
        tx.commit().await.map_err(Error::sql)?;
        println!("fire=idle schedule={schedule_id}");
        return Ok(());
    };
    let due: bool = row.get(1);
    let binding = tx
        .query_one(
            "SELECT b.id::text, b.current_pin, t.period_ms
             FROM jw.capture_bindings AS b
             JOIN jw.schedule_terms AS t ON t.schedule_id = b.schedule_id
             WHERE b.schedule_id = $1::text::uuid",
            &[&schedule_id],
        )
        .await
        .map_err(Error::sql)?;
    let binding_id: String = binding.get(0);
    let pin: i64 = binding.get(1);
    let period_ms: i32 = binding.get(2);
    let busy: bool = tx
        .query_one(
            "SELECT EXISTS (
               SELECT 1 FROM jobs.schedule_occurrences AS occurrence
               JOIN jobs.tasks AS task ON task.id = occurrence.task_id
               WHERE occurrence.schedule_id = $1::text::uuid
                 AND task.state IN ('queued', 'running')
             )",
            &[&schedule_id],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    match fire_choice(due, busy) {
        FireChoice::Idle => {
            tx.commit().await.map_err(Error::sql)?;
            println!("fire=idle schedule={schedule_id}");
            return Ok(());
        }
        FireChoice::Overlap => {
            tx.commit().await.map_err(Error::sql)?;
            println!("fire=overlap schedule={schedule_id}");
            return Ok(());
        }
        FireChoice::Fire => {}
    }
    let payload = crate::capture_plan::CapturePayload::text(&binding_id, pin);
    let digest = sha256_hex(payload.as_bytes());
    let dedupe: String = tx
        .query_one(
            "SELECT 'sched:' || id::text || ':' || to_char(next_run_at AT TIME ZONE 'UTC', 'YYYYMMDDHH24MISSUS')
             FROM jobs.schedules WHERE id = $1::text::uuid",
            &[&schedule_id],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    let enqueued = lease::enqueue(
        &tx,
        Enqueue {
            project_id: CAPTURE_PROJECT,
            kind: CAPTURE_KIND,
            payload_version: 1,
            payload: &payload,
            config_version: CAPTURE_CONFIG,
            digest: &digest,
            dedupe_key: &dedupe,
            run_delay_seconds: 0.0,
            max_attempts: 8,
        },
    )
    .await?;
    if enqueued.status == LeaseStatus::Replayed {
        tx.commit().await.map_err(Error::sql)?;
        println!("fire=duplicate schedule={schedule_id}");
        return Ok(());
    }
    if enqueued.status != LeaseStatus::Enqueued {
        tx.rollback().await.map_err(Error::sql)?;
        return Err(Error::new("schedule"));
    }
    let task_id = enqueued.task_id.ok_or_else(|| Error::new("schedule"))?;
    tx.execute(sql::NOTIFY, &[&CAPTURE_PROJECT]).await.map_err(Error::sql)?;
    if let Err(err) = tx
        .execute(
            "INSERT INTO jobs.schedule_occurrences (schedule_id, scheduled_at, project_id, task_id)
             SELECT id, next_run_at, $2::text::uuid, $3::text::uuid
             FROM jobs.schedules WHERE id = $1::text::uuid",
            &[&schedule_id, &CAPTURE_PROJECT, &task_id],
        )
        .await
    {
        let mapped = Error::sql(err);
        tx.rollback().await.map_err(Error::sql)?;
        if mapped.is_unique() {
            println!("fire=duplicate schedule={schedule_id}");
            return Ok(());
        }
        return Err(mapped);
    }
    let advanced = tx
        .execute(
            "UPDATE jobs.schedules
             SET next_run_at = next_run_at + ($2::int * interval '1 millisecond'),
                 updated_at = clock_timestamp()
             WHERE id = $1::text::uuid AND project_id = $3::text::uuid AND next_run_at <= clock_timestamp()",
            &[&schedule_id, &period_ms, &CAPTURE_PROJECT],
        )
        .await
        .map_err(Error::sql)?;
    if advanced != 1 {
        tx.rollback().await.map_err(Error::sql)?;
        return Err(Error::new("schedule"));
    }
    tx.commit().await.map_err(Error::sql)?;
    println!("fire=fired schedule={schedule_id} task={task_id} pin={pin} period_ms={period_ms}");
    Ok(())
}

struct Claimed {
    task_id: String,
    generation: i64,
    owner: String,
    payload: String,
}

async fn step(config: &Config, worker: &str) -> Result<()> {
    worker_ok(worker)?;
    let mut control = session::runtime(config).await?;
    let claimed = match claim_or_resume(&mut control, worker).await? {
        Some(value) => value,
        None => {
            println!("step=none");
            return Ok(());
        }
    };
    beat_if_due(&mut control, &claimed).await?;
    let parsed = match crate::capture_plan::CapturePayload::parse(&claimed.payload) {
        Ok(value) => value,
        Err(_) => {
            return fail_task(&mut control, &claimed, "payload_rejected", "payload_rejected").await;
        }
    };
    let binding = match load_binding(&control, &parsed.binding, parsed.pin).await {
        Ok(value) => value,
        Err(err) => {
            eprintln!("binding_load={}", err.kind);
            return fail_task(&mut control, &claimed, "payload_rejected", "payload_rejected").await;
        }
    };
    let capture = config.capture()?;
    if let Err(reason) = prepare_input(&capture.capture_root, &binding) {
        return fail_task(&mut control, &claimed, "payload_rejected", reason).await;
    }
    if source_configured(&control, &binding.id).await? {
        println!("handler=source");
        return step_source(config, &mut control, &claimed, &binding).await;
    }
    println!("handler=artifact");
    let file = std::fs::read(&resolve_artifact(&capture.capture_root, &binding.artifact_rel)?)
        .map_err(|_| Error::new("capture"))?;
    if sha256_hex(&file) != binding.artifact_sha {
        return fail_task(&mut control, &claimed, "payload_rejected", "artifact_mismatch").await;
    }
    let batch = match read_capture(&file, &binding.table_name, binding.row_budget, binding.byte_budget) {
        Ok(value) => value,
        Err(err) => {
            let reason = if err.kind == "budget" {
                "budget_exceeded"
            } else {
                "source_not_legal_window"
            };
            return fail_task(&mut control, &claimed, "payload_rejected", reason).await;
        }
    };
    if batch.quality != "DELETES_LAGGED" {
        return fail_task(&mut control, &claimed, "payload_rejected", "source_not_legal_window").await;
    }
    import_captured(config, &mut control, &claimed, &binding, batch).await
}

async fn import_captured(
    config: &Config,
    control: &mut Client,
    claimed: &Claimed,
    binding: &Binding,
    batch: crate::capture_plan::LegalBatch,
) -> Result<()> {
    let copy = encode_batch(&batch)?;
    let batch_name = batch_id(binding.source_generation);
    let digest = payload_digest(&copy, &batch_name, batch.rows.len() as u64, &batch.input_digest);
    let pos = applied_pos(batch.rows.len())?;
    let physical = table_physical(&batch, binding.source_generation)?;
    let mut pipe = session::target_runtime(config).await?;
    let recover: String = pipe
        .query_one(
            "SELECT _oo.full_import_recover($1, $2, $3, $4)",
            &[&SOURCE_DORIS, &physical, &batch_name, &digest],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    println!(
        "planned physical={physical} rows={} batch={batch_name} recover={recover}",
        batch.rows.len()
    );
    let fault = armed_fault(control, &binding.id).await?;
    if recover == "replay" {
        drop(pipe);
        return finish_window(
            control,
            claimed,
            binding,
            &digest,
            &pos,
            false,
            "replay",
            &receipt_key(&physical, binding.source_generation),
        )
        .await;
    }
    if recover != "absent" {
        return Err(Error::new("target"));
    }
    let applied = apply_target(
        &mut pipe,
        binding,
        &batch,
        &copy,
        &digest,
        &pos,
        &physical,
        &batch_name,
        fault.as_deref() == Some("apply_once"),
    )
    .await?;
    drop(pipe);
    match applied {
        ApplyEnd::Aborted => {
            disarm(control, &binding.id).await?;
            let tx = control.transaction().await.map_err(Error::sql)?;
            let result = lease::retry_failure(
                &tx,
                CAPTURE_PROJECT,
                &claimed.task_id,
                &claimed.owner,
                claimed.generation,
                "worker_failed",
            )
            .await?;
            tx.commit().await.map_err(Error::sql)?;
            println!(
                "step=retried reason=apply_once status={} task={} watermark=absent",
                result.status.as_str(),
                claimed.task_id
            );
            if result.status != LeaseStatus::Retried {
                return Err(Error::new("lease"));
            }
            Ok(())
        }
        ApplyEnd::Inserted => {
            if fault.as_deref() == Some("mirror_once") {
                disarm(control, &binding.id).await?;
                println!(
                    "step=mirror_pending task={} physical={physical} rows={} receipt={} watermark=absent copied=true",
                    claimed.task_id,
                    batch.rows.len(),
                    receipt_key(&physical, binding.source_generation)
                );
                return Ok(());
            }
            finish_window(
                control,
                claimed,
                binding,
                &digest,
                &pos,
                true,
                "awaiting_publish",
                &receipt_key(&physical, binding.source_generation),
            )
            .await
        }
    }
}

async fn apply_source_schema(config: &Config) -> Result<()> {
    if !crate::capture_source::sql_pinned() {
        return Err(Error::new("hash"));
    }
    let mut client = session::admin_db(config, &config.database).await?;
    let tx = client.transaction().await.map_err(Error::sql)?;
    tx.batch_execute("SET LOCAL lock_timeout = '15s'").await.map_err(Error::sql)?;
    tx.batch_execute("SET LOCAL statement_timeout = '60s'").await.map_err(Error::sql)?;
    tx.batch_execute("SET LOCAL ROLE oo_jobs_owner").await.map_err(Error::sql)?;
    tx.batch_execute(crate::capture_source::SOURCE_SQL).await.map_err(Error::sql)?;
    tx.execute(
        "INSERT INTO jw.capture_source_install (id, checksum) VALUES (1, $1)
         ON CONFLICT (id) DO UPDATE SET checksum = EXCLUDED.checksum",
        &[&crate::capture_source::SOURCE_SQL_SHA],
    )
    .await
    .map_err(Error::sql)?;
    let stored: String = tx
        .query_one("SELECT checksum FROM jw.capture_source_install WHERE id = 1", &[])
        .await
        .map_err(Error::sql)?
        .get(0);
    if stored != crate::capture_source::SOURCE_SQL_SHA {
        return Err(Error::new("hash"));
    }
    tx.commit().await.map_err(Error::sql)?;
    println!("source_sql=ready");
    println!("source_sql_sha={stored}");
    Ok(())
}

async fn attach_source(config: &Config, flags: &[(String, String)]) -> Result<()> {
    let binding = required(flags, "binding")?;
    let host = required(flags, "host")?;
    let port = required(flags, "port")?.parse::<i32>().map_err(|_| Error::new("input"))?;
    let evidence = required(flags, "evidence")?;
    let secret_ref = required(flags, "secret-ref")?;
    if !crate::api::is_uuid(&binding) {
        return Err(Error::new("input"));
    }
    let capture = config.capture()?;
    let client = session::runtime(config).await?;
    let driver: String = client
        .query_one(
            "SELECT driver_sha FROM jw.capture_bindings
             WHERE id = $1::text::uuid AND project_id = $2::text::uuid AND source = 'doris'",
            &[&binding, &CAPTURE_PROJECT],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    let row = source_row(&driver, &host, port, &evidence, &secret_ref);
    crate::capture_source::fixed_source(&row).map_err(|_| Error::new("input"))?;
    let binary = crate::capture_source::join_under(&capture.capture_root, crate::capture_source::READER_REL)
        .map_err(|_| Error::new("input"))?;
    let bytes = std::fs::read(&binary).map_err(|_| Error::new("capture"))?;
    if !crate::capture_source::binary_digest_ok(&bytes) {
        return Err(Error::new("hash"));
    }
    if ready_secret(&secret_ref).is_err() {
        return Err(Error::new("secret"));
    }
    client
        .execute(
            "INSERT INTO jw.capture_sources (
               binding_id, driver_name, driver_commit, driver_sha256, binary_rel,
               secret_ref, source_host, source_port, evidence_rel, command
             ) VALUES (
               $1::text::uuid, $2, $3, $4, $5, $6, $7, $8, $9, $10
             )",
            &[
                &binding,
                &row.driver_name,
                &row.driver_commit,
                &row.driver_sha256,
                &row.binary_rel,
                &row.secret_ref,
                &row.source_host,
                &row.source_port,
                &row.evidence_rel,
                &row.command,
            ],
        )
        .await
        .map_err(Error::sql)?;
    println!("source=inserted binding={binding}");
    println!("driver={} commit={driver}", row.driver_name);
    println!("driver_sha256={}", row.driver_sha256);
    println!("command={}", row.command);
    println!("host={host} port={port} db={}", crate::capture_source::SOURCE_DB);
    println!("evidence={evidence}");
    println!("secret_ref=directory");
    Ok(())
}

async fn update_source(config: &Config, flags: &[(String, String)]) -> Result<()> {
    let binding = required(flags, "binding")?;
    let host = required(flags, "host")?;
    let port = required(flags, "port")?.parse::<i32>().map_err(|_| Error::new("input"))?;
    let evidence = required(flags, "evidence")?;
    let secret_ref = required(flags, "secret-ref")?;
    if !crate::api::is_uuid(&binding) {
        return Err(Error::new("input"));
    }
    let row = source_row("a5aed1167caa54e68e3a2be7335142465fb2d31a", &host, port, &evidence, &secret_ref);
    crate::capture_source::fixed_source(&row).map_err(|_| Error::new("input"))?;
    if ready_secret(&secret_ref).is_err() {
        return Err(Error::new("secret"));
    }
    let client = session::runtime(config).await?;
    let prior: String = client
        .query_one(
            "SELECT evidence_rel FROM jw.capture_sources WHERE binding_id = $1::text::uuid",
            &[&binding],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    let updated = client
        .execute(
            "UPDATE jw.capture_sources
             SET secret_ref = $2, source_host = $3, source_port = $4, evidence_rel = $5,
                 updated_at = clock_timestamp()
             WHERE binding_id = $1::text::uuid",
            &[&binding, &secret_ref, &host, &port, &evidence],
        )
        .await
        .map_err(Error::sql)?;
    if updated != 1 {
        return Err(Error::new("input"));
    }
    println!("source=updated binding={binding}");
    println!("evidence_was={prior}");
    println!("evidence_now={evidence}");
    println!("host={host} port={port}");
    println!("command=capture-tick");
    Ok(())
}

async fn delete_source(config: &Config, flags: &[(String, String)]) -> Result<()> {
    let binding = required(flags, "binding")?;
    if !crate::api::is_uuid(&binding) {
        return Err(Error::new("input"));
    }
    let client = session::runtime(config).await?;
    let deleted = client
        .execute(
            "DELETE FROM jw.capture_sources WHERE binding_id = $1::text::uuid",
            &[&binding],
        )
        .await
        .map_err(Error::sql)?;
    if deleted != 1 {
        return Err(Error::new("input"));
    }
    println!("source=deleted binding={binding}");
    Ok(())
}

async fn disable_schedule(config: &Config, flags: &[(String, String)]) -> Result<()> {
    let schedule = required(flags, "schedule")?;
    if !crate::api::is_uuid(&schedule) {
        return Err(Error::new("input"));
    }
    let client = session::runtime(config).await?;
    let updated: bool = client
        .query_one(
            "SELECT jw.schedule_set_enabled($1::text::uuid, $2::text::uuid, $3, false)",
            &[&schedule, &CAPTURE_PROJECT, &CAPTURE_KIND],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if !updated {
        return Err(Error::new("input"));
    }
    let enabled: bool = client
        .query_one(
            "SELECT enabled FROM jw.schedule_api WHERE id = $1::text::uuid AND project_id = $2::text::uuid",
            &[&schedule, &CAPTURE_PROJECT],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if enabled {
        return Err(Error::new("schedule"));
    }
    println!("schedule=disabled schedule={schedule}");
    println!("enabled=false");
    println!("schedule_api=jw.schedule_set_enabled");
    Ok(())
}

async fn pause_fe(config: &Config, flags: &[(String, String)]) -> Result<()> {
    let host = required(flags, "host")?;
    let port = required(flags, "port")?.parse::<i32>().map_err(|_| Error::new("input"))?;
    if !crate::capture_source::ipv4_ok(&host) || !(1..=65535).contains(&port) {
        return Err(Error::new("input"));
    }
    let mut client = session::runtime(config).await?;
    let started: String = client
        .query_one("SELECT clock_timestamp()::text", &[])
        .await
        .map_err(Error::sql)?
        .get(0);
    let rows = client
        .query(
            "SELECT b.schedule_id::text, b.id::text, s.dedupe_prefix, api.enabled
             FROM jw.capture_sources AS src
             JOIN jw.capture_bindings AS b ON b.id = src.binding_id
             JOIN jobs.schedules AS s ON s.id = b.schedule_id
             JOIN jw.schedule_api AS api ON api.id = s.id AND api.project_id = s.project_id
             WHERE src.source_host = $1 AND src.source_port = $2
             ORDER BY s.dedupe_prefix, b.schedule_id",
            &[&host, &port],
        )
        .await
        .map_err(Error::sql)?;
    println!("pause_started={started}");
    println!("pause_host={host} pause_port={port}");
    println!("pause_matched={}", rows.len());
    if rows.is_empty() {
        return Err(Error::new("input"));
    }
    let tx = client.transaction().await.map_err(Error::sql)?;
    for row in &rows {
        let schedule: String = row.get(0);
        let binding: String = row.get(1);
        let dedupe: String = row.get(2);
        let enabled: bool = row.get(3);
        if !enabled {
            println!("pause=already schedule={schedule} binding={binding} dedupe={dedupe} enabled=false");
            continue;
        }
        let updated: bool = tx
            .query_one(
                "SELECT jw.schedule_set_enabled($1::text::uuid, $2::text::uuid, $3, false)",
                &[&schedule, &CAPTURE_PROJECT, &CAPTURE_KIND],
            )
            .await
            .map_err(Error::sql)?
            .get(0);
        if !updated {
            tx.rollback().await.map_err(Error::sql)?;
            return Err(Error::new("schedule"));
        }
        println!(
            "pause=disabled schedule={schedule} binding={binding} dedupe={dedupe} enabled=false schedule_api=jw.schedule_set_enabled"
        );
    }
    tx.commit().await.map_err(Error::sql)?;
    let still: i64 = client
        .query_one(
            "SELECT count(*)::bigint
             FROM jw.capture_sources AS src
             JOIN jw.capture_bindings AS b ON b.id = src.binding_id
             JOIN jw.schedule_api AS api ON api.id = b.schedule_id
             WHERE src.source_host = $1 AND src.source_port = $2 AND api.enabled",
            &[&host, &port],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if still != 0 {
        return Err(Error::new("schedule"));
    }
    println!("pause_enabled_remaining=0");
    println!("pause_source_writes=0");
    println!("pause_resume=not_called");
    Ok(())
}

async fn resume_fe(config: &Config, flags: &[(String, String)]) -> Result<()> {
    let host = required(flags, "host")?;
    let port = required(flags, "port")?.parse::<i32>().map_err(|_| Error::new("input"))?;
    if !crate::capture_source::ipv4_ok(&host) || !(1..=65535).contains(&port) {
        return Err(Error::new("input"));
    }
    let mut client = session::runtime(config).await?;
    let started: String = client
        .query_one("SELECT clock_timestamp()::text", &[])
        .await
        .map_err(Error::sql)?
        .get(0);
    let rows = client
        .query(
            "SELECT b.schedule_id::text, b.id::text, s.dedupe_prefix, api.enabled
             FROM jw.capture_sources AS src
             JOIN jw.capture_bindings AS b ON b.id = src.binding_id
             JOIN jobs.schedules AS s ON s.id = b.schedule_id
             JOIN jw.schedule_api AS api ON api.id = s.id AND api.project_id = s.project_id
             WHERE src.source_host = $1 AND src.source_port = $2
             ORDER BY s.dedupe_prefix, b.schedule_id",
            &[&host, &port],
        )
        .await
        .map_err(Error::sql)?;
    println!("resume_started={started}");
    println!("resume_host={host} resume_port={port}");
    println!("resume_matched={}", rows.len());
    println!("source_restart=not_called");
    if rows.is_empty() {
        return Err(Error::new("input"));
    }
    let tx = client.transaction().await.map_err(Error::sql)?;
    for row in &rows {
        let schedule: String = row.get(0);
        let binding: String = row.get(1);
        let dedupe: String = row.get(2);
        let enabled: bool = row.get(3);
        if enabled {
            println!("resume=already schedule={schedule} binding={binding} dedupe={dedupe} enabled=true");
            continue;
        }
        let updated: bool = tx
            .query_one(
                "SELECT jw.schedule_set_enabled($1::text::uuid, $2::text::uuid, $3, true)",
                &[&schedule, &CAPTURE_PROJECT, &CAPTURE_KIND],
            )
            .await
            .map_err(Error::sql)?
            .get(0);
        if !updated {
            tx.rollback().await.map_err(Error::sql)?;
            return Err(Error::new("schedule"));
        }
        println!(
            "resume=enabled schedule={schedule} binding={binding} dedupe={dedupe} enabled=true schedule_api=jw.schedule_set_enabled"
        );
    }
    tx.commit().await.map_err(Error::sql)?;
    let still_off: i64 = client
        .query_one(
            "SELECT count(*)::bigint
             FROM jw.capture_sources AS src
             JOIN jw.capture_bindings AS b ON b.id = src.binding_id
             JOIN jw.schedule_api AS api ON api.id = b.schedule_id
             WHERE src.source_host = $1 AND src.source_port = $2 AND NOT api.enabled",
            &[&host, &port],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if still_off != 0 {
        return Err(Error::new("schedule"));
    }
    println!("resume_disabled_remaining=0");
    println!("resume_source_writes=0");
    println!("pause_resume=called");
    Ok(())
}

async fn schedule_enabled(client: &Client, binding: &str) -> Result<bool> {
    let enabled: Option<bool> = client
        .query_opt(
            "SELECT api.enabled
             FROM jw.capture_bindings AS b
             JOIN jw.schedule_api AS api ON api.id = b.schedule_id
             WHERE b.id = $1::text::uuid",
            &[&binding],
        )
        .await
        .map_err(Error::sql)?
        .map(|row| row.get(0));
    Ok(enabled.unwrap_or(false))
}

async fn replay_receipt(config: &Config, flags: &[(String, String)]) -> Result<()> {
    let binding = required(flags, "binding")?;
    if !crate::api::is_uuid(&binding) {
        return Err(Error::new("input"));
    }
    let client = session::runtime(config).await?;
    let row = client
        .query_one(
            "SELECT receipt_key, payload_digest, source_generation, advanced_at::text
             FROM jw.capture_watermarks
             WHERE binding_id = $1::text::uuid AND project_id = $2::text::uuid",
            &[&binding, &CAPTURE_PROJECT],
        )
        .await
        .map_err(Error::sql)?;
    let key: String = row.get(0);
    let digest: String = row.get(1);
    let generation: i64 = row.get(2);
    let advanced: String = row.get(3);
    let (physical, batch) = crate::capture_source::split_receipt(&key).ok_or_else(|| Error::new("target"))?;
    let physical = physical.to_string();
    let batch = batch.to_string();
    let pipe = session::target_runtime(config).await?;
    let recover: String = pipe
        .query_one(
            "SELECT _oo.full_import_recover($1, $2, $3, $4)",
            &[&SOURCE_DORIS, &physical, &batch, &digest],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    drop(pipe);
    let action = crate::capture_source::reader_action(&recover);
    let argv = crate::capture_source::tick_argv(crate::capture_source::TickSpec {
        action,
        binary: Path::new(crate::capture_source::READER_REL),
        evidence: Path::new("linux/run-source-1/replay"),
        secret_dir: "/run/oo-reader-secrets",
        host: "172.16.43.10",
        port: 9030,
        every_ms: 5000,
        label: "pin1",
    })
    .map_err(|_| Error::new("target"))?;
    let after: String = client
        .query_one(
            "SELECT advanced_at::text FROM jw.capture_watermarks WHERE binding_id = $1::text::uuid",
            &[&binding],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    println!("recover={recover}");
    println!("reader_argv={}", argv.len());
    println!("reader=not_executed");
    println!("watermark_generation={generation}");
    println!("advanced_before={advanced}");
    println!("advanced_after={after}");
    if action != crate::capture_source::ReaderAction::Skip || !argv.is_empty() || advanced != after {
        return Err(Error::new("replay"));
    }
    println!("watermark=unchanged");
    Ok(())
}

async fn source_configured(client: &Client, id: &str) -> Result<bool> {
    let present: Option<String> = client
        .query_one("SELECT to_regclass('jw.capture_sources')::text", &[])
        .await
        .map_err(Error::sql)?
        .get(0);
    if present.is_none() {
        return Ok(false);
    }
    let row = client
        .query_opt(
            "SELECT 1 FROM jw.capture_sources WHERE binding_id = $1::text::uuid",
            &[&id],
        )
        .await
        .map_err(Error::sql)?;
    Ok(row.is_some())
}

async fn load_source(client: &Client, id: &str) -> Result<crate::capture_source::SourceRow> {
    let row = client
        .query_one(
            "SELECT driver_name, driver_commit, driver_sha256, binary_rel, secret_ref,
                    source_host, source_port, evidence_rel, command
             FROM jw.capture_sources WHERE binding_id = $1::text::uuid",
            &[&id],
        )
        .await
        .map_err(Error::sql)?;
    Ok(crate::capture_source::SourceRow {
        driver_name: row.get(0),
        driver_commit: row.get(1),
        driver_sha256: row.get(2),
        binary_rel: row.get(3),
        secret_ref: row.get(4),
        source_host: row.get(5),
        source_port: row.get(6),
        evidence_rel: row.get(7),
        command: row.get(8),
    })
}

async fn step_source(config: &Config, control: &mut Client, claimed: &Claimed, binding: &Binding) -> Result<()> {
    let source = match load_source(control, &binding.id).await {
        Ok(value) => value,
        Err(_) => {
            return fail_task(control, claimed, "payload_rejected", "source_config_rejected").await;
        }
    };
    if crate::capture_source::fixed_source(&source).is_err() || source.driver_commit != binding.driver_sha {
        return fail_task(control, claimed, "payload_rejected", "source_config_rejected").await;
    }
    let capture = config.capture()?;
    let binary = match crate::capture_source::join_under(&capture.capture_root, &source.binary_rel) {
        Ok(path) => path,
        Err(reason) => return fail_task(control, claimed, "payload_rejected", reason).await,
    };
    let bytes = std::fs::read(&binary).map_err(|_| Error::new("capture"))?;
    if !crate::capture_source::binary_digest_ok(&bytes) {
        return fail_task(control, claimed, "payload_rejected", "source_config_rejected").await;
    }
    let secret_dir = match ready_secret(&source.secret_ref) {
        Ok(dir) => dir.to_string(),
        Err(reason) => return fail_task(control, claimed, "payload_rejected", reason).await,
    };
    let pinned = std::fs::read(&resolve_artifact(&capture.capture_root, &binding.artifact_rel)?)
        .map_err(|_| Error::new("capture"))?;
    if sha256_hex(&pinned) != binding.artifact_sha {
        return fail_task(control, claimed, "payload_rejected", "artifact_mismatch").await;
    }
    let stored = control
        .query_opt(
            "SELECT receipt_key, payload_digest, source_generation, advanced_at::text
             FROM jw.capture_watermarks
             WHERE binding_id = $1::text::uuid AND project_id = $2::text::uuid",
            &[&binding.id, &CAPTURE_PROJECT],
        )
        .await
        .map_err(Error::sql)?;
    if let Some(row) = stored {
        let key: String = row.get(0);
        let digest: String = row.get(1);
        let generation: i64 = row.get(2);
        let advanced: String = row.get(3);
        if key.starts_with("native/") {
            println!("stored_native=held");
            println!("advanced_before={advanced}");
            println!("reader=scheduled reason=new_window");
        } else {
        let Some((physical, batch)) = crate::capture_source::split_receipt(&key) else {
            return Err(Error::new("target"));
        };
        let physical = physical.to_string();
        let batch = batch.to_string();
        let pipe = session::target_runtime(config).await?;
        let recover: String = pipe
            .query_one(
                "SELECT _oo.full_import_recover($1, $2, $3, $4)",
                &[&SOURCE_DORIS, &physical, &batch, &digest],
            )
            .await
            .map_err(Error::sql)?
            .get(0);
        drop(pipe);
        println!("stored_recover={recover}");
        if crate::capture_source::reader_action(&recover) == crate::capture_source::ReaderAction::Skip {
            let tx = control.transaction().await.map_err(Error::sql)?;
            let done = lease::complete_success(
                &tx,
                CAPTURE_PROJECT,
                &claimed.task_id,
                &claimed.owner,
                claimed.generation,
            )
            .await?;
            tx.commit().await.map_err(Error::sql)?;
            let after: String = control
                .query_one(
                    "SELECT advanced_at::text FROM jw.capture_watermarks WHERE binding_id = $1::text::uuid",
                    &[&binding.id],
                )
                .await
                .map_err(Error::sql)?
                .get(0);
            println!("reader=not_executed reader_argv=0 watermark_generation={generation}");
            println!("advanced_before={advanced}");
            println!("advanced_after={after}");
            if advanced != after {
                return Err(Error::new("watermark"));
            }
            if done.status != LeaseStatus::Succeeded {
                println!("step=lease status={} watermark=unchanged", done.status.as_str());
                return Err(Error::new("lease"));
            }
            println!("step=replayed task={} watermark=unchanged", claimed.task_id);
            return Ok(());
        }
        if crate::capture_source::reader_action(&recover) == crate::capture_source::ReaderAction::Refuse {
            return Err(Error::new("target"));
        }
        }
    } else {
        println!("stored_recover=absent");
    }
    if !schedule_enabled(control, &binding.id).await? {
        println!("reader=not_executed reader_argv=0 reason=schedule_disabled");
        fail_task(control, claimed, "worker_failed", "schedule_disabled").await?;
        let rows = watermark_rows(control, &binding.id).await?;
        println!("watermark_rows={rows}");
        if rows != 0 {
            return Err(Error::new("watermark"));
        }
        return Ok(());
    }
    let period_ms: i32 = control
        .query_one(
            "SELECT period_ms FROM jw.capture_bindings WHERE id = $1::text::uuid",
            &[&binding.id],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    let label = match crate::capture_source::label_for_pin(binding.source_generation) {
        Ok(label) => label,
        Err(reason) => return fail_task(control, claimed, "payload_rejected", reason).await,
    };
    let evidence = crate::capture_source::join_under(Path::new("/evidence"), &source.evidence_rel)
        .map_err(|_| Error::new("input"))?;
    std::fs::create_dir_all(&evidence).map_err(|_| Error::new("capture"))?;
    if evidence.join("native-applied.json").is_file() {
        println!("reader=not_executed reader_argv=0 reason=native_applied_marker");
        return resume_native(config, control, claimed, binding, &evidence).await;
    }
    let argv = match crate::capture_source::tick_argv(crate::capture_source::TickSpec {
        action: crate::capture_source::ReaderAction::Read,
        binary: &binary,
        evidence: &evidence,
        secret_dir: &secret_dir,
        host: &source.source_host,
        port: source.source_port,
        every_ms: period_ms,
        label: &label,
    }) {
        Ok(argv) => argv,
        Err(reason) => {
            let kind = crate::capture_source::failure_kind(reason);
            return fail_task(control, claimed, kind, reason).await;
        }
    };
    println!(
        "reader_command=capture-tick reader_argv={} label={label} every_ms={period_ms} host={} port={} db={} user={}",
        argv.len(),
        source.source_host,
        source.source_port,
        crate::capture_source::SOURCE_DB,
        crate::capture_source::SOURCE_USER
    );
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| Error::new("capture"))?;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if let Err(err) = beat_if_due(control, claimed).await {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(err);
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::new("capture"));
            }
        }
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_end(&mut stdout);
    }
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_end(&mut stderr);
    }
    stdout.truncate(8192);
    stderr.truncate(8192);
    let secret = secret_text(Path::new(&source.secret_ref));
    let stdout_text = crate::capture_source::scrub_secret(&String::from_utf8_lossy(&stdout), &secret);
    let stderr_text = crate::capture_source::scrub_secret(&String::from_utf8_lossy(&stderr), &secret);
    let mut secret_bytes = secret.into_bytes();
    secret_bytes.fill(0);
    let code = status.code().unwrap_or(1);
    println!("reader_exit={code} reader_executed=true");
    if code != 0 {
        let _ = std::fs::write(evidence.join("reader-stderr.txt"), &stderr_text);
        let _ = std::fs::write(evidence.join("reader-stdout.txt"), &stdout_text);
        let reason = crate::capture_source::reader_refusal(&stderr_text);
        let missing = crate::capture_source::missing_parameter(reason, &source.source_host, source.source_port);
        let kind = crate::capture_source::failure_kind(reason);
        let before = watermark_advanced(control, &binding.id).await?;
        fail_task(control, claimed, kind, reason).await?;
        let after = watermark_advanced(control, &binding.id).await?;
        let rows = watermark_rows(control, &binding.id).await?;
        println!("missing_parameter={missing}");
        println!(
            "fe_host={} fe_port={} fe_db={} reader_user={}",
            source.source_host,
            source.source_port,
            crate::capture_source::SOURCE_DB,
            crate::capture_source::SOURCE_USER
        );
        println!("advanced_before={}", before.as_deref().unwrap_or("absent"));
        println!("advanced_after={}", after.as_deref().unwrap_or("absent"));
        println!("watermark_rows={rows}");
        if before != after {
            println!("watermark=moved");
            return Err(Error::new("watermark"));
        }
        println!("watermark=unchanged");
        return Ok(());
    }
    submit_native(config, control, claimed, binding, &source, &evidence, &label).await
}

fn ready_secret(secret_ref: &str) -> std::result::Result<&str, &'static str> {
    let dir = crate::capture_source::resolve_secret_dir(secret_ref)?;
    if password_ready(Path::new(dir)) {
        Ok(dir)
    } else {
        Err("source_secret_rejected")
    }
}

async fn resume_native(
    config: &Config,
    control: &mut Client,
    claimed: &Claimed,
    binding: &Binding,
    evidence: &Path,
) -> Result<()> {
    let result = invoke_saved(config, control, claimed, &binding.id, evidence).await?;
    finish_native(config, control, claimed, binding, evidence, &result).await
}

async fn submit_native(
    config: &Config,
    control: &mut Client,
    claimed: &Claimed,
    binding: &Binding,
    source: &crate::capture_source::SourceRow,
    evidence: &Path,
    label: &str,
) -> Result<()> {
    let tick_path = evidence.join(format!("{label}.json"));
    let tick_bytes = std::fs::read(&tick_path).map_err(|_| Error::new("capture"))?;
    let tick: serde_json::Value = serde_json::from_slice(&tick_bytes).map_err(|_| Error::new("capture"))?;
    let arrow_path = evidence.join(format!("{label}-{}.arrow", binding.table_name));
    let arrow = std::fs::read(&arrow_path).map_err(|_| Error::new("capture"))?;
    let audit_path = evidence.join(format!("reader-audit-{label}.jsonl"));
    let audit = std::fs::read(&audit_path).unwrap_or_else(|_| tick_bytes.clone());
    let arrow_sha = sha256_hex(&arrow);
    let audit_sha = sha256_hex(&audit);
    let arrow_text = arrow_path.to_str().ok_or_else(|| Error::new("input"))?.to_string();
    let capture = config.capture()?;
    let input = crate::capture_source::NativeRequestInput {
        tick: &tick,
        table: &binding.table_name,
        arrow_path: &arrow_text,
        arrow_sha256: &arrow_sha,
        audit_sha256: &audit_sha,
        driver_sha256: &source.driver_sha256,
        host: &source.source_host,
        port: source.source_port,
        secret_ref: &source.secret_ref,
        generation: binding.source_generation,
        batch_id: label,
        target: crate::capture_source::NativeTarget {
            host: &config.host,
            port: config.port,
            database: &capture.target_database,
            runtime_principal: &capture.target_runtime_user,
            password_file: "/secrets/pipe.pass",
            ca_file: "/secrets/ca.crt",
            shard: &binding.shard,
            owner: &binding.fence_owner,
        },
    };
    let mut request = match crate::capture_source::native_request(&input) {
        Ok(value) => value,
        Err(reason) => {
            println!("native_update=not_called");
            return refuse_kept(control, claimed, &binding.id, crate::capture_source::failure_kind(reason), reason).await;
        }
    };
    let bundle = match crate::capture_source::producer_bundle(&request, &tick, &binding.table_name, &arrow) {
        Ok(value) => value,
        Err(reason) => {
            println!("native_update=not_called");
            return refuse_kept(control, claimed, &binding.id, crate::capture_source::failure_kind(reason), reason).await;
        }
    };
    let receipt_path = evidence.join("producer-receipt.json");
    let registry_path = evidence.join("producer-registry.json");
    let catalog_path = evidence.join("producer-catalog.json");
    if std::fs::write(&receipt_path, &bundle.receipt).is_err()
        || std::fs::write(&registry_path, &bundle.registry).is_err()
        || std::fs::write(&catalog_path, &bundle.catalog).is_err()
    {
        return fail_closed_native(control, claimed, &binding.id, "producer_receipt_unwritten").await;
    }
    let receipt_sha = sha256_hex(&bundle.receipt);
    let receipt_text = receipt_path.to_str().ok_or_else(|| Error::new("input"))?;
    if let Err(reason) = crate::capture_source::attach_admission(&mut request, receipt_text, &receipt_sha) {
        println!("native_update=not_called");
        return refuse_kept(control, claimed, &binding.id, crate::capture_source::failure_kind(reason), reason).await;
    }
    println!("producer_receipt_sha={receipt_sha}");
    println!("producer_registry_sha={}", sha256_hex(&bundle.registry));
    println!("producer_catalog_sha={}", sha256_hex(&bundle.catalog));
    println!("producer_trust=not_asserted entry=worker_file");
    if let Err(reason) = crate::capture_source::attach_observed_lease(&mut request, &claimed.task_id, claimed.generation) {
        println!("native_update=not_called");
        return fail_closed_native(control, claimed, &binding.id, reason).await;
    }
    println!(
        "lease_observe=task task={} generation={} jobs_claim=not_called_by_loader fence_cert=not_written",
        claimed.task_id, claimed.generation
    );
    if !secret_files_match(config) {
        println!("native_update=not_called");
        return fail_closed_native(control, claimed, &binding.id, "loader_secret_rejected").await;
    }
    let bytes = serde_json::to_vec(&request).map_err(|_| Error::new("capture"))?;
    std::fs::write(evidence.join("native-request.json"), &bytes).map_err(|_| Error::new("capture"))?;
    if !ensure_native_schema(config, control, claimed, &binding.id, &binding.shard, &binding.fence_owner).await? {
        let state: String = control
            .query_one(
                "SELECT state FROM jobs.tasks WHERE id = $1::text::uuid AND project_id = $2::text::uuid",
                &[&claimed.task_id, &CAPTURE_PROJECT],
            )
            .await
            .map_err(Error::sql)?
            .get(0);
        if state == "running" {
            println!("native_update=not_called");
            return fail_closed_native(control, claimed, &binding.id, "loader_trust_unrepaired").await;
        }
        return Ok(());
    }
    let result = invoke_bytes(config, control, claimed, evidence, &bytes).await?;
    finish_native(config, control, claimed, binding, evidence, &result).await
}

async fn finish_native(
    config: &Config,
    control: &mut Client,
    claimed: &Claimed,
    binding: &Binding,
    evidence: &Path,
    result: &serde_json::Value,
) -> Result<()> {
    match crate::capture_source::native_decision(result) {
        crate::capture_source::NativeDecision::Advance | crate::capture_source::NativeDecision::Replay => {
            let request: serde_json::Value = serde_json::from_slice(
                &std::fs::read(evidence.join("native-request.json")).map_err(|_| Error::new("capture"))?,
            )
            .map_err(|_| Error::new("capture"))?;
            let physical = result
                .get("physical_name")
                .and_then(|value| value.as_str())
                .ok_or_else(|| Error::new("target"))?;
            let digest = result
                .get("payload_digest")
                .and_then(|value| value.as_str())
                .ok_or_else(|| Error::new("target"))?;
            let batch = request
                .get("batch_id")
                .and_then(|value| value.as_str())
                .ok_or_else(|| Error::new("target"))?;
            if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(Error::new("target"));
            }
            let key = crate::capture_source::native_receipt_key(physical, batch).map_err(|_| Error::new("target"))?;
            finish_window(control, claimed, binding, digest, "0", true, "awaiting_publish", &key).await
        }
        crate::capture_source::NativeDecision::MirrorPending => {
            println!("step=mirror_pending task={} watermark=absent", claimed.task_id);
            let _ = config;
            Ok(())
        }
        crate::capture_source::NativeDecision::Refused => {
            let token = loader_reason_token(result);
            fail_closed_native(control, claimed, &binding.id, &token).await
        }
    }
}

async fn refuse_kept(
    control: &mut Client,
    claimed: &Claimed,
    binding: &str,
    kind: &str,
    reason: &str,
) -> Result<()> {
    let before = watermark_advanced(control, binding).await?;
    fail_task(control, claimed, kind, reason).await?;
    let after = watermark_advanced(control, binding).await?;
    println!("advanced_before={}", before.as_deref().unwrap_or("absent"));
    println!("advanced_after={}", after.as_deref().unwrap_or("absent"));
    if before != after {
        println!("watermark=moved");
        return Err(Error::new("watermark"));
    }
    println!("watermark=unchanged");
    println!("step=refused reason={reason} task={}", claimed.task_id);
    Ok(())
}

async fn fail_closed_native(control: &mut Client, claimed: &Claimed, binding: &str, reason: &str) -> Result<()> {
    let before = watermark_advanced(control, binding).await?;
    fail_task(control, claimed, "worker_failed", reason).await?;
    let after = watermark_advanced(control, binding).await?;
    let rows = watermark_rows(control, binding).await?;
    println!("advanced_before={}", before.as_deref().unwrap_or("absent"));
    println!("advanced_after={}", after.as_deref().unwrap_or("absent"));
    println!("watermark_rows={rows}");
    if before != after {
        println!("watermark=moved");
        return Err(Error::new("watermark"));
    }
    println!("watermark=unchanged");
    println!("step=loader_refused reason={reason} task={}", claimed.task_id);
    Ok(())
}

async fn watermark_advanced(client: &Client, binding: &str) -> Result<Option<String>> {
    client
        .query_opt(
            "SELECT advanced_at::text FROM jw.capture_watermarks WHERE binding_id = $1::text::uuid",
            &[&binding],
        )
        .await
        .map_err(Error::sql)
        .map(|row| row.map(|row| row.get(0)))
}

async fn ensure_native_schema(
    config: &Config,
    control: &mut Client,
    claimed: &Claimed,
    binding_id: &str,
    shard: &str,
    owner: &str,
) -> Result<bool> {
    let pipe = session::target_runtime(config).await?;
    let present: Option<String> = pipe
        .query_one(
            "SELECT to_regprocedure('_oo.native_update_read_batch(text,text,text,text)')::text",
            &[],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    drop(pipe);
    if present.is_some() {
        println!("native_update_sql=present");
        return Ok(true);
    }
    if !crate::capture_source::loader_import_allowed(crate::capture_source::NATIVE_ELF_SHA256) {
        println!(
            "native_update_sql=absent loader_exec=refused reason=loader_trust_unrepaired loader_commit={}",
            crate::capture_source::LOADER_COMMIT
        );
        return Ok(false);
    }
    println!("native_update_sql=absent loader_commit={}", crate::capture_source::LOADER_COMMIT);
    let capture = config.capture()?;
    let request = crate::capture_source::install_request(&crate::capture_source::NativeTarget {
        host: &config.host,
        port: config.port,
        database: &capture.target_database,
        runtime_principal: &config.admin_user,
        password_file: "/secrets/admin.pass",
        ca_file: "/secrets/ca.crt",
        shard,
        owner,
    })
    .map_err(|_| Error::new("input"))?;
    let bytes = serde_json::to_vec(&request).map_err(|_| Error::new("capture"))?;
    let (code, stdout, stderr) = run_loader(control, claimed, "install-native-update", &bytes).await?;
    let stdout_text = scrub_loader(&String::from_utf8_lossy(&stdout), config);
    let stderr_text = scrub_loader(&String::from_utf8_lossy(&stderr), config);
    println!("install_exit={code}");
    if !stderr_text.is_empty() {
        println!("install_stderr_len={}", stderr_text.len());
    }
    let parsed: serde_json::Value = serde_json::from_str(&stdout_text).unwrap_or(serde_json::Value::Null);
    let installed = parsed.get("installed").and_then(|value| value.as_bool()) == Some(true);
    let reason = parsed.get("reason").and_then(|value| value.as_str()).unwrap_or("");
    println!("install_installed={installed} install_reason={reason}");
    if !installed {
        fail_closed_native(control, claimed, binding_id, "loader_install_failed").await?;
        return Ok(false);
    }
    Ok(true)
}

async fn invoke_saved(
    config: &Config,
    control: &mut Client,
    claimed: &Claimed,
    binding_id: &str,
    evidence: &Path,
) -> Result<serde_json::Value> {
    if !ensure_native_schema(
        config,
        control,
        claimed,
        binding_id,
        "src_ns2_replay",
        "d101fd4a-3a89-41f6-b7fe-94b4001d5512",
    )
    .await?
    {
        println!("native_update=not_called");
        println!("loader_exec=refused reason=loader_trust_unrepaired");
        return Ok(crate::capture_source::unrepaired_loader_result());
    }
    let bytes = std::fs::read(evidence.join("native-request.json")).map_err(|_| Error::new("capture"))?;
    invoke_bytes(config, control, claimed, evidence, &bytes).await
}

async fn invoke_bytes(
    config: &Config,
    control: &mut Client,
    claimed: &Claimed,
    evidence: &Path,
    bytes: &[u8],
) -> Result<serde_json::Value> {
    let elf_bytes = std::fs::read(crate::capture_source::NATIVE_ELF_PATH).map_err(|_| Error::new("capture"))?;
    let elf_sha = sha256_hex(&elf_bytes);
    if elf_sha != crate::capture_source::NATIVE_ELF_SHA256 {
        return Err(Error::new("hash"));
    }
    if !crate::capture_source::loader_import_allowed(&elf_sha) {
        println!("native_update=not_called");
        println!("loader_exec=refused reason=loader_trust_unrepaired");
        println!("loader_elf_sha={elf_sha}");
        let parsed = crate::capture_source::unrepaired_loader_result();
        print_native_fields(&parsed);
        return Ok(parsed);
    }
    let (code, stdout, stderr) = run_loader(control, claimed, "import-native-update", bytes).await?;
    let stdout_text = scrub_loader(&String::from_utf8_lossy(&stdout), config);
    let stderr_text = scrub_loader(&String::from_utf8_lossy(&stderr), config);
    let _ = std::fs::write(evidence.join("native-result.json"), &stdout_text);
    if !stderr_text.is_empty() {
        let _ = std::fs::write(evidence.join("native-stderr.txt"), &stderr_text);
    }
    println!("native_update=called import_command=oo-pipeline import-native-update loader_exit={code}");
    println!("loader_elf_sha={}", crate::capture_source::NATIVE_ELF_SHA256);
    let parsed: serde_json::Value = match serde_json::from_str(&stdout_text) {
        Ok(value) => value,
        Err(_) => {
            println!("loader_reason=result_rejected");
            return Err(Error::new("target"));
        }
    };
    let state = parsed.get("load_state").and_then(|value| value.as_str()).unwrap_or("");
    print_native_fields(&parsed);
    if state == "awaiting_publish" || state == "replay" {
        let _ = std::fs::write(evidence.join("native-applied.json"), &stdout_text);
    }
    Ok(parsed)
}

async fn run_loader(control: &mut Client, claimed: &Claimed, command: &str, bytes: &[u8]) -> Result<(i32, Vec<u8>, Vec<u8>)> {
    let elf = PathBuf::from(crate::capture_source::NATIVE_ELF_PATH);
    let elf_bytes = std::fs::read(&elf).map_err(|_| Error::new("capture"))?;
    let elf_sha = sha256_hex(&elf_bytes);
    if elf_sha != crate::capture_source::NATIVE_ELF_SHA256 {
        return Err(Error::new("hash"));
    }
    if !crate::capture_source::loader_import_allowed(&elf_sha) {
        return Err(Error::new("loader"));
    }
    if command != "import-native-update" && command != "install-native-update" {
        return Err(Error::new("usage"));
    }
    let mut child = Command::new(&elf)
        .arg(command)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| Error::new("capture"))?;
    {
        let mut stdin = child.stdin.take().ok_or_else(|| Error::new("capture"))?;
        stdin.write_all(bytes).map_err(|_| Error::new("capture"))?;
    }
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if let Err(err) = beat_if_due(control, claimed).await {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(err);
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::new("capture"));
            }
        }
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_end(&mut stdout);
    }
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_end(&mut stderr);
    }
    stdout.truncate(65536);
    stderr.truncate(8192);
    Ok((status.code().unwrap_or(1), stdout, stderr))
}

fn secret_files_match(config: &Config) -> bool {
    let pipe = config
        .capture()
        .map(|item| item.target_runtime_password)
        .unwrap_or_default();
    file_text_eq("/secrets/pipe.pass", &pipe)
        && file_text_eq("/secrets/admin.pass", &config.admin_password)
        && std::fs::read("/secrets/ca.crt").ok().as_deref() == Some(config.ca_pem.as_slice())
}

fn file_text_eq(path: &str, expect: &str) -> bool {
    let mut text = std::fs::read_to_string(path).unwrap_or_default();
    if text.ends_with('\n') {
        text.pop();
    }
    let ok = text == expect;
    let mut bytes = text.into_bytes();
    bytes.fill(0);
    ok
}

fn print_native_fields(parsed: &serde_json::Value) {
    let state = parsed.get("load_state").and_then(|value| value.as_str()).unwrap_or("");
    let reason = parsed.get("reason").and_then(|value| value.as_str()).unwrap_or("");
    let bound = parsed.get("source_bound").and_then(|value| value.as_bool()).unwrap_or(false);
    let fence = parsed
        .pointer("/jobs_observation/target_fence_certified")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let lease = parsed
        .get("jobs_lease_receipt")
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string());
    let trusted = parsed
        .pointer("/producer_trust/trusted")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let entry = parsed
        .pointer("/producer_trust/entry")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let watermark_state = match parsed.get("watermark_state") {
        Some(serde_json::Value::String(value)) => value.as_str(),
        _ => "null",
    };
    let tombs = match parsed.get("tombstone_count") {
        Some(serde_json::Value::Number(value)) => value.to_string(),
        _ => "null".to_string(),
    };
    println!("load_state={state}");
    println!("loader_reason={reason}");
    println!("source_bound={bound}");
    println!("producer_trust_trusted={trusted}");
    println!("producer_trust_entry={entry}");
    println!("jobs_lease_receipt={lease}");
    println!("target_fence_certified={fence}");
    println!("watermark_state={watermark_state}");
    println!("tombstone_count={tombs}");
    println!(
        "physical_name={} row_count={} payload_digest={}",
        parsed.get("physical_name").and_then(|value| value.as_str()).unwrap_or(""),
        parsed.get("row_count").and_then(|value| value.as_i64()).map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
        parsed.get("payload_digest").and_then(|value| value.as_str()).unwrap_or("")
    );
}

fn scrub_loader(text: &str, config: &Config) -> String {
    let pipe = config
        .capture()
        .map(|item| item.target_runtime_password)
        .unwrap_or_default();
    let once = crate::capture_source::scrub_secret(text, &config.admin_password);
    crate::capture_source::scrub_secret(&once, &pipe)
}

fn loader_reason_token(result: &serde_json::Value) -> String {
    let raw = result.get("reason").and_then(|value| value.as_str()).unwrap_or("not_sealed");
    let mut token = String::from("loader_");
    for byte in raw.bytes() {
        let lower = byte.to_ascii_lowercase();
        if lower.is_ascii_lowercase() || lower.is_ascii_digit() || lower == b'_' {
            token.push(lower as char);
        }
    }
    if token == "loader_" {
        token.push_str("not_sealed");
    }
    token.truncate(64);
    token
}

async fn watermark_rows(client: &Client, binding: &str) -> Result<i64> {
    client
        .query_one(
            "SELECT count(*)::bigint FROM jw.capture_watermarks WHERE binding_id = $1::text::uuid",
            &[&binding],
        )
        .await
        .map_err(Error::sql)
        .map(|row| row.get(0))
}

fn source_row(driver: &str, host: &str, port: i32, evidence: &str, secret_ref: &str) -> crate::capture_source::SourceRow {
    crate::capture_source::SourceRow {
        driver_name: crate::capture_source::READER_NAME.to_string(),
        driver_commit: driver.to_string(),
        driver_sha256: crate::capture_source::READER_SHA256.to_string(),
        binary_rel: crate::capture_source::READER_REL.to_string(),
        secret_ref: secret_ref.to_string(),
        source_host: host.to_string(),
        source_port: port,
        evidence_rel: evidence.to_string(),
        command: crate::capture_source::READER_COMMAND.to_string(),
    }
}

fn password_ready(dir: &Path) -> bool {
    let Some(text) = dir.to_str() else {
        return false;
    };
    if !crate::capture_source::secret_dir_ok(text) {
        return false;
    }
    let path = dir.join("reader.password");
    let Ok(meta) = std::fs::metadata(&path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    if !crate::capture_source::password_file_mode_ok(meta.permissions().mode() & 0o777) {
        return false;
    }
    let mut bytes = match std::fs::read(&path) {
        Ok(value) => value,
        Err(_) => return false,
    };
    while matches!(bytes.last(), Some(b'\n' | b'\r')) {
        bytes.pop();
    }
    let ok = bytes.len() >= 8 && bytes.iter().all(|byte| byte.is_ascii_hexdigit());
    bytes.fill(0);
    ok
}

fn secret_text(dir: &Path) -> String {
    let mut bytes = std::fs::read(dir.join("reader.password")).unwrap_or_default();
    while matches!(bytes.last(), Some(b'\n' | b'\r')) {
        bytes.pop();
    }
    let text = String::from_utf8(bytes.clone()).unwrap_or_default();
    bytes.fill(0);
    text
}

enum ApplyEnd {
    Inserted,
    Aborted,
}

#[allow(clippy::too_many_arguments)]
async fn apply_target(
    pipe: &mut Client,
    binding: &Binding,
    batch: &crate::capture_plan::LegalBatch,
    copy: &[u8],
    digest: &str,
    pos: &str,
    physical: &str,
    batch_name: &str,
    abort: bool,
) -> Result<ApplyEnd> {
    let tx = pipe.transaction().await.map_err(Error::sql)?;
    let generation: i64 = tx
        .query_one(
            "SELECT _oo.adopt_target_generation($1, $2::text::uuid)",
            &[&binding.shard, &binding.fence_owner],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    tx.execute(
        "SELECT _oo.full_import_bind_session($1, $2::text::uuid, $3)",
        &[&binding.shard, &binding.fence_owner, &generation],
    )
    .await
    .map_err(Error::sql)?;
    register_scope(&tx, batch, binding.source_generation).await?;
    let schemas = vec![NAMESPACE.to_string()];
    let names = vec![batch.table.name.to_string()];
    let physicals = vec![physical.to_string()];
    let run_id: String = tx
        .query_one(
            "SELECT _oo.full_import_open_run($1, $2::text::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
            &[
                &binding.shard,
                &binding.fence_owner,
                &generation,
                &SOURCE_DORIS,
                &binding.source_generation,
                &batch.config_digest,
                &batch.type_digest,
                &batch.input_digest,
                &batch.schema_digest,
                &schemas,
                &names,
                &physicals,
            ],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    let (idents, oids, typmods, required) = column_arrays(batch);
    let shadow: String = tx
        .query_one(
            "SELECT _oo.full_import_prepare_shadow($1, $2::text::uuid, $3, $4::text::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
            &[
                &binding.shard,
                &binding.fence_owner,
                &generation,
                &run_id,
                &0_i32,
                &SOURCE_DORIS,
                &batch.config_digest,
                &binding.source_generation,
                &NAMESPACE,
                &batch.table.name,
                &idents,
                &oids,
                &typmods,
                &required,
            ],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if shadow != physical {
        tx.rollback().await.map_err(Error::sql)?;
        return Err(Error::new("plan"));
    }
    let decision: String = tx
        .query_one(
            "SELECT _oo.full_import_stage_decision($1, $2::text::uuid, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12::text::numeric)",
            &[
                &binding.shard,
                &binding.fence_owner,
                &generation,
                &SOURCE_DORIS,
                &physical,
                &batch_name,
                &digest,
                &batch.config_digest,
                &batch.type_digest,
                &hook_digest(),
                &batch.schema_digest,
                &pos,
            ],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if decision != "apply" {
        tx.rollback().await.map_err(Error::sql)?;
        return Err(Error::new("target"));
    }
    let statement = copy_statement(batch, binding.source_generation, physical)?;
    let copied = {
        let sink = tx.copy_in(&statement).await.map_err(Error::sql)?;
        let mut sink = pin!(sink);
        sink.as_mut()
            .send(Bytes::copy_from_slice(copy))
            .await
            .map_err(Error::sql)?;
        sink.finish().await.map_err(Error::sql)?
    };
    if copied != batch.rows.len() as u64 {
        tx.rollback().await.map_err(Error::sql)?;
        return Err(Error::new("target"));
    }
    let reasons: Vec<String> = Vec::new();
    let lens: Vec<i64> = Vec::new();
    let attnums: Vec<i32> = Vec::new();
    let indexes: Vec<i64> = Vec::new();
    let keys: Vec<String> = Vec::new();
    let accepted = batch.rows.len() as i64;
    let rejected = 0_i64;
    let wrote: String = tx
        .query_one(
            "SELECT _oo.full_import_write_batch($1, $2::text::uuid, $3, $4::text::uuid, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15::text::numeric, $16, $17, $18::text[], $19::bigint[], $20::integer[], $21::bigint[], $22::text[])",
            &[
                &binding.shard,
                &binding.fence_owner,
                &generation,
                &run_id,
                &0_i32,
                &SOURCE_DORIS,
                &physical,
                &batch_name,
                &digest,
                &batch.config_digest,
                &batch.type_digest,
                &hook_digest(),
                &batch.schema_digest,
                &batch.input_digest,
                &pos,
                &accepted,
                &rejected,
                &reasons,
                &lens,
                &attnums,
                &indexes,
                &keys,
            ],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if wrote != "inserted" {
        tx.rollback().await.map_err(Error::sql)?;
        return Err(Error::new("target"));
    }
    tx.batch_execute("SET CONSTRAINTS ALL IMMEDIATE")
        .await
        .map_err(Error::sql)?;
    tx.execute(
        "SELECT _oo.full_import_note_table($1, $2::text::uuid, $3, $4::text::uuid, $5, $6, $7, $8)",
        &[
            &binding.shard,
            &binding.fence_owner,
            &generation,
            &run_id,
            &0_i32,
            &accepted,
            &batch.input_digest,
            &SOURCE_DORIS,
        ],
    )
    .await
    .map_err(Error::sql)?;
    let sealed: String = tx
        .query_one(
            "SELECT _oo.full_import_seal_run($1, $2::text::uuid, $3, $4::text::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::text[])",
            &[
                &binding.shard,
                &binding.fence_owner,
                &generation,
                &run_id,
                &SOURCE_DORIS,
                &binding.source_generation,
                &batch.config_digest,
                &batch.type_digest,
                &batch.input_digest,
                &batch.schema_digest,
                &schemas,
                &names,
            ],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if sealed != "awaiting_publish" {
        tx.rollback().await.map_err(Error::sql)?;
        return Err(Error::new("target"));
    }
    if abort {
        tx.rollback().await.map_err(Error::sql)?;
        return Ok(ApplyEnd::Aborted);
    }
    tx.commit().await.map_err(Error::sql)?;
    Ok(ApplyEnd::Inserted)
}

async fn register_scope(
    tx: &tokio_postgres::Transaction<'_>,
    batch: &crate::capture_plan::LegalBatch,
    generation: i64,
) -> Result<()> {
    let levels = std::iter::once((Level::Namespace, "", "")).chain(std::iter::once((
        Level::Table,
        batch.table.name,
        "",
    )));
    for (level, table, column) in levels {
        register_one(tx, batch, generation, level, table, column).await?;
    }
    for column in batch.table.columns {
        register_one(tx, batch, generation, Level::Column, batch.table.name, column.name).await?;
    }
    Ok(())
}

async fn register_one(
    tx: &tokio_postgres::Transaction<'_>,
    batch: &crate::capture_plan::LegalBatch,
    generation: i64,
    level: Level,
    table: &str,
    column: &str,
) -> Result<()> {
    let request = MapRequest {
        source: SOURCE_DORIS.to_string(),
        config_digest: batch.config_digest.clone(),
        run_generation: generation,
        level,
        namespace: NAMESPACE.to_string(),
        table: table.to_string(),
        column: column.to_string(),
    };
    let plan = plan_mapping(&request)?;
    let digest = plan.digest.to_vec();
    let status: String = tx
        .query_one(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &[
                &SOURCE_DORIS,
                &batch.config_digest,
                &generation,
                &level.word(),
                &NAMESPACE,
                &table,
                &column,
                &digest,
            ],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if status != "inserted" && status != "replay" {
        return Err(Error::new("plan"));
    }
    let stored: String = tx
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &[
                &SOURCE_DORIS,
                &batch.config_digest,
                &generation,
                &level.word(),
                &NAMESPACE,
                &table,
                &column,
            ],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if stored != plan.physical_name {
        return Err(Error::new("plan"));
    }
    Ok(())
}

fn column_arrays(
    batch: &crate::capture_plan::LegalBatch,
) -> (Vec<String>, Vec<i64>, Vec<i32>, Vec<bool>) {
    let mut idents = Vec::new();
    let mut oids = Vec::new();
    let mut typmods = Vec::new();
    let mut required = Vec::new();
    for column in batch.table.columns {
        idents.push(column.name.to_string());
        oids.push(column.oid);
        typmods.push(column.typmod);
        required.push(column.not_null);
    }
    (idents, oids, typmods, required)
}

fn table_physical(batch: &crate::capture_plan::LegalBatch, generation: i64) -> Result<String> {
    let plan = plan_mapping(&MapRequest {
        source: SOURCE_DORIS.to_string(),
        config_digest: batch.config_digest.clone(),
        run_generation: generation,
        level: Level::Table,
        namespace: NAMESPACE.to_string(),
        table: batch.table.name.to_string(),
        column: String::new(),
    })?;
    Ok(plan.physical_name)
}

fn copy_statement(
    batch: &crate::capture_plan::LegalBatch,
    generation: i64,
    table_physical: &str,
) -> Result<String> {
    let mut list = String::from("\"_oo_pos\"");
    for column in batch.table.columns {
        let plan = plan_mapping(&MapRequest {
            source: SOURCE_DORIS.to_string(),
            config_digest: batch.config_digest.clone(),
            run_generation: generation,
            level: Level::Column,
            namespace: NAMESPACE.to_string(),
            table: batch.table.name.to_string(),
            column: column.name.to_string(),
        })?;
        list.push_str(", ");
        list.push_str(&quote_physical(&plan.physical_name)?);
    }
    Ok(format!(
        "COPY _oo.{} ({list}) FROM STDIN WITH (FORMAT binary)",
        quote_physical(table_physical)?
    ))
}

#[allow(clippy::too_many_arguments)]
async fn finish_window(
    control: &mut Client,
    claimed: &Claimed,
    binding: &Binding,
    digest: &str,
    pos: &str,
    copied: bool,
    seal: &str,
    key: &str,
) -> Result<()> {
    let tx = control.transaction().await.map_err(Error::sql)?;
    tx.execute(
        "INSERT INTO jw.capture_watermarks (
           binding_id, project_id, source_generation, artifact_sha, receipt_key,
           payload_digest, applied_pos, copied, seal_state, task_id, generation, advanced_at
         ) VALUES (
           $1::text::uuid, $2::text::uuid, $3, $4, $5, $6, $7::text::numeric, $8, $9, $10::text::uuid, $11, clock_timestamp()
         )
         ON CONFLICT (binding_id) DO UPDATE SET
           source_generation = GREATEST(jw.capture_watermarks.source_generation, EXCLUDED.source_generation),
           artifact_sha = CASE
             WHEN EXCLUDED.source_generation >= jw.capture_watermarks.source_generation THEN EXCLUDED.artifact_sha
             ELSE jw.capture_watermarks.artifact_sha END,
           receipt_key = CASE
             WHEN EXCLUDED.source_generation >= jw.capture_watermarks.source_generation THEN EXCLUDED.receipt_key
             ELSE jw.capture_watermarks.receipt_key END,
           payload_digest = CASE
             WHEN EXCLUDED.source_generation >= jw.capture_watermarks.source_generation THEN EXCLUDED.payload_digest
             ELSE jw.capture_watermarks.payload_digest END,
           applied_pos = CASE
             WHEN EXCLUDED.source_generation >= jw.capture_watermarks.source_generation THEN EXCLUDED.applied_pos
             ELSE jw.capture_watermarks.applied_pos END,
           copied = jw.capture_watermarks.copied OR EXCLUDED.copied,
           seal_state = CASE
             WHEN jw.capture_watermarks.seal_state = 'awaiting_publish' THEN jw.capture_watermarks.seal_state
             ELSE EXCLUDED.seal_state END,
           task_id = EXCLUDED.task_id,
           generation = EXCLUDED.generation,
           advanced_at = clock_timestamp()",
        &[
            &binding.id,
            &CAPTURE_PROJECT,
            &binding.source_generation,
            &binding.artifact_sha,
            &key,
            &digest,
            &pos,
            &copied,
            &seal,
            &claimed.task_id,
            &claimed.generation,
        ],
    )
    .await
    .map_err(Error::sql)?;
    let done = lease::complete_success(
        &tx,
        CAPTURE_PROJECT,
        &claimed.task_id,
        &claimed.owner,
        claimed.generation,
    )
    .await?;
    if done.status != LeaseStatus::Succeeded {
        tx.rollback().await.map_err(Error::sql)?;
        println!(
            "step=lease status={} task={} watermark=rolled_back",
            done.status.as_str(),
            claimed.task_id
        );
        return Err(Error::new("lease"));
    }
    tx.commit().await.map_err(Error::sql)?;
    println!(
        "step=succeeded task={} copied={copied} seal={seal} receipt={key} watermark=advanced generation={}",
        claimed.task_id, binding.source_generation
    );
    Ok(())
}

async fn fail_task(control: &mut Client, claimed: &Claimed, error_kind: &str, reason: &str) -> Result<()> {
    let parsed = crate::capture_plan::CapturePayload::parse(&claimed.payload).ok();
    let tx = control.transaction().await.map_err(Error::sql)?;
    if let Some(parsed) = parsed
        && reason_ok(reason)
    {
        tx.execute(
            "INSERT INTO jw.capture_refusals (binding_id, pin, reason) VALUES ($1::text::uuid, $2, $3)",
            &[&parsed.binding, &parsed.pin, &reason],
        )
        .await
        .map_err(Error::sql)?;
    }
    let result = lease::complete_failure(
        &tx,
        CAPTURE_PROJECT,
        &claimed.task_id,
        &claimed.owner,
        claimed.generation,
        error_kind,
    )
    .await?;
    if result.status != LeaseStatus::Failed {
        tx.rollback().await.map_err(Error::sql)?;
        return Err(Error::new("lease"));
    }
    tx.commit().await.map_err(Error::sql)?;
    println!(
        "step=failed reason={reason} status=failed task={} watermark=absent",
        claimed.task_id
    );
    Ok(())
}

async fn claim_or_resume(client: &mut Client, worker: &str) -> Result<Option<Claimed>> {
    let existing = client
        .query_opt(
            "SELECT id::text, generation, lease_owner, payload::text
             FROM jobs.tasks
             WHERE project_id = $1::text::uuid AND kind = $2 AND state = 'running'
               AND lease_owner = $3 AND lease_until > clock_timestamp()
             ORDER BY id LIMIT 1",
            &[&CAPTURE_PROJECT, &CAPTURE_KIND, &worker],
        )
        .await
        .map_err(Error::sql)?;
    if let Some(row) = existing {
        return Ok(Some(Claimed {
            task_id: row.get(0),
            generation: row.get(1),
            owner: row.get(2),
            payload: row.get(3),
        }));
    }
    let tx = client.transaction().await.map_err(Error::sql)?;
    let kinds = vec![CAPTURE_KIND.to_string()];
    let claimed = lease::claim(&tx, CAPTURE_PROJECT, &kinds, worker).await?;
    if claimed.status == LeaseStatus::None {
        tx.commit().await.map_err(Error::sql)?;
        return Ok(None);
    }
    if claimed.status != LeaseStatus::Claimed {
        tx.rollback().await.map_err(Error::sql)?;
        return Err(Error::new("claim"));
    }
    let task_id = claimed.task_id.ok_or_else(|| Error::new("claim"))?;
    let generation = claimed.generation.ok_or_else(|| Error::new("claim"))?;
    let owner = claimed.lease_owner.ok_or_else(|| Error::new("claim"))?;
    let marked: String = tx
        .query_one(sql::INSERT_CLAIM, &[&task_id, &CAPTURE_PROJECT, &owner, &generation])
        .await
        .map_err(Error::sql)?
        .get(0);
    if marked != task_id {
        tx.rollback().await.map_err(Error::sql)?;
        return Err(Error::new("claim"));
    }
    let payload: String = tx
        .query_one(sql::READ_WORK, &[&task_id, &CAPTURE_PROJECT, &owner, &generation])
        .await
        .map_err(Error::sql)?
        .get(0);
    tx.commit().await.map_err(Error::sql)?;
    println!("claimed task={task_id} generation={generation} owner={owner}");
    Ok(Some(Claimed {
        task_id,
        generation,
        owner,
        payload,
    }))
}

async fn beat_if_due(client: &mut Client, claimed: &Claimed) -> Result<()> {
    let due: bool = client
        .query_one(
            sql::HEARTBEAT_DUE,
            &[&claimed.task_id, &claimed.generation, &(HEARTBEAT_SECONDS as f64)],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    if !due {
        return Ok(());
    }
    let tx = client.transaction().await.map_err(Error::sql)?;
    let beat = lease::heartbeat(
        &tx,
        CAPTURE_PROJECT,
        &claimed.task_id,
        &claimed.owner,
        claimed.generation,
    )
    .await?;
    if beat.status != LeaseStatus::Extended {
        tx.rollback().await.map_err(Error::sql)?;
        println!("heartbeat={}", beat.status.as_str());
        return Err(Error::new("lease"));
    }
    tx.execute(
        sql::INSERT_BEAT,
        &[&claimed.task_id, &CAPTURE_PROJECT, &claimed.owner, &claimed.generation],
    )
    .await
    .map_err(Error::sql)?;
    tx.commit().await.map_err(Error::sql)?;
    println!("heartbeat=extended task={}", claimed.task_id);
    Ok(())
}

async fn hold(config: &Config, worker: &str) -> Result<()> {
    worker_ok(worker)?;
    let mut client = session::runtime(config).await?;
    let Some(claimed) = claim_or_resume(&mut client, worker).await? else {
        println!("hold=none");
        return Ok(());
    };
    println!(
        "hold=running task={} generation={} owner={} payload={}",
        claimed.task_id, claimed.generation, claimed.owner, claimed.payload
    );
    Ok(())
}

async fn beat(config: &Config, flags: &[(String, String)]) -> Result<()> {
    let task = required(flags, "task")?;
    let worker = required(flags, "worker")?;
    let generation = required(flags, "generation")?.parse::<i64>().map_err(|_| Error::new("input"))?;
    worker_ok(&worker)?;
    if !crate::api::is_uuid(&task) {
        return Err(Error::new("input"));
    }
    let mut client = session::runtime(config).await?;
    let tx = client.transaction().await.map_err(Error::sql)?;
    let beat = lease::heartbeat(&tx, CAPTURE_PROJECT, &task, &worker, generation).await?;
    if beat.status == LeaseStatus::Extended {
        tx.execute(sql::INSERT_BEAT, &[&task, &CAPTURE_PROJECT, &worker, &generation])
            .await
            .map_err(Error::sql)?;
    }
    tx.commit().await.map_err(Error::sql)?;
    println!("heartbeat={}", beat.status.as_str());
    Ok(())
}

async fn finish_old(config: &Config, flags: &[(String, String)]) -> Result<()> {
    let task = required(flags, "task")?;
    let worker = required(flags, "worker")?;
    let generation = required(flags, "generation")?.parse::<i64>().map_err(|_| Error::new("input"))?;
    worker_ok(&worker)?;
    if !crate::api::is_uuid(&task) {
        return Err(Error::new("input"));
    }
    let mut client = session::runtime(config).await?;
    let tx = client.transaction().await.map_err(Error::sql)?;
    let done = lease::complete_success(&tx, CAPTURE_PROJECT, &task, &worker, generation).await?;
    tx.commit().await.map_err(Error::sql)?;
    println!("old_owner={}", done.status.as_str());
    println!("watermark=not_written");
    if done.status == LeaseStatus::Succeeded {
        return Err(Error::new("lease"));
    }
    Ok(())
}

async fn reject_payloads(config: &Config) -> Result<()> {
    let binding = "d101fd4a-3a89-41f6-b7fe-94b4001d5512";
    let argv = format!(r#"{{"v":1,"binding":"{binding}","pin":1,"argv":["/bin/sh"]}}"#);
    let extra = format!(r#"{{"v":1,"binding":"{binding}","pin":1,"artifact":"x"}}"#);
    println!(
        "reject_argv={}",
        crate::capture_plan::CapturePayload::parse(&argv).is_err()
    );
    println!(
        "reject_extra={}",
        crate::capture_plan::CapturePayload::parse(&extra).is_err()
    );
    let mut client = session::runtime(config).await?;
    let tx = client.transaction().await.map_err(Error::sql)?;
    let secret_payload = r#"{"v":1,"password":"0123456789abcdef"}"#;
    let err = lease::enqueue(
        &tx,
        Enqueue {
            project_id: CAPTURE_PROJECT,
            kind: CAPTURE_KIND,
            payload_version: 1,
            payload: secret_payload,
            config_version: CAPTURE_CONFIG,
            digest: &sha256_hex(secret_payload.as_bytes()),
            dedupe_key: "reject-secret",
            run_delay_seconds: 0.0,
            max_attempts: 1,
        },
    )
    .await;
    tx.rollback().await.map_err(Error::sql)?;
    match err {
        Err(err) => println!(
            "reject_secret=true sqlstate={}",
            err.state.unwrap_or_default()
        ),
        Ok(_) => {
            println!("reject_secret=false");
            return Err(Error::new("payload"));
        }
    }
    Ok(())
}

async fn fence_role(config: &Config) -> Result<()> {
    let mut client = session::runtime(config).await?;
    let lease_id = "d101fd4a-74c0-41f6-b7fe-94b4001d5512";
    let tx = client.transaction().await.map_err(Error::sql)?;
    tx.execute(
        "INSERT INTO jobs.leases (id, project_id, target_key)
         VALUES ($1::text::uuid, $2::text::uuid, 'capture-window')
         ON CONFLICT (project_id, target_key) DO NOTHING",
        &[&lease_id, &CAPTURE_PROJECT],
    )
    .await
    .map_err(Error::sql)?;
    let update = tx
        .execute(
            "UPDATE jobs.leases SET target_fence_certified = true WHERE project_id = $1::text::uuid AND target_key = 'capture-window'",
            &[&CAPTURE_PROJECT],
        )
        .await;
    match update {
        Err(err) => {
            let mapped = Error::sql(err);
            tx.rollback().await.map_err(Error::sql)?;
            println!(
                "fence_update=rejected sqlstate={}",
                mapped.state.unwrap_or_default()
            );
        }
        Ok(_) => {
            tx.rollback().await.map_err(Error::sql)?;
            println!("fence_update=accepted");
            return Err(Error::new("fence"));
        }
    }
    let certified: i64 = client
        .query_one(
            "SELECT count(*)::bigint FROM jobs.leases WHERE target_fence_certified",
            &[],
        )
        .await
        .map_err(Error::sql)?
        .get(0);
    println!("target_fence_true={certified}");
    if certified != 0 {
        return Err(Error::new("fence"));
    }
    Ok(())
}

async fn show(config: &Config, flags: &[(String, String)]) -> Result<()> {
    let client = session::runtime(config).await?;
    let rows = client
        .query(
            "SELECT b.id::text, b.table_name, api.period_ms, b.current_pin, s.dedupe_prefix,
                    api.enabled, s.next_run_at::text,
                    COALESCE((
                      SELECT count(*) FROM jobs.schedule_occurrences o WHERE o.schedule_id = s.id
                    ), 0)::bigint,
                    COALESCE((
                      SELECT count(*) FROM jobs.schedule_occurrences o
                      JOIN jobs.tasks t ON t.id = o.task_id
                      WHERE o.schedule_id = s.id AND t.state IN ('queued', 'running')
                    ), 0)::bigint,
                    api.payload::text
             FROM jw.capture_bindings b
             JOIN jobs.schedules s ON s.id = b.schedule_id
             JOIN jw.schedule_api api ON api.id = s.id
             WHERE b.project_id = $1::text::uuid
             ORDER BY b.table_name, b.id",
            &[&CAPTURE_PROJECT],
        )
        .await
        .map_err(Error::sql)?;
    for row in rows {
        let id: String = row.get(0);
        let table: String = row.get(1);
        let period: i32 = row.get(2);
        let pin: i64 = row.get(3);
        let dedupe: String = row.get(4);
        let enabled: bool = row.get(5);
        let next: String = row.get(6);
        let occurrences: i64 = row.get(7);
        let busy: i64 = row.get(8);
        let payload: String = row.get(9);
        let payload = if payload.contains("secret://") || payload.contains("reader.password") || payload.contains("secret_ref") {
            "rejected".to_string()
        } else {
            payload
        };
        println!(
            "binding={id} table={table} period_ms={period} pin={pin} dedupe={dedupe} enabled={enabled} next={next} occurrences={occurrences} busy={busy} payload={payload}"
        );
    }
    let marks = client
        .query(
            "SELECT binding_id::text, source_generation, receipt_key, copied, seal_state, artifact_sha
             FROM jw.capture_watermarks WHERE project_id = $1::text::uuid ORDER BY binding_id",
            &[&CAPTURE_PROJECT],
        )
        .await
        .map_err(Error::sql)?;
    for row in marks {
        let binding: String = row.get(0);
        let generation: i64 = row.get(1);
        let key: String = row.get(2);
        let copied: bool = row.get(3);
        let seal: String = row.get(4);
        let sha: String = row.get(5);
        println!("watermark binding={binding} generation={generation} copied={copied} seal={seal} receipt={key} artifact={sha}");
    }
    let refusals = client
        .query(
            "SELECT binding_id::text, pin, reason FROM jw.capture_refusals ORDER BY id",
            &[],
        )
        .await
        .map_err(Error::sql)?;
    for row in refusals {
        let binding: String = row.get(0);
        let pin: i64 = row.get(1);
        let reason: String = row.get(2);
        println!("refusal binding={binding} pin={pin} reason={reason}");
    }
    let tasks = client
        .query(
            "SELECT id::text, state, generation, COALESCE(lease_owner, ''), payload::text
             FROM jobs.tasks WHERE project_id = $1::text::uuid AND kind = $2 ORDER BY id",
            &[&CAPTURE_PROJECT, &CAPTURE_KIND],
        )
        .await
        .map_err(Error::sql)?;
    for row in tasks {
        let id: String = row.get(0);
        let state: String = row.get(1);
        let generation: i64 = row.get(2);
        let owner: String = row.get(3);
        let payload: String = row.get(4);
        println!("task={id} state={state} generation={generation} owner={owner} payload={payload}");
    }
    if let Some(physical) = optional(flags, "physical") {
        if !is_physical_name(&physical) {
            return Err(Error::new("input"));
        }
        let capture = config.capture()?;
        let admin = session::admin_db(config, &capture.target_database).await?;
        let quoted = quote_physical(&physical)?;
        let count: i64 = admin
            .query_one(&format!("SELECT count(*)::bigint FROM _oo.{quoted}"), &[])
            .await
            .map_err(Error::sql)?
            .get(0);
        println!("shadow physical={physical} rows={count}");
    }
    Ok(())
}

async fn load_binding(client: &Client, id: &str, pin: i64) -> Result<Binding> {
    let row = client
        .query_one(
            "SELECT b.id::text, b.source, b.driver_sha, b.artifact_rel, b.secret_ref, b.table_name,
                    b.shard, b.fence_owner::text, b.row_budget, b.byte_budget,
                    p.artifact_sha, p.source_generation
             FROM jw.capture_bindings b
             JOIN jw.capture_pins p ON p.binding_id = b.id AND p.pin = $2
             WHERE b.id = $1::text::uuid AND b.project_id = $3::text::uuid",
            &[&id, &pin, &CAPTURE_PROJECT],
        )
        .await
        .map_err(Error::sql)?;
    Ok(Binding {
        id: row.get(0),
        source: row.get(1),
        driver_sha: row.get(2),
        artifact_rel: row.get(3),
        secret_ref: row.get(4),
        table_name: row.get(5),
        shard: row.get(6),
        fence_owner: row.get(7),
        row_budget: row.get(8),
        byte_budget: row.get(9),
        artifact_sha: row.get(10),
        source_generation: row.get(11),
    })
}

fn prepare_input(root: &Path, binding: &Binding) -> std::result::Result<(), &'static str> {
    if !driver_pinned(&binding.source, &binding.driver_sha) {
        return Err("driver_unpinned");
    }
    if check_secret_ref(Path::new(&binding.secret_ref)).is_err() {
        return Err("secret_ref_rejected");
    }
    if resolve_artifact(root, &binding.artifact_rel).is_err() {
        return Err("artifact_mismatch");
    }
    Ok(())
}

async fn armed_fault(client: &Client, binding: &str) -> Result<Option<String>> {
    let row = client
        .query_opt(
            "SELECT kind FROM jw.capture_faults WHERE binding_id = $1::text::uuid AND armed",
            &[&binding],
        )
        .await
        .map_err(Error::sql)?;
    Ok(row.map(|row| row.get(0)))
}

async fn disarm(client: &Client, binding: &str) -> Result<()> {
    client
        .execute(
            "UPDATE jw.capture_faults SET armed = false WHERE binding_id = $1::text::uuid AND armed",
            &[&binding],
        )
        .await
        .map_err(Error::sql)?;
    Ok(())
}

fn resolve_artifact(root: &Path, rel: &str) -> Result<PathBuf> {
    if rel.is_empty()
        || rel.starts_with('/')
        || rel.contains("..")
        || rel.contains('\\')
        || rel.contains('\0')
    {
        return Err(Error::new("input"));
    }
    let root = root.canonicalize().map_err(|_| Error::new("capture"))?;
    let path = root.join(rel);
    let full = path.canonicalize().map_err(|_| Error::new("capture"))?;
    if !full.starts_with(&root) {
        return Err(Error::new("input"));
    }
    Ok(full)
}

fn check_secret_ref(path: &Path) -> Result<()> {
    let text = path.to_str().ok_or_else(|| Error::new("secret"))?;
    if text.contains("..") || text.contains('\0') {
        return Err(Error::new("secret"));
    }
    let allowed = text.starts_with("/secrets/") || !text.starts_with('/');
    if !allowed {
        return Err(Error::new("secret"));
    }
    let mut body = std::fs::read_to_string(path).map_err(|_| Error::new("secret"))?;
    if body.ends_with('\n') {
        body.pop();
    }
    if !secret_shape(&body) {
        return Err(Error::new("secret"));
    }
    Ok(())
}

fn owned_name(name: &str) -> Result<()> {
    let bytes = name.as_bytes();
    if bytes.len() < 9
        || !bytes.starts_with(b"oo_d101_")
        || !bytes[8..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
    {
        return Err(Error::new("config"));
    }
    Ok(())
}

fn shard_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || *byte == b'_'
                || *byte == b'.'
                || *byte == b':'
                || *byte == b'-'
        })
}

fn table_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn worker_ok(name: &str) -> Result<()> {
    let bytes = name.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 128
        || !bytes.iter().all(|byte| {
            byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'.' || *byte == b':' || *byte == b'-'
        })
    {
        return Err(Error::new("input"));
    }
    Ok(())
}

fn reason_ok(reason: &str) -> bool {
    let bytes = reason.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn required(flags: &[(String, String)], name: &str) -> Result<String> {
    flags
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
        .ok_or_else(|| Error::new("usage"))
}

fn optional(flags: &[(String, String)], name: &str) -> Option<String> {
    flags
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
}

fn read_hashed(dir: &Path, name: &str, expected: &str) -> Result<String> {
    if name.contains('/') || name.contains("..") {
        return Err(Error::new("input"));
    }
    let bytes = std::fs::read(dir.join(name)).map_err(|_| Error::new("hash"))?;
    if sha256_hex(&bytes) != expected {
        return Err(Error::new("hash"));
    }
    String::from_utf8(bytes).map_err(|_| Error::new("hash"))
}
