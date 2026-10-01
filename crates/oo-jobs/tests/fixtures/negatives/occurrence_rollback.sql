\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
BEGIN;
INSERT INTO jobs.tasks (
  id, project_id, kind, payload_version, payload, config_version, artifact_digest, dedupe_key, max_attempts, run_at
) VALUES (
  'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaa18',
  '11111111-1111-4111-8111-111111111111',
  'snapshot_block',
  1,
  '{"v":1}'::jsonb,
  'cfg-1',
  repeat('a', 64),
  'occurrence-rollback',
  3,
  clock_timestamp()
);
INSERT INTO jobs.schedule_occurrences (schedule_id, scheduled_at, project_id, task_id)
VALUES (
  'cccccccc-cccc-4ccc-8ccc-ccccccccccc1',
  timestamptz '2026-10-03 00:00:00+00',
  '11111111-1111-4111-8111-111111111111',
  'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaa18'
);
COMMIT;
