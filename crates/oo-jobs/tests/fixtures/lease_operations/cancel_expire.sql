\ir prelude.sql
SET ROLE oo_jobs_owner;
UPDATE jobs.tasks
SET lease_until = clock_timestamp() - interval '1 second'
WHERE project_id = '88888888-8888-4888-8888-888888888888'
  AND dedupe_key = 'dedupe-cancel-r'
  AND state = 'running';
