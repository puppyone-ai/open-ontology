-- docs-llm/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-packs-shares/1, inside the
-- installer transaction (advisory lock 6413202611). No BEGIN or COMMIT here;
-- the installer inserts the schema_migrations row.
-- LLM supplement: block level response cache and daily usage (10 §3.2, §5.2.4).

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-packs-shares/1') THEN
    RAISE EXCEPTION 'docs-packs-shares/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

CREATE TABLE _oo_docs.llm_cache (
  key text PRIMARY KEY CHECK (key ~ '^[0-9a-f]{64}$'),
  prompt_version text NOT NULL, model text NOT NULL, response_b3 text NOT NULL,   -- response body lives in the CAS
  outcome text NOT NULL CHECK (outcome IN ('ok','schema_invalid','refused')),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp());
CREATE TABLE _oo_docs.llm_usage (
  drive_id uuid NOT NULL REFERENCES _oo_docs.drive(id), day date NOT NULL,
  reserved integer NOT NULL DEFAULT 0, completed integer NOT NULL DEFAULT 0, failed integer NOT NULL DEFAULT 0,
  cache_hits integer NOT NULL DEFAULT 0, tokens_in bigint NOT NULL DEFAULT 0, tokens_out bigint NOT NULL DEFAULT 0,
  files_limited integer NOT NULL DEFAULT 0,
  PRIMARY KEY (drive_id, day));

GRANT SELECT, INSERT, UPDATE ON _oo_docs.llm_cache, _oo_docs.llm_usage TO oo_docs_runtime;
