//! Transactions against `oo_token_control`. Plaintext is never a parameter.

use std::future::Future;
use std::time::Duration;

use tokio_postgres::config::SslMode;
use tokio_postgres::{Client, Error, Row, Transaction};
use zeroize::Zeroize;

use crate::cache::{Cached, LookupCache};
use crate::canon::canonicalize_audience;
use crate::error::{commit_error, drift, query_error, StoreError};
use crate::model::{
    check_for_kind, check_grant, check_revoke, classify, deadlines, is_login_name, slide_idle, wall_status,
    CheckedGrant, CurrentTuple, DeclaredTransport, Endpoint, FrozenView, IssueGrant, SystemWall, TokenKind,
    WaitGate, WallClock, WallSource,
};
use crate::sha256::{hex, sha256};
use crate::sql;
use crate::tls::rustls_connector;
use crate::token::{draw_32, token_hash, PlainToken};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ControlFlags {
    pub auth_provider_used: bool,
    pub idp_used: bool,
    pub mtls_authenticated: bool,
    pub pubsub_used: bool,
}

pub fn control_flags() -> ControlFlags {
    ControlFlags {
        auth_provider_used: false,
        idp_used: false,
        mtls_authenticated: false,
        pubsub_used: false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallDisposition {
    Installed,
    Replay,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallReceipt {
    pub disposition: InstallDisposition,
    pub sql_sha256: String,
    pub flags: ControlFlags,
}

#[derive(Debug)]
pub struct IssueReceipt {
    pub token: PlainToken,
    pub hash: String,
    pub kind: TokenKind,
    pub wall: WallSource,
    pub issued_at_ms: i64,
    pub idle_deadline_ms: Option<i64>,
    pub absolute_deadline_ms: i64,
    pub revocation_floor: i64,
    pub flags: ControlFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevokeDisposition {
    Applied,
    Replay,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevokeReceipt {
    pub disposition: RevokeDisposition,
    pub seq: i64,
    pub wall: WallSource,
    pub flags: ControlFlags,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LookupReceipt {
    pub hash: String,
    pub kind: TokenKind,
    pub wall: WallSource,
    pub database_used: bool,
    pub flags: ControlFlags,
}

pub struct LookupRequest<'a> {
    pub endpoint: &'a Endpoint,
    pub user: &'a str,
    pub password: &'a str,
    pub ca_pem: &'a [u8],
    pub cache: &'a LookupCache,
    pub presented_token: &'a str,
    pub current: &'a CurrentTuple,
    pub gateway_audience: &'a str,
    pub transport: &'a DeclaredTransport,
}

pub fn migration_sha256() -> Result<String, StoreError> {
    let digest = sha256(sql::MIGRATION_SQL.as_bytes()).ok_or(StoreError::Input)?;
    Ok(hex(&digest))
}

pub(crate) async fn install(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
) -> Result<InstallReceipt, StoreError> {
    require_login(user, sql::ADMIN_ROLE)?;
    let hash = migration_sha256()?;
    let mut session = open(endpoint, user, password, ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let result = install_body(&tx, &hash).await;
    let disposition = finish(tx, result).await?;
    Ok(InstallReceipt {
        disposition,
        sql_sha256: hash,
        flags: control_flags(),
    })
}

pub(crate) async fn issue_with(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    grant: &IssueGrant,
    clock: &dyn WallClock,
    gate: &WaitGate,
) -> Result<IssueReceipt, StoreError> {
    require_login(user, sql::ADMIN_ROLE)?;
    let checked = check_grant(grant)?;
    let now = clock.now_ms()?;
    let mut session = open(endpoint, user, password, ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let result = issue_body(&tx, &checked, now, clock.source(), gate).await;
    finish(tx, result).await
}

pub(crate) async fn revoke_with(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    scope: &crate::model::RevokeScope,
    clock: &dyn WallClock,
    gate: &WaitGate,
) -> Result<RevokeReceipt, StoreError> {
    require_login(user, sql::ADMIN_ROLE)?;
    let key = check_revoke(scope)?;
    let now = clock.now_ms()?;
    let mut session = open(endpoint, user, password, ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let result = revoke_body(&tx, &key, now, clock.source(), gate).await;
    finish(tx, result).await
}

pub async fn lookup(request: LookupRequest<'_>) -> Result<LookupReceipt, StoreError> {
    lookup_for_test(request, &SystemWall).await
}

pub async fn lookup_for_test(request: LookupRequest<'_>, clock: &dyn WallClock) -> Result<LookupReceipt, StoreError> {
    require_login(request.user, sql::RUNTIME_ROLE)?;
    let (kind, hash) = token_hash(request.presented_token)?;
    let presented = check_for_kind(kind, request.current)?;
    let gateway = canonicalize_audience(request.gateway_audience)?;
    let now = clock.now_ms()?;
    if let Some(cached) = request.cache.positive(&hash)? {
        evaluate(&cached, kind, &presented, &gateway, request.transport, now)?;
        return Ok(lookup_receipt(hash, kind, clock.source(), false));
    }
    match lookup_db(&request, &hash, kind, &presented, &gateway, now).await {
        Ok(cached) => {
            request.cache.insert_positive(cached)?;
            Ok(lookup_receipt(hash, kind, clock.source(), true))
        }
        Err(err) => Err(lookup_fail(err)),
    }
}

fn lookup_receipt(hash: String, kind: TokenKind, wall: WallSource, database_used: bool) -> LookupReceipt {
    LookupReceipt {
        hash,
        kind,
        wall,
        database_used,
        flags: control_flags(),
    }
}

fn evaluate(
    cached: &Cached,
    kind: TokenKind,
    presented: &CheckedGrant,
    gateway: &str,
    transport: &DeclaredTransport,
    now: i64,
) -> Result<(), StoreError> {
    if cached.kind != kind {
        return Err(StoreError::Consistency);
    }
    wall_status(now, cached.issued_at_ms, cached.idle_deadline_ms, cached.absolute_deadline_ms)?;
    classify(&cached.frozen, presented, gateway, transport)
}

async fn lookup_db(
    request: &LookupRequest<'_>,
    hash: &str,
    kind: TokenKind,
    presented: &CheckedGrant,
    gateway: &str,
    now: i64,
) -> Result<Cached, StoreError> {
    let mut session = open(request.endpoint, request.user, request.password, request.ca_pem).await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let result = lookup_body(&tx, hash, kind, presented, gateway, request.transport, now).await;
    finish(tx, result).await
}

async fn lookup_body(
    tx: &Transaction<'_>,
    hash: &str,
    kind: TokenKind,
    presented: &CheckedGrant,
    gateway: &str,
    transport: &DeclaredTransport,
    now: i64,
) -> Result<Cached, StoreError> {
    let user = require_role(tx, sql::RUNTIME_ROLE).await?;
    if presented.principal_id == user || is_login_name(&presented.principal_id) {
        return Err(StoreError::PrincipalNotAuthentication);
    }
    verify_shape(tx).await?;
    let row = tx
        .query_opt(sql::READ_TOKEN, &[&hash])
        .await
        .map_err(|err| query_error(&err))?;
    let Some(row) = row else {
        return Err(StoreError::NotFound);
    };
    let loaded = load_row(&row)?;
    if loaded.kind != kind {
        return Err(StoreError::Consistency);
    }
    let session_key = loaded.frozen.session_id.clone().unwrap_or_default();
    let covering = tx
        .query_opt(
            sql::COVERING,
            &[
                &loaded.frozen.project_id,
                &hash,
                &loaded.floor,
                &loaded.frozen.principal_id,
                &session_key,
            ],
        )
        .await
        .map_err(|err| query_error(&err))?;
    if covering.is_some() {
        return Err(StoreError::Revoked);
    }
    wall_status(now, loaded.issued_at_ms, loaded.idle_deadline_ms, loaded.absolute_deadline_ms)?;
    classify(&loaded.frozen, presented, gateway, transport)?;
    let idle = slide_session(tx, hash, loaded.kind, now, loaded.absolute_deadline_ms, loaded.idle_deadline_ms).await?;
    Ok(Cached {
        hash: hash.to_owned(),
        kind: loaded.kind,
        frozen: loaded.frozen,
        issued_at_ms: loaded.issued_at_ms,
        idle_deadline_ms: idle,
        absolute_deadline_ms: loaded.absolute_deadline_ms,
        floor: loaded.floor,
    })
}

async fn slide_session(
    tx: &Transaction<'_>,
    hash: &str,
    kind: TokenKind,
    now: i64,
    absolute: i64,
    idle: Option<i64>,
) -> Result<Option<i64>, StoreError> {
    if kind != TokenKind::Session {
        return Ok(idle);
    }
    let slid = slide_idle(now, absolute)?;
    let updated = tx
        .execute(sql::SLIDE, &[&now, &slid, &hash])
        .await
        .map_err(|err| query_error(&err))?;
    if updated != 1 {
        return Err(StoreError::Consistency);
    }
    Ok(Some(slid))
}

struct Loaded {
    kind: TokenKind,
    frozen: FrozenView,
    issued_at_ms: i64,
    idle_deadline_ms: Option<i64>,
    absolute_deadline_ms: i64,
    floor: i64,
}

fn load_row(row: &Row) -> Result<Loaded, StoreError> {
    let kind_text: String = row.try_get(0).map_err(|_| StoreError::Consistency)?;
    let kind = TokenKind::parse(&kind_text)?;
    let sandbox: Option<String> = row.try_get(8).map_err(|_| StoreError::Consistency)?;
    let session_id: Option<String> = row.try_get(9).map_err(|_| StoreError::Consistency)?;
    let idle: Option<i64> = row.try_get(11).map_err(|_| StoreError::Consistency)?;
    match kind {
        TokenKind::Session if sandbox.is_none() || session_id.is_none() || idle.is_none() => {
            return Err(StoreError::Consistency);
        }
        TokenKind::Client if sandbox.is_some() || session_id.is_some() || idle.is_some() => {
            return Err(StoreError::Consistency);
        }
        TokenKind::Session | TokenKind::Client => {}
    }
    Ok(Loaded {
        kind,
        frozen: FrozenView {
            project_id: row.try_get(1).map_err(|_| StoreError::Consistency)?,
            principal_id: row.try_get(2).map_err(|_| StoreError::Consistency)?,
            role: row.try_get(3).map_err(|_| StoreError::Consistency)?,
            claims_canonical: row.try_get(4).map_err(|_| StoreError::Consistency)?,
            policy_version: row.try_get(5).map_err(|_| StoreError::Consistency)?,
            knowledge_version: row.try_get(6).map_err(|_| StoreError::Consistency)?,
            audience_url: row.try_get(7).map_err(|_| StoreError::Consistency)?,
            sandbox_id: sandbox,
            session_id,
        },
        issued_at_ms: row.try_get(10).map_err(|_| StoreError::Consistency)?,
        idle_deadline_ms: idle,
        absolute_deadline_ms: row.try_get(12).map_err(|_| StoreError::Consistency)?,
        floor: row.try_get(13).map_err(|_| StoreError::Consistency)?,
    })
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
        if let Err(err) = tx.batch_execute(sql::MIGRATION_SQL).await {
            return Err(pg_shape(&err));
        }
        let inserted = tx
            .execute(sql::INSERT_MANIFEST, &[&hash])
            .await
            .map_err(|err| query_error(&err))?;
        if inserted != 1 {
            return Err(drift("manifest"));
        }
        verify_shape(tx).await?;
        return Ok(InstallDisposition::Installed);
    }
    let row = match tx.query_opt(sql::READ_MANIFEST, &[]).await {
        Ok(row) => row,
        Err(err) if missing_relation(&err) => return Err(drift("manifest")),
        Err(err) => return Err(query_error(&err)),
    };
    let Some(row) = row else {
        return Err(drift("manifest"));
    };
    let stored: String = row.try_get(0).map_err(|_| drift("manifest"))?;
    if stored != hash {
        return Err(drift("hash"));
    }
    verify_shape(tx).await?;
    Ok(InstallDisposition::Replay)
}

async fn issue_body(
    tx: &Transaction<'_>,
    checked: &CheckedGrant,
    now: i64,
    wall: WallSource,
    gate: &WaitGate,
) -> Result<IssueReceipt, StoreError> {
    let user = require_role(tx, sql::ADMIN_ROLE).await?;
    if checked.principal_id == user || is_login_name(&checked.principal_id) {
        return Err(StoreError::PrincipalNotAuthentication);
    }
    let lock = project_key(&checked.project_id)?;
    tx.execute(sql::LOCK_PROJECT, &[&sql::PROJECT_CLASS, &lock])
        .await
        .map_err(|err| query_error(&err))?;
    verify_shape(tx).await?;
    if let Some(session_id) = &checked.session_id {
        let closed = tx
            .query_opt(sql::SESSION_CLOSED, &[&checked.project_id, session_id])
            .await
            .map_err(|err| query_error(&err))?;
        if closed.is_some() {
            return Err(StoreError::ScopeClosed);
        }
    }
    let session_key = checked.session_id.clone().unwrap_or_default();
    let floor_row = tx
        .query_one(sql::FLOOR, &[&checked.project_id, &checked.principal_id, &session_key])
        .await
        .map_err(|err| query_error(&err))?;
    let floor: i64 = floor_row.try_get(0).map_err(|_| StoreError::Consistency)?;
    let mut raw = draw_32()?;
    let token = PlainToken::issue(checked.kind, &raw);
    raw.zeroize();
    let hash = token.sha256_hex()?;
    let (idle, absolute) = deadlines(checked.kind, now, checked.client_ttl_ms)?;
    let kind_text = checked.kind.as_str();
    let sandbox = checked.sandbox_id.clone();
    let session_id = checked.session_id.clone();
    let inserted = tx
        .execute(
            sql::INSERT_TOKEN,
            &[
                &hash,
                &kind_text,
                &checked.project_id,
                &checked.principal_id,
                &checked.role,
                &checked.claims_canonical,
                &checked.policy_version,
                &checked.knowledge_version,
                &checked.audience_url,
                &sandbox,
                &session_id,
                &now,
                &idle,
                &absolute,
                &now,
                &floor,
            ],
        )
        .await
        .map_err(|err| query_error(&err))?;
    if inserted != 1 {
        return Err(StoreError::Consistency);
    }
    wait_gate(gate).await;
    Ok(IssueReceipt {
        token,
        hash,
        kind: checked.kind,
        wall,
        issued_at_ms: now,
        idle_deadline_ms: idle,
        absolute_deadline_ms: absolute,
        revocation_floor: floor,
        flags: control_flags(),
    })
}

async fn revoke_body(
    tx: &Transaction<'_>,
    key: &crate::model::RevokeKey,
    now: i64,
    wall: WallSource,
    gate: &WaitGate,
) -> Result<RevokeReceipt, StoreError> {
    require_role(tx, sql::ADMIN_ROLE).await?;
    let lock = project_key(&key.project_id)?;
    tx.execute(sql::LOCK_PROJECT, &[&sql::PROJECT_CLASS, &lock])
        .await
        .map_err(|err| query_error(&err))?;
    verify_shape(tx).await?;
    let existing = tx
        .query_opt(
            sql::READ_REVOCATION,
            &[&key.scope, &key.project_id, &key.principal_id, &key.session_id, &key.token_sha256],
        )
        .await
        .map_err(|err| query_error(&err))?;
    if let Some(row) = existing {
        let seq: i64 = row.try_get(0).map_err(|_| StoreError::Consistency)?;
        wait_gate(gate).await;
        return Ok(receipt_revoke(RevokeDisposition::Replay, seq, wall));
    }
    let row = tx
        .query_one(
            sql::INSERT_REVOCATION,
            &[&key.scope, &key.project_id, &key.principal_id, &key.session_id, &key.token_sha256, &now],
        )
        .await
        .map_err(|err| query_error(&err))?;
    let seq: i64 = row.try_get(0).map_err(|_| StoreError::Consistency)?;
    wait_gate(gate).await;
    Ok(receipt_revoke(RevokeDisposition::Applied, seq, wall))
}

fn receipt_revoke(disposition: RevokeDisposition, seq: i64, wall: WallSource) -> RevokeReceipt {
    RevokeReceipt {
        disposition,
        seq,
        wall,
        flags: control_flags(),
    }
}

async fn verify_shape(tx: &Transaction<'_>) -> Result<(), StoreError> {
    expect_names(tx, sql::RELATIONS, &sql::EXPECTED_RELATIONS, "relations").await?;
    expect_columns(tx, sql::MANIFEST, &sql::EXPECTED_MANIFEST_COLUMNS, "columns_install_manifest").await?;
    expect_columns(tx, sql::TOKEN, &sql::EXPECTED_TOKEN_COLUMNS, "columns_token_record").await?;
    expect_columns(tx, sql::REVOCATION, &sql::EXPECTED_REVOCATION_COLUMNS, "columns_revocation").await?;
    expect_constraint_names(tx, sql::MANIFEST, &sql::EXPECTED_MANIFEST_CONSTRAINTS, "constraints_install_manifest").await?;
    expect_constraint_names(tx, sql::TOKEN, &sql::EXPECTED_TOKEN_CONSTRAINTS, "constraints_token_record").await?;
    expect_constraint_names(tx, sql::REVOCATION, &sql::EXPECTED_REVOCATION_CONSTRAINTS, "constraints_revocation").await?;
    expect_names(tx, sql::SEQUENCES, &sql::EXPECTED_SEQUENCES, "sequences").await?;
    verify_privileges(tx).await
}

async fn expect_names(
    tx: &Transaction<'_>,
    statement: &str,
    expected: &[&str],
    check: &'static str,
) -> Result<(), StoreError> {
    let rows = tx
        .query(statement, &[&sql::SCHEMA])
        .await
        .map_err(|err| pg_shape(&err))?;
    if rows.len() != expected.len() {
        return Err(drift(check));
    }
    for (row, name) in rows.iter().zip(expected.iter()) {
        let actual: String = row.try_get(0).map_err(|_| StoreError::Consistency)?;
        if actual != *name {
            return Err(drift(check));
        }
    }
    Ok(())
}

async fn expect_constraint_names(
    tx: &Transaction<'_>,
    relation: &str,
    expected: &[&str],
    check: &'static str,
) -> Result<(), StoreError> {
    let rows = tx
        .query(sql::CONSTRAINTS, &[&sql::SCHEMA, &relation])
        .await
        .map_err(|err| pg_shape(&err))?;
    if rows.len() != expected.len() {
        return Err(drift(check));
    }
    for (row, name) in rows.iter().zip(expected.iter()) {
        let actual: String = row.try_get(0).map_err(|_| StoreError::Consistency)?;
        if actual != *name {
            return Err(drift(check));
        }
    }
    Ok(())
}

async fn expect_columns(
    tx: &Transaction<'_>,
    relation: &str,
    expected: &[(&str, &str)],
    check: &'static str,
) -> Result<(), StoreError> {
    let rows = tx
        .query(sql::COLUMNS, &[&sql::SCHEMA, &relation])
        .await
        .map_err(|err| pg_shape(&err))?;
    if rows.len() != expected.len() {
        return Err(drift(check));
    }
    for (index, row) in rows.iter().enumerate() {
        let name: String = row.try_get(0).map_err(|_| StoreError::Consistency)?;
        let kind: String = row.try_get(1).map_err(|_| StoreError::Consistency)?;
        if name != expected[index].0 || kind != expected[index].1 {
            return Err(drift(check));
        }
    }
    Ok(())
}

async fn verify_privileges(tx: &Transaction<'_>) -> Result<(), StoreError> {
    let row = tx.query_one(sql::PRIVILEGES, &[]).await.map_err(|err| pg_shape(&err))?;
    for (index, (name, expected)) in sql::EXPECTED_PRIVILEGES.iter().enumerate() {
        let actual: bool = row.try_get(index).map_err(|_| drift("privilege_read"))?;
        if actual != *expected {
            return Err(drift(name));
        }
    }
    Ok(())
}

fn project_key(project_id: &str) -> Result<i32, StoreError> {
    let digest = sha256(project_id.as_bytes()).ok_or(StoreError::Input)?;
    Ok(i32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]))
}

fn lookup_fail(err: StoreError) -> StoreError {
    match err {
        StoreError::Connect | StoreError::Query { .. } | StoreError::CommitUnknown => StoreError::Unavailable,
        other => other,
    }
}

enum CommitEnd {
    Ack,
    Server { sqlstate: String },
    Unknown,
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

fn require_login(user: &str, expected: &str) -> Result<(), StoreError> {
    if user == expected {
        Ok(())
    } else {
        Err(StoreError::LoginRole)
    }
}

fn pg_config(endpoint: &Endpoint, user: &str, password: &str) -> Result<tokio_postgres::Config, StoreError> {
    if endpoint.host.is_empty() || endpoint.database.is_empty() || endpoint.port == 0 || user.is_empty() || password.is_empty()
    {
        return Err(StoreError::Input);
    }
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(password);
    config.host(&endpoint.host);
    config.port(endpoint.port);
    config.dbname(&endpoint.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("gateway-token-store");
    config.connect_timeout(Duration::from_secs(10));
    Ok(config)
}

fn detach(connection: impl Future<Output = Result<(), Error>> + Send + 'static) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let _ = connection.await;
    })
}

async fn open(endpoint: &Endpoint, user: &str, password: &str, ca_pem: &[u8]) -> Result<Session, StoreError> {
    let config = pg_config(endpoint, user, password)?;
    let tls = rustls_connector(ca_pem)?;
    let (client, connection) = config.connect(tls).await.map_err(|_| StoreError::Connect)?;
    Ok(Session {
        client,
        connection: detach(connection),
    })
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

async fn require_role(tx: &Transaction<'_>, expected: &str) -> Result<String, StoreError> {
    let user = current_user(tx).await?;
    if user == expected {
        Ok(user)
    } else {
        Err(StoreError::LoginRole)
    }
}

async fn current_user(tx: &Transaction<'_>) -> Result<String, StoreError> {
    let row = tx.query_one(sql::CURRENT_USER, &[]).await.map_err(|err| query_error(&err))?;
    row.try_get(0).map_err(|_| StoreError::LoginRole)
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

async fn finish<T>(tx: Transaction<'_>, result: Result<T, StoreError>) -> Result<T, StoreError> {
    match result {
        Ok(value) => match end_commit(tx).await {
            CommitEnd::Ack => Ok(value),
            CommitEnd::Server { sqlstate } => Err(StoreError::Query { sqlstate: Some(sqlstate) }),
            CommitEnd::Unknown => Err(StoreError::CommitUnknown),
        },
        Err(err) => Err(rollback_err(tx, err).await),
    }
}

fn pg_shape(err: &Error) -> StoreError {
    match err.code().map(|code| code.code()) {
        Some("42704") => drift("runtime_role"),
        _ => query_error(err),
    }
}

fn missing_relation(err: &Error) -> bool {
    matches!(err.code().map(|code| code.code()), Some("42P01" | "3F000"))
}

async fn wait_gate(gate: &WaitGate) {
    if let Some(paused) = &gate.paused {
        paused.wait().await;
    }
    if let Some(release) = &gate.release {
        release.wait().await;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn migration_digest_is_64_hex() {
        let hash = super::migration_sha256().expect("hash");
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
        let flags = super::control_flags();
        assert!(!flags.auth_provider_used, "auth provider flag was set");
        assert!(!flags.idp_used, "idp flag was set");
        assert!(!flags.mtls_authenticated, "mtls flag was set");
        assert!(!flags.pubsub_used, "pubsub flag was set");
    }
}
