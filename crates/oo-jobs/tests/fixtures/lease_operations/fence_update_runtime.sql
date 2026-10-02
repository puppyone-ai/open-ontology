\ir prelude.sql
UPDATE jobs.leases
SET target_fence_certified = true
WHERE target_key = 'fence-key';
