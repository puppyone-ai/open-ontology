-- docs-cloud-audit/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-parse-media/1, inside the installer
-- transaction (advisory lock 6413202611). No BEGIN or COMMIT here; the installer inserts the
-- schema_migrations row.
-- L7 Cloud proxy mode (10 11.3, B-S12-01): the second kind of token (`oodc_`, bound to a tenant and a set
-- of Drives) and the append-only audit table that answers "who shared what to which agent, and when".

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-parse-media/1') THEN
    RAISE EXCEPTION 'docs-parse-media/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

-- Only the hash of an `oodc_` token is kept. The token is valid on the cloud entry only (never on the
-- loopback control API) and only for the Drives listed.
CREATE TABLE _oo_docs.cloud_token (
  id uuid PRIMARY KEY,
  token_b3 text NOT NULL UNIQUE,
  tenant text NOT NULL CHECK (char_length(tenant) BETWEEN 1 AND 128),
  drive_ids uuid[] NOT NULL CHECK (cardinality(drive_ids) BETWEEN 1 AND 256),
  label text CHECK (char_length(label) <= 200),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  expires_at timestamptz NOT NULL,
  revoked_at timestamptz,
  last_used_at timestamptz,
  use_count bigint NOT NULL DEFAULT 0
);

-- Append only, hash chained: each row carries the hash of the one before it, so a removed or edited row
-- breaks the chain (`oo_docs::audit::verify_chain`). No foreign keys: the log outlives what it names.
CREATE TABLE _oo_docs.audit_log (
  seq bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  at timestamptz NOT NULL DEFAULT clock_timestamp(),
  action text NOT NULL CHECK (action IN ('share.created', 'share.revoked', 'refs.accessed',
                                         'cloud_token.issued', 'cloud_token.revoked')),
  actor text NOT NULL CHECK (char_length(actor) BETWEEN 1 AND 200),   -- local | cloud:<token id> | cred:<id>
  tenant text,
  drive_id uuid,
  share_id uuid,
  agent text CHECK (char_length(agent) <= 200),                        -- who the share went to
  detail jsonb NOT NULL DEFAULT '{}',
  prev_hash text NOT NULL,
  row_hash text NOT NULL
);
CREATE INDEX audit_log_share ON _oo_docs.audit_log (share_id, seq) WHERE share_id IS NOT NULL;
CREATE INDEX audit_log_at ON _oo_docs.audit_log (at);
CREATE TRIGGER audit_log_insert_only BEFORE UPDATE OR DELETE ON _oo_docs.audit_log
  FOR EACH ROW EXECUTE FUNCTION _oo_docs.reject_mutation();
CREATE TRIGGER audit_log_no_truncate BEFORE TRUNCATE ON _oo_docs.audit_log
  FOR EACH STATEMENT EXECUTE FUNCTION _oo_docs.reject_mutation();

GRANT SELECT, INSERT, UPDATE ON _oo_docs.cloud_token TO oo_docs_runtime;
GRANT SELECT, INSERT ON _oo_docs.audit_log TO oo_docs_runtime;

