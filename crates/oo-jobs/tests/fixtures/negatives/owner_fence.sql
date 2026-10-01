\set ON_ERROR_STOP on
SET ROLE oo_jobs_owner;
UPDATE jobs.leases
SET target_fence_certified = true
WHERE id = 'dddddddd-dddd-4ddd-8ddd-ddddddddddd1';
