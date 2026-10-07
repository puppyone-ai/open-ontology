//! Private PostgreSQL persistence. A decision and its audit event commit together.
//! The compiler runs only on the later receipt path, outside that transaction.

use std::fs;
use std::path::Path;

use serde_json::json;
use tokio_postgres::Transaction;

use crate::canonical::{canonical_sha, sha256_hex};
use crate::compiler::{run_pack, run_review};
use crate::control::{
    authenticate, civil_date_ok, hex_ok, ident_ok, in_scope, key_ok, kind_matches, label_ok,
    object_id_ok, parse_control, principal_ok, Control, Principal,
};
use crate::error::{err, LedgerError};
use crate::fields::{self, Prepared};
use crate::model::{
    BatchItem, BatchRequest, DecideRequest, Endpoint, EnqueueRequest, HistoryRequest, InstallRequest,
    ItemOut, Output, ReceiptRequest,
};
use crate::sql::{
    self, COUNT_CLASSIFICATION, COUNT_DATASET_APPROVAL, INSERT_AUDIT, INSERT_BATCH, INSERT_BATCH_ITEM,
    INSERT_DECISION, INSERT_HEAD, INSERT_RECEIPT, INSERT_REVISION, LOCK_DECISION, LOCK_HEAD,
    READ_AUDITS, READ_DECISIONS, READ_HEADS, READ_MANIFEST, READ_RECEIPTS, SESSION, UPDATE_HEAD,
};
use crate::tls;

const CANDIDATE_PROTOCOLS: &[&str] = &[
    "oo.builder.description-candidate/v1",
    "oo.builder.description-candidate/v2",
];

struct Digests {
    candidate: String,
    input: String,
    evidence: String,
    schema: String,
    curated: String,
    source: String,
}

struct Head {
    revision: i64,
    candidate: String,
    input: String,
    evidence: String,
    schema: String,
    curated: String,
    source: String,
    owner_label: String,
}

struct StoredDecision {
    revision: i64,
    kind: String,
    choice: String,
    body: String,
    candidate: String,
    input: String,
    evidence: String,
    schema: String,
    source: String,
    object_id: String,
}

pub fn migration_sha256() -> Result<String, LedgerError> {
    sha256_hex(sql::MIGRATION_SQL.as_bytes())
}

pub async fn install(request: &InstallRequest) -> Result<Output, LedgerError> {
    if request.protocol != "oo.review-ledger.install/v1" {
        return Err(err("protocol.mismatch", "the request protocol is not this command"));
    }
    if !ident_ok(&request.project_id) {
        return Err(err("schema.invalid", "the project id is not a single identifier"));
    }
    let control_bytes = read_regular(&request.control_file, 65_536)?;
    let control = parse_control(&control_bytes)?;
    if control.project_id != request.project_id {
        return Err(err("project.mismatch", "the control project does not match the request"));
    }
    let sql_sha = migration_sha256()?;
    let mut client = connect(&request.postgres).await?;
    if let Some(existing) = read_manifest(&mut client).await? {
        if existing.sql != sql_sha || existing.project != request.project_id {
            return Err(err("install.drift", "the installed schema does not match this migration"));
        }
        if existing.control != control.sha256 {
            return Err(err("control.drift", "the control file does not match the install"));
        }
        return Ok(Output::bare("replay", "replay"));
    }
    let script = format!(
        "BEGIN;\n{}\nINSERT INTO oo_review_ledger.install_manifest (singleton, sql_sha256, project_id, control_sha256) VALUES (true, '{}', '{}', '{}');\nCOMMIT;\n",
        sql::MIGRATION_SQL,
        sql_sha,
        request.project_id,
        control.sha256
    );
    client
        .batch_execute(&script)
        .await
        .map_err(|_| err("database.refused", "the database refused the statement"))?;
    Ok(Output::bare("persisted", "installed"))
}

pub async fn enqueue(request: &EnqueueRequest) -> Result<Output, LedgerError> {
    if request.protocol != "oo.review-ledger.enqueue/v1" {
        return Err(err("protocol.mismatch", "the request protocol is not this command"));
    }
    let digests = digests(
        &request.candidate_sha256,
        &request.input_sha256,
        &request.evidence_sha256,
        &request.schema_sha256,
        &request.curated_sha256,
        &request.source_sha256,
    )?;
    if !CANDIDATE_PROTOCOLS.contains(&request.candidate_protocol.as_str()) {
        return Err(err("schema.invalid", "the candidate protocol is not a description candidate"));
    }
    if !object_id_ok(&request.object_id) || !kind_matches(&request.object_kind, &request.object_id) {
        return Err(err("schema.invalid", "the object id shape is wrong"));
    }
    if !label_ok(&request.owner_label) {
        return Err(err("schema.invalid", "the owner label is not allowed"));
    }
    let (control, principal) = open_caller(&request.project_id, &request.control_file, &request.credential_file, &request.postgres).await?;
    if principal.role == "auditor" {
        return Err(err("role.forbidden", "an auditor cannot write the review queue"));
    }
    if !in_scope(&principal, &request.object_id) {
        return Err(err("scope.unauthorized", "the object is outside the caller scope"));
    }
    let mut client = connect(&request.postgres).await?;
    require_control(&mut client, &request.project_id, &control.sha256).await?;
    let tx = client.transaction().await.map_err(map_db)?;
    let result = enqueue_tx(&tx, request, &principal, &digests).await;
    finish(tx, result).await
}

pub async fn decide(request: &DecideRequest) -> Result<Output, LedgerError> {
    if request.protocol != "oo.review-ledger.decide/v1" {
        return Err(err("protocol.mismatch", "the request protocol is not this command"));
    }
    let (control, principal) = open_caller(&request.project_id, &request.control_file, &request.credential_file, &request.postgres).await?;
    let mut client = connect(&request.postgres).await?;
    require_control(&mut client, &request.project_id, &control.sha256).await?;
    let tx = client.transaction().await.map_err(map_db)?;
    let result = decide_tx(&tx, request, &principal).await;
    finish(tx, result).await
}

pub async fn batch(request: &BatchRequest) -> Result<Output, LedgerError> {
    if request.protocol != "oo.review-ledger.batch/v1" {
        return Err(err("protocol.mismatch", "the request protocol is not this command"));
    }
    if request.accept_all {
        return Err(err("batch.accept_all", "a batch cannot accept every object"));
    }
    let batch_bytes = request.batch_id.as_bytes();
    if batch_bytes.is_empty()
        || batch_bytes.len() > 64
        || !batch_bytes[0].is_ascii_lowercase()
        || !batch_bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b':' | b'-')
        })
    {
        return Err(err("schema.invalid", "the batch id is not allowed"));
    }
    if request.items.is_empty() || request.items.len() > 8 {
        return Err(err("schema.invalid", "a batch needs one to eight objects"));
    }
    let (control, principal) = open_caller(&request.project_id, &request.control_file, &request.credential_file, &request.postgres).await?;
    let mut client = connect(&request.postgres).await?;
    require_control(&mut client, &request.project_id, &control.sha256).await?;
    if batch_exists(&mut client, &request.project_id, &request.batch_id).await? {
        let mut output = Output::bare("replay", "replay");
        output.batch_status = Some("replay".to_owned());
        return Ok(output);
    }
    let mut items = Vec::new();
    let mut refused = 0i32;
    for item in &request.items {
        let one = DecideRequest {
            protocol: "oo.review-ledger.decide/v1".to_owned(),
            project_id: request.project_id.clone(),
            control_file: request.control_file.clone(),
            credential_file: request.credential_file.clone(),
            postgres: endpoint_clone(&request.postgres),
            object_id: item.object_id.clone(),
            expected_revision: item.expected_revision,
            idempotency_key: item.idempotency_key.clone(),
            decision_kind: item.decision_kind.clone(),
            choice: item.choice.clone(),
            reviewer_label: item.reviewer_label.clone(),
            reviewer_at: item.reviewer_at.clone(),
            candidate_sha256: item.candidate_sha256.clone(),
            input_sha256: item.input_sha256.clone(),
            evidence_sha256: item.evidence_sha256.clone(),
            schema_sha256: item.schema_sha256.clone(),
            curated_sha256: item.curated_sha256.clone(),
            source_sha256: item.source_sha256.clone(),
            fields: item.fields.clone(),
        };
        let outcome = match decide(&one).await {
            Ok(output) => output,
            Err(error) => Output::from_error(&error),
        };
        let stored = matches!(outcome.code.as_str(), "persisted" | "replay" | "installed");
        if !stored {
            refused += 1;
        }
        items.push(ItemOut {
            object_id: item.object_id.clone(),
            code: outcome.code,
        });
    }
    let status = if refused == 0 { "complete" } else { "partial" };
    let tx = client.transaction().await.map_err(map_db)?;
    let result = record_batch(&tx, request, &principal, status, refused, &items).await;
    let mut output = finish(tx, result).await?;
    output.batch_status = Some(status.to_owned());
    output.items = items;
    output.kind = status.to_owned();
    output.code = status.to_owned();
    Ok(output)
}

pub async fn receipt(request: &ReceiptRequest) -> Result<Output, LedgerError> {
    if request.protocol != "oo.review-ledger.receipt/v1" {
        return Err(err("protocol.mismatch", "the request protocol is not this command"));
    }
    if request.compiler != "description-review" && request.compiler != "description-pack" {
        return Err(err("schema.invalid", "the compiler name is not a description compiler"));
    }
    let (control, principal) = open_caller(&request.project_id, &request.control_file, &request.credential_file, &request.postgres).await?;
    if principal.role == "auditor" {
        return Err(err("role.forbidden", "an auditor cannot write a compiler receipt"));
    }
    let mut client = connect(&request.postgres).await?;
    require_control(&mut client, &request.project_id, &control.sha256).await?;
    if request.compiler == "description-pack" {
        if request.required_columns.is_empty() || request.required_columns.len() > 8 {
            return Err(err("schema.invalid", "a pack receipt names one to eight columns"));
        }
        for column in &request.required_columns {
            if !object_id_ok(column) || !kind_matches("column", column) {
                return Err(err("schema.invalid", "a required column id is not a column"));
            }
            if !in_scope(&principal, column) {
                return Err(err("scope.unauthorized", "the object is outside the caller scope"));
            }
            let count = count_one(&mut client, COUNT_CLASSIFICATION, &request.project_id, column).await?;
            if count == 0 {
                return Err(err("classification.missing", "a required column has no classification"));
            }
        }
    } else if !request.required_columns.is_empty() {
        return Err(err("schema.invalid", "a review receipt does not take required columns"));
    }
    let decision = load_decision(&mut client, &request.project_id, &request.idempotency_key).await?;
    if request.compiler == "description-review" {
        if decision.kind != "label_description" || decision.choice != "approve" {
            return Err(err("receipt.unbound", "only an approval can bind a review artifact"));
        }
    } else {
        let dataset = parent_dataset(&decision.object_id);
        let approvals = count_one(&mut client, COUNT_DATASET_APPROVAL, &request.project_id, dataset).await?;
        if approvals == 0 {
            return Err(err("decision.absent", "the dataset has no approved label decision"));
        }
        if decision.kind != "classification" || decision.choice != "confirm" {
            return Err(err("receipt.unbound", "a pack receipt binds a classification decision"));
        }
    }
    if !in_scope(&principal, &decision.object_id) {
        return Err(err("scope.unauthorized", "the object is outside the caller scope"));
    }
    let bytes = read_regular(&request.command_file, 1_048_576)?;
    let command_sha = sha256_hex(&bytes)?;
    if command_sha != decision.source {
        return Err(err("stale", "the compiler command hash does not match the decision"));
    }
    let command = std::str::from_utf8(&bytes)
        .map_err(|_| err("schema.invalid", "the compiler command is not UTF-8"))?;
    let compiled = if request.compiler == "description-review" {
        run_review(
            command,
            &decision.object_id,
            &decision.candidate,
            &decision.input,
            &decision.schema,
            &decision.evidence,
            &decision.choice,
        )?
    } else {
        run_pack(command)?
    };
    let tx = client.transaction().await.map_err(map_db)?;
    let result = receipt_tx(
        &tx,
        request,
        &decision,
        &command_sha,
        &compiled.outcome_sha256,
        &compiled.protocol,
        &principal,
    )
    .await;
    finish(tx, result).await
}

pub async fn history(request: &HistoryRequest) -> Result<Output, LedgerError> {
    if request.protocol != "oo.review-ledger.history/v1" {
        return Err(err("protocol.mismatch", "the request protocol is not this command"));
    }
    let (control, _principal) = open_caller(
        &request.project_id,
        &request.control_file,
        &request.credential_file,
        &request.postgres,
    )
    .await?;
    let mut client = connect(&request.postgres).await?;
    require_control(&mut client, &request.project_id, &control.sha256).await?;
    let tx = client.transaction().await.map_err(map_db)?;
    tx.batch_execute(SESSION).await.map_err(map_db)?;
    let decisions = tx
        .query(READ_DECISIONS, &[&request.project_id])
        .await
        .map_err(map_db)?;
    let audits = tx.query(READ_AUDITS, &[&request.project_id]).await.map_err(map_db)?;
    let receipts = tx
        .query(READ_RECEIPTS, &[&request.project_id])
        .await
        .map_err(map_db)?;
    let heads = tx.query(READ_HEADS, &[&request.project_id]).await.map_err(map_db)?;
    let document = json!({
        "audits": audits.iter().map(|row| json!({
            "action": row.get::<_, String>(2),
            "body_sha256": row.get::<_, String>(6),
            "detail": row.get::<_, String>(3),
            "event_id": row.get::<_, i64>(0),
            "idempotency_key": row.get::<_, String>(1),
            "object_id": row.get::<_, String>(4),
            "principal_id": row.get::<_, String>(7),
            "revision": row.get::<_, i64>(5),
        })).collect::<Vec<_>>(),
        "decisions": decisions.iter().map(|row| json!({
            "authenticated": row.get::<_, bool>(7),
            "authority": row.get::<_, String>(8),
            "body_sha256": row.get::<_, String>(3),
            "curated_applied": row.get::<_, bool>(4),
            "grants": row.get::<_, String>(6),
            "idempotency_key": row.get::<_, String>(0),
            "management_identity": row.get::<_, String>(9),
            "object_id": row.get::<_, String>(1),
            "published": row.get::<_, bool>(5),
            "revision": row.get::<_, i64>(2),
        })).collect::<Vec<_>>(),
        "heads": heads.iter().map(|row| json!({
            "candidate_sha256": row.get::<_, String>(2),
            "object_id": row.get::<_, String>(0),
            "revision": row.get::<_, i64>(1),
            "source_sha256": row.get::<_, String>(3),
        })).collect::<Vec<_>>(),
        "protocol": "oo.review-ledger.history/v1",
        "receipts": receipts.iter().map(|row| json!({
            "candidate_sha256": row.get::<_, String>(3),
            "command_sha256": row.get::<_, String>(2),
            "compiler": row.get::<_, String>(1),
            "curated_files_written": row.get::<_, bool>(8),
            "git_applied": row.get::<_, bool>(6),
            "idempotency_key": row.get::<_, String>(0),
            "outcome_protocol": row.get::<_, String>(5),
            "outcome_sha256": row.get::<_, String>(4),
            "published": row.get::<_, bool>(7),
        })).collect::<Vec<_>>(),
    });
    let digest = canonical_sha(&document)?;
    tx.commit()
        .await
        .map_err(|_| err("commit.unknown", "the commit reply was not confirmed"))?;
    let mut output = Output::bare("history", "history");
    output.history_sha256 = Some(digest);
    Ok(output)
}

async fn enqueue_tx(
    tx: &Transaction<'_>,
    request: &EnqueueRequest,
    principal: &Principal,
    digests: &Digests,
) -> Result<Output, LedgerError> {
    tx.batch_execute(SESSION).await.map_err(map_db)?;
    let head = lock_head(tx, &request.project_id, &request.object_id).await?;
    let Some(head) = head else {
        if request.expected_revision.is_some() {
            return Err(err("revision.conflict", "the expected revision does not match the head"));
        }
        insert_revision(tx, request, 1).await?;
        tx.execute(
            INSERT_HEAD,
            &[
                &request.project_id,
                &request.object_id,
                &request.object_kind,
                &1_i64,
                &digests.candidate,
                &digests.input,
                &digests.evidence,
                &digests.schema,
                &digests.curated,
                &digests.source,
                &request.candidate_protocol,
                &request.owner_label,
            ],
        )
        .await
        .map_err(map_db)?;
        let body = enqueue_body(request, &principal.principal_id, 1)?;
        insert_audit(
            tx,
            AuditWrite {
                project: &request.project_id,
                key: &enqueue_audit_key(&request.object_id, &digests.candidate)?,
                action: "enqueue",
                detail: "",
                object_id: &request.object_id,
                revision: 1,
                body: &body,
                principal: &principal.principal_id,
            },
        )
        .await?;
        return Ok(stored("persisted", "persisted", 1, &body));
    };
    let expected = request
        .expected_revision
        .ok_or_else(|| err("revision.conflict", "the expected revision does not match the head"))?;
    if expected != head.revision {
        return Err(err("revision.conflict", "the expected revision does not match the head"));
    }
    if digests.same(&head) {
        let body = enqueue_body(request, &principal.principal_id, head.revision)?;
        return Ok(stored("replay", "replay", head.revision, &body));
    }
    if digests.candidate == head.candidate {
        return Err(err("idempotency.conflict", "the same candidate has a different hash"));
    }
    let next = head.revision.checked_add(1).ok_or_else(|| err("schema.invalid", "the revision cannot increase"))?;
    insert_revision(tx, request, next).await?;
    let updated = tx
        .execute(
            UPDATE_HEAD,
            &[
                &request.project_id,
                &request.object_id,
                &next,
                &digests.candidate,
                &digests.input,
                &digests.evidence,
                &digests.schema,
                &digests.curated,
                &digests.source,
                &request.candidate_protocol,
                &request.owner_label,
                &head.revision,
            ],
        )
        .await
        .map_err(map_db)?;
    if updated != 1 {
        return Err(err("revision.conflict", "the expected revision does not match the head"));
    }
    let body = enqueue_body(request, &principal.principal_id, next)?;
    insert_audit(
        tx,
        AuditWrite {
            project: &request.project_id,
            key: &enqueue_audit_key(&request.object_id, &digests.candidate)?,
            action: "enqueue",
            detail: "",
            object_id: &request.object_id,
            revision: next,
            body: &body,
            principal: &principal.principal_id,
        },
    )
    .await?;
    Ok(stored("persisted", "persisted", next, &body))
}

async fn decide_tx(
    tx: &Transaction<'_>,
    request: &DecideRequest,
    principal: &Principal,
) -> Result<Output, LedgerError> {
    tx.batch_execute(SESSION).await.map_err(map_db)?;
    if principal.role == "auditor" {
        return Err(err("role.forbidden", "an auditor cannot write a review decision"));
    }
    if !object_id_ok(&request.object_id) || !key_ok(&request.idempotency_key) {
        return Err(err("schema.invalid", "the decision identity is not allowed"));
    }
    if !in_scope(principal, &request.object_id) {
        return Err(err("scope.unauthorized", "the object is outside the caller scope"));
    }
    if request.reviewer_label != principal.reviewer_label || !label_ok(&request.reviewer_label) {
        return Err(err("reviewer.mismatch", "the reviewer label is not the caller principal"));
    }
    if !civil_date_ok(&request.reviewer_at) {
        return Err(err("reviewer.date", "the reviewer date is not a civil date"));
    }
    let digests = digests(
        &request.candidate_sha256,
        &request.input_sha256,
        &request.evidence_sha256,
        &request.schema_sha256,
        &request.curated_sha256,
        &request.source_sha256,
    )?;
    let prepared = fields::prepare(
        &request.decision_kind,
        &request.choice,
        &request.object_id,
        request.fields.as_ref(),
    )?;
    let head = lock_head(tx, &request.project_id, &request.object_id)
        .await?
        .ok_or_else(|| err("decision.absent", "the object is not on the queue"))?;
    if request.expected_revision != head.revision {
        return Err(err("revision.conflict", "the expected revision does not match the head"));
    }
    if digests.candidate != head.candidate {
        return Err(err("candidate.changed", "the candidate hash is not the head candidate"));
    }
    if !digests.same(&head) {
        return Err(err("stale", "a bound hash does not match the head"));
    }
    if prepared.kind == "owner_certification"
        && (principal.role != "owner" || principal.principal_id != head.owner_label)
    {
        return Err(err("reviewer_not_owner", "the caller is not the declared object owner"));
    }
    let body = decision_body(request, principal, &prepared, head.revision)?;
    if let Some(existing) = lock_decision(tx, &request.project_id, &request.idempotency_key).await? {
        if existing.body == body && existing.object_id == request.object_id {
            return Ok(stored("replay", "replay", existing.revision, &existing.body));
        }
        return Err(err("idempotency.conflict", "the idempotency key is already stored"));
    }
    tx.execute(
        INSERT_DECISION,
        &[
            &request.project_id,
            &request.idempotency_key,
            &request.object_id,
            &head.revision,
            &prepared.kind,
            &prepared.choice,
            &principal.principal_id,
            &request.reviewer_label,
            &request.reviewer_at,
            &digests.candidate,
            &digests.input,
            &digests.evidence,
            &digests.schema,
            &digests.curated,
            &digests.source,
            &prepared.fields_json,
            &body,
        ],
    )
    .await
    .map_err(map_db)?;
    insert_audit(
        tx,
        AuditWrite {
            project: &request.project_id,
            key: &request.idempotency_key,
            action: "decide",
            detail: "",
            object_id: &request.object_id,
            revision: head.revision,
            body: &body,
            principal: &principal.principal_id,
        },
    )
    .await?;
    Ok(stored("persisted", "persisted", head.revision, &body))
}

async fn record_batch(
    tx: &Transaction<'_>,
    request: &BatchRequest,
    principal: &Principal,
    status: &str,
    refused: i32,
    items: &[ItemOut],
) -> Result<Output, LedgerError> {
    tx.batch_execute(SESSION).await.map_err(map_db)?;
    let body = canonical_sha(&json!({
        "batch_id": request.batch_id,
        "items": items,
        "project_id": request.project_id,
        "status": status,
    }))?;
    tx.execute(
        INSERT_BATCH,
        &[
            &request.project_id,
            &request.batch_id,
            &principal.principal_id,
            &status,
            &(items.len() as i32),
            &refused,
        ],
    )
    .await
    .map_err(map_db)?;
    for (ordinal, item) in items.iter().enumerate() {
        let key = decision_key_for(&request.items, &item.object_id, item.code.as_str());
        tx.execute(
            INSERT_BATCH_ITEM,
            &[
                &request.project_id,
                &request.batch_id,
                &(ordinal as i32),
                &item.object_id,
                &item.code,
                &key,
            ],
        )
        .await
        .map_err(map_db)?;
    }
    insert_audit(
        tx,
        AuditWrite {
            project: &request.project_id,
            key: &request.batch_id,
            action: "batch",
            detail: "",
            object_id: "batch",
            revision: 0,
            body: &body,
            principal: &principal.principal_id,
        },
    )
    .await?;
    Ok(stored(status, status, 0, &body))
}

async fn receipt_tx(
    tx: &Transaction<'_>,
    request: &ReceiptRequest,
    decision: &StoredDecision,
    command_sha: &str,
    outcome_sha: &str,
    protocol: &str,
    principal: &Principal,
) -> Result<Output, LedgerError> {
    tx.batch_execute(SESSION).await.map_err(map_db)?;
    let current = lock_decision(tx, &request.project_id, &request.idempotency_key)
        .await?
        .ok_or_else(|| err("decision.absent", "the decision is not stored"))?;
    if current.body != decision.body || current.candidate != decision.candidate {
        return Err(err("candidate.changed", "the candidate hash is not the head candidate"));
    }
    let head = lock_head(tx, &request.project_id, &decision.object_id)
        .await?
        .ok_or_else(|| err("decision.absent", "the object is not on the queue"))?;
    if head.revision != decision.revision || head.candidate != decision.candidate {
        return Err(err("candidate.changed", "the candidate hash is not the head candidate"));
    }
    let existing = tx
        .query_opt(
            "SELECT command_sha256, candidate_sha256, outcome_sha256 \
             FROM oo_review_ledger.compiler_receipt \
             WHERE project_id = $1 AND idempotency_key = $2 AND compiler = $3",
            &[&request.project_id, &request.idempotency_key, &request.compiler],
        )
        .await
        .map_err(map_db)?;
    if let Some(row) = existing {
        let same = row.get::<_, String>(0) == command_sha
            && row.get::<_, String>(1) == decision.candidate
            && row.get::<_, String>(2) == outcome_sha;
        if same {
            return Ok(stored("replay", "replay", decision.revision, outcome_sha));
        }
        return Err(err("idempotency.conflict", "a different compiler receipt is stored"));
    }
    tx.execute(
        INSERT_RECEIPT,
        &[
            &request.project_id,
            &request.idempotency_key,
            &request.compiler,
            &command_sha,
            &decision.candidate,
            &outcome_sha,
            &protocol,
        ],
    )
    .await
    .map_err(map_db)?;
    insert_audit(
        tx,
        AuditWrite {
            project: &request.project_id,
            key: &request.idempotency_key,
            action: "receipt",
            detail: &request.compiler,
            object_id: &decision.object_id,
            revision: decision.revision,
            body: outcome_sha,
            principal: &principal.principal_id,
        },
    )
    .await?;
    Ok(stored("bound", "bound", decision.revision, outcome_sha))
}

async fn insert_revision(tx: &Transaction<'_>, request: &EnqueueRequest, revision: i64) -> Result<(), LedgerError> {
    tx.execute(
        INSERT_REVISION,
        &[
            &request.project_id,
            &request.object_id,
            &request.object_kind,
            &revision,
            &request.candidate_sha256,
            &request.input_sha256,
            &request.evidence_sha256,
            &request.schema_sha256,
            &request.curated_sha256,
            &request.source_sha256,
            &request.candidate_protocol,
            &request.owner_label,
        ],
    )
    .await
    .map_err(map_db)?;
    Ok(())
}

struct AuditWrite<'a> {
    project: &'a str,
    key: &'a str,
    action: &'a str,
    detail: &'a str,
    object_id: &'a str,
    revision: i64,
    body: &'a str,
    principal: &'a str,
}

async fn insert_audit(tx: &Transaction<'_>, audit: AuditWrite<'_>) -> Result<(), LedgerError> {
    tx.execute(
        INSERT_AUDIT,
        &[
            &audit.project,
            &audit.key,
            &audit.action,
            &audit.detail,
            &audit.object_id,
            &audit.revision,
            &audit.body,
            &audit.principal,
        ],
    )
    .await
    .map_err(map_db)?;
    Ok(())
}

async fn lock_head(tx: &Transaction<'_>, project: &str, object_id: &str) -> Result<Option<Head>, LedgerError> {
    let row = tx
        .query_opt(LOCK_HEAD, &[&project, &object_id])
        .await
        .map_err(map_db)?;
    Ok(row.map(|row| Head {
        revision: row.get(1),
        candidate: row.get(2),
        input: row.get(3),
        evidence: row.get(4),
        schema: row.get(5),
        curated: row.get(6),
        source: row.get(7),
        owner_label: row.get(9),
    }))
}

async fn lock_decision(
    tx: &Transaction<'_>,
    project: &str,
    key: &str,
) -> Result<Option<StoredDecision>, LedgerError> {
    let row = tx.query_opt(LOCK_DECISION, &[&project, &key]).await.map_err(map_db)?;
    Ok(row.map(stored_decision))
}

fn stored_decision(row: tokio_postgres::Row) -> StoredDecision {
    StoredDecision {
        revision: row.get(0),
        kind: row.get(1),
        choice: row.get(2),
        body: row.get(3),
        candidate: row.get(4),
        input: row.get(5),
        evidence: row.get(6),
        schema: row.get(7),
        source: row.get(9),
        object_id: row.get(10),
    }
}

async fn load_decision(client: &mut tokio_postgres::Client, project: &str, key: &str) -> Result<StoredDecision, LedgerError> {
    let tx = client.transaction().await.map_err(map_db)?;
    tx.batch_execute(SESSION).await.map_err(map_db)?;
    let found = lock_decision(&tx, project, key)
        .await?
        .ok_or_else(|| err("decision.absent", "the decision is not stored"))?;
    tx.commit()
        .await
        .map_err(|_| err("commit.unknown", "the commit reply was not confirmed"))?;
    Ok(found)
}

async fn count_one(
    client: &mut tokio_postgres::Client,
    sql: &str,
    project: &str,
    object_id: &str,
) -> Result<i64, LedgerError> {
    let row = client
        .query_one(sql, &[&project, &object_id])
        .await
        .map_err(map_db)?;
    Ok(row.get(0))
}

async fn batch_exists(client: &mut tokio_postgres::Client, project: &str, batch_id: &str) -> Result<bool, LedgerError> {
    let row = client
        .query_opt(
            "SELECT status FROM oo_review_ledger.batch WHERE project_id = $1 AND batch_id = $2",
            &[&project, &batch_id],
        )
        .await
        .map_err(map_db)?;
    Ok(row.is_some())
}

struct Manifest {
    sql: String,
    project: String,
    control: String,
}

async fn read_manifest(client: &mut tokio_postgres::Client) -> Result<Option<Manifest>, LedgerError> {
    match client.query_opt(READ_MANIFEST, &[]).await {
        Ok(Some(row)) => Ok(Some(Manifest {
            sql: row.get(0),
            project: row.get(1),
            control: row.get(2),
        })),
        Ok(None) => Ok(None),
        Err(error) => {
            if absent(&error) {
                Ok(None)
            } else {
                Err(map_db(error))
            }
        }
    }
}

async fn require_control(client: &mut tokio_postgres::Client, project: &str, control_sha: &str) -> Result<(), LedgerError> {
    let manifest = read_manifest(client)
        .await?
        .ok_or_else(|| err("install.required", "the review ledger schema is not installed"))?;
    if manifest.project != project || manifest.sql != migration_sha256()? {
        return Err(err("install.drift", "the installed schema does not match this migration"));
    }
    if manifest.control != control_sha {
        return Err(err("control.drift", "the control file does not match the install"));
    }
    Ok(())
}

async fn open_caller(
    project: &str,
    control_file: &str,
    credential_file: &str,
    _endpoint: &Endpoint,
) -> Result<(Control, Principal), LedgerError> {
    if !ident_ok(project) {
        return Err(err("schema.invalid", "the project id is not a single identifier"));
    }
    let control = parse_control(&read_regular(control_file, 65_536)?)?;
    if control.project_id != project {
        return Err(err("project.mismatch", "the control project does not match the request"));
    }
    let credential = read_regular(credential_file, 256)?;
    let principal = authenticate(&control, &credential)?.clone();
    Ok((control, principal))
}

async fn connect(endpoint: &Endpoint) -> Result<tokio_postgres::Client, LedgerError> {
    check_endpoint(endpoint)?;
    let password = read_password(&endpoint.password_file)?;
    let ca = read_regular(&endpoint.ca_file, 16_384)?;
    tls::connect(endpoint, &password, &ca).await
}

async fn finish(tx: Transaction<'_>, result: Result<Output, LedgerError>) -> Result<Output, LedgerError> {
    match result {
        Ok(output) => {
            tx.commit()
                .await
                .map_err(|_| err("commit.unknown", "the commit reply was not confirmed"))?;
            Ok(output)
        }
        Err(error) => {
            let _ = tx.rollback().await;
            Err(error)
        }
    }
}

fn digests(
    candidate: &str,
    input: &str,
    evidence: &str,
    schema: &str,
    curated: &str,
    source: &str,
) -> Result<Digests, LedgerError> {
    for hash in [candidate, input, evidence, schema, curated, source] {
        if !hex_ok(hash) {
            return Err(err("schema.invalid", "a hash is not 64 lowercase hex characters"));
        }
    }
    Ok(Digests {
        candidate: candidate.to_owned(),
        input: input.to_owned(),
        evidence: evidence.to_owned(),
        schema: schema.to_owned(),
        curated: curated.to_owned(),
        source: source.to_owned(),
    })
}

impl Digests {
    fn same(&self, head: &Head) -> bool {
        self.candidate == head.candidate
            && self.input == head.input
            && self.evidence == head.evidence
            && self.schema == head.schema
            && self.curated == head.curated
            && self.source == head.source
    }
}

fn enqueue_body(request: &EnqueueRequest, principal: &str, revision: i64) -> Result<String, LedgerError> {
    canonical_sha(&json!({
        "candidate_protocol": request.candidate_protocol,
        "candidate_sha256": request.candidate_sha256,
        "curated_sha256": request.curated_sha256,
        "evidence_sha256": request.evidence_sha256,
        "input_sha256": request.input_sha256,
        "object_id": request.object_id,
        "object_kind": request.object_kind,
        "owner_label": request.owner_label,
        "principal_id": principal,
        "project_id": request.project_id,
        "revision": revision,
        "schema_sha256": request.schema_sha256,
        "source_sha256": request.source_sha256,
    }))
}

fn decision_body(
    request: &DecideRequest,
    principal: &Principal,
    prepared: &Prepared,
    revision: i64,
) -> Result<String, LedgerError> {
    canonical_sha(&json!({
        "candidate_sha256": request.candidate_sha256,
        "choice": prepared.choice,
        "curated_sha256": request.curated_sha256,
        "decision_kind": prepared.kind,
        "evidence_sha256": request.evidence_sha256,
        "fields": prepared.fields_value,
        "idempotency_key": request.idempotency_key,
        "input_sha256": request.input_sha256,
        "object_id": request.object_id,
        "principal_id": principal.principal_id,
        "project_id": request.project_id,
        "reviewer_at": request.reviewer_at,
        "reviewer_label": request.reviewer_label,
        "revision": revision,
        "schema_sha256": request.schema_sha256,
        "source_sha256": request.source_sha256,
    }))
}

fn enqueue_audit_key(object_id: &str, candidate: &str) -> Result<String, LedgerError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"enqueue\n");
    bytes.extend_from_slice(object_id.as_bytes());
    bytes.push(b'\n');
    bytes.extend_from_slice(candidate.as_bytes());
    sha256_hex(&bytes)
}

fn decision_key_for(items: &[BatchItem], object_id: &str, code: &str) -> Option<String> {
    if code == "persisted" || code == "replay" {
        items
            .iter()
            .find(|item| item.object_id == object_id)
            .map(|item| item.idempotency_key.clone())
    } else {
        None
    }
}

fn parent_dataset(object_id: &str) -> &str {
    if object_id.matches('.').count() == 1 {
        object_id
    } else {
        object_id.rsplit_once('.').map(|(head, _)| head).unwrap_or(object_id)
    }
}

fn stored(kind: &str, code: &str, revision: i64, body: &str) -> Output {
    let mut output = Output::bare(kind, code);
    output.revision = Some(revision);
    output.body_sha256 = Some(body.to_owned());
    output
}

fn check_endpoint(endpoint: &Endpoint) -> Result<(), LedgerError> {
    if endpoint.host != endpoint.server_name || !host_ok(&endpoint.host) {
        return Err(err("schema.invalid", "the database host is not the server name"));
    }
    if endpoint.port == 0 {
        return Err(err("schema.invalid", "the database port is not allowed"));
    }
    if !ident_ok(&endpoint.database) || !principal_ok(&endpoint.user) {
        return Err(err("schema.invalid", "the database name or user is not allowed"));
    }
    Ok(())
}

fn host_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
}

pub fn read_request_file(path: &str) -> Result<Vec<u8>, LedgerError> {
    read_regular(path, 262_144)
}

fn read_regular(path: &str, max: usize) -> Result<Vec<u8>, LedgerError> {
    let meta = fs::symlink_metadata(path).map_err(|_| err("io.file", "the named file is not readable"))?;
    if !meta.file_type().is_file() {
        return Err(err("io.file", "the named path is not a regular file"));
    }
    if meta.len() > max as u64 {
        return Err(err("io.budget", "the named file exceeds the byte budget"));
    }
    let bytes = fs::read(Path::new(path)).map_err(|_| err("io.file", "the named file is not readable"))?;
    if bytes.len() > max {
        return Err(err("io.budget", "the named file exceeds the byte budget"));
    }
    Ok(bytes)
}

fn read_password(path: &str) -> Result<String, LedgerError> {
    let mut bytes = read_regular(path, 256)?;
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.is_empty() || bytes.contains(&0) {
        return Err(err("io.file", "the password file is empty"));
    }
    String::from_utf8(bytes).map_err(|_| err("io.file", "the password file is empty"))
}

fn endpoint_clone(endpoint: &Endpoint) -> Endpoint {
    Endpoint {
        host: endpoint.host.clone(),
        port: endpoint.port,
        database: endpoint.database.clone(),
        user: endpoint.user.clone(),
        password_file: endpoint.password_file.clone(),
        ca_file: endpoint.ca_file.clone(),
        server_name: endpoint.server_name.clone(),
    }
}

fn map_db(error: tokio_postgres::Error) -> LedgerError {
    if let Some(db) = error.as_db_error() {
        if db.constraint() == Some("audit_event_idempotency") {
            return err(
                "audit.rollback",
                "the audit insert failed and the decision was not committed",
            );
        }
        if db.code().code() == "23505" {
            return err("idempotency.conflict", "the idempotency key is already stored");
        }
    }
    err("database.refused", "the database refused the statement")
}

fn absent(error: &tokio_postgres::Error) -> bool {
    matches!(
        error.code().map(|code| code.code()),
        Some("42P01") | Some("3F000")
    )
}
