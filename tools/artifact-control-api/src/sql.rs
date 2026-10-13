//! Schema-qualified statements. Callers set `search_path` to `pg_catalog`.

use crate::crypto::sha256_hex;

pub const MIGRATION: &str = include_str!("../migrations/oo_artifact_control_v1.sql");
pub const INSTALL_LOCK: i64 = 7_398_000_398;

pub fn migration_sha256() -> String {
    sha256_hex(MIGRATION.as_bytes())
}

pub const SET_LOCAL: &str = "\
SET LOCAL search_path TO pg_catalog;
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '15s';
SET LOCAL synchronous_commit = on";

pub const ADVISORY_LOCK: &str = "SELECT pg_catalog.pg_advisory_xact_lock($1::bigint)";

pub const SCHEMA_PRESENT: &str = "\
SELECT COUNT(*)::bigint FROM pg_catalog.pg_namespace WHERE nspname = 'oo_artifact_control'";

pub const READ_MANIFEST: &str = "\
SELECT sql_sha256 FROM oo_artifact_control.install_manifest WHERE singleton";

pub const INSERT_MANIFEST: &str = "\
INSERT INTO oo_artifact_control.install_manifest (singleton, sql_sha256) VALUES (TRUE, $1::text)";

pub const LOCK_ANCHOR: &str = "\
SELECT absolute_end_unix FROM oo_artifact_control.session_anchor WHERE session_id = $1::text FOR UPDATE";

pub const INSERT_ANCHOR: &str = "\
INSERT INTO oo_artifact_control.session_anchor (session_id, principal_id, project_id, absolute_end_unix) \
VALUES ($1::text, $2::text, $3::text, $4::bigint)";

pub const EXTEND_ANCHOR: &str = "\
UPDATE oo_artifact_control.session_anchor SET absolute_end_unix = absolute_end_unix + 1 WHERE session_id = $1::text";

pub const DELETE_LOSS: &str = "DELETE FROM oo_artifact_control.artifact_loss";

pub const INSERT_LOSS: &str = "\
INSERT INTO oo_artifact_control.artifact_loss (loss_id, session_id, absolute_end_unix, reason, source_sha256, created_unix_ms) \
VALUES ($1::text, $2::text, $3::bigint, $4::text, $5::text, $6::bigint)";

pub const INSERT_ARTIFACT: &str = "\
INSERT INTO oo_artifact_control.artifact ( \
    object_id, deployment, principal_id, project_id, session_id, role, policy_id, knowledge_id, knowledge_digest, \
    media_type, plaintext_sha256, plaintext_size, source_sha256, sidecar_sha256, nonce, ciphertext_sha256, cipher, \
    created_unix_ms, expires_unix_ms, absolute_end_unix, state \
) VALUES ( \
    $1::text, $2::text, $3::text, $4::text, $5::text, $6::text, $7::text, $8::text, $9::text, \
    'text/csv', $10::text, $11::bigint, $12::text, $13::text, $14::bytea, $15::text, 'ring-aes-256-gcm', \
    $16::bigint, $17::bigint, $18::bigint, 'active' \
)";

pub const INSERT_IDEMPOTENCY: &str = "\
INSERT INTO oo_artifact_control.idempotency (session_id, idempotency_key, body_sha256, response) \
VALUES ($1::text, $2::text, $3::text, ($4::text)::jsonb)";

pub const READ_IDEMPOTENCY: &str = "\
SELECT body_sha256, response::text FROM oo_artifact_control.idempotency \
WHERE session_id = $1::text AND idempotency_key = $2::text";

pub const INSERT_COOKIE: &str = "\
INSERT INTO oo_artifact_control.session_cookie (cookie_sha256, csrf_sha256, session_id, principal_id, project_id) \
VALUES ($1::text, $2::text, $3::text, $4::text, $5::text)";

pub const READ_COOKIE: &str = "\
SELECT csrf_sha256, session_id, principal_id, project_id FROM oo_artifact_control.session_cookie \
WHERE cookie_sha256 = $1::text";

pub const LIST_ACTIVE: &str = "\
SELECT object_id, plaintext_sha256, plaintext_size, source_sha256, expires_unix_ms \
FROM oo_artifact_control.artifact \
WHERE deployment = $1::text AND principal_id = $2::text AND project_id = $3::text AND session_id = $4::text \
  AND role = $5::text AND policy_id = $6::text AND knowledge_id = $7::text AND knowledge_digest = $8::text \
  AND state = 'active' AND expires_unix_ms > $9::bigint \
ORDER BY object_id";

pub const READ_ARTIFACT: &str = "\
SELECT principal_id, project_id, session_id, role, policy_id, knowledge_id, knowledge_digest, \
       plaintext_sha256, plaintext_size, source_sha256, sidecar_sha256, nonce, ciphertext_sha256, \
       created_unix_ms, expires_unix_ms, absolute_end_unix, state \
FROM oo_artifact_control.artifact WHERE deployment = $1::text AND object_id = $2::text";

pub const REVOKE: &str = "\
UPDATE oo_artifact_control.artifact SET state = 'revoked' \
WHERE deployment = $1::text AND object_id = $2::text AND state = 'active'";

pub const EXPIRE: &str = "\
UPDATE oo_artifact_control.artifact SET state = 'expired' \
WHERE deployment = $1::text AND state IN ('active', 'revoked') AND expires_unix_ms <= $2::bigint \
RETURNING object_id";

pub const KNOWN_IDS: &str = "SELECT object_id FROM oo_artifact_control.artifact WHERE deployment = $1::text";

pub const ACTIVE_COUNT: &str = "\
SELECT COUNT(*)::bigint FROM oo_artifact_control.artifact \
WHERE deployment = $1::text AND session_id = $2::text AND state = 'active'";

pub const LATEST_LOSS: &str = "\
SELECT reason, source_sha256, absolute_end_unix FROM oo_artifact_control.artifact_loss \
WHERE session_id = $1::text ORDER BY created_unix_ms DESC, loss_id DESC LIMIT 1";

pub const LOSS_COUNT: &str = "\
SELECT COUNT(*)::bigint FROM oo_artifact_control.artifact_loss WHERE session_id = $1::text";
