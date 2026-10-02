\ir prelude.sql
SELECT (jobs.enqueue(
  '77777777-7777-4777-8777-777777777777'::uuid,
  'overflow', 1, '{"over":true}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-over',
  clock_timestamp(), 3
)).status;
