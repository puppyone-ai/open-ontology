//! One connection per operation. Jobs and their create audit share one transaction.

use std::time::Duration;

use tokio_postgres::config::SslMode;
use tokio_postgres::error::SqlState;
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;
use tokio_postgres::Config;
use tokio_postgres_rustls::MakeRustlsConnect;
use zeroize::Zeroize;

use crate::canonical::sha256_hex;
use crate::config::Endpoint;
use crate::error::ApiError;
use crate::tlsutil::client_config;
use crate::MIGRATION_SQL;

pub const STAGES: [&str; 11] = ["S0", "S1", "S2", "S3", "S4", "S5", "S6", "S7", "S8", "S9", "S10"];

#[derive(Clone, Debug)]
pub struct BuildRow {
    pub build_id: String,
    pub project_id: String,
    pub status: String,
    pub stage: String,
    pub identity_revision: i64,
    pub input_sha256: String,
    pub failure_code: Option<String>,
    pub body_sha256: String,
    pub config_commit: String,
    pub source_ref: String,
    pub allowance_id: String,
    pub subject: String,
    pub role_name: String,
    pub child_exited: bool,
    pub child_pid: Option<i64>,
    pub lease_generation: i64,
}

#[derive(Clone, Debug)]
pub struct StageRow {
    pub stage: String,
    pub status: String,
    pub input_sha256: String,
    pub output_sha256: Option<String>,
    pub detail_code: Option<String>,
}

#[derive(Clone, Debug)]
pub struct NewBuild {
    pub build_id: String,
    pub project_id: String,
    pub idempotency_key: String,
    pub body_sha256: String,
    pub config_commit: String,
    pub source_ref: String,
    pub allowance_id: String,
    pub llm_secret_ref: String,
    pub identity_revision: i64,
    pub subject: String,
    pub role_name: String,
    pub input_sha256: String,
}

#[derive(Debug)]
pub enum CreateOutcome {
    Created(BuildRow),
    Replay(BuildRow),
    Conflict,
    Unknown,
}

struct Secret(Vec<u8>);

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub async fn connect(endpoint: &Endpoint) -> Result<Client, ApiError> {
    let mut secret = read_secret(&endpoint.password_file)?;
    if secret.0.ends_with(b"\n") {
        secret.0.pop();
        if secret.0.ends_with(b"\r") {
            secret.0.pop();
        }
    }
    if secret.0.is_empty() || secret.0.contains(&0) {
        return Err(ApiError::not_ready());
    }
    let password = std::str::from_utf8(&secret.0).map_err(|_| ApiError::not_ready())?;
    let ca = std::fs::read(&endpoint.ca_file).map_err(|_| ApiError::not_ready())?;
    let tls_config = client_config(&ca).map_err(|_| ApiError::not_ready())?;
    let mut builder = Config::new();
    builder
        .host(&endpoint.host)
        .port(endpoint.port)
        .dbname(&endpoint.database)
        .user(&endpoint.user)
        .password(password)
        .ssl_mode(SslMode::Require)
        .connect_timeout(Duration::from_secs(20));
    let tls = MakeRustlsConnect::new(tls_config);
    let (client, task) = builder.connect(tls).await.map_err(|_| ApiError::not_ready())?;
    tokio::spawn(async move {
        let _ = task.await;
    });
    Ok(client)
}

pub async fn install_schema(endpoint: &Endpoint) -> Result<(), ApiError> {
    let mut client = connect(endpoint).await?;
    let hash = sha256_hex(MIGRATION_SQL.as_bytes());
    match client
        .query_opt("SELECT sql_sha256 FROM oo_selfbuild_api.install_manifest WHERE singleton", &[])
        .await
    {
        Ok(Some(row)) => {
            let stored: String = row.get(0);
            if stored == hash {
                Ok(())
            } else {
                Err(ApiError::new(409, "membership.refused", "the install hash drifted", false))
            }
        }
        Ok(None) => apply_migration(&mut client, &hash).await,
        Err(error) if missing_schema(&error) => apply_migration(&mut client, &hash).await,
        Err(_) => Err(ApiError::not_ready()),
    }
}

async fn apply_migration(client: &mut Client, hash: &str) -> Result<(), ApiError> {
    let tx = client.transaction().await.map_err(|_| ApiError::not_ready())?;
    tx.batch_execute("SET LOCAL synchronous_commit = on")
        .await
        .map_err(|_| ApiError::not_ready())?;
    tx.batch_execute(MIGRATION_SQL).await.map_err(|_| ApiError::not_ready())?;
    tx.execute(
        "INSERT INTO oo_selfbuild_api.install_manifest (singleton, sql_sha256) VALUES (true, $1)",
        &[&hash],
    )
    .await
    .map_err(|_| ApiError::not_ready())?;
    tx.commit().await.map_err(|_| ApiError::commit_unknown())
}

pub async fn create_build(endpoint: &Endpoint, job: &NewBuild) -> Result<CreateOutcome, ApiError> {
    let mut client = connect(endpoint).await?;
    let tx = client.transaction().await.map_err(|_| ApiError::not_ready())?;
    if tx.batch_execute("SET LOCAL synchronous_commit = on").await.is_err() {
        return Err(ApiError::not_ready());
    }
    let params: [&(dyn ToSql + Sync); 12] = [
        &job.build_id,
        &job.project_id,
        &job.idempotency_key,
        &job.body_sha256,
        &job.config_commit,
        &job.source_ref,
        &job.allowance_id,
        &job.llm_secret_ref,
        &job.identity_revision,
        &job.subject,
        &job.role_name,
        &job.input_sha256,
    ];
    let inserted = tx
        .execute(
            "INSERT INTO oo_selfbuild_api.build_job (
                build_id, project_id, idempotency_key, body_sha256, config_commit, source_ref,
                allowance_id, llm_secret_ref, identity_revision, subject, role_name, status, stage, input_sha256
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'queued', 'S0', $12
            )",
            &params,
        )
        .await;
    if let Err(error) = inserted {
        drop(tx);
        if error.code() == Some(&SqlState::UNIQUE_VIOLATION) {
            return replay_or_conflict(&client, job).await;
        }
        return Err(ApiError::not_ready());
    }
    for stage in STAGES {
        let status = if stage == "S9" || stage == "S10" { "awaiting" } else { "pending" };
        if tx
            .execute(
                "INSERT INTO oo_selfbuild_api.stage_checkpoint (build_id, stage, status, input_sha256)
                 VALUES ($1, $2, $3, $4)",
                &[&job.build_id, &stage, &status, &job.input_sha256],
            )
            .await
            .is_err()
        {
            return Err(ApiError::not_ready());
        }
    }
    if tx
        .execute(
            "INSERT INTO oo_selfbuild_api.audit_event (project_id, build_id, action, subject, detail_code, body_sha256)
             VALUES ($1, $2, 'create', $3, 'queued', $4)",
            &[&job.project_id, &job.build_id, &job.subject, &job.body_sha256],
        )
        .await
        .is_err()
    {
        return Err(ApiError::not_ready());
    }
    match tx.commit().await {
        Ok(()) => load_build(&client, &job.project_id, &job.build_id)
            .await
            .map(CreateOutcome::Created),
        Err(_) => Err(ApiError::commit_unknown()),
    }
}

async fn replay_or_conflict(client: &Client, job: &NewBuild) -> Result<CreateOutcome, ApiError> {
    let row = client
        .query_one(
            "SELECT body_sha256, build_id FROM oo_selfbuild_api.build_job
             WHERE project_id = $1 AND idempotency_key = $2",
            &[&job.project_id, &job.idempotency_key],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    let body: String = row.get(0);
    let build_id: String = row.get(1);
    if body != job.body_sha256 {
        return Ok(CreateOutcome::Conflict);
    }
    load_build(client, &job.project_id, &build_id)
        .await
        .map(CreateOutcome::Replay)
}

pub async fn load_build(client: &Client, project_id: &str, build_id: &str) -> Result<BuildRow, ApiError> {
    let row = client
        .query_opt(
            "SELECT build_id, project_id, status, stage, identity_revision, input_sha256, failure_code,
                    body_sha256, config_commit, source_ref, allowance_id, subject, role_name,
                    child_exited, child_pid, lease_generation
             FROM oo_selfbuild_api.build_job
             WHERE build_id = $1 AND project_id = $2",
            &[&build_id, &project_id],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    row.map(build_from).ok_or_else(ApiError::forbidden)
}

pub async fn load_build_any(endpoint: &Endpoint, build_id: &str) -> Result<Option<BuildRow>, ApiError> {
    let client = connect(endpoint).await?;
    let row = client
        .query_opt(
            "SELECT build_id, project_id, status, stage, identity_revision, input_sha256, failure_code,
                    body_sha256, config_commit, source_ref, allowance_id, subject, role_name,
                    child_exited, child_pid, lease_generation
             FROM oo_selfbuild_api.build_job WHERE build_id = $1",
            &[&build_id],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(row.map(build_from))
}

pub async fn list_builds(endpoint: &Endpoint, project_id: &str) -> Result<Vec<BuildRow>, ApiError> {
    let client = connect(endpoint).await?;
    let rows = client
        .query(
            "SELECT build_id, project_id, status, stage, identity_revision, input_sha256, failure_code,
                    body_sha256, config_commit, source_ref, allowance_id, subject, role_name,
                    child_exited, child_pid, lease_generation
             FROM oo_selfbuild_api.build_job WHERE project_id = $1 ORDER BY created_at",
            &[&project_id],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(rows.into_iter().map(build_from).collect())
}

pub async fn stage_rows(client: &Client, build_id: &str) -> Result<Vec<StageRow>, ApiError> {
    let rows = client
        .query(
            "SELECT stage, status, input_sha256, output_sha256, detail_code
             FROM oo_selfbuild_api.stage_checkpoint WHERE build_id = $1 ORDER BY stage",
            &[&build_id],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(rows.into_iter().map(stage_from).collect())
}

pub async fn stages(endpoint: &Endpoint, build_id: &str) -> Result<Vec<StageRow>, ApiError> {
    let client = connect(endpoint).await?;
    stage_rows(&client, build_id).await
}

pub async fn insert_member(client: &Client, build_id: &str, member: &crate::membership::Member) -> Result<(), ApiError> {
    let params: [&(dyn ToSql + Sync); 12] = [
        &build_id,
        &member.project_id,
        &member.object_id,
        &member.object_kind,
        &member.schema_name,
        &member.table_name,
        &member.column_name,
        &member.candidate_sha256,
        &member.source_sha256,
        &member.schema_sha256,
        &member.evidence_sha256,
        &member.generation_sha256,
    ];
    client
        .execute(
            "INSERT INTO oo_selfbuild_api.member (
                build_id, project_id, object_id, object_kind, schema_name, table_name, column_name,
                candidate_sha256, source_sha256, schema_sha256, evidence_sha256, generation_sha256
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (build_id, object_id) DO NOTHING",
            &params,
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(())
}

pub async fn count_jobs(endpoint: &Endpoint) -> Result<i64, ApiError> {
    let client = connect(endpoint).await?;
    let row = client
        .query_one("SELECT count(*) FROM oo_selfbuild_api.build_job", &[])
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(row.get(0))
}

pub async fn request_cancel(endpoint: &Endpoint, build_id: &str, subject: &str) -> Result<BuildRow, ApiError> {
    let mut client = connect(endpoint).await?;
    let current = client
        .query_opt(
            "SELECT status, project_id FROM oo_selfbuild_api.build_job WHERE build_id = $1",
            &[&build_id],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    let Some(row) = current else {
        return Err(ApiError::forbidden());
    };
    let status: String = row.get(0);
    let project_id: String = row.get(1);
    if !matches!(status.as_str(), "queued" | "running") {
        return load_build(&client, &project_id, build_id).await;
    }
    let tx = client.transaction().await.map_err(|_| ApiError::not_ready())?;
    tx.batch_execute("SET LOCAL synchronous_commit = on")
        .await
        .map_err(|_| ApiError::not_ready())?;
    tx.execute(
        "UPDATE oo_selfbuild_api.build_job SET status = 'cancel_requested', updated_at = now()
         WHERE build_id = $1 AND status IN ('queued', 'running')",
        &[&build_id],
    )
    .await
    .map_err(|_| ApiError::not_ready())?;
    tx.execute(
        "INSERT INTO oo_selfbuild_api.audit_event (project_id, build_id, action, subject, detail_code)
         VALUES ($1, $2, 'cancel', $3, 'cancel_requested')",
        &[&project_id, &build_id, &subject],
    )
    .await
    .map_err(|_| ApiError::not_ready())?;
    tx.commit().await.map_err(|_| ApiError::commit_unknown())?;
    load_build(&client, &project_id, build_id).await
}

pub async fn audit_update_fails(endpoint: &Endpoint) -> Result<(), ApiError> {
    let client = connect(endpoint).await?;
    client
        .execute(
            "INSERT INTO oo_selfbuild_api.audit_event (project_id, action, subject, detail_code)
             VALUES ('demo', 'probe', 'probe', 'probe')",
            &[],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    match client
        .batch_execute("UPDATE oo_selfbuild_api.audit_event SET detail_code = 'mutated'")
        .await
    {
        Err(error) if error.code().map(|code| code.code()) == Some("55000") => Ok(()),
        Err(error) if format!("{error}").contains("insert-only") => Ok(()),
        _ => Err(ApiError::not_ready()),
    }
}

pub async fn claim(client: &Client, owner: &str) -> Result<Option<BuildRow>, ApiError> {
    let row = client
        .query_opt(
            "UPDATE oo_selfbuild_api.build_job AS j
             SET status = CASE WHEN j.status = 'cancel_requested' THEN 'cancel_requested' ELSE 'running' END,
                 lease_owner = $1,
                 lease_until = now() + interval '30 seconds',
                 lease_generation = j.lease_generation + 1,
                 updated_at = now()
             WHERE j.build_id = (
                 SELECT b.build_id FROM oo_selfbuild_api.build_job b
                 WHERE b.status IN ('queued', 'running', 'cancel_requested')
                   AND (b.lease_until IS NULL OR b.lease_until < now())
                   AND NOT (
                       b.status = 'running'
                       AND EXISTS (
                           SELECT 1 FROM oo_selfbuild_api.stage_checkpoint s
                           WHERE s.build_id = b.build_id AND s.stage = 'S9' AND s.status = 'awaiting'
                       )
                       AND NOT EXISTS (
                           SELECT 1 FROM oo_selfbuild_api.stage_checkpoint s
                           WHERE s.build_id = b.build_id
                             AND s.stage IN ('S0', 'S1', 'S2', 'S3', 'S4', 'S5', 'S6', 'S7', 'S8')
                             AND s.status IN ('pending', 'running', 'failed', 'refused')
                       )
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM oo_selfbuild_api.build_job o
                       WHERE o.project_id = b.project_id
                         AND o.status = 'running'
                         AND o.lease_until > now()
                         AND o.build_id <> b.build_id
                   )
                 ORDER BY b.created_at
                 FOR UPDATE SKIP LOCKED
                 LIMIT 1
             )
             RETURNING build_id, project_id, status, stage, identity_revision, input_sha256, failure_code,
                       body_sha256, config_commit, source_ref, allowance_id, subject, role_name,
                       child_exited, child_pid, lease_generation",
            &[&owner],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(row.map(build_from))
}

pub async fn heartbeat(client: &Client, build_id: &str, generation: i64) -> Result<bool, ApiError> {
    let params: [&(dyn ToSql + Sync); 2] = [&build_id, &generation];
    let count = client
        .execute(
            "UPDATE oo_selfbuild_api.build_job
             SET lease_until = now() + interval '30 seconds', updated_at = now()
             WHERE build_id = $1 AND lease_generation = $2",
            &params,
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(count == 1)
}

pub async fn set_child(client: &Client, build_id: &str, pid: i64) -> Result<(), ApiError> {
    let params: [&(dyn ToSql + Sync); 2] = [&build_id, &pid];
    client
        .execute(
            "UPDATE oo_selfbuild_api.build_job SET child_pid = $2, child_exited = false, updated_at = now()
             WHERE build_id = $1",
            &params,
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(())
}

pub async fn mark_child_exited(client: &Client, build_id: &str) -> Result<(), ApiError> {
    client
        .execute(
            "UPDATE oo_selfbuild_api.build_job
             SET child_exited = true, child_pid = NULL, updated_at = now()
             WHERE build_id = $1",
            &[&build_id],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(())
}

pub async fn finish_canceled(client: &Client, build_id: &str) -> Result<bool, ApiError> {
    let count = client
        .execute(
            "UPDATE oo_selfbuild_api.build_job
             SET status = 'canceled', updated_at = now()
             WHERE build_id = $1 AND status = 'cancel_requested' AND child_exited = true",
            &[&build_id],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(count == 1)
}

pub async fn set_stage(
    client: &Client,
    build_id: &str,
    stage: &str,
    status: &str,
    input_sha256: &str,
    output_sha256: Option<&str>,
    detail_code: Option<&str>,
) -> Result<bool, ApiError> {
    let params: [&(dyn ToSql + Sync); 6] = [
        &build_id,
        &stage,
        &status,
        &output_sha256,
        &detail_code,
        &input_sha256,
    ];
    let count = client
        .execute(
            "UPDATE oo_selfbuild_api.stage_checkpoint
             SET status = $3, output_sha256 = $4, detail_code = $5
             WHERE build_id = $1 AND stage = $2 AND input_sha256 = $6",
            &params,
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(count == 1)
}

pub async fn set_job(
    client: &Client,
    build_id: &str,
    status: &str,
    stage: &str,
    failure_code: Option<&str>,
    input_sha256: &str,
) -> Result<bool, ApiError> {
    let params: [&(dyn ToSql + Sync); 5] = [&build_id, &status, &stage, &failure_code, &input_sha256];
    let count = client
        .execute(
            "UPDATE oo_selfbuild_api.build_job
             SET status = $2, stage = $3, failure_code = $4, updated_at = now()
             WHERE build_id = $1 AND input_sha256 = $5",
            &params,
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(count == 1)
}

fn build_from(row: tokio_postgres::Row) -> BuildRow {
    BuildRow {
        build_id: row.get(0),
        project_id: row.get(1),
        status: row.get(2),
        stage: row.get(3),
        identity_revision: row.get(4),
        input_sha256: row.get(5),
        failure_code: row.get(6),
        body_sha256: row.get(7),
        config_commit: row.get(8),
        source_ref: row.get(9),
        allowance_id: row.get(10),
        subject: row.get(11),
        role_name: row.get(12),
        child_exited: row.get(13),
        child_pid: row.get(14),
        lease_generation: row.get(15),
    }
}

fn stage_from(row: tokio_postgres::Row) -> StageRow {
    StageRow {
        stage: row.get(0),
        status: row.get(1),
        input_sha256: row.get(2),
        output_sha256: row.get(3),
        detail_code: row.get(4),
    }
}

fn missing_schema(error: &tokio_postgres::Error) -> bool {
    matches!(
        error.code().map(|code| code.code()),
        Some("42P01" | "3F000")
    )
}

fn read_secret(path: &str) -> Result<Secret, ApiError> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| ApiError::not_ready())?;
    if !meta.file_type().is_file() || meta.len() > 1026 {
        return Err(ApiError::not_ready());
    }
    Ok(Secret(std::fs::read(path).map_err(|_| ApiError::not_ready())?))
}

pub async fn list_members(endpoint: &Endpoint, build_id: &str) -> Result<Vec<crate::membership::Member>, ApiError> {
    let client = connect(endpoint).await?;
    let rows = client
        .query(
            "SELECT project_id, object_id, object_kind, schema_name, table_name, column_name,
                    candidate_sha256, source_sha256, schema_sha256, evidence_sha256, generation_sha256
             FROM oo_selfbuild_api.member WHERE build_id = $1 ORDER BY object_id",
            &[&build_id],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(rows
        .into_iter()
        .map(|row| crate::membership::Member {
            project_id: row.get(0),
            object_id: row.get(1),
            object_kind: row.get(2),
            schema_name: row.get(3),
            table_name: row.get(4),
            column_name: row.get(5),
            candidate_sha256: row.get(6),
            source_sha256: row.get(7),
            schema_sha256: row.get(8),
            evidence_sha256: row.get(9),
            generation_sha256: row.get(10),
        })
        .collect())
}

#[derive(Clone, Debug)]
pub struct StoredDecision {
    pub body_sha256: String,
}

#[derive(Clone, Debug)]
pub struct DecisionWrite {
    pub project_id: String,
    pub idempotency_key: String,
    pub build_id: String,
    pub object_id: String,
    pub decision_kind: String,
    pub choice: String,
    pub body_sha256: String,
    pub identity_revision: i64,
    pub subject: String,
    pub ledger_code: String,
}

#[derive(Clone, Debug)]
pub struct QueueHead {
    pub revision: i64,
    pub candidate_sha256: String,
    pub input_sha256: String,
    pub evidence_sha256: String,
    pub schema_sha256: String,
    pub curated_sha256: String,
    pub source_sha256: String,
}

#[derive(Clone, Debug)]
pub struct LedgerDecision {
    pub object_id: String,
    pub decision_kind: String,
    pub choice: String,
    pub reviewer_label: String,
    pub reviewer_at: String,
    pub fields_json: String,
    pub authenticated: bool,
}

#[derive(Clone, Debug)]
pub struct OurDecision {
    pub object_id: String,
    pub decision_kind: String,
    pub choice: String,
}

pub async fn find_decision(endpoint: &Endpoint, project_id: &str, idempotency_key: &str) -> Result<Option<StoredDecision>, ApiError> {
    let client = connect(endpoint).await?;
    let row = client
        .query_opt(
            "SELECT body_sha256 FROM oo_selfbuild_api.decision_record
             WHERE project_id = $1 AND idempotency_key = $2",
            &[&project_id, &idempotency_key],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(row.map(|row| StoredDecision { body_sha256: row.get(0) }))
}

pub async fn insert_decision(endpoint: &Endpoint, write: &DecisionWrite) -> Result<(), ApiError> {
    let mut client = connect(endpoint).await?;
    let tx = client.transaction().await.map_err(|_| ApiError::not_ready())?;
    tx.batch_execute("SET LOCAL synchronous_commit = on")
        .await
        .map_err(|_| ApiError::not_ready())?;
    let existing = tx
        .query_opt(
            "SELECT body_sha256 FROM oo_selfbuild_api.decision_record
             WHERE project_id = $1 AND idempotency_key = $2",
            &[&write.project_id, &write.idempotency_key],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    if let Some(row) = existing {
        let body: String = row.get(0);
        return if body == write.body_sha256 {
            Ok(())
        } else {
            Err(ApiError::idempotency())
        };
    }
    let authenticated = false;
    let identity = "verified";
    let params: [&(dyn ToSql + Sync); 12] = [
        &write.project_id,
        &write.idempotency_key,
        &write.build_id,
        &write.object_id,
        &write.decision_kind,
        &write.choice,
        &write.body_sha256,
        &write.identity_revision,
        &write.subject,
        &write.ledger_code,
        &authenticated,
        &identity,
    ];
    tx.execute(
        "INSERT INTO oo_selfbuild_api.decision_record (
            project_id, idempotency_key, build_id, object_id, decision_kind, choice,
            body_sha256, identity_revision, subject, ledger_code, ledger_authenticated, management_identity
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        &params,
    )
    .await
    .map_err(|_| ApiError::not_ready())?;
    tx.execute(
        "INSERT INTO oo_selfbuild_api.audit_event (project_id, build_id, action, subject, detail_code, body_sha256)
         VALUES ($1, $2, 'decide', $3, $4, $5)",
        &[&write.project_id, &write.build_id, &write.subject, &write.decision_kind, &write.body_sha256],
    )
    .await
    .map_err(|_| ApiError::not_ready())?;
    tx.commit().await.map_err(|_| ApiError::commit_unknown())
}

pub async fn note_label_stage(endpoint: &Endpoint, build: &BuildRow) -> Result<(), ApiError> {
    let client = connect(endpoint).await?;
    let decisions: i64 = client
        .query_one(
            "SELECT count(*) FROM oo_selfbuild_api.decision_record
             WHERE build_id = $1 AND decision_kind = 'label_description'",
            &[&build.build_id],
        )
        .await
        .map_err(|_| ApiError::not_ready())?
        .get(0);
    let members: i64 = client
        .query_one(
            "SELECT count(*) FROM oo_selfbuild_api.member WHERE build_id = $1",
            &[&build.build_id],
        )
        .await
        .map_err(|_| ApiError::not_ready())?
        .get(0);
    if members > 0 && decisions >= members && !set_stage(&client, &build.build_id, "S9", "succeeded", &build.input_sha256, None, None).await? {
        return Err(ApiError::not_ready());
    }
    Ok(())
}

pub async fn mark_stage(endpoint: &Endpoint, build: &BuildRow, stage: &str) -> Result<(), ApiError> {
    let client = connect(endpoint).await?;
    if !set_stage(&client, &build.build_id, stage, "succeeded", &build.input_sha256, None, None).await? {
        return Err(ApiError::not_ready());
    }
    Ok(())
}

pub async fn queue_head(endpoint: &Endpoint, project_id: &str, object_id: &str) -> Result<Option<QueueHead>, ApiError> {
    let client = connect(endpoint).await?;
    let row = client
        .query_opt(
            "SELECT revision, candidate_sha256, input_sha256, evidence_sha256, schema_sha256, curated_sha256, source_sha256
             FROM oo_review_ledger.queue_head WHERE project_id = $1 AND object_id = $2",
            &[&project_id, &object_id],
        )
        .await;
    match row {
        Ok(Some(row)) => Ok(Some(QueueHead {
            revision: row.get(0),
            candidate_sha256: row.get(1),
            input_sha256: row.get(2),
            evidence_sha256: row.get(3),
            schema_sha256: row.get(4),
            curated_sha256: row.get(5),
            source_sha256: row.get(6),
        })),
        Ok(None) => Ok(None),
        Err(error) if missing_schema(&error) => Ok(None),
        Err(_) => Err(ApiError::not_ready()),
    }
}

pub async fn ledger_decisions(endpoint: &Endpoint, project_id: &str) -> Result<Vec<LedgerDecision>, ApiError> {
    let client = connect(endpoint).await?;
    let rows = client
        .query(
            "SELECT object_id, decision_kind, choice, reviewer_label, reviewer_at, fields_json, authenticated
             FROM oo_review_ledger.decision WHERE project_id = $1 ORDER BY object_id, decision_kind",
            &[&project_id],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(rows
        .into_iter()
        .map(|row| LedgerDecision {
            object_id: row.get(0),
            decision_kind: row.get(1),
            choice: row.get(2),
            reviewer_label: row.get(3),
            reviewer_at: row.get(4),
            fields_json: row.get(5),
            authenticated: row.get(6),
        })
        .collect())
}

pub async fn our_decisions(endpoint: &Endpoint, build_id: &str) -> Result<Vec<OurDecision>, ApiError> {
    let client = connect(endpoint).await?;
    let rows = client
        .query(
            "SELECT object_id, decision_kind, choice FROM oo_selfbuild_api.decision_record
             WHERE build_id = $1 ORDER BY object_id, decision_kind",
            &[&build_id],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(rows
        .into_iter()
        .map(|row| OurDecision {
            object_id: row.get(0),
            decision_kind: row.get(1),
            choice: row.get(2),
        })
        .collect())
}

#[derive(Clone, Debug)]
pub struct PackHead {
    pub output_sha256: String,
    pub reseal_count: Option<i64>,
}

pub async fn confirm_body(endpoint: &Endpoint, project_id: &str, idempotency_key: &str) -> Result<Option<String>, ApiError> {
    let client = connect(endpoint).await?;
    let row = client
        .query_opt(
            "SELECT body_sha256 FROM oo_selfbuild_api.audit_event
             WHERE project_id = $1 AND action = 'confirm' AND detail_code = $2
             ORDER BY event_id LIMIT 1",
            &[&project_id, &idempotency_key],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(row.map(|row| row.get(0)))
}

pub async fn insert_confirm_audit(
    endpoint: &Endpoint,
    project_id: &str,
    build_id: &str,
    subject: &str,
    idempotency_key: &str,
    body_sha256: &str,
) -> Result<(), ApiError> {
    let client = connect(endpoint).await?;
    client
        .execute(
            "INSERT INTO oo_selfbuild_api.audit_event (project_id, build_id, action, subject, detail_code, body_sha256)
             VALUES ($1, $2, 'confirm', $3, $4, $5)",
            &[&project_id, &build_id, &subject, &idempotency_key, &body_sha256],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(())
}

pub async fn find_pack(endpoint: &Endpoint, build_id: &str) -> Result<Option<PackHead>, ApiError> {
    let client = connect(endpoint).await?;
    let row = client
        .query_opt(
            "SELECT output_sha256, reseal_count FROM oo_selfbuild_api.pack_record WHERE build_id = $1",
            &[&build_id],
        )
        .await
        .map_err(|_| ApiError::not_ready())?;
    Ok(row.map(|row| PackHead {
        output_sha256: row.get(0),
        reseal_count: row.get(1),
    }))
}

pub async fn insert_pack(
    endpoint: &Endpoint,
    build: &BuildRow,
    subject: &str,
    output_sha256: &str,
    reseal_count: Option<i64>,
    body_sha256: &str,
) -> Result<(), ApiError> {
    let mut client = connect(endpoint).await?;
    let tx = client.transaction().await.map_err(|_| ApiError::not_ready())?;
    tx.batch_execute("SET LOCAL synchronous_commit = on")
        .await
        .map_err(|_| ApiError::not_ready())?;
    let published = false;
    let active = false;
    let authenticated = false;
    let params: [&(dyn ToSql + Sync); 6] = [
        &build.build_id,
        &output_sha256,
        &published,
        &active,
        &authenticated,
        &reseal_count,
    ];
    tx.execute(
        "INSERT INTO oo_selfbuild_api.pack_record (build_id, output_sha256, published, active, producer_authenticated, reseal_count)
         VALUES ($1, $2, $3, $4, $5, $6)",
        &params,
    )
    .await
    .map_err(|error| {
        if error.code() == Some(&SqlState::UNIQUE_VIOLATION) {
            ApiError::idempotency()
        } else {
            ApiError::not_ready()
        }
    })?;
    tx.execute(
        "INSERT INTO oo_selfbuild_api.audit_event (project_id, build_id, action, subject, detail_code, body_sha256)
         VALUES ($1, $2, 'compile', $3, 'compiled', $4)",
        &[&build.project_id, &build.build_id, &subject, &body_sha256],
    )
    .await
    .map_err(|_| ApiError::not_ready())?;
    tx.commit().await.map_err(|_| ApiError::commit_unknown())
}

pub async fn new_build_id() -> Result<String, ApiError> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|_| ApiError::not_ready())?;
    let mut out = String::from("b_");
    for byte in bytes {
        out.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        out.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    Ok(out)
}


