\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
UPDATE jobs.tasks
SET state = 'succeeded',
    attempt = 1,
    generation = 1
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa8';
