//! Fixed SQL. Identifiers are literals. Caller values are parameters.

pub const MIGRATION_SQL: &str = include_str!("../migrations/oo_token_control_v1.sql");

pub const SCHEMA: &str = "oo_token_control";
pub const MANIFEST: &str = "install_manifest";
pub const TOKEN: &str = "token_record";
pub const REVOCATION: &str = "revocation";
pub const SEQUENCE: &str = "revocation_seq";
pub const ADMIN_ROLE: &str = "oo_token_admin";
pub const RUNTIME_ROLE: &str = "oo_token_runtime";
pub const FIXTURE_ROLE: &str = "oo_fixture_admin";
pub const INSTALL_LOCK: i64 = 881_275_033;
pub const PROJECT_CLASS: i32 = 881_275_033;

pub const SET_LOCAL: &str = "\
SET LOCAL search_path TO pg_catalog; \
SET LOCAL lock_timeout = '10s'; \
SET LOCAL statement_timeout = '15s'; \
SET LOCAL synchronous_commit = on";

pub const LOCK_INSTALL: &str = "SELECT pg_catalog.pg_advisory_xact_lock($1::bigint)";

pub const LOCK_PROJECT: &str = "SELECT pg_catalog.pg_advisory_xact_lock($1::int, $2::int)";

pub const SCHEMA_EXISTS: &str = "SELECT 1 FROM pg_catalog.pg_namespace WHERE nspname = $1";

pub const INSERT_MANIFEST: &str = "\
INSERT INTO oo_token_control.install_manifest (singleton, sql_sha256) VALUES (TRUE, $1)";

pub const READ_MANIFEST: &str = "\
SELECT sql_sha256 FROM oo_token_control.install_manifest WHERE singleton = TRUE";

pub const RELATIONS: &str = "\
SELECT c.relname \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = $1 AND c.relkind = 'r' \
ORDER BY c.relname";

pub const SEQUENCES: &str = "\
SELECT c.relname \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = $1 AND c.relkind = 'S' \
ORDER BY c.relname";

pub const COLUMNS: &str = "\
SELECT a.attname, pg_catalog.format_type(a.atttypid, a.atttypmod) \
FROM pg_catalog.pg_attribute a \
JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
ORDER BY a.attnum";

pub const CONSTRAINTS: &str = "\
SELECT c.conname \
FROM pg_catalog.pg_constraint c \
JOIN pg_catalog.pg_class r ON r.oid = c.conrelid \
JOIN pg_catalog.pg_namespace n ON n.oid = r.relnamespace \
WHERE n.nspname = $1 AND r.relname = $2 AND c.contype IN ('c', 'p', 'u') \
ORDER BY c.conname";

pub const PRIVILEGES: &str = "\
SELECT \
  pg_catalog.has_schema_privilege('oo_token_runtime', 'oo_token_control', 'USAGE'), \
  pg_catalog.has_schema_privilege('public', 'oo_token_control', 'USAGE'), \
  pg_catalog.has_table_privilege('oo_token_runtime', 'oo_token_control.install_manifest', 'SELECT'), \
  pg_catalog.has_table_privilege('oo_token_runtime', 'oo_token_control.install_manifest', 'INSERT'), \
  pg_catalog.has_table_privilege('oo_token_runtime', 'oo_token_control.install_manifest', 'UPDATE'), \
  pg_catalog.has_table_privilege('oo_token_runtime', 'oo_token_control.install_manifest', 'DELETE'), \
  pg_catalog.has_table_privilege('oo_token_runtime', 'oo_token_control.token_record', 'SELECT'), \
  pg_catalog.has_table_privilege('oo_token_runtime', 'oo_token_control.token_record', 'INSERT'), \
  pg_catalog.has_table_privilege('oo_token_runtime', 'oo_token_control.token_record', 'UPDATE'), \
  pg_catalog.has_table_privilege('oo_token_runtime', 'oo_token_control.token_record', 'DELETE'), \
  pg_catalog.has_table_privilege('oo_token_runtime', 'oo_token_control.token_record', 'TRUNCATE'), \
  pg_catalog.has_column_privilege('oo_token_runtime', 'oo_token_control.token_record', 'last_used_ms', 'UPDATE'), \
  pg_catalog.has_column_privilege('oo_token_runtime', 'oo_token_control.token_record', 'idle_deadline_ms', 'UPDATE'), \
  pg_catalog.has_column_privilege('oo_token_runtime', 'oo_token_control.token_record', 'role_name', 'UPDATE'), \
  pg_catalog.has_column_privilege('oo_token_runtime', 'oo_token_control.token_record', 'token_sha256', 'UPDATE'), \
  pg_catalog.has_table_privilege('oo_token_runtime', 'oo_token_control.revocation', 'SELECT'), \
  pg_catalog.has_table_privilege('oo_token_runtime', 'oo_token_control.revocation', 'INSERT'), \
  pg_catalog.has_table_privilege('oo_token_runtime', 'oo_token_control.revocation', 'UPDATE'), \
  pg_catalog.has_table_privilege('oo_token_runtime', 'oo_token_control.revocation', 'DELETE'), \
  pg_catalog.has_sequence_privilege('oo_token_runtime', 'oo_token_control.revocation_seq', 'USAGE'), \
  pg_catalog.has_sequence_privilege('oo_token_runtime', 'oo_token_control.revocation_seq', 'UPDATE'), \
  pg_catalog.has_table_privilege('public', 'oo_token_control.token_record', 'SELECT')";

pub const CURRENT_USER: &str = "SELECT current_user";

pub const TLS_FACT: &str = "\
SELECT ssl, version FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()";

pub const SESSION_CLOSED: &str = "\
SELECT 1 FROM oo_token_control.revocation \
WHERE scope = 'session' AND project_id = $1 AND principal_id = '' AND session_id = $2 AND token_sha256 = ''";

pub const FLOOR: &str = "\
SELECT COALESCE(MAX(seq), 0)::bigint \
FROM oo_token_control.revocation \
WHERE project_id = $1 AND ( \
  (scope = 'project' AND principal_id = '' AND session_id = '' AND token_sha256 = '') \
  OR (scope = 'principal' AND principal_id = $2 AND session_id = '' AND token_sha256 = '') \
  OR ($3 <> '' AND scope = 'session' AND session_id = $3 AND principal_id = '' AND token_sha256 = '') \
)";

pub const INSERT_TOKEN: &str = "\
INSERT INTO oo_token_control.token_record ( \
  token_sha256, token_kind, project_id, principal_id, role_name, claims_canonical, \
  policy_version, knowledge_version, audience_url, sandbox_id, session_id, \
  issued_at_ms, idle_deadline_ms, absolute_deadline_ms, last_used_ms, revocation_floor \
) VALUES ( \
  $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16 \
)";

pub const READ_TOKEN: &str = "\
SELECT token_kind, project_id, principal_id, role_name, claims_canonical, \
       policy_version, knowledge_version, audience_url, sandbox_id, session_id, \
       issued_at_ms, idle_deadline_ms, absolute_deadline_ms, revocation_floor \
FROM oo_token_control.token_record \
WHERE token_sha256 = $1";

pub const COVERING: &str = "\
SELECT 1 FROM oo_token_control.revocation \
WHERE project_id = $1 AND ( \
  (scope = 'token' AND token_sha256 = $2 AND principal_id = '' AND session_id = '') \
  OR ( \
    seq > $3 AND ( \
      (scope = 'project' AND principal_id = '' AND session_id = '' AND token_sha256 = '') \
      OR (scope = 'principal' AND principal_id = $4 AND session_id = '' AND token_sha256 = '') \
      OR ($5 <> '' AND scope = 'session' AND session_id = $5 AND principal_id = '' AND token_sha256 = '') \
    ) \
  ) \
) LIMIT 1";

pub const SLIDE: &str = "\
UPDATE oo_token_control.token_record \
SET last_used_ms = $1, idle_deadline_ms = $2 \
WHERE token_sha256 = $3";

pub const READ_REVOCATION: &str = "\
SELECT seq FROM oo_token_control.revocation \
WHERE scope = $1 AND project_id = $2 AND principal_id = $3 AND session_id = $4 AND token_sha256 = $5";

pub const INSERT_REVOCATION: &str = "\
INSERT INTO oo_token_control.revocation ( \
  scope, project_id, principal_id, session_id, token_sha256, seq, revoked_at_ms \
) VALUES ( \
  $1, $2, $3, $4, $5, nextval('oo_token_control.revocation_seq'), $6 \
) RETURNING seq";

pub const EXPECTED_RELATIONS: [&str; 3] = [MANIFEST, REVOCATION, TOKEN];
pub const EXPECTED_SEQUENCES: [&str; 1] = [SEQUENCE];

pub const EXPECTED_MANIFEST_COLUMNS: [(&str, &str); 2] =
    [("singleton", "boolean"), ("sql_sha256", "text")];

pub const EXPECTED_TOKEN_COLUMNS: [(&str, &str); 16] = [
    ("token_sha256", "text"),
    ("token_kind", "text"),
    ("project_id", "text"),
    ("principal_id", "text"),
    ("role_name", "text"),
    ("claims_canonical", "text"),
    ("policy_version", "text"),
    ("knowledge_version", "text"),
    ("audience_url", "text"),
    ("sandbox_id", "text"),
    ("session_id", "text"),
    ("issued_at_ms", "bigint"),
    ("idle_deadline_ms", "bigint"),
    ("absolute_deadline_ms", "bigint"),
    ("last_used_ms", "bigint"),
    ("revocation_floor", "bigint"),
];

pub const EXPECTED_REVOCATION_COLUMNS: [(&str, &str); 7] = [
    ("scope", "text"),
    ("project_id", "text"),
    ("principal_id", "text"),
    ("session_id", "text"),
    ("token_sha256", "text"),
    ("seq", "bigint"),
    ("revoked_at_ms", "bigint"),
];

pub const EXPECTED_MANIFEST_CONSTRAINTS: [&str; 3] = [
    "install_manifest_one",
    "install_manifest_pk",
    "install_manifest_sha",
];

pub const EXPECTED_TOKEN_CONSTRAINTS: [&str; 7] = [
    "token_record_deadlines",
    "token_record_floor",
    "token_record_kind",
    "token_record_no_prefix",
    "token_record_pk",
    "token_record_session_binding",
    "token_record_sha",
];

pub const EXPECTED_REVOCATION_CONSTRAINTS: [&str; 5] = [
    "revocation_exact",
    "revocation_pk",
    "revocation_seq_positive",
    "revocation_seq_unique",
    "revocation_time",
];

pub const EXPECTED_PRIVILEGES: [(&str, bool); 22] = [
    ("privilege_schema_usage", true),
    ("privilege_public_schema_usage", false),
    ("privilege_manifest_select", true),
    ("privilege_manifest_insert", false),
    ("privilege_manifest_update", false),
    ("privilege_manifest_delete", false),
    ("privilege_token_select", true),
    ("privilege_token_insert", false),
    ("privilege_token_update", false),
    ("privilege_token_delete", false),
    ("privilege_token_truncate", false),
    ("privilege_last_used_update", true),
    ("privilege_idle_update", true),
    ("privilege_role_update", false),
    ("privilege_hash_update", false),
    ("privilege_revocation_select", true),
    ("privilege_revocation_insert", false),
    ("privilege_revocation_update", false),
    ("privilege_revocation_delete", false),
    ("privilege_sequence_usage", false),
    ("privilege_sequence_update", false),
    ("privilege_public_token_select", false),
];

#[cfg(test)]
mod tests {
    use super::MIGRATION_SQL;

    #[test]
    fn migration_does_not_autofix_or_store_a_raw_token() {
        assert!(!MIGRATION_SQL.contains("IF NOT EXISTS"));
        assert!(!MIGRATION_SQL.contains("CREATE OR REPLACE"));
        assert!(!MIGRATION_SQL.contains("oo_control"));
        assert!(!MIGRATION_SQL.contains("schema_000"));
        assert!(MIGRATION_SQL.contains("oo_token_control"));
        assert!(MIGRATION_SQL.contains("GRANT UPDATE (last_used_ms, idle_deadline_ms)"));
        assert!(!MIGRATION_SQL.contains("GRANT INSERT"));
        assert!(!MIGRATION_SQL.contains("GRANT ALL"));
        assert!(!MIGRATION_SQL.contains("jwt"));
        assert!(MIGRATION_SQL.contains("token_sha256 !~ '^(oost_|ooct_)'"));
        let source = include_str!("sql.rs");
        let format_bang = ["format", "!"].concat();
        assert!(!source.contains(&format_bang));
        assert!(source.contains("contype IN ('c', 'p', 'u')"));
    }
}
