\pset pager off
\echo === session ===
SELECT current_user, session_user;
\echo === privileges on jobs.tasks ===
SELECT has_table_privilege(current_user, 'jobs.tasks', 'INSERT') AS can_insert,
       has_table_privilege(current_user, 'jobs.tasks', 'UPDATE') AS can_update,
       has_table_privilege(current_user, 'jobs.tasks', 'DELETE') AS can_delete;
\echo === jobs.tasks columns ===
SELECT column_name
FROM information_schema.columns
WHERE table_schema = 'jobs' AND table_name = 'tasks'
ORDER BY ordinal_position;
\echo === jobs.schedules columns ===
SELECT column_name
FROM information_schema.columns
WHERE table_schema = 'jobs' AND table_name = 'schedules'
ORDER BY ordinal_position;
\echo === function names visible to runtime ===
SELECT p.proname
FROM pg_proc p
JOIN pg_namespace n ON n.oid = p.pronamespace
WHERE n.nspname = 'jobs'
  AND p.proname = ANY (ARRAY[
    'enqueue', 'claim', 'heartbeat', 'complete_success',
    'complete_failure', 'retry_failure', 'cancel'
  ]::text[])
ORDER BY 1;
