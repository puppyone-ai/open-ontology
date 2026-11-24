-- docs-agent-tokens/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-drive-sync-prefix/1, inside the installer
-- transaction (advisory lock 6413202611). No BEGIN or COMMIT here; the installer inserts the
-- schema_migrations row.
-- D83 Agent tokens (`ooa_`): one token, scoped, with a TTL, revocable and audited. It lives in the table of the
-- `oodc_` cloud tokens (same hash, same issue and revoke code); `kind` tells them apart, the cloud entry only
-- ever accepts `kind = 'cloud'`. A token holds scopes (`docs:read`, `docs:suggest`, `docs:write`, `db:read`),
-- the Drives it may read, and the NAME of a Gateway profile (never a credential) that the CLI proxy uses to
-- read the database ontology for the Agent. Every change only widens a check (10 3.1 rule 5).

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-drive-sync-prefix/1') THEN
    RAISE EXCEPTION 'docs-drive-sync-prefix/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

ALTER TABLE _oo_docs.cloud_token
  ADD COLUMN kind text NOT NULL DEFAULT 'cloud' CHECK (kind IN ('cloud', 'agent')),
  ADD COLUMN scopes text[] NOT NULL DEFAULT '{}',
  ADD COLUMN all_drives boolean NOT NULL DEFAULT false,
  ADD COLUMN gw_profile text CHECK (char_length(gw_profile) BETWEEN 1 AND 128),
  ADD COLUMN agent_name text CHECK (char_length(agent_name) BETWEEN 1 AND 128);

-- A database-only Agent token names no Drive: the 1..256 check becomes "0..256, and at least one unless an Agent token".
ALTER TABLE _oo_docs.cloud_token DROP CONSTRAINT cloud_token_drive_ids_check;
ALTER TABLE _oo_docs.cloud_token ADD CONSTRAINT cloud_token_drive_ids_check
  CHECK (cardinality(drive_ids) <= 256 AND (kind = 'agent' OR cardinality(drive_ids) >= 1));
ALTER TABLE _oo_docs.cloud_token ADD CONSTRAINT cloud_token_scopes_check
  CHECK (kind = 'cloud' OR (cardinality(scopes) BETWEEN 1 AND 8
                            AND scopes <@ ARRAY['docs:read', 'docs:suggest', 'docs:write', 'db:read']::text[]));

-- One live Agent token per name: `oo agent token ls` and the audit trail name the Agent.
CREATE UNIQUE INDEX cloud_token_agent_name ON _oo_docs.cloud_token (agent_name)
  WHERE kind = 'agent' AND revoked_at IS NULL;

-- The audit log learns the Agent token events. `agent.read` is one row per call of an `ooa_` token
-- (route template and Drive only, never a path value or text), `agent.denied` one per refused call.
ALTER TABLE _oo_docs.audit_log DROP CONSTRAINT audit_log_action_check;
ALTER TABLE _oo_docs.audit_log ADD CONSTRAINT audit_log_action_check
  CHECK (action IN ('share.created', 'share.revoked', 'refs.accessed', 'cloud_token.issued', 'cloud_token.revoked',
                    'agent_token.issued', 'agent_token.revoked', 'agent.read', 'agent.denied'));
CREATE INDEX audit_log_actor ON _oo_docs.audit_log (actor, seq);
