\ir prelude.sql
SELECT 'reclaim|' || status || '|' || attempt::text || '|' || generation::text || '|' || lease_owner || '|' || replace(coalesce(token, ''), E'\n', '')
FROM jobs.claim(
  '55555555-5555-4555-8555-555555555551'::uuid,
  ARRAY['reclaim']::text[],
  'worker-same'
);
SELECT 'old|' || status
FROM jobs.complete_success(
  '55555555-5555-4555-8555-555555555551'::uuid,
  (SELECT id FROM jobs.tasks WHERE dedupe_key = 'dedupe-reclaim'),
  'worker-same',
  1
);
SELECT 'oldbeat|' || status
FROM jobs.heartbeat(
  '55555555-5555-4555-8555-555555555551'::uuid,
  (SELECT id FROM jobs.tasks WHERE dedupe_key = 'dedupe-reclaim'),
  'worker-same',
  1
);
SELECT 'new|' || status || '|' || state || '|' || coalesce(lease_owner, '')
FROM jobs.complete_success(
  '55555555-5555-4555-8555-555555555551'::uuid,
  (SELECT id FROM jobs.tasks WHERE dedupe_key = 'dedupe-reclaim'),
  'worker-same',
  (SELECT generation FROM jobs.tasks WHERE dedupe_key = 'dedupe-reclaim')
);
