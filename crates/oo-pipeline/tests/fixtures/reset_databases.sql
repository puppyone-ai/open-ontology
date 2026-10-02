SELECT pg_terminate_backend(pid)
FROM pg_stat_activity
WHERE pid <> pg_backend_pid()
  AND (
    datname IN ('oo_agent', 'oo_agent_fail', 'oo_agent_bare', 'oo_control')
    OR usename IN (
      'oo_a072_runtime',
      'oo_a072_other',
      'oo_a072_bootstrap',
      'oo_a072_gateway',
      'oo_a072_agent',
      'probe_login',
      'probe_runtime',
      'probe_owner'
    )
  );

DROP DATABASE IF EXISTS oo_agent;
DROP DATABASE IF EXISTS oo_agent_fail;
DROP DATABASE IF EXISTS oo_agent_bare;
DROP DATABASE IF EXISTS oo_control;

DROP SCHEMA IF EXISTS probe CASCADE;

DO $$
DECLARE
  role_name text;
BEGIN
  FOREACH role_name IN ARRAY ARRAY[
    'oo_a072_runtime',
    'oo_a072_other',
    'oo_a072_bootstrap',
    'oo_a072_gateway',
    'oo_a072_agent',
    'probe_login',
    'probe_runtime',
    'probe_owner',
    'oo_pipeline_runtime',
    'oo_target_owner'
  ]
  LOOP
    IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = role_name) THEN
      EXECUTE format('REVOKE ALL ON DATABASE postgres FROM %I', role_name);
      EXECUTE format('DROP OWNED BY %I CASCADE', role_name);
      EXECUTE format('DROP ROLE %I', role_name);
    END IF;
  END LOOP;
END
$$;

CREATE DATABASE oo_agent;
CREATE DATABASE oo_agent_fail;
CREATE DATABASE oo_agent_bare;
CREATE DATABASE oo_control;
