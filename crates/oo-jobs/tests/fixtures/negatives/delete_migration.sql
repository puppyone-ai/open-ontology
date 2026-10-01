\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
DELETE FROM jobs.schema_migrations WHERE version = 'jobs-control/1';
