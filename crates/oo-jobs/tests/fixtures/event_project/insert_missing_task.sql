\set ON_ERROR_STOP on
INSERT INTO jobs.task_events (
  project_id, task_id, to_state, generation, attempt, note
) VALUES (
  '11111111-1111-4111-8111-111111111111',
  'ffffffff-ffff-4fff-8fff-ffffffffffff',
  'queued',
  0,
  0,
  'missing task'
);
