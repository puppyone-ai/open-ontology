-- docs-drive-scale/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-search-index/1, inside the installer
-- transaction. No BEGIN or COMMIT here; the installer inserts the schema_migrations row.
-- A code folder linked as a source easily holds more than 10,000 files. The per-Drive file cap was 10,000 (default and
-- CHECK); it is now a real guard against runaway growth (10,000,000), with a default of 1,000,000. Drives that still carry the old
-- default move to the new one; no row is rewritten otherwise.

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-search-index/1') THEN
    RAISE EXCEPTION 'docs-search-index/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

ALTER TABLE _oo_docs.drive DROP CONSTRAINT drive_max_files_check;
ALTER TABLE _oo_docs.drive ADD CONSTRAINT drive_max_files_check CHECK (max_files BETWEEN 1 AND 10000000);
ALTER TABLE _oo_docs.drive ALTER COLUMN max_files SET DEFAULT 1000000;
UPDATE _oo_docs.drive SET max_files = 1000000 WHERE max_files = 10000;
CREATE INDEX IF NOT EXISTS job_file ON _oo_docs.job (file_id, kind) WHERE file_id IS NOT NULL;
