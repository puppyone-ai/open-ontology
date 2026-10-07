//! Fixed SQL. Identifiers are literals. Caller values are parameters.

pub const MIGRATION_SQL: &str = include_str!("../migrations/oo_release_control_v1.sql");
pub const SCHEMA: &str = "oo_release_control";
pub const ADMIN_ROLE: &str = "oo_release_admin";
pub const INSTALL_LOCK: i64 = 881_275_359;

pub const SET_LOCAL: &str = "\
SET LOCAL search_path TO pg_catalog; \
SET LOCAL lock_timeout = '10s'; \
SET LOCAL statement_timeout = '15s'; \
SET LOCAL synchronous_commit = on";

pub const LOCK_INSTALL: &str = "SELECT pg_catalog.pg_advisory_xact_lock($1::bigint)";
pub const CURRENT_USER: &str = "SELECT current_user";
pub const TLS_FACT: &str = "\
SELECT ssl, version FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()";
pub const SCHEMA_EXISTS: &str = "SELECT 1 FROM pg_catalog.pg_namespace WHERE nspname = $1";

pub const INSERT_MANIFEST: &str = "\
INSERT INTO oo_release_control.install_manifest (singleton, sql_sha256) VALUES (TRUE, $1)";
pub const READ_MANIFEST: &str = "SELECT sql_sha256 FROM oo_release_control.install_manifest WHERE singleton";

pub const INSERT_DEPLOYMENT: &str = "\
INSERT INTO oo_release_control.deployment ( \
    singleton, project_id, authority_id, authority_class, authority_statement, authority_statement_sha256, \
    product_mode_can_activate, production_ready_certificate, full_sp08_accepted, public_deployment_claimed, \
    customer_ownership_claimed, standby_configured, owner_authenticated, credential_verified, \
    integrity_verified, customer_gate_verified, runsc_verified \
) VALUES (TRUE, $1, $2, 'bounded_local_fixture', $3, $4, FALSE, FALSE, FALSE, FALSE, FALSE, FALSE, FALSE, FALSE, FALSE, FALSE, FALSE)";

pub const INSERT_AUTHORITY_GATE: &str = "INSERT INTO oo_release_control.authority_gate (gate_id) VALUES ($1)";
pub const READ_DEPLOYMENT: &str = "\
SELECT project_id, authority_id, authority_statement_sha256, product_mode_can_activate, owner_authenticated, \
       credential_verified, integrity_verified, customer_gate_verified, runsc_verified, full_sp08_accepted \
FROM oo_release_control.deployment WHERE singleton";
pub const READ_AUTHORITY_GATES: &str = "SELECT gate_id FROM oo_release_control.authority_gate ORDER BY gate_id";

pub const INSERT_POINTER: &str = "\
INSERT INTO oo_release_control.active_pointer (singleton, project_id, version, candidate_id, source_hash, pack_digest, manifest_sha256) \
VALUES (TRUE, $1, NULL, NULL, NULL, NULL, NULL)";
pub const LOCK_POINTER: &str = "\
SELECT version, candidate_id, source_hash, pack_digest, manifest_sha256 \
FROM oo_release_control.active_pointer WHERE singleton FOR UPDATE";
pub const READ_POINTER: &str = "\
SELECT version, candidate_id, source_hash, pack_digest, manifest_sha256 \
FROM oo_release_control.active_pointer WHERE singleton";
pub const UPDATE_POINTER: &str = "\
UPDATE oo_release_control.active_pointer \
SET version = $1, candidate_id = $2, source_hash = $3, pack_digest = $4, manifest_sha256 = $5 \
WHERE singleton AND project_id = $6 AND version IS NOT DISTINCT FROM $7";

pub const READ_CANDIDATE_LOCK: &str = "\
SELECT candidate_id, project_id, version, base_version, state, source_hash, effective_oos, visibility_json, \
       effective_sha256, visibility_sha256, pack_digest, manifest_sha256, policy_id, policy_version, \
       knowledge_id, knowledge_version, ontology_version, catalog_version, role_name, compiler, \
       plan_hash, plan_scope, plan_epoch \
FROM oo_release_control.candidate WHERE candidate_id = $1 FOR UPDATE";
pub const READ_CANDIDATE: &str = "\
SELECT candidate_id, project_id, version, base_version, state, source_hash, effective_oos, visibility_json, \
       effective_sha256, visibility_sha256, pack_digest, manifest_sha256, policy_id, policy_version, \
       knowledge_id, knowledge_version, ontology_version, catalog_version, role_name, compiler, \
       plan_hash, plan_scope, plan_epoch \
FROM oo_release_control.candidate WHERE candidate_id = $1";
pub const READ_CANDIDATE_VERSION: &str = "\
SELECT candidate_id, state, source_hash, pack_digest, manifest_sha256 \
FROM oo_release_control.candidate WHERE version = $1";

pub const INSERT_CANDIDATE: &str = "\
INSERT INTO oo_release_control.candidate ( \
    candidate_id, project_id, version, base_version, state, source_hash, effective_oos, visibility_json, \
    effective_sha256, visibility_sha256, pack_digest, manifest_sha256, policy_id, policy_version, \
    knowledge_id, knowledge_version, ontology_version, catalog_version, role_name, compiler, \
    plan_hash, plan_scope, plan_epoch \
) VALUES ( \
    $1, $2, $3, $4, 'candidate_built', $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, \
    NULL, NULL, NULL \
)";

pub const UPDATE_SEAL: &str = "\
UPDATE oo_release_control.candidate SET \
    base_version = $2, state = 'candidate_built', source_hash = $3, effective_oos = $4, visibility_json = $5, \
    effective_sha256 = $6, visibility_sha256 = $7, pack_digest = $8, manifest_sha256 = $9, \
    policy_id = $10, policy_version = $11, knowledge_id = $12, knowledge_version = $13, \
    ontology_version = $14, catalog_version = $15, role_name = $16, compiler = $17, \
    plan_hash = NULL, plan_scope = NULL, plan_epoch = NULL \
WHERE candidate_id = $1 AND state IN ('candidate_built', 'gates_passed', 'failed')";

pub const DELETE_GATES: &str = "DELETE FROM oo_release_control.gate_report WHERE candidate_id = $1";
pub const DELETE_CHECKPOINTS: &str = "DELETE FROM oo_release_control.checkpoint WHERE candidate_id = $1";
pub const DELETE_FILES: &str = "DELETE FROM oo_release_control.candidate_file WHERE candidate_id = $1";
pub const INSERT_FILE: &str = "\
INSERT INTO oo_release_control.candidate_file (candidate_id, path, sha256, byte_len) VALUES ($1, $2, $3, $4)";

pub const INSERT_GATE: &str = "\
INSERT INTO oo_release_control.gate_report ( \
    candidate_id, certificate_sha256, gate_id, source_hash, issuer_class, status, blocking, scope, \
    product_mode_can_activate, report_type, authority_statement_sha256, disposition, detail \
) VALUES ($1, $2, $3, $4, $5, $6, $7, 'test_scope', FALSE, $8, $9, $10, $11)";
pub const READ_GATES: &str = "\
SELECT certificate_sha256, gate_id, source_hash, issuer_class, status, blocking, scope, report_type, authority_statement_sha256 \
FROM oo_release_control.gate_report WHERE candidate_id = $1 ORDER BY gate_id, certificate_sha256";
pub const READ_GATE_ONE: &str = "\
SELECT disposition, detail FROM oo_release_control.gate_report WHERE candidate_id = $1 AND certificate_sha256 = $2";

pub const INSERT_CHECKPOINT: &str = "\
INSERT INTO oo_release_control.checkpoint ( \
    candidate_id, kind, certificate_sha256, source_hash, epoch, barrier, sessions_terminated, \
    issuer_class, authority_statement_sha256, disposition \
) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)";
pub const READ_CHECKPOINTS: &str = "\
SELECT kind, certificate_sha256, source_hash, epoch, barrier, sessions_terminated, issuer_class, authority_statement_sha256 \
FROM oo_release_control.checkpoint WHERE candidate_id = $1 ORDER BY kind, certificate_sha256";
pub const READ_CHECKPOINT_ONE: &str = "\
SELECT disposition FROM oo_release_control.checkpoint WHERE candidate_id = $1 AND certificate_sha256 = $2";

pub const UPDATE_PLAN: &str = "\
UPDATE oo_release_control.candidate \
SET state = $2, plan_hash = $3, plan_scope = $4, plan_epoch = $5 \
WHERE candidate_id = $1 AND state = $6";
pub const UPDATE_STATE: &str = "\
UPDATE oo_release_control.candidate SET state = $2 WHERE candidate_id = $1 AND state = $3";
pub const MARK_INACTIVE: &str = "\
UPDATE oo_release_control.candidate SET state = 'inactive' WHERE candidate_id = $1 AND state = 'active'";
pub const MARK_ACTIVE: &str = "\
UPDATE oo_release_control.candidate SET state = 'active', plan_hash = $2, plan_scope = 'test_scope', plan_epoch = $3 \
WHERE candidate_id = $1 AND state = 'applying'";

pub const INSERT_AUDIT: &str = "\
INSERT INTO oo_release_control.audit_event ( \
    audit_id, project_id, candidate_id, event, previous_version, new_version, source_hash, detail \
) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)";
pub const READ_AUDIT: &str = "SELECT detail FROM oo_release_control.audit_event WHERE audit_id = $1";
pub const READ_EVENT_AUDIT: &str = "\
SELECT audit_id FROM oo_release_control.audit_event WHERE candidate_id = $1 AND event = $2";

pub const READ_FILES: &str = "\
SELECT path, sha256, byte_len FROM oo_release_control.candidate_file WHERE candidate_id = $1 ORDER BY path";
