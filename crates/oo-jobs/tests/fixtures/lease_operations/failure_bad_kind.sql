\ir prelude.sql
SELECT jobs.complete_failure(
  '11111111-1111-4111-8111-111111111113'::uuid,
  (SELECT id FROM jobs.tasks WHERE dedupe_key = 'dedupe-fail'),
  'worker-f',
  1,
  'canary-a102-secret-do-not-echo'
);
