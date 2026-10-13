-- Maintenance facts for one control database.
-- This file is not oo_release_control_v1.sql. Installing it must not change that migration hash.
-- not_applicable is not a gate_report status. Absent preconditions stay here and cannot pass production.

CREATE TABLE oo_release_control.maintenance_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    CONSTRAINT maintenance_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT maintenance_manifest_one CHECK (singleton),
    CONSTRAINT maintenance_manifest_sha CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_release_control.maintenance_gate (
    candidate_id text NOT NULL,
    gate_id text NOT NULL,
    disposition text NOT NULL,
    detail text NOT NULL,
    evidence_sha256 text NOT NULL,
    CONSTRAINT maintenance_gate_pk PRIMARY KEY (candidate_id, gate_id),
    CONSTRAINT maintenance_gate_fk FOREIGN KEY (candidate_id) REFERENCES oo_release_control.candidate (candidate_id),
    CONSTRAINT maintenance_gate_id CHECK (gate_id IN ('G5a', 'G5b', 'G6', 'G10', 'G11', 'G12')),
    CONSTRAINT maintenance_gate_disposition CHECK (disposition IN ('not_applicable', 'blocked')),
    CONSTRAINT maintenance_gate_detail CHECK (octet_length(detail) BETWEEN 1 AND 200),
    CONSTRAINT maintenance_gate_sha CHECK (evidence_sha256 ~ '^[0-9a-f]{64}$')
);

-- Operator-selected target mapping is immutable for the sealed candidate.
CREATE TABLE oo_release_control.candidate_target_binding (
    candidate_id text PRIMARY KEY REFERENCES oo_release_control.candidate(candidate_id),
    source_hash text NOT NULL CHECK (source_hash ~ '^[0-9a-f]{64}$'),
    mapping_json text NOT NULL,
    mapping_sha256 text NOT NULL CHECK (mapping_sha256 ~ '^[0-9a-f]{64}$')
);
REVOKE ALL ON oo_release_control.candidate_target_binding FROM PUBLIC;
