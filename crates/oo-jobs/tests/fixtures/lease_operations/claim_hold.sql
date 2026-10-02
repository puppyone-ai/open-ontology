\ir prelude.sql
SET application_name = 'a102-claim-hold';
BEGIN;
SELECT status || '|' || coalesce(task_id::text, '')
FROM jobs.claim(
  '44444444-4444-4444-8444-444444444444'::uuid,
  ARRAY['killme']::text[],
  'worker-hold'
);
SELECT pg_sleep(45);
COMMIT;
