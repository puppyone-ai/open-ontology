\ir prelude.sql
SET ROLE oo_jobs_owner;
UPDATE jobs.leases
SET target_fence_certified = true
WHERE target_key = 'fence-key';
