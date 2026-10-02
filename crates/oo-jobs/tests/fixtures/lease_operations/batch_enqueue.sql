\ir prelude.sql
DO $$
DECLARE
  i integer;
BEGIN
  FOR i IN 1..100 LOOP
    PERFORM jobs.enqueue(
      '33333333-3333-4333-8333-333333333333'::uuid,
      'batch',
      1,
      '{"n":' || i::text || '}',
      'cfg-batch',
      'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      'batch-' || i::text,
      clock_timestamp(),
      3
    );
  END LOOP;
END
$$;
SELECT count(*)::text
FROM jobs.tasks
WHERE project_id = '33333333-3333-4333-8333-333333333333'
  AND state = 'queued';
