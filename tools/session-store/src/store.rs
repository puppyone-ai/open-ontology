//! Transactional adapter. The reducer decides. This module only locks, checks, and persists.

use std::future::Future;
use std::time::Duration;

use postgres_types::ToSql;
use serde::Serialize;
use session_transition::{
    transition as reduce_transition, Event, IdentityTuple, Outcome, Reason, SessionState, TransitionInput,
    TransitionOutput, CONTRACT as REDUCER_CONTRACT,
};
use tokio_postgres::config::SslMode;
use tokio_postgres::{Client, Error, Row, Transaction};

use crate::error::{commit_error, query_error, StoreError};
use crate::sha256::{hex, sha256};
use crate::sql::{self, STATE_JSON_MAX};
use crate::tls::{hide_commit_connector, rustls_connector, CommitFault};
use crate::{CONTRACT, PROBE_CONTRACT};

const UNIQUE_VIOLATION: &str = "23505";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub database: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    Acknowledged,
    RolledBack,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallDisposition {
    Installed,
    Replay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapDisposition {
    Inserted,
    Replay,
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileDisposition {
    Durable,
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct InstallReceipt {
    pub contract: &'static str,
    pub disposition: InstallDisposition,
    pub sql_sha256: String,
    pub database_used: bool,
    pub auth_provider_used: bool,
    pub runtime_used: bool,
    pub physical_release_certified: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct BootstrapReceipt {
    pub contract: &'static str,
    pub disposition: BootstrapDisposition,
    pub reason: Option<Reason>,
    pub database_used: bool,
    pub auth_provider_used: bool,
    pub runtime_used: bool,
    pub physical_release_certified: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct TransitionReceipt {
    pub contract: &'static str,
    pub disposition: Disposition,
    pub outcome: Option<Outcome>,
    pub reason: Option<Reason>,
    pub revision: Option<u64>,
    pub owner_generation: Option<u64>,
    pub phase: Option<session_transition::Phase>,
    pub database_used: bool,
    pub auth_provider_used: bool,
    pub runtime_used: bool,
    pub physical_release_certified: bool,
    pub sqlstate: Option<String>,
    pub reducer: Option<Box<TransitionOutput>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct GetReceipt {
    pub contract: &'static str,
    pub disposition: &'static str,
    pub database_used: bool,
    pub auth_provider_used: bool,
    pub runtime_used: bool,
    pub physical_release_certified: bool,
    pub revision: u64,
    pub owner_generation: u64,
    pub journal_len: usize,
    pub state: SessionState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct ReconcileReceipt {
    pub contract: &'static str,
    pub disposition: ReconcileDisposition,
    pub database_used: bool,
    pub auth_provider_used: bool,
    pub runtime_used: bool,
    pub physical_release_certified: bool,
    pub revision: Option<u64>,
    pub owner_generation: Option<u64>,
}

pub struct TransitionRequest<'a> {
    pub endpoint: &'a Endpoint,
    pub user: &'a str,
    pub password: &'a str,
    pub ca_pem: &'a [u8],
    pub project_id: &'a str,
    pub session_id: &'a str,
    pub identity: &'a IdentityTuple,
    pub expected_revision: u64,
    pub expected_owner_generation: u64,
    pub event_id: &'a str,
    pub now_ms: u64,
    pub event: &'a Event,
}

pub fn migration_sha256() -> Result<String, StoreError> {
    let digest = sha256(sql::MIGRATION_SQL.as_bytes()).ok_or(StoreError::Input)?;
    Ok(hex(&digest))
}

pub async fn install(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
) -> Result<InstallReceipt, StoreError> {
    require_login(user, sql::ADMIN_ROLE)?;
    let hash = migration_sha256()?;
    let mut session = open(endpoint, user, password, ca_pem, CommitFault::None).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let disposition = match install_body(&tx, &hash).await {
        Ok(disposition) => disposition,
        Err(err) => return Err(rollback_err(tx, err).await),
    };
    match end_commit(tx).await {
        CommitEnd::Ack => Ok(install_receipt(disposition, hash)),
        CommitEnd::Server { sqlstate } => Err(StoreError::Query { sqlstate: Some(sqlstate) }),
        CommitEnd::Unknown => Err(StoreError::CommitUnknown),
    }
}

async fn install_body(tx: &Transaction<'_>, hash: &str) -> Result<InstallDisposition, StoreError> {
    require_role(tx, sql::ADMIN_ROLE).await?;
    tx.execute(sql::LOCK_INSTALL, &[&sql::INSTALL_LOCK])
        .await
        .map_err(|err| query_error(&err))?;
    let exists = tx
        .query_opt(sql::SCHEMA_EXISTS, &[&sql::SCHEMA])
        .await
        .map_err(|err| query_error(&err))?
        .is_some();
    if !exists {
        tx.batch_execute(sql::MIGRATION_SQL)
            .await
            .map_err(|err| query_error(&err))?;
        let inserted = tx
            .execute(sql::INSERT_MANIFEST, &[&hash])
            .await
            .map_err(|err| query_error(&err))?;
        if inserted != 1 {
            return Err(drift("manifest_insert"));
        }
        verify_shape(tx).await?;
        return Ok(InstallDisposition::Installed);
    }
    let row = match tx.query_opt(sql::READ_MANIFEST, &[]).await {
        Ok(row) => row,
        Err(err) if missing_relation(&err) => return Err(drift("manifest_relation")),
        Err(err) => return Err(query_error(&err)),
    };
    let Some(row) = row else {
        return Err(drift("manifest_missing"));
    };
    let stored: String = row.try_get(0).map_err(|_| drift("manifest_row"))?;
    if stored != hash {
        return Err(drift("hash"));
    }
    verify_shape(tx).await?;
    Ok(InstallDisposition::Replay)
}

pub async fn bootstrap(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    project_id: &str,
    session_id: &str,
    state: &SessionState,
) -> Result<BootstrapReceipt, StoreError> {
    require_login(user, sql::ADMIN_ROLE)?;
    token_key(project_id)?;
    token_key(session_id)?;
    match probe_state(state) {
        Probe::StateInvalid => return Ok(bootstrap_receipt(BootstrapDisposition::Refused, Some(Reason::StateInvalid), false)),
        Probe::Disagreement => return Err(StoreError::ReducerDisagreement),
        Probe::Valid => {}
    }
    if state.session_id != session_id {
        return Err(StoreError::Input);
    }
    if let Some(bound) = &state.bound
        && bound.project_id != project_id
    {
        return Err(StoreError::ProjectKeyMismatch);
    }
    let write = write_of(state)?;
    let mut session = open(endpoint, user, password, ca_pem, CommitFault::None).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    if let Err(err) = require_role(&tx, sql::ADMIN_ROLE).await {
        return Err(rollback_err(tx, err).await);
    }
    match insert_head(&tx, project_id, session_id, &write).await {
        Ok(()) => match end_commit(tx).await {
            CommitEnd::Ack => Ok(bootstrap_receipt(BootstrapDisposition::Inserted, None, true)),
            CommitEnd::Server { sqlstate } => Err(StoreError::Query { sqlstate: Some(sqlstate) }),
            CommitEnd::Unknown => Err(StoreError::CommitUnknown),
        },
        Err(err) if sqlstate_of(&err) == Some(UNIQUE_VIOLATION) => {
            let _ = tx.rollback().await;
            drop(session);
            bootstrap_existing(endpoint, user, password, ca_pem, project_id, session_id, state).await
        }
        Err(err) => Err(rollback_err(tx, err).await),
    }
}

async fn bootstrap_existing(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    project_id: &str,
    session_id: &str,
    state: &SessionState,
) -> Result<BootstrapReceipt, StoreError> {
    let mut session = open(endpoint, user, password, ca_pem, CommitFault::None).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    if let Err(err) = require_role(&tx, sql::ADMIN_ROLE).await {
        return Err(rollback_err(tx, err).await);
    }
    match load(&tx, project_id, session_id, true).await {
        Ok(head) if head.state == *state => match end_commit(tx).await {
            CommitEnd::Ack => Ok(bootstrap_receipt(BootstrapDisposition::Replay, None, true)),
            CommitEnd::Server { sqlstate } => Err(StoreError::Query { sqlstate: Some(sqlstate) }),
            CommitEnd::Unknown => Err(StoreError::CommitUnknown),
        },
        Ok(_) => {
            let _ = tx.rollback().await;
            Err(StoreError::BootstrapConflict)
        }
        Err(StoreError::NotFound) => {
            let _ = tx.rollback().await;
            Err(StoreError::Query {
                sqlstate: Some(UNIQUE_VIOLATION.to_owned()),
            })
        }
        Err(err) => Err(rollback_err(tx, err).await),
    }
}

pub async fn transition(request: &TransitionRequest<'_>) -> Result<TransitionReceipt, StoreError> {
    transition_with_fault(request, CommitFault::None).await
}

pub async fn transition_with_fault(
    request: &TransitionRequest<'_>,
    fault: CommitFault,
) -> Result<TransitionReceipt, StoreError> {
    require_login(request.user, sql::RUNTIME_ROLE)?;
    token_key(request.project_id)?;
    token_key(request.session_id)?;
    if request.identity.project_id != request.project_id {
        return Err(StoreError::ProjectKeyMismatch);
    }
    if principal_is_login(&request.identity.principal_id, sql::RUNTIME_ROLE) {
        return Err(StoreError::PrincipalNotAuthentication);
    }
    let mut session = open(request.endpoint, request.user, request.password, request.ca_pem, fault).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    if let Err(err) = prepare_runtime(&tx, &request.identity.principal_id).await {
        return Err(rollback_err(tx, err).await);
    }
    let head = match load(&tx, request.project_id, request.session_id, true).await {
        Ok(head) => head,
        Err(err) => return Err(rollback_err(tx, err).await),
    };
    let output = reduce(&head, request);
    if let Err(err) = persist_or_keep(&tx, &head, &output, request).await {
        return Err(rollback_err(tx, err).await);
    }
    // COMMIT was either not sent (rollback fault) or its reply is classified below.
    // A missing SQLSTATE is unknown. This path does not cancel the backend.
    match fault {
        CommitFault::RollBackBeforeCommit => end_rollback(tx).await,
        CommitFault::None | CommitFault::DropCommitResponse => match end_commit(tx).await {
            CommitEnd::Ack => Ok(acknowledged(output)),
            CommitEnd::Server { sqlstate } => Ok(rolled_back(Some(sqlstate))),
            CommitEnd::Unknown => Err(StoreError::CommitUnknown),
        },
    }
}

pub async fn get(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    project_id: &str,
    session_id: &str,
) -> Result<GetReceipt, StoreError> {
    require_login(user, sql::RUNTIME_ROLE)?;
    token_key(project_id)?;
    token_key(session_id)?;
    let mut session = open(endpoint, user, password, ca_pem, CommitFault::None).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    if let Err(err) = require_role(&tx, sql::RUNTIME_ROLE).await {
        return Err(rollback_err(tx, err).await);
    }
    let head = match load(&tx, project_id, session_id, false).await {
        Ok(head) => head,
        Err(err) => return Err(rollback_err(tx, err).await),
    };
    match end_commit(tx).await {
        CommitEnd::Ack => Ok(GetReceipt {
            contract: CONTRACT,
            disposition: "present",
            database_used: true,
            auth_provider_used: false,
            runtime_used: false,
            physical_release_certified: false,
            revision: head.revision,
            owner_generation: head.owner_generation,
            journal_len: head.state.journal.len(),
            state: head.state,
        }),
        CommitEnd::Server { sqlstate } => Err(StoreError::Query { sqlstate: Some(sqlstate) }),
        CommitEnd::Unknown => Err(StoreError::CommitUnknown),
    }
}

pub async fn reconcile(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    project_id: &str,
    session_id: &str,
    event_id: &str,
) -> Result<ReconcileReceipt, StoreError> {
    require_login(user, sql::RUNTIME_ROLE)?;
    token_key(project_id)?;
    token_key(session_id)?;
    if event_id.is_empty() || event_id.as_bytes().contains(&0) {
        return Err(StoreError::Input);
    }
    let mut session = open(endpoint, user, password, ca_pem, CommitFault::None).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    if let Err(err) = require_role(&tx, sql::RUNTIME_ROLE).await {
        return Err(rollback_err(tx, err).await);
    }
    let found = match load(&tx, project_id, session_id, false).await {
        Ok(head) => Some(head),
        Err(StoreError::NotFound) => None,
        Err(err) => return Err(rollback_err(tx, err).await),
    };
    let receipt = match &found {
        Some(head) if head.state.journal.iter().any(|entry| entry.event_id == event_id) => {
            reconcile_receipt(ReconcileDisposition::Durable, Some(head.revision), Some(head.owner_generation))
        }
        Some(_) | None => reconcile_receipt(ReconcileDisposition::Absent, None, None),
    };
    match end_commit(tx).await {
        CommitEnd::Ack => Ok(receipt),
        CommitEnd::Server { sqlstate } => Err(StoreError::Query { sqlstate: Some(sqlstate) }),
        CommitEnd::Unknown => Err(StoreError::CommitUnknown),
    }
}

enum CommitEnd {
    Ack,
    Server { sqlstate: String },
    Unknown,
}

enum Probe {
    Valid,
    StateInvalid,
    Disagreement,
}

struct Session {
    client: Client,
    connection: tokio::task::JoinHandle<()>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.connection.abort();
    }
}

struct Head {
    revision: u64,
    owner_generation: u64,
    revision_text: String,
    generation_text: String,
    state: SessionState,
}

struct HeadWrite {
    revision: String,
    generation: String,
    bound_project_id: Option<String>,
    bound_principal_id: Option<String>,
    bound_role: Option<String>,
    bound_claims_hash: Option<String>,
    bound_policy_version: Option<String>,
    bound_knowledge_version: Option<String>,
    bound_config_version: Option<String>,
    state_json: String,
    journal_len: i32,
}

struct RawHead {
    revision: String,
    owner_generation: String,
    bound_project_id: Option<String>,
    bound_principal_id: Option<String>,
    bound_role: Option<String>,
    bound_claims_hash: Option<String>,
    bound_policy_version: Option<String>,
    bound_knowledge_version: Option<String>,
    bound_config_version: Option<String>,
    state_json: String,
    journal_len: i32,
}

fn require_login(user: &str, expected: &str) -> Result<(), StoreError> {
    if user == expected { Ok(()) } else { Err(StoreError::LoginRole) }
}

fn token_key(value: &str) -> Result<(), StoreError> {
    if session_transition::is_token(value) {
        Ok(())
    } else {
        Err(StoreError::Input)
    }
}

fn principal_is_login(principal_id: &str, login: &str) -> bool {
    principal_id == login || principal_id == sql::ADMIN_ROLE || principal_id == sql::RUNTIME_ROLE
}

fn endpoint_ok(endpoint: &Endpoint) -> Result<(), StoreError> {
    if endpoint.host.is_empty() || endpoint.database.is_empty() || endpoint.port == 0 {
        Err(StoreError::Input)
    } else {
        Ok(())
    }
}

fn pg_config(endpoint: &Endpoint, user: &str, password: &str) -> Result<tokio_postgres::Config, StoreError> {
    endpoint_ok(endpoint)?;
    if user.is_empty() || password.is_empty() {
        return Err(StoreError::Input);
    }
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(password);
    config.host(&endpoint.host);
    config.port(endpoint.port);
    config.dbname(&endpoint.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("session-store");
    config.connect_timeout(Duration::from_secs(10));
    Ok(config)
}

fn detach(
    connection: impl Future<Output = Result<(), Error>> + Send + 'static,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let _ = connection.await;
    })
}

async fn open(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    fault: CommitFault,
) -> Result<Session, StoreError> {
    let config = pg_config(endpoint, user, password)?;
    let (client, connection) = match fault {
        CommitFault::DropCommitResponse => {
            let tls = hide_commit_connector(ca_pem)?;
            let (client, connection) = config.connect(tls).await.map_err(|_| StoreError::Connect)?;
            (client, detach(connection))
        }
        CommitFault::None | CommitFault::RollBackBeforeCommit => {
            let tls = rustls_connector(ca_pem)?;
            let (client, connection) = config.connect(tls).await.map_err(|_| StoreError::Connect)?;
            (client, detach(connection))
        }
    };
    Ok(Session { client, connection })
}

async fn require_tls(client: &Client) -> Result<(), StoreError> {
    let row = client
        .query_one(sql::TLS_FACT, &[])
        .await
        .map_err(|err| query_error(&err))?;
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
    let user = current_user(tx).await?;
    if user == expected {
        Ok(())
    } else {
        Err(StoreError::LoginRole)
    }
}

async fn prepare_runtime(tx: &Transaction<'_>, principal_id: &str) -> Result<(), StoreError> {
    let user = current_user(tx).await?;
    if user != sql::RUNTIME_ROLE {
        return Err(StoreError::LoginRole);
    }
    if principal_is_login(principal_id, &user) {
        Err(StoreError::PrincipalNotAuthentication)
    } else {
        Ok(())
    }
}

async fn current_user(tx: &Transaction<'_>) -> Result<String, StoreError> {
    let row = tx
        .query_one(sql::CURRENT_USER, &[])
        .await
        .map_err(|err| query_error(&err))?;
    row.try_get(0).map_err(|_| StoreError::LoginRole)
}

async fn rollback_err(tx: Transaction<'_>, err: StoreError) -> StoreError {
    let _ = tx.rollback().await;
    err
}

async fn end_commit(tx: Transaction<'_>) -> CommitEnd {
    match tx.commit().await {
        Ok(()) => CommitEnd::Ack,
        Err(err) => classify_commit(&err),
    }
}

fn sqlstate_of(err: &StoreError) -> Option<&str> {
    match err {
        StoreError::Query { sqlstate: Some(sqlstate) } => Some(sqlstate.as_str()),
        _ => None,
    }
}

fn drift(check: &'static str) -> StoreError {
    StoreError::InstallDrift { check }
}

fn missing_relation(err: &Error) -> bool {
    matches!(err.code().map(|code| code.code()), Some("42P01" | "3F000" | "42704"))
}

fn classify_commit(err: &Error) -> CommitEnd {
    match commit_error(err) {
        StoreError::Query { sqlstate: Some(sqlstate) } => CommitEnd::Server { sqlstate },
        _ => CommitEnd::Unknown,
    }
}

async fn end_rollback(tx: Transaction<'_>) -> Result<TransitionReceipt, StoreError> {
    match tx.rollback().await {
        Ok(()) => Ok(rolled_back(None)),
        Err(err) => match classify_commit(&err) {
            CommitEnd::Server { sqlstate } => Err(StoreError::Query { sqlstate: Some(sqlstate) }),
            CommitEnd::Ack | CommitEnd::Unknown => Err(StoreError::CommitUnknown),
        },
    }
}

fn probe_state(state: &SessionState) -> Probe {
    let identity = IdentityTuple {
        project_id: "probe-project".to_owned(),
        principal_id: "probe-principal".to_owned(),
        role: "probe-role".to_owned(),
        claims_hash: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_owned(),
        policy_version: "probe-pol".to_owned(),
        knowledge_version: "probe-know".to_owned(),
        config_version: "probe-cfg".to_owned(),
    };
    let output = reduce_transition(&TransitionInput {
        contract: PROBE_CONTRACT.to_owned(),
        state: state.clone(),
        identity,
        expected_revision: state.revision,
        expected_owner_generation: state.owner_generation,
        event_id: "probe-event".to_owned(),
        now_ms: state.last_now_ms,
        event: Event::IdleTimeout,
    });
    match (output.outcome, output.reason) {
        (Outcome::Refused, Some(Reason::InputInvalid)) => Probe::Valid,
        (Outcome::Refused, Some(Reason::StateInvalid)) => Probe::StateInvalid,
        _ => Probe::Disagreement,
    }
}

fn reduce(head: &Head, request: &TransitionRequest<'_>) -> TransitionOutput {
    reduce_transition(&TransitionInput {
        contract: REDUCER_CONTRACT.to_owned(),
        state: head.state.clone(),
        identity: request.identity.clone(),
        expected_revision: request.expected_revision,
        expected_owner_generation: request.expected_owner_generation,
        event_id: request.event_id.to_owned(),
        now_ms: request.now_ms,
        event: request.event.clone(),
    })
}

async fn persist_or_keep(
    tx: &Transaction<'_>,
    head: &Head,
    output: &TransitionOutput,
    request: &TransitionRequest<'_>,
) -> Result<(), StoreError> {
    match output.outcome {
        Outcome::Applied => {
            accept_applied(&head.state, output, request.project_id, request.session_id, request.event_id)?;
            let write = write_of(&output.state)?;
            let updated = tx
                .execute(
                    sql::UPDATE_HEAD,
                    &[
                        &request.project_id,
                        &request.session_id,
                        &write.revision,
                        &write.generation,
                        &write.bound_project_id,
                        &write.bound_principal_id,
                        &write.bound_role,
                        &write.bound_claims_hash,
                        &write.bound_policy_version,
                        &write.bound_knowledge_version,
                        &write.bound_config_version,
                        &write.state_json,
                        &write.journal_len,
                        &head.revision_text,
                        &head.generation_text,
                    ],
                )
                .await
                .map_err(|err| query_error(&err))?;
            if updated == 1 {
                Ok(())
            } else {
                Err(StoreError::Consistency)
            }
        }
        Outcome::Idempotent | Outcome::Refused => accept_unchanged(&head.state, output),
    }
}

fn accept_applied(
    old: &SessionState,
    output: &TransitionOutput,
    project_id: &str,
    session_id: &str,
    event_id: &str,
) -> Result<(), StoreError> {
    if output.contract != REDUCER_CONTRACT
        || output.outcome != Outcome::Applied
        || output.reason.is_some()
        || output.database_used
        || output.auth_provider_used
        || output.runtime_used
        || output.physical_release_certified
    {
        return Err(StoreError::ReducerDisagreement);
    }
    let Some(next_revision) = old.revision.checked_add(1) else {
        return Err(StoreError::ReducerDisagreement);
    };
    if output.revision != next_revision || output.state.revision != next_revision {
        return Err(StoreError::ReducerDisagreement);
    }
    let generation_ok = output.owner_generation == old.owner_generation
        || old.owner_generation.checked_add(1) == Some(output.owner_generation);
    if !generation_ok || output.state.owner_generation != output.owner_generation {
        return Err(StoreError::ReducerDisagreement);
    }
    if output.state.session_id != session_id || output.phase != output.state.phase {
        return Err(StoreError::ReducerDisagreement);
    }
    if output.state.journal.len() != old.journal.len().saturating_add(1) {
        return Err(StoreError::ReducerDisagreement);
    }
    let Some(last) = output.state.journal.last() else {
        return Err(StoreError::ReducerDisagreement);
    };
    if last.event_id != event_id || output.state.journal[..old.journal.len()] != old.journal[..] {
        return Err(StoreError::ReducerDisagreement);
    }
    if let Some(bound) = &output.state.bound
        && bound.project_id != project_id
    {
        return Err(StoreError::ReducerDisagreement);
    }
    Ok(())
}

fn accept_unchanged(old: &SessionState, output: &TransitionOutput) -> Result<(), StoreError> {
    let flags_clear = !output.database_used
        && !output.auth_provider_used
        && !output.runtime_used
        && !output.physical_release_certified;
    let same = output.contract == REDUCER_CONTRACT
        && output.state == *old
        && output.revision == old.revision
        && output.owner_generation == old.owner_generation
        && output.phase == old.phase
        && flags_clear;
    if !same {
        return Err(StoreError::ReducerDisagreement);
    }
    match output.outcome {
        Outcome::Idempotent if output.reason.is_none() => Ok(()),
        Outcome::Refused if output.reason.is_some() => Ok(()),
        Outcome::Applied | Outcome::Idempotent | Outcome::Refused => Err(StoreError::ReducerDisagreement),
    }
}

fn write_of(state: &SessionState) -> Result<HeadWrite, StoreError> {
    let state_json = serde_json::to_string(state).map_err(|_| StoreError::Input)?;
    if state_json.len() > STATE_JSON_MAX {
        return Err(StoreError::StateTooLarge);
    }
    let journal_len = i32::try_from(state.journal.len()).map_err(|_| StoreError::StateTooLarge)?;
    if journal_len > 1024 {
        return Err(StoreError::StateTooLarge);
    }
    let (
        bound_project_id,
        bound_principal_id,
        bound_role,
        bound_claims_hash,
        bound_policy_version,
        bound_knowledge_version,
        bound_config_version,
    ) = match &state.bound {
        None => (None, None, None, None, None, None, None),
        Some(bound) => (
            Some(bound.project_id.clone()),
            Some(bound.principal_id.clone()),
            Some(bound.role.clone()),
            Some(bound.claims_hash.clone()),
            Some(bound.policy_version.clone()),
            Some(bound.knowledge_version.clone()),
            Some(bound.config_version.clone()),
        ),
    };
    Ok(HeadWrite {
        revision: state.revision.to_string(),
        generation: state.owner_generation.to_string(),
        bound_project_id,
        bound_principal_id,
        bound_role,
        bound_claims_hash,
        bound_policy_version,
        bound_knowledge_version,
        bound_config_version,
        state_json,
        journal_len,
    })
}

async fn insert_head(
    tx: &Transaction<'_>,
    project_id: &str,
    session_id: &str,
    write: &HeadWrite,
) -> Result<(), StoreError> {
    tx.execute(
        sql::INSERT_HEAD,
        &[
            &project_id,
            &session_id,
            &write.revision,
            &write.generation,
            &write.bound_project_id,
            &write.bound_principal_id,
            &write.bound_role,
            &write.bound_claims_hash,
            &write.bound_policy_version,
            &write.bound_knowledge_version,
            &write.bound_config_version,
            &write.state_json,
            &write.journal_len,
        ],
    )
    .await
    .map_err(|err| query_error(&err))?;
    Ok(())
}

async fn load(
    tx: &Transaction<'_>,
    project_id: &str,
    session_id: &str,
    lock: bool,
) -> Result<Head, StoreError> {
    let statement = if lock { sql::LOCK_HEAD } else { sql::READ_HEAD };
    let row = tx
        .query_opt(statement, &[&project_id, &session_id])
        .await
        .map_err(|err| query_error(&err))?;
    let Some(row) = row else {
        return Err(StoreError::NotFound);
    };
    assemble(project_id, session_id, raw_head(&row)?)
}

fn raw_head(row: &Row) -> Result<RawHead, StoreError> {
    Ok(RawHead {
        revision: row.try_get(0).map_err(|_| StoreError::Consistency)?,
        owner_generation: row.try_get(1).map_err(|_| StoreError::Consistency)?,
        bound_project_id: row.try_get(2).map_err(|_| StoreError::Consistency)?,
        bound_principal_id: row.try_get(3).map_err(|_| StoreError::Consistency)?,
        bound_role: row.try_get(4).map_err(|_| StoreError::Consistency)?,
        bound_claims_hash: row.try_get(5).map_err(|_| StoreError::Consistency)?,
        bound_policy_version: row.try_get(6).map_err(|_| StoreError::Consistency)?,
        bound_knowledge_version: row.try_get(7).map_err(|_| StoreError::Consistency)?,
        bound_config_version: row.try_get(8).map_err(|_| StoreError::Consistency)?,
        state_json: row.try_get(9).map_err(|_| StoreError::Consistency)?,
        journal_len: row.try_get(10).map_err(|_| StoreError::Consistency)?,
    })
}

fn assemble(project_id: &str, session_id: &str, raw: RawHead) -> Result<Head, StoreError> {
    if raw.state_json.len() > STATE_JSON_MAX || raw.journal_len < 0 {
        return Err(StoreError::Consistency);
    }
    let state: SessionState = serde_json::from_str(&raw.state_json).map_err(|_| StoreError::Consistency)?;
    let revision = parse_u64_canonical(&raw.revision)?;
    let owner_generation = parse_u64_canonical(&raw.owner_generation)?;
    let bound = bound_from_columns(
        raw.bound_project_id,
        raw.bound_principal_id,
        raw.bound_role,
        raw.bound_claims_hash,
        raw.bound_policy_version,
        raw.bound_knowledge_version,
        raw.bound_config_version,
    )?;
    let journal_len = usize::try_from(raw.journal_len).map_err(|_| StoreError::Consistency)?;
    if state.revision != revision
        || state.owner_generation != owner_generation
        || state.journal.len() != journal_len
        || state.session_id != session_id
        || state.bound != bound
    {
        return Err(StoreError::Consistency);
    }
    if let Some(bound) = &state.bound
        && bound.project_id != project_id
    {
        return Err(StoreError::Consistency);
    }
    Ok(Head {
        revision,
        owner_generation,
        revision_text: raw.revision,
        generation_text: raw.owner_generation,
        state,
    })
}

fn parse_u64_canonical(text: &str) -> Result<u64, StoreError> {
    if text.is_empty() || text.len() > 20 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(StoreError::Consistency);
    }
    if text.len() > 1 && text.as_bytes()[0] == b'0' {
        return Err(StoreError::Consistency);
    }
    text.parse::<u64>().map_err(|_| StoreError::Consistency)
}

fn bound_from_columns(
    project: Option<String>,
    principal: Option<String>,
    role: Option<String>,
    claims: Option<String>,
    policy: Option<String>,
    knowledge: Option<String>,
    config: Option<String>,
) -> Result<Option<IdentityTuple>, StoreError> {
    match (project, principal, role, claims, policy, knowledge, config) {
        (None, None, None, None, None, None, None) => Ok(None),
        (
            Some(project_id),
            Some(principal_id),
            Some(role),
            Some(claims_hash),
            Some(policy_version),
            Some(knowledge_version),
            Some(config_version),
        ) => Ok(Some(IdentityTuple {
            project_id,
            principal_id,
            role,
            claims_hash,
            policy_version,
            knowledge_version,
            config_version,
        })),
        _ => Err(StoreError::Consistency),
    }
}

async fn verify_shape(tx: &Transaction<'_>) -> Result<(), StoreError> {
    expect_names(
        tx,
        sql::RELATIONS,
        &[&sql::SCHEMA],
        &sql::EXPECTED_RELATIONS,
    )
    .await?;
    expect_pairs(
        tx,
        sql::COLUMNS,
        &[&sql::SCHEMA, &sql::HEAD],
        &sql::EXPECTED_HEAD_COLUMNS,
    )
    .await?;
    expect_pairs(
        tx,
        sql::COLUMNS,
        &[&sql::SCHEMA, &sql::MANIFEST],
        &sql::EXPECTED_MANIFEST_COLUMNS,
    )
    .await?;
    expect_names(
        tx,
        sql::CONSTRAINTS,
        &[&sql::SCHEMA, &sql::HEAD],
        &sql::EXPECTED_HEAD_CONSTRAINTS,
    )
    .await?;
    expect_names(
        tx,
        sql::CONSTRAINTS,
        &[&sql::SCHEMA, &sql::MANIFEST],
        &sql::EXPECTED_MANIFEST_CONSTRAINTS,
    )
    .await?;
    let row = tx
        .query_one(
            sql::PRIVILEGES,
            &[&sql::RUNTIME_ROLE, &sql::SCHEMA, &sql::HEAD_REGCLASS, &sql::MANIFEST_REGCLASS],
        )
        .await
        .map_err(|_| drift("privileges"))?;
    let mut actual = [false; 9];
    for (index, slot) in actual.iter_mut().enumerate() {
        *slot = row.try_get(index).map_err(|_| drift("privileges"))?;
    }
    if actual == sql::EXPECTED_PRIVILEGES {
        Ok(())
    } else {
        Err(drift("privileges"))
    }
}

async fn expect_names(
    tx: &Transaction<'_>,
    statement: &str,
    params: &[&(dyn ToSql + Sync)],
    expected: &[&str],
) -> Result<(), StoreError> {
    let rows = tx.query(statement, params).await.map_err(|_| drift("names"))?;
    let mut actual = Vec::with_capacity(rows.len());
    for row in &rows {
        actual.push(row.try_get::<_, String>(0).map_err(|_| drift("names"))?);
    }
    let wanted: Vec<String> = expected.iter().map(|item| (*item).to_owned()).collect();
    if actual == wanted {
        Ok(())
    } else {
        Err(drift("names"))
    }
}

async fn expect_pairs(
    tx: &Transaction<'_>,
    statement: &str,
    params: &[&(dyn ToSql + Sync)],
    expected: &[(&str, &str)],
) -> Result<(), StoreError> {
    let rows = tx.query(statement, params).await.map_err(|_| drift("columns"))?;
    let mut actual = Vec::with_capacity(rows.len());
    for row in &rows {
        let name: String = row.try_get(0).map_err(|_| drift("columns"))?;
        let kind: String = row.try_get(1).map_err(|_| drift("columns"))?;
        actual.push((name, kind));
    }
    let wanted: Vec<(String, String)> = expected
        .iter()
        .map(|(name, kind)| ((*name).to_owned(), (*kind).to_owned()))
        .collect();
    if actual == wanted {
        Ok(())
    } else {
        Err(drift("columns"))
    }
}

fn install_receipt(disposition: InstallDisposition, sql_sha256: String) -> InstallReceipt {
    InstallReceipt {
        contract: CONTRACT,
        disposition,
        sql_sha256,
        database_used: true,
        auth_provider_used: false,
        runtime_used: false,
        physical_release_certified: false,
    }
}

fn bootstrap_receipt(
    disposition: BootstrapDisposition,
    reason: Option<Reason>,
    database_used: bool,
) -> BootstrapReceipt {
    BootstrapReceipt {
        contract: CONTRACT,
        disposition,
        reason,
        database_used,
        auth_provider_used: false,
        runtime_used: false,
        physical_release_certified: false,
    }
}

fn acknowledged(output: TransitionOutput) -> TransitionReceipt {
    let outcome = Some(output.outcome);
    let reason = output.reason;
    let revision = Some(output.revision);
    let owner_generation = Some(output.owner_generation);
    let phase = Some(output.phase);
    TransitionReceipt {
        contract: CONTRACT,
        disposition: Disposition::Acknowledged,
        outcome,
        reason,
        revision,
        owner_generation,
        phase,
        database_used: true,
        auth_provider_used: false,
        runtime_used: false,
        physical_release_certified: false,
        sqlstate: None,
        reducer: Some(Box::new(output)),
    }
}

fn rolled_back(sqlstate: Option<String>) -> TransitionReceipt {
    TransitionReceipt {
        contract: CONTRACT,
        disposition: Disposition::RolledBack,
        outcome: None,
        reason: None,
        revision: None,
        owner_generation: None,
        phase: None,
        database_used: true,
        auth_provider_used: false,
        runtime_used: false,
        physical_release_certified: false,
        sqlstate,
        reducer: None,
    }
}

fn reconcile_receipt(
    disposition: ReconcileDisposition,
    revision: Option<u64>,
    owner_generation: Option<u64>,
) -> ReconcileReceipt {
    ReconcileReceipt {
        contract: CONTRACT,
        disposition,
        database_used: true,
        auth_provider_used: false,
        runtime_used: false,
        physical_release_certified: false,
        revision,
        owner_generation,
    }
}

#[cfg(test)]
mod tests {
    use session_transition::{
        Event, IdentityTuple, Outcome, Phase, Reason, ReleaseRecord, SessionState, TransitionInput,
        CONTRACT as REDUCER_CONTRACT,
    };

    use super::{
        accept_applied, accept_unchanged, assemble, parse_u64_canonical, probe_state, write_of, Probe, RawHead,
    };
    use crate::sql::ADMIN_ROLE;

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn alice(project_id: &str) -> IdentityTuple {
        IdentityTuple {
            project_id: project_id.to_owned(),
            principal_id: "alice".to_owned(),
            role: "analyst".to_owned(),
            claims_hash: ABC.to_owned(),
            policy_version: "pol-1".to_owned(),
            knowledge_version: "know-1".to_owned(),
            config_version: "cfg-1".to_owned(),
        }
    }

    fn requested(session_id: &str) -> SessionState {
        SessionState {
            session_id: session_id.to_owned(),
            conversation_id: "conv-1".to_owned(),
            phase: Phase::Requested,
            revision: 0,
            owner_id: String::new(),
            owner_generation: 0,
            lease_expires_at_ms: 0,
            bound: None,
            workspace_id: None,
            session_started_at_ms: 0,
            last_activity_at_ms: 0,
            last_now_ms: 0,
            turn: None,
            tool_starts: 0,
            started_tool_ids: Vec::new(),
            open_tool_ids: Vec::new(),
            last_turn_outcome: None,
            failure: None,
            release: ReleaseRecord::default(),
            journal: Vec::new(),
        }
    }

    fn raw_for(state: &SessionState) -> RawHead {
        let write = write_of(state).expect("write");
        RawHead {
            revision: write.revision,
            owner_generation: write.generation,
            bound_project_id: write.bound_project_id,
            bound_principal_id: write.bound_principal_id,
            bound_role: write.bound_role,
            bound_claims_hash: write.bound_claims_hash,
            bound_policy_version: write.bound_policy_version,
            bound_knowledge_version: write.bound_knowledge_version,
            bound_config_version: write.bound_config_version,
            state_json: write.state_json,
            journal_len: write.journal_len,
        }
    }

    #[test]
    fn canonical_decimal_rejects_padding_and_signs() {
        assert_eq!(parse_u64_canonical("0").expect("zero"), 0);
        assert_eq!(parse_u64_canonical("10").expect("ten"), 10);
        assert!(parse_u64_canonical("01").is_err());
        assert!(parse_u64_canonical("-1").is_err());
        assert!(parse_u64_canonical("10.0").is_err());
    }

    #[test]
    fn assembled_row_must_match_its_json() {
        let state = requested("sess-1");
        let raw = raw_for(&state);
        let head = assemble("project-1", "sess-1", raw).expect("consistent");
        assert_eq!(head.state, state);
        let mut drifted = raw_for(&state);
        drifted.revision = "1".to_owned();
        assert!(assemble("project-1", "sess-1", drifted).is_err());
    }

    #[test]
    fn probe_accepts_requested_and_refuses_a_blank_session() {
        assert!(matches!(probe_state(&requested("sess-1")), Probe::Valid));
        let mut blank = requested("sess-1");
        blank.session_id.clear();
        assert!(matches!(probe_state(&blank), Probe::StateInvalid));
    }

    #[test]
    fn reducer_begin_bind_is_the_only_accepted_write() {
        let state = requested("sess-1");
        let identity = alice("project-1");
        let event = Event::BeginBind {
            owner_id: "entry-1".to_owned(),
            lease_expires_at_ms: 31_000,
        };
        let input = TransitionInput {
            contract: REDUCER_CONTRACT.to_owned(),
            state: state.clone(),
            identity,
            expected_revision: 0,
            expected_owner_generation: 0,
            event_id: "ev-1".to_owned(),
            now_ms: 1_000,
            event,
        };
        let output = session_transition::transition(&input);
        assert_eq!(output.outcome, Outcome::Applied);
        assert!(!output.database_used);
        assert!(accept_applied(&state, &output, "project-1", "sess-1", "ev-1").is_ok());
        let mut stale_input = input.clone();
        stale_input.expected_revision = 9;
        stale_input.event_id = "ev-2".to_owned();
        let stale = session_transition::transition(&stale_input);
        assert_eq!(stale.reason, Some(Reason::Stale));
        assert!(accept_unchanged(&state, &stale).is_ok());
        let mut mutated = output.clone();
        mutated.database_used = true;
        assert!(accept_applied(&state, &mutated, "project-1", "sess-1", "ev-1").is_err());
        assert!(output.state.failure.is_none());
    }

    #[test]
    fn product_source_has_no_shell_cancel_or_clock() {
        let source = include_str!("store.rs");
        assert!(!source.contains(&["cancel", "_token"].concat()));
        assert!(!source.contains(&["pg_cancel", "_backend"].concat()));
        assert!(!source.contains(&["pg_terminate", "_backend"].concat()));
        assert!(!source.contains(&["System", "Time"].concat()));
        assert!(!source.contains(&["Command", "::"].concat()));
        let main = include_str!("main.rs");
        assert!(!main.contains("env::var"));
        assert!(!main.contains(&["DropCommit", "Response"].concat()));
        assert_ne!(ADMIN_ROLE, "alice");
    }

    #[test]
    fn migration_digest_is_64_hex() {
        let digest = super::migration_sha256().expect("hash");
        assert_eq!(digest.len(), 64);
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    }
}
