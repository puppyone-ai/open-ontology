\set ON_ERROR_STOP on
DO $$
DECLARE
  bootstrap_password text := rtrim(pg_catalog.pg_read_file('/var/lib/oo-fixture/bootstrap_password'), E'\r\n');
  runtime_password text := rtrim(pg_catalog.pg_read_file('/var/lib/oo-fixture/runtime_password'), E'\r\n');
  gateway_password text := rtrim(pg_catalog.pg_read_file('/var/lib/oo-fixture/gateway_password'), E'\r\n');
  agent_password text := rtrim(pg_catalog.pg_read_file('/var/lib/oo-fixture/agent_password'), E'\r\n');
BEGIN
  EXECUTE format(
    'CREATE ROLE oo_a072_bootstrap LOGIN PASSWORD %L NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION',
    bootstrap_password
  );
  EXECUTE format(
    'CREATE ROLE oo_a072_runtime LOGIN PASSWORD %L NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION',
    runtime_password
  );
  EXECUTE format(
    'CREATE ROLE oo_a072_gateway LOGIN PASSWORD %L NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION',
    gateway_password
  );
  EXECUTE format(
    'CREATE ROLE oo_a072_agent LOGIN PASSWORD %L NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION',
    agent_password
  );
END
$$;

GRANT oo_target_owner TO oo_a072_bootstrap WITH INHERIT FALSE, SET TRUE;
GRANT CONNECT ON DATABASE oo_agent TO oo_a072_bootstrap;
GRANT oo_pipeline_runtime TO oo_a072_runtime WITH INHERIT TRUE, SET FALSE;

ALTER ROLE oo_a072_bootstrap SET search_path = pg_catalog;
ALTER ROLE oo_a072_runtime SET search_path = pg_catalog;
ALTER ROLE oo_a072_gateway SET search_path = pg_catalog;
ALTER ROLE oo_a072_agent SET search_path = pg_catalog;
