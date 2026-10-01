\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
INSERT INTO jobs.tasks (
  id, project_id, kind, payload_version, payload, config_version, artifact_digest, dedupe_key, max_attempts, run_at
) VALUES (
  'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaa12',
  '11111111-1111-4111-8111-111111111111',
  'snapshot_block',
  1,
  '{"v":1}'::jsonb,
  'cfg-1',
  'not-a-digest',
  'bad-digest',
  3,
  clock_timestamp()
);
