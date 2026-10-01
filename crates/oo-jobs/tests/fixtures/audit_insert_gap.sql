\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
INSERT INTO jobs.task_events (
  project_id, task_id, from_state, to_state, generation, attempt, note
) VALUES (
  '11111111-1111-4111-8111-111111111111',
  'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa8',
  'queued',
  'queued',
  0,
  0,
  'runtime inserted this event directly'
);
