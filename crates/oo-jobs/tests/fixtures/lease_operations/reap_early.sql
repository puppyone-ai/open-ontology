\set ON_ERROR_STOP on
SELECT pg_terminate_backend(pid)
FROM pg_stat_activity
WHERE pid <> pg_backend_pid()
  AND datname IN (
    'oo_a102_bad',
    'oo_a102_upgrade',
    'oo_a102_adopt',
    'oo_a102_forced',
    'oo_a102_v1',
    'oo_a102_partial',
    'oo_a102_lie'
  );
DROP DATABASE IF EXISTS oo_a102_bad;
DROP DATABASE IF EXISTS oo_a102_upgrade;
DROP DATABASE IF EXISTS oo_a102_adopt;
DROP DATABASE IF EXISTS oo_a102_forced;
DROP DATABASE IF EXISTS oo_a102_v1;
DROP DATABASE IF EXISTS oo_a102_partial;
DROP DATABASE IF EXISTS oo_a102_lie;
DROP ROLE IF EXISTS oo_a102_public_login;
DROP ROLE IF EXISTS oo_a102_runtime_login;
DROP ROLE IF EXISTS oo_a102_owner_login;
DROP ROLE IF EXISTS oo_jobs_runtime;
DROP ROLE IF EXISTS oo_jobs_owner;
CREATE DATABASE oo_a102_bad;
CREATE DATABASE oo_a102_upgrade;
CREATE DATABASE oo_a102_adopt;
CREATE DATABASE oo_a102_forced;
CREATE DATABASE oo_a102_v1;
CREATE DATABASE oo_a102_partial;
CREATE DATABASE oo_a102_lie;
