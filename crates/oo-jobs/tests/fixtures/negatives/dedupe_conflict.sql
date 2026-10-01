\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
INSERT INTO jobs.tasks (
  id, project_id, kind, payload_version, payload, config_version, artifact_digest, dedupe_key, max_attempts, run_at
) VALUES (
  'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaa13',
  '11111111-1111-4111-8111-111111111111',
  'snapshot_block',
  2,
  '{"v":2}'::jsonb,
  'cfg-2',
  repeat('b', 64),
  'shared-key',
  3,
  clock_timestamp()
);
