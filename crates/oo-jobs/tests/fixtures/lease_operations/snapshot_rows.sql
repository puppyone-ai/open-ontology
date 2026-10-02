\ir prelude.sql
SET DateStyle = 'ISO';
SET TimeZone = 'UTC';
SELECT 'T|' || id::text || '|' || project_id::text || '|' || kind || '|' || state
  || '|' || attempt::text || '|' || generation::text
  || '|' || coalesce(lease_owner, '')
  || '|' || coalesce(lease_until::text, '')
  || '|' || coalesce(last_error, '')
  || '|' || payload::text || '|' || dedupe_key
  || '|' || run_at::text || '|' || created_at::text || '|' || updated_at::text
FROM jobs.tasks
ORDER BY id;
SELECT 'E|' || event_id::text || '|' || task_id::text || '|' || project_id::text
  || '|' || coalesce(from_state, '') || '|' || to_state
  || '|' || generation::text || '|' || attempt::text
  || '|' || coalesce(lease_owner, '')
  || '|' || coalesce(note, '')
  || '|' || occurred_at::text
FROM jobs.task_events
ORDER BY event_id;
