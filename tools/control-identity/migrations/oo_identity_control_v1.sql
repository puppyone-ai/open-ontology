-- oo.control-identity/v1
-- Own schema oo_identity_control. Not jobs, audit, identity, oo_session_control,
-- or oo_token_control. The adapter hashes this file. A different hash is drift.
-- This script does not GRANT data roles and does not create a customer principal.

CREATE SCHEMA oo_identity_control;

REVOKE ALL ON SCHEMA oo_identity_control FROM PUBLIC;
GRANT USAGE ON SCHEMA oo_identity_control TO oo_identity_runtime;

CREATE FUNCTION oo_identity_control.reject_mutation() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  RAISE EXCEPTION 'audit_mutation' USING ERRCODE = '42501';
END;
$$;

REVOKE ALL ON FUNCTION oo_identity_control.reject_mutation() FROM PUBLIC;

CREATE TABLE oo_identity_control.install_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    initialized boolean NOT NULL,
    CONSTRAINT install_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT install_manifest_one CHECK (singleton),
    CONSTRAINT install_manifest_sql CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_identity_control.oauth_transaction (
    handle_hash text NOT NULL,
    state_hash text NOT NULL,
    nonce_hash text NOT NULL,
    pkce_challenge text NOT NULL,
    redirect_uri text NOT NULL,
    return_to text NOT NULL,
    bootstrap_hash text,
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    CONSTRAINT oauth_transaction_pk PRIMARY KEY (handle_hash),
    CONSTRAINT oauth_transaction_handle CHECK (handle_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT oauth_transaction_state CHECK (state_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT oauth_transaction_nonce CHECK (nonce_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT oauth_transaction_pkce CHECK (pkce_challenge ~ '^[A-Za-z0-9_-]{43}$'),
    CONSTRAINT oauth_transaction_redirect CHECK (redirect_uri ~ '^https://[^?#]+$'),
    CONSTRAINT oauth_transaction_return CHECK (char_length(return_to) BETWEEN 1 AND 128),
    CONSTRAINT oauth_transaction_bootstrap CHECK (bootstrap_hash IS NULL OR bootstrap_hash ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_identity_control.session (
    cookie_hash text NOT NULL,
    csrf_hash text NOT NULL,
    subject text NOT NULL,
    issuer text NOT NULL,
    config_commit text NOT NULL,
    revision bigint NOT NULL,
    idle_expires_at timestamptz NOT NULL,
    absolute_expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    CONSTRAINT session_pk PRIMARY KEY (cookie_hash),
    CONSTRAINT session_cookie CHECK (cookie_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT session_csrf CHECK (csrf_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT session_subject CHECK (char_length(subject) BETWEEN 1 AND 256),
    CONSTRAINT session_issuer CHECK (issuer ~ '^https://[^/?#]+$'),
    CONSTRAINT session_commit CHECK (config_commit ~ '^[0-9a-f]{64}$'),
    CONSTRAINT session_revision CHECK (revision >= 1),
    CONSTRAINT session_window CHECK (absolute_expires_at >= idle_expires_at)
);

CREATE TABLE oo_identity_control.bootstrap_token (
    token_hash text NOT NULL,
    subject text NOT NULL,
    project_id text NOT NULL,
    role text NOT NULL,
    state text NOT NULL,
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    session_hash text NOT NULL DEFAULT '',
    CONSTRAINT bootstrap_token_pk PRIMARY KEY (token_hash),
    CONSTRAINT bootstrap_token_hash CHECK (token_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT bootstrap_token_subject CHECK (char_length(subject) BETWEEN 1 AND 256),
    CONSTRAINT bootstrap_token_project CHECK (project_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT bootstrap_token_role CHECK (role IN ('admin', 'reviewer', 'steward', 'auditor', 'business_owner')),
    CONSTRAINT bootstrap_token_state CHECK (state IN ('issued', 'consumed', 'superseded')),
    CONSTRAINT bootstrap_token_session CHECK (session_hash = '' OR session_hash ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_identity_control.audit_event (
    id bigint GENERATED ALWAYS AS IDENTITY,
    at timestamptz NOT NULL DEFAULT pg_catalog.now(),
    action text NOT NULL,
    code text NOT NULL,
    subject text NOT NULL DEFAULT '',
    session_hash text NOT NULL DEFAULT '',
    CONSTRAINT audit_event_pk PRIMARY KEY (id),
    CONSTRAINT audit_event_action CHECK (action IN (
        'login_started', 'login_rejected', 'login_completed', 'logout',
        'bootstrap_issued', 'bootstrap_consumed', 'bootstrap_rejected'
    )),
    CONSTRAINT audit_event_code CHECK (char_length(code) BETWEEN 1 AND 64),
    CONSTRAINT audit_event_subject CHECK (char_length(subject) <= 256),
    CONSTRAINT audit_event_session CHECK (session_hash = '' OR session_hash ~ '^[0-9a-f]{64}$')
);

CREATE TRIGGER audit_event_append_only
    BEFORE UPDATE OR DELETE ON oo_identity_control.audit_event
    FOR EACH ROW EXECUTE FUNCTION oo_identity_control.reject_mutation();

REVOKE ALL ON oo_identity_control.install_manifest FROM PUBLIC;
REVOKE ALL ON oo_identity_control.oauth_transaction FROM PUBLIC;
REVOKE ALL ON oo_identity_control.session FROM PUBLIC;
REVOKE ALL ON oo_identity_control.bootstrap_token FROM PUBLIC;
REVOKE ALL ON oo_identity_control.audit_event FROM PUBLIC;

GRANT SELECT, UPDATE ON oo_identity_control.install_manifest TO oo_identity_runtime;
GRANT SELECT, INSERT, UPDATE ON oo_identity_control.oauth_transaction TO oo_identity_runtime;
GRANT SELECT, INSERT, UPDATE ON oo_identity_control.session TO oo_identity_runtime;
GRANT SELECT, INSERT, UPDATE ON oo_identity_control.bootstrap_token TO oo_identity_runtime;
GRANT SELECT, INSERT ON oo_identity_control.audit_event TO oo_identity_runtime;
GRANT USAGE ON SEQUENCE oo_identity_control.audit_event_id_seq TO oo_identity_runtime;
