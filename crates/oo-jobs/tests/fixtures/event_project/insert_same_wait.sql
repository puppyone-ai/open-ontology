\set ON_ERROR_STOP on
SET lock_timeout = '10s';
INSERT INTO jobs.task_events (
  project_id, task_id, to_state, generation, attempt, note
) VALUES (
  '11111111-1111-4111-8111-111111111111',
  'a0750001-0001-4000-8000-000000000012',
  'queued',
  0,
  0,
  'concurrent-legal'
);
