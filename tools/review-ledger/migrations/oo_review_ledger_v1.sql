-- oo.review-ledger/v1
-- Own schema. Not jobs, audit, identity, or another control schema.
-- The adapter hashes this file before execution. A different hash is drift.
-- This script does not create roles, does not GRANT data privileges, and
-- does not install an audit-collector certificate.

CREATE SCHEMA oo_review_ledger;

REVOKE ALL ON SCHEMA oo_review_ledger FROM PUBLIC;
GRANT USAGE ON SCHEMA oo_review_ledger TO oo_review_ledger_runtime;
REVOKE ALL ON SCHEMA public FROM oo_review_ledger_runtime;

CREATE TABLE oo_review_ledger.install_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    project_id text NOT NULL,
    control_sha256 text NOT NULL,
    CONSTRAINT install_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT install_manifest_one CHECK (singleton),
    CONSTRAINT install_manifest_sql CHECK (sql_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT install_manifest_control CHECK (control_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT install_manifest_project CHECK (project_id ~ '^[a-z][a-z0-9_]{0,62}$')
);

CREATE TABLE oo_review_ledger.queue_head (
    project_id text NOT NULL,
    object_id text NOT NULL,
    object_kind text NOT NULL,
    revision bigint NOT NULL,
    candidate_sha256 text NOT NULL,
    input_sha256 text NOT NULL,
    evidence_sha256 text NOT NULL,
    schema_sha256 text NOT NULL,
    curated_sha256 text NOT NULL,
    source_sha256 text NOT NULL,
    candidate_protocol text NOT NULL,
    owner_label text NOT NULL,
    CONSTRAINT queue_head_pk PRIMARY KEY (project_id, object_id),
    CONSTRAINT queue_head_project CHECK (project_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT queue_head_object CHECK (object_id ~ '^[a-z][a-z0-9_]{0,62}(\.[a-z][a-z0-9_]{0,62}){1,2}$'),
    CONSTRAINT queue_head_kind CHECK (object_kind IN ('dataset', 'column')),
    CONSTRAINT queue_head_revision CHECK (revision >= 1),
    CONSTRAINT queue_head_candidate CHECK (candidate_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT queue_head_input CHECK (input_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT queue_head_evidence CHECK (evidence_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT queue_head_schema CHECK (schema_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT queue_head_curated CHECK (curated_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT queue_head_source CHECK (source_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT queue_head_protocol CHECK (
        candidate_protocol IN (
            'oo.builder.description-candidate/v1',
            'oo.builder.description-candidate/v2'
        )
    ),
    CONSTRAINT queue_head_owner CHECK (char_length(owner_label) BETWEEN 1 AND 256)
);

CREATE TABLE oo_review_ledger.queue_revision (
    project_id text NOT NULL,
    object_id text NOT NULL,
    object_kind text NOT NULL,
    revision bigint NOT NULL,
    candidate_sha256 text NOT NULL,
    input_sha256 text NOT NULL,
    evidence_sha256 text NOT NULL,
    schema_sha256 text NOT NULL,
    curated_sha256 text NOT NULL,
    source_sha256 text NOT NULL,
    candidate_protocol text NOT NULL,
    owner_label text NOT NULL,
    CONSTRAINT queue_revision_pk PRIMARY KEY (project_id, object_id, revision),
    CONSTRAINT queue_revision_project CHECK (project_id ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT queue_revision_object CHECK (object_id ~ '^[a-z][a-z0-9_]{0,62}(\.[a-z][a-z0-9_]{0,62}){1,2}$'),
    CONSTRAINT queue_revision_kind CHECK (object_kind IN ('dataset', 'column')),
    CONSTRAINT queue_revision_revision CHECK (revision >= 1),
    CONSTRAINT queue_revision_candidate CHECK (candidate_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT queue_revision_input CHECK (input_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT queue_revision_evidence CHECK (evidence_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT queue_revision_schema CHECK (schema_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT queue_revision_curated CHECK (curated_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT queue_revision_source CHECK (source_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT queue_revision_protocol CHECK (
        candidate_protocol IN (
            'oo.builder.description-candidate/v1',
            'oo.builder.description-candidate/v2'
        )
    ),
    CONSTRAINT queue_revision_owner CHECK (char_length(owner_label) BETWEEN 1 AND 256)
);

CREATE TABLE oo_review_ledger.decision (
    project_id text NOT NULL,
    idempotency_key text NOT NULL,
    object_id text NOT NULL,
    revision bigint NOT NULL,
    decision_kind text NOT NULL,
    choice text NOT NULL,
    reviewer_principal text NOT NULL,
    reviewer_label text NOT NULL,
    reviewer_at text NOT NULL,
    candidate_sha256 text NOT NULL,
    input_sha256 text NOT NULL,
    evidence_sha256 text NOT NULL,
    schema_sha256 text NOT NULL,
    curated_sha256 text NOT NULL,
    source_sha256 text NOT NULL,
    fields_json text NOT NULL,
    body_sha256 text NOT NULL,
    authenticated boolean NOT NULL,
    authority text NOT NULL,
    management_identity text NOT NULL,
    curated_applied boolean NOT NULL,
    published boolean NOT NULL,
    grants text NOT NULL,
    CONSTRAINT decision_pk PRIMARY KEY (project_id, idempotency_key),
    CONSTRAINT decision_key CHECK (idempotency_key ~ '^[A-Za-z0-9_.:-]{1,128}$'),
    CONSTRAINT decision_object CHECK (object_id ~ '^[a-z][a-z0-9_]{0,62}(\.[a-z][a-z0-9_]{0,62}){1,2}$'),
    CONSTRAINT decision_revision CHECK (revision >= 1),
    CONSTRAINT decision_kind CHECK (decision_kind IN ('label_description', 'classification', 'owner_certification')),
    CONSTRAINT decision_choice CHECK (
        (decision_kind = 'label_description' AND choice IN ('approve', 'reject', 'defer'))
        OR (decision_kind = 'classification' AND choice = 'confirm')
        OR (decision_kind = 'owner_certification' AND choice = 'record')
    ),
    CONSTRAINT decision_reviewer CHECK (char_length(reviewer_label) BETWEEN 1 AND 256),
    CONSTRAINT decision_date CHECK (reviewer_at ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'),
    CONSTRAINT decision_candidate CHECK (candidate_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT decision_input CHECK (input_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT decision_evidence CHECK (evidence_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT decision_schema CHECK (schema_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT decision_curated CHECK (curated_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT decision_source CHECK (source_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT decision_body CHECK (body_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT decision_auth CHECK (authenticated = false),
    CONSTRAINT decision_authority CHECK (authority = 'not_verified'),
    CONSTRAINT decision_identity CHECK (management_identity = 'not_claimed'),
    CONSTRAINT decision_not_applied CHECK (curated_applied = false),
    CONSTRAINT decision_not_published CHECK (published = false),
    CONSTRAINT decision_not_grant CHECK (grants = 'not_authorized')
);

CREATE TABLE oo_review_ledger.audit_event (
    event_id bigint GENERATED ALWAYS AS IDENTITY,
    project_id text NOT NULL,
    idempotency_key text NOT NULL,
    action text NOT NULL,
    detail text NOT NULL,
    object_id text NOT NULL,
    revision bigint NOT NULL,
    body_sha256 text NOT NULL,
    principal_id text NOT NULL,
    CONSTRAINT audit_event_pk PRIMARY KEY (event_id),
    CONSTRAINT audit_event_idempotency UNIQUE (project_id, idempotency_key, action, detail),
    CONSTRAINT audit_event_action CHECK (action IN ('enqueue', 'decide', 'receipt', 'batch')),
    CONSTRAINT audit_event_detail CHECK (
        detail IN ('', 'description-review', 'description-pack')
    ),
    CONSTRAINT audit_event_key CHECK (idempotency_key ~ '^[A-Za-z0-9_.:-]{1,200}$'),
    CONSTRAINT audit_event_body CHECK (body_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT audit_event_revision CHECK (revision >= 0)
);

CREATE TABLE oo_review_ledger.compiler_receipt (
    project_id text NOT NULL,
    idempotency_key text NOT NULL,
    compiler text NOT NULL,
    command_sha256 text NOT NULL,
    candidate_sha256 text NOT NULL,
    outcome_sha256 text NOT NULL,
    outcome_protocol text NOT NULL,
    git_applied boolean NOT NULL,
    published boolean NOT NULL,
    curated_files_written boolean NOT NULL,
    CONSTRAINT compiler_receipt_pk PRIMARY KEY (project_id, idempotency_key, compiler),
    CONSTRAINT compiler_receipt_compiler CHECK (
        compiler IN ('description-review', 'description-pack')
    ),
    CONSTRAINT compiler_receipt_command CHECK (command_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT compiler_receipt_candidate CHECK (candidate_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT compiler_receipt_outcome CHECK (outcome_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT compiler_receipt_git CHECK (git_applied = false),
    CONSTRAINT compiler_receipt_published CHECK (published = false),
    CONSTRAINT compiler_receipt_files CHECK (curated_files_written = false)
);

CREATE TABLE oo_review_ledger.batch (
    project_id text NOT NULL,
    batch_id text NOT NULL,
    principal_id text NOT NULL,
    status text NOT NULL,
    item_count integer NOT NULL,
    refused_count integer NOT NULL,
    applied boolean NOT NULL,
    CONSTRAINT batch_pk PRIMARY KEY (project_id, batch_id),
    CONSTRAINT batch_id CHECK (batch_id ~ '^[a-z][a-z0-9_.:-]{0,63}$'),
    CONSTRAINT batch_status CHECK (status IN ('partial', 'complete')),
    CONSTRAINT batch_counts CHECK (
        item_count >= 1
        AND item_count <= 8
        AND refused_count >= 0
        AND refused_count <= item_count
    ),
    CONSTRAINT batch_not_applied CHECK (applied = false),
    CONSTRAINT batch_partial_count CHECK (
        (status = 'complete' AND refused_count = 0)
        OR (status = 'partial' AND refused_count >= 1)
    )
);

CREATE TABLE oo_review_ledger.batch_item (
    project_id text NOT NULL,
    batch_id text NOT NULL,
    ordinal integer NOT NULL,
    object_id text NOT NULL,
    code text NOT NULL,
    decision_key text,
    CONSTRAINT batch_item_pk PRIMARY KEY (project_id, batch_id, ordinal),
    CONSTRAINT batch_item_ordinal CHECK (ordinal >= 0 AND ordinal < 8),
    CONSTRAINT batch_item_code CHECK (code ~ '^[a-z0-9_.]{1,64}$')
);

CREATE FUNCTION oo_review_ledger.reject_mutation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog
AS $$
BEGIN
    RAISE EXCEPTION 'review ledger row is immutable' USING ERRCODE = '23514';
END;
$$;

CREATE TRIGGER queue_revision_no_update
    BEFORE UPDATE OR DELETE ON oo_review_ledger.queue_revision
    FOR EACH ROW EXECUTE FUNCTION oo_review_ledger.reject_mutation();

CREATE TRIGGER decision_no_update
    BEFORE UPDATE OR DELETE ON oo_review_ledger.decision
    FOR EACH ROW EXECUTE FUNCTION oo_review_ledger.reject_mutation();

CREATE TRIGGER audit_event_no_update
    BEFORE UPDATE OR DELETE ON oo_review_ledger.audit_event
    FOR EACH ROW EXECUTE FUNCTION oo_review_ledger.reject_mutation();

CREATE TRIGGER compiler_receipt_no_update
    BEFORE UPDATE OR DELETE ON oo_review_ledger.compiler_receipt
    FOR EACH ROW EXECUTE FUNCTION oo_review_ledger.reject_mutation();

CREATE TRIGGER batch_no_update
    BEFORE UPDATE OR DELETE ON oo_review_ledger.batch
    FOR EACH ROW EXECUTE FUNCTION oo_review_ledger.reject_mutation();

CREATE TRIGGER batch_item_no_update
    BEFORE UPDATE OR DELETE ON oo_review_ledger.batch_item
    FOR EACH ROW EXECUTE FUNCTION oo_review_ledger.reject_mutation();

CREATE FUNCTION oo_review_ledger.head_forward_only()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog
AS $$
BEGIN
    IF NEW.project_id IS DISTINCT FROM OLD.project_id
        OR NEW.object_id IS DISTINCT FROM OLD.object_id
    THEN
        RAISE EXCEPTION 'queue head identity is immutable' USING ERRCODE = '23514';
    END IF;
    IF NEW.revision <= OLD.revision THEN
        RAISE EXCEPTION 'queue head revision must increase' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER queue_head_forward
    BEFORE UPDATE ON oo_review_ledger.queue_head
    FOR EACH ROW EXECUTE FUNCTION oo_review_ledger.head_forward_only();

CREATE TRIGGER queue_head_no_delete
    BEFORE DELETE ON oo_review_ledger.queue_head
    FOR EACH ROW EXECUTE FUNCTION oo_review_ledger.reject_mutation();

REVOKE ALL ON ALL TABLES IN SCHEMA oo_review_ledger FROM PUBLIC;
GRANT SELECT, INSERT ON oo_review_ledger.queue_revision TO oo_review_ledger_runtime;
GRANT SELECT, INSERT ON oo_review_ledger.queue_head TO oo_review_ledger_runtime;
GRANT UPDATE (
    revision,
    candidate_sha256,
    input_sha256,
    evidence_sha256,
    schema_sha256,
    curated_sha256,
    source_sha256,
    candidate_protocol,
    owner_label
) ON oo_review_ledger.queue_head TO oo_review_ledger_runtime;
-- UPDATE lets the runtime take SELECT FOR UPDATE. The decision trigger still rejects changes.
GRANT SELECT, INSERT, UPDATE ON oo_review_ledger.decision TO oo_review_ledger_runtime;
GRANT SELECT, INSERT ON oo_review_ledger.audit_event TO oo_review_ledger_runtime;
GRANT USAGE, SELECT ON SEQUENCE oo_review_ledger.audit_event_event_id_seq TO oo_review_ledger_runtime;
GRANT SELECT, INSERT ON oo_review_ledger.compiler_receipt TO oo_review_ledger_runtime;
GRANT SELECT, INSERT ON oo_review_ledger.batch TO oo_review_ledger_runtime;
GRANT SELECT, INSERT ON oo_review_ledger.batch_item TO oo_review_ledger_runtime;
GRANT SELECT ON oo_review_ledger.install_manifest TO oo_review_ledger_runtime;

COMMENT ON SCHEMA oo_review_ledger IS
    'Declared S9/S10 review queue. A decision is not a curated write, a grant, or a publication.';
