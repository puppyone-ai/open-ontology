\pset pager off
SELECT status AS enqueued
FROM jobs.enqueue(
  'eef831aa-dba4-4af2-a78b-541bc9e0a390'::uuid,
  'review_probe',
  1,
  '{"v":1,"units":1,"fail_attempts":0}',
  'jw-payload-1',
  '8489c431a95a59d2e2a18a8d252deb72996cfeea10723180e710bb37ed84b547',
  'review-retry-a',
  clock_timestamp(),
  3
);
SELECT status AS enqueued
FROM jobs.enqueue(
  'eef831aa-dba4-4af2-a78b-541bc9e0a390'::uuid,
  'review_probe',
  1,
  '{"v":1,"units":1,"fail_attempts":0}',
  'jw-payload-1',
  '8489c431a95a59d2e2a18a8d252deb72996cfeea10723180e710bb37ed84b547',
  'review-retry-b',
  clock_timestamp(),
  3
);
WITH claimed AS (
  SELECT *
  FROM jobs.claim(
    'eef831aa-dba4-4af2-a78b-541bc9e0a390'::uuid,
    ARRAY['review_probe']::text[],
    'review-worker-a'
  )
),
retried AS (
  SELECT r.status, r.attempt, r.run_at, c.task_id
  FROM claimed c
  CROSS JOIN LATERAL jobs.retry_failure(
    c.project_id, c.task_id, c.lease_owner, c.generation, 'worker_failed'
  ) r
  WHERE c.status = 'claimed'
)
SELECT t.dedupe_key, retried.status, retried.attempt,
       round(extract(epoch FROM (retried.run_at - clock_timestamp()))::numeric, 6) AS remain_s
FROM retried
JOIN jobs.tasks t ON t.id = retried.task_id;
WITH claimed AS (
  SELECT *
  FROM jobs.claim(
    'eef831aa-dba4-4af2-a78b-541bc9e0a390'::uuid,
    ARRAY['review_probe']::text[],
    'review-worker-b'
  )
),
retried AS (
  SELECT r.status, r.attempt, r.run_at, c.task_id
  FROM claimed c
  CROSS JOIN LATERAL jobs.retry_failure(
    c.project_id, c.task_id, c.lease_owner, c.generation, 'worker_failed'
  ) r
  WHERE c.status = 'claimed'
)
SELECT t.dedupe_key, retried.status, retried.attempt,
       round(extract(epoch FROM (retried.run_at - clock_timestamp()))::numeric, 6) AS remain_s
FROM retried
JOIN jobs.tasks t ON t.id = retried.task_id;
