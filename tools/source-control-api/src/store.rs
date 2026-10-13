//! PostgreSQL state for sources, objects, operations, and redacted documents.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, Row, Transaction};

use crate::config::{self, Config};
use crate::error::Fault;
use crate::ids::token_detail;
use crate::sha256::sha256_hex;
use crate::tls::pg_tls;

const MIGRATION: &str = include_str!("../migrations/oo_source_control_v1.sql");

#[derive(Clone, Debug)]
pub struct SourceRow {
    pub source_ref: String,
    pub project_id: String,
    pub kind: String,
    pub revision: i64,
    pub state: String,
    pub secret_ref: Option<String>,
    pub host: Option<String>,
    pub port: Option<i32>,
    pub database_name: Option<String>,
    pub username: Option<String>,
    pub tables_json: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ImportRow {
    pub operation_id: String,
    pub project_id: String,
    pub source_ref: String,
    pub kind: String,
    pub status: String,
    pub attempt: i32,
    pub idempotency_key: String,
    pub body_sha256: String,
    pub input_json: String,
    pub producer_code: Option<String>,
    pub producer_detail: Option<String>,
    pub rows_copied: Option<i64>,
    pub child_started: bool,
    pub catalog_sha256: Option<String>,
    pub profile_sha256: Option<String>,
    pub cancel_requested: bool,
    pub pid: Option<i32>,
}

#[derive(Clone, Debug)]
pub struct ObjectRow {
    pub source_ref: String,
    pub sha256: String,
    pub byte_len: i64,
    pub stored_name: String,
}

#[derive(Clone, Debug)]
pub struct ExchangeDocs {
    pub catalog: String,
    pub profile: String,
    pub catalog_sha256: String,
    pub profile_sha256: String,
}

pub struct NewSource {
    pub source_ref: String,
    pub project_id: String,
    pub kind: String,
    pub secret_ref: Option<String>,
    pub host: Option<String>,
    pub port: Option<i32>,
    pub database_name: Option<String>,
    pub username: Option<String>,
    pub tables_json: Option<String>,
    pub idempotency_key: String,
    pub body_sha256: String,
    pub subject: String,
}

pub struct NewObject {
    pub source_ref: String,
    pub project_id: String,
    pub sha256: String,
    pub byte_len: i64,
    pub stored_name: String,
    pub idempotency_key: String,
    pub body_sha256: String,
    pub subject: String,
}

pub struct NewImport {
    pub operation_id: String,
    pub project_id: String,
    pub source_ref: String,
    pub kind: String,
    pub idempotency_key: String,
    pub body_sha256: String,
    pub identity_revision: i64,
    pub subject: String,
    pub expected_revision: i64,
    pub input_json: String,
}

pub struct Finish {
    pub operation_id: String,
    pub success: bool,
    pub producer_code: Option<String>,
    pub producer_detail: Option<String>,
    pub rows: Option<i64>,
    pub child_started: bool,
    pub catalog: Option<String>,
    pub profile: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Apply {
    Created,
    Replayed,
}

pub enum CancelApply {
    Done,
    Signal { pid: Option<i32> },
}

#[derive(Debug)]
pub enum StoreError {
    Unavailable,
    Conflict(&'static str),
    Idempotency,
    Revision,
    NotVisible,
}

pub enum Install {
    Committed,
    Replayed,
}

#[derive(Clone)]
pub struct Store {
    client: Arc<Mutex<Client>>,
}

// One connection. The guard is the transaction lock and stays held across await.
#[allow(clippy::await_holding_lock)]
impl Store {
    pub async fn connect(config: &Config) -> Result<Self, Fault> {
        let password = config::load_secret(&config.pg.password_file)?;
        let password = String::from_utf8(password).map_err(|_| Fault::Refused)?;
        let mut pg = tokio_postgres::Config::new();
        pg.user(&config.pg.user);
        pg.password(password);
        pg.host(&config.pg.host);
        pg.port(config.pg.port);
        pg.dbname(&config.pg.database);
        pg.application_name("oo-source-control-api");
        pg.ssl_mode(tokio_postgres::config::SslMode::Require);
        pg.connect_timeout(Duration::from_secs(10));
        let ca = std::fs::read(&config.pg.ca_file).map_err(|_| Fault::Refused)?;
        let tls = pg_tls(&ca)?;
        let (client, connection) = pg.connect(tls).await.map_err(|_| Fault::Refused)?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        client.batch_execute("SET TIME ZONE 'UTC'").await.map_err(|_| Fault::Refused)?;
        Ok(Self { client: Arc::new(Mutex::new(client)) })
    }

    pub async fn install(&self) -> Result<Install, Fault> {
        let hash = sha256_hex(MIGRATION.as_bytes());
        let mut client = self.client.lock().await;
        let tx = client.transaction().await.map_err(|_| Fault::Refused)?;
        tx.batch_execute("SET LOCAL synchronous_commit = on").await.map_err(|_| Fault::Refused)?;
        let exists = tx
            .query_opt("SELECT 1 FROM pg_namespace WHERE nspname = 'oo_source_control'", &[])
            .await
            .map_err(|_| Fault::Refused)?;
        if exists.is_none() {
            tx.batch_execute(MIGRATION).await.map_err(|_| Fault::Refused)?;
            tx.execute(
                "INSERT INTO oo_source_control.install_manifest (singleton, sql_sha256) VALUES (true, $1)",
                &[&hash],
            )
            .await
            .map_err(|_| Fault::Refused)?;
            tx.commit().await.map_err(|_| Fault::Commit)?;
            return Ok(Install::Committed);
        }
        let row = tx
            .query_one("SELECT sql_sha256 FROM oo_source_control.install_manifest", &[])
            .await
            .map_err(|_| Fault::Refused)?;
        let stored: String = row.get(0);
        if stored != hash {
            return Err(Fault::Refused);
        }
        tx.commit().await.map_err(|_| Fault::Commit)?;
        Ok(Install::Replayed)
    }

    pub async fn require_installed(&self) -> Result<(), Fault> {
        let hash = sha256_hex(MIGRATION.as_bytes());
        let client = self.client.lock().await;
        let row = client.query_opt("SELECT sql_sha256 FROM oo_source_control.install_manifest", &[])
            .await
            .map_err(|_| Fault::Refused)?;
        let Some(row) = row else {
            return Err(Fault::Refused);
        };
        let stored: String = row.get(0);
        if stored == hash {
            Ok(())
        } else {
            Err(Fault::Refused)
        }
    }

    pub async fn recover_on_start(&self) -> Result<(), Fault> {
        let client = self.client.lock().await;
        let rows = client
            .query(
                "SELECT pid FROM oo_source_control.operation WHERE status IN ('running', 'cancel_requested') AND pid IS NOT NULL",
                &[],
            )
            .await
            .map_err(|_| Fault::Refused)?;
        for row in rows {
            let pid: i32 = row.get(0);
            crate::signal::signal_tree(pid);
        }
        client
            .execute(
                "UPDATE oo_source_control.operation SET status = 'resume_uncertain', pid = NULL, updated_at = now() WHERE status IN ('running', 'cancel_requested')",
                &[],
            )
            .await
            .map_err(|_| Fault::Refused)?;
        Ok(())
    }

    pub async fn source(&self, source_ref: &str) -> Result<Option<SourceRow>, StoreError> {
        let client = self.client.lock().await;
        let row = client.query_opt(
                "SELECT source_ref, project_id, kind, revision, state, secret_ref, host, port, database_name, username, tables_json FROM oo_source_control.source WHERE source_ref = $1",
                &[&source_ref],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(row.as_ref().map(source_row))
    }

    pub async fn list_sources(&self, project_id: &str) -> Result<Vec<SourceRow>, StoreError> {
        let client = self.client.lock().await;
        let rows = client.query(
                "SELECT source_ref, project_id, kind, revision, state, secret_ref, host, port, database_name, username, tables_json FROM oo_source_control.source WHERE project_id = $1 ORDER BY source_ref LIMIT 100",
                &[&project_id],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(rows.iter().map(source_row).collect())
    }

    pub async fn object(&self, source_ref: &str, sha256: &str) -> Result<Option<ObjectRow>, StoreError> {
        let client = self.client.lock().await;
        let row = client.query_opt(
                "SELECT source_ref, sha256, byte_len, stored_name FROM oo_source_control.object_blob WHERE source_ref = $1 AND sha256 = $2",
                &[&source_ref, &sha256],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(row.as_ref().map(object_row))
    }

    pub async fn operation(&self, operation_id: &str) -> Result<Option<ImportRow>, StoreError> {
        let client = self.client.lock().await;
        let row = client.query_opt(&format!("SELECT {IMPORT_COLUMNS} FROM oo_source_control.operation WHERE operation_id = $1"), &[&operation_id])
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(row.as_ref().map(import_row))
    }

    pub async fn list_operations(&self, project_id: &str) -> Result<Vec<ImportRow>, StoreError> {
        let client = self.client.lock().await;
        let rows = client.query(
                &format!("SELECT {IMPORT_COLUMNS} FROM oo_source_control.operation WHERE project_id = $1 ORDER BY created_at LIMIT 100"),
                &[&project_id],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(rows.iter().map(import_row).collect())
    }

    pub async fn register(&self, source: &NewSource) -> Result<(SourceRow, Apply), StoreError> {
        if let Some(row) = self.idempotency(&source.project_id, &source.idempotency_key).await? {
            return self.replay_source(row, &source.body_sha256).await;
        }
        let mut client = self.client.lock().await;
        let mut tx = begin(&mut client).await?;
        if let Some(row) = idempotency_tx(&mut tx, &source.project_id, &source.idempotency_key).await? {
            let body: String = row.get(0);
            let resource: String = row.get(1);
            tx.commit().await.map_err(|_| StoreError::Unavailable)?;
            drop(client);
            return self.replay_source((body, resource), &source.body_sha256).await;
        }
        let inserted = tx
            .execute(
                "INSERT INTO oo_source_control.source (source_ref, project_id, kind, revision, state, secret_ref, host, port, database_name, username, tables_json) VALUES ($1,$2,$3,1,'registered',$4,$5,$6,$7,$8,$9)",
                &[
                    &source.source_ref,
                    &source.project_id,
                    &source.kind,
                    &source.secret_ref,
                    &source.host,
                    &source.port,
                    &source.database_name,
                    &source.username,
                    &source.tables_json,
                ],
            )
            .await;
        if let Err(error) = inserted {
            let conflict = unique(&error);
            drop(tx);
            drop(client);
            if !conflict {
                return Err(StoreError::Unavailable);
            }
            if let Some(row) = self.idempotency(&source.project_id, &source.idempotency_key).await? {
                return self.replay_source(row, &source.body_sha256).await;
            }
            return Err(StoreError::Conflict("source_ref"));
        }
        insert_idempotency(&mut tx, &source.project_id, &source.idempotency_key, &source.body_sha256, &source.source_ref).await?;
        audit(&mut tx, &source.subject, &source.project_id, "registerSource", &source.source_ref, "created").await?;
        tx.commit().await.map_err(|_| StoreError::Unavailable)?;
        drop(client);
        let row = self.source(&source.source_ref).await?.ok_or(StoreError::Unavailable)?;
        Ok((row, Apply::Created))
    }

    pub async fn upload(&self, object: &NewObject) -> Result<(ObjectRow, Apply), StoreError> {
        if let Some(row) = self.idempotency(&object.project_id, &object.idempotency_key).await? {
            return self.replay_object(row, &object.body_sha256, &object.source_ref).await;
        }
        let mut client = self.client.lock().await;
        let mut tx = begin(&mut client).await?;
        let source = tx
            .query_opt(
                "SELECT project_id, kind FROM oo_source_control.source WHERE source_ref = $1 FOR UPDATE",
                &[&object.source_ref],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
        let Some(source) = source else {
            return Err(StoreError::NotVisible);
        };
        let project: String = source.get(0);
        let kind: String = source.get(1);
        if project != object.project_id {
            return Err(StoreError::NotVisible);
        }
        if kind != "file_local" {
            return Err(StoreError::Conflict("kind"));
        }
        if let Some(row) = idempotency_tx(&mut tx, &object.project_id, &object.idempotency_key).await? {
            let body: String = row.get(0);
            let resource: String = row.get(1);
            tx.commit().await.map_err(|_| StoreError::Unavailable)?;
            drop(client);
            return self.replay_object((body, resource), &object.body_sha256, &object.source_ref).await;
        }
        let inserted = tx
            .execute(
                "INSERT INTO oo_source_control.object_blob (source_ref, sha256, byte_len, stored_name) VALUES ($1,$2,$3,$4)",
                &[&object.source_ref, &object.sha256, &object.byte_len, &object.stored_name],
            )
            .await;
        if let Err(error) = inserted {
            let conflict = unique(&error);
            drop(tx);
            drop(client);
            if !conflict {
                return Err(StoreError::Unavailable);
            }
            if let Some(idem) = self.idempotency(&object.project_id, &object.idempotency_key).await? {
                return self.replay_object(idem, &object.body_sha256, &object.source_ref).await;
            }
            return Err(StoreError::Conflict("object"));
        }
        insert_idempotency(&mut tx, &object.project_id, &object.idempotency_key, &object.body_sha256, &object.sha256).await?;
        audit(&mut tx, &object.subject, &object.project_id, "uploadSourceObject", &object.source_ref, "created").await?;
        tx.commit().await.map_err(|_| StoreError::Unavailable)?;
        drop(client);
        let row = self.object(&object.source_ref, &object.sha256).await?.ok_or(StoreError::Unavailable)?;
        Ok((row, Apply::Created))
    }

    pub async fn create_import(&self, import: &NewImport) -> Result<(ImportRow, Apply), StoreError> {
        if let Some(row) = self.idempotency(&import.project_id, &import.idempotency_key).await? {
            return self.replay_import(row, &import.body_sha256).await;
        }
        let mut client = self.client.lock().await;
        let mut tx = begin(&mut client).await?;
        if let Some(row) = idempotency_tx(&mut tx, &import.project_id, &import.idempotency_key).await? {
            let body: String = row.get(0);
            let resource: String = row.get(1);
            tx.commit().await.map_err(|_| StoreError::Unavailable)?;
            drop(client);
            return self.replay_import((body, resource), &import.body_sha256).await;
        }
        let source = tx
            .query_opt(
                "SELECT revision, kind FROM oo_source_control.source WHERE source_ref = $1 AND project_id = $2 FOR UPDATE",
                &[&import.source_ref, &import.project_id],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
        let Some(source) = source else {
            return Err(StoreError::NotVisible);
        };
        let revision: i64 = source.get(0);
        let kind: String = source.get(1);
        if revision != import.expected_revision {
            return Err(StoreError::Revision);
        }
        let expected_kind = if import.kind == "pg_inspect" { "pg_local" } else { "file_local" };
        if kind != expected_kind {
            return Err(StoreError::Conflict("kind"));
        }
        let updated = tx
            .execute(
                "UPDATE oo_source_control.source SET revision = revision + 1, state = CASE WHEN state = 'awaiting_review' THEN 'awaiting_review' ELSE 'importing' END WHERE source_ref = $1 AND project_id = $2 AND revision = $3",
                &[&import.source_ref, &import.project_id, &import.expected_revision],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
        if updated != 1 {
            return Err(StoreError::Revision);
        }
        let inserted = tx
            .execute(
                "INSERT INTO oo_source_control.operation (operation_id, project_id, source_ref, kind, status, attempt, idempotency_key, body_sha256, identity_revision, subject, input_json, child_started, cancel_requested) VALUES ($1,$2,$3,$4,'queued',1,$5,$6,$7,$8,$9,false,false)",
                &[
                    &import.operation_id,
                    &import.project_id,
                    &import.source_ref,
                    &import.kind,
                    &import.idempotency_key,
                    &import.body_sha256,
                    &import.identity_revision,
                    &import.subject,
                    &import.input_json,
                ],
            )
            .await;
        if let Err(error) = inserted {
            let conflict = unique(&error);
            drop(tx);
            drop(client);
            if !conflict {
                return Err(StoreError::Unavailable);
            }
            if let Some(row) = self.idempotency(&import.project_id, &import.idempotency_key).await? {
                return self.replay_import(row, &import.body_sha256).await;
            }
            return Err(StoreError::Conflict("operation"));
        }
        insert_idempotency(&mut tx, &import.project_id, &import.idempotency_key, &import.body_sha256, &import.operation_id).await?;
        audit(&mut tx, &import.subject, &import.project_id, "createImport", &import.operation_id, "queued").await?;
        tx.commit().await.map_err(|_| StoreError::Unavailable)?;
        drop(client);
        let row = self.operation(&import.operation_id).await?.ok_or(StoreError::Unavailable)?;
        Ok((row, Apply::Created))
    }

    pub async fn cancel(&self, project_id: &str, operation_id: &str, key: &str, body_sha256: &str, subject: &str) -> Result<(ImportRow, Apply, CancelApply), StoreError> {
        if let Some(row) = self.idempotency(project_id, key).await? {
            let (import_row, apply) = self.replay_import(row, body_sha256).await?;
            return Ok((import_row, apply, CancelApply::Done));
        }
        let mut client = self.client.lock().await;
        let mut tx = begin(&mut client).await?;
        let row = lock_operation(&mut tx, operation_id).await?;
        let Some(current) = row else {
            return Err(StoreError::NotVisible);
        };
        if current.project_id != project_id {
            return Err(StoreError::NotVisible);
        }
        if let Some(existing) = idempotency_tx(&mut tx, project_id, key).await? {
            let body: String = existing.get(0);
            let resource: String = existing.get(1);
            tx.commit().await.map_err(|_| StoreError::Unavailable)?;
            drop(client);
            let (import_row, apply) = self.replay_import((body, resource), body_sha256).await?;
            return Ok((import_row, apply, CancelApply::Done));
        }
        let signal = match current.status.as_str() {
            "queued" => {
                tx.execute(
                    "UPDATE oo_source_control.operation SET status = 'canceled', updated_at = now() WHERE operation_id = $1 AND status = 'queued'",
                    &[&operation_id],
                )
                .await
                .map_err(|_| StoreError::Unavailable)?;
                tx.execute(
                    "UPDATE oo_source_control.source SET state = 'canceled' WHERE source_ref = $1 AND project_id = $2 AND state <> 'awaiting_review'",
                    &[&current.source_ref, &current.project_id],
                )
                .await
                .map_err(|_| StoreError::Unavailable)?;
                None
            }
            "running" => {
                tx.execute(
                    "UPDATE oo_source_control.operation SET status = 'cancel_requested', cancel_requested = true, updated_at = now() WHERE operation_id = $1 AND status = 'running'",
                    &[&operation_id],
                )
                .await
                .map_err(|_| StoreError::Unavailable)?;
                Some(current.pid)
            }
            _ => return Err(StoreError::Conflict("state")),
        };
        insert_idempotency(&mut tx, project_id, key, body_sha256, operation_id).await?;
        audit(&mut tx, subject, project_id, "cancelImport", operation_id, "cancel").await?;
        tx.commit().await.map_err(|_| StoreError::Unavailable)?;
        drop(client);
        let updated = self.operation(operation_id).await?.ok_or(StoreError::Unavailable)?;
        let apply = match signal {
            Some(pid) => CancelApply::Signal { pid },
            None => CancelApply::Done,
        };
        Ok((updated, Apply::Created, apply))
    }

    pub async fn restart(&self, project_id: &str, operation_id: &str, key: &str, body_sha256: &str, subject: &str) -> Result<(ImportRow, Apply), StoreError> {
        if let Some(row) = self.idempotency(project_id, key).await? {
            return self.replay_import(row, body_sha256).await;
        }
        let mut client = self.client.lock().await;
        let mut tx = begin(&mut client).await?;
        let row = lock_operation(&mut tx, operation_id).await?;
        let Some(current) = row else {
            return Err(StoreError::NotVisible);
        };
        if current.project_id != project_id {
            return Err(StoreError::NotVisible);
        }
        if !matches!(current.status.as_str(), "failed" | "canceled" | "resume_uncertain") {
            return Err(StoreError::Conflict("state"));
        }
        tx.execute("DELETE FROM oo_source_control.document WHERE operation_id = $1", &[&operation_id])
            .await
            .map_err(|_| StoreError::Unavailable)?;
        let updated = tx
            .execute(
                "UPDATE oo_source_control.operation SET status = 'queued', attempt = attempt + 1, producer_code = NULL, producer_detail = NULL, rows_copied = NULL, child_started = false, catalog_sha256 = NULL, profile_sha256 = NULL, cancel_requested = false, pid = NULL, updated_at = now() WHERE operation_id = $1 AND status IN ('failed', 'canceled', 'resume_uncertain')",
                &[&operation_id],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
        if updated != 1 {
            return Err(StoreError::Conflict("state"));
        }
        tx.execute(
            "UPDATE oo_source_control.source SET state = 'importing' WHERE source_ref = $1 AND project_id = $2 AND state <> 'awaiting_review'",
            &[&current.source_ref, &current.project_id],
        )
        .await
        .map_err(|_| StoreError::Unavailable)?;
        insert_idempotency(&mut tx, project_id, key, body_sha256, operation_id).await?;
        audit(&mut tx, subject, project_id, "restartImport", operation_id, "queued").await?;
        tx.commit().await.map_err(|_| StoreError::Unavailable)?;
        drop(client);
        let row = self.operation(operation_id).await?.ok_or(StoreError::Unavailable)?;
        Ok((row, Apply::Created))
    }

    pub async fn claim_next(&self) -> Result<Option<ImportRow>, StoreError> {
        let client = self.client.lock().await;
        let row = client.query_opt(
                &format!(
                    "UPDATE oo_source_control.operation SET status = 'running', updated_at = now()
                     WHERE operation_id = (
                        SELECT operation_id FROM oo_source_control.operation
                        WHERE status = 'queued'
                          AND project_id NOT IN (
                            SELECT project_id FROM oo_source_control.operation WHERE status IN ('running', 'cancel_requested')
                          )
                        ORDER BY created_at
                        LIMIT 1
                        FOR UPDATE SKIP LOCKED
                     )
                     RETURNING {IMPORT_COLUMNS}"
                ),
                &[],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(row.as_ref().map(import_row))
    }

    pub async fn set_pid(&self, operation_id: &str, pid: i32) -> Result<(), StoreError> {
        let client = self.client.lock().await;
        client
            .execute(
                "UPDATE oo_source_control.operation SET pid = $2, updated_at = now() WHERE operation_id = $1 AND status IN ('running', 'cancel_requested')",
                &[&operation_id, &pid],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(())
    }

    pub async fn mark_started(&self, operation_id: &str) -> Result<(), StoreError> {
        let client = self.client.lock().await;
        client
            .execute(
                "UPDATE oo_source_control.operation SET child_started = true, updated_at = now() WHERE operation_id = $1",
                &[&operation_id],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(())
    }

    pub async fn finish(&self, finish: &Finish) -> Result<(), StoreError> {
        let mut client = self.client.lock().await;
        let mut tx = begin(&mut client).await?;
        let Some(current) = lock_operation(&mut tx, &finish.operation_id).await? else {
            return Err(StoreError::Unavailable);
        };
        if !matches!(current.status.as_str(), "running" | "cancel_requested") {
            tx.commit().await.map_err(|_| StoreError::Unavailable)?;
            return Ok(());
        }
        let canceled = current.cancel_requested || current.status == "cancel_requested";
        let status = if canceled {
            "canceled"
        } else if finish.success {
            "awaiting_review"
        } else {
            "failed"
        };
        let detail = match finish.producer_detail.as_deref() {
            Some(value) if token_detail(value) => Some(value.to_string()),
            Some(_) => Some("redacted".to_string()),
            None => None,
        };
        let code = match finish.producer_code.as_deref() {
            Some(value) if token_detail(value) => Some(value.to_string()),
            Some(_) => Some("producer".to_string()),
            None => None,
        };
        let (catalog_sha, profile_sha) = if status == "awaiting_review" {
            let catalog = finish.catalog.as_deref().ok_or(StoreError::Unavailable)?;
            let profile = finish.profile.as_deref().ok_or(StoreError::Unavailable)?;
            let catalog_sha = sha256_hex(catalog.as_bytes());
            let profile_sha = sha256_hex(profile.as_bytes());
            tx.execute(
                "INSERT INTO oo_source_control.document (operation_id, doc_kind, sha256, body) VALUES ($1, 'catalog', $2, $3), ($1, 'profile', $4, $5)",
                &[&finish.operation_id, &catalog_sha, &catalog, &profile_sha, &profile],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
            (Some(catalog_sha), Some(profile_sha))
        } else {
            (None, None)
        };
        tx.execute(
            "UPDATE oo_source_control.operation SET status = $2, producer_code = $3, producer_detail = $4, rows_copied = $5, child_started = $6, catalog_sha256 = $7, profile_sha256 = $8, pid = NULL, updated_at = now() WHERE operation_id = $1",
            &[&finish.operation_id, &status, &code, &detail, &finish.rows, &finish.child_started, &catalog_sha, &profile_sha],
        )
        .await
        .map_err(|_| StoreError::Unavailable)?;
        if status == "awaiting_review" {
            tx.execute(
                "UPDATE oo_source_control.source SET state = 'awaiting_review' WHERE source_ref = $1 AND project_id = $2",
                &[&current.source_ref, &current.project_id],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
        } else {
            tx.execute(
                "UPDATE oo_source_control.source SET state = $3 WHERE source_ref = $1 AND project_id = $2 AND state <> 'awaiting_review'",
                &[&current.source_ref, &current.project_id, &status],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
        }
        audit(&mut tx, "worker", &current.project_id, "finishImport", &finish.operation_id, status).await?;
        tx.commit().await.map_err(|_| StoreError::Unavailable)?;
        Ok(())
    }

    pub async fn exchange(&self, project_id: &str, source_ref: &str) -> Result<Option<ExchangeDocs>, StoreError> {
        let client = self.client.lock().await;
        let row = client.query_opt(
                "SELECT operation_id, catalog_sha256, profile_sha256 FROM oo_source_control.operation WHERE project_id = $1 AND source_ref = $2 AND status = 'awaiting_review' AND catalog_sha256 IS NOT NULL AND profile_sha256 IS NOT NULL ORDER BY updated_at DESC LIMIT 1",
                &[&project_id, &source_ref],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let operation_id: String = row.get(0);
        let catalog_sha: String = row.get(1);
        let profile_sha: String = row.get(2);
        let docs = client.query(
                "SELECT doc_kind, sha256, body FROM oo_source_control.document WHERE operation_id = $1 AND doc_kind IN ('catalog', 'profile')",
                &[&operation_id],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
        let mut catalog = None;
        let mut profile = None;
        for doc in docs {
            let kind: String = doc.get(0);
            let sha: String = doc.get(1);
            let body: String = doc.get(2);
            if sha256_hex(body.as_bytes()) != sha {
                return Err(StoreError::Unavailable);
            }
            if kind == "catalog" && sha == catalog_sha {
                catalog = Some(body);
            } else if kind == "profile" && sha == profile_sha {
                profile = Some(body);
            }
        }
        match (catalog, profile) {
            (Some(catalog), Some(profile)) => Ok(Some(ExchangeDocs { catalog, profile, catalog_sha256: catalog_sha, profile_sha256: profile_sha })),
            _ => Err(StoreError::Unavailable),
        }
    }

    async fn idempotency(&self, project_id: &str, key: &str) -> Result<Option<(String, String)>, StoreError> {
        let client = self.client.lock().await;
        let row = client.query_opt(
                "SELECT body_sha256, resource_id FROM oo_source_control.idempotency WHERE project_id = $1 AND idempotency_key = $2",
                &[&project_id, &key],
            )
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(row.map(|row| (row.get(0), row.get(1))))
    }

    async fn replay_source(&self, row: (String, String), body_sha256: &str) -> Result<(SourceRow, Apply), StoreError> {
        if row.0 != body_sha256 {
            return Err(StoreError::Idempotency);
        }
        let source = self.source(&row.1).await?.ok_or(StoreError::Unavailable)?;
        Ok((source, Apply::Replayed))
    }

    async fn replay_object(&self, row: (String, String), body_sha256: &str, source_ref: &str) -> Result<(ObjectRow, Apply), StoreError> {
        if row.0 != body_sha256 {
            return Err(StoreError::Idempotency);
        }
        let object = self.object(source_ref, &row.1).await?.ok_or(StoreError::Unavailable)?;
        Ok((object, Apply::Replayed))
    }

    async fn replay_import(&self, row: (String, String), body_sha256: &str) -> Result<(ImportRow, Apply), StoreError> {
        if row.0 != body_sha256 {
            return Err(StoreError::Idempotency);
        }
        let operation = self.operation(&row.1).await?.ok_or(StoreError::Unavailable)?;
        Ok((operation, Apply::Replayed))
    }

}

const IMPORT_COLUMNS: &str = "operation_id, project_id, source_ref, kind, status, attempt, idempotency_key, body_sha256, input_json, producer_code, producer_detail, rows_copied, child_started, catalog_sha256, profile_sha256, cancel_requested, pid";

fn source_row(row: &Row) -> SourceRow {
    SourceRow {
        source_ref: row.get(0),
        project_id: row.get(1),
        kind: row.get(2),
        revision: row.get(3),
        state: row.get(4),
        secret_ref: row.get(5),
        host: row.get(6),
        port: row.get(7),
        database_name: row.get(8),
        username: row.get(9),
        tables_json: row.get(10),
    }
}

fn object_row(row: &Row) -> ObjectRow {
    ObjectRow { source_ref: row.get(0), sha256: row.get(1), byte_len: row.get(2), stored_name: row.get(3) }
}

fn import_row(row: &Row) -> ImportRow {
    ImportRow {
        operation_id: row.get(0),
        project_id: row.get(1),
        source_ref: row.get(2),
        kind: row.get(3),
        status: row.get(4),
        attempt: row.get(5),
        idempotency_key: row.get(6),
        body_sha256: row.get(7),
        input_json: row.get(8),
        producer_code: row.get(9),
        producer_detail: row.get(10),
        rows_copied: row.get(11),
        child_started: row.get(12),
        catalog_sha256: row.get(13),
        profile_sha256: row.get(14),
        cancel_requested: row.get(15),
        pid: row.get(16),
    }
}

async fn begin(client: &mut Client) -> Result<Transaction<'_>, StoreError> {
    let tx = client.transaction().await.map_err(|_| StoreError::Unavailable)?;
    tx.batch_execute("SET LOCAL synchronous_commit = on").await.map_err(|_| StoreError::Unavailable)?;
    Ok(tx)
}

async fn lock_operation(tx: &mut Transaction<'_>, operation_id: &str) -> Result<Option<ImportRow>, StoreError> {
    let row = tx
        .query_opt(
            &format!("SELECT {IMPORT_COLUMNS} FROM oo_source_control.operation WHERE operation_id = $1 FOR UPDATE"),
            &[&operation_id],
        )
        .await
        .map_err(|_| StoreError::Unavailable)?;
    Ok(row.as_ref().map(import_row))
}

async fn idempotency_tx(tx: &mut Transaction<'_>, project_id: &str, key: &str) -> Result<Option<Row>, StoreError> {
    tx.query_opt(
        "SELECT body_sha256, resource_id FROM oo_source_control.idempotency WHERE project_id = $1 AND idempotency_key = $2",
        &[&project_id, &key],
    )
    .await
    .map_err(|_| StoreError::Unavailable)
}

async fn insert_idempotency(tx: &mut Transaction<'_>, project_id: &str, key: &str, body_sha256: &str, resource_id: &str) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO oo_source_control.idempotency (project_id, idempotency_key, body_sha256, resource_id) VALUES ($1,$2,$3,$4)",
        &[&project_id, &key, &body_sha256, &resource_id],
    )
    .await
    .map_err(|_| StoreError::Unavailable)?;
    Ok(())
}

async fn audit(tx: &mut Transaction<'_>, subject: &str, project_id: &str, action: &str, object_id: &str, outcome: &str) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO oo_source_control.audit (subject, project_id, action, object_id, outcome) VALUES ($1,$2,$3,$4,$5)",
        &[&subject, &project_id, &action, &object_id, &outcome],
    )
    .await
    .map_err(|_| StoreError::Unavailable)?;
    Ok(())
}

fn unique(error: &tokio_postgres::Error) -> bool {
    error.code() == Some(&SqlState::UNIQUE_VIOLATION)
}
