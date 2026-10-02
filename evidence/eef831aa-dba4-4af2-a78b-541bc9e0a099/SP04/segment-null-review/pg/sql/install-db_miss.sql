BEGIN;
SET LOCAL lock_timeout = '5s';
SELECT pg_catalog.pg_advisory_xact_lock(6404202605);
-- target-bootstrap/1
-- Installer script. Run it in the target database as a role that can create
-- roles and revoke pg_catalog grants. oo_target_owner is the NOLOGIN schema
-- owner. oo_pipeline_runtime is the NOLOGIN pipeline principal. Neither role
-- is an Agent or Gateway credential. Membership is not granted here.



ALTER ROLE oo_target_owner SET search_path = pg_catalog;
ALTER ROLE oo_pipeline_runtime SET search_path = pg_catalog;

DO $$
DECLARE
  target_db text := current_database();
  advisory regprocedure;
BEGIN
  EXECUTE format('REVOKE ALL ON DATABASE %I FROM PUBLIC', target_db);
  EXECUTE format(
    'GRANT CONNECT, CREATE, TEMP ON DATABASE %I TO oo_target_owner',
    target_db
  );
  EXECUTE format('GRANT CONNECT ON DATABASE %I TO oo_pipeline_runtime', target_db);
  EXECUTE format('REVOKE CREATE, TEMP ON DATABASE %I FROM oo_pipeline_runtime', target_db);
  EXECUTE format('REVOKE TEMP ON DATABASE %I FROM PUBLIC', target_db);
  EXECUTE format('REVOKE CREATE ON DATABASE %I FROM PUBLIC', target_db);

  -- Installer closure. The schema owner is not a superuser and cannot revoke
  -- these catalog grants itself. Runtime is not on the grant list.
  -- pg_try_advisory_lock does not match the pg_advisory prefix. A later GRANT
  -- on a still-default ACL would keep PUBLIC execute, so revoke both names
  -- before granting the owner the three functions the fence uses.
  FOR advisory IN
    SELECT pg_proc.oid::regprocedure
    FROM pg_proc
    JOIN pg_namespace ON pg_namespace.oid = pg_proc.pronamespace
    WHERE pg_namespace.nspname = 'pg_catalog'
      AND (
        pg_proc.proname LIKE 'pg\_advisory%' ESCAPE '\'
        OR pg_proc.proname LIKE 'pg\_try\_advisory%' ESCAPE '\'
      )
  LOOP
    EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC', advisory);
  END LOOP;

  EXECUTE 'REVOKE ALL ON FUNCTION pg_catalog.set_config(text, text, boolean) FROM PUBLIC';
  EXECUTE 'GRANT EXECUTE ON FUNCTION pg_catalog.set_config(text, text, boolean) TO oo_target_owner';
  EXECUTE 'GRANT EXECUTE ON FUNCTION pg_catalog.pg_try_advisory_lock(integer, integer) TO oo_target_owner';
  EXECUTE 'GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_unlock(integer, integer) TO oo_target_owner';
  EXECUTE 'GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_xact_lock(bigint) TO oo_target_owner';
END
$$;

REVOKE CREATE ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM oo_pipeline_runtime;
GRANT USAGE ON SCHEMA public TO oo_pipeline_runtime;
SET ROLE oo_target_owner;
RESET ROLE;
GRANT CONNECT ON DATABASE db_miss TO oo_a099_bootstrap;
COMMIT;
