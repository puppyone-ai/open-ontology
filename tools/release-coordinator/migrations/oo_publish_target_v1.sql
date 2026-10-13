-- Fence bookkeeping for the target database. The business table is not created here.
-- Row security and GRANT/REVOKE run against the existing relation named at install.

CREATE SCHEMA oo_publish_target;

REVOKE ALL ON SCHEMA oo_publish_target FROM PUBLIC;

CREATE TABLE oo_publish_target.install_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    CONSTRAINT target_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT target_manifest_one CHECK (singleton),
    CONSTRAINT target_manifest_sha CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_publish_target.fence (
    singleton boolean NOT NULL,
    state text NOT NULL,
    role_name text NOT NULL,
    schema_name text NOT NULL,
    relation_name text NOT NULL,
    opened_columns text[] NOT NULL,
    opened_pack_digest text,
    opened_version text,
    candidate_id text,
    epoch text,
    streams_stopped integer NOT NULL,
    CONSTRAINT fence_pk PRIMARY KEY (role_name, schema_name, relation_name),
    CONSTRAINT fence_one CHECK (singleton),
    CONSTRAINT fence_state CHECK (state IN ('closed', 'granted', 'open')),
    CONSTRAINT fence_streams CHECK (streams_stopped >= 0),
    CONSTRAINT fence_open_pair CHECK (
        (state <> 'open' AND opened_pack_digest IS NULL AND opened_version IS NULL)
        OR (
            state = 'open'
            AND opened_pack_digest ~ '^[0-9a-f]{64}$'
            AND opened_version IS NOT NULL
        )
    )
);

CREATE TABLE oo_publish_target.revoked_column (
    role_name text NOT NULL,
    schema_name text NOT NULL,
    relation_name text NOT NULL,
    column_name text NOT NULL,
    revoked_at_version text NOT NULL,
    CONSTRAINT revoked_pk PRIMARY KEY (role_name, schema_name, relation_name, column_name),
    CONSTRAINT revoked_version CHECK (octet_length(revoked_at_version) BETWEEN 1 AND 64)
);

REVOKE ALL ON ALL TABLES IN SCHEMA oo_publish_target FROM PUBLIC;
