\ir prelude.sql
SELECT (jobs.enqueue(
  'eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee'::uuid,
  'poison', 1, '{"p":true}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-poison',
  clock_timestamp(), 3
)).status;
SELECT 'gap|' || status || '|' || round(extract(epoch FROM (lease_until - pg_catalog.clock_timestamp())))::text
FROM jobs.claim(
  'eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee'::uuid,
  ARRAY['poison']::text[],
  'worker-poison'
);
