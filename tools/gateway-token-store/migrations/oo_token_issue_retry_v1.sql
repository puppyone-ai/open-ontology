-- Optional issuance extension: the caller persists random material before dispatch.
-- Only hashes are stored; a replay must supply the same material and grant.
CREATE TABLE oo_token_lifecycle.issue_retry_manifest (
    singleton boolean PRIMARY KEY CHECK (singleton),
    sql_sha256 text NOT NULL CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);
CREATE TABLE oo_token_lifecycle.issue_retry (
    project_id text NOT NULL,
    key_sha256 text NOT NULL CHECK (key_sha256 ~ '^[0-9a-f]{64}$'),
    request_sha256 text NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    token_sha256 text NOT NULL UNIQUE REFERENCES oo_token_control.token_record(token_sha256),
    PRIMARY KEY (project_id, key_sha256)
);
REVOKE ALL ON oo_token_lifecycle.issue_retry_manifest,
    oo_token_lifecycle.issue_retry FROM PUBLIC, oo_token_runtime;
