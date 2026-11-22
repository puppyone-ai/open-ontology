-- docs-file-profile/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-drive-scale/1, inside the installer
-- transaction. No BEGIN or COMMIT here; the installer inserts the schema_migrations row.
-- D89: every file has a profile (what kind of document it is) that decides how it is read. The column is nullable: a
-- row that was never extracted shows the profile the API derives from its path and format. Nothing is rewritten.

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-drive-scale/1') THEN
    RAISE EXCEPTION 'docs-drive-scale/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

ALTER TABLE _oo_docs.file ADD COLUMN profile text
  CHECK (profile IN ('business', 'meeting', 'table', 'instruction', 'code', 'paper', 'media', 'other'));
