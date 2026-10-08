//! Schema-qualified statements. Callers set `search_path` to `pg_catalog`.

pub const MIGRATION: &str = include_str!("../migrations/oo_secret_control_v1.sql");
pub const INSTALL_LOCK: i64 = 7_384_000_384;

pub const SET_LOCAL: &str = "\
SET LOCAL search_path TO pg_catalog;
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '15s';
SET LOCAL synchronous_commit = on";

pub const ADVISORY_LOCK: &str = "SELECT pg_catalog.pg_advisory_xact_lock($1::bigint)";

pub const SCHEMA_PRESENT: &str = "\
SELECT COUNT(*)::bigint FROM pg_catalog.pg_namespace WHERE nspname = 'oo_secret_control'";

pub const READ_MANIFEST: &str = "\
SELECT sql_sha256 FROM oo_secret_control.install_manifest WHERE singleton";

pub const INSERT_MANIFEST: &str = "\
INSERT INTO oo_secret_control.install_manifest (singleton, sql_sha256) VALUES (TRUE, $1::text)";

pub const LOCK_IDEMPOTENCY: &str = "\
SELECT body_sha256, response::text FROM oo_secret_control.idempotency \
WHERE project_id = $1::text AND idempotency_key = $2::text FOR UPDATE";

pub const INSERT_IDEMPOTENCY: &str = "\
INSERT INTO oo_secret_control.idempotency (project_id, idempotency_key, body_sha256, response) \
VALUES ($1::text, $2::text, $3::text, ($4::text)::jsonb)";

pub const LOCK_HEAD: &str = "\
SELECT current_version, state, allowed_workload, secret_ref, subject \
FROM oo_secret_control.secret_head \
WHERE deployment = $1::text AND project_id = $2::text AND namespace = $3::text AND secret_id = $4::text \
FOR UPDATE";

pub const INSERT_VERSION: &str = "\
INSERT INTO oo_secret_control.secret_version ( \
    deployment, project_id, namespace, secret_id, version, nonce, ciphertext, cipher, state \
) VALUES ( \
    $1::text, $2::text, $3::text, $4::text, $5::int4, $6::bytea, $7::bytea, $8::text, 'active' \
)";

pub const INSERT_HEAD: &str = "\
INSERT INTO oo_secret_control.secret_head ( \
    deployment, project_id, namespace, secret_id, current_version, state, allowed_workload, secret_ref, subject \
) VALUES ( \
    $1::text, $2::text, $3::text, $4::text, $5::int4, 'active', $6::text, $7::text, $8::text \
)";

pub const ACTIVATE_HEAD: &str = "\
UPDATE oo_secret_control.secret_head SET current_version = $5::int4 \
WHERE deployment = $1::text AND project_id = $2::text AND namespace = $3::text AND secret_id = $4::text \
  AND current_version = $6::int4 AND state = 'active'";

pub const RETIRE_VERSION: &str = "\
UPDATE oo_secret_control.secret_version SET state = 'retired' \
WHERE deployment = $1::text AND project_id = $2::text AND namespace = $3::text AND secret_id = $4::text \
  AND version = $5::int4 AND state = 'active'";

pub const REVOKE_HEAD: &str = "\
UPDATE oo_secret_control.secret_head SET state = 'revoked' \
WHERE deployment = $1::text AND project_id = $2::text AND namespace = $3::text AND secret_id = $4::text \
  AND state = 'active'";

pub const REVOKE_VERSION: &str = "\
UPDATE oo_secret_control.secret_version SET state = 'revoked' \
WHERE deployment = $1::text AND project_id = $2::text AND namespace = $3::text AND secret_id = $4::text \
  AND version = $5::int4 AND state = 'active'";

pub const READ_VERSION: &str = "\
SELECT nonce, ciphertext, cipher, state FROM oo_secret_control.secret_version \
WHERE deployment = $1::text AND project_id = $2::text AND namespace = $3::text AND secret_id = $4::text \
  AND version = $5::int4";

pub const INSERT_AUDIT: &str = "\
INSERT INTO oo_secret_control.audit_event ( \
    action, code, project_id, namespace, secret_id, version, subject, workload \
) VALUES ( \
    $1::text, $2::text, $3::text, $4::text, $5::text, $6::int4, $7::text, $8::text \
)";

pub fn migration_sha256() -> String {
    crate::crypto::sha256_hex(MIGRATION.as_bytes())
}
