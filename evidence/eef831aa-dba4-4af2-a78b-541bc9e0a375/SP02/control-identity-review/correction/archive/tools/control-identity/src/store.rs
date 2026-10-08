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

use crate::config::Config;
use crate::cookie::session_hash;
use crate::crypto::{b64, hashes_equal, opaque_token, pkce_challenge, sha256_hex};
use crate::error::{Fault, CONTRACT};
use crate::guard::VerifiedSession;
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
        store.install_once().await
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
        commit(transaction).await
    }

    async fn install_once(&self) -> Result<InstallReport, Fault> {
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(db)?;
        set_local(&transaction).await?;
        transaction.execute(sql::ADVISORY_LOCK, &[&sql::INSTALL_LOCK]).await.map_err(db)?;
        let hash = sql::migration_sha256();
        let disposition = if schema_count(&transaction).await? == 0 {
            transaction.batch_execute(sql::MIGRATION).await.map_err(db)?;
            transaction.execute(sql::INSERT_MANIFEST, &[&hash]).await.map_err(db)?;
            "committed"
        } else {
            match read_manifest(&transaction).await? {
                Some((stored, _)) if hashes_equal(&stored, &hash) => "replayed",
                Some(_) => return Err(Fault::drift()),
                None => return Err(Fault::drift()),
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
                insert_session(&transaction, config, &cookie_hash, &csrf_hash, subject, idle, absolute).await?;
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
        if row.config_commit != config.config_commit || row.revision != config.revision {
            return Err(Fault::revision());
        }
        let updated = transaction.query_opt(sql::SLIDE_SESSION, &[&cookie_hash, &csrf_hash, &idle]).await.map_err(db)?;
        let Some(updated) = updated else {
            return Err(Fault::unauthenticated());
        };
        let idle_expires_at: String = updated.get("idle_at");
        let absolute_expires_at: String = updated.get("absolute_at");
        commit(transaction).await?;
        Ok(CurrentRecord {
            subject: row.subject,
            issuer: row.issuer,
            config_commit: row.config_commit,
            revision: row.revision,
            initialized,
            csrf,
            idle_expires_at,
            absolute_expires_at,
        })
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

async fn insert_session(
    transaction: &Transaction<'_>,
    config: &Config,
    cookie_hash: &str,
    csrf_hash: &str,
    subject: &str,
    idle: i32,
    absolute: i32,
) -> Result<(), Fault> {
    transaction
        .execute(
            sql::INSERT_SESSION,
            &[&cookie_hash, &csrf_hash, &subject, &config.issuer, &config.config_commit, &config.revision, &idle, &absolute],
        )
        .await
        .map_err(db)?;
    Ok(())
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
