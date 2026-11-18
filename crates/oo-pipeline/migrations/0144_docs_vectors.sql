-- docs-vectors/1 phase expand
-- OPTIONAL (10 11.3, S13). Run by oo_docs::store::install as oo_docs_owner, inside the installer
-- transaction (advisory lock 6413202611), only when the server ships the `vector` extension (the
-- pgvector docs-db image, supply-chain/pgvector-admission.json) and the installer already created
-- the extension (CREATE EXTENSION needs an administrator). No BEGIN or COMMIT here; the installer
-- inserts the schema_migrations row. Without pgvector the migration is skipped and the engine
-- computes the same scores from the block text (no table is needed for correctness).
-- Vectors are stored per (drive, block id, model version); the model version is part of the key,
-- so a new embedding model never overwrites the one a replayed pack was compiled with.

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-suggest/1') THEN
    RAISE EXCEPTION 'docs-suggest/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_catalog.pg_extension WHERE extname = 'vector') THEN
    RAISE EXCEPTION 'the vector extension is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

CREATE TABLE _oo_docs.block_vector (
  drive_id uuid NOT NULL REFERENCES _oo_docs.drive(id),
  block_id text NOT NULL,
  model_version text NOT NULL CHECK (model_version ~ '^[a-z0-9._/-]{1,64}$'),
  dim integer NOT NULL CHECK (dim BETWEEN 8 AND 2000),
  embedding public.vector(256) NOT NULL,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (drive_id, model_version, block_id));

GRANT SELECT, INSERT, DELETE ON _oo_docs.block_vector TO oo_docs_runtime;
