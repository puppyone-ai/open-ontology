-- jobs-bootstrap/1
-- Installer script. Run it in the control database as a role that can create
-- roles. oo_jobs_owner is the NOLOGIN schema owner. oo_jobs_runtime is the
-- internal jobs principal. Neither role is an Agent credential. Membership is
-- not granted in either direction: the runtime cannot become the owner.

CREATE ROLE oo_jobs_owner
  NOLOGIN
  NOSUPERUSER
  NOCREATEDB
  NOCREATEROLE
  NOINHERIT
  NOBYPASSRLS
  NOREPLICATION;

CREATE ROLE oo_jobs_runtime
  NOLOGIN
  NOSUPERUSER
  NOCREATEDB
  NOCREATEROLE
  NOINHERIT
  NOBYPASSRLS
  NOREPLICATION;

DO $$
DECLARE
  control_db text := current_database();
BEGIN
  EXECUTE format('REVOKE ALL ON DATABASE %I FROM PUBLIC', control_db);
  EXECUTE format('GRANT CONNECT, CREATE ON DATABASE %I TO oo_jobs_owner', control_db);
  EXECUTE format('GRANT CONNECT ON DATABASE %I TO oo_jobs_runtime', control_db);
  EXECUTE format('REVOKE CREATE ON DATABASE %I FROM oo_jobs_runtime', control_db);
  EXECUTE format('REVOKE TEMP ON DATABASE %I FROM PUBLIC', control_db);
  EXECUTE format('REVOKE TEMP ON DATABASE %I FROM oo_jobs_runtime', control_db);
END
$$;

REVOKE CREATE ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM oo_jobs_runtime;
GRANT USAGE ON SCHEMA public TO oo_jobs_runtime;
