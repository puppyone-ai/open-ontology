\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
UPDATE jobs.schema_migrations
SET checksum = repeat('e', 64)
WHERE version = 'jobs-control/1';
