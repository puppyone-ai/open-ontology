\ir prelude.sql
SELECT (jobs.enqueue(
  '11111111-1111-4111-8111-111111111112'::uuid,
  'future', 1, '{"f":true}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-future',
  clock_timestamp() + interval '1 hour', 3
)).status;
SELECT 'future|' || status || '|' || coalesce(state, '')
FROM jobs.claim(
  '11111111-1111-4111-8111-111111111112'::uuid,
  ARRAY['future']::text[],
  'worker-future'
);
SELECT 'stored|' || state
FROM jobs.tasks
WHERE dedupe_key = 'dedupe-future';
SELECT 'empty|' || status
FROM jobs.claim(
  '00000000-0000-4000-8000-000000000099'::uuid,
  ARRAY['missing']::text[],
  'worker-none'
);
