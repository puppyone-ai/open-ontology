\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
SELECT pg_terminate_backend(pid)::text
FROM pg_stat_activity
WHERE application_name = :'app_name'
  AND pid <> pg_backend_pid();
