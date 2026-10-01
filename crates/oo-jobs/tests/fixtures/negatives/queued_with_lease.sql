\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
UPDATE jobs.tasks
SET lease_owner = 'worker-a',
    lease_until = clock_timestamp() + interval '30 seconds'
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa8';
