\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
INSERT INTO jobs.tasks (
  id, project_id, kind, payload_version, payload, config_version, artifact_digest, dedupe_key, max_attempts, run_at
) VALUES (
  'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaa11',
  '11111111-1111-4111-8111-111111111111',
  'snapshot_block',
  1,
  jsonb_build_object('p', repeat('x', 17000)),
  'cfg-1',
  repeat('a', 64),
  'too-big',
  3,
  clock_timestamp()
);
