\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
SET search_path = jobs, pg_catalog;

INSERT INTO jobs.tasks (
  id, project_id, kind, payload_version, payload, config_version, artifact_digest, dedupe_key, max_attempts, run_at
) VALUES
  ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1', '11111111-1111-4111-8111-111111111111', 'snapshot_block', 1, '{"v":1}'::jsonb, 'cfg-1', repeat('a', 64), 'claim-me', 3, clock_timestamp()),
  ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2', '11111111-1111-4111-8111-111111111111', 'snapshot_block', 1, '{"v":1}'::jsonb, 'cfg-1', repeat('a', 64), 'cancel-me', 3, clock_timestamp()),
  ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3', '11111111-1111-4111-8111-111111111111', 'snapshot_block', 1, '{"v":1}'::jsonb, 'cfg-1', repeat('a', 64), 'fail-me', 1, clock_timestamp()),
  ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa4', '11111111-1111-4111-8111-111111111111', 'snapshot_block', 1, '{"v":1}'::jsonb, 'cfg-1', repeat('a', 64), 'retry-me', 3, clock_timestamp()),
  ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa5', '11111111-1111-4111-8111-111111111111', 'snapshot_block', 1, '{"v":1}'::jsonb, 'cfg-1', repeat('a', 64), 'shared-key', 3, clock_timestamp()),
  ('bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb5', '22222222-2222-4222-8222-222222222222', 'snapshot_block', 1, '{"v":9}'::jsonb, 'cfg-9', repeat('b', 64), 'shared-key', 3, clock_timestamp()),
  ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa6', '11111111-1111-4111-8111-111111111111', 'snapshot_block', 1, '{"v":1}'::jsonb, 'cfg-1', repeat('a', 64), 'schedule-once', 3, clock_timestamp()),
  ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa7', '11111111-1111-4111-8111-111111111111', 'snapshot_block', 1, '{"v":1}'::jsonb, 'cfg-1', repeat('a', 64), 'long-lease', 3, clock_timestamp()),
  ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa8', '11111111-1111-4111-8111-111111111111', 'snapshot_block', 1, '{"v":1}'::jsonb, 'cfg-1', repeat('a', 64), 'stay-queued', 3, clock_timestamp()),
  ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa9', '11111111-1111-4111-8111-111111111111', 'snapshot_block', 1, '{"v":1}'::jsonb, 'cfg-1', repeat('a', 64), 'exhausted-running', 1, clock_timestamp()),
  ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaa10', '11111111-1111-4111-8111-111111111111', 'snapshot_block', 1, '{"v":1}'::jsonb, 'cfg-1', repeat('a', 64), 'succeed-me', 3, clock_timestamp());

UPDATE jobs.tasks
SET state = 'running',
    attempt = attempt + 1,
    generation = generation + 1,
    lease_owner = 'worker-a',
    lease_until = clock_timestamp() + interval '30 seconds',
    updated_at = clock_timestamp()
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1'
  AND state = 'queued'
  AND generation = 0
  AND lease_owner IS NULL;

UPDATE jobs.tasks
SET lease_until = clock_timestamp() + interval '10 seconds',
    updated_at = clock_timestamp()
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1'
  AND state = 'running'
  AND lease_owner = 'worker-a'
  AND generation = 1;

UPDATE jobs.tasks
SET state = 'cancelled',
    updated_at = clock_timestamp()
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2'
  AND state = 'queued';

UPDATE jobs.tasks
SET state = 'running',
    attempt = attempt + 1,
    generation = generation + 1,
    lease_owner = 'worker-a',
    lease_until = clock_timestamp() + interval '30 seconds',
    updated_at = clock_timestamp()
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3'
  AND state = 'queued';

UPDATE jobs.tasks
SET state = 'failed',
    lease_owner = NULL,
    lease_until = NULL,
    last_error = 'block did not apply',
    updated_at = clock_timestamp()
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3'
  AND state = 'running'
  AND generation = 1
  AND lease_owner = 'worker-a';

UPDATE jobs.tasks
SET state = 'running',
    attempt = attempt + 1,
    generation = generation + 1,
    lease_owner = 'worker-a',
    lease_until = clock_timestamp() + interval '30 seconds',
    updated_at = clock_timestamp()
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa4'
  AND state = 'queued';

UPDATE jobs.tasks
SET state = 'queued',
    lease_owner = NULL,
    lease_until = NULL,
    run_at = clock_timestamp() + interval '1 minute',
    updated_at = clock_timestamp()
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa4'
  AND state = 'running'
  AND attempt < max_attempts;

UPDATE jobs.tasks
SET state = 'running',
    attempt = attempt + 1,
    generation = generation + 1,
    lease_owner = 'worker-b',
    lease_until = clock_timestamp() + interval '30 seconds',
    updated_at = clock_timestamp()
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa4'
  AND state = 'queued'
  AND generation = 1;

UPDATE jobs.tasks
SET state = 'running',
    attempt = attempt + 1,
    generation = generation + 1,
    lease_owner = 'worker-a',
    lease_until = clock_timestamp() + interval '24 hours',
    updated_at = clock_timestamp()
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa7'
  AND state = 'queued';

UPDATE jobs.tasks
SET state = 'running',
    attempt = attempt + 1,
    generation = generation + 1,
    lease_owner = 'worker-a',
    lease_until = clock_timestamp() + interval '30 seconds',
    updated_at = clock_timestamp()
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa9'
  AND state = 'queued';

UPDATE jobs.tasks
SET state = 'running',
    attempt = attempt + 1,
    generation = generation + 1,
    lease_owner = 'worker-a',
    lease_until = clock_timestamp() + interval '30 seconds',
    updated_at = clock_timestamp()
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaa10'
  AND state = 'queued';

UPDATE jobs.tasks
SET state = 'succeeded',
    lease_owner = NULL,
    lease_until = NULL,
    updated_at = clock_timestamp()
WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaa10'
  AND state = 'running'
  AND generation = 1
  AND lease_owner = 'worker-a';

INSERT INTO jobs.schedules (
  id, project_id, kind, config_version, artifact_digest, dedupe_prefix, next_run_at
) VALUES (
  'cccccccc-cccc-4ccc-8ccc-ccccccccccc1',
  '11111111-1111-4111-8111-111111111111',
  'snapshot_block',
  'cfg-1',
  repeat('a', 64),
  'nightly',
  clock_timestamp()
);

INSERT INTO jobs.schedule_occurrences (schedule_id, scheduled_at, project_id, task_id)
VALUES (
  'cccccccc-cccc-4ccc-8ccc-ccccccccccc1',
  timestamptz '2026-10-03 00:00:00+00',
  '11111111-1111-4111-8111-111111111111',
  'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa6'
);

INSERT INTO jobs.leases (id, project_id, target_key)
VALUES (
  'dddddddd-dddd-4ddd-8ddd-ddddddddddd1',
  '11111111-1111-4111-8111-111111111111',
  'source:erp.orders'
);

UPDATE jobs.leases
SET owner = 'worker-a',
    lease_until = clock_timestamp() + interval '30 seconds',
    generation = 1,
    holder_task_id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1',
    updated_at = clock_timestamp()
WHERE id = 'dddddddd-dddd-4ddd-8ddd-ddddddddddd1'
  AND generation = 0
  AND owner IS NULL;

DO $check$
DECLARE
  shared_projects integer;
  dead_letters integer;
  claim_events integer;
  long_lease boolean;
  fence_false boolean;
  succeeded_hidden integer;
  created_ok boolean;
BEGIN
  SELECT count(DISTINCT project_id) INTO shared_projects
  FROM jobs.tasks
  WHERE dedupe_key = 'shared-key';
  IF shared_projects <> 2 THEN
    RAISE EXCEPTION 'project scoped dedupe collapsed projects' USING ERRCODE = 'OO004';
  END IF;

  IF (
    SELECT payload FROM jobs.tasks
    WHERE project_id = '11111111-1111-4111-8111-111111111111'
      AND dedupe_key = 'shared-key'
  ) <> '{"v":1}'::jsonb THEN
    RAISE EXCEPTION 'project A payload changed' USING ERRCODE = 'OO004';
  END IF;

  SELECT count(*) INTO dead_letters
  FROM jobs.dead_letters
  WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3';
  IF dead_letters <> 1 THEN
    RAISE EXCEPTION 'failed task is missing from the dead letter view' USING ERRCODE = 'OO004';
  END IF;

  SELECT count(*) INTO succeeded_hidden
  FROM jobs.dead_letters
  WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaa10';
  IF succeeded_hidden <> 0 THEN
    RAISE EXCEPTION 'succeeded task appeared in the dead letter view' USING ERRCODE = 'OO004';
  END IF;

  SELECT count(*) INTO claim_events
  FROM jobs.task_audit
  WHERE task_id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1'
    AND from_state = 'queued'
    AND to_state = 'running'
    AND generation = 1;
  IF claim_events <> 1 THEN
    RAISE EXCEPTION 'claim was not audited' USING ERRCODE = 'OO004';
  END IF;

  SELECT lease_until > clock_timestamp() + interval '20 hours' INTO long_lease
  FROM jobs.tasks
  WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa7';
  IF long_lease IS NOT TRUE THEN
    RAISE EXCEPTION 'long lease was rejected or rewritten' USING ERRCODE = 'OO004';
  END IF;

  SELECT target_fence_certified = false AND exclusivity_domain = 'control_jobs' INTO fence_false
  FROM jobs.leases
  WHERE id = 'dddddddd-dddd-4ddd-8ddd-ddddddddddd1';
  IF fence_false IS NOT TRUE THEN
    RAISE EXCEPTION 'control lease certified a target fence' USING ERRCODE = 'OO004';
  END IF;

  SELECT bool_and(created_at <= clock_timestamp() AND created_at > clock_timestamp() - interval '5 minutes')
    INTO created_ok
  FROM jobs.tasks;
  IF created_ok IS NOT TRUE THEN
    RAISE EXCEPTION 'created_at is not database time' USING ERRCODE = 'OO004';
  END IF;

  IF (
    SELECT state FROM jobs.tasks WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa4'
  ) <> 'running' OR (
    SELECT attempt FROM jobs.tasks WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa4'
  ) <> 2 THEN
    RAISE EXCEPTION 'retry did not claim the next attempt' USING ERRCODE = 'OO004';
  END IF;

  IF (
    SELECT state FROM jobs.tasks WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa8'
  ) <> 'queued' THEN
    RAISE EXCEPTION 'queued control row changed' USING ERRCODE = 'OO004';
  END IF;

  IF (
    SELECT count(*) FROM jobs.schedule_occurrences
    WHERE schedule_id = 'cccccccc-cccc-4ccc-8ccc-ccccccccccc1'
      AND scheduled_at = timestamptz '2026-10-03 00:00:00+00'
  ) <> 1 THEN
    RAISE EXCEPTION 'schedule occurrence was not recorded once' USING ERRCODE = 'OO004';
  END IF;

  IF (SELECT state FROM jobs.tasks WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2') <> 'cancelled'
     OR (SELECT state FROM jobs.tasks WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3') <> 'failed'
     OR (SELECT state FROM jobs.tasks WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaa10') <> 'succeeded'
  THEN
    RAISE EXCEPTION 'terminal control states were not reached' USING ERRCODE = 'OO004';
  END IF;
END
$check$;
