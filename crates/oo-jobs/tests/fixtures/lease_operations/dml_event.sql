\ir prelude.sql
INSERT INTO jobs.task_events (project_id, task_id, to_state, generation, attempt)
SELECT project_id, id, state, generation, attempt
FROM jobs.tasks
WHERE dedupe_key = 'dedupe-alpha'
  AND project_id = '11111111-1111-4111-8111-111111111111';
