-- docs-bootstrap/1 phase expand
-- Run by oo_docs::store::install on a superuser connection, inside the
-- installer transaction that already holds pg_advisory_xact_lock(6413202611).
-- The installer owns BEGIN/COMMIT and the schema_migrations row; this file
-- contains neither. Roles are cluster-wide, so creation is guarded.
--
-- oo_docs_owner owns _oo_docs and every object in it. It is NOLOGIN and is
-- never granted to the runtime. oo_docs_runtime is the engine principal: it
-- gets table grants and EXECUTE on SECURITY DEFINER functions only where a
-- later file says so. Neither role is an Agent or Gateway credential.

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = current_user AND rolsuper
  ) THEN
    RAISE EXCEPTION 'docs bootstrap must run as a superuser'
      USING ERRCODE = 'OO013';
  END IF;
  IF to_regnamespace('_oo_docs') IS NOT NULL THEN
    RAISE EXCEPTION 'refusing to adopt an existing _oo_docs schema'
      USING ERRCODE = 'OO014';
  END IF;
END
$guard$;

DO $roles$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_docs_owner') THEN
    CREATE ROLE oo_docs_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE
      NOINHERIT NOBYPASSRLS NOREPLICATION;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_docs_runtime') THEN
    CREATE ROLE oo_docs_runtime NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE
      NOINHERIT NOBYPASSRLS NOREPLICATION;
  END IF;
END
$roles$;

CREATE SCHEMA _oo_docs AUTHORIZATION oo_docs_owner;
REVOKE ALL ON SCHEMA _oo_docs FROM PUBLIC;
GRANT USAGE ON SCHEMA _oo_docs TO oo_docs_runtime;
ALTER DEFAULT PRIVILEGES FOR ROLE oo_docs_owner REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;

SET LOCAL ROLE oo_docs_owner;

CREATE TABLE _oo_docs.schema_migrations (
  version text PRIMARY KEY CHECK (version ~ '^docs-[a-z-]+/[0-9]+$'),
  sha256 text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
  compat text NOT NULL DEFAULT 'additive' CHECK (compat IN ('additive', 'breaking')),
  applied_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE _oo_docs.migration_lock (
  singleton boolean PRIMARY KEY CHECK (singleton)
);
INSERT INTO _oo_docs.migration_lock VALUES (true);

CREATE FUNCTION _oo_docs.reject_mutation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog
AS $$
BEGIN
  RAISE EXCEPTION 'insert-only table' USING ERRCODE = 'OO030';
END
$$;

CREATE TRIGGER schema_migrations_insert_only
  BEFORE UPDATE OR DELETE ON _oo_docs.schema_migrations
  FOR EACH ROW EXECUTE FUNCTION _oo_docs.reject_mutation();
CREATE TRIGGER schema_migrations_no_truncate
  BEFORE TRUNCATE ON _oo_docs.schema_migrations
  FOR EACH STATEMENT EXECUTE FUNCTION _oo_docs.reject_mutation();

REVOKE ALL ON FUNCTION _oo_docs.reject_mutation() FROM PUBLIC;
GRANT SELECT ON _oo_docs.schema_migrations TO oo_docs_runtime;

RESET ROLE;
