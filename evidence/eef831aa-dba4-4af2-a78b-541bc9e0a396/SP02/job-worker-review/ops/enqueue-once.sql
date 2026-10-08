\pset pager off
\echo === review-once ===
SELECT status, state
FROM jobs.enqueue(
  'eef831aa-dba4-4af2-a78b-541bc9e0a390'::uuid,
  'pg_effect',
  1,
  '{"v":1,"units":1,"fail_attempts":0}',
  'jw-payload-1',
  '8489c431a95a59d2e2a18a8d252deb72996cfeea10723180e710bb37ed84b547',
  'review-once',
  clock_timestamp(),
  3
);
\echo === review-cancel ===
SELECT status
FROM jobs.enqueue(
  'eef831aa-dba4-4af2-a78b-541bc9e0a390'::uuid,
  'review_probe',
  1,
  '{"v":1,"units":1,"fail_attempts":0}',
  'jw-payload-1',
  '8489c431a95a59d2e2a18a8d252deb72996cfeea10723180e710bb37ed84b547',
  'review-cancel',
  clock_timestamp() + interval '1 hour',
  3
);
SELECT c.status AS cancel_status
FROM jobs.tasks t
CROSS JOIN LATERAL jobs.cancel(
  t.project_id, t.id, 'review-canceller', t.generation
) c
WHERE t.project_id = 'eef831aa-dba4-4af2-a78b-541bc9e0a390'::uuid
  AND t.dedupe_key = 'review-cancel';
SELECT state AS cancel_state
FROM jobs.tasks
WHERE project_id = 'eef831aa-dba4-4af2-a78b-541bc9e0a390'::uuid
  AND dedupe_key = 'review-cancel';
SELECT count(*)::bigint AS cancel_effects
FROM jw.effects e
JOIN jobs.tasks t ON t.id = e.task_id
WHERE t.dedupe_key = 'review-cancel';
