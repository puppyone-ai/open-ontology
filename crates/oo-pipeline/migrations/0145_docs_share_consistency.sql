-- docs-share-consistency/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-consistency/1, inside the
-- installer transaction (advisory lock 6413202611). No BEGIN or COMMIT here;
-- the installer inserts the schema_migrations row.
-- Share side of continuous consistency (10 §16.3 .. §16.6, §16.9): task groups,
-- the append-only update stream behind UPDATES.md, per-section use records and
-- the version pin counts.

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-consistency/1') THEN
    RAISE EXCEPTION 'docs-consistency/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

CREATE TABLE _oo_docs.share_group (
  id uuid PRIMARY KEY,
  ticket_id text NOT NULL UNIQUE CHECK (ticket_id ~ '^[A-Za-z0-9._#:-]{1,128}$'),
  drive_id uuid NOT NULL REFERENCES _oo_docs.drive(id),
  ontology_version bigint NOT NULL REFERENCES _oo_docs.ontology_version(n),
  refresh_policy text NOT NULL DEFAULT 'manual' CHECK (refresh_policy IN ('manual','at-checkpoint','pinned')),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  revision bigint NOT NULL DEFAULT 1);

ALTER TABLE _oo_docs.share
  ADD COLUMN group_id uuid REFERENCES _oo_docs.share_group(id),
  ADD COLUMN refresh_policy text NOT NULL DEFAULT 'manual'
    CHECK (refresh_policy IN ('manual','at-checkpoint','pinned')),
  ADD COLUMN coverage jsonb,
  ADD COLUMN compile_params jsonb,
  ADD COLUMN revoke_reason text,
  ADD COLUMN update_seq bigint NOT NULL DEFAULT 0;
CREATE INDEX share_group_members ON _oo_docs.share (group_id) WHERE group_id IS NOT NULL;

-- UPDATES.md entries (16.4): seq per share, monotonic, no gaps, never changed.
CREATE TABLE _oo_docs.share_update (
  share_id uuid NOT NULL REFERENCES _oo_docs.share(id),
  seq bigint NOT NULL CHECK (seq >= 1),
  from_version bigint NOT NULL, to_version bigint NOT NULL,
  level text NOT NULL CHECK (level IN ('info','warn','block')),
  section text,
  fact_keys text[] NOT NULL DEFAULT '{}',
  summary_key text NOT NULL,
  summary_args jsonb NOT NULL DEFAULT '{}'::jsonb,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (share_id, seq));
CREATE TRIGGER share_update_insert_only BEFORE UPDATE OR DELETE ON _oo_docs.share_update
  FOR EACH ROW EXECUTE FUNCTION _oo_docs.reject_mutation();
CREATE TRIGGER share_update_no_truncate BEFORE TRUNCATE ON _oo_docs.share_update
  FOR EACH STATEMENT EXECUTE FUNCTION _oo_docs.reject_mutation();

-- Which facts each section of a pack used, with the section's digest (16.1, 16.3).
CREATE TABLE _oo_docs.share_section_use (
  share_id uuid NOT NULL REFERENCES _oo_docs.share(id),
  section text NOT NULL, fact_key text NOT NULL, section_digest text NOT NULL CHECK (section_digest ~ '^[0-9a-f]{64}$'),
  PRIMARY KEY (share_id, section, fact_key));
CREATE INDEX share_section_use_fact ON _oo_docs.share_section_use (fact_key);
CREATE TRIGGER share_section_use_insert_only BEFORE UPDATE OR DELETE ON _oo_docs.share_section_use
  FOR EACH ROW EXECUTE FUNCTION _oo_docs.reject_mutation();

-- ontology_version is insert-only, so the pin count is derived, not stored (16.9, D58).
CREATE VIEW _oo_docs.ontology_version_pins AS
  SELECT v.n, count(s.id) AS pinned_by
    FROM _oo_docs.ontology_version v
    LEFT JOIN _oo_docs.context_pack p ON p.version_n = v.n
    LEFT JOIN _oo_docs.share s ON s.pack_id = p.id AND s.status IN ('active','drifted')
         AND s.expires_at > clock_timestamp()
   GROUP BY v.n;

GRANT SELECT, INSERT, UPDATE ON _oo_docs.share_group TO oo_docs_runtime;
GRANT SELECT, INSERT ON _oo_docs.share_update, _oo_docs.share_section_use TO oo_docs_runtime;
GRANT SELECT ON _oo_docs.ontology_version_pins TO oo_docs_runtime;
