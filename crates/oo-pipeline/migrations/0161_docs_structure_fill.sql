-- docs-structure-fill/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-structure/1, inside the installer
-- transaction. No BEGIN or COMMIT here; the installer inserts the schema_migrations row.
-- D93: a confirmed table class is filled row by row (columns are properties, rows are instances). What one file's
-- extraction filled is counted here, once per file, so the Structure tab shows the filled instances per class
-- without reading facts. Additive: nothing existing is rewritten.

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-structure/1') THEN
    RAISE EXCEPTION 'docs-structure/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

CREATE TABLE _oo_docs.structure_fill (
  drive_id uuid NOT NULL REFERENCES _oo_docs.drive (id),
  file_id uuid NOT NULL REFERENCES _oo_docs.file (id),
  version_id uuid NOT NULL REFERENCES _oo_docs.file_version (id),
  class_key text NOT NULL CHECK (class_key ~ '^[a-z][a-z0-9_]{0,63}$'),
  rows_total integer NOT NULL CHECK (rows_total >= 0),
  rows_filled integer NOT NULL CHECK (rows_filled >= 0),
  rows_unnamed integer NOT NULL CHECK (rows_unnamed >= 0),
  cells integer NOT NULL CHECK (cells >= 0),
  relations integer NOT NULL CHECK (relations >= 0),
  rejected integer NOT NULL CHECK (rejected >= 0),
  truncated boolean NOT NULL DEFAULT false,
  filled_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (drive_id, file_id)
);
CREATE INDEX structure_fill_class ON _oo_docs.structure_fill (drive_id, class_key);

GRANT SELECT, INSERT, UPDATE, DELETE ON _oo_docs.structure_fill TO oo_docs_runtime;
