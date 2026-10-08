//! PostgreSQL session state. Cookies and tokens are stored only as hashes.

use std::fs::{self, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::Mutex;
use tokio_postgres::config::SslMode;
use tokio_postgres::Transaction;
use zeroize::Zeroizing;

use crate::binding::{receipt_sha256, request_sha256, Change, Plan};
use crate::config::{management_document, Binding, Config, Role};
use crate::cookie::session_hash;
use crate::crypto::{b64, hashes_equal, opaque_token, pkce_challenge, sha256_hex};
use crate::error::{Fault, CONTRACT};
use crate::guard::{projects_from, ProjectView, VerifiedSession};
use crate::sql;
use crate::tls::pg_tls;

#[derive(Serialize)]
pub struct InstallReport {
    pub contract: &'static str,
    pub disposition: &'static str,
    pub sql_sha256: String,
    pub data_grants: &'static str,
    pub owner_certification: &'static str,
    pub agent_session: &'static str,
    pub harness_session: &'static str,
    pub gateway_token: &'static str,
    pub customer_integration: &'static str,
}

#[derive(Serialize)]
pub struct IssueReport {
    pub contract: &'static str,
    pub disposition: &'static str,
    pub token_hash: String,
    pub fixture_class: &'static str,
    pub customer_owner: bool,
    pub data_grants: &'static str,
    pub owner_certification: &'static str,
    pub agent_session: &'static str,
    pub harness_session: &'static str,
    pub gateway_token: &'static str,
    pub customer_integration: &'static str,
}

pub struct BurnedLogin {
    pub nonce_hash: String,
    pub return_to: String,
    pub bootstrap_hash: Option<String>,
    pub redirect_uri: String,
}

pub struct CurrentRecord {
    pub subject: String,
    pub issuer: String,
    pub config_commit: String,
    pub revision: i64,
    pub initialized: bool,
    pub csrf: String,
    pub idle_expires_at: String,
    pub absolute_expires_at: String,
    pub projects: Vec<ProjectView>,
}

pub struct PreviewReport {
    pub outcome: &'static str,
    pub effect: String,
    pub project_id: String,
    pub subject: String,
    pub role: Option<Role>,
    pub revision: i64,
    pub config_commit: String,
    pub document: String,
    pub document_sha256: String,
}

pub struct ApproveReport {
    pub disposition: &'static str,
    pub outcome: &'static str,
    pub effect: String,
    pub project_id: String,
    pub subject: String,
    pub role: Option<Role>,
    pub revision: i64,
    pub config_commit: String,
    pub git_commit: String,
    pub document_sha256: String,
    pub receipt_sha256: String,
}

struct OauthRow {
    state_hash: String,
    nonce_hash: String,
    pkce_challenge: String,
    redirect_uri: String,
    return_to: String,
    bootstrap_hash: Option<String>,
    expired: bool,
    consumed: bool,
}

struct SessionRow {
    subject: String,
    issuer: String,
    config_commit: String,
    revision: i64,
    csrf_hash: String,
    _idle_at: String,
    _absolute_at: String,
    dead: bool,
}

struct BootRow {
    subject: String,
    project_id: String,
    role: String,
    state: String,
    expired: bool,
}

pub struct Store {
    client: Mutex<tokio_postgres::Client>,
}

impl Store {
    pub async fn connect(config: &Config) -> Result<Self, Fault> {
        let mut pg = tokio_postgres::Config::new();
        pg.user(&config.pg_user);
        pg.password(config.pg_password.as_str());
        pg.host(&config.pg_host);
        pg.port(config.pg_port);
        pg.dbname(&config.pg_database);
        pg.ssl_mode(SslMode::Require);
        pg.application_name("oo-control-identity");
        pg.connect_timeout(Duration::from_secs(10));
        pg.keepalives(true);
        let tls = pg_tls(&config.pg_ca_pem)?;
        let (client, connection) = pg.connect(tls).await.map_err(db)?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        client.batch_execute("SET TIME ZONE 'UTC'").await.map_err(db)?;
        Ok(Self { client: Mutex::new(client) })
    }

    pub async fn install(config: &Config) -> Result<InstallReport, Fault> {
        let store = Self::connect(config).await?;
        store.install_once(config).await
    }

    pub async fn issue_bootstrap(config: &Config, out: &Path) -> Result<IssueReport, Fault> {
        if out.exists() {
            return Err(Fault::malformed());
        }
        let store = Self::connect(config).await?;
        store.issue_once(config, out).await
    }

    pub async fn require_installed(&self) -> Result<(), Fault> {
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(db)?;
        set_local(&transaction).await?;
        if schema_count(&transaction).await? == 0 {
            return Err(Fault::unavailable());
        }
        if read_manifest(&transaction).await?.is_none() {
            return Err(Fault::drift());
        }
        if !v2_matches(&transaction).await? {
            return Err(Fault::drift());
        }
        commit(transaction).await
    }

    async fn install_once(&self, config: &Config) -> Result<InstallReport, Fault> {
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(db)?;
        set_local(&transaction).await?;
        transaction.execute(sql::ADVISORY_LOCK, &[&sql::INSTALL_LOCK]).await.map_err(db)?;
        let hash = sql::migration_sha256();
        let disposition = if schema_count(&transaction).await? == 0 {
            transaction.batch_execute(sql::MIGRATION).await.map_err(db)?;
            transaction.execute(sql::INSERT_MANIFEST, &[&hash]).await.map_err(db)?;
            transaction.batch_execute(sql::MIGRATION_V2).await.map_err(db)?;
            let hash_v2 = sql::migration_v2_sha256();
            transaction.execute(sql::INSERT_V2, &[&hash_v2]).await.map_err(db)?;
            seed_bindings(&transaction, config).await?;
            "committed"
        } else {
            match read_manifest(&transaction).await? {
                Some((stored, _)) if hashes_equal(&stored, &hash) && v2_matches(&transaction).await? => "replayed",
                _ => return Err(Fault::drift()),
            }
        };
        commit(transaction).await?;
        Ok(install_report(disposition, hash))
    }

    async fn issue_once(&self, config: &Config, out: &Path) -> Result<IssueReport, Fault> {
        let token = Zeroizing::new(opaque_token()?);
        let token_hash = sha256_hex(token.as_bytes());
        let issued = {
            let mut client = self.client.lock().await;
            let transaction = client.transaction().await.map_err(db)?;
            set_local(&transaction).await?;
            let initialized = lock_initialized(&transaction).await?;
            if initialized {
                Err(Fault::bootstrap_unavailable())
            } else {
                transaction.execute(sql::SUPERSEDE_BOOTSTRAP, &[]).await.map_err(db)?;
                let ttl = seconds(config.bootstrap_ttl_seconds)?;
                transaction
                    .execute(
                        sql::INSERT_BOOTSTRAP,
                        &[&token_hash, &config.bootstrap.subject, &config.bootstrap.project_id, &config.bootstrap.role.as_str(), &ttl],
                    )
                    .await
                    .map_err(db)?;
                audit(&transaction, "bootstrap_issued", "ok", &config.bootstrap.subject, "").await?;
                commit(transaction).await.map(|_| ())
            }
        };
        match issued {
            Ok(()) => {}
            Err(fault) if bootstrap_code(&fault) => {
                self.record("bootstrap_rejected", fault.code, &config.bootstrap.subject, "").await?;
                return Err(fault);
            }
            Err(fault) => return Err(fault),
        }
        write_token_file(out, &token)?;
        Ok(issue_report(token_hash))
    }

    pub async fn begin_login(
        &self,
        config: &Config,
        return_to: &str,
        bootstrap_token: Option<&str>,
        cookie: &crate::cookie::LoginCookie,
        state: &str,
        nonce: &str,
    ) -> Result<(), Fault> {
        let bootstrap_hash = bootstrap_token.map(|token| sha256_hex(token.as_bytes()));
        let handle_hash = sha256_hex(&cookie.handle);
        let state_hash = sha256_hex(state.as_bytes());
        let nonce_hash = sha256_hex(nonce.as_bytes());
        let challenge = pkce_challenge(&b64(&cookie.verifier));
        let ttl = seconds(config.oauth_ttl_seconds)?;
        let started = {
            let mut client = self.client.lock().await;
            let transaction = client.transaction().await.map_err(db)?;
            set_local(&transaction).await?;
            async {
                if let Some(hash) = bootstrap_hash.as_deref() {
                    preflight_bootstrap(&transaction, config, hash).await?;
                }
                transaction
                    .execute(
                        sql::INSERT_OAUTH,
                        &[
                            &handle_hash,
                            &state_hash,
                            &nonce_hash,
                            &challenge,
                            &config.callback_uri,
                            &return_to,
                            &bootstrap_hash,
                            &ttl,
                        ],
                    )
                    .await
                    .map_err(db)?;
                audit(&transaction, "login_started", "ok", "", "").await?;
                commit(transaction).await
            }
            .await
        };
        match started {
            Ok(()) => Ok(()),
            Err(fault) if bootstrap_code(&fault) => {
                self.record("bootstrap_rejected", fault.code, "", "").await?;
                Err(fault)
            }
            Err(fault) => Err(fault),
        }
    }

    pub async fn burn_login(&self, handle_hash: &str, state: Option<&str>, verifier: &str, redirect_uri: &str) -> Result<BurnedLogin, Fault> {
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(db)?;
        set_local(&transaction).await?;
        let Some(row) = lock_oauth(&transaction, handle_hash).await? else {
            return Err(Fault::unauthenticated());
        };
        let decision = oauth_decision(&row, state, verifier, redirect_uri);
        match decision {
            Ok(burned) => {
                consume_oauth(&transaction, handle_hash).await?;
                commit(transaction).await?;
                Ok(burned)
            }
            Err(fault) => {
                if !row.consumed {
                    consume_oauth(&transaction, handle_hash).await?;
                }
                audit(&transaction, "login_rejected", fault.code, "", "").await?;
                commit(transaction).await?;
                Err(fault)
            }
        }
    }

    pub async fn reject_login(&self, fault: &Fault) -> Result<(), Fault> {
        self.record("login_rejected", fault.code, "", "").await
    }

    pub async fn finish_login(&self, config: &Config, subject: &str, bootstrap_hash: Option<&str>) -> Result<String, Fault> {
        let session_cookie = opaque_token()?;
        let cookie_hash = session_hash(&session_cookie)?;
        let csrf_hash = sha256_hex(opaque_token()?.as_bytes());
        let idle = seconds(config.idle_seconds)?;
        let absolute = seconds(config.absolute_seconds)?;
        let finished = {
            let mut client = self.client.lock().await;
            let transaction = client.transaction().await.map_err(db)?;
            set_local(&transaction).await?;
            async {
                if let Some(hash) = bootstrap_hash {
                    consume_for_login(&transaction, config, hash, subject, &cookie_hash).await?;
                }
                let head = read_head(&transaction, sql::READ_HEAD_SHARE).await?;
                transaction
                    .execute(
                        sql::INSERT_SESSION,
                        &[&cookie_hash, &csrf_hash, &subject, &config.issuer, &head.config_commit, &head.revision, &idle, &absolute],
                    )
                    .await
                    .map_err(db)?;
                audit(&transaction, "login_completed", "ok", subject, &cookie_hash).await?;
                commit(transaction).await.map(|_| ())
            }
            .await
        };
        match finished {
            Ok(()) => Ok(session_cookie),
            Err(fault) if bootstrap_code(&fault) => {
                self.record("bootstrap_rejected", fault.code, subject, "").await?;
                Err(fault)
            }
            Err(fault) => Err(fault),
        }
    }

    pub async fn current(&self, config: &Config, cookie_hash: &str) -> Result<CurrentRecord, Fault> {
        let csrf = opaque_token()?;
        let csrf_hash = sha256_hex(csrf.as_bytes());
        let idle = seconds(config.idle_seconds)?;
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(db)?;
        set_local(&transaction).await?;
        let initialized = lock_initialized(&transaction).await?;
        let Some(row) = lock_session(&transaction, cookie_hash).await? else {
            return Err(Fault::unauthenticated());
        };
        if row.dead {
            return Err(Fault::unauthenticated());
        }
        if row.issuer != config.issuer {
            return Err(Fault::issuer());
        }
        let head = read_head(&transaction, sql::READ_HEAD_SHARE).await?;
        if row.config_commit != head.config_commit || row.revision != head.revision {
            return Err(Fault::revision());
        }
        let updated = transaction.query_opt(sql::SLIDE_SESSION, &[&cookie_hash, &csrf_hash, &idle]).await.map_err(db)?;
        let Some(updated) = updated else {
            return Err(Fault::unauthenticated());
        };
        let bindings = read_binding_rows(&transaction).await?;
        let idle_expires_at: String = updated.get("idle_at");
        let absolute_expires_at: String = updated.get("absolute_at");
        commit(transaction).await?;
        Ok(CurrentRecord {
            subject: row.subject.clone(),
            issuer: row.issuer,
            config_commit: row.config_commit,
            revision: row.revision,
            initialized,
            csrf,
            idle_expires_at,
            absolute_expires_at,
            projects: projects_from(&bindings, &row.subject, initialized),
        })
    }

    pub async fn preview_binding(&self, config: &Config, cookie_hash: &str, origin_ok: bool, csrf: &str, change: &Change) -> Result<PreviewReport, Fault> {
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(db)?;
        set_local(&transaction).await?;
        let caller = bind_caller(&transaction, config, cookie_hash, origin_ok, csrf).await?;
        if config.binding_git.is_none() {
            return Err(Fault::binding_unavailable());
        }
        let head = read_head(&transaction, sql::READ_HEAD_SHARE).await?;
        let bindings = read_binding_rows(&transaction).await?;
        let planned = crate::binding::plan(config, &caller.subject, &bindings, head.revision, &head.config_commit, change)?;
        commit(transaction).await?;
        Ok(preview_from(&planned))
    }

    pub async fn approve_binding(
        &self,
        config: &Config,
        cookie_hash: &str,
        origin_ok: bool,
        csrf: &str,
        change: &Change,
        git_commit: Option<&str>,
    ) -> Result<ApproveReport, Fault> {
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(db)?;
        set_local(&transaction).await?;
        let report = approve_in(&transaction, config, cookie_hash, origin_ok, csrf, change, git_commit).await?;
        commit(transaction).await?;
        Ok(report)
    }

    pub async fn logout(&self, cookie_hash: &str, origin_ok: bool, csrf: &str) -> Result<(), Fault> {
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(db)?;
        set_local(&transaction).await?;
        let Some(row) = lock_session(&transaction, cookie_hash).await? else {
            return Err(Fault::unauthenticated());
        };
        if row.dead {
            return Err(Fault::unauthenticated());
        }
        if !origin_ok {
            return Err(Fault::origin());
        }
        if !hashes_equal(&sha256_hex(csrf.as_bytes()), &row.csrf_hash) {
            return Err(Fault::csrf());
        }
        let updated = transaction.execute(sql::REVOKE_SESSION, &[&cookie_hash]).await.map_err(db)?;
        if updated != 1 {
            return Err(Fault::unauthenticated());
        }
        audit(&transaction, "logout", "ok", &row.subject, cookie_hash).await?;
        commit(transaction).await
    }

    pub async fn consume_bootstrap(&self, config: &Config, cookie_hash: &str, origin_ok: bool, csrf: &str, token: &str) -> Result<(), Fault> {
        let token_hash = sha256_hex(token.as_bytes());
        let subject = {
            let mut client = self.client.lock().await;
            let transaction = client.transaction().await.map_err(db)?;
            set_local(&transaction).await?;
            let result = consume_post(&transaction, config, cookie_hash, origin_ok, csrf, &token_hash).await;
            match result {
                Ok(subject) => match commit(transaction).await {
                    Ok(()) => Ok(subject),
                    Err(fault) => Err(fault),
                },
                Err(fault) => Err(fault),
            }
        };
        match subject {
            Ok(_) => Ok(()),
            Err(fault) if bootstrap_code(&fault) => {
                self.record("bootstrap_rejected", fault.code, "", "").await?;
                Err(fault)
            }
            Err(fault) => Err(fault),
        }
    }

    pub async fn authenticate(&self, cookie: &str) -> Result<VerifiedSession, Fault> {
        let cookie_hash = crate::cookie::session_hash(cookie)?;
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(db)?;
        set_local(&transaction).await?;
        let Some(row) = read_session(&transaction, &cookie_hash).await? else {
            return Err(Fault::unauthenticated());
        };
        if row.dead {
            return Err(Fault::unauthenticated());
        }
        let initialized = read_initialized(&transaction).await?;
        commit(transaction).await?;
        Ok(VerifiedSession::new(row.subject, row.issuer, row.config_commit, row.revision, initialized))
    }

    async fn record(&self, action: &str, code: &str, subject: &str, session_hash: &str) -> Result<(), Fault> {
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(db)?;
        set_local(&transaction).await?;
        audit(&transaction, action, code, subject, session_hash).await?;
        commit(transaction).await
    }
}

fn install_report(disposition: &'static str, sql_sha256: String) -> InstallReport {
    InstallReport {
        contract: CONTRACT,
        disposition,
        sql_sha256,
        data_grants: "none",
        owner_certification: "not_issued",
        agent_session: "not_issued",
        harness_session: "not_issued",
        gateway_token: "not_issued",
        customer_integration: "not_claimed",
    }
}

fn issue_report(token_hash: String) -> IssueReport {
    IssueReport {
        contract: CONTRACT,
        disposition: "issued",
        token_hash,
        fixture_class: "explicit_test_fixture",
        customer_owner: false,
        data_grants: "none",
        owner_certification: "not_issued",
        agent_session: "not_issued",
        harness_session: "not_issued",
        gateway_token: "not_issued",
        customer_integration: "not_claimed",
    }
}

async fn preflight_bootstrap(transaction: &Transaction<'_>, config: &Config, hash: &str) -> Result<(), Fault> {
    let initialized = lock_initialized(transaction).await?;
    let row = lock_bootstrap(transaction, hash).await?;
    classify_bootstrap(row, initialized, None, config)
}

async fn consume_for_login(transaction: &Transaction<'_>, config: &Config, hash: &str, subject: &str, session_hash: &str) -> Result<(), Fault> {
    let initialized = lock_initialized(transaction).await?;
    let row = lock_bootstrap(transaction, hash).await?;
    classify_bootstrap(row, initialized, Some(subject), config)?;
    let binding_ok = subject == config.bootstrap.subject;
    if !binding_ok {
        return Err(Fault::bootstrap_subject());
    }
    mark_bootstrap_consumed(transaction, hash, session_hash).await?;
    mark_initialized(transaction).await?;
    audit(transaction, "bootstrap_consumed", "ok", subject, session_hash).await
}

async fn consume_post(
    transaction: &Transaction<'_>,
    config: &Config,
    cookie_hash: &str,
    origin_ok: bool,
    csrf: &str,
    token_hash: &str,
) -> Result<String, Fault> {
    let initialized = lock_initialized(transaction).await?;
    let row = lock_bootstrap(transaction, token_hash).await?;
    let Some(session) = lock_session(transaction, cookie_hash).await? else {
        return Err(Fault::unauthenticated());
    };
    if session.dead {
        return Err(Fault::unauthenticated());
    }
    if !origin_ok {
        return Err(Fault::origin());
    }
    if !hashes_equal(&sha256_hex(csrf.as_bytes()), &session.csrf_hash) {
        return Err(Fault::csrf());
    }
    if session.issuer != config.issuer {
        return Err(Fault::issuer());
    }
    classify_bootstrap(row, initialized, Some(session.subject.as_str()), config)?;
    if session.subject != config.bootstrap.subject {
        return Err(Fault::bootstrap_subject());
    }
    mark_bootstrap_consumed(transaction, token_hash, cookie_hash).await?;
    mark_initialized(transaction).await?;
    audit(transaction, "bootstrap_consumed", "ok", &session.subject, cookie_hash).await?;
    Ok(session.subject)
}

fn classify_bootstrap(row: Option<BootRow>, initialized: bool, subject: Option<&str>, config: &Config) -> Result<(), Fault> {
    let Some(row) = row else {
        return Err(Fault::bootstrap_rejected());
    };
    if row.state == "consumed" {
        return Err(Fault::bootstrap_replay());
    }
    if initialized {
        return Err(Fault::bootstrap_unavailable());
    }
    if row.state != "issued" {
        return Err(Fault::bootstrap_rejected());
    }
    if row.expired {
        return Err(Fault::bootstrap_expired());
    }
    let binding = &config.bootstrap;
    if row.subject != binding.subject || row.project_id != binding.project_id || row.role != binding.role.as_str() {
        return Err(Fault::bootstrap_rejected());
    }
    if subject.is_some_and(|subject| row.subject != subject) {
        return Err(Fault::bootstrap_subject());
    }
    Ok(())
}

fn oauth_decision(row: &OauthRow, state: Option<&str>, verifier: &str, redirect_uri: &str) -> Result<BurnedLogin, Fault> {
    if row.consumed {
        return Err(Fault::state());
    }
    if row.expired {
        return Err(Fault::expired());
    }
    let state_ok = state.is_some_and(|value| hashes_equal(&sha256_hex(value.as_bytes()), &row.state_hash));
    if !state_ok {
        return Err(Fault::state());
    }
    if !hashes_equal(&pkce_challenge(verifier), &row.pkce_challenge) {
        return Err(Fault::pkce());
    }
    if row.redirect_uri != redirect_uri {
        return Err(Fault::redirect());
    }
    Ok(BurnedLogin {
        nonce_hash: row.nonce_hash.clone(),
        return_to: row.return_to.clone(),
        bootstrap_hash: row.bootstrap_hash.clone(),
        redirect_uri: row.redirect_uri.clone(),
    })
}

struct HeadRow {
    revision: i64,
    config_commit: String,
    git_commit: String,
    document_sha256: String,
}

struct IdempotentRow {
    request_sha256: String,
    revision: i64,
    config_commit: String,
    git_commit: String,
    document_sha256: String,
    receipt_sha256: String,
}

async fn seed_bindings(transaction: &Transaction<'_>, config: &Config) -> Result<(), Fault> {
    let document = management_document(&config.bindings)?;
    let document_sha256 = sha256_hex(document.as_bytes());
    transaction
        .execute(sql::INSERT_HEAD, &[&config.revision, &config.config_commit, &"", &document_sha256])
        .await
        .map_err(db)?;
    for binding in &config.bindings {
        transaction.execute(sql::INSERT_BINDING, &[&binding.project_id, &binding.subject, &binding.role.as_str()]).await.map_err(db)?;
    }
    Ok(())
}

async fn v2_matches(transaction: &Transaction<'_>) -> Result<bool, Fault> {
    let count = transaction.query_one(sql::V2_PRESENT, &[]).await.map_err(db)?;
    let present: i64 = count.get(0);
    if present == 0 {
        return Ok(false);
    }
    let row = transaction.query_opt(sql::READ_V2, &[]).await.map_err(db)?;
    let Some(row) = row else {
        return Ok(false);
    };
    let stored: String = row.get(0);
    Ok(hashes_equal(&stored, &sql::migration_v2_sha256()))
}

async fn read_head(transaction: &Transaction<'_>, statement: &str) -> Result<HeadRow, Fault> {
    let row = transaction.query_opt(statement, &[]).await.map_err(db)?;
    let Some(row) = row else {
        return Err(Fault::unavailable());
    };
    Ok(HeadRow { revision: row.get(0), config_commit: row.get(1), git_commit: row.get(2), document_sha256: row.get(3) })
}

async fn read_binding_rows(transaction: &Transaction<'_>) -> Result<Vec<Binding>, Fault> {
    let rows = transaction.query(sql::READ_BINDINGS, &[]).await.map_err(db)?;
    let mut bindings = Vec::with_capacity(rows.len());
    for row in rows {
        let project_id: String = row.get(0);
        let subject: String = row.get(1);
        let role: String = row.get(2);
        bindings.push(Binding { project_id, subject, role: Role::parse(&role)? });
    }
    Ok(bindings)
}

async fn bind_caller(transaction: &Transaction<'_>, config: &Config, cookie_hash: &str, origin_ok: bool, csrf: &str) -> Result<SessionRow, Fault> {
    let initialized = lock_initialized(transaction).await?;
    let Some(row) = lock_session(transaction, cookie_hash).await? else {
        return Err(Fault::unauthenticated());
    };
    if row.dead {
        return Err(Fault::unauthenticated());
    }
    if row.issuer != config.issuer {
        return Err(Fault::issuer());
    }
    if !origin_ok {
        return Err(Fault::origin());
    }
    if !hashes_equal(&sha256_hex(csrf.as_bytes()), &row.csrf_hash) {
        return Err(Fault::csrf());
    }
    if !initialized {
        return Err(Fault::forbidden());
    }
    Ok(row)
}

fn preview_from(planned: &Plan) -> PreviewReport {
    PreviewReport {
        outcome: planned.outcome,
        effect: planned.effect.clone(),
        project_id: planned.project_id.clone(),
        subject: planned.subject.clone(),
        role: planned.role,
        revision: planned.next_revision,
        config_commit: planned.next_config_commit.clone(),
        document: planned.document.clone(),
        document_sha256: planned.document_sha256.clone(),
    }
}

struct Applied<'a> {
    disposition: &'static str,
    outcome: &'static str,
    revision: i64,
    config_commit: &'a str,
    git_commit: &'a str,
    document_sha256: &'a str,
    receipt_sha256: &'a str,
}

fn approve_from(change: &Change, applied: Applied<'_>) -> ApproveReport {
    ApproveReport {
        disposition: applied.disposition,
        outcome: applied.outcome,
        effect: change.effect.clone(),
        project_id: change.project_id.clone(),
        subject: change.subject.clone(),
        role: change.role,
        revision: applied.revision,
        config_commit: applied.config_commit.to_string(),
        git_commit: applied.git_commit.to_string(),
        document_sha256: applied.document_sha256.to_string(),
        receipt_sha256: applied.receipt_sha256.to_string(),
    }
}

async fn approve_in(
    transaction: &Transaction<'_>,
    config: &Config,
    cookie_hash: &str,
    origin_ok: bool,
    csrf: &str,
    change: &Change,
    git_commit: Option<&str>,
) -> Result<ApproveReport, Fault> {
    let caller = bind_caller(transaction, config, cookie_hash, origin_ok, csrf).await?;
    let Some(git) = config.binding_git.as_ref() else {
        return Err(Fault::binding_unavailable());
    };
    let head = read_head(transaction, sql::LOCK_HEAD).await?;
    let bindings = read_binding_rows(transaction).await?;
    let request_hash = request_sha256(change)?;
    if let Some(stored) = read_idempotency(transaction, &change.idempotency_key).await? {
        if !hashes_equal(&stored.request_sha256, &request_hash) {
            return Err(Fault::idempotency());
        }
        let outcome = if stored.revision != change.expected_revision { "applied" } else { "unchanged" };
        let receipt = receipt_sha256(outcome, stored.revision, &stored.config_commit, &stored.git_commit, &stored.document_sha256)?;
        if !hashes_equal(&receipt, &stored.receipt_sha256) {
            return Err(Fault::unavailable());
        }
        return Ok(approve_from(
            change,
            Applied {
                disposition: "replayed",
                outcome,
                revision: stored.revision,
                config_commit: &stored.config_commit,
                git_commit: &stored.git_commit,
                document_sha256: &stored.document_sha256,
                receipt_sha256: &receipt,
            },
        ));
    }
    let planned = crate::binding::plan(config, &caller.subject, &bindings, head.revision, &head.config_commit, change)?;
    if planned.outcome == "unchanged" {
        if head.document_sha256 != planned.document_sha256 {
            return Err(Fault::git_rejected());
        }
        let actual = crate::git::read_head(git)?;
        if actual != change.expected_git_head {
            return Err(Fault::git_cas());
        }
        let outcome = "unchanged";
        let receipt = receipt_sha256(outcome, head.revision, &head.config_commit, &head.git_commit, &head.document_sha256)?;
        transaction
            .execute(
                sql::INSERT_IDEMPOTENCY,
                &[&change.idempotency_key, &request_hash, &head.revision, &head.config_commit, &head.git_commit, &head.document_sha256, &receipt],
            )
            .await
            .map_err(db)?;
        return Ok(approve_from(
            change,
            Applied {
                disposition: "unchanged",
                outcome,
                revision: head.revision,
                config_commit: &head.config_commit,
                git_commit: &head.git_commit,
                document_sha256: &head.document_sha256,
                receipt_sha256: &receipt,
            },
        ));
    }
    let Some(provided) = git_commit.filter(|value| crate::binding::hex40(value)) else {
        return Err(Fault::malformed());
    };
    let git_commit = provided.to_string();
    let current_document = management_document(&bindings)?;
    crate::git::verify_new_commit(git, &change.expected_git_head, &git_commit, planned.document.as_bytes(), &change.idempotency_key, &planned.document_sha256)?;
    if head.git_commit.is_empty() {
        crate::git::verify_tree_blob(git, &change.expected_git_head, current_document.as_bytes())?;
    } else if head.git_commit != change.expected_git_head {
        return Err(Fault::git_cas());
    }
    let updated = transaction
        .execute(
            sql::UPDATE_HEAD,
            &[&planned.next_revision, &planned.next_config_commit, &git_commit, &planned.document_sha256, &head.revision, &head.config_commit],
        )
        .await
        .map_err(db)?;
    if updated != 1 {
        return Err(Fault::revision());
    }
    match change.effect.as_str() {
        "grant" => {
            let role = change.role.ok_or_else(Fault::malformed)?;
            let wrote = transaction.execute(sql::UPSERT_BINDING, &[&change.project_id, &change.subject, &role.as_str()]).await.map_err(db)?;
            if wrote != 1 {
                return Err(Fault::unavailable());
            }
        }
        "revoke" => {
            let wrote = transaction.execute(sql::DELETE_BINDING, &[&change.project_id, &change.subject]).await.map_err(db)?;
            if wrote != 1 {
                return Err(Fault::malformed());
            }
        }
        _ => return Err(Fault::malformed()),
    }
    audit(transaction, "binding_applied", "ok", &change.subject, cookie_hash).await?;
    let outcome = "applied";
    let receipt = receipt_sha256(outcome, planned.next_revision, &planned.next_config_commit, &git_commit, &planned.document_sha256)?;
    transaction
        .execute(
            sql::INSERT_IDEMPOTENCY,
            &[&change.idempotency_key, &request_hash, &planned.next_revision, &planned.next_config_commit, &git_commit, &planned.document_sha256, &receipt],
        )
        .await
        .map_err(db)?;
    Ok(approve_from(
        change,
        Applied {
            disposition: "committed",
            outcome,
            revision: planned.next_revision,
            config_commit: &planned.next_config_commit,
            git_commit: &git_commit,
            document_sha256: &planned.document_sha256,
            receipt_sha256: &receipt,
        },
    ))
}

async fn read_idempotency(transaction: &Transaction<'_>, key: &str) -> Result<Option<IdempotentRow>, Fault> {
    let row = transaction.query_opt(sql::READ_IDEMPOTENCY, &[&key]).await.map_err(db)?;
    Ok(row.map(|row| IdempotentRow {
        request_sha256: row.get(0),
        revision: row.get(1),
        config_commit: row.get(2),
        git_commit: row.get(3),
        document_sha256: row.get(4),
        receipt_sha256: row.get(5),
    }))
}

async fn schema_count(transaction: &Transaction<'_>) -> Result<i64, Fault> {
    let row = transaction.query_one(sql::SCHEMA_PRESENT, &[]).await.map_err(db)?;
    Ok(row.get(0))
}

async fn read_manifest(transaction: &Transaction<'_>) -> Result<Option<(String, bool)>, Fault> {
    let row = transaction.query_opt(sql::READ_MANIFEST, &[]).await.map_err(db)?;
    Ok(row.map(|row| (row.get(0), row.get(1))))
}

async fn read_initialized(transaction: &Transaction<'_>) -> Result<bool, Fault> {
    match read_manifest(transaction).await? {
        Some((_, initialized)) => Ok(initialized),
        None => Err(Fault::unavailable()),
    }
}

async fn lock_initialized(transaction: &Transaction<'_>) -> Result<bool, Fault> {
    let row = transaction.query_opt(sql::LOCK_MANIFEST, &[]).await.map_err(db)?;
    match row {
        Some(row) => Ok(row.get(0)),
        None => Err(Fault::unavailable()),
    }
}

async fn lock_oauth(transaction: &Transaction<'_>, handle_hash: &str) -> Result<Option<OauthRow>, Fault> {
    let row = transaction.query_opt(sql::LOCK_OAUTH, &[&handle_hash]).await.map_err(db)?;
    Ok(row.map(|row| OauthRow {
        state_hash: row.get(0),
        nonce_hash: row.get(1),
        pkce_challenge: row.get(2),
        redirect_uri: row.get(3),
        return_to: row.get(4),
        bootstrap_hash: row.get(5),
        expired: row.get(6),
        consumed: row.get(7),
    }))
}

async fn consume_oauth(transaction: &Transaction<'_>, handle_hash: &str) -> Result<(), Fault> {
    let updated = transaction.execute(sql::CONSUME_OAUTH, &[&handle_hash]).await.map_err(db)?;
    if updated == 1 { Ok(()) } else { Err(Fault::state()) }
}

async fn lock_bootstrap(transaction: &Transaction<'_>, token_hash: &str) -> Result<Option<BootRow>, Fault> {
    let row = transaction.query_opt(sql::LOCK_BOOTSTRAP, &[&token_hash]).await.map_err(db)?;
    Ok(row.map(|row| BootRow {
        subject: row.get(0),
        project_id: row.get(1),
        role: row.get(2),
        state: row.get(3),
        expired: row.get(4),
    }))
}

async fn mark_bootstrap_consumed(transaction: &Transaction<'_>, token_hash: &str, session_hash: &str) -> Result<(), Fault> {
    let updated = transaction.execute(sql::CONSUME_BOOTSTRAP, &[&token_hash, &session_hash]).await.map_err(db)?;
    if updated == 1 { Ok(()) } else { Err(Fault::bootstrap_replay()) }
}

async fn mark_initialized(transaction: &Transaction<'_>) -> Result<(), Fault> {
    let updated = transaction.execute(sql::MARK_INITIALIZED, &[]).await.map_err(db)?;
    if updated == 1 { Ok(()) } else { Err(Fault::bootstrap_replay()) }
}

async fn lock_session(transaction: &Transaction<'_>, cookie_hash: &str) -> Result<Option<SessionRow>, Fault> {
    let row = transaction.query_opt(sql::LOCK_SESSION, &[&cookie_hash]).await.map_err(db)?;
    Ok(row.map(session_from_lock))
}

async fn read_session(transaction: &Transaction<'_>, cookie_hash: &str) -> Result<Option<SessionRow>, Fault> {
    let row = transaction.query_opt(sql::READ_SESSION, &[&cookie_hash]).await.map_err(db)?;
    Ok(row.map(|row| SessionRow {
        subject: row.get(0),
        issuer: row.get(1),
        config_commit: row.get(2),
        revision: row.get(3),
        csrf_hash: String::new(),
        _idle_at: String::new(),
        _absolute_at: String::new(),
        dead: row.get(4),
    }))
}

fn session_from_lock(row: tokio_postgres::Row) -> SessionRow {
    SessionRow {
        subject: row.get(0),
        issuer: row.get(1),
        config_commit: row.get(2),
        revision: row.get(3),
        csrf_hash: row.get(4),
        _idle_at: row.get(5),
        _absolute_at: row.get(6),
        dead: row.get(7),
    }
}

async fn audit(transaction: &Transaction<'_>, action: &str, code: &str, subject: &str, session_hash: &str) -> Result<(), Fault> {
    transaction.execute(sql::INSERT_AUDIT, &[&action, &code, &subject, &session_hash]).await.map_err(db)?;
    Ok(())
}

async fn set_local(transaction: &Transaction<'_>) -> Result<(), Fault> {
    transaction.batch_execute(sql::SET_LOCAL).await.map_err(db)
}

async fn commit(transaction: Transaction<'_>) -> Result<(), Fault> {
    transaction.commit().await.map_err(|_| Fault::commit_unknown())
}

fn write_token_file(path: &Path, token: &str) -> Result<(), Fault> {
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(path).map_err(|_| Fault::unavailable())?;
    let write_result = (|| -> Result<(), Fault> {
        file.set_permissions(std::fs::Permissions::from_mode(0o600)).map_err(|_| Fault::unavailable())?;
        use std::io::Write;
        file.write_all(format!("{token}\n").as_bytes()).map_err(|_| Fault::unavailable())?;
        file.sync_all().map_err(|_| Fault::unavailable())?;
        let mode = file.metadata().map_err(|_| Fault::unavailable())?.permissions().mode() & 0o777;
        if mode == 0o600 { Ok(()) } else { Err(Fault::unavailable()) }
    })();
    if write_result.is_err() {
        drop(file);
        let _ = fs::remove_file(path);
    }
    write_result
}

fn seconds(value: i64) -> Result<i32, Fault> {
    i32::try_from(value).map_err(|_| Fault::malformed())
}

fn bootstrap_code(fault: &Fault) -> bool {
    fault.code.starts_with("bootstrap.")
}

fn db<T>(_error: T) -> Fault {
    Fault::unavailable()
}
