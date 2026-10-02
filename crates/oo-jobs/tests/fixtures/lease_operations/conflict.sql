\ir prelude.sql
SELECT jobs.enqueue(
  '11111111-1111-4111-8111-111111111111'::uuid,
  'alpha', 1, '{"a":3}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-alpha',
  clock_timestamp(), 3
);
