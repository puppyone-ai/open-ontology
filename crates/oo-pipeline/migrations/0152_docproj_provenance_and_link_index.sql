-- docs-projection/2: provenance and reverse index of the projection (D83). Agent DB, installed after 0147 (the
-- coordinator installs it with install-retained-target, or the engine runs the same text on first use).
-- Same text as crates/oo-docs/src/publish/projection_v2.sql (a test compares them). Idempotent.
-- Touches only `_oo_docproj`; nothing of the database pipeline.

-- Per entity digest: an incremental run rewrites exactly the entities whose digest changed.
ALTER TABLE _oo_docproj.doc_entity ADD COLUMN IF NOT EXISTS row_digest text NOT NULL DEFAULT '';

-- What triggered the last run (ontology version, Drive revision, link rules): the next run is skipped when it is equal.
ALTER TABLE _oo_docproj.projection_state ADD COLUMN IF NOT EXISTS trigger_key text NOT NULL DEFAULT '';

-- Provenance of a document entity, one row per piece of evidence: the file, its version and the anchor inside it
-- (page and box, sheet and cell, heading and line; the JSON of the document IR anchor).
CREATE TABLE IF NOT EXISTS _oo_docproj.doc_entity_evidence (
  drive_id     text NOT NULL,
  entity_key   text NOT NULL,
  ordinal      integer NOT NULL,
  path         text NOT NULL,
  file_version text NOT NULL,
  block_id     text NOT NULL,
  anchor       jsonb NOT NULL,
  PRIMARY KEY (drive_id, entity_key, ordinal)
);

-- Provenance of a link: the table row side. Which column matched, under which rule revision and ontology version.
ALTER TABLE _oo_docproj.entity_link ADD COLUMN IF NOT EXISTS matched_column text NOT NULL DEFAULT '';
ALTER TABLE _oo_docproj.entity_link ADD COLUMN IF NOT EXISTS rule_revision bigint NOT NULL DEFAULT 0;
ALTER TABLE _oo_docproj.entity_link ADD COLUMN IF NOT EXISTS ontology_version bigint NOT NULL DEFAULT 0;

-- The reverse lookup of the link routes: from a database row to the documents.
CREATE INDEX IF NOT EXISTS entity_link_db_row ON _oo_docproj.entity_link (db_table, db_key);

-- One provenance view over both kinds of source: `file` (an anchor in a document) and `table_row` (a row of a
-- database table that a document entity is linked to).
CREATE OR REPLACE VIEW _oo_docproj.entity_provenance AS
  SELECT drive_id, entity_key, 'file'::text AS source_kind, path AS source_ref,
         jsonb_build_object('path', path, 'file_version', file_version, 'block_id', block_id, 'anchor', anchor) AS detail
    FROM _oo_docproj.doc_entity_evidence
  UNION ALL
  SELECT drive_id, doc_entity_key, 'table_row', db_table || '/' || db_key,
         jsonb_build_object('table', db_table, 'key', db_key, 'matched_column', matched_column, 'matched_via', matched_via,
                            'confidence_bp', confidence_bp, 'status', status, 'rule_revision', rule_revision,
                            'ontology_version', ontology_version)
    FROM _oo_docproj.entity_link WHERE status <> 'rejected';
