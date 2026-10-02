SELECT pg_terminate_backend(pid)
FROM pg_stat_activity
WHERE datname IN (
  'oo_control',
  'oo_control_upgrade',
  'oo_control_bad',
  'oo_control_rollback'
)
  AND pid <> pg_backend_pid();

DROP DATABASE IF EXISTS oo_control;
DROP DATABASE IF EXISTS oo_control_upgrade;
DROP DATABASE IF EXISTS oo_control_bad;
DROP DATABASE IF EXISTS oo_control_rollback;

DROP ROLE IF EXISTS oo_a075_login;
DROP ROLE IF EXISTS oo_a075_owner_login;
DROP ROLE IF EXISTS oo_jobs_runtime;
DROP ROLE IF EXISTS oo_jobs_owner;
