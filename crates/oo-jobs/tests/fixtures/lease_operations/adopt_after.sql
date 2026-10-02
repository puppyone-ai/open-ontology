\ir prelude.sql
SELECT count(*) FILTER (WHERE p.prosecdef)::text
  || '|' || coalesce(string_agg(pg_get_function_identity_arguments(p.oid), ','), '')
FROM pg_proc AS p
JOIN pg_namespace AS n ON n.oid = p.pronamespace
WHERE n.nspname = 'jobs'
  AND p.proname = 'claim';
SELECT jobs.claim();
