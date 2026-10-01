\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
SELECT id FROM jobs.migration_lock WHERE id = 1 FOR UPDATE;
