//! Transactional adapter. The role-pack compiler decides the seal. This file stores it.

use std::time::Duration;

use postgres_types::ToSql;
use serde_json::{Value, json};
use tokio_postgres::config::SslMode;
use tokio_postgres::{Client, Transaction};

use crate::canon::{canonical, canonical_sha};
use crate::decide::{
    CheckpointFact, GateFact, Summary, certificate_sha, checkpoint_disposition, fact_disposition,
    is_external, is_gate, plan_document, production_outcome, summarize_external, summarize_gates,
};
use crate::error::{StoreError, query_error};
use crate::seal::{self, Expectation, Seal, compile_seal, recompile_g8};
use crate::sha256::sha256_hex;
use crate::sql::{self, ADMIN_ROLE, INSTALL_LOCK};
use crate::tls::rustls_connector;
use crate::{
    CONTRACT, FULL_SP08_ACCEPTED, PRODUCTION_READY_CERTIFICATE,
    TEST_MODE_CANNOT_PRODUCE_PRODUCTION_CERTIFICATE,
};

const UNIQUE: &str = "23505";

macro_rules! p {
    ($($value:expr),* $(,)?) => {
        &[$(&$value as &(dyn ToSql + Sync)),*]
    };
}

macro_rules! audit {
    ($tx:expr, $event:expr, $project:expr, $candidate:expr, $previous:expr, $new_version:expr, $source:expr, $detail:expr) => {
        insert_audit(
            $tx,
            Audit {
                event: $event,
                project: $project,
                candidate: $candidate,
                previous: $previous,
                new_version: $new_version,
                source: $source,
                detail: $detail,
            },
        )
    };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub database: String,
}

pub struct Receipt {
    pub code: u8,
    pub body: Value,
}

pub(crate) struct Session {
    pub(crate) client: Client,
    _connection: tokio::task::JoinHandle<()>,
}

struct Candidate {
    id: String,
    project: String,
    version: String,
    base: Option<String>,
    state: String,
    source_hash: String,
    effective: String,
    visibility: String,
    pack_digest: String,
    manifest_sha256: String,
    role: String,
    plan_hash: Option<String>,
    plan_scope: Option<String>,
    plan_epoch: Option<String>,
}

struct Pointer {
    version: Option<String>,
    candidate_id: Option<String>,
}

struct RollbackTarget {
    version: Option<String>,
    id: Option<String>,
    source: Option<String>,
    pack: Option<String>,
    manifest: Option<String>,
}

struct Deployment {
    project: String,
    authority_sha: String,
    covers: Vec<String>,
}

pub fn migration_sha256() -> String {
    sha256_hex(sql::MIGRATION_SQL.as_bytes())
}

pub struct InstallRequest<'a> {
    pub endpoint: &'a Endpoint,
    pub user: &'a str,
    pub password: &'a str,
    pub ca_pem: &'a [u8],
    pub project_id: &'a str,
    pub authority_id: &'a str,
    pub covers: &'a [String],
}

pub struct StageRequest<'a> {
    pub endpoint: &'a Endpoint,
    pub user: &'a str,
    pub password: &'a str,
    pub ca_pem: &'a [u8],
    pub project_id: &'a str,
    pub candidate_id: &'a str,
    pub version: &'a str,
    pub base_version: Option<&'a str>,
    pub effective: &'a Value,
    pub visibility: &'a Value,
    pub policy_id: &'a str,
    pub policy_version: &'a str,
    pub knowledge_id: &'a str,
    pub knowledge_version: &'a str,
    pub catalog_version: &'a str,
}

pub struct GateRequest<'a> {
    pub endpoint: &'a Endpoint,
    pub user: &'a str,
    pub password: &'a str,
    pub ca_pem: &'a [u8],
    pub project_id: &'a str,
    pub candidate_id: &'a str,
    pub source_hash: &'a str,
    pub gate_id: &'a str,
    pub issuer_class: &'a str,
    pub status: &'a str,
    pub blocking: bool,
    pub report_type: &'a str,
    pub authority_statement_sha256: Option<&'a str>,
}

pub struct CheckpointRequest<'a> {
    pub endpoint: &'a Endpoint,
    pub user: &'a str,
    pub password: &'a str,
    pub ca_pem: &'a [u8],
    pub project_id: &'a str,
    pub candidate_id: &'a str,
    pub kind: &'a str,
    pub source_hash: &'a str,
    pub epoch: &'a str,
    pub barrier: &'a str,
    pub sessions_terminated: bool,
    pub issuer_class: &'a str,
    pub authority_statement_sha256: Option<&'a str>,
}

pub struct PlanRequest<'a> {
    pub endpoint: &'a Endpoint,
    pub user: &'a str,
    pub password: &'a str,
    pub ca_pem: &'a [u8],
    pub project_id: &'a str,
    pub candidate_id: &'a str,
    pub source_hash: &'a str,
    pub scope: &'a str,
    pub expected_version: Option<&'a str>,
}

pub struct RecoverRequest<'a> {
    pub endpoint: &'a Endpoint,
    pub user: &'a str,
    pub password: &'a str,
    pub ca_pem: &'a [u8],
    pub action: &'a str,
    pub project_id: &'a str,
    pub candidate_id: &'a str,
    pub expected_version: Option<&'a str>,
}

pub async fn install(request: &InstallRequest<'_>) -> Result<Receipt, StoreError> {
    checked(request.endpoint, request.user, request.project_id)?;
    if !seal::token_ok(request.authority_id) || request.covers.is_empty() {
        return Err(StoreError::Input);
    }
    let mut covers: Vec<String> = request.covers.to_vec();
    covers.sort();
    covers.dedup();
    if covers.len() != request.covers.len() || covers.iter().any(|gate| !is_gate(gate)) {
        return Err(StoreError::Input);
    }
    let statement = seal::authority_statement(request.authority_id, &covers);
    let statement_text = canonical(&statement);
    let statement_sha = sha256_hex(statement_text.as_bytes());
    let hash = migration_sha256();
    let mut session = open(
        request.endpoint,
        request.user,
        request.password,
        request.ca_pem,
    )
    .await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let receipt = match install_body(
        &tx,
        request,
        &hash,
        &statement_text,
        &statement_sha,
        &covers,
    )
    .await
    {
        Ok(receipt) => receipt,
        Err(err) => return Err(rollback(tx, err).await),
    };
    commit(tx, receipt).await
}

pub async fn stage(request: &StageRequest<'_>) -> Result<Receipt, StoreError> {
    checked(request.endpoint, request.user, request.project_id)?;
    if !seal::token_ok(request.candidate_id) || !seal::token_ok(request.version) {
        return Err(StoreError::Input);
    }
    if let Some(base) = request.base_version
        && !seal::token_ok(base)
    {
        return Err(StoreError::Input);
    }
    let mut session = open(
        request.endpoint,
        request.user,
        request.password,
        request.ca_pem,
    )
    .await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let receipt = match stage_body(&tx, request).await {
        Ok(receipt) => receipt,
        Err(err) => return Err(rollback(tx, err).await),
    };
    commit(tx, receipt).await
}

pub async fn gate_record(request: &GateRequest<'_>) -> Result<Receipt, StoreError> {
    write_gate(request, None).await
}

pub(crate) async fn record_server_gate(
    request: &GateRequest<'_>,
    detail: &str,
) -> Result<Receipt, StoreError> {
    if detail.is_empty() || request.issuer_class != "fixture_authority" {
        return Err(StoreError::Input);
    }
    write_gate(request, Some(detail)).await
}

async fn write_gate(
    request: &GateRequest<'_>,
    server_detail: Option<&str>,
) -> Result<Receipt, StoreError> {
    checked(request.endpoint, request.user, request.project_id)?;
    if !seal::hex64(request.source_hash)
        || request
            .authority_statement_sha256
            .is_some_and(|hash| !seal::hex64(hash))
    {
        return Err(StoreError::Input);
    }
    if !is_gate(request.gate_id) && !is_external(request.gate_id) {
        return Err(StoreError::Input);
    }
    if !matches!(
        request.issuer_class,
        "test_issuer" | "fixture_authority" | "component_authority"
    ) {
        return Err(StoreError::Input);
    }
    if !matches!(request.status, "pass" | "fail" | "warn" | "not_run") {
        return Err(StoreError::Input);
    }
    if server_detail.is_none() && request.report_type == "target_measurement" {
        return Ok(refused("foreign_receipt", json!({"database_used": false})));
    }
    let mut session = open(
        request.endpoint,
        request.user,
        request.password,
        request.ca_pem,
    )
    .await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let receipt = match gate_body(&tx, request, server_detail).await {
        Ok(receipt) => receipt,
        Err(err) => return Err(rollback(tx, err).await),
    };
    commit(tx, receipt).await
}

pub async fn checkpoint(request: &CheckpointRequest<'_>) -> Result<Receipt, StoreError> {
    checked(request.endpoint, request.user, request.project_id)?;
    if !matches!(request.kind, "fence" | "replica")
        || !seal::token_ok(request.epoch)
        || !seal::hex64(request.source_hash)
    {
        return Err(StoreError::Input);
    }
    if !matches!(request.barrier, "closed" | "replayed" | "stale" | "missing") {
        return Err(StoreError::Input);
    }
    if !matches!(request.issuer_class, "test_issuer" | "fixture_authority") {
        return Err(StoreError::Input);
    }
    let mut session = open(
        request.endpoint,
        request.user,
        request.password,
        request.ca_pem,
    )
    .await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let receipt = match checkpoint_body(&tx, request).await {
        Ok(receipt) => receipt,
        Err(err) => return Err(rollback(tx, err).await),
    };
    commit(tx, receipt).await
}

pub async fn plan(request: &PlanRequest<'_>) -> Result<Receipt, StoreError> {
    scope_request(request)?;
    let mut session = open(
        request.endpoint,
        request.user,
        request.password,
        request.ca_pem,
    )
    .await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let receipt = match plan_body(&tx, request).await {
        Ok(receipt) => receipt,
        Err(err) => return Err(rollback(tx, err).await),
    };
    commit(tx, receipt).await
}

pub async fn enter_applying(request: &PlanRequest<'_>) -> Result<Receipt, StoreError> {
    scope_request(request)?;
    let mut session = open(
        request.endpoint,
        request.user,
        request.password,
        request.ca_pem,
    )
    .await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let receipt = match enter_body(&tx, request).await {
        Ok(receipt) => receipt,
        Err(err) => return Err(rollback(tx, err).await),
    };
    commit(tx, receipt).await
}

pub async fn commit_activation(request: &PlanRequest<'_>) -> Result<Receipt, StoreError> {
    scope_request(request)?;
    let mut session = open(
        request.endpoint,
        request.user,
        request.password,
        request.ca_pem,
    )
    .await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let receipt = match commit_body(&tx, request).await {
        Ok(receipt) => receipt,
        Err(err) => return Err(rollback(tx, err).await),
    };
    commit(tx, receipt).await
}

pub async fn activate(request: &PlanRequest<'_>) -> Result<Receipt, StoreError> {
    let entered = enter_applying(request).await?;
    if entered.code == 2 || entered.body.get("outcome").and_then(Value::as_str) == Some("replay") {
        return Ok(entered);
    }
    commit_activation(request).await
}

pub async fn recover(request: &RecoverRequest<'_>) -> Result<Receipt, StoreError> {
    checked(request.endpoint, request.user, request.project_id)?;
    if !matches!(request.action, "recover" | "abort" | "retire" | "rollback") {
        return Err(StoreError::Input);
    }
    if let Some(version) = request.expected_version
        && !seal::token_ok(version)
    {
        return Err(StoreError::Input);
    }
    let mut session = open(
        request.endpoint,
        request.user,
        request.password,
        request.ca_pem,
    )
    .await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let receipt = match recover_body(&tx, request).await {
        Ok(receipt) => receipt,
        Err(err) => return Err(rollback(tx, err).await),
    };
    commit(tx, receipt).await
}

pub async fn status(request: &PlanRequest<'_>) -> Result<Receipt, StoreError> {
    checked(request.endpoint, request.user, request.project_id)?;
    if !seal::token_ok(request.candidate_id) || !seal::hex64(request.source_hash) {
        return Err(StoreError::Input);
    }
    let mut session = open(
        request.endpoint,
        request.user,
        request.password,
        request.ca_pem,
    )
    .await?;
    require_tls(&session.client).await?;
    let tx = begin(&mut session).await?;
    let receipt = match status_body(&tx, request).await {
        Ok(receipt) => receipt,
        Err(err) => return Err(rollback(tx, err).await),
    };
    commit(tx, receipt).await
}

fn scope_request(request: &PlanRequest<'_>) -> Result<(), StoreError> {
    checked(request.endpoint, request.user, request.project_id)?;
    if !seal::token_ok(request.candidate_id) || !seal::hex64(request.source_hash) {
        return Err(StoreError::Input);
    }
    if !matches!(request.scope, "test_scope" | "production") {
        return Err(StoreError::Input);
    }
    if let Some(version) = request.expected_version
        && !seal::token_ok(version)
    {
        return Err(StoreError::Input);
    }
    Ok(())
}

fn checked(endpoint: &Endpoint, user: &str, project: &str) -> Result<(), StoreError> {
    if user != ADMIN_ROLE
        || !seal::token_ok(project)
        || endpoint.host.is_empty()
        || endpoint.database.is_empty()
        || endpoint.port == 0
    {
        return Err(StoreError::Input);
    }
    if endpoint.host.len() > 253 || endpoint.host.as_bytes().contains(&0) {
        return Err(StoreError::Input);
    }
    Ok(())
}

async fn install_body(
    tx: &Transaction<'_>,
    request: &InstallRequest<'_>,
    hash: &str,
    statement_text: &str,
    statement_sha: &str,
    covers: &[String],
) -> Result<Receipt, StoreError> {
    require_role(tx).await?;
    lock_install(tx).await?;
    let exists = tx
        .query_opt(sql::SCHEMA_EXISTS, &[&sql::SCHEMA])
        .await
        .map_err(query_error)?
        .is_some();
    if !exists {
        tx.batch_execute(sql::MIGRATION_SQL)
            .await
            .map_err(query_error)?;
        one(tx, sql::INSERT_MANIFEST, p!(hash)).await?;
        one(
            tx,
            sql::INSERT_DEPLOYMENT,
            p!(
                request.project_id,
                request.authority_id,
                statement_text,
                statement_sha
            ),
        )
        .await?;
        for gate in covers {
            one(tx, sql::INSERT_AUTHORITY_GATE, p!(gate)).await?;
        }
        one(tx, sql::INSERT_POINTER, p!(request.project_id)).await?;
        return Ok(stamped(
            "installed",
            None,
            json!({
                "sql_sha256": hash,
                "authority_statement_sha256": statement_sha,
                "database_used": true,
            }),
            "not_issued",
        ));
    }
    let stored = one_text(tx, sql::READ_MANIFEST).await?;
    if stored != hash {
        return Ok(refused(
            "install_drift",
            json!({"check": "hash", "database_used": true}),
        ));
    }
    let deployment = read_deployment(tx).await?;
    if deployment.project != request.project_id {
        return Ok(refused("project_binding", json!({})));
    }
    if deployment.authority_sha != statement_sha || deployment.covers != covers {
        return Ok(refused(
            "install_drift",
            json!({"check": "authority", "database_used": true}),
        ));
    }
    Ok(stamped(
        "replay",
        None,
        json!({
            "sql_sha256": hash,
            "authority_statement_sha256": statement_sha,
            "database_used": true,
        }),
        "not_issued",
    ))
}

async fn stage_body(
    tx: &Transaction<'_>,
    request: &StageRequest<'_>,
) -> Result<Receipt, StoreError> {
    require_role(tx).await?;
    lock_install(tx).await?;
    let deployment = read_deployment(tx).await?;
    if deployment.project != request.project_id {
        return Ok(refused("project_binding", json!({})));
    }
    let pointer = lock_pointer(tx).await?;
    if pointer.version.as_deref() != request.base_version {
        return Ok(refused(
            "base_mismatch",
            json!({"pointer_version": pointer.version}),
        ));
    }
    let seal = match compile_seal(
        request.effective,
        request.visibility,
        &Expectation {
            project_id: request.project_id,
            candidate_id: request.candidate_id,
            version: request.version,
            base_version: request.base_version,
            policy_id: request.policy_id,
            policy_version: request.policy_version,
            knowledge_id: request.knowledge_id,
            knowledge_version: request.knowledge_version,
            catalog_version: request.catalog_version,
        },
    ) {
        Ok(seal) => seal,
        Err(reason) => return Ok(refused(reason, json!({"database_used": true}))),
    };
    let existing = read_candidate(tx, request.candidate_id, true).await?;
    if let Some(existing) = existing {
        if existing.project != request.project_id || existing.version != request.version {
            return Ok(refused("seal_conflict", json!({})));
        }
        if existing.source_hash == seal.source_hash && existing.pack_digest == seal.pack_digest {
            return Ok(seal_receipt("replay", &seal, &existing.state));
        }
        if !matches!(
            existing.state.as_str(),
            "candidate_built" | "gates_passed" | "failed"
        ) {
            return Ok(refused("seal_conflict", json!({"state": existing.state})));
        }
        let updated = tx
            .execute(
                sql::UPDATE_SEAL,
                p!(
                    request.candidate_id,
                    request.base_version,
                    seal.source_hash,
                    seal.effective,
                    seal.visibility,
                    seal.effective_sha256,
                    seal.visibility_sha256,
                    seal.pack_digest,
                    seal.manifest_sha256,
                    seal.policy_id,
                    seal.policy_version,
                    seal.knowledge_id,
                    seal.knowledge_version,
                    seal.ontology_version,
                    seal.catalog_version,
                    seal.role,
                    seal.compiler,
                ),
            )
            .await
            .map_err(query_error)?;
        if updated != 1 {
            return Ok(refused("seal_conflict", json!({})));
        }
        tx.execute(sql::DELETE_GATES, p!(request.candidate_id))
            .await
            .map_err(query_error)?;
        tx.execute(sql::DELETE_CHECKPOINTS, p!(request.candidate_id))
            .await
            .map_err(query_error)?;
        tx.execute(sql::DELETE_FILES, p!(request.candidate_id))
            .await
            .map_err(query_error)?;
        insert_files(tx, request.candidate_id, &seal).await?;
        audit!(
            tx,
            "invalidated",
            request.project_id,
            request.candidate_id,
            pointer.version.as_deref(),
            None,
            &seal.source_hash,
            "new_source_hash"
        )
        .await?;
        return Ok(seal_receipt("invalidated", &seal, "candidate_built"));
    }
    let inserted = tx
        .execute(
            sql::INSERT_CANDIDATE,
            p!(
                request.candidate_id,
                request.project_id,
                request.version,
                request.base_version,
                seal.source_hash,
                seal.effective,
                seal.visibility,
                seal.effective_sha256,
                seal.visibility_sha256,
                seal.pack_digest,
                seal.manifest_sha256,
                seal.policy_id,
                seal.policy_version,
                seal.knowledge_id,
                seal.knowledge_version,
                seal.ontology_version,
                seal.catalog_version,
                seal.role,
                seal.compiler,
            ),
        )
        .await;
    if let Err(err) = inserted {
        if err.code().map(|code| code.code()) == Some(UNIQUE) {
            return Ok(refused("seal_conflict", json!({})));
        }
        return Err(query_error(err));
    }
    insert_files(tx, request.candidate_id, &seal).await?;
    audit!(
        tx,
        "staged",
        request.project_id,
        request.candidate_id,
        pointer.version.as_deref(),
        None,
        &seal.source_hash,
        "candidate_built"
    )
    .await?;
    Ok(seal_receipt("staged", &seal, "candidate_built"))
}

fn seal_receipt(outcome: &str, seal: &Seal, state: &str) -> Receipt {
    stamped(
        outcome,
        None,
        json!({
            "state": state,
            "source_hash": seal.source_hash,
            "pack_digest": seal.pack_digest,
            "manifest_sha256": seal.manifest_sha256,
            "policy_id": seal.policy_id,
            "policy_version": seal.policy_version,
            "knowledge_id": seal.knowledge_id,
            "knowledge_version": seal.knowledge_version,
            "catalog_version": seal.catalog_version,
            "role": seal.role,
            "compiler": seal.compiler,
            "file_count": seal.files.len(),
            "database_used": true,
            "paths_normalized": true,
        }),
        "not_issued",
    )
}

async fn insert_files(
    tx: &Transaction<'_>,
    candidate_id: &str,
    seal: &Seal,
) -> Result<(), StoreError> {
    for file in &seal.files {
        one(
            tx,
            sql::INSERT_FILE,
            p!(candidate_id, file.path, file.sha256, file.byte_len),
        )
        .await?;
    }
    Ok(())
}

fn blocks_test_scope(summary: &Summary) -> bool {
    if summary.test_admissible || summary.disposition == "not_applicable" {
        return false;
    }
    // G7 is recorded after the target is open, so a missing or unrun G7 is not a plan input. A failed G7 still blocks.
    if summary.gate_id == "G7" && matches!(summary.disposition.as_str(), "missing" | "not_run") {
        return false;
    }
    // G3/G4 not_run is an unmeasured warning. An unmeasured G9 token budget blocks.
    if summary.disposition == "not_run" && matches!(summary.gate_id.as_str(), "G3" | "G4") {
        return false;
    }
    true
}

pub(crate) fn overlay_one(summary: &mut Summary, disposition: &str) -> bool {
    if disposition == "blocked" {
        summary.test_admissible = false;
        summary.production_admissible = false;
        summary.disposition = "blocked".to_owned();
        return false;
    }
    if disposition != "not_applicable" {
        return false;
    }
    summary.production_admissible = false;
    if summary.disposition == "failed" || summary.disposition == "conflict" {
        summary.test_admissible = false;
        return false;
    }
    if summary.test_admissible {
        return false;
    }
    summary.disposition = "not_applicable".to_owned();
    summary.test_admissible = false;
    true
}

async fn gate_body(
    tx: &Transaction<'_>,
    request: &GateRequest<'_>,
    server_detail: Option<&str>,
) -> Result<Receipt, StoreError> {
    require_role(tx).await?;
    lock_install(tx).await?;
    let deployment = read_deployment(tx).await?;
    if deployment.project != request.project_id {
        return Ok(refused("project_binding", json!({})));
    }
    let Some(candidate) = read_candidate(tx, request.candidate_id, true).await? else {
        return Ok(refused("activation", json!({"check": "candidate"})));
    };
    if candidate.project != request.project_id {
        return Ok(refused("project_binding", json!({})));
    }
    // A caller receipt cannot be added after the candidate leaves candidate_built.
    // The one exception is the server G7 measurement, which exists only after open.
    let server_g7 = server_detail.is_some()
        && request.gate_id == "G7"
        && request.status == "pass"
        && !request.blocking;
    if matches!(
        candidate.state.as_str(),
        "active" | "inactive" | "aborted" | "retired" | "applying"
    ) && !(server_g7 && candidate.state == "active")
    {
        return Ok(refused("seal_conflict", json!({"state": candidate.state})));
    }
    let (status, report_type, detail, issuer) = if request.issuer_class == "component_authority" {
        if request.gate_id != "G8" {
            (
                "not_run".to_owned(),
                "component_recompile".to_owned(),
                "component_authority_does_not_cover".to_owned(),
                "component_authority".to_owned(),
            )
        } else if recompile_g8(
            &candidate.effective,
            &candidate.visibility,
            &candidate.pack_digest,
            &candidate.manifest_sha256,
        )
        .is_ok()
        {
            (
                "pass".to_owned(),
                "component_recompile".to_owned(),
                "role_pack_samples_omitted".to_owned(),
                "component_authority".to_owned(),
            )
        } else {
            (
                "not_run".to_owned(),
                "component_recompile".to_owned(),
                "recompile_mismatch".to_owned(),
                "component_authority".to_owned(),
            )
        }
    } else if let Some(detail) = server_detail {
        (
            request.status.to_owned(),
            "target_measurement".to_owned(),
            detail.to_owned(),
            "fixture_authority".to_owned(),
        )
    } else {
        (
            request.status.to_owned(),
            request.report_type.to_owned(),
            "caller_receipt".to_owned(),
            request.issuer_class.to_owned(),
        )
    };
    let blocking = if issuer == "component_authority" {
        false
    } else {
        request.blocking
    };
    let body = json!({
        "protocol": "oo.release-coordinator/gate-body/v1",
        "candidate_id": request.candidate_id,
        "source_hash": request.source_hash,
        "gate_id": request.gate_id,
        "issuer_class": issuer,
        "status": status,
        "blocking": blocking,
        "scope": "test_scope",
        "report_type": report_type,
        "authority_statement_sha256": request.authority_statement_sha256,
        "detail": detail,
    });
    let certificate = certificate_sha(&body);
    if let Some(row) = tx
        .query_opt(sql::READ_GATE_ONE, p!(request.candidate_id, certificate))
        .await
        .map_err(query_error)?
    {
        let disposition: String = row
            .try_get(0)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        return Ok(stamped(
            "replay",
            None,
            json!({
                "disposition": disposition,
                "certificate_sha256": certificate,
                "detail": detail,
                "pii_database_scan": "not_performed",
                "database_used": true,
            }),
            "not_issued",
        ));
    }
    let fact = GateFact {
        gate_id: request.gate_id.to_owned(),
        source_hash: request.source_hash.to_owned(),
        issuer_class: issuer.clone(),
        status: status.clone(),
        blocking,
        scope: "test_scope".to_owned(),
        report_type: report_type.clone(),
        authority_statement_sha256: request.authority_statement_sha256.map(str::to_owned),
        certificate_sha256: certificate.clone(),
    };
    let disposition = fact_disposition(
        &fact,
        &candidate.source_hash,
        &deployment.authority_sha,
        &deployment.covers,
    );
    one(
        tx,
        sql::INSERT_GATE,
        p!(
            request.candidate_id,
            certificate,
            request.gate_id,
            request.source_hash,
            issuer,
            status,
            blocking,
            report_type,
            request.authority_statement_sha256,
            disposition,
            detail,
        ),
    )
    .await?;
    if candidate.state == "gates_passed" {
        clear_plan(tx, &candidate.id).await?;
    }
    audit!(
        tx,
        "gate_recorded",
        request.project_id,
        request.candidate_id,
        None,
        None,
        request.source_hash,
        disposition
    )
    .await?;
    Ok(stamped(
        "recorded",
        None,
        json!({
            "disposition": disposition,
            "certificate_sha256": certificate,
            "detail": detail,
            "pii_database_scan": "not_performed",
            "test_admissible": disposition == "admissible",
            "production_admissible": false,
            "database_used": true,
        }),
        "not_issued",
    ))
}

async fn checkpoint_body(
    tx: &Transaction<'_>,
    request: &CheckpointRequest<'_>,
) -> Result<Receipt, StoreError> {
    require_role(tx).await?;
    lock_install(tx).await?;
    let deployment = read_deployment(tx).await?;
    if deployment.project != request.project_id {
        return Ok(refused("project_binding", json!({})));
    }
    let Some(candidate) = read_candidate(tx, request.candidate_id, true).await? else {
        return Ok(refused("activation", json!({"check": "candidate"})));
    };
    if matches!(
        candidate.state.as_str(),
        "active" | "inactive" | "aborted" | "retired" | "applying"
    ) {
        return Ok(refused("seal_conflict", json!({"state": candidate.state})));
    }
    let body = json!({
        "protocol": "oo.release-coordinator/checkpoint-body/v1",
        "candidate_id": request.candidate_id,
        "kind": request.kind,
        "source_hash": request.source_hash,
        "epoch": request.epoch,
        "barrier": request.barrier,
        "sessions_terminated": request.sessions_terminated,
        "issuer_class": request.issuer_class,
        "authority_statement_sha256": request.authority_statement_sha256,
    });
    let certificate = certificate_sha(&body);
    if tx
        .query_opt(
            sql::READ_CHECKPOINT_ONE,
            p!(request.candidate_id, certificate),
        )
        .await
        .map_err(query_error)?
        .is_some()
    {
        return Ok(stamped(
            "replay",
            None,
            json!({"certificate_sha256": certificate, "database_used": true}),
            "not_issued",
        ));
    }
    let fact = CheckpointFact {
        kind: request.kind.to_owned(),
        source_hash: request.source_hash.to_owned(),
        epoch: request.epoch.to_owned(),
        barrier: request.barrier.to_owned(),
        sessions_terminated: request.sessions_terminated,
        issuer_class: request.issuer_class.to_owned(),
        authority_statement_sha256: request.authority_statement_sha256.map(str::to_owned),
        certificate_sha256: certificate.clone(),
    };
    let disposition =
        checkpoint_disposition(&fact, &candidate.source_hash, &deployment.authority_sha);
    one(
        tx,
        sql::INSERT_CHECKPOINT,
        p!(
            request.candidate_id,
            request.kind,
            certificate,
            request.source_hash,
            request.epoch,
            request.barrier,
            request.sessions_terminated,
            request.issuer_class,
            request.authority_statement_sha256,
            disposition,
        ),
    )
    .await?;
    if candidate.state == "gates_passed" {
        clear_plan(tx, &candidate.id).await?;
    }
    Ok(stamped(
        "recorded",
        None,
        json!({
            "disposition": disposition,
            "certificate_sha256": certificate,
            "sessions_terminated_by_tool": false,
            "database_used": true,
        }),
        "not_issued",
    ))
}

struct Assessment {
    test_ready: bool,
    production_reasons: Vec<String>,
    test_reasons: Vec<String>,
    epoch: Option<String>,
    plan_hash: String,
    gates: Value,
}

async fn assess(
    tx: &Transaction<'_>,
    candidate: &Candidate,
    scope_expected: Option<&str>,
    deployment: &Deployment,
    pointer: &Pointer,
) -> Result<Assessment, StoreError> {
    let facts = read_gates(tx, &candidate.id).await?;
    let checkpoints = read_checkpoints(tx, &candidate.id).await?;
    let mut gates = summarize_gates(
        &facts,
        &candidate.source_hash,
        &deployment.authority_sha,
        &deployment.covers,
    );
    let evidence = overlay_maintenance(tx, &candidate.id, &mut gates).await?;
    let external = summarize_external(
        &facts,
        &candidate.source_hash,
        &deployment.authority_sha,
        &deployment.covers,
    );
    let mut test_reasons = Vec::new();
    for summary in &gates {
        if blocks_test_scope(summary) {
            test_reasons.push(format!("{}_{}", summary.gate_id, summary.disposition));
        }
    }
    let fresh_fences: Vec<&CheckpointFact> = checkpoints
        .iter()
        .filter(|fact| {
            fact.kind == "fence"
                && checkpoint_disposition(fact, &candidate.source_hash, &deployment.authority_sha)
                    == "fresh"
        })
        .collect();
    let fresh_replicas: Vec<&CheckpointFact> = checkpoints
        .iter()
        .filter(|fact| {
            fact.kind == "replica"
                && checkpoint_disposition(fact, &candidate.source_hash, &deployment.authority_sha)
                    == "fresh"
        })
        .collect();
    let honest_missing_replica = fresh_replicas.is_empty()
        && checkpoints
            .iter()
            .filter(|fact| {
                fact.kind == "replica" && fact.barrier == "missing" && !fact.sessions_terminated
            })
            .count()
            == 1;
    if fresh_fences.len() > 1 || fresh_replicas.len() > 1 {
        test_reasons.push("conflict".to_owned());
    }
    if fresh_fences.is_empty() {
        test_reasons.push(if checkpoints.iter().any(|fact| fact.kind == "fence") {
            "stale_fence".to_owned()
        } else {
            "missing_fence".to_owned()
        });
    }
    if fresh_replicas.is_empty() && !honest_missing_replica {
        test_reasons.push(if checkpoints.iter().any(|fact| fact.kind == "replica") {
            "stale_replica".to_owned()
        } else {
            "missing_replica".to_owned()
        });
    }
    let same_epoch = fresh_fences.len() == 1
        && fresh_replicas.len() == 1
        && fresh_fences[0].epoch == fresh_replicas[0].epoch;
    if fresh_fences.len() == 1 && fresh_replicas.len() == 1 && !same_epoch {
        test_reasons.push("stale_replica".to_owned());
    }
    let epoch = if same_epoch || (honest_missing_replica && fresh_fences.len() == 1) {
        Some(fresh_fences[0].epoch.clone())
    } else {
        None
    };
    let pointer_is_self = pointer.candidate_id.as_deref() == Some(candidate.id.as_str())
        && pointer.version.as_deref() == Some(candidate.version.as_str());
    if !pointer_is_self
        && (pointer.version.as_deref() != candidate.base.as_deref()
            || scope_expected != candidate.base.as_deref())
    {
        test_reasons.push("version_mismatch".to_owned());
    }
    let blocked: Vec<&str> = gates
        .iter()
        .filter(|summary| summary.disposition == "blocked")
        .map(|summary| summary.gate_id.as_str())
        .collect();
    let mut certificates = Vec::new();
    for fact in &facts {
        if blocked.contains(&fact.gate_id.as_str()) {
            continue;
        }
        if fact_disposition(
            fact,
            &candidate.source_hash,
            &deployment.authority_sha,
            &deployment.covers,
        ) == "admissible"
        {
            certificates.push(fact.certificate_sha256.clone());
        }
    }
    certificates.extend(evidence);
    for fact in &checkpoints {
        if checkpoint_disposition(fact, &candidate.source_hash, &deployment.authority_sha)
            == "fresh"
        {
            certificates.push(fact.certificate_sha256.clone());
        }
    }
    certificates.sort();
    let epoch_text = epoch.clone().unwrap_or_default();
    let plan_hash = canonical_sha(&plan_document(
        &candidate.source_hash,
        "test_scope",
        &epoch_text,
        candidate.base.as_deref(),
        &certificates,
    ));
    let mut production_reasons = Vec::new();
    for summary in gates.iter().chain(external.iter()) {
        if summary.disposition == "missing" {
            production_reasons.push(format!("{}_missing", summary.gate_id));
        } else if !summary.production_admissible {
            production_reasons.push(format!("{}_not_production_admissible", summary.gate_id));
        }
    }
    production_reasons.push("fence_not_production_admissible".to_owned());
    production_reasons.push("replica_not_production_admissible".to_owned());
    let gate_json = serde_json::to_value(
        gates
            .iter()
            .map(|summary| {
                json!({
                    "gate_id": summary.gate_id,
                    "disposition": summary.disposition,
                    "test_admissible": summary.test_admissible,
                    "production_admissible": summary.production_admissible,
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or(Value::Null);
    Ok(Assessment {
        test_ready: test_reasons.is_empty(),
        production_reasons,
        test_reasons,
        epoch,
        plan_hash,
        gates: gate_json,
    })
}

async fn plan_body(tx: &Transaction<'_>, request: &PlanRequest<'_>) -> Result<Receipt, StoreError> {
    require_role(tx).await?;
    lock_install(tx).await?;
    let deployment = read_deployment(tx).await?;
    if deployment.project != request.project_id {
        return Ok(refused("project_binding", json!({})));
    }
    let pointer = lock_pointer(tx).await?;
    let Some(candidate) = read_candidate(tx, request.candidate_id, true).await? else {
        return Ok(refused("activation", json!({"check": "candidate"})));
    };
    if candidate.source_hash != request.source_hash {
        return Ok(refused("stale", json!({})));
    }
    let assessment = assess(
        tx,
        &candidate,
        request.expected_version,
        &deployment,
        &pointer,
    )
    .await?;
    if request.scope == "production" {
        let outcome = production_outcome(&assessment.production_reasons);
        let reason = if outcome == "ready" {
            "production_certificate_not_issued"
        } else {
            "production_blocked"
        };
        return Ok(stamped(
            "refused",
            Some(reason),
            json!({
                "state": candidate.state,
                "reasons": assessment.production_reasons,
                "production_outcome": outcome,
                "gates": assessment.gates,
                "pointer_version": pointer.version,
                "database_used": true,
            }),
            "not_issued",
        ));
    }
    if !assessment.test_ready {
        if candidate.state == "gates_passed" {
            clear_plan(tx, &candidate.id).await?;
        }
        return Ok(stamped(
            "refused",
            Some("not_ready"),
            json!({
                "state": if candidate.state == "gates_passed" { "candidate_built" } else { candidate.state.as_str() },
                "reasons": assessment.test_reasons,
                "gates": assessment.gates,
                "pointer_version": pointer.version,
                "database_used": true,
            }),
            "not_issued",
        ));
    }
    let epoch = assessment.epoch.as_deref().unwrap_or("");
    if candidate.state == "gates_passed"
        && candidate.plan_hash.as_deref() == Some(assessment.plan_hash.as_str())
    {
        return Ok(stamped(
            "replay",
            None,
            json!({
                "state": "gates_passed",
                "plan_hash": assessment.plan_hash,
                "gates": assessment.gates,
                "database_used": true,
            }),
            "not_issued",
        ));
    }
    if !matches!(
        candidate.state.as_str(),
        "candidate_built" | "failed" | "gates_passed"
    ) {
        return Ok(refused("seal_conflict", json!({"state": candidate.state})));
    }
    one(
        tx,
        sql::UPDATE_PLAN,
        p!(
            candidate.id,
            "gates_passed",
            assessment.plan_hash,
            "test_scope",
            epoch,
            candidate.state
        ),
    )
    .await?;
    audit!(
        tx,
        "planned",
        request.project_id,
        &candidate.id,
        pointer.version.as_deref(),
        None,
        &candidate.source_hash,
        "gates_passed"
    )
    .await?;
    Ok(stamped(
        "planned",
        None,
        json!({
            "state": "gates_passed",
            "plan_hash": assessment.plan_hash,
            "gates": assessment.gates,
            "production_reasons": assessment.production_reasons,
            "database_used": true,
        }),
        "not_issued",
    ))
}

async fn enter_body(
    tx: &Transaction<'_>,
    request: &PlanRequest<'_>,
) -> Result<Receipt, StoreError> {
    require_role(tx).await?;
    lock_install(tx).await?;
    let deployment = read_deployment(tx).await?;
    let pointer = read_pointer(tx).await?;
    let Some(candidate) = read_candidate(tx, request.candidate_id, true).await? else {
        return Ok(refused("activation", json!({"check": "candidate"})));
    };
    if candidate.source_hash != request.source_hash || deployment.project != request.project_id {
        return Ok(refused("stale", json!({})));
    }
    let assessment = assess(
        tx,
        &candidate,
        request.expected_version,
        &deployment,
        &pointer,
    )
    .await?;
    if request.scope == "production" {
        let outcome = production_outcome(&assessment.production_reasons);
        let reason = if outcome == "ready" {
            "production_certificate_not_issued"
        } else {
            "production_blocked"
        };
        return Ok(stamped(
            "refused",
            Some(reason),
            json!({
                "reasons": assessment.production_reasons,
                "production_outcome": outcome,
                "gates": assessment.gates,
                "state": candidate.state,
                "pointer_version": pointer.version,
                "database_used": true,
            }),
            "not_issued",
        ));
    }
    if candidate.state == "active"
        && pointer.candidate_id.as_deref() == Some(candidate.id.as_str())
        && pointer.version.as_deref() == Some(candidate.version.as_str())
    {
        let audit = event_audit(tx, &candidate.id, "activated").await?;
        return Ok(stamped(
            "replay",
            None,
            json!({
                "state": "active",
                "audit_id": audit,
                "pointer_version": pointer.version,
                "source_hash": candidate.source_hash,
                "database_used": true,
            }),
            "test_scope_not_production",
        ));
    }
    if candidate.state == "applying"
        && candidate.plan_hash.as_deref() == Some(assessment.plan_hash.as_str())
    {
        return Ok(stamped(
            "applying",
            None,
            json!({
                "state": "applying",
                "pointer_version": pointer.version,
                "database_used": true,
            }),
            "not_issued",
        ));
    }
    if !assessment.test_ready
        || candidate.state != "gates_passed"
        || candidate.plan_hash.as_deref() != Some(assessment.plan_hash.as_str())
    {
        return Ok(stamped(
            "refused",
            Some("not_ready"),
            json!({
                "reasons": assessment.test_reasons,
                "state": candidate.state,
                "pointer_version": pointer.version,
                "database_used": true,
            }),
            "not_issued",
        ));
    }
    one(
        tx,
        sql::UPDATE_STATE,
        p!(candidate.id, "applying", "gates_passed"),
    )
    .await?;
    audit!(
        tx,
        "applying",
        request.project_id,
        &candidate.id,
        pointer.version.as_deref(),
        None,
        &candidate.source_hash,
        "intent"
    )
    .await?;
    Ok(stamped(
        "applying",
        None,
        json!({
            "state": "applying",
            "pointer_version": pointer.version,
            "half_published": false,
            "database_used": true,
        }),
        "not_issued",
    ))
}

async fn commit_body(
    tx: &Transaction<'_>,
    request: &PlanRequest<'_>,
) -> Result<Receipt, StoreError> {
    require_role(tx).await?;
    lock_install(tx).await?;
    let deployment = read_deployment(tx).await?;
    let pointer = lock_pointer(tx).await?;
    let Some(candidate) = read_candidate(tx, request.candidate_id, true).await? else {
        return Ok(refused("activation", json!({"check": "candidate"})));
    };
    if candidate.state == "active" && pointer.candidate_id.as_deref() == Some(candidate.id.as_str())
    {
        let audit = event_audit(tx, &candidate.id, "activated").await?;
        return Ok(stamped(
            "replay",
            None,
            json!({
                "state": "active",
                "audit_id": audit,
                "pointer_version": pointer.version,
                "source_hash": candidate.source_hash,
                "pack_digest": candidate.pack_digest,
                "database_used": true,
            }),
            "test_scope_not_production",
        ));
    }
    if request.scope != "test_scope"
        || candidate.state != "applying"
        || candidate.source_hash != request.source_hash
    {
        return Ok(refused("not_ready", json!({"state": candidate.state})));
    }
    let assessment = assess(
        tx,
        &candidate,
        request.expected_version,
        &deployment,
        &pointer,
    )
    .await?;
    if !assessment.test_ready
        || candidate.plan_hash.as_deref() != Some(assessment.plan_hash.as_str())
    {
        one(
            tx,
            sql::UPDATE_STATE,
            p!(candidate.id, "failed", "applying"),
        )
        .await?;
        audit!(
            tx,
            "conflict_failed",
            request.project_id,
            &candidate.id,
            pointer.version.as_deref(),
            None,
            &candidate.source_hash,
            "recheck"
        )
        .await?;
        let reason = if assessment
            .test_reasons
            .iter()
            .any(|reason| reason == "version_mismatch")
        {
            "cas_conflict"
        } else if assessment
            .test_reasons
            .iter()
            .any(|reason| reason.contains("fence"))
        {
            "stale_fence"
        } else if assessment
            .test_reasons
            .iter()
            .any(|reason| reason.contains("replica"))
        {
            "stale_replica"
        } else {
            "not_ready"
        };
        return Ok(stamped(
            "refused",
            Some(reason),
            json!({
                "state": "failed",
                "reasons": assessment.test_reasons,
                "pointer_version": pointer.version,
                "half_published": false,
                "database_used": true,
            }),
            "not_issued",
        ));
    }
    let epoch = assessment.epoch.as_deref().unwrap_or("");
    let updated = tx
        .execute(
            sql::UPDATE_POINTER,
            p!(
                candidate.version,
                candidate.id,
                candidate.source_hash,
                candidate.pack_digest,
                candidate.manifest_sha256,
                request.project_id,
                candidate.base,
            ),
        )
        .await
        .map_err(query_error)?;
    if updated != 1 {
        one(
            tx,
            sql::UPDATE_STATE,
            p!(candidate.id, "failed", "applying"),
        )
        .await?;
        audit!(
            tx,
            "conflict_failed",
            request.project_id,
            &candidate.id,
            pointer.version.as_deref(),
            None,
            &candidate.source_hash,
            "cas"
        )
        .await?;
        return Ok(stamped(
            "refused",
            Some("cas_conflict"),
            json!({
                "state": "failed",
                "pointer_version": pointer.version,
                "half_published": false,
                "database_used": true,
            }),
            "not_issued",
        ));
    }
    if let Some(previous) = &pointer.candidate_id
        && previous != &candidate.id
    {
        tx.execute(sql::MARK_INACTIVE, p!(previous))
            .await
            .map_err(query_error)?;
    }
    one(
        tx,
        sql::MARK_ACTIVE,
        p!(candidate.id, assessment.plan_hash, epoch),
    )
    .await?;
    audit!(
        tx,
        "replica_ready",
        request.project_id,
        &candidate.id,
        pointer.version.as_deref(),
        Some(&candidate.version),
        &candidate.source_hash,
        epoch
    )
    .await?;
    let audit = audit!(
        tx,
        "activated",
        request.project_id,
        &candidate.id,
        pointer.version.as_deref(),
        Some(&candidate.version),
        &candidate.source_hash,
        "test_scope"
    )
    .await?;
    Ok(stamped(
        "activated",
        None,
        json!({
            "state": "active",
            "audit_id": audit,
            "pointer_version": candidate.version,
            "previous_version": pointer.version,
            "source_hash": candidate.source_hash,
            "pack_digest": candidate.pack_digest,
            "manifest_sha256": candidate.manifest_sha256,
            "half_published": false,
            "database_used": true,
        }),
        "test_scope_not_production",
    ))
}

async fn recover_body(
    tx: &Transaction<'_>,
    request: &RecoverRequest<'_>,
) -> Result<Receipt, StoreError> {
    require_role(tx).await?;
    lock_install(tx).await?;
    let deployment = read_deployment(tx).await?;
    if deployment.project != request.project_id {
        return Ok(refused("project_binding", json!({})));
    }
    let pointer = lock_pointer(tx).await?;
    let Some(candidate) = read_candidate(tx, request.candidate_id, true).await? else {
        return Ok(refused("activation", json!({"check": "candidate"})));
    };
    match request.action {
        "recover" => {
            if pointer.candidate_id.as_deref() == Some(candidate.id.as_str()) {
                return Ok(refused("seal_conflict", json!({"state": candidate.state})));
            }
            if candidate.state == "failed" {
                return Ok(stamped(
                    "replay",
                    None,
                    json!({"state": "failed", "pointer_version": pointer.version, "database_used": true}),
                    "not_issued",
                ));
            }
            if candidate.state != "applying" {
                return Ok(refused("seal_conflict", json!({"state": candidate.state})));
            }
            one(
                tx,
                sql::UPDATE_STATE,
                p!(candidate.id, "failed", "applying"),
            )
            .await?;
            audit!(
                tx,
                "recovering",
                request.project_id,
                &candidate.id,
                pointer.version.as_deref(),
                None,
                &candidate.source_hash,
                "crash"
            )
            .await?;
            audit!(
                tx,
                "recovered",
                request.project_id,
                &candidate.id,
                pointer.version.as_deref(),
                None,
                &candidate.source_hash,
                "failed"
            )
            .await?;
            Ok(stamped(
                "recovered",
                None,
                json!({
                    "state": "failed",
                    "pointer_version": pointer.version,
                    "half_published": false,
                    "database_used": true,
                }),
                "not_issued",
            ))
        }
        "abort" | "retire" => {
            let target = if request.action == "abort" {
                "aborted"
            } else {
                "retired"
            };
            if candidate.state == target {
                return Ok(stamped(
                    "replay",
                    None,
                    json!({"state": target, "pointer_version": pointer.version, "database_used": true}),
                    "not_issued",
                ));
            }
            if matches!(candidate.state.as_str(), "active" | "inactive")
                || pointer.candidate_id.as_deref() == Some(candidate.id.as_str())
            {
                return Ok(refused("seal_conflict", json!({"state": candidate.state})));
            }
            if !matches!(
                candidate.state.as_str(),
                "candidate_built" | "gates_passed" | "applying" | "failed"
            ) {
                return Ok(refused("seal_conflict", json!({"state": candidate.state})));
            }
            one(
                tx,
                sql::UPDATE_STATE,
                p!(candidate.id, target, candidate.state),
            )
            .await?;
            let event = if request.action == "abort" {
                "aborted"
            } else {
                "retired"
            };
            audit!(
                tx,
                event,
                request.project_id,
                &candidate.id,
                pointer.version.as_deref(),
                None,
                &candidate.source_hash,
                target
            )
            .await?;
            Ok(stamped(
                event,
                None,
                json!({"state": target, "pointer_version": pointer.version, "database_used": true}),
                "not_issued",
            ))
        }
        "rollback" => rollback_body(tx, request, &candidate, &pointer).await,
        _ => Err(StoreError::Input),
    }
}

async fn rollback_body(
    tx: &Transaction<'_>,
    request: &RecoverRequest<'_>,
    candidate: &Candidate,
    pointer: &Pointer,
) -> Result<Receipt, StoreError> {
    if candidate.state == "inactive" && pointer.version.as_deref() == candidate.base.as_deref() {
        return Ok(stamped(
            "replay",
            None,
            json!({
                "state": "inactive",
                "pointer_version": pointer.version,
                "database_used": true,
            }),
            "not_issued",
        ));
    }
    if candidate.state != "active"
        || pointer.candidate_id.as_deref() != Some(candidate.id.as_str())
        || pointer.version.as_deref() != Some(candidate.version.as_str())
    {
        return Ok(refused("seal_conflict", json!({"state": candidate.state})));
    }
    if request.expected_version != Some(candidate.version.as_str()) {
        return Ok(refused("cas_conflict", json!({})));
    }
    let target = if let Some(base) = &candidate.base {
        let row = tx
            .query_opt(sql::READ_CANDIDATE_VERSION, &[&base])
            .await
            .map_err(query_error)?;
        let Some(row) = row else {
            return Ok(refused("activation", json!({"check": "base"})));
        };
        let base_id: String = row
            .try_get(0)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        let base_state: String = row
            .try_get(1)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        let base_source: String = row
            .try_get(2)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        let base_pack: String = row
            .try_get(3)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        let base_manifest: String = row
            .try_get(4)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        if base_state != "inactive" {
            return Ok(refused("seal_conflict", json!({"state": base_state})));
        }
        one(tx, sql::UPDATE_STATE, p!(base_id, "active", "inactive")).await?;
        RollbackTarget {
            version: Some(base.clone()),
            id: Some(base_id),
            source: Some(base_source),
            pack: Some(base_pack),
            manifest: Some(base_manifest),
        }
    } else {
        RollbackTarget {
            version: None,
            id: None,
            source: None,
            pack: None,
            manifest: None,
        }
    };
    let expected = Some(candidate.version.as_str());
    let updated = tx
        .execute(
            sql::UPDATE_POINTER,
            p!(
                target.version,
                target.id,
                target.source,
                target.pack,
                target.manifest,
                request.project_id,
                expected
            ),
        )
        .await
        .map_err(query_error)?;
    if updated != 1 {
        return Ok(refused("cas_conflict", json!({})));
    }
    one(
        tx,
        sql::UPDATE_STATE,
        p!(candidate.id, "inactive", "active"),
    )
    .await?;
    audit!(
        tx,
        "rolled_back",
        request.project_id,
        &candidate.id,
        Some(&candidate.version),
        target.version.as_deref(),
        &candidate.source_hash,
        "pointer"
    )
    .await?;
    Ok(stamped(
        "rolled_back",
        None,
        json!({
            "state": "inactive",
            "pointer_version": target.version,
            "pack_digest": candidate.pack_digest,
            "manifest_sha256": candidate.manifest_sha256,
            "half_published": false,
            "database_used": true,
        }),
        "not_issued",
    ))
}

async fn status_body(
    tx: &Transaction<'_>,
    request: &PlanRequest<'_>,
) -> Result<Receipt, StoreError> {
    require_role(tx).await?;
    let deployment = read_deployment(tx).await?;
    if deployment.project != request.project_id {
        return Ok(refused("project_binding", json!({})));
    }
    let pointer = read_pointer(tx).await?;
    let Some(candidate) = read_candidate(tx, request.candidate_id, false).await? else {
        return Ok(refused("activation", json!({"check": "candidate"})));
    };
    if candidate.source_hash != request.source_hash {
        return Ok(refused("stale", json!({"state": candidate.state})));
    }
    let assessment = assess(
        tx,
        &candidate,
        candidate.base.as_deref(),
        &deployment,
        &pointer,
    )
    .await?;
    let files = tx
        .query(sql::READ_FILES, &[&candidate.id])
        .await
        .map_err(query_error)?;
    let mut listed = Vec::new();
    for row in &files {
        let path: String = row
            .try_get(0)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        let sha: String = row
            .try_get(1)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        let bytes: i32 = row
            .try_get(2)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        if path.contains("..") || path.starts_with('/') || path.contains('\\') {
            return Ok(refused("path", json!({})));
        }
        listed.push(json!({"path": path, "sha256": sha, "bytes": bytes}));
    }
    let pointer_is_candidate = pointer.candidate_id.as_deref() == Some(candidate.id.as_str());
    Ok(stamped(
        "status",
        None,
        json!({
            "state": candidate.state,
            "source_hash": candidate.source_hash,
            "pack_digest": candidate.pack_digest,
            "manifest_sha256": candidate.manifest_sha256,
            "role": candidate.role,
            "plan_scope": candidate.plan_scope,
            "plan_epoch": candidate.plan_epoch,
            "pointer_version": pointer.version,
            "pointer_is_candidate": pointer_is_candidate,
            "half_published": false,
            "gates": assessment.gates,
            "test_reasons": assessment.test_reasons,
            "production_reasons": assessment.production_reasons,
            "production_blocked": !assessment.production_reasons.is_empty(),
            "files": listed,
            "database_used": true,
        }),
        "not_issued",
    ))
}

pub(crate) async fn overlay_maintenance(
    tx: &Transaction<'_>,
    candidate: &str,
    gates: &mut [Summary],
) -> Result<Vec<String>, StoreError> {
    let present = tx
        .query_opt(sql::MAINTENANCE_TABLE, &[])
        .await
        .map_err(query_error)?;
    if present.is_none() {
        return Ok(Vec::new());
    }
    let rows = tx
        .query(sql::READ_MAINTENANCE_GATES, &[&candidate])
        .await
        .map_err(query_error)?;
    let mut evidence = Vec::new();
    for row in rows {
        let gate_id: String = row
            .try_get(0)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        let disposition: String = row
            .try_get(1)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        let hash: String = row
            .try_get(2)
            .map_err(|_| StoreError::Query { sqlstate: None })?;
        let Some(summary) = gates.iter_mut().find(|item| item.gate_id == gate_id) else {
            continue;
        };
        if overlay_one(summary, &disposition) {
            evidence.push(hash);
        }
    }
    Ok(evidence)
}

pub(crate) fn stamped(
    outcome: &str,
    reason: Option<&str>,
    mut body: Value,
    certificate_class: &str,
) -> Receipt {
    body["outcome"] = json!(outcome);
    body["reason"] = json!(reason);
    body["contract"] = json!(CONTRACT);
    body["full_sp08_accepted"] = json!(FULL_SP08_ACCEPTED);
    body["public_deployment_claimed"] = json!(false);
    body["customer_ownership_claimed"] = json!(false);
    body["standby_configured"] = json!(false);
    body["production_ready_certificate"] = json!(PRODUCTION_READY_CERTIFICATE);
    body["test_mode_cannot_produce_production_certificate"] =
        json!(TEST_MODE_CANNOT_PRODUCE_PRODUCTION_CERTIFICATE);
    body["owner_authenticated"] = json!(false);
    body["credential_verified"] = json!(false);
    body["integrity_verified"] = json!(false);
    body["customer_gate_verified"] = json!(false);
    body["runsc_verified"] = json!(false);
    body["sessions_terminated_by_tool"] = json!(false);
    body["certificate_class"] = json!(certificate_class);
    Receipt {
        code: if outcome == "refused" { 2 } else { 0 },
        body,
    }
}

pub(crate) fn refused(reason: &'static str, extra: Value) -> Receipt {
    let mut body = extra;
    if !body.is_object() {
        body = json!({});
    }
    stamped("refused", Some(reason), body, "not_issued")
}

struct Audit<'a> {
    event: &'a str,
    project: &'a str,
    candidate: &'a str,
    previous: Option<&'a str>,
    new_version: Option<&'a str>,
    source: &'a str,
    detail: &'a str,
}

async fn insert_audit(tx: &Transaction<'_>, audit: Audit<'_>) -> Result<String, StoreError> {
    let Audit {
        event,
        project,
        candidate,
        previous,
        new_version,
        source,
        detail,
    } = audit;
    let id = canonical_sha(&json!({
        "event": event,
        "project_id": project,
        "candidate_id": candidate,
        "previous_version": previous,
        "new_version": new_version,
        "source_hash": source,
        "detail": detail,
    }));
    if tx
        .query_opt(sql::READ_AUDIT, &[&id])
        .await
        .map_err(query_error)?
        .is_some()
    {
        return Ok(id);
    }
    one(
        tx,
        sql::INSERT_AUDIT,
        p!(
            id,
            project,
            candidate,
            event,
            previous,
            new_version,
            source,
            detail
        ),
    )
    .await?;
    Ok(id)
}

async fn event_audit(
    tx: &Transaction<'_>,
    candidate: &str,
    event: &str,
) -> Result<String, StoreError> {
    let rows = tx
        .query(sql::READ_EVENT_AUDIT, p!(candidate, event))
        .await
        .map_err(query_error)?;
    if rows.len() != 1 {
        return Err(StoreError::Query { sqlstate: None });
    }
    rows[0]
        .try_get(0)
        .map_err(|_| StoreError::Query { sqlstate: None })
}

async fn clear_plan(tx: &Transaction<'_>, candidate: &str) -> Result<(), StoreError> {
    let empty: Option<String> = None;
    one(
        tx,
        sql::UPDATE_PLAN,
        p!(
            candidate,
            "candidate_built",
            empty,
            empty,
            empty,
            "gates_passed"
        ),
    )
    .await
}

async fn read_deployment(tx: &Transaction<'_>) -> Result<Deployment, StoreError> {
    let row = tx
        .query_opt(sql::READ_DEPLOYMENT, &[])
        .await
        .map_err(query_error)?;
    let Some(row) = row else {
        return Err(StoreError::Query { sqlstate: None });
    };
    let project: String = row
        .try_get(0)
        .map_err(|_| StoreError::Query { sqlstate: None })?;
    let authority_sha: String = row
        .try_get(2)
        .map_err(|_| StoreError::Query { sqlstate: None })?;
    let product: bool = row
        .try_get(3)
        .map_err(|_| StoreError::Query { sqlstate: None })?;
    let owner: bool = row
        .try_get(4)
        .map_err(|_| StoreError::Query { sqlstate: None })?;
    let credential: bool = row
        .try_get(5)
        .map_err(|_| StoreError::Query { sqlstate: None })?;
    let integrity: bool = row
        .try_get(6)
        .map_err(|_| StoreError::Query { sqlstate: None })?;
    let customer: bool = row
        .try_get(7)
        .map_err(|_| StoreError::Query { sqlstate: None })?;
    let runsc: bool = row
        .try_get(8)
        .map_err(|_| StoreError::Query { sqlstate: None })?;
    let accepted: bool = row
        .try_get(9)
        .map_err(|_| StoreError::Query { sqlstate: None })?;
    if product || owner || credential || integrity || customer || runsc || accepted {
        return Err(StoreError::Query { sqlstate: None });
    }
    let rows = tx
        .query(sql::READ_AUTHORITY_GATES, &[])
        .await
        .map_err(query_error)?;
    let mut covers = Vec::new();
    for row in rows {
        covers.push(
            row.try_get(0)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
        );
    }
    Ok(Deployment {
        project,
        authority_sha,
        covers,
    })
}

async fn read_candidate(
    tx: &Transaction<'_>,
    id: &str,
    lock: bool,
) -> Result<Option<Candidate>, StoreError> {
    let sql = if lock {
        sql::READ_CANDIDATE_LOCK
    } else {
        sql::READ_CANDIDATE
    };
    let row = tx.query_opt(sql, &[&id]).await.map_err(query_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(Candidate {
        id: row
            .try_get(0)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        project: row
            .try_get(1)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        version: row
            .try_get(2)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        base: row
            .try_get(3)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        state: row
            .try_get(4)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        source_hash: row
            .try_get(5)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        effective: row
            .try_get(6)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        visibility: row
            .try_get(7)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        pack_digest: row
            .try_get(10)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        manifest_sha256: row
            .try_get(11)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        role: row
            .try_get(18)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        plan_hash: row
            .try_get(20)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        plan_scope: row
            .try_get(21)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        plan_epoch: row
            .try_get(22)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
    }))
}

fn pointer_from(row: &tokio_postgres::Row) -> Result<Pointer, StoreError> {
    Ok(Pointer {
        version: row
            .try_get(0)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
        candidate_id: row
            .try_get(1)
            .map_err(|_| StoreError::Query { sqlstate: None })?,
    })
}

async fn lock_pointer(tx: &Transaction<'_>) -> Result<Pointer, StoreError> {
    let row = tx
        .query_one(sql::LOCK_POINTER, &[])
        .await
        .map_err(query_error)?;
    pointer_from(&row)
}

async fn read_pointer(tx: &Transaction<'_>) -> Result<Pointer, StoreError> {
    let row = tx
        .query_one(sql::READ_POINTER, &[])
        .await
        .map_err(query_error)?;
    pointer_from(&row)
}

async fn read_gates(tx: &Transaction<'_>, id: &str) -> Result<Vec<GateFact>, StoreError> {
    let rows = tx
        .query(sql::READ_GATES, &[&id])
        .await
        .map_err(query_error)?;
    let mut facts = Vec::new();
    for row in rows {
        facts.push(GateFact {
            certificate_sha256: row
                .try_get(0)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
            gate_id: row
                .try_get(1)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
            source_hash: row
                .try_get(2)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
            issuer_class: row
                .try_get(3)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
            status: row
                .try_get(4)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
            blocking: row
                .try_get(5)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
            scope: row
                .try_get(6)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
            report_type: row
                .try_get(7)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
            authority_statement_sha256: row
                .try_get(8)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
        });
    }
    Ok(facts)
}

async fn read_checkpoints(
    tx: &Transaction<'_>,
    id: &str,
) -> Result<Vec<CheckpointFact>, StoreError> {
    let rows = tx
        .query(sql::READ_CHECKPOINTS, &[&id])
        .await
        .map_err(query_error)?;
    let mut facts = Vec::new();
    for row in rows {
        facts.push(CheckpointFact {
            kind: row
                .try_get(0)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
            certificate_sha256: row
                .try_get(1)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
            source_hash: row
                .try_get(2)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
            epoch: row
                .try_get(3)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
            barrier: row
                .try_get(4)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
            sessions_terminated: row
                .try_get(5)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
            issuer_class: row
                .try_get(6)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
            authority_statement_sha256: row
                .try_get(7)
                .map_err(|_| StoreError::Query { sqlstate: None })?,
        });
    }
    Ok(facts)
}

async fn require_role(tx: &Transaction<'_>) -> Result<(), StoreError> {
    let user = one_text(tx, sql::CURRENT_USER).await?;
    if user == ADMIN_ROLE {
        Ok(())
    } else {
        Err(StoreError::LoginRole)
    }
}

async fn lock_install(tx: &Transaction<'_>) -> Result<(), StoreError> {
    tx.execute(sql::LOCK_INSTALL, p!(INSTALL_LOCK))
        .await
        .map_err(query_error)?;
    Ok(())
}

async fn one(
    tx: &Transaction<'_>,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<(), StoreError> {
    let count = tx.execute(sql, params).await.map_err(query_error)?;
    if count == 1 {
        Ok(())
    } else {
        Err(StoreError::Query { sqlstate: None })
    }
}

async fn one_text(tx: &Transaction<'_>, sql: &str) -> Result<String, StoreError> {
    let row = tx.query_one(sql, &[]).await.map_err(query_error)?;
    row.try_get(0)
        .map_err(|_| StoreError::Query { sqlstate: None })
}

pub(crate) async fn open(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
) -> Result<Session, StoreError> {
    let tls = rustls_connector(ca_pem)?;
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(password);
    config.host(&endpoint.host);
    config.port(endpoint.port);
    config.dbname(&endpoint.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("release-coordinator");
    config.connect_timeout(Duration::from_secs(10));
    let (client, connection) = config.connect(tls).await.map_err(|_| StoreError::Connect)?;
    let _connection = tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(Session {
        client,
        _connection,
    })
}

pub(crate) async fn require_tls(client: &Client) -> Result<(), StoreError> {
    let row = client
        .query_one(sql::TLS_FACT, &[])
        .await
        .map_err(query_error)?;
    let ssl: bool = row.try_get(0).map_err(|_| StoreError::Tls)?;
    let version: Option<String> = row.try_get(1).map_err(|_| StoreError::Tls)?;
    match version {
        Some(version) if ssl && version.starts_with("TLSv1.") => Ok(()),
        _ => Err(StoreError::Tls),
    }
}

pub(crate) async fn begin(session: &mut Session) -> Result<Transaction<'_>, StoreError> {
    begin_for(session, "10s", "15s").await
}

pub(crate) async fn begin_for<'a>(
    session: &'a mut Session,
    lock_timeout: &str,
    statement_timeout: &str,
) -> Result<Transaction<'a>, StoreError> {
    let local = match (lock_timeout, statement_timeout) {
        ("10s", "15s") => sql::SET_LOCAL.to_owned(),
        ("60s", "60s") => "\
SET LOCAL search_path TO pg_catalog; \
SET LOCAL lock_timeout = '60s'; \
SET LOCAL statement_timeout = '60s'; \
SET LOCAL synchronous_commit = on"
            .to_owned(),
        _ => return Err(StoreError::Input),
    };
    let tx = session.client.transaction().await.map_err(query_error)?;
    if let Err(err) = tx.batch_execute(&local).await {
        return Err(rollback(tx, query_error(err)).await);
    }
    Ok(tx)
}

pub(crate) async fn rollback(tx: Transaction<'_>, err: StoreError) -> StoreError {
    let _ = tx.rollback().await;
    err
}

pub(crate) async fn commit(tx: Transaction<'_>, receipt: Receipt) -> Result<Receipt, StoreError> {
    match tx.commit().await {
        Ok(()) => Ok(receipt),
        Err(err) => {
            let mapped = query_error(err);
            if matches!(mapped, StoreError::Query { sqlstate: Some(_) }) {
                Err(mapped)
            } else {
                Err(StoreError::CommitUnknown)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{blocks_test_scope, overlay_one};
    use crate::decide::Summary;

    fn summary(disposition: &str, admissible: bool) -> Summary {
        Summary {
            gate_id: "G5a".to_owned(),
            disposition: disposition.to_owned(),
            test_admissible: admissible,
            production_admissible: false,
        }
    }

    #[test]
    fn not_applicable_does_not_cover_a_failed_gate_or_become_admissible() {
        let mut failed = summary("failed", false);
        assert!(!overlay_one(&mut failed, "not_applicable"));
        assert_eq!(failed.disposition, "failed");
        assert!(!failed.test_admissible);
        let mut missing = summary("missing", false);
        assert!(overlay_one(&mut missing, "not_applicable"));
        assert_eq!(missing.disposition, "not_applicable");
        assert!(!missing.test_admissible);
        assert!(!blocks_test_scope(&missing));
        assert!(blocks_test_scope(&failed));
        let mut blocked = summary("missing", false);
        assert!(!overlay_one(&mut blocked, "blocked"));
        assert_eq!(blocked.disposition, "blocked");
        assert!(blocks_test_scope(&blocked));
        let g7_missing = Summary {
            gate_id: "G7".to_owned(),
            disposition: "missing".to_owned(),
            test_admissible: false,
            production_admissible: false,
        };
        assert!(!blocks_test_scope(&g7_missing));
        let g7_failed = Summary {
            gate_id: "G7".to_owned(),
            disposition: "failed".to_owned(),
            test_admissible: false,
            production_admissible: false,
        };
        assert!(blocks_test_scope(&g7_failed));
        let g9_unmeasured = Summary {
            gate_id: "G9".to_owned(),
            disposition: "not_run".to_owned(),
            test_admissible: false,
            production_admissible: false,
        };
        assert!(blocks_test_scope(&g9_unmeasured));
        let g3_unmeasured = Summary {
            gate_id: "G3".to_owned(),
            disposition: "not_run".to_owned(),
            test_admissible: false,
            production_admissible: false,
        };
        assert!(!blocks_test_scope(&g3_unmeasured));
    }
}
