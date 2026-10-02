\ir prelude.sql
SELECT 'first|' || status || '|' || coalesce(task_id::text, '')
FROM jobs.claim('66666666-6666-4666-8666-666666666666'::uuid, ARRAY['capkind']::text[], 'worker-cap');
SELECT 'counts|' ||
  count(*) FILTER (WHERE state = 'failed' AND last_error = 'lease_expired_attempt_cap')::text
  || '|' || count(*) FILTER (WHERE state = 'queued')::text
FROM jobs.tasks
WHERE project_id = '66666666-6666-4666-8666-666666666666';
SELECT 'second|' || status || '|' || coalesce(state, '') || '|' || coalesce(attempt::text, '')
  || '|' || coalesce(generation::text, '')
FROM jobs.claim('66666666-6666-4666-8666-666666666666'::uuid, ARRAY['capkind']::text[], 'worker-cap');
SELECT 'resurrect|' || status
FROM jobs.complete_success(
  '66666666-6666-4666-8666-666666666666'::uuid,
  (SELECT id FROM jobs.tasks WHERE project_id = '66666666-6666-4666-8666-666666666666' AND dedupe_key = 'cap-1'),
  'old-worker',
  1
);
