\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
SELECT count(*)
FROM pg_stat_activity
WHERE application_name = :'app_name'
  AND state = 'active'
  AND query LIKE '%pg_sleep%';
