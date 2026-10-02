\ir prelude.sql
SELECT (jobs.enqueue(
  'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'::uuid,
  'dead', 1, '{"d":true}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-dead',
  clock_timestamp(), 3
)).status;
