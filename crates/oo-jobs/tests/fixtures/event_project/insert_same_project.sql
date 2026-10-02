\set ON_ERROR_STOP on
INSERT INTO jobs.task_events (
  project_id, task_id, from_state, to_state, generation, attempt, lease_owner, note
) VALUES (
  '11111111-1111-4111-8111-111111111111',
  'a0750001-0001-4000-8000-000000000012',
  'queued',
  'queued',
  99,
  0,
  'spoofed-worker',
  'same-project direct'
);

SELECT task.generation::text
       || '|' || coalesce(task.lease_owner, '')
       || '|' || event.generation::text
       || '|' || coalesce(event.lease_owner, '')
       || '|' || (event.project_id = task.project_id)::text
FROM jobs.tasks AS task
JOIN jobs.task_events AS event
  ON event.task_id = task.id
 AND event.note = 'same-project direct'
WHERE task.id = 'a0750001-0001-4000-8000-000000000012';
