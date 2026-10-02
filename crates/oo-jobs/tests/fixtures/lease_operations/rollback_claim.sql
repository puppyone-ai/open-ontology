\ir prelude.sql
BEGIN;
SELECT jobs.claim(
  '12121212-1212-4121-8121-121212121213'::uuid,
  ARRAY['rollc']::text[],
  'worker-roll'
);
ROLLBACK;
