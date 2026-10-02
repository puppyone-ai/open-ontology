\set ON_ERROR_STOP on
INSERT INTO jobs.tasks (
  id, project_id, kind, payload_version, payload, config_version, artifact_digest,
  dedupe_key, max_attempts, run_at
) VALUES
  (
    'a0750001-0001-4000-8000-000000000011',
    '11111111-1111-4111-8111-111111111111',
    'snapshot_block', 1, '{"v":1}'::jsonb, 'cfg-1', repeat('a', 64),
    'behavior-transition', 3, '2026-10-02T00:00:00Z'
  ),
  (
    'a0750001-0001-4000-8000-000000000012',
    '11111111-1111-4111-8111-111111111111',
    'snapshot_block', 1, '{"v":1}'::jsonb, 'cfg-1', repeat('a', 64),
    'behavior-direct', 3, '2026-10-02T00:00:00Z'
  ),
  (
    'a0750001-0001-4000-8000-000000000013',
    '22222222-2222-4222-8222-222222222222',
    'snapshot_block', 1, '{"v":2}'::jsonb, 'cfg-2', repeat('b', 64),
    'behavior-other', 3, '2026-10-02T00:00:00Z'
  ),
  (
    'a0750001-0001-4000-8000-000000000014',
    '11111111-1111-4111-8111-111111111111',
    'snapshot_block', 1, '{"v":1}'::jsonb, 'cfg-1', repeat('a', 64),
    'behavior-heartbeat', 3, '2026-10-02T00:00:00Z'
  );

UPDATE jobs.tasks
SET state = 'running',
    attempt = attempt + 1,
    generation = generation + 1,
    lease_owner = 'worker-a',
    lease_until = clock_timestamp() + interval '30 seconds',
    updated_at = clock_timestamp()
WHERE id = 'a0750001-0001-4000-8000-000000000014'
  AND state = 'queued'
  AND generation = 0;
