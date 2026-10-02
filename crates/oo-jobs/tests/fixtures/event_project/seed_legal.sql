\set ON_ERROR_STOP on
INSERT INTO jobs.tasks (
  id, project_id, kind, payload_version, payload, config_version, artifact_digest,
  dedupe_key, max_attempts, run_at
) VALUES
  (
    'a0750001-0001-4000-8000-000000000001',
    '11111111-1111-4111-8111-111111111111',
    'snapshot_block', 1, '{"v":1}'::jsonb, 'cfg-1', repeat('a', 64),
    'keep-me', 3, '2026-10-02T00:00:00Z'
  ),
  (
    'a0750001-0001-4000-8000-000000000002',
    '22222222-2222-4222-8222-222222222222',
    'snapshot_block', 1, '{"v":2}'::jsonb, 'cfg-2', repeat('b', 64),
    'keep-me', 3, '2026-10-02T00:00:00Z'
  );

UPDATE jobs.tasks
SET state = 'running',
    attempt = attempt + 1,
    generation = generation + 1,
    lease_owner = 'worker-a',
    lease_until = '2026-10-02T00:00:30Z',
    updated_at = clock_timestamp()
WHERE id = 'a0750001-0001-4000-8000-000000000001'
  AND state = 'queued'
  AND generation = 0
  AND lease_owner IS NULL;

UPDATE jobs.tasks
SET state = 'succeeded',
    lease_owner = NULL,
    lease_until = NULL,
    updated_at = clock_timestamp()
WHERE id = 'a0750001-0001-4000-8000-000000000001'
  AND state = 'running'
  AND generation = 1
  AND lease_owner = 'worker-a';

INSERT INTO jobs.task_events (
  project_id, task_id, from_state, to_state, generation, attempt, lease_owner, note
) VALUES (
  '11111111-1111-4111-8111-111111111111',
  'a0750001-0001-4000-8000-000000000001',
  'succeeded',
  'succeeded',
  1,
  1,
  NULL,
  'same-project direct'
);

INSERT INTO jobs.schedules (
  id, project_id, kind, config_version, artifact_digest, dedupe_prefix, next_run_at
) VALUES (
  'a0750001-0001-4000-8000-000000000051',
  '11111111-1111-4111-8111-111111111111',
  'snapshot_block',
  'cfg-1',
  repeat('a', 64),
  'nightly',
  '2026-10-03T00:00:00Z'
);

INSERT INTO jobs.schedule_occurrences (schedule_id, scheduled_at, project_id, task_id)
VALUES (
  'a0750001-0001-4000-8000-000000000051',
  '2026-10-02T00:00:00Z',
  '11111111-1111-4111-8111-111111111111',
  'a0750001-0001-4000-8000-000000000001'
);

INSERT INTO jobs.leases (id, project_id, target_key)
VALUES (
  'a0750001-0001-4000-8000-0000000000b1',
  '11111111-1111-4111-8111-111111111111',
  'block-1'
);
