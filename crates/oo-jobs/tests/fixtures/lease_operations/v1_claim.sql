\ir prelude.sql
SELECT jobs.claim(
  '11111111-1111-4111-8111-111111111111'::uuid,
  ARRAY['alpha']::text[],
  'worker-v1'
);
