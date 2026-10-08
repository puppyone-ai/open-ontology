-- oo.feedback.receiver/v1
-- Applied by an administrator. The receiver is not the owner.
-- Feedback rows and review candidates only. No grant table. No secret table.

CREATE SCHEMA IF NOT EXISTS oo_feedback_receiver;
REVOKE ALL ON SCHEMA oo_feedback_receiver FROM PUBLIC;

CREATE TABLE IF NOT EXISTS oo_feedback_receiver.receipt (
    receipt_id uuid PRIMARY KEY,
    project text NOT NULL,
    payload_hash bytea NOT NULL,
    catalog_sha256 bytea NOT NULL,
    relationships_sha256 bytea NOT NULL,
    profile_sha256 bytea NOT NULL,
    feedback_sha256 bytea NOT NULL,
    evidence_sha256 bytea NOT NULL,
    schema_hash text NOT NULL,
    effects jsonb NOT NULL,
    dedup jsonb NOT NULL,
    scope_drops jsonb NOT NULL,
    outcome jsonb NOT NULL,
    received_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT receipt_hash_len CHECK (octet_length(payload_hash) = 32),
    CONSTRAINT receipt_catalog_len CHECK (octet_length(catalog_sha256) = 32),
    CONSTRAINT receipt_relationships_len CHECK (octet_length(relationships_sha256) = 32),
    CONSTRAINT receipt_profile_len CHECK (octet_length(profile_sha256) = 32),
    CONSTRAINT receipt_feedback_len CHECK (octet_length(feedback_sha256) = 32),
    CONSTRAINT receipt_evidence_len CHECK (octet_length(evidence_sha256) = 32),
    CONSTRAINT receipt_project_shape CHECK (project ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT receipt_schema_hash CHECK (
        schema_hash = 'unsealed' OR schema_hash ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT receipt_not_published CHECK ((effects->>'published') = 'false'),
    CONSTRAINT receipt_not_granted CHECK ((effects->>'grants_issued') = 'false'),
    CONSTRAINT receipt_not_roles CHECK ((effects->>'roles_changed') = 'false'),
    CONSTRAINT receipt_not_curated CHECK ((effects->>'curated_written') = 'false'),
    CONSTRAINT receipt_not_automerged CHECK ((effects->>'auto_merge') = 'false'),
    CONSTRAINT receipt_not_certified CHECK ((effects->>'metrics_certified') = 'false'),
    CONSTRAINT receipt_outcome_bound CHECK (octet_length(outcome::text) <= 1048576)
);

CREATE TABLE IF NOT EXISTS oo_feedback_receiver.feedback_item (
    receipt_id uuid NOT NULL,
    ordinal integer NOT NULL,
    candidate_id text NOT NULL,
    kind text NOT NULL,
    target text NOT NULL,
    item_text text NOT NULL,
    untrusted boolean NOT NULL,
    PRIMARY KEY (receipt_id, ordinal),
    CONSTRAINT feedback_candidate_shape CHECK (candidate_id ~ '^cand_[0-9a-f]{32}$'),
    CONSTRAINT feedback_kind CHECK (kind IN ('gap', 'reject', 'failure', 'comment')),
    CONSTRAINT feedback_text_len CHECK (char_length(item_text) BETWEEN 1 AND 500),
    CONSTRAINT feedback_untrusted CHECK (untrusted = true),
    CONSTRAINT feedback_receipt_fk FOREIGN KEY (receipt_id)
        REFERENCES oo_feedback_receiver.receipt (receipt_id)
);

CREATE TABLE IF NOT EXISTS oo_feedback_receiver.candidate (
    receipt_id uuid NOT NULL,
    candidate_id text NOT NULL,
    channel text NOT NULL,
    queue_key text NOT NULL,
    score bigint NOT NULL,
    impact integer NOT NULL,
    uncertainty_millis integer NOT NULL,
    trusted_metric boolean NOT NULL,
    knowledge_verified boolean NOT NULL,
    prior_overlap boolean NOT NULL,
    source jsonb NOT NULL,
    PRIMARY KEY (receipt_id, candidate_id),
    CONSTRAINT candidate_id_shape CHECK (candidate_id ~ '^cand_[0-9a-f]{32}$'),
    CONSTRAINT candidate_channel CHECK (channel IN (
        'entity',
        'metric_hypothesis',
        'example',
        'untrusted_feedback',
        'stale_override',
        'negative_entity'
    )),
    CONSTRAINT candidate_not_trusted CHECK (trusted_metric = false),
    CONSTRAINT candidate_not_verified CHECK (knowledge_verified = false),
    CONSTRAINT candidate_score CHECK (score = impact::bigint * uncertainty_millis::bigint),
    CONSTRAINT candidate_source_producer CHECK (
        (source->>'producer') = 'oo.selfbuild-suggestions/v1'
    ),
    CONSTRAINT candidate_source_untrusted CHECK (
        (
            channel = 'untrusted_feedback'
            AND (source->>'untrusted') = 'true'
            AND (source->>'feedback_text') IS NOT NULL
        )
        OR (
            channel <> 'untrusted_feedback'
            AND (source->>'untrusted') = 'false'
            AND (source->>'feedback_text') IS NULL
        )
    ),
    CONSTRAINT candidate_source_bound CHECK (octet_length(source::text) <= 8192),
    CONSTRAINT candidate_receipt_fk FOREIGN KEY (receipt_id)
        REFERENCES oo_feedback_receiver.receipt (receipt_id)
);

CREATE INDEX IF NOT EXISTS receipt_project
    ON oo_feedback_receiver.receipt (project);

REVOKE ALL ON ALL TABLES IN SCHEMA oo_feedback_receiver FROM PUBLIC;

GRANT USAGE ON SCHEMA oo_feedback_receiver TO feedback_receiver;
GRANT INSERT, SELECT ON oo_feedback_receiver.receipt TO feedback_receiver;
GRANT INSERT, SELECT ON oo_feedback_receiver.feedback_item TO feedback_receiver;
GRANT INSERT, SELECT ON oo_feedback_receiver.candidate TO feedback_receiver;
