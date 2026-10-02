\ir prelude.sql
SELECT 'first|' || status || '|' || task_id::text || '|' || state || '|' || run_at::text
FROM jobs.enqueue(
  '11111111-1111-4111-8111-111111111111'::uuid,
  'alpha', 1, '{"b":1,"a":2}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-alpha',
  clock_timestamp(), 3
);
SELECT 'replay|' || status || '|' || task_id::text || '|' || state || '|' || run_at::text
FROM jobs.enqueue(
  '11111111-1111-4111-8111-111111111111'::uuid,
  'alpha', 1, '{"a":2,"b":1}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-alpha',
  clock_timestamp() + interval '2 days', 3
);
SELECT 'cross|' || status || '|' || task_id::text || '|' || state
FROM jobs.enqueue(
  '22222222-2222-4222-8222-222222222222'::uuid,
  'alpha', 1, '{"b":1,"a":2}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-alpha',
  clock_timestamp(), 3
);
