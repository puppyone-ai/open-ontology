-- oo.selfbuild-control-api/v1
-- Schema oo_selfbuild_api. Not jobs, audit, identity, or review_ledger.
-- The adapter hashes this file before execution. A different hash is drift.
-- This script does not CREATE ROLE, does not GRANT data privileges, and
-- does not install a publication.

CREATE SCHEMA oo_selfbuild_api;

REVOKE ALL ON SCHEMA oo_selfbuild_api FROM PUBLIC;

CREATE TABLE oo_selfbuild_api.install_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    CONSTRAINT install_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT install_manifest_one CHECK (singleton),
    CONSTRAINT install_manifest_sql CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_selfbuild_api.build_job (
    build_id text NOT NULL,
    project_id text NOT NULL,
    idempotency_key text NOT NULL,
    body_sha256 text NOT NULL,
    config_commit text NOT NULL,
    source_ref text NOT NULL,
    allowance_id text NOT NULL,
    llm_secret_ref text NOT NULL,
    identity_revision bigint NOT NULL,
    subject text NOT NULL,
    role_name text NOT NULL,
    status text NOT NULL,
    stage text NOT NULL,
    input_sha256 text NOT NULL,
    lease_owner text,
    lease_until timestamptz,
    lease_generation bigint NOT NULL DEFAULT 0,
    child_pid bigint,
    child_exited boolean NOT NULL DEFAULT false,
    failure_code text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT build_job_pk PRIMARY KEY (build_id),
    CONSTRAINT build_job_idem UNIQUE (project_id, idempotency_key),
    CONSTRAINT build_job_id CHECK (build_id ~ '^b_[0-9a-f]{32}$'),
    CONSTRAINT build_job_project CHECK (project_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT build_job_key CHECK (idempotency_key ~ '^[A-Za-z0-9_-]{16,128}$'),
    CONSTRAINT build_job_body CHECK (body_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT build_job_commit CHECK (config_commit ~ '^[0-9a-f]{64}$'),
    CONSTRAINT build_job_source CHECK (source_ref ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT build_job_allowance CHECK (allowance_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT build_job_secret CHECK (llm_secret_ref ~ '^secret://[A-Za-z0-9_./-]{1,128}$'),
    CONSTRAINT build_job_revision CHECK (identity_revision >= 1),
    CONSTRAINT build_job_subject CHECK (char_length(subject) BETWEEN 1 AND 256),
    CONSTRAINT build_job_status CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancel_requested', 'canceled')),
    CONSTRAINT build_job_input CHECK (input_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT build_job_lease CHECK (lease_generation >= 0)
);

CREATE INDEX build_job_claim ON oo_selfbuild_api.build_job (status, created_at);

CREATE TABLE oo_selfbuild_api.stage_checkpoint (
    build_id text NOT NULL,
    stage text NOT NULL,
    status text NOT NULL,
    input_sha256 text NOT NULL,
    output_sha256 text,
    detail_code text,
    CONSTRAINT stage_checkpoint_pk PRIMARY KEY (build_id, stage),
    CONSTRAINT stage_checkpoint_job FOREIGN KEY (build_id) REFERENCES oo_selfbuild_api.build_job (build_id),
    CONSTRAINT stage_checkpoint_stage CHECK (stage IN ('S0', 'S1', 'S2', 'S3', 'S4', 'S5', 'S6', 'S7', 'S8', 'S9', 'S10')),
    CONSTRAINT stage_checkpoint_status CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'refused', 'awaiting')),
    CONSTRAINT stage_checkpoint_input CHECK (input_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT stage_checkpoint_output CHECK (output_sha256 IS NULL OR output_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_selfbuild_api.member (
    build_id text NOT NULL,
    project_id text NOT NULL,
    object_id text NOT NULL,
    object_kind text NOT NULL,
    schema_name text NOT NULL,
    table_name text NOT NULL,
    column_name text,
    candidate_sha256 text NOT NULL,
    source_sha256 text NOT NULL,
    schema_sha256 text NOT NULL,
    evidence_sha256 text NOT NULL,
    generation_sha256 text NOT NULL,
    CONSTRAINT member_pk PRIMARY KEY (build_id, object_id),
    CONSTRAINT member_job FOREIGN KEY (build_id) REFERENCES oo_selfbuild_api.build_job (build_id),
    CONSTRAINT member_kind CHECK (object_kind IN ('dataset', 'column')),
    CONSTRAINT member_hash CHECK (
        candidate_sha256 ~ '^[0-9a-f]{64}$'
        AND source_sha256 ~ '^[0-9a-f]{64}$'
        AND schema_sha256 ~ '^[0-9a-f]{64}$'
        AND evidence_sha256 ~ '^[0-9a-f]{64}$'
        AND generation_sha256 ~ '^[0-9a-f]{64}$'
    )
);

CREATE TABLE oo_selfbuild_api.audit_event (
    event_id bigint GENERATED ALWAYS AS IDENTITY,
    project_id text NOT NULL,
    build_id text,
    action text NOT NULL,
    subject text NOT NULL,
    detail_code text NOT NULL,
    body_sha256 text,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT audit_event_pk PRIMARY KEY (event_id),
    CONSTRAINT audit_event_body CHECK (body_sha256 IS NULL OR body_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_selfbuild_api.decision_record (
    project_id text NOT NULL,
    idempotency_key text NOT NULL,
    build_id text NOT NULL,
    object_id text NOT NULL,
    decision_kind text NOT NULL,
    choice text NOT NULL,
    body_sha256 text NOT NULL,
    identity_revision bigint NOT NULL,
    subject text NOT NULL,
    ledger_code text NOT NULL,
    ledger_authenticated boolean NOT NULL,
    management_identity text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT decision_record_pk PRIMARY KEY (project_id, idempotency_key),
    CONSTRAINT decision_record_job FOREIGN KEY (build_id) REFERENCES oo_selfbuild_api.build_job (build_id),
    CONSTRAINT decision_record_body CHECK (body_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT decision_record_ledger CHECK (ledger_authenticated = false),
    CONSTRAINT decision_record_identity CHECK (management_identity = 'verified'),
    CONSTRAINT decision_record_revision CHECK (identity_revision >= 1)
);

CREATE TABLE oo_selfbuild_api.pack_record (
    build_id text NOT NULL,
    output_sha256 text NOT NULL,
    published boolean NOT NULL,
    active boolean NOT NULL,
    producer_authenticated boolean NOT NULL,
    reseal_count bigint,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT pack_record_pk PRIMARY KEY (build_id),
    CONSTRAINT pack_record_job FOREIGN KEY (build_id) REFERENCES oo_selfbuild_api.build_job (build_id),
    CONSTRAINT pack_record_hash CHECK (output_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT pack_record_published CHECK (published = false),
    CONSTRAINT pack_record_active CHECK (active = false),
    CONSTRAINT pack_record_auth CHECK (producer_authenticated = false)
);

REVOKE ALL ON ALL TABLES IN SCHEMA oo_selfbuild_api FROM PUBLIC;

CREATE FUNCTION oo_selfbuild_api.reject_mutation() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'insert-only' USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER audit_event_insert_only
    BEFORE UPDATE OR DELETE ON oo_selfbuild_api.audit_event
    FOR EACH ROW EXECUTE FUNCTION oo_selfbuild_api.reject_mutation();

CREATE TRIGGER decision_record_insert_only
    BEFORE UPDATE OR DELETE ON oo_selfbuild_api.decision_record
    FOR EACH ROW EXECUTE FUNCTION oo_selfbuild_api.reject_mutation();

CREATE TRIGGER pack_record_insert_only
    BEFORE UPDATE OR DELETE ON oo_selfbuild_api.pack_record
    FOR EACH ROW EXECUTE FUNCTION oo_selfbuild_api.reject_mutation();
