//! Fixed SQL. Identifiers are literals. Caller values are parameters.

pub const MIGRATION_SQL: &str = include_str!("../migrations/oo_session_control_v1.sql");

pub const SCHEMA: &str = "oo_session_control";
pub const HEAD: &str = "session_head";
pub const MANIFEST: &str = "install_manifest";
pub const ADMIN_ROLE: &str = "oo_session_admin";
pub const RUNTIME_ROLE: &str = "oo_session_runtime";
pub const HEAD_REGCLASS: &str = "oo_session_control.session_head";
pub const MANIFEST_REGCLASS: &str = "oo_session_control.install_manifest";
pub const INSTALL_LOCK: i64 = 881_275_031;
pub const STATE_JSON_MAX: usize = 1_048_576;

pub const SET_LOCAL: &str = "\
SET LOCAL search_path TO pg_catalog; \
SET LOCAL lock_timeout = '10s'; \
SET LOCAL statement_timeout = '15s'; \
SET LOCAL synchronous_commit = on";

pub const LOCK_INSTALL: &str = "SELECT pg_catalog.pg_advisory_xact_lock($1::bigint)";

pub const SCHEMA_EXISTS: &str = "SELECT 1 FROM pg_catalog.pg_namespace WHERE nspname = $1";

pub const INSERT_MANIFEST: &str = "\
INSERT INTO oo_session_control.install_manifest (singleton, sql_sha256) VALUES (TRUE, $1)";

pub const READ_MANIFEST: &str = "\
SELECT sql_sha256 FROM oo_session_control.install_manifest WHERE singleton = TRUE";

pub const RELATIONS: &str = "\
SELECT c.relname \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = $1 AND c.relkind = 'r' \
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
WHERE n.nspname = $1 AND r.relname = $2 AND c.contype IN ('c', 'p') \
ORDER BY c.conname";

pub const PRIVILEGES: &str = "\
SELECT \
  pg_catalog.has_schema_privilege($1, $2, 'USAGE'), \
  pg_catalog.has_table_privilege($1, $3, 'SELECT'), \
  pg_catalog.has_table_privilege($1, $3, 'UPDATE'), \
  pg_catalog.has_table_privilege($1, $3, 'INSERT'), \
  pg_catalog.has_table_privilege($1, $3, 'DELETE'), \
  pg_catalog.has_table_privilege($1, $3, 'TRUNCATE'), \
  pg_catalog.has_table_privilege($1, $4, 'SELECT'), \
  pg_catalog.has_table_privilege($1, $4, 'UPDATE'), \
  pg_catalog.has_table_privilege($1, $4, 'INSERT')";

pub const CURRENT_USER: &str = "SELECT current_user";

pub const TLS_FACT: &str = "\
SELECT ssl, version FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()";

pub const LOCK_HEAD: &str = "\
SELECT revision::text, owner_generation::text, \
       bound_project_id, bound_principal_id, bound_role, bound_claims_hash, \
       bound_policy_version, bound_knowledge_version, bound_config_version, \
       state_json, journal_len \
FROM oo_session_control.session_head \
WHERE project_id = $1 AND session_id = $2 \
FOR UPDATE";

pub const READ_HEAD: &str = "\
SELECT revision::text, owner_generation::text, \
       bound_project_id, bound_principal_id, bound_role, bound_claims_hash, \
       bound_policy_version, bound_knowledge_version, bound_config_version, \
       state_json, journal_len \
FROM oo_session_control.session_head \
WHERE project_id = $1 AND session_id = $2";

// The parameter stays text, which `String` can send. The server casts that decimal to numeric.
pub const INSERT_HEAD: &str = "\
INSERT INTO oo_session_control.session_head ( \
  project_id, session_id, revision, owner_generation, \
  bound_project_id, bound_principal_id, bound_role, bound_claims_hash, \
  bound_policy_version, bound_knowledge_version, bound_config_version, \
  state_json, journal_len \
) VALUES ( \
  $1, $2, $3::text::numeric, $4::text::numeric, \
  $5, $6, $7, $8, $9, $10, $11, $12, $13 \
)";

pub const UPDATE_HEAD: &str = "\
UPDATE oo_session_control.session_head SET \
  revision = $3::text::numeric, \
  owner_generation = $4::text::numeric, \
  bound_project_id = $5, \
  bound_principal_id = $6, \
  bound_role = $7, \
  bound_claims_hash = $8, \
  bound_policy_version = $9, \
  bound_knowledge_version = $10, \
  bound_config_version = $11, \
  state_json = $12, \
  journal_len = $13 \
WHERE project_id = $1 AND session_id = $2 \
  AND revision = $14::text::numeric AND owner_generation = $15::text::numeric";

pub const EXPECTED_RELATIONS: [&str; 2] = [MANIFEST, HEAD];

pub const EXPECTED_HEAD_COLUMNS: [(&str, &str); 13] = [
    ("project_id", "text"),
    ("session_id", "text"),
    ("revision", "numeric(20,0)"),
    ("owner_generation", "numeric(20,0)"),
    ("bound_project_id", "text"),
    ("bound_principal_id", "text"),
    ("bound_role", "text"),
    ("bound_claims_hash", "text"),
    ("bound_policy_version", "text"),
    ("bound_knowledge_version", "text"),
    ("bound_config_version", "text"),
    ("state_json", "text"),
    ("journal_len", "integer"),
];

pub const EXPECTED_MANIFEST_COLUMNS: [(&str, &str); 2] =
    [("singleton", "boolean"), ("sql_sha256", "text")];

pub const EXPECTED_HEAD_CONSTRAINTS: [&str; 6] = [
    "session_head_bound_all_or_none",
    "session_head_bound_project_matches",
    "session_head_generation_nonneg",
    "session_head_journal_bound",
    "session_head_pk",
    "session_head_revision_nonneg",
];

pub const EXPECTED_MANIFEST_CONSTRAINTS: [&str; 3] = [
    "install_manifest_one",
    "install_manifest_pk",
    "install_manifest_sha",
];

/// USAGE, SELECT, UPDATE on the head; no insert/delete/truncate; no manifest DML.
pub const EXPECTED_PRIVILEGES: [bool; 9] = [true, true, true, false, false, false, false, false, false];

#[cfg(test)]
mod tests {
    use super::{MIGRATION_SQL, UPDATE_HEAD};

    #[test]
    fn migration_does_not_autofix_or_reuse_foreign_names() {
        assert!(!MIGRATION_SQL.contains("IF NOT EXISTS"));
        assert!(!MIGRATION_SQL.contains("CREATE OR REPLACE"));
        assert!(!MIGRATION_SQL.contains("oo_control."));
        assert!(!MIGRATION_SQL.contains("schema_000"));
        assert!(MIGRATION_SQL.contains("oo_session_control"));
        assert!(MIGRATION_SQL.contains("GRANT SELECT, UPDATE"));
        assert!(!MIGRATION_SQL.contains("GRANT INSERT"));
        assert!(!MIGRATION_SQL.contains("GRANT ALL"));
    }

    #[test]
    fn statements_keep_identifiers_fixed_and_values_bound() {
        let source = include_str!("sql.rs");
        let format_bang = ["format", "!"].concat();
        assert!(!source.contains(&format_bang));
        assert!(UPDATE_HEAD.contains("$1"));
        assert!(UPDATE_HEAD.contains("oo_session_control.session_head"));
        assert!(source.contains("FOR UPDATE"));
    }
}
