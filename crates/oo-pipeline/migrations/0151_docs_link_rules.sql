-- docs-link-rules/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-agent-tokens/1, inside the installer
-- transaction (advisory lock 6413202611). No BEGIN or COMMIT here; the installer inserts the
-- schema_migrations row.
-- D83 one ontology layer: the rules that link document entities to rows of database tables, and the binding to
-- the Agent DB the projection writes into. Both live in docs-db (the engine's own database); the Agent DB is
-- only written under `_oo_docproj` (10 11.3). The binding holds a secret REFERENCE, never a URL.

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-agent-tokens/1') THEN
    RAISE EXCEPTION 'docs-agent-tokens/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

-- One rule per (Drive, table): match `name_col` (and the optional `alias_cols`) of `db_table` against document
-- entities of `entity_type` (empty: any type). `key_col` identifies the row. Plain identifiers only; the rule
-- is validated again by the projection before any SQL is built.
CREATE TABLE _oo_docs.db_link_rule (
  drive_id uuid NOT NULL REFERENCES _oo_docs.drive (id),
  db_table text NOT NULL CHECK (db_table ~ '^[A-Za-z_][A-Za-z0-9_]{0,62}[.][A-Za-z_][A-Za-z0-9_]{0,62}$'),
  key_col text NOT NULL CHECK (key_col ~ '^[A-Za-z_][A-Za-z0-9_]{0,62}$'),
  name_col text NOT NULL CHECK (name_col ~ '^[A-Za-z_][A-Za-z0-9_]{0,62}$'),
  alias_cols text[] NOT NULL DEFAULT '{}' CHECK (cardinality(alias_cols) <= 8),
  entity_type text NOT NULL DEFAULT '' CHECK (char_length(entity_type) <= 64),
  revision bigint NOT NULL DEFAULT 1 CHECK (revision >= 1),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (drive_id, db_table)
);

-- At most one row: where the Agent DB login is (a secret reference: `env:NAME`, `file:/abs/path` or a name in
-- the secrets directory). `status` is the last projection outcome, for `oo docs link ls --status`.
CREATE TABLE _oo_docs.agent_db_binding (
  singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
  secret_ref text NOT NULL CHECK (char_length(secret_ref) BETWEEN 1 AND 300),
  status text NOT NULL DEFAULT 'unchecked' CHECK (status IN ('unchecked', 'ok', 'failed')),
  last_error_code text CHECK (char_length(last_error_code) <= 64),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

GRANT SELECT, INSERT, UPDATE, DELETE ON _oo_docs.db_link_rule TO oo_docs_runtime;
GRANT SELECT, INSERT, UPDATE, DELETE ON _oo_docs.agent_db_binding TO oo_docs_runtime;
