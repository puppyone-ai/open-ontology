\ir prelude.sql
SELECT state || '|' || attempt::text || '|' || generation::text || '|' ||
  (SELECT count(*) FROM jobs.task_events WHERE task_id = jobs.tasks.id)::text
FROM jobs.tasks
WHERE dedupe_key = 'dedupe-kill';
