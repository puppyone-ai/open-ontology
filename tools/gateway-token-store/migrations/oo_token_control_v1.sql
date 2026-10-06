-- gateway-token-store-v1 schema oo_token_control.
-- The adapter hashes this file and stores the digest in install_manifest.
-- Exact replay does not execute this file again.
-- A different hash or a partial schema is drift. This script does not
-- create objects conditionally and it does not repair an existing schema.
-- The schema has no raw token column.

CREATE SCHEMA oo_token_control;

CREATE TABLE oo_token_control.install_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    CONSTRAINT install_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT install_manifest_one CHECK (singleton),
    CONSTRAINT install_manifest_sha CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE SEQUENCE oo_token_control.revocation_seq AS bigint;

CREATE TABLE oo_token_control.token_record (
    token_sha256 text NOT NULL,
    token_kind text NOT NULL,
    project_id text NOT NULL,
    principal_id text NOT NULL,
    role_name text NOT NULL,
    claims_canonical text NOT NULL,
    policy_version text NOT NULL,
    knowledge_version text NOT NULL,
    audience_url text NOT NULL,
    sandbox_id text,
    session_id text,
    issued_at_ms bigint NOT NULL,
    idle_deadline_ms bigint,
    absolute_deadline_ms bigint NOT NULL,
    last_used_ms bigint NOT NULL,
    revocation_floor bigint NOT NULL,
    CONSTRAINT token_record_pk PRIMARY KEY (token_sha256),
    CONSTRAINT token_record_deadlines CHECK (
        issued_at_ms >= 0
        AND absolute_deadline_ms > issued_at_ms
        AND last_used_ms >= issued_at_ms
        AND (
            idle_deadline_ms IS NULL
            OR (idle_deadline_ms > issued_at_ms AND idle_deadline_ms <= absolute_deadline_ms)
        )
    ),
    CONSTRAINT token_record_floor CHECK (revocation_floor >= 0),
    CONSTRAINT token_record_kind CHECK (token_kind IN ('session', 'client')),
    CONSTRAINT token_record_no_prefix CHECK (token_sha256 !~ '^(oost_|ooct_)'),
    CONSTRAINT token_record_session_binding CHECK (
        (
            token_kind = 'session'
            AND sandbox_id IS NOT NULL
            AND session_id IS NOT NULL
            AND idle_deadline_ms IS NOT NULL
        )
        OR (
            token_kind = 'client'
            AND sandbox_id IS NULL
            AND session_id IS NULL
            AND idle_deadline_ms IS NULL
        )
    ),
    CONSTRAINT token_record_sha CHECK (token_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_token_control.revocation (
    scope text NOT NULL,
    project_id text NOT NULL,
    principal_id text NOT NULL,
    session_id text NOT NULL,
    token_sha256 text NOT NULL,
    seq bigint NOT NULL,
    revoked_at_ms bigint NOT NULL,
    CONSTRAINT revocation_pk PRIMARY KEY (scope, project_id, principal_id, session_id, token_sha256),
    CONSTRAINT revocation_exact CHECK (
        (
            scope = 'token'
            AND token_sha256 ~ '^[0-9a-f]{64}$'
            AND principal_id = ''
            AND session_id = ''
        )
        OR (
            scope = 'session'
            AND session_id <> ''
            AND principal_id = ''
            AND token_sha256 = ''
        )
        OR (
            scope = 'principal'
            AND principal_id <> ''
            AND session_id = ''
            AND token_sha256 = ''
        )
        OR (
            scope = 'project'
            AND principal_id = ''
            AND session_id = ''
            AND token_sha256 = ''
        )
    ),
    CONSTRAINT revocation_seq_positive CHECK (seq > 0),
    CONSTRAINT revocation_seq_unique UNIQUE (seq),
    CONSTRAINT revocation_time CHECK (revoked_at_ms >= 0)
);

REVOKE ALL ON SCHEMA oo_token_control FROM PUBLIC;
REVOKE ALL ON TABLE oo_token_control.install_manifest FROM PUBLIC;
REVOKE ALL ON TABLE oo_token_control.token_record FROM PUBLIC;
REVOKE ALL ON TABLE oo_token_control.revocation FROM PUBLIC;
REVOKE ALL ON SEQUENCE oo_token_control.revocation_seq FROM PUBLIC;
GRANT USAGE ON SCHEMA oo_token_control TO oo_token_runtime;
GRANT SELECT ON TABLE oo_token_control.install_manifest TO oo_token_runtime;
GRANT SELECT ON TABLE oo_token_control.token_record TO oo_token_runtime;
GRANT SELECT ON TABLE oo_token_control.revocation TO oo_token_runtime;
GRANT UPDATE (last_used_ms, idle_deadline_ms) ON TABLE oo_token_control.token_record TO oo_token_runtime;
