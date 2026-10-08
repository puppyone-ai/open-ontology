-- oo.secret-control/v1
-- Schema oo_secret_control. The adapter hashes this file. A different hash is drift.
-- No DELETE grant. Customer KMS is not represented here.

CREATE SCHEMA oo_secret_control;

REVOKE ALL ON SCHEMA oo_secret_control FROM PUBLIC;
GRANT USAGE ON SCHEMA oo_secret_control TO oo_secret_runtime;

CREATE FUNCTION oo_secret_control.reject_mutation() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  RAISE EXCEPTION 'audit_mutation' USING ERRCODE = '42501';
END;
$$;

REVOKE ALL ON FUNCTION oo_secret_control.reject_mutation() FROM PUBLIC;

CREATE TABLE oo_secret_control.install_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    CONSTRAINT install_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT install_manifest_one CHECK (singleton),
    CONSTRAINT install_manifest_sql CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_secret_control.secret_version (
    deployment text NOT NULL,
    project_id text NOT NULL,
    namespace text NOT NULL,
    secret_id text NOT NULL,
    version integer NOT NULL,
    nonce bytea NOT NULL,
    ciphertext bytea NOT NULL,
    cipher text NOT NULL,
    state text NOT NULL,
    CONSTRAINT secret_version_pk PRIMARY KEY (deployment, project_id, namespace, secret_id, version),
    CONSTRAINT secret_version_deployment CHECK (deployment ~ '^[a-z0-9][a-z0-9_-]{0,62}$'),
    CONSTRAINT secret_version_project CHECK (project_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT secret_version_namespace CHECK (namespace ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT secret_version_id CHECK (secret_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT secret_version_version CHECK (version BETWEEN 1 AND 1000000),
    CONSTRAINT secret_version_nonce_len CHECK (octet_length(nonce) = 12),
    CONSTRAINT secret_version_nonce_unique UNIQUE (nonce),
    CONSTRAINT secret_version_ct CHECK (octet_length(ciphertext) BETWEEN 16 AND 8192),
    CONSTRAINT secret_version_cipher CHECK (cipher = 'ring-aes-256-gcm'),
    CONSTRAINT secret_version_state CHECK (state IN ('active', 'retired', 'revoked'))
);

CREATE TABLE oo_secret_control.secret_head (
    deployment text NOT NULL,
    project_id text NOT NULL,
    namespace text NOT NULL,
    secret_id text NOT NULL,
    current_version integer NOT NULL,
    state text NOT NULL,
    allowed_workload text NOT NULL,
    secret_ref text NOT NULL,
    subject text NOT NULL,
    CONSTRAINT secret_head_pk PRIMARY KEY (deployment, project_id, namespace, secret_id),
    CONSTRAINT secret_head_deployment CHECK (deployment ~ '^[a-z0-9][a-z0-9_-]{0,62}$'),
    CONSTRAINT secret_head_project CHECK (project_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT secret_head_namespace CHECK (namespace ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT secret_head_id CHECK (secret_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT secret_head_version CHECK (current_version BETWEEN 1 AND 1000000),
    CONSTRAINT secret_head_state CHECK (state IN ('active', 'revoked')),
    CONSTRAINT secret_head_workload CHECK (allowed_workload ~ '^urn:oo:workload:[A-Za-z0-9_.:-]{1,128}$'),
    CONSTRAINT secret_head_ref CHECK (char_length(secret_ref) BETWEEN 16 AND 320),
    CONSTRAINT secret_head_subject CHECK (char_length(subject) BETWEEN 1 AND 256)
);

ALTER TABLE oo_secret_control.secret_head
    ADD CONSTRAINT secret_head_version_fk
    FOREIGN KEY (deployment, project_id, namespace, secret_id, current_version)
    REFERENCES oo_secret_control.secret_version (deployment, project_id, namespace, secret_id, version)
    DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE oo_secret_control.idempotency (
    project_id text NOT NULL,
    idempotency_key text NOT NULL,
    body_sha256 text NOT NULL,
    response jsonb NOT NULL,
    CONSTRAINT idempotency_pk PRIMARY KEY (project_id, idempotency_key),
    CONSTRAINT idempotency_project CHECK (project_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT idempotency_key CHECK (idempotency_key ~ '^[A-Za-z0-9_-]{16,128}$'),
    CONSTRAINT idempotency_hash CHECK (body_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT idempotency_no_value CHECK (NOT (response ? 'value'))
);

CREATE TABLE oo_secret_control.audit_event (
    id bigint GENERATED ALWAYS AS IDENTITY,
    at timestamptz NOT NULL DEFAULT pg_catalog.now(),
    action text NOT NULL,
    code text NOT NULL,
    project_id text NOT NULL DEFAULT '',
    namespace text NOT NULL DEFAULT '',
    secret_id text NOT NULL DEFAULT '',
    version integer,
    subject text NOT NULL DEFAULT '',
    workload text NOT NULL DEFAULT '',
    CONSTRAINT audit_event_pk PRIMARY KEY (id),
    CONSTRAINT audit_event_action CHECK (action IN ('write', 'rotate', 'revoke', 'resolve', 'resolve_rejected')),
    CONSTRAINT audit_event_code CHECK (char_length(code) BETWEEN 1 AND 64),
    CONSTRAINT audit_event_project CHECK (char_length(project_id) <= 63),
    CONSTRAINT audit_event_namespace CHECK (char_length(namespace) <= 63),
    CONSTRAINT audit_event_secret CHECK (char_length(secret_id) <= 63),
    CONSTRAINT audit_event_version CHECK (version IS NULL OR version BETWEEN 1 AND 1000000),
    CONSTRAINT audit_event_subject CHECK (char_length(subject) <= 256),
    CONSTRAINT audit_event_workload CHECK (char_length(workload) <= 160)
);

CREATE TRIGGER audit_event_append_only
    BEFORE UPDATE OR DELETE ON oo_secret_control.audit_event
    FOR EACH ROW EXECUTE FUNCTION oo_secret_control.reject_mutation();

REVOKE ALL ON oo_secret_control.install_manifest FROM PUBLIC;
REVOKE ALL ON oo_secret_control.secret_version FROM PUBLIC;
REVOKE ALL ON oo_secret_control.secret_head FROM PUBLIC;
REVOKE ALL ON oo_secret_control.idempotency FROM PUBLIC;
REVOKE ALL ON oo_secret_control.audit_event FROM PUBLIC;

GRANT SELECT, INSERT ON oo_secret_control.install_manifest TO oo_secret_runtime;
GRANT SELECT, INSERT, UPDATE ON oo_secret_control.secret_version TO oo_secret_runtime;
GRANT SELECT, INSERT, UPDATE ON oo_secret_control.secret_head TO oo_secret_runtime;
GRANT SELECT, INSERT, UPDATE ON oo_secret_control.idempotency TO oo_secret_runtime;
GRANT SELECT, INSERT ON oo_secret_control.audit_event TO oo_secret_runtime;
GRANT USAGE ON SEQUENCE oo_secret_control.audit_event_id_seq TO oo_secret_runtime;
