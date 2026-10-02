\ir prelude.sql
INSERT INTO jobs.schedules (
  project_id, kind, config_version, artifact_digest, dedupe_prefix, next_run_at
) VALUES (
  '11111111-1111-4111-8111-111111111111',
  'sched',
  'cfg-1',
  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'sched-prefix',
  clock_timestamp()
)
RETURNING 'scheduled|' || id::text;
