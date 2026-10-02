\ir prelude.sql
SELECT 'miss|' || status || '|' || state
FROM jobs.retry_failure(
  '99999999-9999-4999-8999-999999999999'::uuid,
  (SELECT id FROM jobs.tasks WHERE dedupe_key = 'retry-1'),
  'worker-retry',
  2,
  'worker_failed'
);
SELECT (jobs.retry_failure(
  '99999999-9999-4999-8999-999999999999'::uuid,
  id, 'worker-retry', generation, 'worker_failed'
)).status AS ignored
FROM jobs.tasks WHERE dedupe_key = 'retry-1';
SELECT 'd1|' || state || '|' || round(extract(epoch FROM (run_at - clock_timestamp())))::text
  || '|' || coalesce(last_error, '') || '|' || attempt::text
FROM jobs.tasks WHERE dedupe_key = 'retry-1';
SELECT (jobs.retry_failure(
  '99999999-9999-4999-8999-999999999999'::uuid,
  id, 'worker-retry', generation, 'dependency_unavailable'
)).status
FROM jobs.tasks WHERE dedupe_key = 'retry-6';
SELECT 'd6|' || state || '|' || round(extract(epoch FROM (run_at - clock_timestamp())))::text
FROM jobs.tasks WHERE dedupe_key = 'retry-6';
SELECT (jobs.retry_failure(
  '99999999-9999-4999-8999-999999999999'::uuid,
  id, 'worker-retry', generation, 'external_timeout'
)).status
FROM jobs.tasks WHERE dedupe_key = 'retry-7';
SELECT 'd7|' || state || '|' || round(extract(epoch FROM (run_at - clock_timestamp())))::text
FROM jobs.tasks WHERE dedupe_key = 'retry-7';
SELECT (jobs.retry_failure(
  '99999999-9999-4999-8999-999999999999'::uuid,
  id, 'worker-retry', generation, 'payload_rejected'
)).status AS ex_status
FROM jobs.tasks WHERE dedupe_key = 'retry-x';
SELECT 'ex|' || state || '|' || coalesce(last_error, '')
FROM jobs.tasks WHERE dedupe_key = 'retry-x';
