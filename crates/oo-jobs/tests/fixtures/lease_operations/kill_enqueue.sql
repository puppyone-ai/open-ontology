\ir prelude.sql
SELECT (jobs.enqueue(
  '44444444-4444-4444-8444-444444444444'::uuid,
  'killme', 1, '{"k":true}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-kill',
  clock_timestamp(), 3
)).status;
