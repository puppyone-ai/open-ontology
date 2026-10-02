SELECT
  (to_regclass('jobs.event_project_readiness') IS NULL)::text
  || '|' ||
  (SELECT schema_version_present::text FROM jobs.service_readiness)
  || '|' ||
  (SELECT count(*)::text FROM jobs.schema_migrations WHERE version = 'jobs-event-project/1')
  || '|' ||
  (SELECT count(*)::text FROM pg_constraint WHERE conname = 'task_events_task_project_fkey')
  || '|' ||
  (SELECT count(*)::text FROM pg_constraint WHERE conname = 'tasks_id_project_unique');
