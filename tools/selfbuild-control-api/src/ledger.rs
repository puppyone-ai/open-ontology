//! Server-built review-ledger calls. The child stays unauthenticated.
//!
//! The control file and credential path are reconstructed from the operator
//! principal that matches the verified subject. Caller role JSON is ignored.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use zeroize::Zeroize;

use crate::canonical::sha256_hex;
use crate::config::{AppConfig, Endpoint, PrincipalCfg};
use crate::error::ApiError;
use crate::membership::Member;
use crate::spawn::{run_child, ChildSpec};
use crate::stages::accept_ledger;
use crate::store::{self, BuildRow};
use crate::worker::utc_date;
use crate::CONTRACT;

const CANDIDATE_PROTOCOL: &str = "oo.builder.description-candidate/v1";

pub struct DecisionIn<'a> {
    pub operation: &'a str,
    pub subject: &'a str,
    pub identity_revision: i64,
    pub build: &'a BuildRow,
    pub member: &'a Member,
    pub expected_revision: Option<i64>,
    pub idempotency_key: &'a str,
    pub decision_kind: &'a str,
    pub choice: &'a str,
    pub fields: Option<Value>,
    pub body_sha256: &'a str,
}

pub async fn decide(config: &AppConfig, input: DecisionIn<'_>) -> Result<Value, ApiError> {
    let principal = principal_for(config, input.subject, &input.member.object_id)?;
    if let Some(existing) = store::find_decision(&config.postgres, &input.build.project_id, input.idempotency_key).await? {
        if existing.body_sha256 == input.body_sha256 {
            return Ok(result(input.operation, "replay", None));
        }
        return Err(ApiError::idempotency());
    }
    let control = ensure_control(config, &input.build.project_id)?;
    let credential = read_secret(&principal.credential_file)?;
    let password = read_secret(&config.ledger.password_file)?;
    let needles = NeedleSet(vec![credential.bytes.clone(), password.bytes.clone()]);
    let request_dir = PathBuf::from(&config.work_dir)
        .join("ledger")
        .join(&input.build.project_id)
        .join("requests");
    std::fs::create_dir_all(&request_dir).map_err(|_| ApiError::not_ready())?;
    run_json(
        &config.binaries.review_ledger,
        "install",
        &json!({
            "protocol": "oo.review-ledger.install/v1",
            "project_id": &input.build.project_id,
            "control_file": &control,
            "postgres": postgres(&config.ledger)
        }),
        &request_dir,
        needles.copy(),
    )
    .await?;
    let head = store::queue_head(&config.ledger, &input.build.project_id, &input.member.object_id).await?;
    let revision = match (&head, input.expected_revision) {
        (None, Some(expected)) if expected != 1 => return Err(ApiError::revision()),
        (None, _) => 1,
        (Some(head), Some(expected)) if expected != head.revision => return Err(ApiError::revision()),
        (Some(head), _) => {
            if !same_binding(head, input.member, &input.build.input_sha256) {
                return Err(ApiError::membership());
            }
            head.revision
        }
    };
    let bound = binding_json(input.member, &input.build.input_sha256);
    let mut enqueue = json!({
        "protocol": "oo.review-ledger.enqueue/v1",
        "project_id": &input.build.project_id,
        "control_file": &control,
        "credential_file": &principal.credential_file,
        "postgres": postgres(&config.ledger),
        "object_kind": &input.member.object_kind,
        "object_id": &input.member.object_id,
        "candidate_sha256": bound["candidate_sha256"],
        "input_sha256": bound["input_sha256"],
        "evidence_sha256": bound["evidence_sha256"],
        "schema_sha256": bound["schema_sha256"],
        "curated_sha256": bound["curated_sha256"],
        "source_sha256": bound["source_sha256"],
        "candidate_protocol": CANDIDATE_PROTOCOL,
        "owner_label": &principal.reviewer_label
    });
    if head.is_some() {
        enqueue["expected_revision"] = json!(revision);
    }
    run_json(
        &config.binaries.review_ledger,
        "enqueue",
        &enqueue,
        &request_dir,
        needles.copy(),
    )
    .await?;
    let mut decide_body = json!({
        "protocol": "oo.review-ledger.decide/v1",
        "project_id": &input.build.project_id,
        "control_file": &control,
        "credential_file": &principal.credential_file,
        "postgres": postgres(&config.ledger),
        "object_id": &input.member.object_id,
        "expected_revision": revision,
        "idempotency_key": input.idempotency_key,
        "decision_kind": input.decision_kind,
        "choice": input.choice,
        "reviewer_label": &principal.reviewer_label,
        "reviewer_at": utc_date(),
        "candidate_sha256": bound["candidate_sha256"],
        "input_sha256": bound["input_sha256"],
        "evidence_sha256": bound["evidence_sha256"],
        "schema_sha256": bound["schema_sha256"],
        "curated_sha256": bound["curated_sha256"],
        "source_sha256": bound["source_sha256"]
    });
    if let Some(fields) = &input.fields {
        decide_body["fields"] = fields.clone();
    }
    let outcome = run_json(
        &config.binaries.review_ledger,
        "decide",
        &decide_body,
        &request_dir,
        needles.copy(),
    )
    .await?;
    let code = outcome.get("code").and_then(Value::as_str).unwrap_or("");
    if !matches!(code, "persisted" | "replay") {
        return Err(map_code(code));
    }
    let stored_revision = outcome.get("revision").and_then(Value::as_i64).unwrap_or(revision);
    store::insert_decision(
        &config.postgres,
        &store::DecisionWrite {
            project_id: input.build.project_id.clone(),
            idempotency_key: input.idempotency_key.to_owned(),
            build_id: input.build.build_id.clone(),
            object_id: input.member.object_id.clone(),
            decision_kind: input.decision_kind.to_owned(),
            choice: input.choice.to_owned(),
            body_sha256: input.body_sha256.to_owned(),
            identity_revision: input.identity_revision,
            subject: input.subject.to_owned(),
            ledger_code: code.to_owned(),
        },
    )
    .await?;
    if input.decision_kind == "label_description" {
        let _ = store::note_label_stage(&config.postgres, input.build).await;
    }
    Ok(result(input.operation, code, Some(stored_revision)))
}

fn same_binding(head: &store::QueueHead, member: &Member, input_sha256: &str) -> bool {
    head.candidate_sha256 == member.candidate_sha256
        && head.input_sha256 == input_sha256
        && head.evidence_sha256 == member.evidence_sha256
        && head.schema_sha256 == member.schema_sha256
        && head.curated_sha256 == member.generation_sha256
        && head.source_sha256 == member.source_sha256
}

fn binding_json(member: &Member, input_sha256: &str) -> Value {
    json!({
        "candidate_sha256": member.candidate_sha256,
        "input_sha256": input_sha256,
        "evidence_sha256": member.evidence_sha256,
        "schema_sha256": member.schema_sha256,
        "curated_sha256": member.generation_sha256,
        "source_sha256": member.source_sha256
    })
}

fn principal_for<'a>(config: &'a AppConfig, subject: &str, object_id: &str) -> Result<&'a PrincipalCfg, ApiError> {
    let principal = config
        .principals
        .iter()
        .find(|item| item.subject == subject)
        .ok_or_else(ApiError::forbidden)?;
    if principal.ledger_role == "auditor" || !matches!(principal.ledger_role.as_str(), "reviewer" | "owner") {
        return Err(ApiError::forbidden());
    }
    if !principal.object_scope.iter().any(|item| item == object_id) {
        return Err(ApiError::forbidden());
    }
    Ok(principal)
}

fn ensure_control(config: &AppConfig, project_id: &str) -> Result<String, ApiError> {
    if config.principals.is_empty() || config.principals.len() > 16 {
        return Err(ApiError::not_ready());
    }
    let mut listed = Vec::new();
    for principal in &config.principals {
        let bytes = read_secret(&principal.credential_file)?;
        listed.push(json!({
            "principal_id": &principal.principal_id,
            "role": &principal.ledger_role,
            "reviewer_label": &principal.reviewer_label,
            "credential_sha256": sha256_hex(&bytes.bytes),
            "scope": &principal.object_scope
        }));
    }
    let body = serde_json::to_vec(&json!({
        "protocol": "oo.review-ledger.control/v1",
        "project_id": project_id,
        "authority": "not_verified",
        "management_identity": "not_claimed",
        "principals": listed
    }))
    .map_err(|_| ApiError::not_ready())?;
    let path = PathBuf::from(&config.work_dir).join("ledger").join(project_id).join("control.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| ApiError::not_ready())?;
    }
    if path.is_file() {
        let existing = std::fs::read(&path).map_err(|_| ApiError::not_ready())?;
        if existing != body {
            return Err(ApiError::membership());
        }
        return Ok(path.display().to_string());
    }
    write_private(&path, &body)?;
    Ok(path.display().to_string())
}

fn postgres(endpoint: &Endpoint) -> Value {
    json!({
        "host": endpoint.host,
        "port": endpoint.port,
        "database": endpoint.database,
        "user": endpoint.user,
        "password_file": endpoint.password_file,
        "ca_file": endpoint.ca_file,
        "server_name": endpoint.server_name
    })
}

async fn run_json(
    binary: &str,
    command: &str,
    body: &Value,
    dir: &std::path::Path,
    needles: Vec<Vec<u8>>,
) -> Result<Value, ApiError> {
    if !crate::config::regular_file(binary) {
        return Err(ApiError::not_ready());
    }
    let path = dir.join(format!("{command}-{}.json", sha256_hex(body.to_string().as_bytes())));
    let bytes = serde_json::to_vec(body).map_err(|_| ApiError::not_ready())?;
    write_private(&path, &bytes)?;
    let output = run_child(ChildSpec {
        binary: binary.to_owned(),
        args: vec![command.to_owned(), path.display().to_string()],
        stdin: Vec::new(),
        deadline: Duration::from_secs(30),
        needles,
        cancel: Arc::new(AtomicBool::new(false)),
        pid_out: Arc::new(AtomicU32::new(0)),
    })
    .await;
    if output.secret_leak || !output.child_exited || output.killed {
        return Err(ApiError::not_ready());
    }
    if output.exit == Some(3) {
        return Err(ApiError::commit_unknown());
    }
    let value = accept_ledger(&output.stdout)?;
    let code = value.get("code").and_then(Value::as_str).unwrap_or("");
    if matches!(code, "persisted" | "replay" | "installed" | "history") {
        Ok(value)
    } else {
        Err(map_code(code))
    }
}

fn map_code(code: &str) -> ApiError {
    match code {
        "revision.conflict" => ApiError::revision(),
        "idempotency.conflict" => ApiError::idempotency(),
        "pii.unknown" => ApiError::pii_unknown(),
        "batch.accept_all" => ApiError::accept_all(),
        "commit.unknown" => ApiError::commit_unknown(),
        "role.forbidden" | "scope.unauthorized" | "credential.mismatch" | "reviewer.mismatch" | "reviewer_not_owner" => {
            ApiError::forbidden()
        }
        "schema.invalid" | "reviewer.date" | "protocol.mismatch" => ApiError::malformed(),
        "decision.absent" | "candidate.changed" | "stale" | "install.drift" | "control.drift" | "project.mismatch" => {
            ApiError::membership()
        }
        _ => ApiError::not_ready(),
    }
}

fn result(operation: &str, code: &str, revision: Option<i64>) -> Value {
    let mut value = json!({
        "contract": CONTRACT,
        "operation": operation,
        "code": code,
        "ledger_authenticated": false,
        "ledger_management_identity": "not_claimed",
        "management_identity": "verified",
        "published": false,
        "data_grants": "none",
        "grants": "not_authorized"
    });
    if let Some(revision) = revision {
        value["revision"] = json!(revision);
    }
    value
}

struct NeedleSet(Vec<Vec<u8>>);

impl NeedleSet {
    fn copy(&self) -> Vec<Vec<u8>> {
        self.0.clone()
    }
}

impl Drop for NeedleSet {
    fn drop(&mut self) {
        for item in &mut self.0 {
            item.zeroize();
        }
    }
}

struct Secret {
    bytes: Vec<u8>,
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

fn read_secret(path: &str) -> Result<Secret, ApiError> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| ApiError::not_ready())?;
    if !meta.file_type().is_file() || meta.len() > 4096 {
        return Err(ApiError::not_ready());
    }
    Ok(Secret {
        bytes: std::fs::read(path).map_err(|_| ApiError::not_ready())?,
    })
}

fn write_private(path: &std::path::Path, bytes: &[u8]) -> Result<(), ApiError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| ApiError::not_ready())?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| ApiError::not_ready())?;
    file.write_all(bytes).map_err(|_| ApiError::not_ready())
}
