\ir prelude.sql
SELECT status || '|' || coalesce(task_id::text, '')
FROM jobs.claim(
  'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'::uuid,
  ARRAY['dead']::text[],
  'worker-dead'
);
