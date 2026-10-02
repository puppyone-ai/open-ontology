\ir prelude.sql
SELECT 'q|' || status || '|' || generation::text
FROM jobs.enqueue(
  '88888888-8888-4888-8888-888888888888'::uuid, 'cancelq', 1, '{"q":true}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'dedupe-cancel-q', clock_timestamp(), 3
);
SELECT 'qc|' || status || '|' || state || '|' || coalesce(invalidated_owner, '') || '|' || coalesce(invalidated_generation::text, '')
FROM jobs.cancel(
  '88888888-8888-4888-8888-888888888888'::uuid,
  (SELECT id FROM jobs.tasks WHERE project_id = '88888888-8888-4888-8888-888888888888' AND dedupe_key = 'dedupe-cancel-q'),
  'worker-z',
  0
);
SELECT 'qr|' || status
FROM jobs.cancel(
  '88888888-8888-4888-8888-888888888888'::uuid,
  (SELECT id FROM jobs.tasks WHERE project_id = '88888888-8888-4888-8888-888888888888' AND dedupe_key = 'dedupe-cancel-q'),
  'worker-z',
  0
);
SELECT 'enq|' || status
FROM jobs.enqueue(
  '88888888-8888-4888-8888-888888888888'::uuid, 'cancelr', 1, '{"r":true}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'dedupe-cancel-r', clock_timestamp(), 3
);
SELECT 'run|' || status || '|' || generation::text || '|' || lease_owner
FROM jobs.claim('88888888-8888-4888-8888-888888888888'::uuid, ARRAY['cancelr']::text[], 'worker-c');
