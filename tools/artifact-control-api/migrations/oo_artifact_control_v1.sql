-- oo.artifact-control/v1
-- Schema oo_artifact_control. The adapter hashes this file. A different hash is drift.
-- No DELETE grant. A larger session end is rejected by trigger.

CREATE SCHEMA oo_artifact_control;

REVOKE ALL ON SCHEMA oo_artifact_control FROM PUBLIC;
GRANT USAGE ON SCHEMA oo_artifact_control TO oo_artifact_runtime;

CREATE FUNCTION oo_artifact_control.reject_mutation() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  RAISE EXCEPTION 'audit_mutation' USING ERRCODE = '42501';
END;
$$;

CREATE FUNCTION oo_artifact_control.reject_extension() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF NEW.absolute_end_unix IS DISTINCT FROM OLD.absolute_end_unix THEN
    RAISE EXCEPTION 'session_extended' USING ERRCODE = '42501';
  END IF;
  RETURN NEW;
END;
$$;

REVOKE ALL ON FUNCTION oo_artifact_control.reject_mutation() FROM PUBLIC;
REVOKE ALL ON FUNCTION oo_artifact_control.reject_extension() FROM PUBLIC;

CREATE TABLE oo_artifact_control.install_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    CONSTRAINT install_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT install_manifest_one CHECK (singleton),
    CONSTRAINT install_manifest_sql CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_artifact_control.session_anchor (
    session_id text NOT NULL,
    principal_id text NOT NULL,
    project_id text NOT NULL,
    absolute_end_unix bigint NOT NULL,
    CONSTRAINT session_anchor_pk PRIMARY KEY (session_id),
    CONSTRAINT session_anchor_session CHECK (session_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT session_anchor_principal CHECK (principal_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT session_anchor_project CHECK (project_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT session_anchor_end CHECK (absolute_end_unix >= 0)
);

CREATE TRIGGER session_anchor_no_extension
BEFORE UPDATE ON oo_artifact_control.session_anchor
FOR EACH ROW EXECUTE FUNCTION oo_artifact_control.reject_extension();

CREATE TABLE oo_artifact_control.artifact (
    object_id text NOT NULL,
    deployment text NOT NULL,
    principal_id text NOT NULL,
    project_id text NOT NULL,
    session_id text NOT NULL,
    role text NOT NULL,
    policy_id text NOT NULL,
    knowledge_id text NOT NULL,
    knowledge_digest text NOT NULL,
    media_type text NOT NULL,
    plaintext_sha256 text NOT NULL,
    plaintext_size bigint NOT NULL,
    source_sha256 text NOT NULL,
    sidecar_sha256 text NOT NULL,
    nonce bytea NOT NULL,
    ciphertext_sha256 text NOT NULL,
    cipher text NOT NULL,
    created_unix_ms bigint NOT NULL,
    expires_unix_ms bigint NOT NULL,
    absolute_end_unix bigint NOT NULL,
    state text NOT NULL,
    CONSTRAINT artifact_pk PRIMARY KEY (object_id),
    CONSTRAINT artifact_id CHECK (object_id ~ '^[0-9a-f]{32}$'),
    CONSTRAINT artifact_deployment CHECK (deployment ~ '^[a-z0-9][a-z0-9_-]{0,62}$'),
    CONSTRAINT artifact_principal CHECK (principal_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT artifact_project CHECK (project_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT artifact_session CHECK (session_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT artifact_role CHECK (role ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT artifact_policy CHECK (policy_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT artifact_knowledge CHECK (knowledge_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT artifact_digest CHECK (knowledge_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT artifact_media CHECK (media_type = 'text/csv'),
    CONSTRAINT artifact_plain CHECK (plaintext_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT artifact_size CHECK (plaintext_size BETWEEN 1 AND 33554432),
    CONSTRAINT artifact_source CHECK (source_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT artifact_side CHECK (sidecar_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT artifact_nonce_len CHECK (octet_length(nonce) = 12),
    CONSTRAINT artifact_nonce_unique UNIQUE (nonce),
    CONSTRAINT artifact_ct CHECK (ciphertext_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT artifact_cipher CHECK (cipher = 'ring-aes-256-gcm'),
    CONSTRAINT artifact_created CHECK (created_unix_ms >= 0),
    CONSTRAINT artifact_expires CHECK (expires_unix_ms = created_unix_ms + 86400000),
    CONSTRAINT artifact_end CHECK (absolute_end_unix >= 0),
    CONSTRAINT artifact_state CHECK (state IN ('active', 'revoked', 'expired'))
);

CREATE TABLE oo_artifact_control.artifact_loss (
    loss_id text NOT NULL,
    session_id text NOT NULL,
    absolute_end_unix bigint NOT NULL,
    reason text NOT NULL,
    source_sha256 text NOT NULL,
    created_unix_ms bigint NOT NULL,
    CONSTRAINT artifact_loss_pk PRIMARY KEY (loss_id),
    CONSTRAINT artifact_loss_id CHECK (loss_id ~ '^[0-9a-f]{32}$'),
    CONSTRAINT artifact_loss_session CHECK (session_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT artifact_loss_end CHECK (absolute_end_unix >= 0),
    CONSTRAINT artifact_loss_reason CHECK (reason IN ('checksum', 'secret_scan', 'incomplete', 'producer')),
    CONSTRAINT artifact_loss_source CHECK (source_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT artifact_loss_created CHECK (created_unix_ms >= 0)
);

CREATE TRIGGER artifact_loss_append_only
BEFORE UPDATE OR DELETE ON oo_artifact_control.artifact_loss
FOR EACH ROW EXECUTE FUNCTION oo_artifact_control.reject_mutation();

CREATE TABLE oo_artifact_control.session_cookie (
    cookie_sha256 text NOT NULL,
    csrf_sha256 text NOT NULL,
    session_id text NOT NULL,
    principal_id text NOT NULL,
    project_id text NOT NULL,
    CONSTRAINT session_cookie_pk PRIMARY KEY (cookie_sha256),
    CONSTRAINT session_cookie_sha CHECK (cookie_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT session_cookie_csrf CHECK (csrf_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT session_cookie_session CHECK (session_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT session_cookie_principal CHECK (principal_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT session_cookie_project CHECK (project_id ~ '^[a-z][a-z0-9_]{0,62}$')
);

CREATE TABLE oo_artifact_control.idempotency (
    session_id text NOT NULL,
    idempotency_key text NOT NULL,
    body_sha256 text NOT NULL,
    response jsonb NOT NULL,
    CONSTRAINT idempotency_pk PRIMARY KEY (session_id, idempotency_key),
    CONSTRAINT idempotency_session CHECK (session_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT idempotency_key CHECK (idempotency_key ~ '^[A-Za-z0-9_-]{16,128}$'),
    CONSTRAINT idempotency_body CHECK (body_sha256 ~ '^[0-9a-f]{64}$')
);

GRANT SELECT, INSERT, UPDATE ON oo_artifact_control.install_manifest TO oo_artifact_runtime;
GRANT SELECT, INSERT ON oo_artifact_control.session_anchor TO oo_artifact_runtime;
GRANT SELECT, INSERT, UPDATE ON oo_artifact_control.artifact TO oo_artifact_runtime;
GRANT SELECT, INSERT ON oo_artifact_control.artifact_loss TO oo_artifact_runtime;
GRANT SELECT, INSERT ON oo_artifact_control.session_cookie TO oo_artifact_runtime;
GRANT SELECT, INSERT ON oo_artifact_control.idempotency TO oo_artifact_runtime;
