\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
UPDATE jobs.tasks
SET last_error = repeat('e', 2049)
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3';
