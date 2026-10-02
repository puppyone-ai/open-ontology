\set ON_ERROR_STOP on
UPDATE jobs.tasks
SET state = 'running',
    attempt = attempt + 1,
    generation = generation + 1,
    lease_owner = 'worker-a',
    lease_until = clock_timestamp() + interval '30 seconds',
    updated_at = clock_timestamp()
WHERE id = 'a0750001-0001-4000-8000-000000000011'
  AND state = 'queued'
  AND generation = 0
  AND lease_owner IS NULL;

UPDATE jobs.tasks
SET state = 'succeeded',
    lease_owner = NULL,
    lease_until = NULL,
    updated_at = clock_timestamp()
WHERE id = 'a0750001-0001-4000-8000-000000000011'
  AND state = 'running'
  AND generation = 1
  AND lease_owner = 'worker-a';

SELECT count(*)::text || '|' || bool_and(event.project_id = task.project_id)::text
FROM jobs.task_events AS event
JOIN jobs.tasks AS task ON task.id = event.task_id
WHERE task.id = 'a0750001-0001-4000-8000-000000000011';
