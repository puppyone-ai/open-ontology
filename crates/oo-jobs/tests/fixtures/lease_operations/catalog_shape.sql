\ir prelude.sql
SELECT p.proname || '|' || p.prosecdef::text || '|' || coalesce(p.proconfig::text, '')
  || '|' || pg_get_function_identity_arguments(p.oid)
FROM pg_proc AS p
JOIN pg_namespace AS n ON n.oid = p.pronamespace
WHERE n.nspname = 'jobs'
  AND p.proname IN (
    'enqueue', 'claim', 'heartbeat', 'complete_success', 'complete_failure',
    'retry_failure', 'cancel', '_lease_token', '_require_lease_install'
  )
ORDER BY p.proname;
