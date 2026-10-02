\ir prelude.sql
SELECT coalesce(string_agg(
  p.proname || '|' || p.prosecdef::text || '|' || pg_get_function_identity_arguments(p.oid),
  E'\n' ORDER BY p.proname, pg_get_function_identity_arguments(p.oid)
), '')
FROM pg_proc AS p
JOIN pg_namespace AS n ON n.oid = p.pronamespace
WHERE n.nspname = 'jobs'
  AND p.proname IN ('claim', 'enqueue', 'heartbeat');
