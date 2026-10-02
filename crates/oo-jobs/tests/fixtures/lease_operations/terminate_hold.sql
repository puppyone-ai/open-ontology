\ir prelude.sql
SELECT pg_terminate_backend(pid)::text
FROM pg_stat_activity
WHERE application_name = 'a102-claim-hold'
  AND pid <> pg_backend_pid();
