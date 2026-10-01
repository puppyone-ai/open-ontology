\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
INSERT INTO jobs.schedule_occurrences (schedule_id, scheduled_at, project_id, task_id)
VALUES (
  'cccccccc-cccc-4ccc-8ccc-ccccccccccc1',
  timestamptz '2026-10-04 00:00:00+00',
  '22222222-2222-4222-8222-222222222222',
  'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb5'
);
