\ir prelude.sql
SELECT 'fail|' || status || '|' || state
FROM jobs.complete_failure(
  '11111111-1111-4111-8111-111111111113'::uuid,
  (SELECT id FROM jobs.tasks WHERE dedupe_key = 'dedupe-fail'),
  'worker-f',
  1,
  'payload_rejected'
);
SELECT 'stored|' || last_error
FROM jobs.tasks WHERE dedupe_key = 'dedupe-fail';
SELECT 'again|' || status
FROM jobs.complete_failure(
  '11111111-1111-4111-8111-111111111113'::uuid,
  (SELECT id FROM jobs.tasks WHERE dedupe_key = 'dedupe-fail'),
  'worker-f',
  1,
  'payload_rejected'
);
