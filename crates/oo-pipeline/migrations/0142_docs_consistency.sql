-- docs-consistency/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-llm/1, inside the
-- installer transaction (advisory lock 6413202611). No BEGIN or COMMIT here;
-- the installer inserts the schema_migrations row.
-- Merge bookkeeping, freshness token, dependency-chain hashes and integrity
-- constraints (10 §3.6, §16.1, §16.2, §16.7). Share-side tables (share_group,
-- share_update, share_section_use) belong to the S6 cards and are added to this
-- file by the coordinator.

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-llm/1') THEN
    RAISE EXCEPTION 'docs-llm/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

-- Merge and split (3.6): where an alias came from, and which entity a redirect
-- row pointed at before the chain was flattened.
ALTER TABLE _oo_docs.alias ADD COLUMN original_entity_id uuid;
UPDATE _oo_docs.alias SET original_entity_id = entity_id WHERE original_entity_id IS NULL;
ALTER TABLE _oo_docs.entity_redirect ADD COLUMN via_key text;
-- The redirect table is the one index table that may lose rows (a split) or be
-- rewritten (chain flattening); a split also writes the overlay entity_split.
GRANT UPDATE, DELETE ON _oo_docs.entity_redirect TO oo_docs_runtime;

-- Review kinds of docs-api/1.0 that the engine now writes (relaxing CHECK).
ALTER TABLE _oo_docs.review_item DROP CONSTRAINT review_item_kind_check;
ALTER TABLE _oo_docs.review_item ADD CONSTRAINT review_item_kind_check CHECK (kind IN (
  'low_confidence_fact','entity_merge','alias_conflict','orphaned_overlay','type_promotion',
  'suggestion','reprocess_diff','constraint_conflict','speaker_identity')) NOT VALID;
ALTER TABLE _oo_docs.review_item VALIDATE CONSTRAINT review_item_kind_check;

-- Freshness token (16.2) and dependency-chain hashes (16.1).
ALTER TABLE _oo_docs.drive ADD COLUMN rev bigint NOT NULL DEFAULT 0;
ALTER TABLE _oo_docs.file_version ADD COLUMN drive_rev bigint;
ALTER TABLE _oo_docs.file_version ADD COLUMN assertion_set_hash text
  CHECK (assertion_set_hash IS NULL OR assertion_set_hash ~ '^[0-9a-f]{64}$');
ALTER TABLE _oo_docs.parse_result ADD COLUMN block_hashes text[];
ALTER TABLE _oo_docs.overlay ADD COLUMN drive_rev bigint;
ALTER TABLE _oo_docs.ontology_version ADD COLUMN covers_drive_rev bigint NOT NULL DEFAULT 0;

CREATE TABLE _oo_docs.entity_state (
  entity_id uuid NOT NULL REFERENCES _oo_docs.entity(id),
  ontology_version bigint NOT NULL REFERENCES _oo_docs.ontology_version(n),
  hash text NOT NULL CHECK (hash ~ '^[0-9a-f]{64}$'),
  PRIMARY KEY (entity_id, ontology_version));

-- Integrity constraints (16.7): one row per violated constraint and entity.
CREATE TABLE _oo_docs.constraint_violation (
  id uuid PRIMARY KEY,
  constraint_id text NOT NULL,
  entity_key text NOT NULL,
  dedupe_key text NOT NULL UNIQUE,
  fact_ids uuid[] NOT NULL,
  details jsonb NOT NULL,
  status text NOT NULL DEFAULT 'open' CHECK (status IN ('open','resolved')),
  review_id uuid,
  detected_in bigint,
  detected_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  resolved_at timestamptz);
CREATE INDEX constraint_violation_open ON _oo_docs.constraint_violation (entity_key) WHERE status = 'open';

GRANT SELECT, INSERT, UPDATE ON _oo_docs.entity_state, _oo_docs.constraint_violation TO oo_docs_runtime;

-- Overlays are append-only (3.2): a row that another row names in
-- payload.supersedes_overlay is retired. The view keeps the current ones.
CREATE VIEW _oo_docs.overlay_current AS
  SELECT o.* FROM _oo_docs.overlay o
   WHERE NOT EXISTS (SELECT 1 FROM _oo_docs.overlay n
                      WHERE n.payload->>'supersedes_overlay' = o.id::text);
GRANT SELECT ON _oo_docs.overlay_current TO oo_docs_runtime;

-- Freshness token (16.2): every new file version and every file that is
-- superseded or deleted advances the Drive's rev in the same transaction and
-- writes it to the file version. Overlay corrections pass their rev on insert.
CREATE FUNCTION _oo_docs.bump_drive_rev(p_drive uuid) RETURNS bigint
LANGUAGE sql SECURITY DEFINER SET search_path = pg_catalog, _oo_docs
AS $$ UPDATE _oo_docs.drive SET rev = rev + 1 WHERE id = p_drive RETURNING rev $$;
REVOKE ALL ON FUNCTION _oo_docs.bump_drive_rev(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo_docs.bump_drive_rev(uuid) TO oo_docs_runtime;

CREATE FUNCTION _oo_docs.file_version_rev() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo_docs
AS $$
BEGIN
  NEW.drive_rev := _oo_docs.bump_drive_rev((SELECT drive_id FROM _oo_docs.file WHERE id = NEW.file_id));
  RETURN NEW;
END $$;
CREATE TRIGGER file_version_rev BEFORE INSERT ON _oo_docs.file_version
  FOR EACH ROW EXECUTE FUNCTION _oo_docs.file_version_rev();

CREATE FUNCTION _oo_docs.file_state_rev() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo_docs
AS $$
BEGIN
  UPDATE _oo_docs.file_version SET drive_rev = _oo_docs.bump_drive_rev(NEW.drive_id)
   WHERE id = NEW.current_version_id;
  RETURN NEW;
END $$;
CREATE TRIGGER file_state_rev AFTER UPDATE OF state ON _oo_docs.file
  FOR EACH ROW WHEN (NEW.state IN ('superseded', 'deleted') AND OLD.state IS DISTINCT FROM NEW.state
                     AND NEW.current_version_id IS NOT NULL)
  EXECUTE FUNCTION _oo_docs.file_state_rev();
GRANT UPDATE (drive_rev) ON _oo_docs.file_version TO oo_docs_runtime;

-- Optimistic concurrency of a person's corrections (16.8): the revision of a fact.
ALTER TABLE _oo_docs.fact ADD COLUMN revision bigint NOT NULL DEFAULT 1;
