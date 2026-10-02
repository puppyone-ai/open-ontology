\ir prelude.sql
SELECT jobs.claim(
  '77777777-7777-4777-8777-777777777777'::uuid,
  ARRAY['overflow']::text[],
  'worker-over'
);
