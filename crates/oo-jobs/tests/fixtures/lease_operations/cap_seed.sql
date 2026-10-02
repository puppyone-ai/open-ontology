\ir prelude.sql
SET ROLE oo_jobs_owner;
INSERT INTO jobs.tasks (
  project_id, kind, payload_version, payload, config_version, artifact_digest,
  dedupe_key, state, run_at, attempt, max_attempts, lease_owner, lease_until, generation
)
SELECT
  '66666666-6666-4666-8666-666666666666'::uuid,
  'capkind',
  1,
  jsonb_build_object('n', g),
  'cfg-cap',
  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'cap-' || g::text,
  'running',
  clock_timestamp() - interval '1 hour',
  1,
  1,
  'old-worker',
  clock_timestamp() - interval '1 minute',
  1
FROM generate_series(1, 8) AS g;
INSERT INTO jobs.tasks (
  project_id, kind, payload_version, payload, config_version, artifact_digest,
  dedupe_key, state, run_at, max_attempts
) VALUES (
  '66666666-6666-4666-8666-666666666666'::uuid,
  'capkind',
  1,
  '{"later":true}'::jsonb,
  'cfg-cap',
  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'cap-later',
  'queued',
  clock_timestamp(),
  3
);
