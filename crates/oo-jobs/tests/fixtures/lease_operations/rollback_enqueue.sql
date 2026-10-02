\ir prelude.sql
BEGIN;
SELECT jobs.enqueue(
  '12121212-1212-4121-8121-121212121212'::uuid,
  'roll', 1, '{"roll":true}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-rollback',
  clock_timestamp(), 3
);
SELECT 1/0;
COMMIT;
