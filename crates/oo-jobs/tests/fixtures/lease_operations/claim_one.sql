\ir prelude.sql
SELECT status || '|' || coalesce(task_id::text, '') || '|' || coalesce(generation::text, '')
  || '|' || coalesce(lease_owner, '')
FROM jobs.claim(
  '33333333-3333-4333-8333-333333333333'::uuid,
  ARRAY['batch']::text[],
  :'worker_id'
);
