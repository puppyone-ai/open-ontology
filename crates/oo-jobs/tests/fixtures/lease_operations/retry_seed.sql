\ir prelude.sql
SET ROLE oo_jobs_owner;
INSERT INTO jobs.tasks (
  project_id, kind, payload_version, payload, config_version, artifact_digest,
  dedupe_key, state, run_at, attempt, max_attempts, lease_owner, lease_until, generation
) VALUES
(
  '99999999-9999-4999-8999-999999999999', 'retrykind', 1, '{"n":1}'::jsonb, 'cfg-r', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'retry-1', 'running', clock_timestamp(), 1, 3, 'worker-retry', clock_timestamp() + interval '5 minutes', 1
),
(
  '99999999-9999-4999-8999-999999999999', 'retrykind', 1, '{"n":6}'::jsonb, 'cfg-r', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'retry-6', 'running', clock_timestamp(), 6, 8, 'worker-retry', clock_timestamp() + interval '5 minutes', 1
),
(
  '99999999-9999-4999-8999-999999999999', 'retrykind', 1, '{"n":7}'::jsonb, 'cfg-r', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'retry-7', 'running', clock_timestamp(), 7, 8, 'worker-retry', clock_timestamp() + interval '5 minutes', 1
),
(
  '99999999-9999-4999-8999-999999999999', 'retrykind', 1, '{"n":0}'::jsonb, 'cfg-r', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'retry-x', 'running', clock_timestamp(), 1, 1, 'worker-retry', clock_timestamp() + interval '5 minutes', 1
);
