-- docs-projection/1: read-only projection of published document entities into the Agent DB (D12, 10 11.3, B-S11-04).
-- Same text as crates/oo-docs/src/publish/projection.sql (a test compares them). Idempotent. Installed into the
-- Agent DB by the coordinator (after install_logical_identity in install-retained-target) or created on first use
-- by oo_docs::publish::projection::project. Nothing here touches the Agent DB tables of the database pipeline.

-- Read-only projection of published document entities into the Agent DB, and links to database
-- entities (D12, 10 11.3, B-S11-04). Written only by `oo_docs::publish::projection`; everything in
-- it can be dropped and rebuilt from docs-db. Idempotent (IF NOT EXISTS), safe to run twice.
CREATE SCHEMA IF NOT EXISTS _oo_docproj;
REVOKE ALL ON SCHEMA _oo_docproj FROM PUBLIC;

-- What was projected last: the digest of the visible graph, so an unchanged ontology writes nothing.
CREATE TABLE IF NOT EXISTS _oo_docproj.projection_state (
  drive_id         text PRIMARY KEY,
  ontology_version bigint NOT NULL,
  source_digest    text NOT NULL,
  entities         integer NOT NULL,
  projected_at     timestamptz NOT NULL DEFAULT clock_timestamp()
);

-- One row per published document entity the Drive policy lets the system see.
CREATE TABLE IF NOT EXISTS _oo_docproj.doc_entity (
  drive_id       text NOT NULL,
  entity_key     text NOT NULL,
  type           text NOT NULL,
  canonical_name text NOT NULL,
  norm_name      text NOT NULL,
  aliases        jsonb NOT NULL DEFAULT '[]',
  fact_count     integer NOT NULL DEFAULT 0,
  PRIMARY KEY (drive_id, entity_key)
);

-- Files the entity is evidenced in (Drive paths).
CREATE TABLE IF NOT EXISTS _oo_docproj.doc_entity_source (
  drive_id   text NOT NULL,
  entity_key text NOT NULL,
  path       text NOT NULL,
  PRIMARY KEY (drive_id, entity_key, path)
);

-- Link between a document entity and one row of a database table. `auto` links are recomputed on
-- every rebuild; a person's `confirmed` and `rejected` decisions are never touched.
CREATE TABLE IF NOT EXISTS _oo_docproj.entity_link (
  drive_id       text NOT NULL,
  doc_entity_key text NOT NULL,
  db_table       text NOT NULL,
  db_key         text NOT NULL,
  matched_via    text NOT NULL CHECK (matched_via IN ('name', 'alias')),
  matched_text   text NOT NULL,
  confidence_bp  integer NOT NULL CHECK (confidence_bp BETWEEN 0 AND 10000),
  status         text NOT NULL DEFAULT 'auto' CHECK (status IN ('auto', 'confirmed', 'rejected')),
  created_at     timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (drive_id, doc_entity_key, db_table, db_key)
);

-- Which files talk about which database row: the contract PDF to the customer row.
CREATE OR REPLACE VIEW _oo_docproj.document_link AS
  SELECT l.drive_id, l.db_table, l.db_key, l.doc_entity_key, e.type AS doc_entity_type,
         e.canonical_name, s.path, l.matched_via, l.confidence_bp, l.status
    FROM _oo_docproj.entity_link l
    JOIN _oo_docproj.doc_entity e ON e.drive_id = l.drive_id AND e.entity_key = l.doc_entity_key
    JOIN _oo_docproj.doc_entity_source s ON s.drive_id = l.drive_id AND s.entity_key = l.doc_entity_key
   WHERE l.status <> 'rejected';
