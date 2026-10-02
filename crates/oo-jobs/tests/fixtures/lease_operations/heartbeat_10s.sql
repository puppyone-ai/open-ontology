\ir prelude.sql
SELECT (jobs.enqueue(
  'ffffffff-ffff-4fff-8fff-ffffffffffff'::uuid,
  'hbkind', 1, '{"hb":true}', 'cfg-hb', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-hb', clock_timestamp(), 3
)).status AS enq_status
\gset
SELECT (jobs.claim(
  'ffffffff-ffff-4fff-8fff-ffffffffffff'::uuid,
  ARRAY['hbkind']::text[],
  'worker-hb'
)).status AS claim_status
\gset
SELECT extract(epoch FROM lease_until)::text AS old_epoch
FROM jobs.tasks
WHERE project_id = 'ffffffff-ffff-4fff-8fff-ffffffffffff'
  AND dedupe_key = 'dedupe-hb'
\gset
SELECT pg_sleep(10);
SELECT (jobs.heartbeat(
  'ffffffff-ffff-4fff-8fff-ffffffffffff'::uuid,
  (SELECT id FROM jobs.tasks WHERE project_id = 'ffffffff-ffff-4fff-8fff-ffffffffffff' AND dedupe_key = 'dedupe-hb'),
  'worker-hb',
  (SELECT generation FROM jobs.tasks WHERE project_id = 'ffffffff-ffff-4fff-8fff-ffffffffffff' AND dedupe_key = 'dedupe-hb')
)).status AS hb_status
\gset
SELECT :'enq_status' || '|' || :'claim_status' || '|' || :'hb_status' || '|' ||
  (lease_until > to_timestamp(:'old_epoch'::double precision))::text || '|' ||
  round(extract(epoch FROM (lease_until - clock_timestamp())))::text
FROM jobs.tasks
WHERE project_id = 'ffffffff-ffff-4fff-8fff-ffffffffffff'
  AND dedupe_key = 'dedupe-hb';
