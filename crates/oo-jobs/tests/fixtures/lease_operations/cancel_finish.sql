\ir prelude.sql
SELECT 'cx|' || status || '|' || state || '|' || coalesce(invalidated_owner, '') || '|' || coalesce(invalidated_generation::text, '') || '|' || coalesce(invalidated_until::text, '')
FROM jobs.cancel(
  '88888888-8888-4888-8888-888888888888'::uuid,
  (SELECT id FROM jobs.tasks WHERE project_id = '88888888-8888-4888-8888-888888888888' AND dedupe_key = 'dedupe-cancel-r'),
  'worker-c',
  1
);
SELECT 'hb|' || status
FROM jobs.heartbeat(
  '88888888-8888-4888-8888-888888888888'::uuid,
  (SELECT id FROM jobs.tasks WHERE project_id = '88888888-8888-4888-8888-888888888888' AND dedupe_key = 'dedupe-cancel-r'),
  'worker-c',
  1
);
SELECT 'again|' || status
FROM jobs.cancel(
  '88888888-8888-4888-8888-888888888888'::uuid,
  (SELECT id FROM jobs.tasks WHERE project_id = '88888888-8888-4888-8888-888888888888' AND dedupe_key = 'dedupe-cancel-r'),
  'worker-c',
  1
);
