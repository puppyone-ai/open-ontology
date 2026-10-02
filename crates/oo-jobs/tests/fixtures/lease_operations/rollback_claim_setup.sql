\ir prelude.sql
SELECT (jobs.enqueue(
  '12121212-1212-4121-8121-121212121213'::uuid,
  'rollc', 1, '{"rollc":true}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-rollc',
  clock_timestamp(), 3
)).status;
SELECT 'before|' || state || '|' || attempt::text || '|' || generation::text || '|' ||
  (SELECT count(*) FROM jobs.task_events WHERE task_id = jobs.tasks.id)::text
FROM jobs.tasks
WHERE dedupe_key = 'dedupe-rollc';
