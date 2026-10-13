//! PostgreSQL metadata. Plaintext is sealed before it touches disk.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio_postgres::error::SqlState;
use tokio_postgres::config::SslMode;
use tokio_postgres::Transaction;
use zeroize::Zeroizing;

use crate::config::Config;
use crate::crypto::{self, aad, open, random_bytes, random_hex, seal, sha256_hex, TTL_MS};
use crate::error::{card_flags, Fault, CONTRACT};
use crate::export_run::ReadyExport;
use crate::paths::{object_id_ok, read_ciphertext, remove_object, write_ciphertext};
use crate::scope::Scope;
use crate::sql::{self, migration_sha256};
use crate::tlsutil::pg_tls;

pub struct Store {
    client: Mutex<tokio_postgres::Client>,
    deployment: String,
}

#[derive(Debug)]
pub struct CleanupReport {
    pub expired: i64,
    pub idle: bool,
}

pub struct AnchorLoss {
    pub reason: String,
    pub source_sha256: String,
    pub absolute_end_unix: i64,
}

#[derive(Debug)]
pub enum Outcome {
    Created(Value),
    Replay(Value),
    Loss(Value),
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
        pg.application_name("oo-artifact-control");
        pg.connect_timeout(std::time::Duration::from_secs(10));
        let tls = pg_tls(&config.pg_ca_pem)?;
        let (client, connection) = pg.connect(tls).await.map_err(|_| Fault::not_ready())?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        client.batch_execute("SET TIME ZONE 'UTC'").await.map_err(|_| Fault::not_ready())?;
        Ok(Self { client: Mutex::new(client), deployment: config.deployment.clone() })
    }

    pub async fn install(config: &Config) -> Result<Value, Fault> {
        let store = Self::connect(config).await?;
        store.install_once().await
    }

    pub async fn require_installed(&self) -> Result<(), Fault> {
        let mut client = self.client.lock().await;
        let tx = client.transaction().await.map_err(|_| Fault::not_ready())?;
        tx.batch_execute(sql::SET_LOCAL).await.map_err(|_| Fault::not_ready())?;
        let result = manifest_matches(&tx).await;
        finish(tx, result).await
    }

    pub async fn anchor_end(&self, session_id: &str) -> Result<i64, Fault> {
        let client = self.client.lock().await;
        let row = client.query_opt(sql::LOCK_ANCHOR.trim_end_matches(" FOR UPDATE"), &[&session_id]).await.map_err(|_| Fault::not_ready())?;
        row.map(|row| row.get(0)).ok_or_else(Fault::not_found)
    }

    pub async fn active_count(&self, session_id: &str) -> Result<i64, Fault> {
        let client = self.client.lock().await;
        let row = client.query_one(sql::ACTIVE_COUNT, &[&self.deployment, &session_id]).await.map_err(|_| Fault::not_ready())?;
        Ok(row.get(0))
    }

    pub async fn loss_count(&self, session_id: &str) -> Result<i64, Fault> {
        let client = self.client.lock().await;
        let row = client.query_one(sql::LOSS_COUNT, &[&session_id]).await.map_err(|_| Fault::not_ready())?;
        Ok(row.get(0))
    }

    pub async fn latest_loss(&self, session_id: &str) -> Result<AnchorLoss, Fault> {
        let client = self.client.lock().await;
        let row = client.query_opt(sql::LATEST_LOSS, &[&session_id]).await.map_err(|_| Fault::not_ready())?;
        let row = row.ok_or_else(Fault::not_found)?;
        Ok(AnchorLoss { reason: row.get(0), source_sha256: row.get(1), absolute_end_unix: row.get(2) })
    }

    pub async fn extension_is_rejected(&self, session_id: &str) -> Result<(), Fault> {
        let client = self.client.lock().await;
        match client.execute(sql::EXTEND_ANCHOR, &[&session_id]).await {
            Ok(_) => Err(Fault::forbidden()),
            Err(error) if error.code() == Some(&SqlState::INSUFFICIENT_PRIVILEGE) => Ok(()),
            Err(_) => Err(Fault::not_ready()),
        }
    }

    pub async fn loss_delete_is_rejected(&self) -> Result<(), Fault> {
        let client = self.client.lock().await;
        match client.execute(sql::DELETE_LOSS, &[]).await {
            Ok(_) => Err(Fault::forbidden()),
            Err(error) if error.code() == Some(&SqlState::INSUFFICIENT_PRIVILEGE) => Ok(()),
            Err(_) => Err(Fault::not_ready()),
        }
    }

    pub async fn record_loss(
        &self,
        scope: &Scope,
        reason: &'static str,
        source_sha256: &str,
        idempotency_key: &str,
        body_hash: &str,
    ) -> Result<Outcome, Fault> {
        let mut client = self.client.lock().await;
        let tx = client.transaction().await.map_err(|_| Fault::not_ready())?;
        tx.batch_execute(sql::SET_LOCAL).await.map_err(|_| Fault::not_ready())?;
        tx.execute(sql::ADVISORY_LOCK, &[&sql::INSTALL_LOCK]).await.map_err(|_| Fault::not_ready())?;
        let result = loss_in(&tx, scope, reason, source_sha256, idempotency_key, body_hash).await;
        finish(tx, result).await
    }

    pub async fn store_ready(
        &self,
        config: &Config,
        scope: &Scope,
        ready: &ReadyExport,
        idempotency_key: &str,
        body_hash: &str,
    ) -> Result<Outcome, Fault> {
        let mut client = self.client.lock().await;
        let tx = client.transaction().await.map_err(|_| Fault::not_ready())?;
        tx.batch_execute(sql::SET_LOCAL).await.map_err(|_| Fault::not_ready())?;
        tx.execute(sql::ADVISORY_LOCK, &[&sql::INSTALL_LOCK]).await.map_err(|_| Fault::not_ready())?;
        let result = store_in(&tx, &self.deployment, config, scope, ready, idempotency_key, body_hash).await;
        finish(tx, result).await
    }

    pub async fn list_active(&self, scope: &Scope) -> Result<Vec<Value>, Fault> {
        self.ensure_open(scope).await?;
        let now = now_ms()?;
        let client = self.client.lock().await;
        let rows = client
            .query(
                sql::LIST_ACTIVE,
                &[
                    &self.deployment,
                    &scope.principal_id,
                    &scope.project_id,
                    &scope.session_id,
                    &scope.role,
                    &scope.policy_id,
                    &scope.knowledge_id,
                    &scope.knowledge_digest,
                    &now,
                ],
            )
            .await
            .map_err(|_| Fault::not_ready())?;
        let mut out = Vec::new();
        for row in rows {
            out.push(manifest_value(&row.get::<_, String>(0), &row.get::<_, String>(1), row.get(2), &row.get::<_, String>(3), row.get(4), "active"));
        }
        Ok(out)
    }

    pub async fn detail(&self, scope: &Scope, object_id: &str) -> Result<Value, Fault> {
        if !object_id_ok(object_id) {
            return Err(Fault::not_found());
        }
        self.ensure_open(scope).await?;
        let client = self.client.lock().await;
        let row = client.query_opt(sql::READ_ARTIFACT, &[&self.deployment, &object_id]).await.map_err(|_| Fault::not_ready())?;
        let row = row.ok_or_else(Fault::not_found)?;
        authorize(scope, &row, now_ms()?, now_unix()?)?;
        Ok(manifest_value(
            object_id,
            &row.get::<_, String>(7),
            row.get(8),
            &row.get::<_, String>(9),
            row.get(14),
            row.get::<_, String>(16).as_str(),
        ))
    }

    pub async fn download(&self, config: &Config, scope: &Scope, object_id: &str) -> Result<Zeroizing<Vec<u8>>, Fault> {
        if !object_id_ok(object_id) {
            return Err(Fault::not_found());
        }
        self.ensure_open(scope).await?;
        let client = self.client.lock().await;
        let row = client.query_opt(sql::READ_ARTIFACT, &[&self.deployment, &object_id]).await.map_err(|_| Fault::not_ready())?;
        let row = row.ok_or_else(Fault::not_found)?;
        authorize(scope, &row, now_ms()?, now_unix()?)?;
        let nonce_vec: Vec<u8> = row.get(11);
        let nonce: [u8; 12] = nonce_vec.try_into().map_err(|_| Fault::aead())?;
        let ciphertext_sha: String = row.get(12);
        let plaintext_sha: String = row.get(7);
        let plaintext_size: i64 = row.get(8);
        drop(client);
        let bytes = read_ciphertext(&config.object_dir, object_id)?;
        if sha256_hex(&bytes) != ciphertext_sha {
            return Err(Fault::aead());
        }
        let aad_bytes = artifact_aad(&self.deployment, scope_tuple(scope), object_id, &plaintext_sha)?;
        let plain = open(&config.aead_key, &nonce, &aad_bytes, &bytes)?;
        if sha256_hex(&plain) != plaintext_sha || i64::try_from(plain.len()).ok() != Some(plaintext_size) {
            return Err(Fault::aead());
        }
        Ok(plain)
    }

    pub async fn revoke(&self, scope: &Scope, object_id: &str, idempotency_key: &str, body_hash: &str) -> Result<Outcome, Fault> {
        if !object_id_ok(object_id) {
            return Err(Fault::not_found());
        }
        self.ensure_open(scope).await?;
        let mut client = self.client.lock().await;
        let tx = client.transaction().await.map_err(|_| Fault::not_ready())?;
        tx.batch_execute(sql::SET_LOCAL).await.map_err(|_| Fault::not_ready())?;
        tx.execute(sql::ADVISORY_LOCK, &[&sql::INSTALL_LOCK]).await.map_err(|_| Fault::not_ready())?;
        let result = revoke_in(&tx, &self.deployment, scope, object_id, idempotency_key, body_hash).await;
        finish(tx, result).await
    }

    pub async fn cleanup_before(&self, object_dir: &Path, now: i64) -> Result<CleanupReport, Fault> {
        let mut client = self.client.lock().await;
        let tx = client.transaction().await.map_err(|_| Fault::not_ready())?;
        tx.batch_execute(sql::SET_LOCAL).await.map_err(|_| Fault::not_ready())?;
        let statement = sql::EXPIRE.replace(" RETURNING object_id", "");
        let updated = tx.execute(statement.as_str(), &[&self.deployment, &now]).await.map_err(|_| Fault::not_ready())?;
        let rows = tx.query("SELECT object_id FROM oo_artifact_control.artifact WHERE deployment = $1::text AND state = 'expired'", &[&self.deployment]).await.map_err(|_| Fault::not_ready())?;
        finish(tx, Ok(())).await?;
        for row in rows {
            let id: String = row.get(0);
            remove_object(object_dir, &id)?;
        }
        let expired = i64::try_from(updated).map_err(|_| Fault::not_ready())?;
        Ok(CleanupReport { expired, idle: expired == 0 })
    }

    pub async fn reconcile(&self, object_dir: &Path) -> Result<usize, Fault> {
        let client = self.client.lock().await;
        let rows = client.query(sql::KNOWN_IDS, &[&self.deployment]).await.map_err(|_| Fault::not_ready())?;
        let mut known = HashSet::new();
        for row in rows {
            known.insert(row.get::<_, String>(0));
        }
        drop(client);
        let mut removed = 0;
        let entries = fs::read_dir(object_dir).map_err(|_| Fault::path())?;
        for entry in entries {
            let entry = entry.map_err(|_| Fault::path())?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let meta = fs::symlink_metadata(entry.path()).map_err(|_| Fault::path())?;
            if !(meta.file_type().is_file() || meta.file_type().is_symlink()) {
                continue;
            }
            if let Some(id) = name.strip_suffix(".bin")
                && object_id_ok(id)
                && !known.contains(id)
            {
                fs::remove_file(entry.path()).map_err(|_| Fault::path())?;
                removed += 1;
            } else if let Some(rest) = name.strip_prefix('.')
                && let Some(id) = rest.strip_suffix(".partial")
                && object_id_ok(id)
            {
                fs::remove_file(entry.path()).map_err(|_| Fault::path())?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub async fn consume(&self, scope: &Scope) -> Result<(String, String), Fault> {
        let cookie = crypto::token()?;
        let csrf = crypto::token()?;
        let client = self.client.lock().await;
        client
            .execute(sql::INSERT_COOKIE, &[&sha256_hex(cookie.as_bytes()), &sha256_hex(csrf.as_bytes()), &scope.session_id, &scope.principal_id, &scope.project_id])
            .await
            .map_err(|_| Fault::not_ready())?;
        Ok((cookie, csrf))
    }

    pub async fn session_for_cookie(&self, cookie: &str) -> Result<(String, String, String, String), Fault> {
        let client = self.client.lock().await;
        let row = client.query_opt(sql::READ_COOKIE, &[&sha256_hex(cookie.as_bytes())]).await.map_err(|_| Fault::not_ready())?;
        let row = row.ok_or_else(Fault::unauthenticated)?;
        Ok((row.get(0), row.get(1), row.get(2), row.get(3)))
    }

    async fn install_once(&self) -> Result<Value, Fault> {
        let mut client = self.client.lock().await;
        let tx = client.transaction().await.map_err(|_| Fault::not_ready())?;
        tx.batch_execute(sql::SET_LOCAL).await.map_err(|_| Fault::not_ready())?;
        tx.execute(sql::ADVISORY_LOCK, &[&sql::INSTALL_LOCK]).await.map_err(|_| Fault::not_ready())?;
        let present: i64 = tx.query_one(sql::SCHEMA_PRESENT, &[]).await.map_err(|_| Fault::not_ready())?.get(0);
        let hash = migration_sha256();
        let disposition = if present == 0 {
            tx.batch_execute(sql::MIGRATION).await.map_err(|_| Fault::not_ready())?;
            tx.execute(sql::INSERT_MANIFEST, &[&hash]).await.map_err(|_| Fault::not_ready())?;
            "installed"
        } else {
            let stored: String = tx.query_one(sql::READ_MANIFEST, &[]).await.map_err(|_| Fault::not_ready())?.get(0);
            if stored != hash {
                return finish(tx, Err(Fault::drift())).await;
            }
            "unchanged"
        };
        let body = json!({
            "contract": CONTRACT,
            "disposition": disposition,
            "sql_sha256": hash,
            "signed_url": null,
            "workspace": "disabled",
            "production_signed": false,
            "runsc": "not_claimed",
            "customer_integration": "not_claimed",
            "parent_complete": false
        });
        finish(tx, Ok(body)).await
    }

    async fn ensure_open(&self, scope: &Scope) -> Result<(), Fault> {
        let now = now_unix()?;
        if now >= scope.absolute_end_unix {
            return Err(Fault::expired());
        }
        match self.anchor_end(&scope.session_id).await {
            Ok(end) if now >= end => Err(Fault::expired()),
            Ok(_) => Ok(()),
            Err(fault) if fault == Fault::not_found() => Ok(()),
            Err(fault) => Err(fault),
        }
    }
}

fn scope_tuple(scope: &Scope) -> (&str, &str, &str, &str, &str, &str, &str) {
    (
        scope.project_id.as_str(),
        scope.principal_id.as_str(),
        scope.session_id.as_str(),
        scope.role.as_str(),
        scope.policy_id.as_str(),
        scope.knowledge_id.as_str(),
        scope.knowledge_digest.as_str(),
    )
}

fn artifact_aad(deployment: &str, tuple: (&str, &str, &str, &str, &str, &str, &str), object_id: &str, plaintext_sha: &str) -> Result<Vec<u8>, Fault> {
    aad(&[
        b"oo.artifact-control/aad/v1",
        deployment.as_bytes(),
        tuple.0.as_bytes(),
        tuple.1.as_bytes(),
        tuple.2.as_bytes(),
        tuple.3.as_bytes(),
        tuple.4.as_bytes(),
        tuple.5.as_bytes(),
        tuple.6.as_bytes(),
        object_id.as_bytes(),
        plaintext_sha.as_bytes(),
    ])
}

async fn manifest_matches(tx: &Transaction<'_>) -> Result<(), Fault> {
    let present: i64 = tx.query_one(sql::SCHEMA_PRESENT, &[]).await.map_err(|_| Fault::not_ready())?.get(0);
    if present == 0 {
        return Err(Fault::not_ready());
    }
    let stored: String = tx.query_one(sql::READ_MANIFEST, &[]).await.map_err(|_| Fault::not_ready())?.get(0);
    if stored != migration_sha256() {
        Err(Fault::drift())
    } else {
        Ok(())
    }
}

async fn loss_in(
    tx: &Transaction<'_>,
    scope: &Scope,
    reason: &str,
    source_sha256: &str,
    idempotency_key: &str,
    body_hash: &str,
) -> Result<Outcome, Fault> {
    if let Some(outcome) = replay(tx, &scope.session_id, idempotency_key, body_hash).await? {
        return Ok(outcome);
    }
    let end = remember_end(tx, scope).await?;
    let loss_id = random_hex(16)?;
    let created = now_ms()?;
    tx.execute(sql::INSERT_LOSS, &[&loss_id, &scope.session_id, &end, &reason, &source_sha256, &created])
        .await
        .map_err(|_| Fault::not_ready())?;
    let mut body = loss_body(reason);
    body["replayed"] = json!(false);
    insert_idempotency(tx, &scope.session_id, idempotency_key, body_hash, &body).await?;
    Ok(Outcome::Loss(body))
}

async fn store_in(
    tx: &Transaction<'_>,
    deployment: &str,
    config: &Config,
    scope: &Scope,
    ready: &ReadyExport,
    idempotency_key: &str,
    body_hash: &str,
) -> Result<Outcome, Fault> {
    if let Some(outcome) = replay(tx, &scope.session_id, idempotency_key, body_hash).await? {
        return Ok(outcome);
    }
    let end = remember_end(tx, scope).await?;
    let object_id = random_hex(16)?;
    let mut nonce = [0_u8; 12];
    nonce.copy_from_slice(&random_bytes(12)?);
    let created = now_ms()?;
    let expires = created.checked_add(TTL_MS).ok_or_else(Fault::budget)?;
    let aad_bytes = artifact_aad(deployment, scope_tuple(scope), &object_id, &ready.plaintext_sha256)?;
    let ciphertext = seal(&config.aead_key, &nonce, &aad_bytes, &ready.csv)?;
    let ciphertext_sha = sha256_hex(&ciphertext);
    write_ciphertext(&config.object_dir, &object_id, &ciphertext)?;
    let inserted = tx
        .execute(
            sql::INSERT_ARTIFACT,
            &[
                &object_id,
                &deployment,
                &scope.principal_id,
                &scope.project_id,
                &scope.session_id,
                &scope.role,
                &scope.policy_id,
                &scope.knowledge_id,
                &scope.knowledge_digest,
                &ready.plaintext_sha256,
                &ready.plaintext_size,
                &ready.source_sha256,
                &ready.sidecar_sha256,
                &nonce.as_slice(),
                &ciphertext_sha,
                &created,
                &expires,
                &end,
            ],
        )
        .await;
    if let Err(error) = inserted {
        let _ = remove_object(&config.object_dir, &object_id);
        return Err(map_db(error));
    }
    let body = stored_body(&object_id, ready, expires);
    if let Err(error) = insert_idempotency(tx, &scope.session_id, idempotency_key, body_hash, &body).await {
        let _ = remove_object(&config.object_dir, &object_id);
        return Err(error);
    }
    Ok(Outcome::Created(body))
}

async fn revoke_in(
    tx: &Transaction<'_>,
    deployment: &str,
    scope: &Scope,
    object_id: &str,
    idempotency_key: &str,
    body_hash: &str,
) -> Result<Outcome, Fault> {
    if let Some(outcome) = replay(tx, &scope.session_id, idempotency_key, body_hash).await? {
        return Ok(outcome);
    }
    let row = tx.query_opt(sql::READ_ARTIFACT, &[&deployment, &object_id]).await.map_err(|_| Fault::not_ready())?;
    let row = row.ok_or_else(Fault::not_found)?;
    authorize(scope, &row, now_ms()?, now_unix()?)?;
    let state: String = row.get(16);
    if state == "active" {
        tx.execute(sql::REVOKE, &[&deployment, &object_id]).await.map_err(|_| Fault::not_ready())?;
    }
    let body = json!({
        "contract": CONTRACT,
        "disposition": "revoked",
        "object_id": object_id,
        "replayed": false,
        "signed_url": null,
        "workspace": "disabled",
        "production_signed": false,
        "runsc": "not_claimed",
        "customer_integration": "not_claimed",
        "parent_complete": false
    });
    insert_idempotency(tx, &scope.session_id, idempotency_key, body_hash, &body).await?;
    Ok(Outcome::Created(body))
}

async fn replay(tx: &Transaction<'_>, session_id: &str, key: &str, body_hash: &str) -> Result<Option<Outcome>, Fault> {
    let row = tx.query_opt(sql::READ_IDEMPOTENCY, &[&session_id, &key]).await.map_err(|_| Fault::not_ready())?;
    let Some(row) = row else {
        return Ok(None);
    };
    let stored_hash: String = row.get(0);
    if stored_hash != body_hash {
        return Err(Fault::idempotency_conflict());
    }
    let text: String = row.get(1);
    let mut value: Value = serde_json::from_str(&text).map_err(|_| Fault::not_ready())?;
    if let Some(object) = value.as_object_mut() {
        object.insert("replayed".to_owned(), json!(true));
    }
    if value.get("code").and_then(Value::as_str) == Some("artifact_loss") {
        Ok(Some(Outcome::Loss(value)))
    } else {
        Ok(Some(Outcome::Replay(value)))
    }
}

async fn remember_end(tx: &Transaction<'_>, scope: &Scope) -> Result<i64, Fault> {
    let row = tx.query_opt(sql::LOCK_ANCHOR, &[&scope.session_id]).await.map_err(|_| Fault::not_ready())?;
    if let Some(row) = row {
        let stored: i64 = row.get(0);
        if scope.absolute_end_unix > stored {
            return Err(Fault::session_extended());
        }
        return Ok(stored);
    }
    tx.execute(sql::INSERT_ANCHOR, &[&scope.session_id, &scope.principal_id, &scope.project_id, &scope.absolute_end_unix])
        .await
        .map_err(|_| Fault::not_ready())?;
    Ok(scope.absolute_end_unix)
}

async fn insert_idempotency(tx: &Transaction<'_>, session_id: &str, key: &str, body_hash: &str, body: &Value) -> Result<(), Fault> {
    let text = serde_json::to_string(body).map_err(|_| Fault::not_ready())?;
    tx.execute(sql::INSERT_IDEMPOTENCY, &[&session_id, &key, &body_hash, &text]).await.map_err(|_| Fault::not_ready())?;
    Ok(())
}

fn stored_body(object_id: &str, ready: &ReadyExport, expires: i64) -> Value {
    let mut body = json!({
        "contract": CONTRACT,
        "disposition": "stored",
        "object_id": object_id,
        "media_type": "text/csv",
        "plaintext_sha256": ready.plaintext_sha256,
        "plaintext_size": ready.plaintext_size,
        "source_sha256": ready.source_sha256,
        "sidecar_sha256": ready.sidecar_sha256,
        "complete": true,
        "expires_unix_ms": expires,
        "session_extended": false,
        "replayed": false
    });
    merge_flags(&mut body);
    body
}

fn loss_body(reason: &str) -> Value {
    let mut body = json!({
        "contract": CONTRACT,
        "disposition": "artifact_loss",
        "code": "artifact_loss",
        "reason": reason,
        "session_extended": false,
        "replayed": false
    });
    merge_flags(&mut body);
    body
}

fn manifest_value(object_id: &str, plaintext_sha: &str, size: i64, source_sha: &str, expires: i64, state: &str) -> Value {
    let mut body = json!({
        "object_id": object_id,
        "media_type": "text/csv",
        "plaintext_sha256": plaintext_sha,
        "plaintext_size": size,
        "source_sha256": source_sha,
        "complete": true,
        "expires_unix_ms": expires,
        "state": state
    });
    merge_flags(&mut body);
    body
}

fn merge_flags(body: &mut Value) {
    if let (Some(object), Some(flags)) = (body.as_object_mut(), card_flags().as_object()) {
        for (key, value) in flags {
            object.insert(key.clone(), value.clone());
        }
    }
}

fn authorize(scope: &Scope, row: &tokio_postgres::Row, now_ms: i64, now_unix: i64) -> Result<(), Fault> {
    let principal: String = row.get(0);
    let project: String = row.get(1);
    let session: String = row.get(2);
    let role: String = row.get(3);
    let policy: String = row.get(4);
    let knowledge_id: String = row.get(5);
    let knowledge_digest: String = row.get(6);
    let expires: i64 = row.get(14);
    let end: i64 = row.get(15);
    let state: String = row.get(16);
    if principal != scope.principal_id || project != scope.project_id || session != scope.session_id {
        return Err(Fault::forbidden());
    }
    if role != scope.role || policy != scope.policy_id || knowledge_id != scope.knowledge_id || knowledge_digest != scope.knowledge_digest {
        return Err(Fault::forbidden());
    }
    if state == "revoked" {
        return Err(Fault::revoked());
    }
    if state == "expired" || now_ms >= expires || now_unix >= end || now_unix >= scope.absolute_end_unix {
        return Err(Fault::expired());
    }
    if state != "active" {
        return Err(Fault::forbidden());
    }
    Ok(())
}

pub fn now_ms() -> Result<i64, Fault> {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| Fault::not_ready())?.as_millis();
    i64::try_from(millis).map_err(|_| Fault::not_ready())
}

pub fn now_unix() -> Result<i64, Fault> {
    Ok(now_ms()? / 1000)
}

async fn finish<T>(tx: Transaction<'_>, result: Result<T, Fault>) -> Result<T, Fault> {
    match result {
        Ok(value) => {
            tx.commit().await.map_err(|_| Fault::commit_unknown())?;
            Ok(value)
        }
        Err(fault) => {
            let _ = tx.rollback().await;
            Err(fault)
        }
    }
}

fn map_db(error: tokio_postgres::Error) -> Fault {
    match error.code() {
        Some(code) if code == &SqlState::INSUFFICIENT_PRIVILEGE => Fault::session_extended(),
        _ => Fault::not_ready(),
    }
}
