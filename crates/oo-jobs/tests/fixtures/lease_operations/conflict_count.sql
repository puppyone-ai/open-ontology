\ir prelude.sql
SELECT count(*)::text
FROM jobs.task_events AS event
JOIN jobs.tasks AS task ON task.id = event.task_id
WHERE task.dedupe_key = 'dedupe-alpha'
  AND task.project_id = '11111111-1111-4111-8111-111111111111';
