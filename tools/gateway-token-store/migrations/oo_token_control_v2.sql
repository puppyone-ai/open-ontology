-- Forward-only lifecycle extension. Apply after v1; do not alter its manifest,
-- relation shape or bytes: the accepted runtime verifies them on every miss.
CREATE SCHEMA oo_token_lifecycle;
CREATE TABLE oo_token_lifecycle.migration (
    version integer PRIMARY KEY CHECK (version = 2),
    sql_sha256 text NOT NULL CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);
CREATE TABLE oo_token_lifecycle.policy (
    project_id text PRIMARY KEY,
    max_lifetime_ms bigint NOT NULL DEFAULT 7776000000 CHECK (max_lifetime_ms BETWEEN 1 AND 31536000000),
    default_ttl_ms bigint NOT NULL DEFAULT 7776000000 CHECK (default_ttl_ms BETWEEN 1 AND max_lifetime_ms),
    self_renew_enabled boolean NOT NULL DEFAULT false,
    self_renew_window_ms bigint NOT NULL DEFAULT 1209600000 CHECK (self_renew_window_ms BETWEEN 0 AND max_lifetime_ms),
    rotate_overlap_ms bigint NOT NULL DEFAULT 604800000 CHECK (rotate_overlap_ms BETWEEN 0 AND 2592000000),
    enroll_ttl_ms bigint NOT NULL DEFAULT 86400000 CHECK (enroll_ttl_ms BETWEEN 1 AND 604800000)
);
CREATE TABLE oo_token_lifecycle.token (
    token_sha256 text PRIMARY KEY REFERENCES oo_token_control.token_record(token_sha256),
    first_issued_at_ms bigint NOT NULL CHECK (first_issued_at_ms >= 0),
    rotated_from text REFERENCES oo_token_lifecycle.token(token_sha256),
    overlap_until_ms bigint CHECK (overlap_until_ms >= first_issued_at_ms),
    issued_by text NOT NULL,
    purpose text NOT NULL
);
INSERT INTO oo_token_lifecycle.token (token_sha256, first_issued_at_ms, issued_by, purpose)
SELECT token_sha256, issued_at_ms, 'v1-migration', '' FROM oo_token_control.token_record;
CREATE TABLE oo_token_lifecycle.enrollment (
    code_sha256 text PRIMARY KEY CHECK (code_sha256 ~ '^[0-9a-f]{64}$'),
    project_id text NOT NULL,
    grant_json text NOT NULL,
    created_at_ms bigint NOT NULL CHECK (created_at_ms >= 0),
    expires_at_ms bigint NOT NULL CHECK (expires_at_ms > created_at_ms),
    redeemed_at_ms bigint,
    redeemed_by text,
    CHECK ((redeemed_at_ms IS NULL AND redeemed_by IS NULL) OR
           (redeemed_at_ms IS NOT NULL AND redeemed_by IS NOT NULL AND
            redeemed_at_ms >= created_at_ms AND redeemed_at_ms < expires_at_ms))
);
CREATE TABLE oo_token_lifecycle.audit (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    at_ms bigint NOT NULL CHECK (at_ms >= 0),
    project_id text NOT NULL,
    actor text NOT NULL,
    operation text NOT NULL CHECK (operation IN ('install','issue','list','renew','self_renew','rotate','revoke','enroll','redeem','policy')),
    outcome text NOT NULL,
    token_sha256 text CHECK (token_sha256 ~ '^[0-9a-f]{64}$'),
    related_sha256 text CHECK (related_sha256 ~ '^[0-9a-f]{64}$'),
    old_deadline_ms bigint,
    new_deadline_ms bigint
);
REVOKE ALL ON SCHEMA oo_token_lifecycle FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA oo_token_lifecycle FROM PUBLIC;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA oo_token_lifecycle FROM PUBLIC;
GRANT USAGE ON SCHEMA oo_token_lifecycle TO oo_token_runtime;
GRANT SELECT ON oo_token_lifecycle.token TO oo_token_runtime;
