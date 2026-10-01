SELECT pg_terminate_backend(pid)
FROM pg_stat_activity
WHERE datname IN ('oo_control', 'oo_control_fail')
  AND pid <> pg_backend_pid();

DROP DATABASE IF EXISTS oo_control;
DROP DATABASE IF EXISTS oo_control_fail;
DROP ROLE IF EXISTS oo_jobs_nobody;
DROP ROLE IF EXISTS oo_jobs_runtime;
DROP ROLE IF EXISTS oo_jobs_owner;

CREATE DATABASE oo_control;
CREATE DATABASE oo_control_fail;
