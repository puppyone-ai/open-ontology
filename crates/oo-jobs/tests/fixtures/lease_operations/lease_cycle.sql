\ir prelude.sql
SELECT 'enq|' || status || '|' || state || '|' || attempt::text || '|' || generation::text
FROM jobs.enqueue(
  '55555555-5555-4555-8555-555555555555'::uuid, 'lease', 1, '{"lease":true}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'dedupe-lease', clock_timestamp(), 3
);
SELECT 'claim|' || status || '|' || state || '|' || attempt::text || '|' || generation::text
  || '|' || lease_owner || '|' || round(extract(epoch FROM (lease_until - clock_timestamp())))::text
  || '|' || left(token, 5)
FROM jobs.claim('55555555-5555-4555-8555-555555555555'::uuid, ARRAY['lease']::text[], 'worker-a');
SELECT 'stale|' || status || '|' || coalesce(generation::text, '')
FROM jobs.heartbeat(
  '55555555-5555-4555-8555-555555555555'::uuid,
  (SELECT id FROM jobs.tasks WHERE project_id = '55555555-5555-4555-8555-555555555555' AND dedupe_key = 'dedupe-lease'),
  'worker-other',
  (SELECT generation FROM jobs.tasks WHERE project_id = '55555555-5555-4555-8555-555555555555' AND dedupe_key = 'dedupe-lease')
);
SELECT 'beat|' || status || '|' || round(extract(epoch FROM (lease_until - clock_timestamp())))::text
  || '|' || left(coalesce(token, ''), 5)
FROM jobs.heartbeat(
  '55555555-5555-4555-8555-555555555555'::uuid,
  (SELECT id FROM jobs.tasks WHERE project_id = '55555555-5555-4555-8555-555555555555' AND dedupe_key = 'dedupe-lease'),
  'worker-a',
  (SELECT generation FROM jobs.tasks WHERE project_id = '55555555-5555-4555-8555-555555555555' AND dedupe_key = 'dedupe-lease')
);
SELECT 'events|' || count(*)::text
FROM jobs.task_events
WHERE task_id = (SELECT id FROM jobs.tasks WHERE project_id = '55555555-5555-4555-8555-555555555555' AND dedupe_key = 'dedupe-lease');
SELECT 'oldgen|' || status
FROM jobs.complete_success(
  '55555555-5555-4555-8555-555555555555'::uuid,
  (SELECT id FROM jobs.tasks WHERE project_id = '55555555-5555-4555-8555-555555555555' AND dedupe_key = 'dedupe-lease'),
  'worker-a',
  2
);
SELECT 'done|' || status || '|' || state || '|' || coalesce(lease_owner, '')
FROM jobs.complete_success(
  '55555555-5555-4555-8555-555555555555'::uuid,
  (SELECT id FROM jobs.tasks WHERE project_id = '55555555-5555-4555-8555-555555555555' AND dedupe_key = 'dedupe-lease'),
  'worker-a',
  (SELECT generation FROM jobs.tasks WHERE project_id = '55555555-5555-4555-8555-555555555555' AND dedupe_key = 'dedupe-lease')
);
SELECT 'again|' || status
FROM jobs.complete_success(
  '55555555-5555-4555-8555-555555555555'::uuid,
  (SELECT id FROM jobs.tasks WHERE project_id = '55555555-5555-4555-8555-555555555555' AND dedupe_key = 'dedupe-lease'),
  'worker-a',
  1
);
SELECT 'anygen|' || status
FROM jobs.complete_success(
  '55555555-5555-4555-8555-555555555555'::uuid,
  (SELECT id FROM jobs.tasks WHERE project_id = '55555555-5555-4555-8555-555555555555' AND dedupe_key = 'dedupe-lease'),
  'worker-a',
  99
);
