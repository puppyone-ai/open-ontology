\set ON_ERROR_STOP on
SET lock_timeout = '10s';
INSERT INTO jobs.task_events (
  project_id, task_id, to_state, generation, attempt, note
) VALUES (
  '22222222-2222-4222-8222-222222222222',
  'a0750001-0001-4000-8000-000000000012',
  'queued',
  0,
  0,
  'concurrent-cross'
);
