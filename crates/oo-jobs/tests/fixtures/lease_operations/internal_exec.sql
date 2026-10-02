\ir prelude.sql
SELECT jobs._lease_token(
  '11111111-1111-4111-8111-111111111111'::uuid,
  '11111111-1111-4111-8111-111111111111'::uuid,
  'worker-a',
  1,
  clock_timestamp()
);
