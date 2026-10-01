\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
UPDATE jobs.tasks
SET state = 'queued',
    last_error = NULL
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3';
