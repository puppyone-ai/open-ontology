-- docs-media-profile/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-drive-scale/1, inside the installer
-- transaction. No BEGIN or COMMIT here; the installer inserts the schema_migrations row.
-- D74 revision (2026-11-21): a recording is classified before speakers are told apart. One row per blob
-- (the same audio under any name shares it): the class, why, the signals the decision used, and who decided
-- (the rules, or the model when the rules were unsure).

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

CREATE TABLE _oo_docs.media_profile (
  b3 text PRIMARY KEY CHECK (b3 ~ '^[0-9a-f]{64}$'),
  class text NOT NULL CHECK (class IN ('conversation', 'non_conversation', 'unsure')),
  kind text NOT NULL CHECK (char_length(kind) BETWEEN 1 AND 40),
  confidence_bp integer NOT NULL CHECK (confidence_bp BETWEEN 0 AND 10000),
  source text NOT NULL CHECK (source IN ('rules', 'model')),
  signals jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (pg_catalog.jsonb_typeof(signals) = 'object'),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

GRANT SELECT, INSERT, UPDATE, DELETE ON _oo_docs.media_profile TO oo_docs_runtime;
