-- session-control-api-v1 schema oo_session_api.
-- The adapter hashes this file. Exact replay does not execute it again.
-- A different hash is drift. This script does not repair an existing schema.

CREATE SCHEMA oo_session_api;

CREATE TABLE oo_session_api.install_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    CONSTRAINT install_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT install_manifest_one CHECK (singleton),
    CONSTRAINT install_manifest_sha CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_session_api.entry_grant (
    grant_sha256 text NOT NULL,
    principal_id text NOT NULL,
    project_id text NOT NULL,
    consumed boolean NOT NULL,
    CONSTRAINT entry_grant_pk PRIMARY KEY (grant_sha256),
    CONSTRAINT entry_grant_sha CHECK (grant_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_session_api.http_session (
    session_sha256 text NOT NULL,
    csrf_sha256 text NOT NULL,
    principal_id text NOT NULL,
    project_id text NOT NULL,
    role_name text NOT NULL,
    claims_hash text NOT NULL,
    policy_version text NOT NULL,
    knowledge_version text NOT NULL,
    config_version text NOT NULL,
    revoked boolean NOT NULL,
    CONSTRAINT http_session_pk PRIMARY KEY (session_sha256),
    CONSTRAINT http_session_sha CHECK (session_sha256 ~ '^[0-9a-f]{64}$' AND csrf_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_session_api.conversation (
    project_id text NOT NULL,
    conversation_id text NOT NULL,
    principal_id text NOT NULL,
    session_id text NOT NULL,
    CONSTRAINT conversation_pk PRIMARY KEY (project_id, conversation_id),
    CONSTRAINT conversation_session UNIQUE (project_id, session_id)
);

CREATE TABLE oo_session_api.api_event (
    project_id text NOT NULL,
    conversation_id text NOT NULL,
    seq integer NOT NULL,
    event_id text NOT NULL,
    kind text NOT NULL,
    body text NOT NULL,
    CONSTRAINT api_event_pk PRIMARY KEY (project_id, conversation_id, seq),
    CONSTRAINT api_event_seq CHECK (seq >= 1 AND seq <= 256),
    CONSTRAINT api_event_body CHECK (char_length(body) <= 1024)
);

CREATE TABLE oo_session_api.resource (
    project_id text NOT NULL,
    session_id text NOT NULL,
    conversation_id text NOT NULL,
    principal_id text NOT NULL,
    role_name text NOT NULL,
    claims_hash text NOT NULL,
    policy_version text NOT NULL,
    knowledge_version text NOT NULL,
    config_version text NOT NULL,
    workspace_id text NOT NULL,
    event_prefix text NOT NULL,
    pool_slot text NOT NULL,
    permit_id text NOT NULL,
    mapping_id text NOT NULL,
    token_sha256 text NOT NULL,
    application_name text NOT NULL,
    backend_pid integer,
    clock_ms bigint NOT NULL,
    stage text NOT NULL,
    harness_pid bigint,
    claims_json text NOT NULL,
    CONSTRAINT resource_pk PRIMARY KEY (project_id, session_id),
    CONSTRAINT resource_stage CHECK (stage IN ('ready', 'running', 'fixture_done', 'closed')),
    CONSTRAINT resource_clock CHECK (clock_ms >= 0),
    CONSTRAINT resource_claims CHECK (char_length(claims_json) BETWEEN 2 AND 2048)
);

CREATE TABLE oo_session_api.permit (
    permit_id text NOT NULL,
    project_id text NOT NULL,
    principal_id text NOT NULL,
    session_id text NOT NULL,
    state text NOT NULL,
    CONSTRAINT permit_pk PRIMARY KEY (permit_id),
    CONSTRAINT permit_state CHECK (state IN ('held', 'released'))
);

CREATE TABLE oo_session_api.proxy_mapping (
    mapping_id text NOT NULL,
    project_id text NOT NULL,
    session_id text NOT NULL,
    sandbox_id text NOT NULL,
    credential_handle text NOT NULL,
    map_ip text NOT NULL,
    state text NOT NULL,
    CONSTRAINT proxy_mapping_pk PRIMARY KEY (mapping_id),
    CONSTRAINT proxy_mapping_state CHECK (state IN ('present', 'deleted'))
);

CREATE TABLE oo_session_api.pool_slot (
    slot_id text NOT NULL,
    state text NOT NULL,
    session_id text,
    replacement_for text,
    CONSTRAINT pool_slot_pk PRIMARY KEY (slot_id),
    CONSTRAINT pool_slot_state CHECK (state IN ('empty', 'used', 'destroyed'))
);

CREATE TABLE oo_session_api.output_object (
    project_id text NOT NULL,
    session_id text NOT NULL,
    name text NOT NULL,
    sha256 text NOT NULL,
    bytes bytea NOT NULL,
    bound_role text NOT NULL,
    bound_claims_hash text NOT NULL,
    bound_policy_version text NOT NULL,
    bound_knowledge_version text NOT NULL,
    bound_config_version text NOT NULL,
    CONSTRAINT output_object_pk PRIMARY KEY (project_id, session_id, name),
    CONSTRAINT output_object_name CHECK (name IN ('outputs', 'transcript')),
    CONSTRAINT output_object_sha CHECK (sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_session_api.release_audit (
    project_id text NOT NULL,
    session_id text NOT NULL,
    step_id text NOT NULL,
    evidence_sha256 text NOT NULL,
    CONSTRAINT release_audit_pk PRIMARY KEY (project_id, session_id, step_id),
    CONSTRAINT release_audit_sha CHECK (evidence_sha256 ~ '^[0-9a-f]{64}$')
);

REVOKE ALL ON SCHEMA oo_session_api FROM PUBLIC;
REVOKE ALL ON TABLE oo_session_api.install_manifest FROM PUBLIC;
REVOKE ALL ON TABLE oo_session_api.entry_grant FROM PUBLIC;
REVOKE ALL ON TABLE oo_session_api.http_session FROM PUBLIC;
REVOKE ALL ON TABLE oo_session_api.conversation FROM PUBLIC;
REVOKE ALL ON TABLE oo_session_api.api_event FROM PUBLIC;
REVOKE ALL ON TABLE oo_session_api.resource FROM PUBLIC;
REVOKE ALL ON TABLE oo_session_api.permit FROM PUBLIC;
REVOKE ALL ON TABLE oo_session_api.proxy_mapping FROM PUBLIC;
REVOKE ALL ON TABLE oo_session_api.pool_slot FROM PUBLIC;
REVOKE ALL ON TABLE oo_session_api.output_object FROM PUBLIC;
REVOKE ALL ON TABLE oo_session_api.release_audit FROM PUBLIC;

GRANT USAGE ON SCHEMA oo_session_api TO oo_session_api_runtime;
GRANT SELECT, INSERT, UPDATE ON TABLE oo_session_api.entry_grant TO oo_session_api_runtime;
GRANT SELECT, INSERT, UPDATE ON TABLE oo_session_api.http_session TO oo_session_api_runtime;
GRANT SELECT, INSERT, UPDATE ON TABLE oo_session_api.conversation TO oo_session_api_runtime;
GRANT SELECT, INSERT ON TABLE oo_session_api.api_event TO oo_session_api_runtime;
GRANT SELECT, INSERT, UPDATE ON TABLE oo_session_api.resource TO oo_session_api_runtime;
GRANT SELECT, INSERT, UPDATE ON TABLE oo_session_api.permit TO oo_session_api_runtime;
GRANT SELECT, INSERT, UPDATE ON TABLE oo_session_api.proxy_mapping TO oo_session_api_runtime;
GRANT SELECT, INSERT, UPDATE ON TABLE oo_session_api.pool_slot TO oo_session_api_runtime;
GRANT SELECT, INSERT ON TABLE oo_session_api.output_object TO oo_session_api_runtime;
GRANT SELECT, INSERT ON TABLE oo_session_api.release_audit TO oo_session_api_runtime;
