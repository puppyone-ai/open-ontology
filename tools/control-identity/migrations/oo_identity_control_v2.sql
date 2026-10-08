-- oo.control-identity/v1 binding generation.
-- Applied once after oo_identity_control_v1.sql. A different hash is drift.
-- This script does not GRANT data roles and does not create a customer principal.
-- The v1 script hash stays the install manifest hash.

CREATE TABLE oo_identity_control.binding_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    CONSTRAINT binding_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT binding_manifest_one CHECK (singleton),
    CONSTRAINT binding_manifest_sql CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_identity_control.binding_head (
    singleton boolean NOT NULL,
    revision bigint NOT NULL,
    config_commit text NOT NULL,
    git_commit text NOT NULL,
    document_sha256 text NOT NULL,
    CONSTRAINT binding_head_pk PRIMARY KEY (singleton),
    CONSTRAINT binding_head_one CHECK (singleton),
    CONSTRAINT binding_head_revision CHECK (revision >= 1),
    CONSTRAINT binding_head_commit CHECK (config_commit ~ '^[0-9a-f]{64}$'),
    CONSTRAINT binding_head_git CHECK (git_commit = '' OR git_commit ~ '^[0-9a-f]{40}$'),
    CONSTRAINT binding_head_document CHECK (document_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_identity_control.management_binding (
    project_id text NOT NULL,
    subject text NOT NULL,
    role text NOT NULL,
    CONSTRAINT management_binding_pk PRIMARY KEY (project_id, subject),
    CONSTRAINT management_binding_project CHECK (project_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT management_binding_subject CHECK (char_length(subject) BETWEEN 1 AND 256),
    CONSTRAINT management_binding_role CHECK (role IN ('admin', 'reviewer', 'steward', 'auditor', 'business_owner'))
);

CREATE TABLE oo_identity_control.binding_idempotency (
    idempotency_key text NOT NULL,
    request_sha256 text NOT NULL,
    revision bigint NOT NULL,
    config_commit text NOT NULL,
    git_commit text NOT NULL,
    document_sha256 text NOT NULL,
    receipt_sha256 text NOT NULL,
    CONSTRAINT binding_idempotency_pk PRIMARY KEY (idempotency_key),
    CONSTRAINT binding_idempotency_key CHECK (idempotency_key ~ '^[A-Za-z0-9._:-]{1,128}$'),
    CONSTRAINT binding_idempotency_request CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT binding_idempotency_revision CHECK (revision >= 1),
    CONSTRAINT binding_idempotency_commit CHECK (config_commit ~ '^[0-9a-f]{64}$'),
    CONSTRAINT binding_idempotency_git CHECK (git_commit = '' OR git_commit ~ '^[0-9a-f]{40}$'),
    CONSTRAINT binding_idempotency_document CHECK (document_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT binding_idempotency_receipt CHECK (receipt_sha256 ~ '^[0-9a-f]{64}$')
);

ALTER TABLE oo_identity_control.audit_event DROP CONSTRAINT audit_event_action;
ALTER TABLE oo_identity_control.audit_event ADD CONSTRAINT audit_event_action CHECK (action IN (
    'login_started', 'login_rejected', 'login_completed', 'logout',
    'bootstrap_issued', 'bootstrap_consumed', 'bootstrap_rejected',
    'binding_applied'
));

REVOKE ALL ON oo_identity_control.binding_manifest FROM PUBLIC;
REVOKE ALL ON oo_identity_control.binding_head FROM PUBLIC;
REVOKE ALL ON oo_identity_control.management_binding FROM PUBLIC;
REVOKE ALL ON oo_identity_control.binding_idempotency FROM PUBLIC;

GRANT SELECT ON oo_identity_control.binding_manifest TO oo_identity_runtime;
GRANT SELECT, UPDATE ON oo_identity_control.binding_head TO oo_identity_runtime;
GRANT SELECT, INSERT, UPDATE, DELETE ON oo_identity_control.management_binding TO oo_identity_runtime;
GRANT SELECT, INSERT ON oo_identity_control.binding_idempotency TO oo_identity_runtime;
