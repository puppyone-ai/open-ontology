\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
UPDATE jobs.tasks
SET lease_owner = 'worker-b'
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1'
  AND state = 'running';
