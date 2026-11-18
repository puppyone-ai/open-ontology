-- docs-search-index/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-link-rules/1, inside the installer
-- transaction (advisory lock 6413202611). No BEGIN or COMMIT here; the installer inserts the
-- schema_migrations row.
-- D83 WP3b: GET /v1/docs/search answers from GIN indexes instead of scanning every fact and every block.
-- The text folding (NFKC, lowercase, one token per CJK character, punctuation to spaces) lives in one IMMUTABLE
-- function so the stored vectors and the query side agree byte for byte.

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-link-rules/1') THEN
    RAISE EXCEPTION 'docs-link-rules/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

CREATE FUNCTION _oo_docs.fts_fold(t text) RETURNS text
LANGUAGE sql IMMUTABLE PARALLEL SAFE STRICT
SET search_path = pg_catalog
AS $$
  SELECT regexp_replace(
           regexp_replace(lower(normalize(t, NFKC)),
             '([\u3400-\u4dbf\u4e00-\u9fff\u3040-\u30ff\uac00-\ud7af])', ' \1 ', 'g'),
           '[\x01-\x2f\x3a-\x40\x5b-\x60\x7b-\x7f\u2000-\u206f\u3000-\u3004\u3008-\u3020]+', ' ', 'g')
$$;

CREATE FUNCTION _oo_docs.fts_tsv(t text) RETURNS tsvector
LANGUAGE sql IMMUTABLE PARALLEL SAFE STRICT
SET search_path = pg_catalog
AS $$ SELECT to_tsvector('simple'::regconfig, _oo_docs.fts_fold(t)) $$;

REVOKE ALL ON FUNCTION _oo_docs.fts_fold(text), _oo_docs.fts_tsv(text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo_docs.fts_fold(text), _oo_docs.fts_tsv(text) TO oo_docs_runtime;

-- ---- facts: the search vector of a fact is kept on the row (subject name, predicate words, value, object name).
ALTER TABLE _oo_docs.fact ADD COLUMN fts tsvector;

CREATE FUNCTION _oo_docs.fact_fts_set() RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
  NEW.fts := _oo_docs.fts_tsv(concat_ws(' ',
    (SELECT s.canonical_name FROM _oo_docs.entity s WHERE s.id = NEW.subject_id),
    replace(replace(NEW.predicate, '.', ' '), '_', ' '),
    NEW.value->>'text', NEW.value->>'enum', NEW.value->>'date',
    (SELECT o.canonical_name FROM _oo_docs.entity o WHERE o.id = NEW.object_id)));
  RETURN NEW;
END
$$;

CREATE TRIGGER fact_fts_set
  BEFORE INSERT OR UPDATE OF subject_id, object_id, predicate, value ON _oo_docs.fact
  FOR EACH ROW EXECUTE FUNCTION _oo_docs.fact_fts_set();

-- A renamed entity refreshes the vectors of the facts that name it.
CREATE FUNCTION _oo_docs.entity_fts_refresh() RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
  UPDATE _oo_docs.fact SET subject_id = subject_id WHERE subject_id = NEW.id OR object_id = NEW.id;
  RETURN NULL;
END
$$;

CREATE TRIGGER entity_fts_refresh
  AFTER UPDATE OF canonical_name ON _oo_docs.entity
  FOR EACH ROW WHEN (OLD.canonical_name IS DISTINCT FROM NEW.canonical_name)
  EXECUTE FUNCTION _oo_docs.entity_fts_refresh();

-- Backfill (the BEFORE UPDATE trigger computes the vector).
UPDATE _oo_docs.fact SET subject_id = subject_id;

CREATE INDEX fact_fts_gin ON _oo_docs.fact USING gin (fts) WHERE status = 'published' AND retracted_in IS NULL;

-- ---- blocks: one row per block of a stored IR (content addressed, so immutable), written the first time a search
-- meets a live file (`block_fts_done` is the marker) and never rewritten. Only the vector is kept; the text,
-- anchor and path of a hit are read from the IR of the files that have hits.
CREATE TABLE _oo_docs.block_fts_done (
  ir_b3 text PRIMARY KEY,
  blocks integer NOT NULL CHECK (blocks >= 0),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE _oo_docs.block_fts (
  ir_b3 text NOT NULL REFERENCES _oo_docs.block_fts_done (ir_b3) ON DELETE CASCADE,
  ord integer NOT NULL CHECK (ord >= 0),
  tsv tsvector NOT NULL,
  PRIMARY KEY (ir_b3, ord)
);
CREATE INDEX block_fts_gin ON _oo_docs.block_fts USING gin (tsv);

GRANT SELECT, INSERT, DELETE ON _oo_docs.block_fts_done, _oo_docs.block_fts TO oo_docs_runtime;
