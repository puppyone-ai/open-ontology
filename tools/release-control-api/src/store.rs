//! API schema and the operation queue. The producer owns the pointer.

use std::sync::Arc;
use std::time::Duration;

use release_coordinator::{
    activate, checkpoint, gate_record, install as producer_install, plan, recover, status, CheckpointRequest, Endpoint,
    GateRequest, InstallRequest, PlanRequest, Receipt, RecoverRequest, StageRequest, StoreError, GATE_IDS,
};
use release_coordinator::stage;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio_postgres::config::SslMode;
use tokio_postgres::{Client, Transaction};

use crate::config::{Config, PgEndpoint};
use crate::error::{ApiError, Fault, CONTRACT};
use crate::identity::{self, VerifiedIdentity};
use crate::ids::{detail_ok, hex64, operation_id_ok};
use crate::pack::{self, permission_compatible, Intake, Lineage, LineageFields, Prepared};
use crate::sha256::sha256_hex;
use crate::tls::pg_tls;
use crate::PRODUCER_CONTRACT;

const MIGRATION_SQL: &str = include_str!("../migrations/oo_release_api_v1.sql");
const SCHEMA: &str = "oo_release_api";
const API_ROLE: &str = "oo_release_api_admin";
const READER_ROLE: &str = "oo_release_reader";
const INSTALL_LOCK: i64 = 881_275_392;
const UNIQUE: &str = "23505";
const SET_LOCAL: &str = "\
SET LOCAL search_path TO pg_catalog; \
SET LOCAL lock_timeout = '10s'; \
SET LOCAL statement_timeout = '15s'; \
SET LOCAL synchronous_commit = on";

pub fn migration_sha256() -> String {
    sha256_hex(MIGRATION_SQL.as_bytes())
}

struct Inner {
    config: Arc<Config>,
    api: Mutex<Client>,
    reader: Mutex<Client>,
    authority: Mutex<Option<String>>,
    identity: reqwest::Client,
}

#[derive(Clone)]
pub struct Runtime {
    inner: Arc<Inner>,
}

#[derive(Clone, Debug)]
pub struct Operation {
    pub operation_id: String,
    pub project_id: String,
    pub body_sha256: String,
    pub kind: String,
    pub status: String,
    pub candidate_id: Option<String>,
    pub version: Option<String>,
    pub base_version: Option<String>,
    pub expected_version: Option<String>,
    pub role_name: Option<String>,
    pub scope_name: Option<String>,
    pub config_commit: Option<String>,
    pub source_hash: Option<String>,
    pub pack_digest: Option<String>,
    pub manifest_sha256: Option<String>,
    pub columns_text: String,
    pub policy_id: Option<String>,
    pub policy_version: Option<String>,
    pub knowledge_id: Option<String>,
    pub knowledge_version: Option<String>,
    pub catalog_version: Option<String>,
    pub review_admitted: bool,
    pub producer_outcome: Option<String>,
    pub producer_reason: Option<String>,
    pub certificate_class: String,
    pub gates_json: String,
    pub subject_name: String,
    pub target_operation_id: Option<String>,
}

pub struct Queued {
    pub operation: Operation,
    pub replay: bool,
}

#[derive(Clone, Debug)]
pub struct PointerView {
    pub version: Option<String>,
    pub candidate_id: Option<String>,
    pub source_hash: Option<String>,
    pub pack_digest: Option<String>,
    pub manifest_sha256: Option<String>,
}

struct Finish {
    status: &'static str,
    outcome: Option<String>,
    reason: Option<String>,
    source_hash: Option<String>,
    pack_digest: Option<String>,
    manifest_sha256: Option<String>,
    class: &'static str,
    gates: String,
}

enum Control {
    Finish(Finish),
    Uncertain,
}

pub struct PublishDraft {
    pub identity: VerifiedIdentity,
    pub body_sha256: String,
    pub idempotency_key: String,
    pub config_commit: String,
    pub candidate_id: String,
    pub version: String,
    pub base_version: Option<String>,
    pub expected_version: Option<String>,
    pub scope: String,
    pub prepared: Prepared,
}

pub struct RollbackDraft {
    pub identity: VerifiedIdentity,
    pub body_sha256: String,
    pub idempotency_key: String,
    pub expected_version: String,
    pub target: Operation,
    pub restored: Option<Operation>,
}

struct NewOperation<'a> {
    operation_id: &'a str,
    idempotency_key: &'a str,
    body_sha256: &'a str,
    kind: &'a str,
    candidate_id: Option<String>,
    version: Option<String>,
    base_version: Option<String>,
    expected_version: Option<String>,
    role_name: Option<String>,
    scope_name: Option<String>,
    config_commit: Option<String>,
    columns_text: &'a str,
    policy_id: Option<String>,
    policy_version: Option<String>,
    knowledge_id: Option<String>,
    knowledge_version: Option<String>,
    catalog_version: Option<String>,
    review_admitted: bool,
    subject: &'a str,
    target: Option<String>,
    pack_file_sha: Option<String>,
}

impl Runtime {
    pub async fn connect(config: Arc<Config>) -> Result<Self, Fault> {
        if release_coordinator::CONTRACT != PRODUCER_CONTRACT {
            return Err(Fault::refused("producer"));
        }
        let api = connect_pg(&config.api_pg).await?;
        let reader = connect_pg(&config.reader_pg).await?;
        require_user(&api, API_ROLE).await?;
        require_user(&reader, READER_ROLE).await?;
        require_tls(&api).await?;
        require_tls(&reader).await?;
        let identity = identity::identity_client(&config.identity_ca_pem).map_err(|_| Fault::refused("dependency.not_ready"))?;
        Ok(Self {
            inner: Arc::new(Inner {
                config,
                api: Mutex::new(api),
                reader: Mutex::new(reader),
                authority: Mutex::new(None),
                identity,
            }),
        })
    }

    pub fn project_id(&self) -> &str {
        &self.inner.config.project_id
    }

    pub fn pack_dir(&self) -> &std::path::Path {
        &self.inner.config.pack_dir
    }

    pub fn origins(&self) -> &[String] {
        &self.inner.config.deployment_origins
    }

    pub fn identity_client(&self) -> &reqwest::Client {
        &self.inner.identity
    }

    pub fn identity_origin(&self) -> &str {
        &self.inner.config.identity_origin
    }

    pub async fn boot(&self) -> Result<(), Fault> {
        self.require_schema().await?;
        self.producer_install().await?;
        self.reader_view().await?;
        self.reconcile().await
    }

    pub async fn run_worker(&self) {
        loop {
            match self.claim().await {
                Ok(Some(id)) => {
                    let _ = self.execute(&id).await;
                }
                Ok(None) => tokio::time::sleep(Duration::from_millis(40)).await,
                Err(ApiError { code: "commit.unknown", .. }) => {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
            }
        }
    }

    pub async fn pointer(&self) -> Result<PointerView, ApiError> {
        let mut client = self.inner.reader.lock().await;
        let tx = begin_api(&mut client).await?;
        let row = tx
            .query_opt(
                "SELECT version, candidate_id, source_hash, pack_digest, manifest_sha256, \
                 production_ready_certificate, full_sp08_accepted, public_deployment_claimed, \
                 customer_ownership_claimed, standby_configured \
                 FROM oo_release_control.publication_status",
                &[],
            )
            .await
            .map_err(|_| ApiError::not_ready("producer"))?;
        commit_api(tx).await?;
        let Some(row) = row else {
            return Err(ApiError::not_ready("producer"));
        };
        Ok(PointerView {
            version: row.try_get(0).map_err(|_| ApiError::not_ready("producer"))?,
            candidate_id: row.try_get(1).map_err(|_| ApiError::not_ready("producer"))?,
            source_hash: row.try_get(2).map_err(|_| ApiError::not_ready("producer"))?,
            pack_digest: row.try_get(3).map_err(|_| ApiError::not_ready("producer"))?,
            manifest_sha256: row.try_get(4).map_err(|_| ApiError::not_ready("producer"))?,
        })
    }

    pub async fn enqueue_publish(&self, draft: PublishDraft) -> Result<Queued, ApiError> {
        let operation_id = operation_id(&self.inner.config.project_id, &draft.idempotency_key);
        let prepared = &draft.prepared;
        self.insert_operation(NewOperation {
            operation_id: &operation_id,
            idempotency_key: &draft.idempotency_key,
            body_sha256: &draft.body_sha256,
            kind: "publish",
            candidate_id: Some(draft.candidate_id),
            version: Some(draft.version),
            base_version: draft.base_version,
            expected_version: draft.expected_version,
            role_name: Some(prepared.lineage.role.clone()),
            scope_name: Some(draft.scope),
            config_commit: Some(draft.config_commit.clone()),
            columns_text: &prepared.columns_text,
            policy_id: Some(prepared.lineage.policy_id.clone()),
            policy_version: Some(prepared.lineage.policy_version.clone()),
            knowledge_id: Some(prepared.lineage.knowledge_id.clone()),
            knowledge_version: Some(prepared.lineage.knowledge_version.clone()),
            catalog_version: Some(prepared.lineage.catalog_version.clone()),
            review_admitted: true,
            subject: &draft.identity.subject,
            target: None,
            pack_file_sha: Some(prepared.bytes_sha256.clone()),
        })
        .await
    }

    pub async fn enqueue_rollback(&self, draft: RollbackDraft) -> Result<Queued, ApiError> {
        let operation_id = operation_id(&self.inner.config.project_id, &draft.idempotency_key);
        self.insert_operation(NewOperation {
            operation_id: &operation_id,
            idempotency_key: &draft.idempotency_key,
            body_sha256: &draft.body_sha256,
            kind: "rollback",
            candidate_id: draft.target.candidate_id.clone(),
            version: Some(draft.expected_version),
            base_version: draft.target.base_version.clone(),
            expected_version: draft.target.version.clone(),
            role_name: draft.target.role_name.clone(),
            scope_name: None,
            config_commit: draft.target.config_commit.clone(),
            columns_text: "",
            policy_id: None,
            policy_version: None,
            knowledge_id: None,
            knowledge_version: None,
            catalog_version: None,
            review_admitted: false,
            subject: &draft.identity.subject,
            target: Some(draft.target.operation_id.clone()),
            pack_file_sha: None,
        })
        .await
    }

    pub async fn operation(&self, operation_id: &str) -> Result<Option<Operation>, ApiError> {
        if !operation_id_ok(operation_id) {
            return Ok(None);
        }
        let mut client = self.inner.api.lock().await;
        let tx = begin_api(&mut client).await?;
        let found = load_id(&tx, operation_id).await?;
        commit_api(tx).await?;
        Ok(found)
    }

    pub async fn list_project(&self) -> Result<Vec<Operation>, ApiError> {
        let mut client = self.inner.api.lock().await;
        let tx = begin_api(&mut client).await?;
        let rows = tx
            .query(
                &format!("{SELECT_OP} WHERE project_id = $1 ORDER BY operation_id"),
                &[&self.inner.config.project_id],
            )
            .await
            .map_err(|_| ApiError::not_ready("postgres"))?;
        let mut out = Vec::new();
        for row in &rows {
            out.push(read_op(row).map_err(|_| ApiError::not_ready("postgres"))?);
        }
        commit_api(tx).await?;
        Ok(out)
    }

    pub async fn succeeded_publish(&self, version: &str) -> Result<Option<Operation>, ApiError> {
        let mut client = self.inner.api.lock().await;
        let tx = begin_api(&mut client).await?;
        let row = tx
            .query_opt(
                &format!(
                    "{SELECT_OP} WHERE project_id = $1 AND kind = 'publish' AND status = 'succeeded' AND version = $2 ORDER BY operation_id DESC LIMIT 1"
                ),
                &[&self.inner.config.project_id, &version],
            )
            .await
            .map_err(|_| ApiError::not_ready("postgres"))?;
        let found = match row {
            Some(row) => Some(read_op(&row).map_err(|_| ApiError::not_ready("postgres"))?),
            None => None,
        };
        commit_api(tx).await?;
        Ok(found)
    }

    pub async fn role_status(&self, operation: &Operation) -> Result<Receipt, ApiError> {
        let candidate = operation.candidate_id.as_deref().ok_or(ApiError::conflict("inactive"))?;
        let source = operation.source_hash.as_deref().ok_or(ApiError::conflict("inactive"))?;
        let endpoint = self.endpoint();
        let config = &self.inner.config;
        let request = PlanRequest {
            endpoint: &endpoint,
            user: &config.producer_pg.user,
            password: &config.producer_pg.password,
            ca_pem: &config.producer_pg.ca_pem,
            project_id: &config.project_id,
            candidate_id: candidate,
            source_hash: source,
            scope: "test_scope",
            expected_version: operation.base_version.as_deref(),
        };
        status(&request).await.map_err(|_| ApiError::not_ready("producer"))
    }

    async fn insert_operation(&self, row: NewOperation<'_>) -> Result<Queued, ApiError> {
        let NewOperation {
            operation_id,
            idempotency_key,
            body_sha256,
            kind,
            candidate_id,
            version,
            base_version,
            expected_version,
            role_name,
            scope_name,
            config_commit,
            columns_text,
            policy_id,
            policy_version,
            knowledge_id,
            knowledge_version,
            catalog_version,
            review_admitted,
            subject,
            target,
            pack_file_sha,
        } = row;
        let project = self.inner.config.project_id.clone();
        let mut client = self.inner.api.lock().await;
        let tx = begin_api(&mut client).await?;
        if let Some(existing) = load_key(&tx, &project, operation_id).await? {
            commit_api(tx).await?;
            return replay_or_conflict(existing, body_sha256);
        }
        let inserted = tx
            .execute(
                "INSERT INTO oo_release_api.operation ( \
                    operation_id, project_id, idempotency_key, body_sha256, kind, status, \
                    candidate_id, version, base_version, expected_version, role_name, scope_name, \
                    config_commit, source_hash, pack_digest, manifest_sha256, columns_text, \
                    policy_id, policy_version, knowledge_id, knowledge_version, catalog_version, \
                    review_admitted, producer_outcome, producer_reason, certificate_class, \
                    production_ready, customer_gate_verified, owner_authenticated, runsc_verified, \
                    gates_json, subject_name, target_operation_id \
                 ) VALUES ( \
                    $1, $2, $3, $4, $5, 'queued', \
                    $6, $7, $8, $9, $10, $11, \
                    $12, NULL, NULL, NULL, $13, \
                    $14, $15, $16, $17, $18, \
                    $19, NULL, NULL, 'not_issued', \
                    FALSE, FALSE, FALSE, FALSE, \
                    '[]', $20, $21 \
                 )",
                &[
                    &operation_id,
                    &project,
                    &idempotency_key,
                    &body_sha256,
                    &kind,
                    &candidate_id,
                    &version,
                    &base_version,
                    &expected_version,
                    &role_name,
                    &scope_name,
                    &config_commit,
                    &columns_text,
                    &policy_id,
                    &policy_version,
                    &knowledge_id,
                    &knowledge_version,
                    &catalog_version,
                    &review_admitted,
                    &subject,
                    &target,
                ],
            )
            .await;
        if let Err(err) = inserted {
            let unique = err.code().map(|code| code.code()) == Some(UNIQUE);
            let _ = tx.rollback().await;
            drop(client);
            if unique {
                return self.read_conflict(operation_id, body_sha256).await;
            }
            return Err(ApiError::not_ready("postgres"));
        }
        insert_audit(&tx, operation_id, &project, "queued", "queued").await?;
        if let Some(hash) = pack_file_sha.as_deref() {
            insert_audit(&tx, operation_id, &project, "pack.file", hash).await?;
        }
        let operation = load_id(&tx, operation_id).await?.ok_or(ApiError::not_ready("postgres"))?;
        commit_api(tx).await?;
        Ok(Queued { operation, replay: false })
    }

    async fn read_conflict(&self, operation_id: &str, body_sha256: &str) -> Result<Queued, ApiError> {
        let mut client = self.inner.api.lock().await;
        let tx = begin_api(&mut client).await?;
        let existing = load_id(&tx, operation_id).await?.ok_or(ApiError::not_ready("postgres"))?;
        commit_api(tx).await?;
        replay_or_conflict(existing, body_sha256)
    }

    async fn require_schema(&self) -> Result<(), Fault> {
        let mut client = self.inner.api.lock().await;
        let tx = begin_fault(&mut client).await?;
        let hash = migration_sha256();
        let stored = manifest_hash(&tx).await?;
        commit_fault(tx).await?;
        match stored {
            Some(stored) if stored == hash => Ok(()),
            _ => Err(Fault::refused("install_drift")),
        }
    }

    async fn reader_view(&self) -> Result<(), Fault> {
        let mut client = self.inner.reader.lock().await;
        let tx = begin_fault(&mut client).await?;
        let found = tx
            .query_opt("SELECT version FROM oo_release_control.publication_status", &[])
            .await
            .map_err(|_| Fault::refused("dependency.not_ready"))?;
        commit_fault(tx).await?;
        if found.is_some() { Ok(()) } else { Err(Fault::refused("dependency.not_ready")) }
    }

    async fn reconcile(&self) -> Result<(), Fault> {
        let project = self.inner.config.project_id.clone();
        let mut client = self.inner.api.lock().await;
        let tx = begin_fault(&mut client).await?;
        let rows = tx
            .query(
                "UPDATE oo_release_api.operation SET status = 'resume_uncertain' \
                 WHERE project_id = $1 AND status = 'running' \
                 RETURNING operation_id, candidate_id, source_hash",
                &[&project],
            )
            .await
            .map_err(|_| Fault::refused("dependency.not_ready"))?;
        let mut pending = Vec::new();
        for row in &rows {
            let candidate: Option<String> = row.try_get(1).map_err(|_| Fault::refused("dependency.not_ready"))?;
            let source: Option<String> = row.try_get(2).map_err(|_| Fault::refused("dependency.not_ready"))?;
            if let (Some(candidate), Some(source)) = (candidate, source) {
                pending.push((candidate, source));
            }
        }
        commit_fault(tx).await?;
        drop(client);
        for (candidate, source) in pending {
            self.recover_if_applying(&candidate, &source).await?;
        }
        Ok(())
    }

    async fn recover_if_applying(&self, candidate: &str, source: &str) -> Result<(), Fault> {
        let endpoint = self.endpoint();
        let config = &self.inner.config;
        let request = PlanRequest {
            endpoint: &endpoint,
            user: &config.producer_pg.user,
            password: &config.producer_pg.password,
            ca_pem: &config.producer_pg.ca_pem,
            project_id: &config.project_id,
            candidate_id: candidate,
            source_hash: source,
            scope: "test_scope",
            expected_version: None,
        };
        let receipt = match status(&request).await {
            Ok(receipt) => receipt,
            Err(StoreError::CommitUnknown) => return Err(Fault::commit_unknown()),
            Err(_) => return Ok(()),
        };
        if receipt.body.get("state").and_then(Value::as_str) != Some("applying") {
            return Ok(());
        }
        let recover_request = RecoverRequest {
            endpoint: &endpoint,
            user: &config.producer_pg.user,
            password: &config.producer_pg.password,
            ca_pem: &config.producer_pg.ca_pem,
            action: "recover",
            project_id: &config.project_id,
            candidate_id: candidate,
            expected_version: None,
        };
        match recover(&recover_request).await {
            Ok(_) => Ok(()),
            Err(StoreError::CommitUnknown) => Err(Fault::commit_unknown()),
            Err(_) => Ok(()),
        }
    }

    async fn producer_install(&self) -> Result<(), Fault> {
        let endpoint = self.endpoint();
        let config = &self.inner.config;
        let covers: Vec<String> = GATE_IDS.iter().map(|gate| (*gate).to_owned()).collect();
        let request = InstallRequest {
            endpoint: &endpoint,
            user: &config.producer_pg.user,
            password: &config.producer_pg.password,
            ca_pem: &config.producer_pg.ca_pem,
            project_id: &config.project_id,
            authority_id: &config.authority_id,
            covers: &covers,
        };
        let receipt = producer_install(&request).await.map_err(store_fault)?;
        if receipt.code != 0 {
            return Err(Fault::refused("install_drift"));
        }
        let hash = receipt.body.get("authority_statement_sha256").and_then(Value::as_str).unwrap_or("");
        if !hex64(hash) {
            return Err(Fault::refused("install_drift"));
        }
        let mut guard = self.inner.authority.lock().await;
        match guard.as_deref() {
            Some(stored) if stored != hash => return Err(Fault::refused("install_drift")),
            _ => *guard = Some(hash.to_owned()),
        }
        Ok(())
    }

    async fn claim(&self) -> Result<Option<String>, ApiError> {
        let project = self.inner.config.project_id.clone();
        let mut client = self.inner.api.lock().await;
        let tx = begin_api(&mut client).await?;
        let row = tx
            .query_opt(
                "SELECT operation_id FROM oo_release_api.operation \
                 WHERE project_id = $1 AND status = 'queued' \
                 ORDER BY operation_id \
                 FOR UPDATE SKIP LOCKED LIMIT 1",
                &[&project],
            )
            .await
            .map_err(|_| ApiError::not_ready("postgres"))?;
        let Some(row) = row else {
            commit_api(tx).await?;
            return Ok(None);
        };
        let id: String = row.try_get(0).map_err(|_| ApiError::not_ready("postgres"))?;
        let updated = tx
            .execute(
                "UPDATE oo_release_api.operation SET status = 'running' WHERE operation_id = $1 AND status = 'queued'",
                &[&id],
            )
            .await
            .map_err(|_| ApiError::not_ready("postgres"))?;
        commit_api(tx).await?;
        if updated == 1 { Ok(Some(id)) } else { Ok(None) }
    }

    async fn execute(&self, operation_id: &str) -> Result<(), ApiError> {
        let Some(operation) = self.operation(operation_id).await? else {
            return Ok(());
        };
        let control = if operation.kind == "publish" {
            self.execute_publish(&operation).await
        } else {
            self.execute_rollback(&operation).await
        };
        match control {
            Control::Uncertain => self.mark_uncertain(operation_id).await,
            Control::Finish(finish) => self.save_finish(operation_id, finish).await,
        }
    }

    async fn execute_publish(&self, operation: &Operation) -> Control {
        let Some(config_commit) = operation.config_commit.clone() else {
            return refused_control("pack", None, None, None);
        };
        let Some(role) = operation.role_name.clone() else {
            return refused_control("role", None, None, None);
        };
        let prepared = match pack::load_pack(self.pack_dir(), &config_commit, &role) {
            Ok(prepared) => prepared,
            Err(_) => return refused_control("pack", None, None, None),
        };
        let stored_hash = match self.audit_detail(&operation.operation_id, "pack.file").await {
            Ok(Some(hash)) => hash,
            Ok(None) => return refused_control("pack", None, None, None),
            Err(ApiError { code: "commit.unknown", .. }) => return Control::Uncertain,
            Err(_) => return refused_control("producer", None, None, None),
        };
        if stored_hash != prepared.bytes_sha256 || prepared.columns_text != operation.columns_text {
            return refused_control("pack_changed", None, None, None);
        }
        let intake = match pack::load_intake(self.pack_dir(), &config_commit) {
            Ok(intake) => intake,
            Err(_) => return refused_control("pack", None, None, None),
        };
        match self.producer_install().await {
            Ok(()) => {}
            Err(fault) if fault.code == "commit.unknown" => return Control::Uncertain,
            Err(_) => return refused_control("producer", None, None, None),
        }
        let authority = self.inner.authority.lock().await.clone().unwrap_or_default();
        if !hex64(&authority) {
            return refused_control("producer", None, None, None);
        }
        let endpoint = self.endpoint();
        let staged = match self.stage_of(&endpoint, operation, &prepared).await {
            Ok(receipt) => receipt,
            Err(Control::Uncertain) => return Control::Uncertain,
            Err(other) => return other,
        };
        if staged.code != 0 {
            return finish_receipt(&staged, "not_issued", "refused");
        }
        let source = text_field(&staged.body, "source_hash");
        let digest = text_field(&staged.body, "pack_digest");
        let manifest = text_field(&staged.body, "manifest_sha256");
        if digest.as_deref() != Some(prepared.pack_digest.as_str()) {
            return refused_control("pack_digest", source, digest, manifest);
        }
        let Some(source_hash) = source.clone() else {
            return refused_control("producer", source, digest, manifest);
        };
        if let Err(control) = self.record_intake(&endpoint, operation, &intake, &source_hash, &authority).await {
            return match control {
                Control::Finish(mut finish) => {
                    finish.source_hash = finish.source_hash.or(source);
                    finish.pack_digest = finish.pack_digest.or(digest);
                    finish.manifest_sha256 = finish.manifest_sha256.or(manifest);
                    Control::Finish(finish)
                }
                other => other,
            };
        }
        let scope = operation.scope_name.as_deref().unwrap_or("");
        if scope == "production" {
            let receipt = match self.activate_of(&endpoint, operation, &source_hash, "production").await {
                Ok(receipt) => receipt,
                Err(Control::Uncertain) => return Control::Uncertain,
                Err(other) => return other,
            };
            return production_finish(&receipt, source, digest, manifest);
        }
        let planned = match self.plan_of(&endpoint, operation, &source_hash, "test_scope").await {
            Ok(receipt) => receipt,
            Err(Control::Uncertain) => return Control::Uncertain,
            Err(other) => return other,
        };
        if !plan_admissible(&planned) {
            return finish_receipt(&planned, "not_issued", "refused");
        }
        match self.activate_of(&endpoint, operation, &source_hash, "test_scope").await {
            Ok(receipt) => publish_finish(&receipt, source, digest, manifest),
            Err(Control::Uncertain) => Control::Uncertain,
            Err(other) => other,
        }
    }

    async fn execute_rollback(&self, operation: &Operation) -> Control {
        let Some(target_id) = operation.target_operation_id.clone() else {
            return refused_control("compat", None, None, None);
        };
        let target = match self.operation(&target_id).await {
            Ok(Some(target)) => target,
            Ok(None) => return refused_control("compat", None, None, None),
            Err(ApiError { code: "commit.unknown", .. }) => return Control::Uncertain,
            Err(_) => return refused_control("producer", None, None, None),
        };
        let pointer = match self.pointer().await {
            Ok(pointer) => pointer,
            Err(ApiError { code: "commit.unknown", .. }) => return Control::Uncertain,
            Err(_) => return refused_control("producer", None, None, None),
        };
        if pointer.version.as_deref() != operation.expected_version.as_deref() || target.version.as_deref() != pointer.version.as_deref() {
            return refused_control("pointer", None, None, None);
        }
        let active = lineage_of_op(&target);
        let restored = match target.base_version.as_deref() {
            None => None,
            Some(version) => match self.succeeded_publish(version).await {
                Ok(Some(row)) => Some(lineage_of_op(&row)),
                Ok(None) => return refused_control("compat", None, None, None),
                Err(ApiError { code: "commit.unknown", .. }) => return Control::Uncertain,
                Err(_) => return refused_control("producer", None, None, None),
            },
        };
        if !permission_compatible(restored.as_ref(), &active) {
            return refused_control("compat", None, None, None);
        }
        let Some(candidate) = target.candidate_id.clone() else {
            return refused_control("producer", None, None, None);
        };
        let endpoint = self.endpoint();
        let config = &self.inner.config;
        let request = RecoverRequest {
            endpoint: &endpoint,
            user: &config.producer_pg.user,
            password: &config.producer_pg.password,
            ca_pem: &config.producer_pg.ca_pem,
            action: "rollback",
            project_id: &config.project_id,
            candidate_id: &candidate,
            expected_version: operation.expected_version.as_deref(),
        };
        match recover(&request).await {
            Ok(receipt) => rollback_finish(&receipt),
            Err(StoreError::CommitUnknown) => Control::Uncertain,
            Err(_) => refused_control("producer", None, None, None),
        }
    }

    async fn stage_of(&self, endpoint: &Endpoint, operation: &Operation, prepared: &Prepared) -> Result<Receipt, Control> {
        let config = &self.inner.config;
        let candidate = operation.candidate_id.as_deref().ok_or_else(|| refused_control("producer", None, None, None))?;
        let version = operation.version.as_deref().ok_or_else(|| refused_control("producer", None, None, None))?;
        let request = StageRequest {
            endpoint,
            user: &config.producer_pg.user,
            password: &config.producer_pg.password,
            ca_pem: &config.producer_pg.ca_pem,
            project_id: &config.project_id,
            candidate_id: candidate,
            version,
            base_version: operation.base_version.as_deref(),
            effective: &prepared.effective,
            visibility: &prepared.visibility,
            policy_id: &prepared.lineage.policy_id,
            policy_version: &prepared.lineage.policy_version,
            knowledge_id: &prepared.lineage.knowledge_id,
            knowledge_version: &prepared.lineage.knowledge_version,
            catalog_version: &prepared.lineage.catalog_version,
        };
        stage(&request).await.map_err(store_control)
    }

    async fn record_intake(&self, endpoint: &Endpoint, operation: &Operation, intake: &Intake, source_hash: &str, authority: &str) -> Result<(), Control> {
        let candidate = operation.candidate_id.as_deref().ok_or_else(|| refused_control("producer", None, None, None))?;
        for gate in &intake.gates {
            let authority_hash = if gate.issuer_class == "fixture_authority" { Some(authority) } else { None };
            let request = GateRequest {
                endpoint,
                user: &self.inner.config.producer_pg.user,
                password: &self.inner.config.producer_pg.password,
                ca_pem: &self.inner.config.producer_pg.ca_pem,
                project_id: &self.inner.config.project_id,
                candidate_id: candidate,
                source_hash,
                gate_id: &gate.gate_id,
                issuer_class: &gate.issuer_class,
                status: &gate.status,
                blocking: gate.blocking,
                report_type: &gate.report_type,
                authority_statement_sha256: authority_hash,
            };
            let receipt = gate_record(&request).await.map_err(store_control)?;
            if receipt.code != 0 {
                return Err(finish_receipt(&receipt, "not_issued", "refused"));
            }
        }
        for checkpoint_in in &intake.checkpoints {
            let authority_hash = if checkpoint_in.issuer_class == "fixture_authority" { Some(authority) } else { None };
            let request = CheckpointRequest {
                endpoint,
                user: &self.inner.config.producer_pg.user,
                password: &self.inner.config.producer_pg.password,
                ca_pem: &self.inner.config.producer_pg.ca_pem,
                project_id: &self.inner.config.project_id,
                candidate_id: candidate,
                kind: &checkpoint_in.kind,
                source_hash,
                epoch: &checkpoint_in.epoch,
                barrier: &checkpoint_in.barrier,
                sessions_terminated: checkpoint_in.sessions_terminated,
                issuer_class: &checkpoint_in.issuer_class,
                authority_statement_sha256: authority_hash,
            };
            let receipt = checkpoint(&request).await.map_err(store_control)?;
            if receipt.code != 0 {
                return Err(finish_receipt(&receipt, "not_issued", "refused"));
            }
        }
        Ok(())
    }

    async fn plan_of(&self, endpoint: &Endpoint, operation: &Operation, source_hash: &str, scope: &str) -> Result<Receipt, Control> {
        self.scope_call(endpoint, operation, source_hash, scope, true).await
    }

    async fn activate_of(&self, endpoint: &Endpoint, operation: &Operation, source_hash: &str, scope: &str) -> Result<Receipt, Control> {
        self.scope_call(endpoint, operation, source_hash, scope, false).await
    }

    async fn scope_call(&self, endpoint: &Endpoint, operation: &Operation, source_hash: &str, scope: &str, planning: bool) -> Result<Receipt, Control> {
        let candidate = operation.candidate_id.as_deref().ok_or_else(|| refused_control("producer", None, None, None))?;
        let request = PlanRequest {
            endpoint,
            user: &self.inner.config.producer_pg.user,
            password: &self.inner.config.producer_pg.password,
            ca_pem: &self.inner.config.producer_pg.ca_pem,
            project_id: &self.inner.config.project_id,
            candidate_id: candidate,
            source_hash,
            scope,
            expected_version: operation.base_version.as_deref(),
        };
        let result = if planning { plan(&request).await } else { activate(&request).await };
        result.map_err(store_control)
    }

    async fn save_finish(&self, operation_id: &str, finish: Finish) -> Result<(), ApiError> {
        let project = self.inner.config.project_id.clone();
        let mut client = self.inner.api.lock().await;
        let tx = begin_api(&mut client).await?;
        let updated = tx
            .execute(
                "UPDATE oo_release_api.operation SET \
                    status = $2, source_hash = $3, pack_digest = $4, manifest_sha256 = $5, \
                    producer_outcome = $6, producer_reason = $7, certificate_class = $8, gates_json = $9, \
                    production_ready = FALSE, customer_gate_verified = FALSE, \
                    owner_authenticated = FALSE, runsc_verified = FALSE \
                 WHERE operation_id = $1 AND status = 'running'",
                &[
                    &operation_id,
                    &finish.status,
                    &finish.source_hash,
                    &finish.pack_digest,
                    &finish.manifest_sha256,
                    &finish.outcome,
                    &finish.reason,
                    &finish.class,
                    &finish.gates,
                ],
            )
            .await
            .map_err(|_| ApiError::not_ready("postgres"))?;
        if updated == 1 {
            insert_audit(&tx, operation_id, &project, "result", finish.status).await?;
        }
        commit_api(tx).await
    }

    async fn mark_uncertain(&self, operation_id: &str) -> Result<(), ApiError> {
        let mut client = self.inner.api.lock().await;
        let tx = begin_api(&mut client).await?;
        tx.execute(
            "UPDATE oo_release_api.operation SET status = 'resume_uncertain', producer_reason = 'commit_unknown', \
             production_ready = FALSE, customer_gate_verified = FALSE, owner_authenticated = FALSE, runsc_verified = FALSE \
             WHERE operation_id = $1 AND status = 'running'",
            &[&operation_id],
        )
        .await
        .map_err(|_| ApiError::commit_unknown())?;
        commit_api(tx).await
    }

    async fn audit_detail(&self, operation_id: &str, event_name: &str) -> Result<Option<String>, ApiError> {
        let mut client = self.inner.api.lock().await;
        let tx = begin_api(&mut client).await?;
        let row = tx
            .query_opt(
                "SELECT detail FROM oo_release_api.audit_event WHERE operation_id = $1 AND event_name = $2",
                &[&operation_id, &event_name],
            )
            .await
            .map_err(|_| ApiError::not_ready("postgres"))?;
        let detail = match row {
            Some(row) => Some(row.try_get(0).map_err(|_| ApiError::not_ready("postgres"))?),
            None => None,
        };
        commit_api(tx).await?;
        Ok(detail)
    }

    fn endpoint(&self) -> Endpoint {
        Endpoint {
            host: self.inner.config.producer_pg.host.clone(),
            port: self.inner.config.producer_pg.port,
            database: self.inner.config.producer_pg.database.clone(),
        }
    }
}

pub async fn install(config: &Config) -> Result<Value, Fault> {
    if config.api_pg.user != API_ROLE {
        return Err(Fault::malformed());
    }
    let mut client = connect_pg(&config.api_pg).await?;
    require_user(&client, API_ROLE).await?;
    require_tls(&client).await?;
    let tx = begin_fault(&mut client).await?;
    tx.execute("SELECT pg_catalog.pg_advisory_xact_lock($1::bigint)", &[&INSTALL_LOCK]).await.map_err(|_| Fault::refused("dependency.not_ready"))?;
    let hash = migration_sha256();
    let exists = tx
        .query_opt("SELECT 1 FROM pg_catalog.pg_namespace WHERE nspname = $1", &[&SCHEMA])
        .await
        .map_err(|_| Fault::refused("dependency.not_ready"))?
        .is_some();
    let outcome = if !exists {
        tx.batch_execute(MIGRATION_SQL).await.map_err(|_| Fault::refused("dependency.not_ready"))?;
        tx.execute("INSERT INTO oo_release_api.install_manifest (singleton, sql_sha256) VALUES (TRUE, $1)", &[&hash])
            .await
            .map_err(|_| Fault::refused("dependency.not_ready"))?;
        "installed"
    } else {
        match manifest_hash(&tx).await? {
            Some(stored) if stored == hash => "replay",
            _ => {
                let _ = tx.rollback().await;
                return Err(Fault::refused("install_drift"));
            }
        }
    };
    commit_fault(tx).await?;
    Ok(json!({
        "contract": CONTRACT,
        "outcome": outcome,
        "sql_sha256": hash,
        "production_ready": false,
        "full_sp08_accepted": false
    }))
}

pub(crate) fn operation_id(project: &str, key: &str) -> String {
    let digest = sha256_hex(format!("{project}\n{key}").as_bytes());
    format!("op_{}", &digest[..32])
}

fn replay_or_conflict(existing: Operation, body_sha256: &str) -> Result<Queued, ApiError> {
    if existing.body_sha256 == body_sha256 {
        Ok(Queued { operation: existing, replay: true })
    } else {
        Err(ApiError::idempotency())
    }
}

const SELECT_OP: &str = "SELECT operation_id, project_id, body_sha256, kind, status, candidate_id, version, \
    base_version, expected_version, role_name, scope_name, config_commit, source_hash, pack_digest, manifest_sha256, \
    columns_text, policy_id, policy_version, knowledge_id, knowledge_version, catalog_version, review_admitted, \
    producer_outcome, producer_reason, certificate_class, gates_json, subject_name, target_operation_id \
    FROM oo_release_api.operation";

async fn load_id(tx: &Transaction<'_>, operation_id: &str) -> Result<Option<Operation>, ApiError> {
    let row = tx
        .query_opt(&format!("{SELECT_OP} WHERE operation_id = $1"), &[&operation_id])
        .await
        .map_err(|_| ApiError::not_ready("postgres"))?;
    match row {
        Some(row) => Ok(Some(read_op(&row).map_err(|_| ApiError::not_ready("postgres"))?)),
        None => Ok(None),
    }
}

async fn load_key(tx: &Transaction<'_>, project: &str, operation_id: &str) -> Result<Option<Operation>, ApiError> {
    let row = tx
        .query_opt(&format!("{SELECT_OP} WHERE project_id = $1 AND operation_id = $2"), &[&project, &operation_id])
        .await
        .map_err(|_| ApiError::not_ready("postgres"))?;
    match row {
        Some(row) => Ok(Some(read_op(&row).map_err(|_| ApiError::not_ready("postgres"))?)),
        None => Ok(None),
    }
}

fn read_op(row: &tokio_postgres::Row) -> Result<Operation, ()> {
    Ok(Operation {
        operation_id: row.try_get(0).map_err(|_| ())?,
        project_id: row.try_get(1).map_err(|_| ())?,
        body_sha256: row.try_get(2).map_err(|_| ())?,
        kind: row.try_get(3).map_err(|_| ())?,
        status: row.try_get(4).map_err(|_| ())?,
        candidate_id: row.try_get(5).map_err(|_| ())?,
        version: row.try_get(6).map_err(|_| ())?,
        base_version: row.try_get(7).map_err(|_| ())?,
        expected_version: row.try_get(8).map_err(|_| ())?,
        role_name: row.try_get(9).map_err(|_| ())?,
        scope_name: row.try_get(10).map_err(|_| ())?,
        config_commit: row.try_get(11).map_err(|_| ())?,
        source_hash: row.try_get(12).map_err(|_| ())?,
        pack_digest: row.try_get(13).map_err(|_| ())?,
        manifest_sha256: row.try_get(14).map_err(|_| ())?,
        columns_text: row.try_get(15).map_err(|_| ())?,
        policy_id: row.try_get(16).map_err(|_| ())?,
        policy_version: row.try_get(17).map_err(|_| ())?,
        knowledge_id: row.try_get(18).map_err(|_| ())?,
        knowledge_version: row.try_get(19).map_err(|_| ())?,
        catalog_version: row.try_get(20).map_err(|_| ())?,
        review_admitted: row.try_get(21).map_err(|_| ())?,
        producer_outcome: row.try_get(22).map_err(|_| ())?,
        producer_reason: row.try_get(23).map_err(|_| ())?,
        certificate_class: row.try_get(24).map_err(|_| ())?,
        gates_json: row.try_get(25).map_err(|_| ())?,
        subject_name: row.try_get(26).map_err(|_| ())?,
        target_operation_id: row.try_get(27).map_err(|_| ())?,
    })
}

async fn insert_audit(tx: &Transaction<'_>, operation_id: &str, project: &str, event_name: &str, detail: &str) -> Result<(), ApiError> {
    if !detail_ok(event_name) || !detail_ok(detail) {
        return Err(ApiError::not_ready("postgres"));
    }
    let audit_id = sha256_hex(format!("{operation_id}\n{event_name}\n{detail}").as_bytes());
    tx.execute(
        "INSERT INTO oo_release_api.audit_event (audit_id, operation_id, project_id, event_name, detail) VALUES ($1, $2, $3, $4, $5)",
        &[&audit_id, &operation_id, &project, &event_name, &detail],
    )
    .await
    .map_err(|_| ApiError::not_ready("postgres"))?;
    Ok(())
}

async fn manifest_hash(tx: &Transaction<'_>) -> Result<Option<String>, Fault> {
    let row = tx
        .query_opt("SELECT sql_sha256 FROM oo_release_api.install_manifest WHERE singleton", &[])
        .await
        .map_err(|_| Fault::refused("dependency.not_ready"))?;
    match row {
        Some(row) => Ok(Some(row.try_get(0).map_err(|_| Fault::refused("dependency.not_ready"))?)),
        None => Ok(None),
    }
}

async fn connect_pg(endpoint: &PgEndpoint) -> Result<Client, Fault> {
    let mut config = tokio_postgres::Config::new();
    config.user(&endpoint.user);
    config.password(&endpoint.password);
    config.host(&endpoint.host);
    config.port(endpoint.port);
    config.dbname(&endpoint.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("oo-release-control-api");
    config.connect_timeout(Duration::from_secs(10));
    let tls = pg_tls(&endpoint.ca_pem)?;
    let (client, connection) = config.connect(tls).await.map_err(|_| Fault::refused("dependency.not_ready"))?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

async fn require_user(client: &Client, expected: &str) -> Result<(), Fault> {
    let row = client.query_one("SELECT current_user", &[]).await.map_err(|_| Fault::refused("dependency.not_ready"))?;
    let user: String = row.try_get(0).map_err(|_| Fault::refused("dependency.not_ready"))?;
    if user == expected { Ok(()) } else { Err(Fault::refused("login_role")) }
}

async fn require_tls(client: &Client) -> Result<(), Fault> {
    let row = client
        .query_one("SELECT ssl, version FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()", &[])
        .await
        .map_err(|_| Fault::refused("dependency.not_ready"))?;
    let ssl: bool = row.try_get(0).map_err(|_| Fault::refused("dependency.not_ready"))?;
    let version: Option<String> = row.try_get(1).map_err(|_| Fault::refused("dependency.not_ready"))?;
    match version {
        Some(version) if ssl && version.starts_with("TLSv1.") => Ok(()),
        _ => Err(Fault::refused("dependency.not_ready")),
    }
}

async fn begin_api(client: &mut Client) -> Result<Transaction<'_>, ApiError> {
    let tx = client.transaction().await.map_err(|_| ApiError::not_ready("postgres"))?;
    if let Err(err) = tx.batch_execute(SET_LOCAL).await {
        let _ = tx.rollback().await;
        return Err(db_api(err, false));
    }
    Ok(tx)
}

async fn commit_api(tx: Transaction<'_>) -> Result<(), ApiError> {
    match tx.commit().await {
        Ok(()) => Ok(()),
        Err(err) => Err(db_api(err, true)),
    }
}

async fn begin_fault(client: &mut Client) -> Result<Transaction<'_>, Fault> {
    let tx = client.transaction().await.map_err(|_| Fault::refused("dependency.not_ready"))?;
    if let Err(err) = tx.batch_execute(SET_LOCAL).await {
        let _ = tx.rollback().await;
        return Err(if err.code().is_none() { Fault::commit_unknown() } else { Fault::refused("dependency.not_ready") });
    }
    Ok(tx)
}

async fn commit_fault(tx: Transaction<'_>) -> Result<(), Fault> {
    match tx.commit().await {
        Ok(()) => Ok(()),
        Err(err) => Err(if err.code().is_none() { Fault::commit_unknown() } else { Fault::refused("dependency.not_ready") }),
    }
}

fn db_api(err: tokio_postgres::Error, commit: bool) -> ApiError {
    if commit && err.code().is_none() { ApiError::commit_unknown() } else { ApiError::not_ready("postgres") }
}

fn store_fault(err: StoreError) -> Fault {
    match err {
        StoreError::CommitUnknown => Fault::commit_unknown(),
        StoreError::Input => Fault::malformed(),
        _ => Fault::refused("dependency.not_ready"),
    }
}

fn store_control(err: StoreError) -> Control {
    match err {
        StoreError::CommitUnknown => Control::Uncertain,
        _ => refused_control("producer", None, None, None),
    }
}

fn lineage_of_op(operation: &Operation) -> Lineage {
    Lineage::from_columns(
        &operation.columns_text,
        LineageFields {
            role: operation.role_name.as_deref().unwrap_or(""),
            policy_id: operation.policy_id.as_deref().unwrap_or(""),
            policy_version: operation.policy_version.as_deref().unwrap_or(""),
            knowledge_id: operation.knowledge_id.as_deref().unwrap_or(""),
            knowledge_version: operation.knowledge_version.as_deref().unwrap_or(""),
            catalog_version: operation.catalog_version.as_deref().unwrap_or(""),
            review_admitted: operation.review_admitted,
        },
    )
}

fn text_field(body: &Value, key: &str) -> Option<String> {
    let text = body.get(key).and_then(Value::as_str)?;
    if hex64(text) { Some(text.to_owned()) } else { None }
}

fn token_field(value: Option<&str>) -> Option<String> {
    let text = value?;
    if (1..=64).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
        Some(text.to_owned())
    } else {
        None
    }
}

fn unsafe_claim(body: &Value) -> bool {
    ["production_ready_certificate", "customer_gate_verified", "owner_authenticated", "runsc_verified", "production_ready", "credential_verified", "integrity_verified", "full_sp08_accepted"]
        .iter()
        .any(|key| body.get(*key) == Some(&Value::Bool(true)))
}

fn gates_text(body: &Value) -> String {
    let mut items = Vec::new();
    if let Some(gates) = body.get("gates").and_then(Value::as_array) {
        for gate in gates {
            let gate_id = gate.get("gate_id").and_then(Value::as_str).unwrap_or("");
            let disposition = gate.get("disposition").and_then(Value::as_str).unwrap_or("");
            if token_field(Some(gate_id)).is_none() || token_field(Some(disposition)).is_none() {
                continue;
            }
            items.push(json!({
                "gate_id": gate_id,
                "disposition": disposition,
                "test_admissible": gate.get("test_admissible").and_then(Value::as_bool).unwrap_or(false),
                "production_admissible": false
            }));
        }
    }
    if let Some(reasons) = body.get("reasons").and_then(Value::as_array) {
        for reason in reasons {
            let Some(token) = reason.as_str().and_then(|text| token_field(Some(text))) else {
                continue;
            };
            items.push(json!({
                "gate_id": token,
                "disposition": "reported",
                "test_admissible": false,
                "production_admissible": false
            }));
        }
    }
    serde_json::to_string(&Value::Array(items)).unwrap_or_else(|_| "[]".to_owned())
}

fn plan_admissible(receipt: &Receipt) -> bool {
    if receipt.code != 0 || unsafe_claim(&receipt.body) {
        return false;
    }
    let outcome = receipt.body.get("outcome").and_then(Value::as_str);
    let state = receipt.body.get("state").and_then(Value::as_str);
    matches!(outcome, Some("planned") | Some("replay")) && state == Some("gates_passed")
}

fn publish_finish(receipt: &Receipt, source: Option<String>, digest: Option<String>, manifest: Option<String>) -> Control {
    if unsafe_claim(&receipt.body) {
        return refused_control("false_claim", source, digest, manifest);
    }
    let outcome = receipt.body.get("outcome").and_then(Value::as_str);
    let state = receipt.body.get("state").and_then(Value::as_str);
    let class = receipt.body.get("certificate_class").and_then(Value::as_str);
    let ok = outcome == Some("activated") || (outcome == Some("replay") && state == Some("active"));
    if ok && state == Some("active") && class == Some("test_scope_not_production") {
        Control::Finish(Finish {
            status: "succeeded",
            outcome: token_field(outcome),
            reason: token_field(receipt.body.get("reason").and_then(Value::as_str)),
            source_hash: text_field(&receipt.body, "source_hash").or(source),
            pack_digest: text_field(&receipt.body, "pack_digest").or(digest),
            manifest_sha256: text_field(&receipt.body, "manifest_sha256").or(manifest),
            class: "test_scope_not_production",
            gates: gates_text(&receipt.body),
        })
    } else {
        finish_receipt(receipt, "not_issued", "refused")
    }
}

fn production_finish(receipt: &Receipt, source: Option<String>, digest: Option<String>, manifest: Option<String>) -> Control {
    let mut finish = match finish_receipt(receipt, "not_issued", "refused") {
        Control::Finish(finish) => finish,
        other => return other,
    };
    finish.source_hash = finish.source_hash.or(source);
    finish.pack_digest = finish.pack_digest.or(digest);
    finish.manifest_sha256 = finish.manifest_sha256.or(manifest);
    finish.class = "not_issued";
    finish.status = "refused";
    if finish.reason.is_none() {
        finish.reason = Some("production_blocked".to_owned());
    }
    Control::Finish(finish)
}

fn rollback_finish(receipt: &Receipt) -> Control {
    if unsafe_claim(&receipt.body) {
        return refused_control("false_claim", None, None, None);
    }
    let outcome = receipt.body.get("outcome").and_then(Value::as_str);
    let state = receipt.body.get("state").and_then(Value::as_str);
    let ok = outcome == Some("rolled_back") || (outcome == Some("replay") && state == Some("inactive"));
    if !ok {
        return finish_receipt(receipt, "not_issued", "refused");
    }
    Control::Finish(Finish {
        status: "succeeded",
        outcome: token_field(outcome),
        reason: token_field(receipt.body.get("reason").and_then(Value::as_str)),
        source_hash: None,
        pack_digest: text_field(&receipt.body, "pack_digest"),
        manifest_sha256: text_field(&receipt.body, "manifest_sha256"),
        class: "not_issued",
        gates: gates_text(&receipt.body),
    })
}

fn finish_receipt(receipt: &Receipt, class: &'static str, status: &'static str) -> Control {
    if unsafe_claim(&receipt.body) {
        return refused_control("false_claim", text_field(&receipt.body, "source_hash"), text_field(&receipt.body, "pack_digest"), text_field(&receipt.body, "manifest_sha256"));
    }
    let outcome = token_field(receipt.body.get("outcome").and_then(Value::as_str));
    let reason = token_field(receipt.body.get("reason").and_then(Value::as_str)).or_else(|| {
        if status == "refused" { Some("refused".to_owned()) } else { None }
    });
    Control::Finish(Finish {
        status,
        outcome,
        reason,
        source_hash: text_field(&receipt.body, "source_hash"),
        pack_digest: text_field(&receipt.body, "pack_digest"),
        manifest_sha256: text_field(&receipt.body, "manifest_sha256"),
        class,
        gates: gates_text(&receipt.body),
    })
}

fn refused_control(reason: &str, source: Option<String>, digest: Option<String>, manifest: Option<String>) -> Control {
    Control::Finish(Finish {
        status: "refused",
        outcome: Some("refused".to_owned()),
        reason: token_field(Some(reason)),
        source_hash: source,
        pack_digest: digest,
        manifest_sha256: manifest,
        class: "not_issued",
        gates: "[]".to_owned(),
    })
}
