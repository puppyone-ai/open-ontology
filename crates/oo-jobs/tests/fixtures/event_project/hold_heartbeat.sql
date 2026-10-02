\set ON_ERROR_STOP on
BEGIN;
SET LOCAL lock_timeout = '5s';
UPDATE jobs.tasks
SET lease_until = clock_timestamp() + interval '10 seconds',
    updated_at = clock_timestamp()
WHERE id = 'a0750001-0001-4000-8000-000000000014'
  AND state = 'running'
  AND lease_owner = 'worker-a'
  AND generation = 1;
SELECT pg_sleep(3);
COMMIT;
