-- oo.source-control-api/v1
-- Own schema oo_source_control. No data-role GRANT and no login.

CREATE SCHEMA oo_source_control;

REVOKE ALL ON SCHEMA oo_source_control FROM PUBLIC;

CREATE FUNCTION oo_source_control.reject_mutation() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  RAISE EXCEPTION 'audit_mutation' USING ERRCODE = '42501';
END;
$$;

REVOKE ALL ON FUNCTION oo_source_control.reject_mutation() FROM PUBLIC;

CREATE TABLE oo_source_control.install_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    CONSTRAINT install_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT install_manifest_one CHECK (singleton),
    CONSTRAINT install_manifest_sql CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_source_control.source (
    source_ref text NOT NULL,
    project_id text NOT NULL,
    kind text NOT NULL,
    revision bigint NOT NULL,
    state text NOT NULL,
    secret_ref text,
    host text,
    port integer,
    database_name text,
    username text,
    tables_json text,
    CONSTRAINT source_pk PRIMARY KEY (source_ref),
    CONSTRAINT source_ref CHECK (source_ref ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT source_project CHECK (project_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT source_kind CHECK (kind IN ('pg_local', 'file_local')),
    CONSTRAINT source_revision CHECK (revision >= 1),
    CONSTRAINT source_state CHECK (state IN ('registered', 'importing', 'awaiting_review', 'failed', 'canceled')),
    CONSTRAINT source_port CHECK (port IS NULL OR port BETWEEN 1 AND 65535)
);

CREATE TABLE oo_source_control.object_blob (
    source_ref text NOT NULL,
    sha256 text NOT NULL,
    byte_len bigint NOT NULL,
    stored_name text NOT NULL,
    CONSTRAINT object_pk PRIMARY KEY (source_ref, sha256),
    CONSTRAINT object_sha CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT object_len CHECK (byte_len >= 0 AND byte_len <= 16777216)
);

CREATE TABLE oo_source_control.operation (
    operation_id text NOT NULL,
    project_id text NOT NULL,
    source_ref text NOT NULL,
    kind text NOT NULL,
    status text NOT NULL,
    attempt integer NOT NULL,
    idempotency_key text NOT NULL,
    body_sha256 text NOT NULL,
    identity_revision bigint NOT NULL,
    subject text NOT NULL,
    input_json text NOT NULL,
    producer_code text,
    producer_detail text,
    rows_copied bigint,
    child_started boolean NOT NULL,
    catalog_sha256 text,
    profile_sha256 text,
    cancel_requested boolean NOT NULL,
    pid integer,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT operation_pk PRIMARY KEY (operation_id),
    CONSTRAINT operation_id CHECK (operation_id ~ '^op_[0-9a-f]{32}$'),
    CONSTRAINT operation_kind CHECK (kind IN ('pg_inspect', 'file_replace')),
    CONSTRAINT operation_status CHECK (status IN (
        'queued', 'running', 'awaiting_review', 'failed',
        'canceled', 'cancel_requested', 'resume_uncertain'
    )),
    CONSTRAINT operation_attempt CHECK (attempt >= 1),
    CONSTRAINT operation_rows CHECK (rows_copied IS NULL OR rows_copied >= 0),
    CONSTRAINT operation_child CHECK (child_started IN (true, false))
);

CREATE TABLE oo_source_control.document (
    operation_id text NOT NULL,
    doc_kind text NOT NULL,
    sha256 text NOT NULL,
    body text NOT NULL,
    CONSTRAINT document_pk PRIMARY KEY (operation_id, doc_kind),
    CONSTRAINT document_kind CHECK (doc_kind IN ('catalog', 'profile')),
    CONSTRAINT document_sha CHECK (sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_source_control.idempotency (
    project_id text NOT NULL,
    idempotency_key text NOT NULL,
    body_sha256 text NOT NULL,
    resource_id text NOT NULL,
    CONSTRAINT idempotency_pk PRIMARY KEY (project_id, idempotency_key),
    CONSTRAINT idempotency_sha CHECK (body_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_source_control.audit (
    audit_id bigint GENERATED ALWAYS AS IDENTITY,
    at timestamptz NOT NULL DEFAULT now(),
    subject text NOT NULL,
    project_id text NOT NULL,
    action text NOT NULL,
    object_id text NOT NULL,
    outcome text NOT NULL,
    CONSTRAINT audit_pk PRIMARY KEY (audit_id)
);

CREATE TRIGGER audit_no_update
    BEFORE UPDATE OR DELETE ON oo_source_control.audit
    FOR EACH ROW EXECUTE FUNCTION oo_source_control.reject_mutation();

CREATE INDEX operation_queue ON oo_source_control.operation (project_id, status, created_at);
