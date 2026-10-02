\ir prelude.sql
INSERT INTO jobs.tasks (
  project_id, kind, payload_version, payload, config_version, artifact_digest,
  dedupe_key, max_attempts, run_at
) VALUES (
  'cccccccc-cccc-4ccc-8ccc-cccccccccccc',
  'legal',
  1,
  '{"ok":true}'::jsonb,
  'cfg-legal',
  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'dedupe-legal',
  3,
  clock_timestamp()
);
INSERT INTO jobs.task_events (project_id, task_id, to_state, generation, attempt)
SELECT project_id, id, 'queued', 0, 0
FROM jobs.tasks
WHERE dedupe_key = 'dedupe-legal';
