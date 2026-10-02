\ir prelude.sql
SELECT (jobs.enqueue(
  '11111111-1111-4111-8111-111111111113'::uuid,
  'failkind', 1, '{"fail":true}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-fail',
  clock_timestamp(), 3
)).status;
SELECT 'claim|' || status || '|' || generation::text
FROM jobs.claim(
  '11111111-1111-4111-8111-111111111113'::uuid,
  ARRAY['failkind']::text[],
  'worker-f'
);
