\ir prelude.sql
SELECT count(*)::text
FROM pg_constraint AS constraint_row
JOIN pg_namespace AS namespace_row
  ON namespace_row.oid = constraint_row.connamespace
WHERE namespace_row.nspname = 'jobs'
  AND constraint_row.conname = 'task_events_task_project_fkey';
