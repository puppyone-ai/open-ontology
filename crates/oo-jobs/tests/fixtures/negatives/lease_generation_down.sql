\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
UPDATE jobs.leases
SET generation = 0
WHERE id = 'dddddddd-dddd-4ddd-8ddd-ddddddddddd1';
