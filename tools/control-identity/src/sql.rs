//! Schema-qualified statements. Callers set `search_path` to `pg_catalog`.

pub const MIGRATION: &str = include_str!("../migrations/oo_identity_control_v1.sql");
pub const INSTALL_LOCK: i64 = 5_481_003_541;

pub const SET_LOCAL: &str = "\
SET LOCAL search_path TO pg_catalog;
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '15s';
SET LOCAL synchronous_commit = on";

pub const ADVISORY_LOCK: &str = "SELECT pg_catalog.pg_advisory_xact_lock($1::bigint)";

pub const SCHEMA_PRESENT: &str = "\
SELECT COUNT(*)::bigint FROM pg_catalog.pg_namespace WHERE nspname = 'oo_identity_control'";

pub const READ_MANIFEST: &str = "\
SELECT sql_sha256, initialized FROM oo_identity_control.install_manifest WHERE singleton";

pub const LOCK_MANIFEST: &str = "\
SELECT initialized FROM oo_identity_control.install_manifest WHERE singleton FOR UPDATE";

pub const INSERT_MANIFEST: &str = "\
INSERT INTO oo_identity_control.install_manifest (singleton, sql_sha256, initialized) \
VALUES (TRUE, $1::text, FALSE)";

pub const MARK_INITIALIZED: &str = "\
UPDATE oo_identity_control.install_manifest SET initialized = TRUE \
WHERE singleton AND initialized = FALSE";

pub const INSERT_OAUTH: &str = "\
INSERT INTO oo_identity_control.oauth_transaction ( \
    handle_hash, state_hash, nonce_hash, pkce_challenge, redirect_uri, return_to, bootstrap_hash, expires_at \
) VALUES ( \
    $1::text, $2::text, $3::text, $4::text, $5::text, $6::text, $7::text, \
    pg_catalog.now() + ($8::int4 * interval '1 second') \
)";

pub const LOCK_OAUTH: &str = "\
SELECT state_hash, nonce_hash, pkce_challenge, redirect_uri, return_to, bootstrap_hash, \
       (expires_at <= pg_catalog.now()) AS expired, \
       (consumed_at IS NOT NULL) AS consumed \
FROM oo_identity_control.oauth_transaction WHERE handle_hash = $1::text FOR UPDATE";

pub const CONSUME_OAUTH: &str = "\
UPDATE oo_identity_control.oauth_transaction SET consumed_at = pg_catalog.now() \
WHERE handle_hash = $1::text AND consumed_at IS NULL";

pub const INSERT_SESSION: &str = "\
INSERT INTO oo_identity_control.session ( \
    cookie_hash, csrf_hash, subject, issuer, config_commit, revision, idle_expires_at, absolute_expires_at \
) VALUES ( \
    $1::text, $2::text, $3::text, $4::text, $5::text, $6::bigint, \
    pg_catalog.now() + ($7::int4 * interval '1 second'), \
    pg_catalog.now() + ($8::int4 * interval '1 second') \
)";

pub const LOCK_SESSION: &str = "\
SELECT subject, issuer, config_commit, revision, csrf_hash, \
       to_char(idle_expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS idle_at, \
       to_char(absolute_expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS absolute_at, \
       (revoked_at IS NOT NULL OR idle_expires_at <= pg_catalog.now() OR absolute_expires_at <= pg_catalog.now()) AS dead \
FROM oo_identity_control.session WHERE cookie_hash = $1::text FOR UPDATE";

pub const READ_SESSION: &str = "\
SELECT subject, issuer, config_commit, revision, \
       (revoked_at IS NOT NULL OR idle_expires_at <= pg_catalog.now() OR absolute_expires_at <= pg_catalog.now()) AS dead \
FROM oo_identity_control.session WHERE cookie_hash = $1::text";

pub const SLIDE_SESSION: &str = "\
UPDATE oo_identity_control.session SET \
    csrf_hash = $2::text, \
    idle_expires_at = LEAST(pg_catalog.now() + ($3::int4 * interval '1 second'), absolute_expires_at) \
WHERE cookie_hash = $1::text AND revoked_at IS NULL \
  AND idle_expires_at > pg_catalog.now() AND absolute_expires_at > pg_catalog.now() \
RETURNING \
    to_char(idle_expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS idle_at, \
    to_char(absolute_expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS absolute_at";

pub const REVOKE_SESSION: &str = "\
UPDATE oo_identity_control.session SET revoked_at = pg_catalog.now() \
WHERE cookie_hash = $1::text AND revoked_at IS NULL";

pub const SUPERSEDE_BOOTSTRAP: &str = "\
UPDATE oo_identity_control.bootstrap_token SET state = 'superseded' WHERE state = 'issued'";

pub const INSERT_BOOTSTRAP: &str = "\
INSERT INTO oo_identity_control.bootstrap_token ( \
    token_hash, subject, project_id, role, state, expires_at \
) VALUES ( \
    $1::text, $2::text, $3::text, $4::text, 'issued', \
    pg_catalog.now() + ($5::int4 * interval '1 second') \
)";

pub const LOCK_BOOTSTRAP: &str = "\
SELECT subject, project_id, role, state, (expires_at <= pg_catalog.now()) AS expired \
FROM oo_identity_control.bootstrap_token WHERE token_hash = $1::text FOR UPDATE";

pub const CONSUME_BOOTSTRAP: &str = "\
UPDATE oo_identity_control.bootstrap_token \
SET state = 'consumed', consumed_at = pg_catalog.now(), session_hash = $2::text \
WHERE token_hash = $1::text AND state = 'issued'";

pub const INSERT_AUDIT: &str = "\
INSERT INTO oo_identity_control.audit_event (action, code, subject, session_hash) \
VALUES ($1::text, $2::text, $3::text, $4::text)";

pub fn migration_sha256() -> String {
    crate::crypto::sha256_hex(MIGRATION.as_bytes())
}
