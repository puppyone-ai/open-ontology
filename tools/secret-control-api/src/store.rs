//! PostgreSQL secret versions. Plaintext is sealed before insert and is not a column.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_postgres::config::SslMode;
use tokio_postgres::error::SqlState;
use tokio_postgres::Transaction;
use zeroize::{Zeroize, Zeroizing};

use crate::aad::{canonical, Scope};
use crate::config::Config;
use crate::crypto::{hashes_equal, open, seal, NonceSource, CIPHER};
use crate::error::{metadata_body, Fault, CONTRACT};
use crate::sql::{self, migration_sha256};
use crate::tlsutil::pg_tls;

#[derive(Serialize)]
pub struct InstallReport {
    pub contract: &'static str,
    pub disposition: &'static str,
    pub sql_sha256: String,
    pub customer_kms: &'static str,
    pub mtls_ca: &'static str,
}

pub struct Mutation {
    pub replayed: bool,
    pub body: Value,
}

pub struct Resolved {
    pub meta: Value,
    pub value: Zeroizing<String>,
}

pub struct WriteRequest<'a> {
    pub scope: &'a Scope,
    pub workload: &'a str,
    pub subject: &'a str,
    pub value: &'a Zeroizing<String>,
    pub body_hash: &'a str,
    pub idempotency_key: &'a str,
    pub key: &'a [u8],
}

pub struct RotateRequest<'a> {
    pub scope: &'a Scope,
    pub workload: &'a str,
    pub subject: &'a str,
    pub value: &'a Zeroizing<String>,
    pub expected_version: i32,
    pub body_hash: &'a str,
    pub idempotency_key: &'a str,
    pub key: &'a [u8],
}

struct HeadRow {
    version: i32,
    state: String,
    workload: String,
    secret_ref: String,
    subject: String,
}

pub struct Store {
    client: Mutex<tokio_postgres::Client>,
    nonces: Arc<dyn NonceSource>,
}

impl Store {
    pub async fn connect(config: &Config, nonces: Arc<dyn NonceSource>) -> Result<Self, Fault> {
        let mut pg = tokio_postgres::Config::new();
        pg.user(&config.pg_user);
        pg.password(config.pg_password.as_str());
        pg.host(&config.pg_host);
        pg.port(config.pg_port);
        pg.dbname(&config.pg_database);
        pg.ssl_mode(SslMode::Require);
        pg.application_name("oo-secret-control");
        pg.connect_timeout(Duration::from_secs(10));
        let tls = pg_tls(&config.pg_ca_pem)?;
        let (client, connection) = pg.connect(tls).await.map_err(|_| Fault::not_ready())?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        client.batch_execute("SET TIME ZONE 'UTC'").await.map_err(|_| Fault::not_ready())?;
        Ok(Self { client: Mutex::new(client), nonces })
    }

    pub async fn install(config: &Config) -> Result<InstallReport, Fault> {
        let store = Self::connect(config, Arc::new(crate::crypto::OsNonce)).await?;
        store.install_once().await
    }

    pub async fn require_installed(&self) -> Result<(), Fault> {
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(|_| Fault::not_ready())?;
        transaction.batch_execute(sql::SET_LOCAL).await.map_err(|_| Fault::not_ready())?;
        let result = installed(&transaction).await;
        finish(transaction, result).await
    }

    pub async fn create(&self, request: WriteRequest<'_>) -> Result<Mutation, Fault> {
        let nonce = self.nonces.next()?;
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(|_| Fault::not_ready())?;
        transaction.batch_execute(sql::SET_LOCAL).await.map_err(|_| Fault::not_ready())?;
        let result = create_in(&transaction, &request, &nonce).await;
        finish(transaction, result).await
    }

    pub async fn rotate(&self, request: RotateRequest<'_>) -> Result<Mutation, Fault> {
        if request.expected_version < 1 || request.expected_version >= 1_000_000 {
            return Err(Fault::malformed());
        }
        let nonce = self.nonces.next()?;
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(|_| Fault::not_ready())?;
        transaction.batch_execute(sql::SET_LOCAL).await.map_err(|_| Fault::not_ready())?;
        let result = rotate_in(&transaction, &request, &nonce).await;
        finish(transaction, result).await
    }

    pub async fn revoke(&self, scope: &Scope, subject: &str, body_hash: &str, idempotency_key: &str) -> Result<Mutation, Fault> {
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(|_| Fault::not_ready())?;
        transaction.batch_execute(sql::SET_LOCAL).await.map_err(|_| Fault::not_ready())?;
        let result = revoke_in(&transaction, scope, subject, body_hash, idempotency_key).await;
        finish(transaction, result).await
    }

    pub async fn status(&self, scope: &Scope) -> Result<Value, Fault> {
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(|_| Fault::not_ready())?;
        transaction.batch_execute(sql::SET_LOCAL).await.map_err(|_| Fault::not_ready())?;
        let result = status_in(&transaction, scope).await;
        finish(transaction, result).await
    }

    pub async fn resolve(&self, scope: &Scope, workload: &str, pinned: Option<i32>, key: &[u8]) -> Result<Resolved, Fault> {
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(|_| Fault::not_ready())?;
        transaction.batch_execute(sql::SET_LOCAL).await.map_err(|_| Fault::not_ready())?;
        let result = resolve_in(&transaction, scope, workload, pinned, key).await;
        finish(transaction, result).await
    }

    async fn install_once(&self) -> Result<InstallReport, Fault> {
        let hash = migration_sha256();
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(|_| Fault::not_ready())?;
        transaction.batch_execute(sql::SET_LOCAL).await.map_err(|_| Fault::not_ready())?;
        let result = install_in(&transaction, &hash).await;
        let disposition = finish(transaction, result).await?;
        Ok(InstallReport {
            contract: CONTRACT,
            disposition,
            sql_sha256: hash,
            customer_kms: "external_gate",
            mtls_ca: "not_issued",
        })
    }
}

async fn finish<T>(transaction: Transaction<'_>, result: Result<T, Fault>) -> Result<T, Fault> {
    match result {
        Ok(value) => {
            transaction.commit().await.map_err(|_| Fault::commit_unknown())?;
            Ok(value)
        }
        Err(fault) => Err(fault),
    }
}

async fn installed(transaction: &Transaction<'_>) -> Result<(), Fault> {
    if schema_count(transaction).await? == 0 {
        return Err(Fault::not_ready());
    }
    match read_manifest(transaction).await? {
        Some(stored) if hashes_equal(&stored, &migration_sha256()) => Ok(()),
        _ => Err(Fault::drift()),
    }
}

async fn install_in(transaction: &Transaction<'_>, hash: &str) -> Result<&'static str, Fault> {
    transaction.execute(sql::ADVISORY_LOCK, &[&sql::INSTALL_LOCK]).await.map_err(|_| Fault::not_ready())?;
    if schema_count(transaction).await? == 0 {
        transaction.batch_execute(sql::MIGRATION).await.map_err(|_| Fault::not_ready())?;
        transaction.execute(sql::INSERT_MANIFEST, &[&hash]).await.map_err(|_| Fault::not_ready())?;
        Ok("committed")
    } else {
        match read_manifest(transaction).await? {
            Some(stored) if hashes_equal(&stored, hash) => Ok("replayed"),
            _ => Err(Fault::drift()),
        }
    }
}

async fn create_in(transaction: &Transaction<'_>, request: &WriteRequest<'_>, nonce: &[u8; 12]) -> Result<Mutation, Fault> {
    if let Some(body) = idempotency(transaction, request.scope.project.as_str(), request.idempotency_key, request.body_hash).await? {
        return Ok(Mutation { replayed: true, body });
    }
    if lock_head(transaction, request.scope).await?.is_some() {
        return Err(Fault::version_conflict());
    }
    let version = 1_i32;
    insert_sealed(transaction, request.scope, version, nonce, request.key, request.value).await?;
    let reference = request.scope.reference();
    transaction
        .execute(
            sql::INSERT_HEAD,
            &[
                &request.scope.deployment,
                &request.scope.project,
                &request.scope.namespace,
                &request.scope.secret_id,
                &version,
                &request.workload,
                &reference,
                &request.subject,
            ],
        )
        .await
        .map_err(map_write)?;
    let body = metadata_body("putSecret", &reference, version, &request.scope.namespace, &request.scope.project, request.workload, "active");
    record(transaction, request.scope, saved("write", version, request.subject, request.workload, request.idempotency_key, request.body_hash, &body)).await?;
    Ok(Mutation { replayed: false, body })
}

async fn rotate_in(transaction: &Transaction<'_>, request: &RotateRequest<'_>, nonce: &[u8; 12]) -> Result<Mutation, Fault> {
    if let Some(body) = idempotency(transaction, request.scope.project.as_str(), request.idempotency_key, request.body_hash).await? {
        return Ok(Mutation { replayed: true, body });
    }
    let Some(head) = lock_head(transaction, request.scope).await? else {
        return Err(Fault::not_found());
    };
    if head.state == "revoked" {
        return Err(Fault::revoked());
    }
    if head.workload != request.workload {
        return Err(Fault::workload());
    }
    if head.version != request.expected_version {
        return Err(Fault::version_conflict());
    }
    let version = request.expected_version + 1;
    insert_sealed(transaction, request.scope, version, nonce, request.key, request.value).await?;
    let activated = transaction
        .execute(
            sql::ACTIVATE_HEAD,
            &[&request.scope.deployment, &request.scope.project, &request.scope.namespace, &request.scope.secret_id, &version, &request.expected_version],
        )
        .await
        .map_err(map_write)?;
    one_row(activated)?;
    let retired = transaction
        .execute(sql::RETIRE_VERSION, &[&request.scope.deployment, &request.scope.project, &request.scope.namespace, &request.scope.secret_id, &request.expected_version])
        .await
        .map_err(map_write)?;
    one_row(retired)?;
    let body = metadata_body("rotateSecret", &head.secret_ref, version, &request.scope.namespace, &request.scope.project, &head.workload, "active");
    record(transaction, request.scope, saved("rotate", version, request.subject, &head.workload, request.idempotency_key, request.body_hash, &body)).await?;
    Ok(Mutation { replayed: false, body })
}

async fn revoke_in(transaction: &Transaction<'_>, scope: &Scope, subject: &str, body_hash: &str, idempotency_key: &str) -> Result<Mutation, Fault> {
    if let Some(body) = idempotency(transaction, scope.project.as_str(), idempotency_key, body_hash).await? {
        return Ok(Mutation { replayed: true, body });
    }
    let Some(head) = lock_head(transaction, scope).await? else {
        return Err(Fault::not_found());
    };
    if head.state == "revoked" {
        return Err(Fault::revoked());
    }
    one_row(transaction.execute(sql::REVOKE_HEAD, &[&scope.deployment, &scope.project, &scope.namespace, &scope.secret_id]).await.map_err(map_write)?)?;
    one_row(transaction.execute(sql::REVOKE_VERSION, &[&scope.deployment, &scope.project, &scope.namespace, &scope.secret_id, &head.version]).await.map_err(map_write)?)?;
    let body = metadata_body("revokeSecret", &head.secret_ref, head.version, &scope.namespace, &scope.project, &head.workload, "revoked");
    record(transaction, scope, saved("revoke", head.version, subject, &head.workload, idempotency_key, body_hash, &body)).await?;
    Ok(Mutation { replayed: false, body })
}

async fn status_in(transaction: &Transaction<'_>, scope: &Scope) -> Result<Value, Fault> {
    let Some(head) = lock_head(transaction, scope).await? else {
        return Err(Fault::not_found());
    };
    let mut body = metadata_body("getSecretStatus", &head.secret_ref, head.version, &scope.namespace, &scope.project, &head.workload, &head.state);
    body["subject"] = Value::String(head.subject);
    Ok(body)
}

async fn resolve_in(transaction: &Transaction<'_>, scope: &Scope, workload: &str, pinned: Option<i32>, key: &[u8]) -> Result<Resolved, Fault> {
    let Some(head) = lock_head(transaction, scope).await? else {
        return Err(Fault::not_found());
    };
    if head.secret_ref != scope.reference() {
        return reject(transaction, scope, &head, workload, Fault::aead()).await;
    }
    if workload != head.workload {
        return reject(transaction, scope, &head, workload, Fault::workload()).await;
    }
    if head.state == "revoked" {
        return reject(transaction, scope, &head, workload, Fault::revoked()).await;
    }
    if pinned.is_some_and(|version| version != head.version) {
        return reject(transaction, scope, &head, workload, Fault::retired()).await;
    }
    let row = transaction
        .query_opt(sql::READ_VERSION, &[&scope.deployment, &scope.project, &scope.namespace, &scope.secret_id, &head.version])
        .await
        .map_err(|_| Fault::not_ready())?;
    let Some(row) = row else {
        return reject(transaction, scope, &head, workload, Fault::retired()).await;
    };
    let nonce: Vec<u8> = row.get(0);
    let ciphertext: Vec<u8> = row.get(1);
    let cipher: String = row.get(2);
    let state: String = row.get(3);
    if state != "active" || cipher != CIPHER || nonce.len() != 12 {
        return reject(transaction, scope, &head, workload, Fault::retired()).await;
    }
    let mut nonce_bytes = [0_u8; 12];
    nonce_bytes.copy_from_slice(&nonce);
    let opened = match open(key, &nonce_bytes, &canonical(scope, head.version), &ciphertext) {
        Ok(bytes) => bytes,
        Err(fault) => return reject(transaction, scope, &head, workload, fault).await,
    };
    let text = match String::from_utf8(opened.to_vec()) {
        Ok(text) => Zeroizing::new(text),
        Err(_error) => return reject(transaction, scope, &head, workload, Fault::aead()).await,
    };
    audit(transaction, "resolve", "ok", scope, head.version, &head.subject, workload).await?;
    let meta = metadata_body("resolveSecret", &head.secret_ref, head.version, &scope.namespace, &scope.project, &head.workload, "active");
    Ok(Resolved { meta, value: text })
}

async fn reject<T>(transaction: &Transaction<'_>, scope: &Scope, head: &HeadRow, workload: &str, fault: Fault) -> Result<T, Fault> {
    audit(transaction, "resolve_rejected", fault.code, scope, head.version, &head.subject, workload).await?;
    Err(fault)
}

async fn insert_sealed(transaction: &Transaction<'_>, scope: &Scope, version: i32, nonce: &[u8; 12], key: &[u8], value: &Zeroizing<String>) -> Result<(), Fault> {
    let aad = canonical(scope, version);
    let mut ciphertext = seal(key, nonce, &aad, value.as_bytes())?;
    let nonce_bytes: &[u8] = nonce;
    let cipher_bytes: &[u8] = ciphertext.as_slice();
    let inserted = transaction
        .execute(sql::INSERT_VERSION, &[&scope.deployment, &scope.project, &scope.namespace, &scope.secret_id, &version, &nonce_bytes, &cipher_bytes, &CIPHER])
        .await
        .map_err(map_write);
    ciphertext.zeroize();
    inserted?;
    Ok(())
}

struct Saved<'a> {
    action: &'a str,
    version: i32,
    subject: &'a str,
    workload: &'a str,
    idempotency_key: &'a str,
    body_hash: &'a str,
    body: &'a Value,
}

fn saved<'a>(action: &'a str, version: i32, subject: &'a str, workload: &'a str, idempotency_key: &'a str, body_hash: &'a str, body: &'a Value) -> Saved<'a> {
    Saved { action, version, subject, workload, idempotency_key, body_hash, body }
}

async fn record(transaction: &Transaction<'_>, scope: &Scope, row: Saved<'_>) -> Result<(), Fault> {
    let encoded = serde_json::to_string(row.body).map_err(|_| Fault::not_ready())?;
    transaction.execute(sql::INSERT_IDEMPOTENCY, &[&scope.project, &row.idempotency_key, &row.body_hash, &encoded]).await.map_err(map_write)?;
    audit(transaction, row.action, "ok", scope, row.version, row.subject, row.workload).await
}

async fn idempotency(transaction: &Transaction<'_>, project: &str, key: &str, body_hash: &str) -> Result<Option<Value>, Fault> {
    let row = transaction.query_opt(sql::LOCK_IDEMPOTENCY, &[&project, &key]).await.map_err(|_| Fault::not_ready())?;
    let Some(row) = row else {
        return Ok(None);
    };
    let stored_hash: String = row.get(0);
    let response: String = row.get(1);
    if !hashes_equal(&stored_hash, body_hash) {
        return Err(Fault::idempotency());
    }
    let value: Value = serde_json::from_str(&response).map_err(|_| Fault::not_ready())?;
    if value.get("value").is_some() {
        return Err(Fault::not_ready());
    }
    Ok(Some(value))
}

async fn lock_head(transaction: &Transaction<'_>, scope: &Scope) -> Result<Option<HeadRow>, Fault> {
    let row = transaction.query_opt(sql::LOCK_HEAD, &[&scope.deployment, &scope.project, &scope.namespace, &scope.secret_id]).await.map_err(|_| Fault::not_ready())?;
    Ok(row.map(|row| HeadRow {
        version: row.get(0),
        state: row.get(1),
        workload: row.get(2),
        secret_ref: row.get(3),
        subject: row.get(4),
    }))
}

async fn audit(transaction: &Transaction<'_>, action: &str, code: &str, scope: &Scope, version: i32, subject: &str, workload: &str) -> Result<(), Fault> {
    transaction
        .execute(sql::INSERT_AUDIT, &[&action, &code, &scope.project, &scope.namespace, &scope.secret_id, &version, &subject, &workload])
        .await
        .map_err(|_| Fault::not_ready())?;
    Ok(())
}

async fn schema_count(transaction: &Transaction<'_>) -> Result<i64, Fault> {
    let row = transaction.query_one(sql::SCHEMA_PRESENT, &[]).await.map_err(|_| Fault::not_ready())?;
    Ok(row.get(0))
}

async fn read_manifest(transaction: &Transaction<'_>) -> Result<Option<String>, Fault> {
    let row = transaction.query_opt(sql::READ_MANIFEST, &[]).await.map_err(|_| Fault::not_ready())?;
    Ok(row.map(|row| row.get(0)))
}

fn one_row(count: u64) -> Result<(), Fault> {
    if count == 1 { Ok(()) } else { Err(Fault::version_conflict()) }
}

fn map_write(error: tokio_postgres::Error) -> Fault {
    if error.code() == Some(&SqlState::UNIQUE_VIOLATION) {
        let name = error.as_db_error().and_then(|db| db.constraint()).unwrap_or("");
        if name.contains("idempotency") {
            return Fault::idempotency();
        }
        return Fault::version_conflict();
    }
    Fault::not_ready()
}
