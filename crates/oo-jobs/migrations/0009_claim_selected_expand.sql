-- Claim one exact due task from a trusted occurrence, preserving 0003 lease rules.
CREATE FUNCTION jobs.claim_selected(
  p_project_id uuid,
  p_task_id uuid,
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
     OR p_task_id IS NULL
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

  -- Only the selected row is eligible. An expired task at the attempt cap
  -- becomes failed; a later loop returns none without claiming another task.
  FOR v_step IN 1..8 LOOP
    SELECT * INTO v_row
    FROM jobs.tasks
    WHERE id = p_task_id
      AND project_id = p_project_id
      AND kind = ANY (p_allowed_kinds)
      AND (
        (state = 'queued' AND run_at <= clock_timestamp())
        OR (state = 'running' AND lease_until <= clock_timestamp())
      )
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

REVOKE ALL ON FUNCTION jobs.claim_selected(uuid,uuid,text[],text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION jobs.claim_selected(uuid,uuid,text[],text) TO oo_jobs_runtime;
