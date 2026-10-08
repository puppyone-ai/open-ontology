-- oo.release-control-api/v1. The service hashes this file before execution.
-- Exact replay does not execute it again. A different hash is drift.
-- This script creates no login and grants no data role.

CREATE SCHEMA oo_release_api;

CREATE TABLE oo_release_api.install_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    CONSTRAINT install_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT install_manifest_one CHECK (singleton),
    CONSTRAINT install_manifest_sha CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_release_api.operation (
    operation_id text NOT NULL,
    project_id text NOT NULL,
    idempotency_key text NOT NULL,
    body_sha256 text NOT NULL,
    kind text NOT NULL,
    status text NOT NULL,
    candidate_id text,
    version text,
    base_version text,
    expected_version text,
    role_name text,
    scope_name text,
    config_commit text,
    source_hash text,
    pack_digest text,
    manifest_sha256 text,
    columns_text text NOT NULL,
    policy_id text,
    policy_version text,
    knowledge_id text,
    knowledge_version text,
    catalog_version text,
    review_admitted boolean NOT NULL,
    producer_outcome text,
    producer_reason text,
    certificate_class text NOT NULL,
    production_ready boolean NOT NULL,
    customer_gate_verified boolean NOT NULL,
    owner_authenticated boolean NOT NULL,
    runsc_verified boolean NOT NULL,
    gates_json text NOT NULL,
    subject_name text NOT NULL,
    target_operation_id text,
    CONSTRAINT operation_pk PRIMARY KEY (operation_id),
    CONSTRAINT operation_id CHECK (operation_id ~ '^op_[0-9a-f]{32}$'),
    CONSTRAINT operation_project CHECK (project_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT operation_key CHECK (idempotency_key ~ '^[A-Za-z0-9_-]{16,128}$'),
    CONSTRAINT operation_body CHECK (body_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT operation_kind CHECK (kind IN ('publish', 'rollback')),
    CONSTRAINT operation_status CHECK (status IN (
        'queued', 'running', 'succeeded', 'failed', 'refused', 'resume_uncertain'
    )),
    CONSTRAINT operation_scope CHECK (scope_name IS NULL OR scope_name IN ('test_scope', 'production')),
    CONSTRAINT operation_commit CHECK (config_commit IS NULL OR config_commit ~ '^[0-9a-f]{64}$'),
    CONSTRAINT operation_source CHECK (source_hash IS NULL OR source_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT operation_pack CHECK (pack_digest IS NULL OR pack_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT operation_manifest CHECK (manifest_sha256 IS NULL OR manifest_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT operation_cert CHECK (certificate_class IN ('not_issued', 'test_scope_not_production')),
    CONSTRAINT operation_flags CHECK (
        production_ready = FALSE
        AND customer_gate_verified = FALSE
        AND owner_authenticated = FALSE
        AND runsc_verified = FALSE
    ),
    CONSTRAINT operation_review CHECK (review_admitted IN (TRUE, FALSE)),
    CONSTRAINT operation_idem UNIQUE (project_id, idempotency_key)
);

CREATE INDEX operation_version ON oo_release_api.operation (project_id, version);

CREATE TABLE oo_release_api.audit_event (
    audit_id text NOT NULL,
    operation_id text,
    project_id text NOT NULL,
    event_name text NOT NULL,
    detail text NOT NULL,
    CONSTRAINT audit_event_pk PRIMARY KEY (audit_id),
    CONSTRAINT audit_event_id CHECK (audit_id ~ '^[0-9a-f]{64}$'),
    CONSTRAINT audit_event_name CHECK (event_name ~ '^[a-z0-9._]{1,64}$'),
    CONSTRAINT audit_event_detail CHECK (detail ~ '^[a-z0-9._]{1,64}$')
);

CREATE FUNCTION oo_release_api.audit_immutable() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'audit_immutable' USING ERRCODE = '42501';
END;
$$;

CREATE TRIGGER audit_event_no_update
    BEFORE UPDATE OR DELETE ON oo_release_api.audit_event
    FOR EACH ROW EXECUTE FUNCTION oo_release_api.audit_immutable();

REVOKE ALL ON SCHEMA oo_release_api FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA oo_release_api FROM PUBLIC;
