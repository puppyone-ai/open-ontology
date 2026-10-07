//! Transactional adapter. The business-calendar helper decides the dates.

use std::time::Duration;

use business_calendar::{resolve, sha256_hex, Closure, Input};
use postgres_types::ToSql;
use serde::Serialize;
use tokio_postgres::config::SslMode;
use tokio_postgres::{Client, Error, Transaction};

use crate::adopt;
use crate::error::{commit_error, query_error, StoreError};
use crate::publish::{self, PublicRow};
use crate::role::{self, ADMIN_ROLE, VIEW_ROLE};
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
    pub finance_approval: bool,
    pub authenticated: bool,
    pub ontology_published: bool,
    pub clock_used: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct PutReceipt {
    pub contract: &'static str,
    pub disposition: PutDisposition,
    pub snapshot_id: String,
    pub resolved_sha256: String,
    pub config_sha256: String,
    pub as_of: String,
    pub database_used: bool,
    pub finance_approval: bool,
    pub authenticated: bool,
    pub ontology_published: bool,
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
    pub finance_approval: bool,
    pub authenticated: bool,
    pub ontology_published: bool,
    pub clock_used: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AnchorColumn {
    pub name: &'static str,
    pub date: Option<String>,
    pub midnight_status: String,
    pub midnight_utc: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct ReadReceipt {
    pub contract: &'static str,
    pub project_id: String,
    pub business_tz: String,
    pub as_of_utc: String,
    pub calendar_version: String,
    pub anchors: Vec<AnchorColumn>,
    pub database_used: bool,
    pub finance_approval: bool,
    pub authenticated: bool,
    pub ontology_published: bool,
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
    pub calendar_version: &'a str,
    pub calendar_yaml: &'a str,
    pub as_of: &'a str,
    pub closure: Option<Closure<'a>>,
    pub expected_resolved_sha256: &'a str,
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
    pub calendar_version: &'a str,
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
    let hash = sha256_hex(sql::MIGRATION_SQL.as_bytes());
    if hash != sql::ACCEPTED_MIGRATION_SHA256 {
        return Err(StoreError::InstallDrift { check: "migration_sha256" });
    }
    Ok(hash)
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
    if !role::token_ok(request.project_id) || !role::token_ok(request.calendar_version) {
        return Err(StoreError::Input);
    }
    if !role::hex64(request.expected_resolved_sha256) {
        return Err(StoreError::Input);
    }
    let report = resolve(&Input {
        calendar_yaml: request.calendar_yaml,
        as_of: request.as_of,
        closure: request.closure,
    })
    .map_err(|refusal| StoreError::SnapshotRefused {
        reason: refusal.reason,
        detail: refusal.detail,
    })?;
    let (row, resolved, fingerprint) =
        publish::project_public(&report, request.calendar_yaml, request.project_id, request.calendar_version)?;
    if resolved != request.expected_resolved_sha256 {
        return Err(StoreError::ResolvedMismatch);
    }
    let json = publish::resolved_json(&report)?;
    let mut session = open(request.endpoint, request.user, request.password, request.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    match put_body(&tx, request, &row, &resolved, &fingerprint, &json).await {
        Ok(disposition) => finish_put(tx, disposition, row, resolved, report.config_sha256, report.as_of).await,
        Err(err) if sqlstate_of(&err) == Some(UNIQUE) => {
            let _ = tx.rollback().await;
            replay_after_conflict(request, &row, &resolved, &fingerprint, &json).await
        }
        Err(err) => Err(rollback_err(tx, err).await),
    }
}

pub async fn activate(request: &ActivateRequest<'_>) -> Result<ActivateReceipt, StoreError> {
    require_login(request.user, ADMIN_ROLE)?;
    endpoint_ok(request.endpoint)?;
    if !role::token_ok(request.project_id)
        || !role::token_ok(request.calendar_version)
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
    if rows.len() != 1 {
        let err = if rows.is_empty() { StoreError::NotFound } else { StoreError::HelperDisagreement };
        return Err(rollback_err(tx, err).await);
    }
    let receipt = match read_row(&rows[0]) {
        Ok(receipt) => receipt,
        Err(err) => return Err(rollback_err(tx, err).await),
    };
    match end_commit(tx).await {
        CommitEnd::Ack => Ok(receipt),
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
    tx.execute(sql::LOCK_INSTALL, &[&sql::DICT_LOCK]).await.map_err(|err| query_error(&err))?;
    tx.execute(sql::LOCK_INSTALL, &[&INSTALL_LOCK]).await.map_err(|err| query_error(&err))?;
    ensure_roles(tx, roles).await?;
    let sibling = adopt::sibling_state(tx).await?;
    if matches!(sibling, adopt::Sibling::Hostile) {
        return Err(StoreError::Collision { check: "sibling_role" });
    }
    let exists = tx
        .query_opt(sql::SCHEMA_EXISTS, &[&sql::SCHEMA])
        .await
        .map_err(|err| query_error(&err))?
        .is_some();
    if !exists {
        let dict_present = tx
            .query_opt(sql::SCHEMA_EXISTS, &[&sql::DICT_SCHEMA])
            .await
            .map_err(|err| query_error(&err))?
            .is_some();
        if dict_present {
            adopt::require_adoptable(tx).await?;
            let script = adopt::adoption_script()?;
            tx.batch_execute(&script).await.map_err(|err| map_migrate(&err))?;
        } else if tx.query_opt(sql::DICT_RELATION, &[]).await.map_err(|err| query_error(&err))?.is_some() {
            return Err(StoreError::Collision { check: "oo_dict" });
        } else {
            tx.batch_execute(sql::MIGRATION_SQL).await.map_err(|err| map_migrate(&err))?;
            let grant_sibling = matches!(sibling, adopt::Sibling::Safe);
            tx.batch_execute(&adopt::fresh_tail(grant_sibling)).await.map_err(|err| map_migrate(&err))?;
        }
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

async fn put_body(
    tx: &Transaction<'_>,
    request: &PutRequest<'_>,
    row: &PublicRow,
    resolved: &str,
    fingerprint: &str,
    json: &str,
) -> Result<PutDisposition, StoreError> {
    require_role(tx, ADMIN_ROLE).await?;
    tx.execute(sql::LOCK_INSTALL, &[&INSTALL_LOCK]).await.map_err(|err| query_error(&err))?;
    let (bound, flags_ok) = read_deployment(tx).await.map_err(not_installed)?;
    if !flags_ok {
        return Err(StoreError::Flag);
    }
    if bound != request.project_id {
        return Err(StoreError::ProjectBinding);
    }
    if let Some(existing) = tx
        .query_opt(sql::READ_SNAPSHOT, &[&row.snapshot_id])
        .await
        .map_err(|err| query_error(&err))?
    {
        return if snapshot_matches(&existing, request, row, resolved, fingerprint, json)? {
            Ok(PutDisposition::Replay)
        } else {
            Err(StoreError::SnapshotDrift)
        };
    }
    insert_snapshot(tx, request, row, resolved, fingerprint, json).await?;
    insert_published(tx, row).await?;
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
    let Some(snapshot) = tx
        .query_opt(sql::READ_SNAPSHOT, &[&request.snapshot_id])
        .await
        .map_err(|err| query_error(&err))?
    else {
        return Err(StoreError::Activation);
    };
    let project: String = snapshot.try_get(0).map_err(|_| StoreError::Activation)?;
    let version: String = snapshot.try_get(1).map_err(|_| StoreError::Activation)?;
    let config: String = snapshot.try_get(2).map_err(|_| StoreError::Activation)?;
    let as_of: String = snapshot.try_get(3).map_err(|_| StoreError::Activation)?;
    if project != request.project_id
        || version != request.calendar_version
        || config != request.config_sha256
        || as_of != request.as_of
    {
        return Err(StoreError::Activation);
    }
    let expected = request.expected_current.map(str::to_owned);
    let updated = tx
        .execute(
            sql::ACTIVATE,
            &[&request.snapshot_id, &request.calendar_version, &request.as_of, &request.project_id, &expected],
        )
        .await
        .map_err(|err| query_error(&err))?;
    if updated != 1 {
        return Err(StoreError::CasConflict);
    }
    let roles = text_column(tx, sql::READ_ALLOW, &[]).await?;
    let dict_owner = one_text(tx, sql::SCHEMA_OWNER, &[&sql::DICT_SCHEMA]).await?;
    for role in &roles {
        if dict_owner == ADMIN_ROLE {
            let usage = role::grant_usage_sql(role)?;
            tx.batch_execute(&usage).await.map_err(|err| query_error(&err))?;
        } else {
            tx.execute(sql::CALL_GRANT_RUNTIME, &[&role.as_str()]).await.map_err(|err| query_error(&err))?;
        }
        let granted = tx
            .query_one(sql::SCHEMA_USAGE, &[&role.as_str(), &sql::DICT_SCHEMA])
            .await
            .map_err(|err| query_error(&err))?;
        let ok: bool = granted.try_get(0).map_err(|_| StoreError::Activation)?;
        if !ok {
            return Err(StoreError::Activation);
        }
        let select = role::grant_select_batch(role)?;
        tx.batch_execute(&select).await.map_err(|err| query_error(&err))?;
    }
    Ok(roles.len())
}

async fn replay_after_conflict(
    request: &PutRequest<'_>,
    row: &PublicRow,
    resolved: &str,
    fingerprint: &str,
    json: &str,
) -> Result<PutReceipt, StoreError> {
    let mut session = open(request.endpoint, request.user, request.password, request.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let existing = match tx.query_opt(sql::READ_SNAPSHOT, &[&row.snapshot_id]).await {
        Ok(Some(row)) => row,
        Ok(None) => return Err(rollback_err(tx, StoreError::SnapshotDrift).await),
        Err(err) => return Err(rollback_err(tx, query_error(&err)).await),
    };
    let matched = snapshot_matches(&existing, request, row, resolved, fingerprint, json).unwrap_or(false);
    if matched {
        finish_put(
            tx,
            PutDisposition::Replay,
            row.clone(),
            resolved.to_owned(),
            sha256_hex(request.calendar_yaml.as_bytes()),
            row.as_of.clone(),
        )
        .await
    } else {
        Err(rollback_err(tx, StoreError::SnapshotDrift).await)
    }
}

fn snapshot_matches(
    existing: &tokio_postgres::Row,
    request: &PutRequest<'_>,
    row: &PublicRow,
    resolved: &str,
    fingerprint: &str,
    json: &str,
) -> Result<bool, StoreError> {
    let project: String = existing.try_get(0).map_err(|_| StoreError::SnapshotDrift)?;
    let version: String = existing.try_get(1).map_err(|_| StoreError::SnapshotDrift)?;
    let config: String = existing.try_get(2).map_err(|_| StoreError::SnapshotDrift)?;
    let as_of: String = existing.try_get(3).map_err(|_| StoreError::SnapshotDrift)?;
    let stored_fingerprint: String = existing.try_get(4).map_err(|_| StoreError::SnapshotDrift)?;
    let stored_resolved: String = existing.try_get(5).map_err(|_| StoreError::SnapshotDrift)?;
    let yaml: String = existing.try_get(6).map_err(|_| StoreError::SnapshotDrift)?;
    let closure_date: Option<String> = existing.try_get(7).map_err(|_| StoreError::SnapshotDrift)?;
    let closure_label: Option<String> = existing.try_get(8).map_err(|_| StoreError::SnapshotDrift)?;
    let closure_hash: Option<String> = existing.try_get(9).map_err(|_| StoreError::SnapshotDrift)?;
    let stored_json: String = existing.try_get(10).map_err(|_| StoreError::SnapshotDrift)?;
    let tz: String = existing.try_get(11).map_err(|_| StoreError::SnapshotDrift)?;
    let (date, label, hash) = closure_parts(request);
    Ok(project == request.project_id
        && version == request.calendar_version
        && config == sha256_hex(request.calendar_yaml.as_bytes())
        && as_of == row.as_of
        && stored_fingerprint == fingerprint
        && stored_resolved == resolved
        && yaml == request.calendar_yaml
        && closure_date.as_deref() == date
        && closure_label.as_deref() == label
        && closure_hash.as_deref() == hash
        && stored_json == json
        && tz == row.business_tz)
}

fn closure_parts<'a>(request: &'a PutRequest<'_>) -> (Option<&'a str>, Option<&'a str>, Option<&'a str>) {
    match request.closure {
        Some(closure) => (
            Some(closure.last_closed_period_end),
            Some(closure.label),
            Some(closure.record_sha256),
        ),
        None => (None, None, None),
    }
}

async fn insert_snapshot(
    tx: &Transaction<'_>,
    request: &PutRequest<'_>,
    row: &PublicRow,
    resolved: &str,
    fingerprint: &str,
    json: &str,
) -> Result<(), StoreError> {
    let (date, label, hash) = closure_parts(request);
    let config = sha256_hex(request.calendar_yaml.as_bytes());
    let inserted = tx
        .execute(
            sql::INSERT_SNAPSHOT,
            &[
                &row.snapshot_id,
                &request.project_id,
                &request.calendar_version,
                &config,
                &row.as_of,
                &fingerprint,
                &resolved,
                &request.calendar_yaml,
                &date,
                &label,
                &hash,
                &json,
                &row.business_tz,
            ],
        )
        .await
        .map_err(|err| query_error(&err))?;
    if inserted == 1 { Ok(()) } else { Err(StoreError::SnapshotDrift) }
}

async fn insert_published(tx: &Transaction<'_>, row: &PublicRow) -> Result<(), StoreError> {
    let s = &row.slots;
    let inserted = tx
        .execute(
            sql::INSERT_PUBLISHED,
            &[
                &row.snapshot_id,
                &row.project_id,
                &row.business_tz,
                &row.as_of,
                &row.calendar_version,
                &s[0].date,
                &s[0].status,
                &s[0].midnight,
                &s[1].date,
                &s[1].status,
                &s[1].midnight,
                &s[2].date,
                &s[2].status,
                &s[2].midnight,
                &s[3].date,
                &s[3].status,
                &s[3].midnight,
                &s[4].date,
                &s[4].status,
                &s[4].midnight,
                &s[5].date,
                &s[5].status,
                &s[5].midnight,
                &s[6].date,
                &s[6].status,
                &s[6].midnight,
                &s[7].date,
                &s[7].status,
                &s[7].midnight,
                &s[8].date,
                &s[8].status,
                &s[8].midnight,
                &s[9].date,
                &s[9].status,
                &s[9].midnight,
            ],
        )
        .await
        .map_err(|err| query_error(&err))?;
    if inserted == 1 { Ok(()) } else { Err(StoreError::SnapshotDrift) }
}

fn read_row(row: &tokio_postgres::Row) -> Result<ReadReceipt, StoreError> {
    let mut anchors = Vec::with_capacity(publish::ANCHORS.len());
    for (index, name) in publish::ANCHORS.iter().enumerate() {
        let base = 4 + index * 3;
        anchors.push(AnchorColumn {
            name,
            date: row.try_get(base).map_err(|_| StoreError::HelperDisagreement)?,
            midnight_status: row.try_get(base + 1).map_err(|_| StoreError::HelperDisagreement)?,
            midnight_utc: row.try_get(base + 2).map_err(|_| StoreError::HelperDisagreement)?,
        });
    }
    Ok(ReadReceipt {
        contract: CONTRACT,
        project_id: row.try_get(0).map_err(|_| StoreError::HelperDisagreement)?,
        business_tz: row.try_get(1).map_err(|_| StoreError::HelperDisagreement)?,
        as_of_utc: row.try_get(2).map_err(|_| StoreError::HelperDisagreement)?,
        calendar_version: row.try_get(3).map_err(|_| StoreError::HelperDisagreement)?,
        anchors,
        database_used: true,
        finance_approval: false,
        authenticated: false,
        ontology_published: false,
        clock_used: false,
    })
}

async fn verify_catalog(tx: &Transaction<'_>, roles: &[String]) -> Result<(), StoreError> {
    let owner = one_text(tx, sql::SCHEMA_OWNER, &[&sql::SCHEMA]).await?;
    if owner != ADMIN_ROLE {
        return Err(StoreError::InstallDrift { check: "control_owner" });
    }
    adopt::verify_shared(tx).await?;
    expect_list(tx, sql::RELATIONS, &[&sql::SCHEMA], &sql::EXPECTED_RELATIONS, "relations").await?;
    expect_columns(tx, "install_manifest", &sql::EXPECTED_MANIFEST_COLUMNS, "manifest_columns").await?;
    expect_columns(tx, "deployment", &sql::EXPECTED_DEPLOYMENT_COLUMNS, "deployment_columns").await?;
    expect_columns(tx, "runtime_allow", &sql::EXPECTED_ALLOW_COLUMNS, "allow_columns").await?;
    expect_columns(tx, "snapshot", &sql::EXPECTED_SNAPSHOT_COLUMNS, "snapshot_columns").await?;
    expect_columns(tx, "published", &sql::EXPECTED_PUBLISHED_COLUMNS, "published_columns").await?;
    expect_columns(tx, "active_pointer", &sql::EXPECTED_POINTER_COLUMNS, "pointer_columns").await?;
    expect_list(tx, sql::CONSTRAINTS, &[&sql::SCHEMA, &"install_manifest"], &sql::EXPECTED_MANIFEST_CONSTRAINTS, "manifest_constraints").await?;
    expect_list(tx, sql::CONSTRAINTS, &[&sql::SCHEMA, &"deployment"], &sql::EXPECTED_DEPLOYMENT_CONSTRAINTS, "deployment_constraints").await?;
    expect_list(tx, sql::CONSTRAINTS, &[&sql::SCHEMA, &"runtime_allow"], &sql::EXPECTED_ALLOW_CONSTRAINTS, "allow_constraints").await?;
    expect_list(tx, sql::CONSTRAINTS, &[&sql::SCHEMA, &"snapshot"], &sql::EXPECTED_SNAPSHOT_CONSTRAINTS, "snapshot_constraints").await?;
    expect_list(tx, sql::CONSTRAINTS, &[&sql::SCHEMA, &"published"], &sql::EXPECTED_PUBLISHED_CONSTRAINTS, "published_constraints").await?;
    expect_list(tx, sql::CONSTRAINTS, &[&sql::SCHEMA, &"active_pointer"], &sql::EXPECTED_POINTER_CONSTRAINTS, "pointer_constraints").await?;
    expect_view(tx).await?;
    expect_bools(tx, sql::VIEW_GRANTS, &[], &sql::EXPECTED_VIEW_GRANTS, "view_grants").await?;
    for role in roles {
        expect_bools(tx, sql::CONTROL_GRANTS, &[role], &sql::EXPECTED_RUNTIME_CONTROL, "runtime_control").await?;
    }
    expect_closed(tx, sql::SCHEMA).await?;
    expect_closed(tx, sql::DICT_SCHEMA).await?;
    for table in ["install_manifest", "deployment", "runtime_allow", "snapshot", "published", "active_pointer"] {
        expect_relation_closed(tx, sql::SCHEMA, table).await?;
    }
    expect_relation_closed(tx, sql::DICT_SCHEMA, sql::VIEW).await?;
    Ok(())
}

async fn expect_view(tx: &Transaction<'_>) -> Result<(), StoreError> {
    let objects = tx.query(sql::DICT_OBJECTS, &[]).await.map_err(|err| query_error(&err))?;
    let mut found = false;
    for object in &objects {
        let name: String = object.try_get(0).map_err(|_| StoreError::InstallDrift { check: "dict_objects" })?;
        let kind: String = object.try_get(1).map_err(|_| StoreError::InstallDrift { check: "dict_objects" })?;
        if name == sql::VIEW {
            if kind != "v" {
                return Err(StoreError::InstallDrift { check: "dict_objects" });
            }
            found = true;
        }
    }
    if !found {
        return Err(StoreError::InstallDrift { check: "dict_objects" });
    }
    let fact = tx.query_one(sql::VIEW_FACT, &[]).await.map_err(|err| query_error(&err))?;
    let owner: String = fact.try_get(0).map_err(|_| StoreError::InstallDrift { check: "view_owner" })?;
    let options: String = fact.try_get(1).map_err(|_| StoreError::InstallDrift { check: "view_options" })?;
    let relkind: String = fact.try_get(2).map_err(|_| StoreError::InstallDrift { check: "view_kind" })?;
    if owner != VIEW_ROLE || relkind != "v" || !options.contains("security_barrier=true") || !options.contains("security_invoker=false")
    {
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
    let flags = [1, 2, 3, 4].iter().all(|index| row.try_get::<_, bool>(*index).ok() == Some(false));
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

async fn text_column(
    tx: &Transaction<'_>,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<Vec<String>, StoreError> {
    let rows = tx.query(sql, params).await.map_err(|err| query_error(&err))?;
    rows.iter().map(|row| row.try_get(0).map_err(|_| StoreError::InstallDrift { check: "text" })).collect()
}

async fn one_text(tx: &Transaction<'_>, sql: &str, params: &[&(dyn ToSql + Sync)]) -> Result<String, StoreError> {
    let row = tx.query_one(sql, params).await.map_err(|err| query_error(&err))?;
    row.try_get(0).map_err(|_| StoreError::InstallDrift { check: "text" })
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
    if endpoint.host.is_empty()
        || endpoint.database.is_empty()
        || endpoint.port == 0
        || endpoint.host.chars().any(char::is_whitespace)
    {
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
            finance_approval: false,
            authenticated: false,
            ontology_published: false,
            clock_used: false,
        }),
        CommitEnd::Server { sqlstate } => Err(StoreError::Query { sqlstate: Some(sqlstate) }),
        CommitEnd::Unknown => Err(StoreError::CommitUnknown),
    }
}

async fn finish_put(
    tx: Transaction<'_>,
    disposition: PutDisposition,
    row: PublicRow,
    resolved: String,
    config: String,
    as_of: String,
) -> Result<PutReceipt, StoreError> {
    match end_commit(tx).await {
        CommitEnd::Ack => Ok(PutReceipt {
            contract: CONTRACT,
            disposition,
            snapshot_id: row.snapshot_id,
            resolved_sha256: resolved,
            config_sha256: config,
            as_of,
            database_used: true,
            finance_approval: false,
            authenticated: false,
            ontology_published: false,
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
        finance_approval: false,
        authenticated: false,
        ontology_published: false,
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
    config.application_name("calendar-store");
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
