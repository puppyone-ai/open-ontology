\ir prelude.sql
SELECT target_fence_certified::text
FROM jobs.leases
WHERE target_key = 'fence-key';
