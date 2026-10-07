-- oo.release-coordinator/v1. The adapter hashes this file before execution.
-- Exact replay does not execute it again. A different hash is drift.
-- This script does not create objects conditionally and does not call a clock.

CREATE SCHEMA oo_release_control;

CREATE TABLE oo_release_control.install_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    CONSTRAINT install_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT install_manifest_one CHECK (singleton),
    CONSTRAINT install_manifest_sha CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_release_control.deployment (
    singleton boolean NOT NULL,
    project_id text NOT NULL,
    authority_id text NOT NULL,
    authority_class text NOT NULL,
    authority_statement text NOT NULL,
    authority_statement_sha256 text NOT NULL,
    product_mode_can_activate boolean NOT NULL,
    production_ready_certificate boolean NOT NULL,
    full_sp08_accepted boolean NOT NULL,
    public_deployment_claimed boolean NOT NULL,
    customer_ownership_claimed boolean NOT NULL,
    standby_configured boolean NOT NULL,
    owner_authenticated boolean NOT NULL,
    credential_verified boolean NOT NULL,
    integrity_verified boolean NOT NULL,
    customer_gate_verified boolean NOT NULL,
    runsc_verified boolean NOT NULL,
    CONSTRAINT deployment_pk PRIMARY KEY (singleton),
    CONSTRAINT deployment_one CHECK (singleton),
    CONSTRAINT deployment_project CHECK (project_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT deployment_authority CHECK (authority_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT deployment_class CHECK (authority_class = 'bounded_local_fixture'),
    CONSTRAINT deployment_statement_sha CHECK (authority_statement_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT deployment_flags CHECK (
        product_mode_can_activate = FALSE
        AND production_ready_certificate = FALSE
        AND full_sp08_accepted = FALSE
        AND public_deployment_claimed = FALSE
        AND customer_ownership_claimed = FALSE
        AND standby_configured = FALSE
        AND owner_authenticated = FALSE
        AND credential_verified = FALSE
        AND integrity_verified = FALSE
        AND customer_gate_verified = FALSE
        AND runsc_verified = FALSE
    )
);

CREATE TABLE oo_release_control.authority_gate (
    gate_id text NOT NULL,
    CONSTRAINT authority_gate_pk PRIMARY KEY (gate_id),
    CONSTRAINT authority_gate_id CHECK (gate_id IN (
        'G1', 'G2', 'G3', 'G4', 'G5a', 'G5b', 'G6', 'G7', 'G8', 'G9', 'G10', 'G11', 'G12', 'G13'
    ))
);

CREATE TABLE oo_release_control.candidate (
    candidate_id text NOT NULL,
    project_id text NOT NULL,
    version text NOT NULL,
    base_version text,
    state text NOT NULL,
    source_hash text NOT NULL,
    effective_oos text NOT NULL,
    visibility_json text NOT NULL,
    effective_sha256 text NOT NULL,
    visibility_sha256 text NOT NULL,
    pack_digest text NOT NULL,
    manifest_sha256 text NOT NULL,
    policy_id text NOT NULL,
    policy_version text NOT NULL,
    knowledge_id text NOT NULL,
    knowledge_version text NOT NULL,
    ontology_version text NOT NULL,
    catalog_version text NOT NULL,
    role_name text NOT NULL,
    compiler text NOT NULL,
    plan_hash text,
    plan_scope text,
    plan_epoch text,
    CONSTRAINT candidate_pk PRIMARY KEY (candidate_id),
    CONSTRAINT candidate_project CHECK (project_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT candidate_id_token CHECK (candidate_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT candidate_version_token CHECK (version ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT candidate_base_token CHECK (base_version IS NULL OR base_version ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT candidate_version_uq UNIQUE (version),
    CONSTRAINT candidate_state CHECK (state IN (
        'candidate_built', 'gates_passed', 'applying', 'active', 'inactive', 'failed', 'aborted', 'retired'
    )),
    CONSTRAINT candidate_hashes CHECK (
        source_hash ~ '^[0-9a-f]{64}$'
        AND effective_sha256 ~ '^[0-9a-f]{64}$'
        AND visibility_sha256 ~ '^[0-9a-f]{64}$'
        AND pack_digest ~ '^[0-9a-f]{64}$'
        AND manifest_sha256 ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT candidate_docs CHECK (
        octet_length(effective_oos) BETWEEN 2 AND 1048576
        AND octet_length(visibility_json) BETWEEN 2 AND 1048576
    ),
    CONSTRAINT candidate_plan CHECK (
        (plan_hash IS NULL AND plan_scope IS NULL AND plan_epoch IS NULL)
        OR (
            plan_hash ~ '^[0-9a-f]{64}$'
            AND plan_scope IN ('test_scope', 'production')
            AND plan_epoch ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'
        )
    )
);

CREATE TABLE oo_release_control.candidate_file (
    candidate_id text NOT NULL,
    path text NOT NULL,
    sha256 text NOT NULL,
    byte_len integer NOT NULL,
    CONSTRAINT candidate_file_pk PRIMARY KEY (candidate_id, path),
    CONSTRAINT candidate_file_fk FOREIGN KEY (candidate_id) REFERENCES oo_release_control.candidate (candidate_id),
    CONSTRAINT candidate_file_sha CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT candidate_file_len CHECK (byte_len > 0 AND byte_len <= 1048576),
    CONSTRAINT candidate_file_path CHECK (
        octet_length(path) BETWEEN 1 AND 256
        AND path !~ '^/'
        AND strpos(path, chr(92)) = 0
        AND path !~ '(^|/)\.\.(/|$)'
    )
);

CREATE TABLE oo_release_control.gate_report (
    candidate_id text NOT NULL,
    certificate_sha256 text NOT NULL,
    gate_id text NOT NULL,
    source_hash text NOT NULL,
    issuer_class text NOT NULL,
    status text NOT NULL,
    blocking boolean NOT NULL,
    scope text NOT NULL,
    product_mode_can_activate boolean NOT NULL,
    report_type text NOT NULL,
    authority_statement_sha256 text,
    disposition text NOT NULL,
    detail text NOT NULL,
    CONSTRAINT gate_report_pk PRIMARY KEY (candidate_id, certificate_sha256),
    CONSTRAINT gate_report_fk FOREIGN KEY (candidate_id) REFERENCES oo_release_control.candidate (candidate_id),
    CONSTRAINT gate_report_sha CHECK (
        certificate_sha256 ~ '^[0-9a-f]{64}$'
        AND source_hash ~ '^[0-9a-f]{64}$'
        AND (authority_statement_sha256 IS NULL OR authority_statement_sha256 ~ '^[0-9a-f]{64}$')
    ),
    CONSTRAINT gate_report_gate CHECK (gate_id IN (
        'G1', 'G2', 'G3', 'G4', 'G5a', 'G5b', 'G6', 'G7', 'G8', 'G9', 'G10', 'G11', 'G12', 'G13',
        'credential', 'integrity', 'authenticated_owner', 'customer', 'runsc'
    )),
    CONSTRAINT gate_report_issuer CHECK (issuer_class IN ('test_issuer', 'fixture_authority', 'component_authority')),
    CONSTRAINT gate_report_status CHECK (status IN ('pass', 'fail', 'warn', 'not_run')),
    CONSTRAINT gate_report_scope CHECK (scope = 'test_scope'),
    CONSTRAINT gate_report_product CHECK (product_mode_can_activate = FALSE),
    CONSTRAINT gate_report_disposition CHECK (disposition IN (
        'admissible', 'not_run', 'stale', 'foreign_receipt', 'failed', 'component_uncovered'
    ))
);

CREATE TABLE oo_release_control.checkpoint (
    candidate_id text NOT NULL,
    kind text NOT NULL,
    certificate_sha256 text NOT NULL,
    source_hash text NOT NULL,
    epoch text NOT NULL,
    barrier text NOT NULL,
    sessions_terminated boolean NOT NULL,
    issuer_class text NOT NULL,
    authority_statement_sha256 text,
    disposition text NOT NULL,
    CONSTRAINT checkpoint_pk PRIMARY KEY (candidate_id, certificate_sha256),
    CONSTRAINT checkpoint_fk FOREIGN KEY (candidate_id) REFERENCES oo_release_control.candidate (candidate_id),
    CONSTRAINT checkpoint_kind CHECK (kind IN ('fence', 'replica')),
    CONSTRAINT checkpoint_sha CHECK (
        certificate_sha256 ~ '^[0-9a-f]{64}$'
        AND source_hash ~ '^[0-9a-f]{64}$'
        AND (authority_statement_sha256 IS NULL OR authority_statement_sha256 ~ '^[0-9a-f]{64}$')
    ),
    CONSTRAINT checkpoint_epoch CHECK (epoch ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT checkpoint_barrier CHECK (barrier IN ('closed', 'replayed', 'stale', 'missing')),
    CONSTRAINT checkpoint_issuer CHECK (issuer_class IN ('test_issuer', 'fixture_authority')),
    CONSTRAINT checkpoint_disposition CHECK (disposition IN (
        'fresh', 'stale', 'foreign_receipt', 'fabricated_session', 'not_fresh'
    ))
);

CREATE TABLE oo_release_control.active_pointer (
    singleton boolean NOT NULL,
    project_id text NOT NULL,
    version text,
    candidate_id text,
    source_hash text,
    pack_digest text,
    manifest_sha256 text,
    CONSTRAINT active_pointer_pk PRIMARY KEY (singleton),
    CONSTRAINT active_pointer_one CHECK (singleton),
    CONSTRAINT active_pointer_project CHECK (project_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT active_pointer_pair CHECK (
        (
            version IS NULL
            AND candidate_id IS NULL
            AND source_hash IS NULL
            AND pack_digest IS NULL
            AND manifest_sha256 IS NULL
        )
        OR (
            version IS NOT NULL
            AND candidate_id IS NOT NULL
            AND source_hash ~ '^[0-9a-f]{64}$'
            AND pack_digest ~ '^[0-9a-f]{64}$'
            AND manifest_sha256 ~ '^[0-9a-f]{64}$'
        )
    )
);

CREATE TABLE oo_release_control.audit_event (
    audit_id text NOT NULL,
    project_id text NOT NULL,
    candidate_id text,
    event text NOT NULL,
    previous_version text,
    new_version text,
    source_hash text,
    detail text NOT NULL,
    CONSTRAINT audit_event_pk PRIMARY KEY (audit_id),
    CONSTRAINT audit_event_id CHECK (audit_id ~ '^[0-9a-f]{64}$'),
    CONSTRAINT audit_event_name CHECK (event IN (
        'staged', 'invalidated', 'gate_recorded', 'planned', 'applying',
        'replica_ready', 'activated', 'conflict_failed', 'recovering',
        'recovered', 'aborted', 'retired', 'rolled_back'
    ))
);

CREATE VIEW oo_release_control.publication_status AS
SELECT
    version,
    candidate_id,
    source_hash,
    pack_digest,
    manifest_sha256,
    FALSE AS production_ready_certificate,
    FALSE AS full_sp08_accepted,
    FALSE AS public_deployment_claimed,
    FALSE AS customer_ownership_claimed,
    FALSE AS standby_configured
FROM oo_release_control.active_pointer
WHERE singleton;

REVOKE ALL ON SCHEMA oo_release_control FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA oo_release_control FROM PUBLIC;
GRANT USAGE ON SCHEMA oo_release_control TO oo_release_reader;
GRANT SELECT ON oo_release_control.publication_status TO oo_release_reader;
