\ir prelude.sql
SELECT count(*)::text || '|' || count(DISTINCT id)::text || '|' || count(DISTINCT lease_owner)::text
  || '|' || count(*) FILTER (WHERE state = 'running')::text
  || '|' || count(*) FILTER (WHERE attempt = 1)::text
  || '|' || count(*) FILTER (WHERE generation = 1)::text
FROM jobs.tasks
WHERE project_id = '33333333-3333-4333-8333-333333333333';
