//! Transactional adapter. enum-history decides the report. This file stores it.

use std::time::Duration;

use enum_history::{compile, CompileInput, Outcome};
use postgres_types::ToSql;
use serde::Serialize;
use tokio_postgres::config::SslMode;
use tokio_postgres::{Client, Error, Row, Transaction};

use crate::error::{commit_error, query_error, StoreError};
use crate::publish::{self, Materialization};
use crate::role::{self, ADMIN_ROLE, VIEW_ROLE};
use crate::sha256::sha256_hex;
use crate::sql::{self, INSTALL_LOCK};
use crate::tls::rustls_connector;
use crate::CONTRACT;

const UNIQUE: &str = "23505";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub database: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallDisposition {
    Installed,
    Replay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PutDisposition {
    Inserted,
    Replay,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct InstallReceipt {
    pub contract: &'static str,
    pub disposition: InstallDisposition,
    pub sql_sha256: String,
    pub project_id: String,
    pub database_used: bool,
    pub finance_certified: bool,
    pub semantic_certified: bool,
    pub owner_authenticated: bool,
    pub metric_certified: bool,
    pub clock_used: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct PutReceipt {
    pub contract: &'static str,
    pub disposition: PutDisposition,
    pub snapshot_id: String,
    pub report_sha256: String,
    pub config_sha256: String,
    pub as_of: String,
    pub database_used: bool,
    pub finance_certified: bool,
    pub semantic_certified: bool,
    pub owner_authenticated: bool,
    pub metric_certified: bool,
    pub clock_used: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct ActivateReceipt {
    pub contract: &'static str,
    pub disposition: &'static str,
    pub snapshot_id: String,
    pub grants: usize,
    pub database_used: bool,
    pub finance_certified: bool,
    pub semantic_certified: bool,
    pub owner_authenticated: bool,
    pub metric_certified: bool,
    pub clock_used: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EnumColumn {
    pub enum_id: String,
    pub code: String,
    pub description: Option<String>,
    pub class: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub version: String,
    pub as_of: String,
    pub project_id: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct ReadReceipt {
    pub contract: &'static str,
    pub rows: Vec<EnumColumn>,
    pub database_used: bool,
    pub finance_certified: bool,
    pub semantic_certified: bool,
    pub owner_authenticated: bool,
    pub metric_certified: bool,
    pub clock_used: bool,
}

pub struct InstallRequest<'a> {
    pub endpoint: &'a Endpoint,
    pub user: &'a str,
    pub password: &'a str,
    pub ca_pem: &'a [u8],
    pub project_id: &'a str,
    pub runtime_roles: &'a [&'a str],
}

pub struct PutRequest<'a> {
    pub endpoint: &'a Endpoint,
    pub user: &'a str,
    pub password: &'a str,
    pub ca_pem: &'a [u8],
    pub project_id: &'a str,
    pub enum_version: &'a str,
    pub enum_yaml: &'a str,
    pub as_of: &'a str,
    pub observations: &'a [enum_history::Observation],
    pub threshold: Option<&'a str>,
    pub expected_report_sha256: &'a str,
}

pub struct ActivateRequest<'a> {
    pub endpoint: &'a Endpoint,
    pub user: &'a str,
    pub password: &'a str,
    pub ca_pem: &'a [u8],
    pub project_id: &'a str,
    pub expected_current: Option<&'a str>,
    pub snapshot_id: &'a str,
    pub config_sha256: &'a str,
    pub enum_version: &'a str,
    pub as_of: &'a str,
}

pub struct ReadRequest<'a> {
    pub endpoint: &'a Endpoint,
    pub user: &'a str,
    pub password: &'a str,
    pub ca_pem: &'a [u8],
}

struct Session {
    client: Client,
    _connection: tokio::task::JoinHandle<()>,
}

enum CommitEnd {
    Ack,
    Server { sqlstate: String },
    Unknown,
}

pub fn migration_sha256() -> Result<String, StoreError> {
    sha256_hex(sql::MIGRATION_SQL.as_bytes()).ok_or(StoreError::Input)
}

pub async fn install(request: &InstallRequest<'_>) -> Result<InstallReceipt, StoreError> {
    require_login(request.user, ADMIN_ROLE)?;
    endpoint_ok(request.endpoint)?;
    if !role::token_ok(request.project_id) {
        return Err(StoreError::Input);
    }
    let roles = normalize_roles(request.runtime_roles)?;
    let hash = migration_sha256()?;
    let mut session = open(request.endpoint, request.user, request.password, request.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let disposition = match install_body(&tx, &hash, request.project_id, &roles).await {
        Ok(disposition) => disposition,
        Err(err) => return Err(rollback_err(tx, err).await),
    };
    finish_install(tx, disposition, hash, request.project_id).await
}

pub async fn put_snapshot(request: &PutRequest<'_>) -> Result<PutReceipt, StoreError> {
    require_login(request.user, ADMIN_ROLE)?;
    endpoint_ok(request.endpoint)?;
    if !role::token_ok(request.project_id) || !role::token_ok(request.enum_version) || !role::hex64(request.expected_report_sha256)
    {
        return Err(StoreError::Input);
    }
    if request.enum_yaml.is_empty()
        || request.enum_yaml.len() > enum_history::STDIN_MAX_BYTES
        || request.observations.len() > enum_history::MAX_OBSERVATIONS
    {
        return Err(StoreError::Input);
    }
    let spelling = publish::threshold_spelling(request.threshold);
    let outcome = compile(&CompileInput {
        enum_yaml: request.enum_yaml.to_owned(),
        as_of: request.as_of.to_owned(),
        observations: request.observations.to_vec(),
        threshold: request.threshold.map(str::to_owned),
    });
    let report = match outcome {
        Outcome::Compiled(report) => report,
        Outcome::Refused(refusal) => {
            return Err(StoreError::SnapshotRefused { reason: refusal.reason, detail: refusal.detail });
        }
    };
    let mat = publish::materialize(&report, request.project_id, request.enum_version, request.enum_yaml, request.observations, spelling)?;
    if mat.report_sha256 != request.expected_report_sha256 {
        return Err(StoreError::ReportMismatch);
    }
    if mat.as_of != request.as_of {
        return Err(StoreError::HelperDisagreement);
    }
    let mut session = open(request.endpoint, request.user, request.password, request.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    match put_body(&tx, request, &mat).await {
        Ok(disposition) => finish_put(tx, disposition, &mat).await,
        Err(err) if sqlstate_of(&err) == Some(UNIQUE) => {
            let _ = tx.rollback().await;
            replay_after_conflict(request, &mat).await
        }
        Err(err) => Err(rollback_err(tx, err).await),
    }
}

pub async fn activate(request: &ActivateRequest<'_>) -> Result<ActivateReceipt, StoreError> {
    require_login(request.user, ADMIN_ROLE)?;
    endpoint_ok(request.endpoint)?;
    if !role::token_ok(request.project_id)
        || !role::token_ok(request.enum_version)
        || !role::hex64(request.snapshot_id)
        || !role::hex64(request.config_sha256)
    {
        return Err(StoreError::Input);
    }
    if let Some(current) = request.expected_current
        && !role::hex64(current)
    {
        return Err(StoreError::Input);
    }
    let mut session = open(request.endpoint, request.user, request.password, request.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let grants = match activate_body(&tx, request).await {
        Ok(grants) => grants,
        Err(err) => return Err(rollback_err(tx, err).await),
    };
    match end_commit(tx).await {
        CommitEnd::Ack => Ok(activate_receipt(request.snapshot_id, grants)),
        CommitEnd::Server { sqlstate } => Err(StoreError::Query { sqlstate: Some(sqlstate) }),
        CommitEnd::Unknown => Err(StoreError::CommitUnknown),
    }
}

pub async fn read_view(request: &ReadRequest<'_>) -> Result<ReadReceipt, StoreError> {
    endpoint_ok(request.endpoint)?;
    if !role::runtime_role_ok(request.user) {
        return Err(StoreError::LoginRole);
    }
    let mut session = open(request.endpoint, request.user, request.password, request.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    if let Err(err) = require_role(&tx, request.user).await {
        return Err(rollback_err(tx, err).await);
    }
    let rows = match tx.query(sql::READ_VIEW, &[]).await {
        Ok(rows) => rows,
        Err(err) => return Err(rollback_err(tx, query_error(&err)).await),
    };
    let mut projected = Vec::with_capacity(rows.len());
    for row in &rows {
        match read_column(row) {
            Ok(column) => projected.push(column),
            Err(err) => return Err(rollback_err(tx, err).await),
        }
    }
    match end_commit(tx).await {
        CommitEnd::Ack => Ok(ReadReceipt {
            contract: CONTRACT,
            rows: projected,
            database_used: true,
            finance_certified: false,
            semantic_certified: false,
            owner_authenticated: false,
            metric_certified: false,
            clock_used: false,
        }),
        CommitEnd::Server { sqlstate } => Err(StoreError::Query { sqlstate: Some(sqlstate) }),
        CommitEnd::Unknown => Err(StoreError::CommitUnknown),
    }
}

async fn install_body(
    tx: &Transaction<'_>,
    hash: &str,
    project_id: &str,
    roles: &[String],
) -> Result<InstallDisposition, StoreError> {
    require_role(tx, ADMIN_ROLE).await?;
    tx.execute(sql::LOCK_INSTALL, &[&INSTALL_LOCK]).await.map_err(|err| query_error(&err))?;
    ensure_roles(tx, roles).await?;
    let exists = tx.query_opt(sql::SCHEMA_EXISTS, &[&sql::SCHEMA]).await.map_err(|err| query_error(&err))?.is_some();
    if !exists {
        if tx.query_opt(sql::SCHEMA_EXISTS, &[&sql::DICT_SCHEMA]).await.map_err(|err| query_error(&err))?.is_some()
            || tx.query_opt(sql::DICT_RELATION, &[]).await.map_err(|err| query_error(&err))?.is_some()
        {
            return Err(StoreError::Collision { check: "oo_dict" });
        }
        tx.batch_execute(sql::MIGRATION_SQL).await.map_err(|err| map_migrate(&err))?;
        exec_one(tx, sql::INSERT_MANIFEST, &[&hash], "manifest_insert").await?;
        exec_one(tx, sql::INSERT_DEPLOYMENT, &[&project_id], "deployment_insert").await?;
        for role in roles {
            exec_one(tx, sql::INSERT_ALLOW, &[role], "allow_insert").await?;
        }
        exec_one(tx, sql::INSERT_POINTER, &[&project_id], "pointer_insert").await?;
        verify_catalog(tx, roles).await?;
        let fresh = read_pointer(tx).await?;
        if fresh.0 != project_id || fresh.1.is_some() {
            return Err(StoreError::InstallDrift { check: "pointer" });
        }
        return Ok(InstallDisposition::Installed);
    }
    let stored = match tx.query_opt(sql::READ_MANIFEST, &[]).await {
        Ok(Some(row)) => row.try_get::<_, String>(0).map_err(|_| StoreError::InstallDrift { check: "manifest_row" })?,
        Ok(None) => return Err(StoreError::InstallDrift { check: "manifest_missing" }),
        Err(err) if missing_relation(&err) => return Err(StoreError::InstallDrift { check: "manifest_relation" }),
        Err(err) => return Err(query_error(&err)),
    };
    if stored != hash {
        return Err(StoreError::InstallDrift { check: "hash" });
    }
    verify_catalog(tx, roles).await?;
    let (bound, flags_ok) = read_deployment(tx).await?;
    if !flags_ok {
        return Err(StoreError::Flag);
    }
    if bound != project_id {
        return Err(StoreError::ProjectBinding);
    }
    let stored_roles = text_column(tx, sql::READ_ALLOW, &[]).await?;
    if stored_roles != roles {
        return Err(StoreError::Allowlist);
    }
    Ok(InstallDisposition::Replay)
}

async fn put_body(tx: &Transaction<'_>, request: &PutRequest<'_>, mat: &Materialization) -> Result<PutDisposition, StoreError> {
    require_role(tx, ADMIN_ROLE).await?;
    tx.execute(sql::LOCK_INSTALL, &[&INSTALL_LOCK]).await.map_err(|err| query_error(&err))?;
    let (bound, flags_ok) = read_deployment(tx).await.map_err(not_installed)?;
    if !flags_ok {
        return Err(StoreError::Flag);
    }
    if bound != request.project_id {
        return Err(StoreError::ProjectBinding);
    }
    if let Some(existing) = tx.query_opt(sql::READ_SNAPSHOT, &[&mat.snapshot_id]).await.map_err(|err| query_error(&err))? {
        return if payload_matches(tx, &existing, request, mat).await? {
            Ok(PutDisposition::Replay)
        } else {
            Err(StoreError::SnapshotDrift)
        };
    }
    insert_snapshot(tx, request, mat).await?;
    insert_notices(tx, mat).await?;
    insert_published(tx, mat).await?;
    Ok(PutDisposition::Inserted)
}

async fn activate_body(tx: &Transaction<'_>, request: &ActivateRequest<'_>) -> Result<usize, StoreError> {
    require_role(tx, ADMIN_ROLE).await?;
    tx.execute(sql::LOCK_INSTALL, &[&INSTALL_LOCK]).await.map_err(|err| query_error(&err))?;
    let (bound, flags_ok) = read_deployment(tx).await.map_err(not_installed)?;
    if !flags_ok {
        return Err(StoreError::Flag);
    }
    if bound != request.project_id {
        return Err(StoreError::ProjectBinding);
    }
    let Some(snapshot) = tx.query_opt(sql::READ_SNAPSHOT, &[&request.snapshot_id]).await.map_err(|err| query_error(&err))? else {
        return Err(StoreError::Activation);
    };
    let project: String = snapshot.try_get(0).map_err(|_| StoreError::Activation)?;
    let version: String = snapshot.try_get(1).map_err(|_| StoreError::Activation)?;
    let config: String = snapshot.try_get(2).map_err(|_| StoreError::Activation)?;
    let as_of: String = snapshot.try_get(3).map_err(|_| StoreError::Activation)?;
    let warning_count: i32 = snapshot.try_get(22).map_err(|_| StoreError::Activation)?;
    let published_count: i32 = snapshot.try_get(23).map_err(|_| StoreError::Activation)?;
    if project != request.project_id || version != request.enum_version || config != request.config_sha256 || as_of != request.as_of {
        return Err(StoreError::Activation);
    }
    let notices = tx.query(sql::READ_NOTICES, &[&request.snapshot_id]).await.map_err(|err| query_error(&err))?;
    let published = tx.query(sql::READ_PUBLISHED, &[&request.snapshot_id]).await.map_err(|err| query_error(&err))?;
    if notices.len() != usize::try_from(warning_count).unwrap_or(usize::MAX) || published.len() != usize::try_from(published_count).unwrap_or(usize::MAX) {
        return Err(StoreError::Activation);
    }
    let expected = request.expected_current.map(str::to_owned);
    let updated = tx
        .execute(sql::ACTIVATE, &[&request.snapshot_id, &request.enum_version, &request.as_of, &request.project_id, &expected])
        .await
        .map_err(|err| query_error(&err))?;
    if updated != 1 {
        return Err(StoreError::CasConflict);
    }
    tx.execute(sql::CLEAR_SERVED, &[]).await.map_err(|err| query_error(&err))?;
    let inserted = tx.execute(sql::INSERT_SERVED, &[&request.snapshot_id]).await.map_err(|err| query_error(&err))?;
    let served: i64 = count_one(tx, sql::COUNT_SERVED, &[]).await?;
    let published_n: i64 = count_one(tx, sql::COUNT_PUBLISHED, &[&request.snapshot_id]).await?;
    let Ok(want) = u64::try_from(published_n) else {
        return Err(StoreError::Activation);
    };
    if inserted != want || served != published_n || published_n != i64::from(published_count) {
        return Err(StoreError::Activation);
    }
    let roles = text_column(tx, sql::READ_ALLOW, &[]).await?;
    for role in &roles {
        let usage = role::grant_usage_sql(role)?;
        let select = role::grant_select_batch(role)?;
        tx.batch_execute(&usage).await.map_err(|err| query_error(&err))?;
        tx.batch_execute(&select).await.map_err(|err| query_error(&err))?;
    }
    Ok(roles.len())
}

async fn replay_after_conflict(request: &PutRequest<'_>, mat: &Materialization) -> Result<PutReceipt, StoreError> {
    let mut session = open(request.endpoint, request.user, request.password, request.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let existing = match tx.query_opt(sql::READ_SNAPSHOT, &[&mat.snapshot_id]).await {
        Ok(Some(row)) => row,
        Ok(None) => {
            match tx
                .query_opt(
                    sql::READ_IDENTITY,
                    &[
                        &request.project_id,
                        &request.enum_version,
                        &mat.config_sha256,
                        &mat.as_of,
                        &mat.observations_sha256,
                        &mat.threshold_spelling,
                    ],
                )
                .await
            {
                Ok(_) => return Err(rollback_err(tx, StoreError::SnapshotDrift).await),
                Err(err) => return Err(rollback_err(tx, query_error(&err)).await),
            }
        }
        Err(err) => return Err(rollback_err(tx, query_error(&err)).await),
    };
    match payload_matches(&tx, &existing, request, mat).await {
        Ok(true) => finish_put(tx, PutDisposition::Replay, mat).await,
        Ok(false) => Err(rollback_err(tx, StoreError::SnapshotDrift).await),
        Err(err) => Err(rollback_err(tx, err).await),
    }
}

async fn payload_matches(
    tx: &Transaction<'_>,
    existing: &Row,
    request: &PutRequest<'_>,
    mat: &Materialization,
) -> Result<bool, StoreError> {
    if !snapshot_columns_match(existing, request, mat)? {
        return Ok(false);
    }
    let published = tx.query(sql::READ_PUBLISHED, &[&mat.snapshot_id]).await.map_err(|err| query_error(&err))?;
    if published.len() != mat.lines.len() {
        return Ok(false);
    }
    for (row, line) in published.iter().zip(&mat.lines) {
        let line_no: i32 = row.try_get(0).map_err(|_| StoreError::SnapshotDrift)?;
        let kind: String = row.try_get(1).map_err(|_| StoreError::SnapshotDrift)?;
        let enum_id: String = row.try_get(2).map_err(|_| StoreError::SnapshotDrift)?;
        let code: String = row.try_get(3).map_err(|_| StoreError::SnapshotDrift)?;
        let description: Option<String> = row.try_get(4).map_err(|_| StoreError::SnapshotDrift)?;
        let class: Option<String> = row.try_get(5).map_err(|_| StoreError::SnapshotDrift)?;
        let valid_from: Option<String> = row.try_get(6).map_err(|_| StoreError::SnapshotDrift)?;
        let valid_to: Option<String> = row.try_get(7).map_err(|_| StoreError::SnapshotDrift)?;
        let version: String = row.try_get(8).map_err(|_| StoreError::SnapshotDrift)?;
        let as_of: String = row.try_get(9).map_err(|_| StoreError::SnapshotDrift)?;
        let project_id: String = row.try_get(10).map_err(|_| StoreError::SnapshotDrift)?;
        let status: String = row.try_get(11).map_err(|_| StoreError::SnapshotDrift)?;
        if line_no != line.line_no
            || kind != line.row_kind
            || enum_id != line.enum_id
            || code != line.code
            || description != line.description
            || class != line.class
            || valid_from != line.valid_from
            || valid_to != line.valid_to
            || version != line.version
            || as_of != line.as_of
            || project_id != line.project_id
            || status != line.status
        {
            return Ok(false);
        }
    }
    let notices = tx.query(sql::READ_NOTICES, &[&mat.snapshot_id]).await.map_err(|err| query_error(&err))?;
    if notices.len() != mat.notices.len() {
        return Ok(false);
    }
    for (row, notice) in notices.iter().zip(&mat.notices) {
        let notice_no: i32 = row.try_get(0).map_err(|_| StoreError::SnapshotDrift)?;
        let code: String = row.try_get(1).map_err(|_| StoreError::SnapshotDrift)?;
        let kind: String = row.try_get(2).map_err(|_| StoreError::SnapshotDrift)?;
        let reason: String = row.try_get(3).map_err(|_| StoreError::SnapshotDrift)?;
        let count_text: String = row.try_get(4).map_err(|_| StoreError::SnapshotDrift)?;
        let source_index: Option<i32> = row.try_get(5).map_err(|_| StoreError::SnapshotDrift)?;
        if notice_no != notice.notice_no
            || code != notice.code
            || kind != notice.kind
            || reason != notice.reason
            || count_text != notice.count_text
            || source_index != notice.source_index
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn snapshot_columns_match(existing: &Row, request: &PutRequest<'_>, mat: &Materialization) -> Result<bool, StoreError> {
    let project: String = existing.try_get(0).map_err(|_| StoreError::SnapshotDrift)?;
    let version: String = existing.try_get(1).map_err(|_| StoreError::SnapshotDrift)?;
    let config: String = existing.try_get(2).map_err(|_| StoreError::SnapshotDrift)?;
    let as_of: String = existing.try_get(3).map_err(|_| StoreError::SnapshotDrift)?;
    let observations_sha: String = existing.try_get(4).map_err(|_| StoreError::SnapshotDrift)?;
    let spelling: String = existing.try_get(5).map_err(|_| StoreError::SnapshotDrift)?;
    let report_sha: String = existing.try_get(6).map_err(|_| StoreError::SnapshotDrift)?;
    let yaml: String = existing.try_get(7).map_err(|_| StoreError::SnapshotDrift)?;
    let observations_json: String = existing.try_get(8).map_err(|_| StoreError::SnapshotDrift)?;
    let report_json: String = existing.try_get(9).map_err(|_| StoreError::SnapshotDrift)?;
    let enum_id: String = existing.try_get(10).map_err(|_| StoreError::SnapshotDrift)?;
    let observed_defined: bool = existing.try_get(11).map_err(|_| StoreError::SnapshotDrift)?;
    let coverage_numerator: Option<String> = existing.try_get(12).map_err(|_| StoreError::SnapshotDrift)?;
    let coverage_denominator: Option<String> = existing.try_get(13).map_err(|_| StoreError::SnapshotDrift)?;
    let coverage_ratio: Option<String> = existing.try_get(14).map_err(|_| StoreError::SnapshotDrift)?;
    let threshold_decimal: String = existing.try_get(15).map_err(|_| StoreError::SnapshotDrift)?;
    let threshold_numerator: String = existing.try_get(16).map_err(|_| StoreError::SnapshotDrift)?;
    let threshold_denominator: String = existing.try_get(17).map_err(|_| StoreError::SnapshotDrift)?;
    let threshold_defaulted: bool = existing.try_get(18).map_err(|_| StoreError::SnapshotDrift)?;
    let threshold_met: Option<bool> = existing.try_get(19).map_err(|_| StoreError::SnapshotDrift)?;
    let comparison: Option<String> = existing.try_get(20).map_err(|_| StoreError::SnapshotDrift)?;
    let review_needed: bool = existing.try_get(21).map_err(|_| StoreError::SnapshotDrift)?;
    let warning_count: i32 = existing.try_get(22).map_err(|_| StoreError::SnapshotDrift)?;
    let published_count: i32 = existing.try_get(23).map_err(|_| StoreError::SnapshotDrift)?;
    let declared_coverage: Option<String> = existing.try_get(24).map_err(|_| StoreError::SnapshotDrift)?;
    let declared_owner: Option<String> = existing.try_get(25).map_err(|_| StoreError::SnapshotDrift)?;
    Ok(project == request.project_id
        && version == request.enum_version
        && config == mat.config_sha256
        && as_of == mat.as_of
        && observations_sha == mat.observations_sha256
        && spelling == mat.threshold_spelling
        && report_sha == mat.report_sha256
        && yaml == request.enum_yaml
        && observations_json == mat.observations_json
        && report_json == mat.report_json
        && enum_id == mat.enum_id
        && observed_defined == mat.observed_defined
        && coverage_numerator == mat.coverage_numerator
        && coverage_denominator == mat.coverage_denominator
        && coverage_ratio == mat.coverage_ratio
        && threshold_decimal == mat.threshold_decimal
        && threshold_numerator == mat.threshold_numerator
        && threshold_denominator == mat.threshold_denominator
        && threshold_defaulted == mat.threshold_defaulted
        && threshold_met == mat.threshold_met
        && comparison == mat.comparison
        && review_needed == mat.review_needed
        && warning_count == mat.warning_count
        && published_count == mat.published_count
        && declared_coverage == mat.declared_coverage
        && declared_owner == mat.declared_owner)
}

async fn insert_snapshot(tx: &Transaction<'_>, request: &PutRequest<'_>, mat: &Materialization) -> Result<(), StoreError> {
    let inserted = tx
        .execute(
            sql::INSERT_SNAPSHOT,
            &[
                &mat.snapshot_id,
                &request.project_id,
                &request.enum_version,
                &mat.config_sha256,
                &mat.as_of,
                &mat.observations_sha256,
                &mat.threshold_spelling,
                &mat.report_sha256,
                &request.enum_yaml,
                &mat.observations_json,
                &mat.report_json,
                &mat.enum_id,
                &mat.observed_defined,
                &mat.coverage_numerator,
                &mat.coverage_denominator,
                &mat.coverage_ratio,
                &mat.threshold_decimal,
                &mat.threshold_numerator,
                &mat.threshold_denominator,
                &mat.threshold_defaulted,
                &mat.threshold_met,
                &mat.comparison,
                &mat.review_needed,
                &mat.warning_count,
                &mat.published_count,
                &mat.declared_coverage,
                &mat.declared_owner,
            ],
        )
        .await
        .map_err(|err| query_error(&err))?;
    if inserted == 1 { Ok(()) } else { Err(StoreError::SnapshotDrift) }
}

async fn insert_notices(tx: &Transaction<'_>, mat: &Materialization) -> Result<(), StoreError> {
    for notice in &mat.notices {
        let inserted = tx
            .execute(
                sql::INSERT_NOTICE,
                &[
                    &mat.snapshot_id,
                    &notice.notice_no,
                    &notice.code,
                    &notice.kind,
                    &notice.reason,
                    &notice.count_text,
                    &notice.source_index,
                ],
            )
            .await
            .map_err(|err| query_error(&err))?;
        if inserted != 1 {
            return Err(StoreError::SnapshotDrift);
        }
    }
    Ok(())
}

async fn insert_published(tx: &Transaction<'_>, mat: &Materialization) -> Result<(), StoreError> {
    for line in &mat.lines {
        let inserted = tx
            .execute(
                sql::INSERT_PUBLISHED,
                &[
                    &mat.snapshot_id,
                    &line.line_no,
                    &line.row_kind,
                    &line.enum_id,
                    &line.code,
                    &line.description,
                    &line.class,
                    &line.valid_from,
                    &line.valid_to,
                    &line.version,
                    &line.as_of,
                    &line.project_id,
                    &line.status,
                ],
            )
            .await
            .map_err(|err| query_error(&err))?;
        if inserted != 1 {
            return Err(StoreError::SnapshotDrift);
        }
    }
    Ok(())
}

fn read_column(row: &Row) -> Result<EnumColumn, StoreError> {
    Ok(EnumColumn {
        enum_id: row.try_get(0).map_err(|_| StoreError::HelperDisagreement)?,
        code: row.try_get(1).map_err(|_| StoreError::HelperDisagreement)?,
        description: row.try_get(2).map_err(|_| StoreError::HelperDisagreement)?,
        class: row.try_get(3).map_err(|_| StoreError::HelperDisagreement)?,
        valid_from: row.try_get(4).map_err(|_| StoreError::HelperDisagreement)?,
        valid_to: row.try_get(5).map_err(|_| StoreError::HelperDisagreement)?,
        version: row.try_get(6).map_err(|_| StoreError::HelperDisagreement)?,
        as_of: row.try_get(7).map_err(|_| StoreError::HelperDisagreement)?,
        project_id: row.try_get(8).map_err(|_| StoreError::HelperDisagreement)?,
        status: row.try_get(9).map_err(|_| StoreError::HelperDisagreement)?,
    })
}

async fn verify_catalog(tx: &Transaction<'_>, roles: &[String]) -> Result<(), StoreError> {
    let owner = one_text(tx, sql::SCHEMA_OWNER, &[&sql::SCHEMA]).await?;
    if owner != ADMIN_ROLE {
        return Err(StoreError::InstallDrift { check: "control_owner" });
    }
    let dict_owner = one_text(tx, sql::SCHEMA_OWNER, &[&sql::DICT_SCHEMA]).await?;
    if dict_owner != ADMIN_ROLE {
        return Err(StoreError::InstallDrift { check: "dict_owner" });
    }
    expect_list(tx, sql::RELATIONS, &[&sql::SCHEMA], &sql::EXPECTED_RELATIONS, "relations").await?;
    expect_columns(tx, "install_manifest", &sql::EXPECTED_MANIFEST_COLUMNS, "manifest_columns").await?;
    expect_columns(tx, "deployment", &sql::EXPECTED_DEPLOYMENT_COLUMNS, "deployment_columns").await?;
    expect_columns(tx, "runtime_allow", &sql::EXPECTED_ALLOW_COLUMNS, "allow_columns").await?;
    expect_columns(tx, "snapshot", &sql::EXPECTED_SNAPSHOT_COLUMNS, "snapshot_columns").await?;
    expect_columns(tx, "notice", &sql::EXPECTED_NOTICE_COLUMNS, "notice_columns").await?;
    expect_columns(tx, "published", &sql::EXPECTED_PUBLISHED_COLUMNS, "published_columns").await?;
    expect_columns(tx, "served", &sql::EXPECTED_SERVED_COLUMNS, "served_columns").await?;
    expect_columns(tx, "active_pointer", &sql::EXPECTED_POINTER_COLUMNS, "pointer_columns").await?;
    expect_list(tx, sql::CONSTRAINTS, &[&sql::SCHEMA, &"install_manifest"], &sql::EXPECTED_MANIFEST_CONSTRAINTS, "manifest_constraints").await?;
    expect_list(tx, sql::CONSTRAINTS, &[&sql::SCHEMA, &"deployment"], &sql::EXPECTED_DEPLOYMENT_CONSTRAINTS, "deployment_constraints").await?;
    expect_list(tx, sql::CONSTRAINTS, &[&sql::SCHEMA, &"runtime_allow"], &sql::EXPECTED_ALLOW_CONSTRAINTS, "allow_constraints").await?;
    expect_list(tx, sql::CONSTRAINTS, &[&sql::SCHEMA, &"snapshot"], &sql::EXPECTED_SNAPSHOT_CONSTRAINTS, "snapshot_constraints").await?;
    expect_list(tx, sql::CONSTRAINTS, &[&sql::SCHEMA, &"notice"], &sql::EXPECTED_NOTICE_CONSTRAINTS, "notice_constraints").await?;
    expect_list(tx, sql::CONSTRAINTS, &[&sql::SCHEMA, &"published"], &sql::EXPECTED_PUBLISHED_CONSTRAINTS, "published_constraints").await?;
    expect_list(tx, sql::CONSTRAINTS, &[&sql::SCHEMA, &"served"], &sql::EXPECTED_SERVED_CONSTRAINTS, "served_constraints").await?;
    expect_list(tx, sql::CONSTRAINTS, &[&sql::SCHEMA, &"active_pointer"], &sql::EXPECTED_POINTER_CONSTRAINTS, "pointer_constraints").await?;
    expect_view(tx).await?;
    expect_bools(tx, sql::VIEW_GRANTS, &[], &sql::EXPECTED_VIEW_GRANTS, "view_grants").await?;
    for role in roles {
        expect_bools(tx, sql::CONTROL_GRANTS, &[role], &sql::EXPECTED_RUNTIME_CONTROL, "runtime_control").await?;
    }
    expect_closed(tx, sql::SCHEMA).await?;
    expect_closed(tx, sql::DICT_SCHEMA).await?;
    for table in ["install_manifest", "deployment", "runtime_allow", "snapshot", "notice", "published", "served", "active_pointer"] {
        expect_relation_closed(tx, sql::SCHEMA, table).await?;
    }
    expect_relation_closed(tx, sql::DICT_SCHEMA, sql::VIEW).await?;
    Ok(())
}

async fn expect_view(tx: &Transaction<'_>) -> Result<(), StoreError> {
    let objects = tx.query(sql::DICT_OBJECTS, &[]).await.map_err(|err| query_error(&err))?;
    if objects.len() != 1 {
        return Err(StoreError::InstallDrift { check: "dict_objects" });
    }
    let name: String = objects[0].try_get(0).map_err(|_| StoreError::InstallDrift { check: "dict_objects" })?;
    let kind: String = objects[0].try_get(1).map_err(|_| StoreError::InstallDrift { check: "dict_objects" })?;
    if name != sql::VIEW || kind != "v" {
        return Err(StoreError::InstallDrift { check: "dict_objects" });
    }
    let fact = tx.query_one(sql::VIEW_FACT, &[]).await.map_err(|err| query_error(&err))?;
    let owner: String = fact.try_get(0).map_err(|_| StoreError::InstallDrift { check: "view_owner" })?;
    let options: String = fact.try_get(1).map_err(|_| StoreError::InstallDrift { check: "view_options" })?;
    let relkind: String = fact.try_get(2).map_err(|_| StoreError::InstallDrift { check: "view_kind" })?;
    if owner != VIEW_ROLE || relkind != "v" || !options.contains("security_barrier=true") || !options.contains("security_invoker=false") {
        return Err(StoreError::InstallDrift { check: "view_owner" });
    }
    let def = one_text(tx, sql::VIEW_DEF, &[]).await?.to_ascii_lowercase();
    if sql::VIEW_FORBIDDEN.iter().any(|token| def.contains(token)) {
        return Err(StoreError::InstallDrift { check: "view_def" });
    }
    Ok(())
}

async fn ensure_roles(tx: &Transaction<'_>, roles: &[String]) -> Result<(), StoreError> {
    let admin = role_attr(tx, ADMIN_ROLE).await?.ok_or(StoreError::LoginRole)?;
    if admin.0 || admin.1 {
        return Err(StoreError::LoginRole);
    }
    let view = role_attr(tx, VIEW_ROLE).await?.ok_or(StoreError::LoginRole)?;
    if view.0 || view.1 || view.2 || view.3 || view.4 {
        return Err(StoreError::LoginRole);
    }
    if tx.query_opt(sql::ADMIN_MEMBER, &[]).await.map_err(|err| query_error(&err))?.is_none() {
        return Err(StoreError::LoginRole);
    }
    for role in roles {
        let Some(attr) = role_attr(tx, role).await? else {
            return Err(StoreError::Allowlist);
        };
        if attr.0 || attr.1 || !role::runtime_role_ok(role) {
            return Err(StoreError::Allowlist);
        }
        if tx.query_opt(sql::RUNTIME_MEMBER, &[role]).await.map_err(|err| query_error(&err))?.is_some() {
            return Err(StoreError::Allowlist);
        }
    }
    Ok(())
}

async fn role_attr(tx: &Transaction<'_>, role: &str) -> Result<Option<(bool, bool, bool, bool, bool)>, StoreError> {
    let Some(row) = tx.query_opt(sql::ROLE_ATTR, &[&role]).await.map_err(|err| query_error(&err))? else {
        return Ok(None);
    };
    Ok(Some((
        row.try_get(0).map_err(|_| StoreError::LoginRole)?,
        row.try_get(1).map_err(|_| StoreError::LoginRole)?,
        row.try_get(2).map_err(|_| StoreError::LoginRole)?,
        row.try_get(3).map_err(|_| StoreError::LoginRole)?,
        row.try_get(4).map_err(|_| StoreError::LoginRole)?,
    )))
}

async fn read_deployment(tx: &Transaction<'_>) -> Result<(String, bool), StoreError> {
    let row = tx.query_one(sql::READ_DEPLOYMENT, &[]).await.map_err(|err| query_error(&err))?;
    let project: String = row.try_get(0).map_err(|_| StoreError::InstallDrift { check: "deployment" })?;
    let flags = (1..6).all(|index| row.try_get::<_, bool>(index).ok() == Some(false));
    Ok((project, flags))
}

async fn read_pointer(tx: &Transaction<'_>) -> Result<(String, Option<String>), StoreError> {
    let row = tx.query_one(sql::READ_POINTER, &[]).await.map_err(|err| query_error(&err))?;
    Ok((
        row.try_get(0).map_err(|_| StoreError::InstallDrift { check: "pointer" })?,
        row.try_get(1).map_err(|_| StoreError::InstallDrift { check: "pointer" })?,
    ))
}

async fn expect_closed(tx: &Transaction<'_>, schema: &str) -> Result<(), StoreError> {
    let count: i64 = tx
        .query_one(sql::PUBLIC_SCHEMA, &[&schema])
        .await
        .map_err(|err| query_error(&err))?
        .try_get(0)
        .map_err(|_| StoreError::InstallDrift { check: "public_acl" })?;
    if count == 0 { Ok(()) } else { Err(StoreError::InstallDrift { check: "public_acl" }) }
}

async fn expect_relation_closed(tx: &Transaction<'_>, schema: &str, relation: &str) -> Result<(), StoreError> {
    let closed: bool = tx
        .query_one(sql::PUBLIC_RELATION, &[&schema, &relation])
        .await
        .map_err(|err| query_error(&err))?
        .try_get(0)
        .map_err(|_| StoreError::InstallDrift { check: "public_acl" })?;
    if closed { Ok(()) } else { Err(StoreError::InstallDrift { check: "public_acl" }) }
}

async fn expect_columns(
    tx: &Transaction<'_>,
    table: &str,
    expected: &[(&str, &str)],
    check: &'static str,
) -> Result<(), StoreError> {
    let rows = tx.query(sql::COLUMNS, &[&sql::SCHEMA, &table]).await.map_err(|err| query_error(&err))?;
    if rows.len() != expected.len() {
        return Err(StoreError::InstallDrift { check });
    }
    for (row, expect) in rows.iter().zip(expected) {
        let name: String = row.try_get(0).map_err(|_| StoreError::InstallDrift { check })?;
        let kind: String = row.try_get(1).map_err(|_| StoreError::InstallDrift { check })?;
        if name != expect.0 || kind != expect.1 {
            return Err(StoreError::InstallDrift { check });
        }
    }
    Ok(())
}

async fn expect_list(
    tx: &Transaction<'_>,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
    expected: &[&str],
    check: &'static str,
) -> Result<(), StoreError> {
    let got = text_column(tx, sql, params).await.map_err(|_| StoreError::InstallDrift { check })?;
    if got.len() == expected.len() && got.iter().zip(expected).all(|(item, want)| item == want) {
        Ok(())
    } else {
        Err(StoreError::InstallDrift { check })
    }
}

async fn expect_bools(
    tx: &Transaction<'_>,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
    expected: &[bool],
    check: &'static str,
) -> Result<(), StoreError> {
    let row = tx.query_one(sql, params).await.map_err(|err| query_error(&err))?;
    if row.len() != expected.len() {
        return Err(StoreError::InstallDrift { check });
    }
    for (index, want) in expected.iter().enumerate() {
        let got: bool = row.try_get(index).map_err(|_| StoreError::InstallDrift { check })?;
        if got != *want {
            return Err(StoreError::InstallDrift { check });
        }
    }
    Ok(())
}

async fn text_column(tx: &Transaction<'_>, sql: &str, params: &[&(dyn ToSql + Sync)]) -> Result<Vec<String>, StoreError> {
    let rows = tx.query(sql, params).await.map_err(|err| query_error(&err))?;
    rows.iter().map(|row| row.try_get(0).map_err(|_| StoreError::InstallDrift { check: "text" })).collect()
}

async fn one_text(tx: &Transaction<'_>, sql: &str, params: &[&(dyn ToSql + Sync)]) -> Result<String, StoreError> {
    let row = tx.query_one(sql, params).await.map_err(|err| query_error(&err))?;
    row.try_get(0).map_err(|_| StoreError::InstallDrift { check: "text" })
}

async fn count_one(tx: &Transaction<'_>, sql: &str, params: &[&(dyn ToSql + Sync)]) -> Result<i64, StoreError> {
    let row = tx.query_one(sql, params).await.map_err(|err| query_error(&err))?;
    row.try_get(0).map_err(|_| StoreError::Activation)
}

async fn exec_one(
    tx: &Transaction<'_>,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
    check: &'static str,
) -> Result<(), StoreError> {
    let inserted = tx.execute(sql, params).await.map_err(|err| query_error(&err))?;
    if inserted == 1 { Ok(()) } else { Err(StoreError::InstallDrift { check }) }
}

fn normalize_roles(roles: &[&str]) -> Result<Vec<String>, StoreError> {
    if roles.len() > 8 {
        return Err(StoreError::Input);
    }
    let mut owned: Vec<String> = roles.iter().map(|role| (*role).to_owned()).collect();
    owned.sort();
    if owned.windows(2).any(|pair| pair[0] == pair[1]) || owned.iter().any(|role| !role::runtime_role_ok(role)) {
        return Err(StoreError::Allowlist);
    }
    Ok(owned)
}

fn not_installed(err: StoreError) -> StoreError {
    match err {
        StoreError::Query { sqlstate: Some(ref code) } if code == "42P01" || code == "3F000" => StoreError::NotInstalled,
        other => other,
    }
}

fn map_migrate(err: &Error) -> StoreError {
    match err.code().map(|code| code.code()) {
        Some("42P06" | "42P07" | "42710") => StoreError::Collision { check: "create" },
        _ => query_error(err),
    }
}

fn require_login(user: &str, expected: &str) -> Result<(), StoreError> {
    if user == expected { Ok(()) } else { Err(StoreError::LoginRole) }
}

fn endpoint_ok(endpoint: &Endpoint) -> Result<(), StoreError> {
    if endpoint.host.is_empty() || endpoint.database.is_empty() || endpoint.port == 0 || endpoint.host.chars().any(char::is_whitespace) {
        Err(StoreError::Input)
    } else {
        Ok(())
    }
}

fn sqlstate_of(err: &StoreError) -> Option<&str> {
    match err {
        StoreError::Query { sqlstate: Some(sqlstate) } => Some(sqlstate.as_str()),
        _ => None,
    }
}

fn missing_relation(err: &Error) -> bool {
    matches!(err.code().map(|code| code.code()), Some("42P01" | "3F000" | "42704"))
}

async fn finish_install(
    tx: Transaction<'_>,
    disposition: InstallDisposition,
    hash: String,
    project_id: &str,
) -> Result<InstallReceipt, StoreError> {
    match end_commit(tx).await {
        CommitEnd::Ack => Ok(InstallReceipt {
            contract: CONTRACT,
            disposition,
            sql_sha256: hash,
            project_id: project_id.to_owned(),
            database_used: true,
            finance_certified: false,
            semantic_certified: false,
            owner_authenticated: false,
            metric_certified: false,
            clock_used: false,
        }),
        CommitEnd::Server { sqlstate } => Err(StoreError::Query { sqlstate: Some(sqlstate) }),
        CommitEnd::Unknown => Err(StoreError::CommitUnknown),
    }
}

async fn finish_put(tx: Transaction<'_>, disposition: PutDisposition, mat: &Materialization) -> Result<PutReceipt, StoreError> {
    let snapshot_id = mat.snapshot_id.clone();
    let report_sha256 = mat.report_sha256.clone();
    let config_sha256 = mat.config_sha256.clone();
    let as_of = mat.as_of.clone();
    match end_commit(tx).await {
        CommitEnd::Ack => Ok(PutReceipt {
            contract: CONTRACT,
            disposition,
            snapshot_id,
            report_sha256,
            config_sha256,
            as_of,
            database_used: true,
            finance_certified: false,
            semantic_certified: false,
            owner_authenticated: false,
            metric_certified: false,
            clock_used: false,
        }),
        CommitEnd::Server { sqlstate } => Err(StoreError::Query { sqlstate: Some(sqlstate) }),
        CommitEnd::Unknown => Err(StoreError::CommitUnknown),
    }
}

fn activate_receipt(snapshot_id: &str, grants: usize) -> ActivateReceipt {
    ActivateReceipt {
        contract: CONTRACT,
        disposition: "activated",
        snapshot_id: snapshot_id.to_owned(),
        grants,
        database_used: true,
        finance_certified: false,
        semantic_certified: false,
        owner_authenticated: false,
        metric_certified: false,
        clock_used: false,
    }
}

async fn open(endpoint: &Endpoint, user: &str, password: &str, ca_pem: &[u8]) -> Result<Session, StoreError> {
    let tls = rustls_connector(ca_pem)?;
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(password);
    config.host(&endpoint.host);
    config.port(endpoint.port);
    config.dbname(&endpoint.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("enum-store");
    config.connect_timeout(Duration::from_secs(10));
    let (client, connection) = config.connect(tls).await.map_err(|_| StoreError::Connect)?;
    let _connection = tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(Session { client, _connection })
}

async fn require_tls(client: &Client) -> Result<(), StoreError> {
    let row = client.query_one(sql::TLS_FACT, &[]).await.map_err(|err| query_error(&err))?;
    let ssl: bool = row.try_get(0).map_err(|_| StoreError::Tls)?;
    let version: Option<String> = row.try_get(1).map_err(|_| StoreError::Tls)?;
    match version {
        Some(version) if ssl && version.starts_with("TLSv1.") => Ok(()),
        _ => Err(StoreError::Tls),
    }
}

async fn begin(session: &mut Session) -> Result<Transaction<'_>, StoreError> {
    let tx = session.client.transaction().await.map_err(|err| query_error(&err))?;
    if let Err(err) = tx.batch_execute(sql::SET_LOCAL).await {
        return Err(rollback_err(tx, query_error(&err)).await);
    }
    Ok(tx)
}

async fn require_role(tx: &Transaction<'_>, expected: &str) -> Result<(), StoreError> {
    let user = one_text(tx, sql::CURRENT_USER, &[]).await.map_err(|_| StoreError::LoginRole)?;
    if user == expected { Ok(()) } else { Err(StoreError::LoginRole) }
}

async fn rollback_err(tx: Transaction<'_>, err: StoreError) -> StoreError {
    let _ = tx.rollback().await;
    err
}

async fn end_commit(tx: Transaction<'_>) -> CommitEnd {
    match tx.commit().await {
        Ok(()) => CommitEnd::Ack,
        Err(err) => match commit_error(&err) {
            StoreError::Query { sqlstate: Some(sqlstate) } => CommitEnd::Server { sqlstate },
            _ => CommitEnd::Unknown,
        },
    }
}
