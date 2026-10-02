\ir prelude.sql
SELECT status || '|' || state
FROM jobs.enqueue(
  '11111111-1111-4111-8111-111111111114'::uuid,
  'liekind', 1, '{"lie":true}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'dedupe-lie',
  clock_timestamp(), 3
);
