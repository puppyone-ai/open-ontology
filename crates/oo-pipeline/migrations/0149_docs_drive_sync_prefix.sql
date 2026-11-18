-- docs-drive-sync-prefix/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-cloud-audit/1, inside the installer
-- transaction (advisory lock 6413202611). No BEGIN or COMMIT here; the installer inserts the
-- schema_migrations row.
-- L7 parallel listing (10 11.3, B-S11-03): a huge bucket is listed per first level prefix, several at once,
-- and each prefix is its own unit: its own row, its own generation counter, its own failure. A prefix that
-- fails to list leaves the others swept and the Drive generation uncommitted.

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-cloud-audit/1') THEN
    RAISE EXCEPTION 'docs-cloud-audit/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

-- One row per (sync run, first level prefix). `prefix` is '' for the objects directly under the Drive root.
-- `generation` is the Drive's run (it marks `file.last_seen_generation`); `prefix_generation` counts the
-- complete listings of this prefix and moves only when this prefix listed completely.
CREATE TABLE _oo_docs.drive_sync_prefix (
  drive_id uuid NOT NULL,
  generation bigint NOT NULL,
  prefix text NOT NULL CHECK (char_length(prefix) <= 255),
  prefix_generation bigint NOT NULL CHECK (prefix_generation >= 1),
  status text NOT NULL CHECK (status IN ('running', 'complete', 'failed')),
  listed bigint NOT NULL DEFAULT 0,
  pages integer NOT NULL DEFAULT 0,
  error_code text,
  started_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  finished_at timestamptz,
  PRIMARY KEY (drive_id, generation, prefix),
  FOREIGN KEY (drive_id, generation) REFERENCES _oo_docs.drive_sync (drive_id, generation)
);
CREATE INDEX drive_sync_prefix_last ON _oo_docs.drive_sync_prefix (drive_id, prefix, prefix_generation)
  WHERE status = 'complete';

GRANT SELECT, INSERT, UPDATE, DELETE ON _oo_docs.drive_sync_prefix TO oo_docs_runtime;
