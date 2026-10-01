\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
UPDATE jobs.tasks
SET artifact_digest = repeat('b', 64)
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa8';
