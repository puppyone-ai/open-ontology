\set ON_ERROR_STOP on
DO $$
DECLARE
  bootstrap_password text := rtrim(pg_catalog.pg_read_file('/var/lib/oo-fixture/bootstrap_password'), E'\r\n');
  runtime_password text := rtrim(pg_catalog.pg_read_file('/var/lib/oo-fixture/runtime_password'), E'\r\n');
  gateway_password text := rtrim(pg_catalog.pg_read_file('/var/lib/oo-fixture/gateway_password'), E'\r\n');
  agent_password text := rtrim(pg_catalog.pg_read_file('/var/lib/oo-fixture/agent_password'), E'\r\n');
BEGIN
  EXECUTE format('ALTER ROLE oo_a072_bootstrap PASSWORD %L', bootstrap_password);
  EXECUTE format('ALTER ROLE oo_a072_runtime PASSWORD %L', runtime_password);
  EXECUTE format('ALTER ROLE oo_a072_gateway PASSWORD %L', gateway_password);
  EXECUTE format('ALTER ROLE oo_a072_agent PASSWORD %L', agent_password);
END
$$;
