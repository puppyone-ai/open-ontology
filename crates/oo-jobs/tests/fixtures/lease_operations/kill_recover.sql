\ir prelude.sql
SELECT status || '|' || attempt::text || '|' || generation::text || '|' || lease_owner
FROM jobs.claim(
  '44444444-4444-4444-8444-444444444444'::uuid,
  ARRAY['killme']::text[],
  'worker-hold'
);
