\ir prelude.sql
SELECT (jobs.enqueue(
  '55555555-5555-4555-8555-555555555551'::uuid,
  'reclaim', 1, '{"reclaim":true}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-reclaim', clock_timestamp(), 4
)).status;
SELECT 'claim|' || status || '|' || attempt::text || '|' || generation::text || '|' || lease_owner || '|' || replace(coalesce(token, ''), E'\n', '')
FROM jobs.claim(
  '55555555-5555-4555-8555-555555555551'::uuid,
  ARRAY['reclaim']::text[],
  'worker-same'
);
