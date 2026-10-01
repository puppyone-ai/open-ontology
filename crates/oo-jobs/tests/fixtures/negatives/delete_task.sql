\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
DELETE FROM jobs.tasks WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa8';
