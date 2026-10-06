-- session-store-v1 schema oo_session_control.
-- The adapter hashes this file and stores the digest in install_manifest.
-- Exact replay does not execute this file again.
-- A different hash or a partial schema is drift. This script does not
-- create objects conditionally and it does not repair an existing schema.

CREATE SCHEMA oo_session_control;

CREATE TABLE oo_session_control.install_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    CONSTRAINT install_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT install_manifest_one CHECK (singleton),
    CONSTRAINT install_manifest_sha CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_session_control.session_head (
    project_id text NOT NULL,
    session_id text NOT NULL,
    revision numeric(20, 0) NOT NULL,
    owner_generation numeric(20, 0) NOT NULL,
    bound_project_id text,
    bound_principal_id text,
    bound_role text,
    bound_claims_hash text,
    bound_policy_version text,
    bound_knowledge_version text,
    bound_config_version text,
    state_json text NOT NULL,
    journal_len integer NOT NULL,
    CONSTRAINT session_head_pk PRIMARY KEY (project_id, session_id),
    CONSTRAINT session_head_revision_nonneg CHECK (revision >= 0),
    CONSTRAINT session_head_generation_nonneg CHECK (owner_generation >= 0),
    CONSTRAINT session_head_journal_bound CHECK (journal_len >= 0 AND journal_len <= 1024),
    CONSTRAINT session_head_bound_all_or_none CHECK (
        (
            bound_project_id IS NULL
            AND bound_principal_id IS NULL
            AND bound_role IS NULL
            AND bound_claims_hash IS NULL
            AND bound_policy_version IS NULL
            AND bound_knowledge_version IS NULL
            AND bound_config_version IS NULL
        )
        OR (
            bound_project_id IS NOT NULL
            AND bound_principal_id IS NOT NULL
            AND bound_role IS NOT NULL
            AND bound_claims_hash IS NOT NULL
            AND bound_policy_version IS NOT NULL
            AND bound_knowledge_version IS NOT NULL
            AND bound_config_version IS NOT NULL
        )
    ),
    CONSTRAINT session_head_bound_project_matches CHECK (
        bound_project_id IS NULL OR bound_project_id = project_id
    )
);

REVOKE ALL ON SCHEMA oo_session_control FROM PUBLIC;
REVOKE ALL ON TABLE oo_session_control.install_manifest FROM PUBLIC;
REVOKE ALL ON TABLE oo_session_control.session_head FROM PUBLIC;
GRANT USAGE ON SCHEMA oo_session_control TO oo_session_runtime;
GRANT SELECT, UPDATE ON TABLE oo_session_control.session_head TO oo_session_runtime;
