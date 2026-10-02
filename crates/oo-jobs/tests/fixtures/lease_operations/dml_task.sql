\ir prelude.sql
UPDATE jobs.tasks
SET state = 'succeeded'
WHERE dedupe_key = 'dedupe-alpha'
  AND project_id = '11111111-1111-4111-8111-111111111111';
