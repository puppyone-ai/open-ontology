\ir prelude.sql
INSERT INTO jobs.tasks (
  project_id, kind, payload_version, payload, config_version, artifact_digest,
  dedupe_key, max_attempts, run_at
) VALUES (
  'abababab-abab-4aba-8aba-abababababab',
  'badkind',
  1,
  '{"ok":true}'::jsonb,
  'cfg-bad',
  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'dedupe-bad',
  3,
  clock_timestamp()
);
INSERT INTO jobs.task_events (project_id, task_id, to_state, generation, attempt)
SELECT
  'cdcdcdcd-cdcd-4cdc-8cdc-cdcdcdcdcdcd',
  id,
  'queued',
  0,
  0
FROM jobs.tasks
WHERE dedupe_key = 'dedupe-bad';
