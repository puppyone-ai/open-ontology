\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
UPDATE jobs.task_events SET note = 'rewritten' WHERE task_id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1';
