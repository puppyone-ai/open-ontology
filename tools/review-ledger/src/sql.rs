//! Static statements for schema oo_review_ledger. Names are schema-qualified.

pub const MIGRATION_SQL: &str = include_str!("../migrations/oo_review_ledger_v1.sql");

pub const SESSION: &str = "\
SET LOCAL lock_timeout = '5s'; \
SET LOCAL statement_timeout = '15s'; \
SET LOCAL synchronous_commit = on; \
SET LOCAL search_path = pg_catalog";

pub const READ_MANIFEST: &str = "\
SELECT sql_sha256, project_id, control_sha256 \
FROM oo_review_ledger.install_manifest \
WHERE singleton = true";

pub const LOCK_HEAD: &str = "\
SELECT object_kind, revision, candidate_sha256, input_sha256, evidence_sha256, \
       schema_sha256, curated_sha256, source_sha256, candidate_protocol, owner_label \
FROM oo_review_ledger.queue_head \
WHERE project_id = $1 AND object_id = $2 \
FOR UPDATE";

pub const INSERT_REVISION: &str = "\
INSERT INTO oo_review_ledger.queue_revision ( \
  project_id, object_id, object_kind, revision, candidate_sha256, input_sha256, \
  evidence_sha256, schema_sha256, curated_sha256, source_sha256, candidate_protocol, owner_label \
) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)";

pub const INSERT_HEAD: &str = "\
INSERT INTO oo_review_ledger.queue_head ( \
  project_id, object_id, object_kind, revision, candidate_sha256, input_sha256, \
  evidence_sha256, schema_sha256, curated_sha256, source_sha256, candidate_protocol, owner_label \
) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)";

pub const UPDATE_HEAD: &str = "\
UPDATE oo_review_ledger.queue_head \
SET revision = $3, candidate_sha256 = $4, input_sha256 = $5, evidence_sha256 = $6, \
    schema_sha256 = $7, curated_sha256 = $8, source_sha256 = $9, candidate_protocol = $10, \
    owner_label = $11 \
WHERE project_id = $1 AND object_id = $2 AND revision = $12";

pub const LOCK_DECISION: &str = "\
SELECT revision, decision_kind, choice, body_sha256, candidate_sha256, input_sha256, \
       evidence_sha256, schema_sha256, curated_sha256, source_sha256, object_id, \
       curated_applied, published, grants, authenticated \
FROM oo_review_ledger.decision \
WHERE project_id = $1 AND idempotency_key = $2 \
FOR UPDATE";

pub const INSERT_DECISION: &str = "\
INSERT INTO oo_review_ledger.decision ( \
  project_id, idempotency_key, object_id, revision, decision_kind, choice, \
  reviewer_principal, reviewer_label, reviewer_at, candidate_sha256, input_sha256, \
  evidence_sha256, schema_sha256, curated_sha256, source_sha256, fields_json, body_sha256, \
  authenticated, authority, management_identity, curated_applied, published, grants \
) VALUES ( \
  $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17, \
  false, 'not_verified', 'not_claimed', false, false, 'not_authorized' \
)";

pub const INSERT_AUDIT: &str = "\
INSERT INTO oo_review_ledger.audit_event ( \
  project_id, idempotency_key, action, detail, object_id, revision, body_sha256, principal_id \
) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)";

pub const INSERT_RECEIPT: &str = "\
INSERT INTO oo_review_ledger.compiler_receipt ( \
  project_id, idempotency_key, compiler, command_sha256, candidate_sha256, outcome_sha256, \
  outcome_protocol, git_applied, published, curated_files_written \
) VALUES ($1,$2,$3,$4,$5,$6,$7,false,false,false)";

pub const COUNT_CLASSIFICATION: &str = "\
SELECT count(*)::bigint \
FROM oo_review_ledger.decision AS decision \
JOIN oo_review_ledger.queue_head AS head \
  ON head.project_id = decision.project_id AND head.object_id = decision.object_id \
WHERE decision.project_id = $1 \
  AND decision.object_id = $2 \
  AND decision.decision_kind = 'classification' \
  AND decision.choice = 'confirm' \
  AND decision.revision = head.revision \
  AND decision.candidate_sha256 = head.candidate_sha256";

pub const COUNT_DATASET_APPROVAL: &str = "\
SELECT count(*)::bigint \
FROM oo_review_ledger.decision AS decision \
JOIN oo_review_ledger.queue_head AS head \
  ON head.project_id = decision.project_id AND head.object_id = decision.object_id \
WHERE decision.project_id = $1 \
  AND decision.object_id = $2 \
  AND decision.decision_kind = 'label_description' \
  AND decision.choice = 'approve' \
  AND decision.revision = head.revision \
  AND decision.candidate_sha256 = head.candidate_sha256";

pub const INSERT_BATCH: &str = "\
INSERT INTO oo_review_ledger.batch ( \
  project_id, batch_id, principal_id, status, item_count, refused_count, applied \
) VALUES ($1,$2,$3,$4,$5,$6,false)";

pub const INSERT_BATCH_ITEM: &str = "\
INSERT INTO oo_review_ledger.batch_item ( \
  project_id, batch_id, ordinal, object_id, code, decision_key \
) VALUES ($1,$2,$3,$4,$5,$6)";

pub const READ_DECISIONS: &str = "\
SELECT idempotency_key, object_id, revision, body_sha256, curated_applied, published, grants, \
       authenticated, authority, management_identity \
FROM oo_review_ledger.decision \
WHERE project_id = $1 \
ORDER BY idempotency_key";

pub const READ_AUDITS: &str = "\
SELECT event_id, idempotency_key, action, detail, object_id, revision, body_sha256, principal_id \
FROM oo_review_ledger.audit_event \
WHERE project_id = $1 \
ORDER BY event_id";

pub const READ_RECEIPTS: &str = "\
SELECT idempotency_key, compiler, command_sha256, candidate_sha256, outcome_sha256, \
       outcome_protocol, git_applied, published, curated_files_written \
FROM oo_review_ledger.compiler_receipt \
WHERE project_id = $1 \
ORDER BY idempotency_key, compiler";

pub const READ_HEADS: &str = "\
SELECT object_id, revision, candidate_sha256, source_sha256 \
FROM oo_review_ledger.queue_head \
WHERE project_id = $1 \
ORDER BY object_id";
