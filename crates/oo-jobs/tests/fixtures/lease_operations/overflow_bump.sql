\ir prelude.sql
SET ROLE oo_jobs_owner;
UPDATE jobs.tasks
SET generation = 9223372036854775807
WHERE project_id = '77777777-7777-4777-8777-777777777777'
  AND dedupe_key = 'dedupe-over'
  AND state = 'queued';
