-- oo.audit.ingest/v1
-- Applied by an administrator. The receiver is not the owner.
-- Event and receipt only. No config table. No secret table.

CREATE SCHEMA IF NOT EXISTS oo_audit_ingest;
REVOKE ALL ON SCHEMA oo_audit_ingest FROM PUBLIC;

CREATE TABLE IF NOT EXISTS oo_audit_ingest.event (
    spool_id uuid NOT NULL,
    request_id uuid NOT NULL,
    event_seq bigint NOT NULL,
    payload_hash bytea NOT NULL,
    payload bytea NOT NULL,
    project text NOT NULL,
    kind text NOT NULL,
    received_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (spool_id, request_id, event_seq),
    CONSTRAINT event_seq_bounds CHECK (event_seq >= 1 AND event_seq <= 1000000),
    CONSTRAINT event_hash_len CHECK (octet_length(payload_hash) = 32),
    CONSTRAINT event_payload_len CHECK (octet_length(payload) <= 16384),
    CONSTRAINT event_project_shape CHECK (project ~ '^[a-z][a-z0-9_-]{0,62}$'),
    CONSTRAINT event_kind CHECK (kind IN ('admission', 'completion', 'refusal'))
);

CREATE TABLE IF NOT EXISTS oo_audit_ingest.receipt (
    spool_id uuid NOT NULL,
    request_id uuid NOT NULL,
    event_seq bigint NOT NULL,
    payload_hash bytea NOT NULL,
    receipt_id uuid NOT NULL,
    PRIMARY KEY (spool_id, request_id, event_seq),
    CONSTRAINT receipt_hash_len CHECK (octet_length(payload_hash) = 32),
    CONSTRAINT receipt_event_fk FOREIGN KEY (spool_id, request_id, event_seq)
        REFERENCES oo_audit_ingest.event (spool_id, request_id, event_seq)
);

REVOKE ALL ON ALL TABLES IN SCHEMA oo_audit_ingest FROM PUBLIC;

GRANT USAGE ON SCHEMA oo_audit_ingest TO audit_ingest;
GRANT INSERT, SELECT ON oo_audit_ingest.event TO audit_ingest;
GRANT INSERT, SELECT ON oo_audit_ingest.receipt TO audit_ingest;
