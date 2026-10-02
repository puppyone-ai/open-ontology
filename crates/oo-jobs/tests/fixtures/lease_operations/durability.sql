\ir prelude.sql
SELECT project_id::text || '|' || dedupe_key || '|' || state || '|' || attempt::text
  || '|' || generation::text || '|' || coalesce(lease_owner, '')
FROM jobs.tasks
WHERE project_id IN (
  '33333333-3333-4333-8333-333333333333',
  '11111111-1111-4111-8111-111111111111'
)
ORDER BY project_id, dedupe_key;
