SELECT oid::text
FROM pg_constraint
WHERE conname = 'task_events_task_project_fkey';
