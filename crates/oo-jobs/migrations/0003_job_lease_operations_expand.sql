-- jobs-lease-operations/1 phase expand
-- Run as oo_jobs_owner after jobs-event-project/1, inside the applicator
-- transaction that already holds pg_advisory_xact_lock(6401202601).
-- This file creates owner-owned SECURITY DEFINER functions. It does not
-- rewrite task or event rows, disable triggers, grant the owner role to
-- the runtime, or install a Rust driver.
--
-- oo_jobs_runtime is the trusted control principal for every project. The
-- project_id argument is a minimum filter, not authentication and not
-- project RBAC. Gateway and management auth stay separate. The role is
-- never an Agent credential. Owner and migrator stay privileged.
--
-- Payload is opaque bounded metadata. The functions have no credential
-- parameter. A top-level JSON object key in a fixed denylist is rejected.
-- Nested values and renamed keys are not scanned. Callers must not put raw
-- credentials in payload. Errors are static text and do not echo inputs.
--
-- The functions do not COMMIT. A caller may enqueue in the same control
-- transaction as its own business writes. Claim, heartbeat, and completion
-- hold a row lock only until that caller commits. They are not a place for
-- business work. There is no NOTIFY. A missed wake does not drop a due row;
-- claim scans. Lease length is 30 seconds from clock_timestamp(). Heartbeat
-- extends 30 seconds from clock_timestamp() and does not accept a caller TTL.
-- The later Rust driver polls; 10 seconds is the intended heartbeat period
-- and is not a second lease length.
--
-- jobs.schedules, jobs.schedule_occurrences, and jobs.leases keep the
-- jobs-control/1 runtime grants. This version has no replacement API for
-- them. An active jobs.leases row still cannot set target_fence_certified.
-- jobs-control/1 readiness does not authorize these functions.
--
-- Repeated completion cannot return already_applied. Terminal rows clear
-- lease_owner, and this version does not add a terminal owner column or
-- invent a matching token. A repeated completion returns terminal.
-- Generation overflow fails closed. Expired running rows at the attempt cap
-- are finalized failed, at most 8 per claim call, then selection stops.
-- The a079 lock cycle between SHARE ROW EXCLUSIVE and a claim is not closed
-- here. 0002's lock order is unchanged.

SET LOCAL search_path = pg_catalog, jobs;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_jobs_owner' THEN
    RAISE EXCEPTION 'lease operations migration must run as the owner'
      USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM pg_catalog.pg_constraint AS constraint_row
    JOIN pg_catalog.pg_namespace AS namespace_row
      ON namespace_row.oid = constraint_row.connamespace
    WHERE namespace_row.nspname = 'jobs'
      AND constraint_row.conname = 'task_events_task_project_fkey'
  ) THEN
    RAISE EXCEPTION 'migration bookkeeping incomplete'
      USING ERRCODE = 'OO002';
  END IF;
  IF EXISTS (
    SELECT 1
    FROM pg_catalog.pg_proc AS proc_row
    JOIN pg_catalog.pg_namespace AS namespace_row
      ON namespace_row.oid = proc_row.pronamespace
    WHERE namespace_row.nspname = 'jobs'
      AND proc_row.proname = ANY (ARRAY[
        'enqueue',
        'claim',
        'heartbeat',
        'complete_success',
        'complete_failure',
        'retry_failure',
        'cancel',
        '_lease_result',
        '_lease_token',
        '_require_lease_install'
      ]::pg_catalog.text[])
  ) THEN
    RAISE EXCEPTION 'refusing to adopt an existing jobs function'
      USING ERRCODE = 'OO014';
  END IF;
END
$guard$;

CREATE TYPE jobs.lease_result AS (
  status text,
  task_id uuid,
  project_id uuid,
  kind text,
  state text,
  attempt integer,
  generation bigint,
  lease_owner text,
  lease_until timestamptz,
  run_at timestamptz,
  token text,
  invalidated_owner text,
  invalidated_generation bigint,
  invalidated_until timestamptz
);

COMMENT ON TYPE jobs.lease_result IS
  'Typed result of a lease operation. status is the outcome. token is an unauthenticated jlt1 binding of task, project, owner, generation, and lease_until. Callers do not present the token back. already_applied is not produced.';

CREATE FUNCTION jobs._require_lease_install()
RETURNS void
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog, jobs
AS $$
BEGIN
  -- The row proves the applicator committed this version. It does not open
  -- the release file. Ready for a dependent process is this row plus an
  -- external compare of checksum to 0003_job_lease_operations_expand.sql,
  -- and the same compare for 0000, 0001, and 0002.
  IF NOT EXISTS (
    SELECT 1
    FROM jobs.schema_migrations
    WHERE version = 'jobs-lease-operations/1'
      AND phase = 'expand'
      AND filename = '0003_job_lease_operations_expand.sql'
      AND checksum ~ '^[0-9a-f]{64}$'
  ) THEN
    RAISE EXCEPTION 'lease operations are not ready'
      USING ERRCODE = 'OO015';
  END IF;
END;
$$;

CREATE FUNCTION jobs._lease_token(
  p_task_id uuid,
  p_project_id uuid,
  p_owner text,
  p_generation bigint,
  p_until timestamptz
)
RETURNS text
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog, jobs
AS $$
BEGIN
  RETURN 'jlt1.' || pg_catalog.encode(
    pg_catalog.convert_to(
      p_task_id::text
        || '|' || p_project_id::text
        || '|' || p_owner
        || '|' || p_generation::text
        || '|' || pg_catalog.to_char(
          p_until AT TIME ZONE 'UTC',
          'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
        ),
      'UTF8'
    ),
    'base64'
  );
END;
$$;

CREATE FUNCTION jobs._lease_result(
  p_status text,
  p_task jobs.tasks,
  p_token text,
  p_invalidated_owner text,
  p_invalidated_generation bigint,
  p_invalidated_until timestamptz
)
RETURNS jobs.lease_result
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog, jobs
AS $$
BEGIN
  RETURN ROW(
    p_status,
    p_task.id,
    p_task.project_id,
    p_task.kind,
    p_task.state,
    p_task.attempt,
    p_task.generation,
    p_task.lease_owner,
    p_task.lease_until,
    p_task.run_at,
    p_token,
    p_invalidated_owner,
    p_invalidated_generation,
    p_invalidated_until
  )::jobs.lease_result;
END;
$$;

CREATE FUNCTION jobs.enqueue(
  p_project_id uuid,
  p_kind text,
  p_payload_version integer,
  p_payload text,
  p_config_version text,
  p_artifact_digest text,
  p_dedupe_key text,
  p_run_at timestamptz,
  p_max_attempts integer
)
RETURNS jobs.lease_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, jobs
AS $$
DECLARE
  v_payload jsonb;
  v_row jobs.tasks%ROWTYPE;
  v_constraint text;
BEGIN
  PERFORM jobs._require_lease_install();
  IF p_project_id IS NULL
     OR p_kind IS NULL
     OR p_payload_version IS NULL
     OR p_payload IS NULL
     OR p_config_version IS NULL
     OR p_artifact_digest IS NULL
     OR p_dedupe_key IS NULL
     OR p_run_at IS NULL
     OR p_max_attempts IS NULL
     OR p_kind !~ '^[a-z][a-z0-9_]{0,63}$'
     OR p_payload_version < 1
     OR p_payload_version > 1000000
     OR octet_length(p_payload) < 1
     OR octet_length(p_payload) > 16384
     OR octet_length(p_config_version) < 1
     OR octet_length(p_config_version) > 128
     OR p_artifact_digest !~ '^[0-9a-f]{64}$'
     OR octet_length(p_dedupe_key) < 1
     OR octet_length(p_dedupe_key) > 256
     OR p_max_attempts < 1
     OR p_max_attempts > 32
     OR p_run_at = 'infinity'::timestamptz
     OR p_run_at = '-infinity'::timestamptz
  THEN
    RAISE EXCEPTION 'invalid lease operation input'
      USING ERRCODE = 'OO010';
  END IF;

  BEGIN
    v_payload := p_payload::jsonb;
  EXCEPTION
    WHEN invalid_text_representation THEN
      RAISE EXCEPTION 'invalid lease operation input'
        USING ERRCODE = 'OO010';
  END;

  IF v_payload IS NULL
     OR jsonb_typeof(v_payload) IS DISTINCT FROM 'object'
     OR octet_length(v_payload::text) > 16384
     OR EXISTS (
       SELECT 1
       FROM jsonb_object_keys(v_payload) AS object_key
       WHERE object_key = ANY (ARRAY[
         'password',
         'passwd',
         'secret',
         'credential',
         'credentials',
         'api_key',
         'access_token',
         'private_key',
         'authorization'
       ]::text[])
     )
  THEN
    RAISE EXCEPTION 'invalid lease operation input'
      USING ERRCODE = 'OO010';
  END IF;

  BEGIN
    INSERT INTO jobs.tasks (
      project_id,
      kind,
      payload_version,
      payload,
      config_version,
      artifact_digest,
      dedupe_key,
      state,
      run_at,
      max_attempts
    ) VALUES (
      p_project_id,
      p_kind,
      p_payload_version,
      v_payload,
      p_config_version,
      p_artifact_digest,
      p_dedupe_key,
      'queued',
      p_run_at,
      p_max_attempts
    )
    RETURNING * INTO v_row;
    RETURN jobs._lease_result('enqueued', v_row, NULL, NULL, NULL, NULL);
  EXCEPTION
    WHEN unique_violation THEN
      GET STACKED DIAGNOSTICS v_constraint = CONSTRAINT_NAME;
      IF v_constraint IS DISTINCT FROM 'tasks_project_dedupe_unique' THEN
        RAISE EXCEPTION 'lease operation failed'
          USING ERRCODE = 'OO016';
      END IF;
    WHEN check_violation OR not_null_violation THEN
      RAISE EXCEPTION 'invalid lease operation input'
        USING ERRCODE = 'OO010';
  END;

  SELECT * INTO v_row
  FROM jobs.tasks
  WHERE project_id = p_project_id
    AND dedupe_key = p_dedupe_key;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'lease operation failed'
      USING ERRCODE = 'OO016';
  END IF;
  IF v_row.kind IS NOT DISTINCT FROM p_kind
     AND v_row.payload_version IS NOT DISTINCT FROM p_payload_version
     AND v_row.payload IS NOT DISTINCT FROM v_payload
     AND v_row.config_version IS NOT DISTINCT FROM p_config_version
     AND v_row.artifact_digest IS NOT DISTINCT FROM p_artifact_digest
     AND v_row.max_attempts IS NOT DISTINCT FROM p_max_attempts
  THEN
    RETURN jobs._lease_result('replayed', v_row, NULL, NULL, NULL, NULL);
  END IF;
  RAISE EXCEPTION 'dedupe identity conflict'
    USING ERRCODE = 'OO011';
END;
$$;

COMMENT ON FUNCTION jobs.enqueue(
  uuid, text, integer, text, text, text, text, timestamptz, integer
) IS
  'Insert a queued task or return the same task when project, dedupe key, kind, payload version, payload, config version, artifact digest, and max_attempts match. run_at is not part of that identity and is not updated on replay. A different identity raises OO011 and writes nothing. Does not COMMIT.';

CREATE FUNCTION jobs.claim(
  p_project_id uuid,
  p_allowed_kinds text[],
  p_worker_id text
)
RETURNS jobs.lease_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, jobs
AS $$
DECLARE
  v_row jobs.tasks%ROWTYPE;
  v_until timestamptz;
  v_step integer;
BEGIN
  PERFORM jobs._require_lease_install();
  IF p_project_id IS NULL
     OR p_allowed_kinds IS NULL
     OR p_worker_id IS NULL
     OR cardinality(p_allowed_kinds) < 1
     OR cardinality(p_allowed_kinds) > 64
     OR octet_length(p_worker_id) < 1
     OR octet_length(p_worker_id) > 128
     OR p_worker_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
     OR EXISTS (
       SELECT 1
       FROM unnest(p_allowed_kinds) AS kind
       WHERE kind IS NULL
          OR kind !~ '^[a-z][a-z0-9_]{0,63}$'
     )
     OR (
       SELECT count(*) FROM unnest(p_allowed_kinds)
     ) IS DISTINCT FROM (
       SELECT count(DISTINCT kind) FROM unnest(p_allowed_kinds) AS kind
     )
  THEN
    RAISE EXCEPTION 'invalid lease operation input'
      USING ERRCODE = 'OO010';
  END IF;

  -- At most 8 locked rows per call. Expired running rows at the attempt cap
  -- are finalized failed and the scan continues. The call then stops, even
  -- if a later due row exists. The next call continues. This is not a sweep.
  FOR v_step IN 1..8 LOOP
    SELECT * INTO v_row
    FROM jobs.tasks
    WHERE project_id = p_project_id
      AND kind = ANY (p_allowed_kinds)
      AND (
        (state = 'queued' AND run_at <= clock_timestamp())
        OR (state = 'running' AND lease_until <= clock_timestamp())
      )
    ORDER BY run_at, id
    FOR UPDATE SKIP LOCKED
    LIMIT 1;
    IF NOT FOUND THEN
      RETURN jobs._lease_result('none', NULL::jobs.tasks, NULL, NULL, NULL, NULL);
    END IF;

    IF v_row.state = 'running' AND v_row.attempt >= v_row.max_attempts THEN
      UPDATE jobs.tasks
      SET state = 'failed',
          lease_owner = NULL,
          lease_until = NULL,
          last_error = 'lease_expired_attempt_cap'
      WHERE id = v_row.id
        AND project_id = p_project_id
        AND state = 'running'
        AND attempt >= max_attempts
        AND lease_until <= clock_timestamp();
      IF NOT FOUND THEN
        RAISE EXCEPTION 'lease operation failed'
          USING ERRCODE = 'OO016';
      END IF;
      CONTINUE;
    END IF;

    IF v_row.generation >= 9223372036854775807 THEN
      RAISE EXCEPTION 'generation overflow'
        USING ERRCODE = 'OO012';
    END IF;

    IF v_row.state = 'running' THEN
      UPDATE jobs.tasks
      SET state = 'queued',
          lease_owner = NULL,
          lease_until = NULL,
          last_error = NULL
      WHERE id = v_row.id
        AND project_id = p_project_id
        AND state = 'running'
        AND attempt < max_attempts
        AND attempt = v_row.attempt
        AND generation = v_row.generation
        AND lease_until <= clock_timestamp();
      IF NOT FOUND THEN
        RAISE EXCEPTION 'lease operation failed'
          USING ERRCODE = 'OO016';
      END IF;
    END IF;

    v_until := clock_timestamp() + interval '30 seconds';
    UPDATE jobs.tasks
    SET state = 'running',
        attempt = attempt + 1,
        generation = generation + 1,
        lease_owner = p_worker_id,
        lease_until = v_until,
        last_error = NULL
    WHERE id = v_row.id
      AND project_id = p_project_id
      AND state = 'queued'
      AND attempt = v_row.attempt
      AND generation = v_row.generation
    RETURNING * INTO v_row;
    IF NOT FOUND THEN
      RAISE EXCEPTION 'lease operation failed'
        USING ERRCODE = 'OO016';
    END IF;
    RETURN jobs._lease_result(
      'claimed',
      v_row,
      jobs._lease_token(
        v_row.id,
        v_row.project_id,
        v_row.lease_owner,
        v_row.generation,
        v_row.lease_until
      ),
      NULL,
      NULL,
      NULL
    );
  END LOOP;

  RETURN jobs._lease_result('none', NULL::jobs.tasks, NULL, NULL, NULL, NULL);
END;
$$;

COMMENT ON FUNCTION jobs.claim(uuid, text[], text) IS
  'Claim one due queued task, or reclaim one expired running task, in project_id whose kind is in the bounded list. Order is run_at, id. FOR UPDATE SKIP LOCKED. Lease is clock_timestamp() plus 30 seconds. Expired running is queued without a new attempt, then claimed as a new attempt and generation, including when the worker id string is unchanged. At the attempt cap the expired row becomes failed with last_error lease_expired_attempt_cap and is not resurrected. Empty selection returns status none. Does not COMMIT and does not NOTIFY.';

CREATE FUNCTION jobs.heartbeat(
  p_project_id uuid,
  p_task_id uuid,
  p_worker_id text,
  p_generation bigint
)
RETURNS jobs.lease_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, jobs
AS $$
DECLARE
  v_row jobs.tasks%ROWTYPE;
  v_until timestamptz;
BEGIN
  PERFORM jobs._require_lease_install();
  IF p_project_id IS NULL
     OR p_task_id IS NULL
     OR p_worker_id IS NULL
     OR p_generation IS NULL
     OR p_generation < 1
     OR octet_length(p_worker_id) < 1
     OR octet_length(p_worker_id) > 128
     OR p_worker_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
  THEN
    RAISE EXCEPTION 'invalid lease operation input'
      USING ERRCODE = 'OO010';
  END IF;

  v_until := clock_timestamp() + interval '30 seconds';
  UPDATE jobs.tasks
  SET lease_until = v_until
  WHERE id = p_task_id
    AND project_id = p_project_id
    AND state = 'running'
    AND lease_owner = p_worker_id
    AND generation = p_generation
    AND lease_until > clock_timestamp()
  RETURNING * INTO v_row;
  IF FOUND THEN
    RETURN jobs._lease_result(
      'extended',
      v_row,
      jobs._lease_token(
        v_row.id,
        v_row.project_id,
        v_row.lease_owner,
        v_row.generation,
        v_row.lease_until
      ),
      NULL,
      NULL,
      NULL
    );
  END IF;

  SELECT * INTO v_row
  FROM jobs.tasks
  WHERE id = p_task_id
    AND project_id = p_project_id;
  IF NOT FOUND THEN
    RETURN jobs._lease_result('lost_lease', NULL::jobs.tasks, NULL, NULL, NULL, NULL);
  END IF;
  IF v_row.state IN ('succeeded', 'failed', 'cancelled') THEN
    RETURN jobs._lease_result('terminal', v_row, NULL, NULL, NULL, NULL);
  END IF;
  RETURN jobs._lease_result('lost_lease', v_row, NULL, NULL, NULL, NULL);
END;
$$;

COMMENT ON FUNCTION jobs.heartbeat(uuid, uuid, text, bigint) IS
  'Extend lease_until by 30 seconds from clock_timestamp() only while the row is running, the project, task, worker, and generation match, and lease_until is still after clock_timestamp(). An expired lease is not revived. There is no caller TTL. A pure lease_until change does not append task_events; the jobs-control/1 trigger audits state, attempt, generation, owner, and last_error.';

CREATE FUNCTION jobs.complete_success(
  p_project_id uuid,
  p_task_id uuid,
  p_worker_id text,
  p_generation bigint
)
RETURNS jobs.lease_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, jobs
AS $$
DECLARE
  v_row jobs.tasks%ROWTYPE;
BEGIN
  PERFORM jobs._require_lease_install();
  IF p_project_id IS NULL
     OR p_task_id IS NULL
     OR p_worker_id IS NULL
     OR p_generation IS NULL
     OR p_generation < 1
     OR octet_length(p_worker_id) < 1
     OR octet_length(p_worker_id) > 128
     OR p_worker_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
  THEN
    RAISE EXCEPTION 'invalid lease operation input'
      USING ERRCODE = 'OO010';
  END IF;

  UPDATE jobs.tasks
  SET state = 'succeeded',
      lease_owner = NULL,
      lease_until = NULL,
      last_error = NULL
  WHERE id = p_task_id
    AND project_id = p_project_id
    AND state = 'running'
    AND lease_owner = p_worker_id
    AND generation = p_generation
    AND lease_until > clock_timestamp()
  RETURNING * INTO v_row;
  IF FOUND THEN
    RETURN jobs._lease_result('succeeded', v_row, NULL, NULL, NULL, NULL);
  END IF;

  SELECT * INTO v_row
  FROM jobs.tasks
  WHERE id = p_task_id
    AND project_id = p_project_id;
  IF NOT FOUND THEN
    RETURN jobs._lease_result('lost_lease', NULL::jobs.tasks, NULL, NULL, NULL, NULL);
  END IF;
  IF v_row.state IN ('succeeded', 'failed', 'cancelled') THEN
    -- No terminal owner column is stored, so a repeated success is terminal
    -- rather than already_applied. Any generation is not treated as success.
    RETURN jobs._lease_result('terminal', v_row, NULL, NULL, NULL, NULL);
  END IF;
  RETURN jobs._lease_result('lost_lease', v_row, NULL, NULL, NULL, NULL);
END;
$$;

COMMENT ON FUNCTION jobs.complete_success(uuid, uuid, text, bigint) IS
  'Move a live matching lease to succeeded and clear owner and lease_until. The existing audit trigger records the transition in the same transaction. A repeat returns terminal. Does not COMMIT.';

CREATE FUNCTION jobs.complete_failure(
  p_project_id uuid,
  p_task_id uuid,
  p_worker_id text,
  p_generation bigint,
  p_error_kind text
)
RETURNS jobs.lease_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, jobs
AS $$
DECLARE
  v_row jobs.tasks%ROWTYPE;
BEGIN
  PERFORM jobs._require_lease_install();
  IF p_project_id IS NULL
     OR p_task_id IS NULL
     OR p_worker_id IS NULL
     OR p_generation IS NULL
     OR p_error_kind IS NULL
     OR p_generation < 1
     OR octet_length(p_worker_id) < 1
     OR octet_length(p_worker_id) > 128
     OR p_worker_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
     OR p_error_kind <> ALL (ARRAY[
       'worker_failed',
       'payload_rejected',
       'dependency_unavailable',
       'external_timeout'
     ]::text[])
  THEN
    RAISE EXCEPTION 'invalid lease operation input'
      USING ERRCODE = 'OO010';
  END IF;

  UPDATE jobs.tasks
  SET state = 'failed',
      lease_owner = NULL,
      lease_until = NULL,
      last_error = p_error_kind
  WHERE id = p_task_id
    AND project_id = p_project_id
    AND state = 'running'
    AND lease_owner = p_worker_id
    AND generation = p_generation
    AND lease_until > clock_timestamp()
  RETURNING * INTO v_row;
  IF FOUND THEN
    RETURN jobs._lease_result('failed', v_row, NULL, NULL, NULL, NULL);
  END IF;

  SELECT * INTO v_row
  FROM jobs.tasks
  WHERE id = p_task_id
    AND project_id = p_project_id;
  IF NOT FOUND THEN
    RETURN jobs._lease_result('lost_lease', NULL::jobs.tasks, NULL, NULL, NULL, NULL);
  END IF;
  IF v_row.state IN ('succeeded', 'failed', 'cancelled') THEN
    RETURN jobs._lease_result('terminal', v_row, NULL, NULL, NULL, NULL);
  END IF;
  RETURN jobs._lease_result('lost_lease', v_row, NULL, NULL, NULL, NULL);
END;
$$;

COMMENT ON FUNCTION jobs.complete_failure(uuid, uuid, text, bigint, text) IS
  'Move a live matching lease to failed. last_error is the allowlisted kind and not a caller message. A repeat returns terminal. Does not COMMIT.';

CREATE FUNCTION jobs.retry_failure(
  p_project_id uuid,
  p_task_id uuid,
  p_worker_id text,
  p_generation bigint,
  p_error_kind text
)
RETURNS jobs.lease_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, jobs
AS $$
DECLARE
  v_row jobs.tasks%ROWTYPE;
  v_delay bigint;
BEGIN
  PERFORM jobs._require_lease_install();
  IF p_project_id IS NULL
     OR p_task_id IS NULL
     OR p_worker_id IS NULL
     OR p_generation IS NULL
     OR p_error_kind IS NULL
     OR p_generation < 1
     OR octet_length(p_worker_id) < 1
     OR octet_length(p_worker_id) > 128
     OR p_worker_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
     OR p_error_kind <> ALL (ARRAY[
       'worker_failed',
       'payload_rejected',
       'dependency_unavailable',
       'external_timeout'
     ]::text[])
  THEN
    RAISE EXCEPTION 'invalid lease operation input'
      USING ERRCODE = 'OO010';
  END IF;

  SELECT * INTO v_row
  FROM jobs.tasks
  WHERE id = p_task_id
    AND project_id = p_project_id
  FOR UPDATE;
  IF NOT FOUND THEN
    RETURN jobs._lease_result('lost_lease', NULL::jobs.tasks, NULL, NULL, NULL, NULL);
  END IF;
  IF v_row.state IN ('succeeded', 'failed', 'cancelled') THEN
    RETURN jobs._lease_result('terminal', v_row, NULL, NULL, NULL, NULL);
  END IF;
  IF v_row.state IS DISTINCT FROM 'running'
     OR v_row.lease_owner IS DISTINCT FROM p_worker_id
     OR v_row.generation IS DISTINCT FROM p_generation
     OR v_row.lease_until IS NULL
     OR v_row.lease_until <= clock_timestamp()
  THEN
    RETURN jobs._lease_result('lost_lease', v_row, NULL, NULL, NULL, NULL);
  END IF;

  IF v_row.attempt >= v_row.max_attempts THEN
    UPDATE jobs.tasks
    SET state = 'failed',
        lease_owner = NULL,
        lease_until = NULL,
        last_error = 'attempt_exhausted'
    WHERE id = v_row.id
      AND project_id = p_project_id
      AND state = 'running'
      AND lease_owner = p_worker_id
      AND generation = p_generation
      AND attempt >= max_attempts
      AND lease_until > clock_timestamp()
    RETURNING * INTO v_row;
    IF NOT FOUND THEN
      RAISE EXCEPTION 'lease operation failed'
        USING ERRCODE = 'OO016';
    END IF;
    RETURN jobs._lease_result('failed', v_row, NULL, NULL, NULL, NULL);
  END IF;

  IF v_row.attempt < 1 OR v_row.attempt > 32 THEN
    RAISE EXCEPTION 'lease operation failed'
      USING ERRCODE = 'OO016';
  END IF;
  -- attempt 1 waits 1s, then 2, 4, 8, 16, 32. attempt 7 and above wait 60s.
  -- The shift is only used for attempt 1..6, so the exponent stays in 0..5.
  IF v_row.attempt >= 7 THEN
    v_delay := 60;
  ELSE
    v_delay := 1::bigint << (v_row.attempt - 1);
  END IF;

  UPDATE jobs.tasks
  SET state = 'queued',
      run_at = clock_timestamp() + make_interval(secs => v_delay::double precision),
      lease_owner = NULL,
      lease_until = NULL,
      last_error = p_error_kind
  WHERE id = v_row.id
    AND project_id = p_project_id
    AND state = 'running'
    AND lease_owner = p_worker_id
    AND generation = p_generation
    AND attempt = v_row.attempt
    AND attempt < max_attempts
    AND lease_until > clock_timestamp()
  RETURNING * INTO v_row;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'lease operation failed'
      USING ERRCODE = 'OO016';
  END IF;
  RETURN jobs._lease_result('retried', v_row, NULL, NULL, NULL, NULL);
END;
$$;

COMMENT ON FUNCTION jobs.retry_failure(uuid, uuid, text, bigint, text) IS
  'Requeue a live matching lease without incrementing attempt. run_at is clock_timestamp() plus min(60, 2^(attempt-1)) seconds. The next claim increments attempt. At the attempt cap the row becomes failed with last_error attempt_exhausted. last_error on a requeue is the allowlisted kind. Does not COMMIT.';

CREATE FUNCTION jobs.cancel(
  p_project_id uuid,
  p_task_id uuid,
  p_worker_id text,
  p_generation bigint
)
RETURNS jobs.lease_result
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, jobs
AS $$
DECLARE
  v_row jobs.tasks%ROWTYPE;
  v_owner text;
  v_generation bigint;
  v_until timestamptz;
BEGIN
  PERFORM jobs._require_lease_install();
  IF p_project_id IS NULL
     OR p_task_id IS NULL
     OR p_worker_id IS NULL
     OR p_generation IS NULL
     OR p_generation < 0
     OR octet_length(p_worker_id) < 1
     OR octet_length(p_worker_id) > 128
     OR p_worker_id !~ '^[A-Za-z0-9_.:-]{1,128}$'
  THEN
    RAISE EXCEPTION 'invalid lease operation input'
      USING ERRCODE = 'OO010';
  END IF;

  SELECT * INTO v_row
  FROM jobs.tasks
  WHERE id = p_task_id
    AND project_id = p_project_id
  FOR UPDATE;
  IF NOT FOUND THEN
    RETURN jobs._lease_result('lost_lease', NULL::jobs.tasks, NULL, NULL, NULL, NULL);
  END IF;
  IF v_row.state IN ('succeeded', 'failed', 'cancelled') THEN
    RETURN jobs._lease_result('terminal', v_row, NULL, NULL, NULL, NULL);
  END IF;

  IF v_row.state = 'queued' AND v_row.generation = p_generation THEN
    UPDATE jobs.tasks
    SET state = 'cancelled',
        lease_owner = NULL,
        lease_until = NULL,
        last_error = NULL
    WHERE id = v_row.id
      AND project_id = p_project_id
      AND state = 'queued'
      AND generation = p_generation
    RETURNING * INTO v_row;
    IF NOT FOUND THEN
      RAISE EXCEPTION 'lease operation failed'
        USING ERRCODE = 'OO016';
    END IF;
    RETURN jobs._lease_result('cancelled', v_row, NULL, NULL, NULL, NULL);
  END IF;

  IF v_row.state = 'running'
     AND v_row.lease_owner = p_worker_id
     AND v_row.generation = p_generation
  THEN
    v_owner := v_row.lease_owner;
    v_generation := v_row.generation;
    v_until := v_row.lease_until;
    UPDATE jobs.tasks
    SET state = 'cancelled',
        lease_owner = NULL,
        lease_until = NULL,
        last_error = NULL
    WHERE id = v_row.id
      AND project_id = p_project_id
      AND state = 'running'
      AND lease_owner = p_worker_id
      AND generation = p_generation
    RETURNING * INTO v_row;
    IF NOT FOUND THEN
      RAISE EXCEPTION 'lease operation failed'
        USING ERRCODE = 'OO016';
    END IF;
    RETURN jobs._lease_result(
      'cancelled',
      v_row,
      NULL,
      v_owner,
      v_generation,
      v_until
    );
  END IF;

  RETURN jobs._lease_result('lost_lease', v_row, NULL, NULL, NULL, NULL);
END;
$$;

COMMENT ON FUNCTION jobs.cancel(uuid, uuid, text, bigint) IS
  'Cancel one queued row in the project when the generation matches, or one running row when worker and generation match, including after lease_until. A queued cancel does not match a lease owner because the row has none. A running cancel returns the owner, generation, and lease_until that were cleared. It does not terminate a target connection or certify a fence. A second cancel returns terminal. Does not COMMIT.';

CREATE INDEX tasks_running_due_idx
  ON jobs.tasks (run_at, id)
  WHERE state = 'running';

COMMENT ON INDEX jobs.tasks_running_due_idx IS
  'Supports the running side of claim. Eligibility still requires lease_until <= clock_timestamp(), which is not an immutable index predicate.';

REVOKE ALL ON FUNCTION jobs._require_lease_install() FROM PUBLIC;
REVOKE ALL ON FUNCTION jobs._require_lease_install() FROM oo_jobs_runtime;
REVOKE ALL ON FUNCTION jobs._lease_token(uuid, uuid, text, bigint, timestamptz) FROM PUBLIC;
REVOKE ALL ON FUNCTION jobs._lease_token(uuid, uuid, text, bigint, timestamptz) FROM oo_jobs_runtime;
REVOKE ALL ON FUNCTION jobs._lease_result(text, jobs.tasks, text, text, bigint, timestamptz) FROM PUBLIC;
REVOKE ALL ON FUNCTION jobs._lease_result(text, jobs.tasks, text, text, bigint, timestamptz) FROM oo_jobs_runtime;

REVOKE ALL ON FUNCTION jobs.enqueue(uuid, text, integer, text, text, text, text, timestamptz, integer) FROM PUBLIC;
REVOKE ALL ON FUNCTION jobs.claim(uuid, text[], text) FROM PUBLIC;
REVOKE ALL ON FUNCTION jobs.heartbeat(uuid, uuid, text, bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION jobs.complete_success(uuid, uuid, text, bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION jobs.complete_failure(uuid, uuid, text, bigint, text) FROM PUBLIC;
REVOKE ALL ON FUNCTION jobs.retry_failure(uuid, uuid, text, bigint, text) FROM PUBLIC;
REVOKE ALL ON FUNCTION jobs.cancel(uuid, uuid, text, bigint) FROM PUBLIC;

GRANT EXECUTE ON FUNCTION jobs.enqueue(uuid, text, integer, text, text, text, text, timestamptz, integer) TO oo_jobs_runtime;
GRANT EXECUTE ON FUNCTION jobs.claim(uuid, text[], text) TO oo_jobs_runtime;
GRANT EXECUTE ON FUNCTION jobs.heartbeat(uuid, uuid, text, bigint) TO oo_jobs_runtime;
GRANT EXECUTE ON FUNCTION jobs.complete_success(uuid, uuid, text, bigint) TO oo_jobs_runtime;
GRANT EXECUTE ON FUNCTION jobs.complete_failure(uuid, uuid, text, bigint, text) TO oo_jobs_runtime;
GRANT EXECUTE ON FUNCTION jobs.retry_failure(uuid, uuid, text, bigint, text) TO oo_jobs_runtime;
GRANT EXECUTE ON FUNCTION jobs.cancel(uuid, uuid, text, bigint) TO oo_jobs_runtime;

REVOKE ALL ON TYPE jobs.lease_result FROM PUBLIC;
GRANT USAGE ON TYPE jobs.lease_result TO oo_jobs_runtime;

-- Direct task and event writes are closed for the runtime. Rows already
-- stored under jobs-control/1 and jobs-event-project/1 are not rewritten.
-- Schedule and control-lease grants from jobs-control/1 stay.
REVOKE ALL ON jobs.tasks FROM oo_jobs_runtime;
GRANT SELECT ON jobs.tasks TO oo_jobs_runtime;
REVOKE ALL ON jobs.task_events FROM oo_jobs_runtime;
GRANT SELECT ON jobs.task_events TO oo_jobs_runtime;
REVOKE ALL ON SEQUENCE jobs.task_events_event_id_seq FROM oo_jobs_runtime;

CREATE VIEW jobs.lease_operations_readiness
WITH (security_invoker = true) AS
SELECT
  version,
  checksum,
  phase,
  filename,
  applied_at,
  (version = 'jobs-lease-operations/1' AND phase = 'expand') AS schema_version_present
FROM jobs.schema_migrations
WHERE version = 'jobs-lease-operations/1';

COMMENT ON VIEW jobs.lease_operations_readiness IS
  'Version marker for jobs-lease-operations/1 expand. schema_version_present does not prove these file bytes match checksum. jobs.service_readiness still reports only jobs-control/1 and does not authorize these functions. The view does not repair old events and does not close the a079 40P01 lock cycle.';

REVOKE ALL ON jobs.lease_operations_readiness FROM PUBLIC;
GRANT SELECT ON jobs.lease_operations_readiness TO oo_jobs_runtime;

DO $check$
DECLARE
  v_bad integer;
BEGIN
  SELECT count(*) INTO v_bad
  FROM pg_catalog.pg_proc AS proc_row
  JOIN pg_catalog.pg_namespace AS namespace_row
    ON namespace_row.oid = proc_row.pronamespace
  WHERE namespace_row.nspname = 'jobs'
    AND proc_row.proname = ANY (ARRAY[
      'enqueue',
      'claim',
      'heartbeat',
      'complete_success',
      'complete_failure',
      'retry_failure',
      'cancel'
    ]::text[])
    AND (
      proc_row.prosecdef IS NOT TRUE
      OR proc_row.proowner IS DISTINCT FROM 'oo_jobs_owner'::regrole
      OR NOT EXISTS (
        SELECT 1
        FROM unnest(proc_row.proconfig) AS cfg
        WHERE cfg = 'search_path=pg_catalog, jobs'
      )
      OR proc_row.proacl IS NULL
      OR EXISTS (
        SELECT 1
        FROM aclexplode(proc_row.proacl) AS acl_row
        WHERE acl_row.grantee = 0
          AND acl_row.privilege_type = 'EXECUTE'
      )
    );
  IF v_bad <> 0
     OR NOT has_function_privilege(
       'oo_jobs_runtime',
       'jobs.claim(uuid,text[],text)',
       'EXECUTE'
     )
     OR has_function_privilege(
       'public',
       'jobs.claim(uuid,text[],text)',
       'EXECUTE'
     )
     OR has_function_privilege(
       'oo_jobs_runtime',
       'jobs._lease_token(uuid,uuid,text,bigint,timestamptz)',
       'EXECUTE'
     )
     OR has_table_privilege('oo_jobs_runtime', 'jobs.tasks', 'INSERT')
     OR has_table_privilege('oo_jobs_runtime', 'jobs.tasks', 'UPDATE')
     OR has_table_privilege('oo_jobs_runtime', 'jobs.tasks', 'DELETE')
     OR has_column_privilege('oo_jobs_runtime', 'jobs.tasks', 'project_id', 'INSERT')
     OR has_column_privilege('oo_jobs_runtime', 'jobs.tasks', 'state', 'UPDATE')
     OR has_table_privilege('oo_jobs_runtime', 'jobs.task_events', 'INSERT')
     OR has_table_privilege('oo_jobs_runtime', 'jobs.task_events', 'UPDATE')
     OR has_column_privilege('oo_jobs_runtime', 'jobs.task_events', 'note', 'INSERT')
     OR has_sequence_privilege('oo_jobs_runtime', 'jobs.task_events_event_id_seq', 'USAGE')
     OR NOT has_table_privilege('oo_jobs_runtime', 'jobs.tasks', 'SELECT')
     OR NOT has_table_privilege('oo_jobs_runtime', 'jobs.task_events', 'SELECT')
     OR NOT has_column_privilege('oo_jobs_runtime', 'jobs.schedules', 'kind', 'INSERT')
     OR NOT has_column_privilege('oo_jobs_runtime', 'jobs.leases', 'owner', 'UPDATE')
     OR has_column_privilege(
       'oo_jobs_runtime',
       'jobs.leases',
       'target_fence_certified',
       'UPDATE'
     )
     OR EXISTS (
       SELECT 1
       FROM pg_catalog.pg_trigger
       WHERE tgname = ANY (ARRAY[
         'tasks_enforce_update',
         'tasks_audit_insert',
         'tasks_audit_update',
         'task_events_reject_update',
         'task_events_reject_delete'
       ]::text[])
         AND NOT tgisinternal
         AND tgenabled <> 'O'
     )
  THEN
    RAISE EXCEPTION 'lease operations install check failed'
      USING ERRCODE = 'OO017';
  END IF;
END
$check$;
