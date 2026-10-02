\ir prelude.sql
SELECT p.oid::text
FROM pg_proc AS p
JOIN pg_namespace AS n ON n.oid = p.pronamespace
WHERE n.nspname = 'jobs'
  AND p.proname = 'claim'
  AND p.prosecdef;
