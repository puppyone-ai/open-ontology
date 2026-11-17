-- docs-packs-shares/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-index/1, inside the
-- installer transaction (advisory lock 6413202611). No BEGIN or COMMIT here;
-- the installer inserts the schema_migrations row.
-- Context packs, shares, refs credentials (10 §3.2).

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-index/1') THEN
    RAISE EXCEPTION 'docs-index/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

CREATE TABLE _oo_docs.context_pack (
  id uuid PRIMARY KEY,
  version_n bigint NOT NULL REFERENCES _oo_docs.ontology_version(n),
  drive_id uuid NOT NULL REFERENCES _oo_docs.drive(id),
  scope_sha256 text NOT NULL, params_sha256 text NOT NULL, compiler text NOT NULL,
  pack_digest text NOT NULL CHECK (pack_digest ~ '^[0-9a-f]{64}$'),
  snapshot_sha256 text NOT NULL,          -- 范围内文件版本集合的哈希
  manifest jsonb NOT NULL, tokens_brief integer NOT NULL, trimmed jsonb NOT NULL,
  blocked_facts integer NOT NULL, blocked_entities integer NOT NULL,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  UNIQUE (version_n, drive_id, scope_sha256, params_sha256, compiler));
CREATE TABLE _oo_docs.context_pack_file (
  pack_id uuid NOT NULL REFERENCES _oo_docs.context_pack(id),
  path text NOT NULL, sha256 text NOT NULL, bytes integer NOT NULL, tokens integer NOT NULL,
  cas_b3 text NOT NULL, PRIMARY KEY (pack_id, path));
CREATE TABLE _oo_docs.share (
  id uuid PRIMARY KEY,
  pack_id uuid NOT NULL REFERENCES _oo_docs.context_pack(id),
  drive_id uuid NOT NULL REFERENCES _oo_docs.drive(id),
  scope jsonb NOT NULL, role text NOT NULL CHECK (role IN ('viewer','suggester','editor')),
  task text CHECK (char_length(task) <= 2000), budget integer NOT NULL CHECK (budget BETWEEN 200 AND 20000),
  status text NOT NULL CHECK (status IN ('active','drifted','refreshed','expired','revoked')),
  grantor text NOT NULL, mount_ref text NOT NULL,   -- 守护进程 mounts.json 中的条目 id；不存宿主路径
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(), expires_at timestamptz NOT NULL,
  drifted_at timestamptz, revoked_at timestamptz, refreshed_from uuid REFERENCES _oo_docs.share(id),
  revision bigint NOT NULL DEFAULT 1);
CREATE INDEX share_live ON _oo_docs.share (status, expires_at) WHERE status IN ('active','drifted','refreshed');
CREATE TABLE _oo_docs.share_fact_use (
  share_id uuid NOT NULL REFERENCES _oo_docs.share(id), fact_id uuid NOT NULL REFERENCES _oo_docs.fact(id),
  section text NOT NULL, precision text NOT NULL CHECK (precision IN ('full','summary','title')),
  PRIMARY KEY (share_id, fact_id));
CREATE INDEX share_fact_use_fact ON _oo_docs.share_fact_use (fact_id);
CREATE TABLE _oo_docs.refs_credential (
  id uuid PRIMARY KEY, share_id uuid NOT NULL REFERENCES _oo_docs.share(id),
  token_b3 text NOT NULL UNIQUE,          -- 只存哈希
  scopes text[] NOT NULL CHECK (scopes <@ ARRAY['refs:read','suggest:write']),
  expires_at timestamptz NOT NULL, revoked_at timestamptz,
  last_used_at timestamptz, use_count bigint NOT NULL DEFAULT 0);

-- Packs are immutable once compiled; shares and credentials are mutable.
CREATE TRIGGER context_pack_insert_only BEFORE UPDATE OR DELETE ON _oo_docs.context_pack
  FOR EACH ROW EXECUTE FUNCTION _oo_docs.reject_mutation();
CREATE TRIGGER context_pack_file_insert_only BEFORE UPDATE OR DELETE ON _oo_docs.context_pack_file
  FOR EACH ROW EXECUTE FUNCTION _oo_docs.reject_mutation();
CREATE TRIGGER context_pack_no_truncate BEFORE TRUNCATE ON _oo_docs.context_pack
  FOR EACH STATEMENT EXECUTE FUNCTION _oo_docs.reject_mutation();
CREATE TRIGGER context_pack_file_no_truncate BEFORE TRUNCATE ON _oo_docs.context_pack_file
  FOR EACH STATEMENT EXECUTE FUNCTION _oo_docs.reject_mutation();
GRANT SELECT, INSERT ON _oo_docs.context_pack, _oo_docs.context_pack_file,
  _oo_docs.share_fact_use TO oo_docs_runtime;
GRANT SELECT, INSERT, UPDATE ON _oo_docs.share, _oo_docs.refs_credential TO oo_docs_runtime;
