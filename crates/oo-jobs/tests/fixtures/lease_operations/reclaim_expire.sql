\ir prelude.sql
SET ROLE oo_jobs_owner;
UPDATE jobs.tasks
SET lease_until = clock_timestamp() - interval '1 second'
WHERE project_id = '55555555-5555-4555-8555-555555555551'
  AND dedupe_key = 'dedupe-reclaim'
  AND state = 'running'
  AND lease_owner = 'worker-same';
