\set ON_ERROR_STOP on
DO $check$
DECLARE
  owner_row pg_roles%ROWTYPE;
  runtime_row pg_roles%ROWTYPE;
BEGIN
  SELECT * INTO owner_row FROM pg_roles WHERE rolname = 'oo_jobs_owner';
  SELECT * INTO runtime_row FROM pg_roles WHERE rolname = 'oo_jobs_runtime';
  IF owner_row.rolname IS NULL OR runtime_row.rolname IS NULL THEN
    RAISE EXCEPTION 'jobs roles are missing' USING ERRCODE = 'OO004';
  END IF;
  IF owner_row.rolsuper OR owner_row.rolcreatedb OR owner_row.rolcreaterole
     OR owner_row.rolcanlogin OR owner_row.rolbypassrls OR owner_row.rolreplication
     OR runtime_row.rolsuper OR runtime_row.rolcreatedb OR runtime_row.rolcreaterole
     OR runtime_row.rolcanlogin OR runtime_row.rolbypassrls OR runtime_row.rolreplication
  THEN
    RAISE EXCEPTION 'jobs role has a forbidden attribute' USING ERRCODE = 'OO004';
  END IF;
  IF pg_has_role('oo_jobs_runtime', 'oo_jobs_owner', 'member')
     OR pg_has_role('oo_jobs_owner', 'oo_jobs_runtime', 'member')
  THEN
    RAISE EXCEPTION 'migration owner is granted to the runtime role' USING ERRCODE = 'OO004';
  END IF;
  IF has_schema_privilege('oo_jobs_runtime', 'jobs', 'CREATE')
     OR has_schema_privilege('public', 'jobs', 'USAGE')
     OR has_schema_privilege('public', 'jobs', 'CREATE')
     OR has_database_privilege('oo_jobs_runtime', current_database(), 'CREATE')
     OR has_database_privilege('oo_jobs_runtime', current_database(), 'TEMP')
  THEN
    RAISE EXCEPTION 'runtime or public received schema CREATE' USING ERRCODE = 'OO004';
  END IF;
  IF has_table_privilege('oo_jobs_runtime', 'jobs.tasks', 'DELETE')
     OR has_table_privilege('oo_jobs_runtime', 'jobs.tasks', 'TRUNCATE')
     OR has_table_privilege('oo_jobs_runtime', 'jobs.schema_migrations', 'INSERT')
     OR has_table_privilege('oo_jobs_runtime', 'jobs.schema_migrations', 'UPDATE')
     OR has_table_privilege('oo_jobs_runtime', 'jobs.schema_migrations', 'DELETE')
     OR has_table_privilege('oo_jobs_runtime', 'jobs.migration_lock', 'SELECT')
     OR has_column_privilege('oo_jobs_runtime', 'jobs.tasks', 'artifact_digest', 'UPDATE')
     OR has_column_privilege('oo_jobs_runtime', 'jobs.tasks', 'payload', 'UPDATE')
     OR has_column_privilege('oo_jobs_runtime', 'jobs.tasks', 'config_version', 'UPDATE')
     OR has_column_privilege('oo_jobs_runtime', 'jobs.tasks', 'state', 'INSERT')
     OR has_column_privilege('oo_jobs_runtime', 'jobs.leases', 'target_fence_certified', 'UPDATE')
     OR has_column_privilege('oo_jobs_runtime', 'jobs.leases', 'target_fence_certified', 'INSERT')
  THEN
    RAISE EXCEPTION 'runtime grant is wider than the queue contract' USING ERRCODE = 'OO004';
  END IF;
  IF NOT has_table_privilege('oo_jobs_runtime', 'jobs.tasks', 'SELECT')
     OR NOT has_column_privilege('oo_jobs_runtime', 'jobs.tasks', 'project_id', 'INSERT')
     OR NOT has_column_privilege('oo_jobs_runtime', 'jobs.tasks', 'state', 'UPDATE')
     OR NOT has_column_privilege('oo_jobs_runtime', 'jobs.tasks', 'lease_owner', 'UPDATE')
     OR NOT has_table_privilege('oo_jobs_runtime', 'jobs.dead_letters', 'SELECT')
     OR NOT has_table_privilege('oo_jobs_runtime', 'jobs.task_audit', 'SELECT')
     OR NOT has_table_privilege('oo_jobs_runtime', 'jobs.service_readiness', 'SELECT')
     OR NOT has_function_privilege('oo_jobs_runtime', 'jobs.enforce_task_update()', 'EXECUTE')
  THEN
    RAISE EXCEPTION 'runtime is missing a required queue grant' USING ERRCODE = 'OO004';
  END IF;
  IF has_function_privilege('public', 'jobs.enforce_task_update()', 'EXECUTE')
     OR has_function_privilege('public', 'jobs.audit_task_change()', 'EXECUTE')
  THEN
    RAISE EXCEPTION 'public can execute a jobs function' USING ERRCODE = 'OO004';
  END IF;
END
$check$;

BEGIN;
SET ROLE oo_jobs_owner;
CREATE TABLE jobs.future_probe (id integer);
CREATE FUNCTION jobs.future_probe()
RETURNS integer
LANGUAGE sql
SET search_path = pg_catalog
AS 'SELECT 1';
DO $future$
DECLARE
  table_acl text;
  function_acl text;
BEGIN
  SELECT relacl::text INTO table_acl
  FROM pg_class
  WHERE oid = 'jobs.future_probe'::regclass;
  SELECT proacl::text INTO function_acl
  FROM pg_proc
  WHERE oid = 'jobs.future_probe()'::regprocedure;
  IF table_acl IS NOT NULL AND (table_acl LIKE '{=%' OR table_acl LIKE '%,=%') THEN
    RAISE EXCEPTION 'future table leaked a public grant' USING ERRCODE = 'OO004';
  END IF;
  IF has_table_privilege('public', 'jobs.future_probe', 'SELECT')
     OR has_function_privilege('public', 'jobs.future_probe()', 'EXECUTE')
  THEN
    RAISE EXCEPTION 'future object leaked a public grant' USING ERRCODE = 'OO004';
  END IF;
  IF function_acl IS NOT NULL AND (function_acl LIKE '{=%' OR function_acl LIKE '%,=%') THEN
    RAISE EXCEPTION 'future function acl names public' USING ERRCODE = 'OO004';
  END IF;
END
$future$;
ROLLBACK;
