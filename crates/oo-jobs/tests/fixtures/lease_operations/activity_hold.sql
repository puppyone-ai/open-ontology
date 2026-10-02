\ir prelude.sql
SELECT count(*)::text
FROM pg_stat_activity
WHERE application_name = 'a102-claim-hold'
  AND query LIKE '%pg_sleep%';
