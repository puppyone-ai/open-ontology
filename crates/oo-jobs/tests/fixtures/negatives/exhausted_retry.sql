\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
UPDATE jobs.tasks
SET state = 'queued',
    lease_owner = NULL,
    lease_until = NULL
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa9';
