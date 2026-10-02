\set ON_ERROR_STOP on
INSERT INTO jobs.tasks (
  id, project_id, kind, payload_version, payload, config_version, artifact_digest,
  dedupe_key, max_attempts, run_at
) VALUES (
  'a0750001-0001-4000-8000-0000000000d1',
  '11111111-1111-4111-8111-111111111111',
  'snapshot_block', 1, '{"v":1}'::jsonb, 'cfg-1', repeat('a', 64),
  'bad-history', 3, '2026-10-02T00:00:00Z'
);

INSERT INTO jobs.task_events (
  project_id, task_id, to_state, generation, attempt, note
) VALUES (
  '22222222-2222-4222-8222-222222222222',
  'a0750001-0001-4000-8000-0000000000d1',
  'queued',
  0,
  0,
  'cross-project preexisting'
);
