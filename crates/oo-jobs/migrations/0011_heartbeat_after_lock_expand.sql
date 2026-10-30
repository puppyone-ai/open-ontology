-- Renew from the clock after a Loader transaction releases the task row.
-- Computing the deadline before waiting can overwrite a newer renewal with
-- an already-expired timestamp. The lease remains fixed at 30 seconds.
DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_jobs_owner' THEN
    RAISE EXCEPTION 'lease operations migration must run as the owner'
      USING ERRCODE = 'OO013';
  END IF;
END
$guard$;

CREATE OR REPLACE FUNCTION jobs.heartbeat(
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

  SELECT * INTO v_row FROM jobs.tasks
  WHERE id = p_task_id AND project_id = p_project_id
  FOR UPDATE;
  IF NOT FOUND THEN
    RETURN jobs._lease_result('lost_lease', NULL::jobs.tasks, NULL, NULL, NULL, NULL);
  END IF;
  IF v_row.state IN ('succeeded', 'failed', 'cancelled') THEN
    RETURN jobs._lease_result('terminal', v_row, NULL, NULL, NULL, NULL);
  END IF;
  IF v_row.state <> 'running'
     OR v_row.lease_owner IS DISTINCT FROM p_worker_id
     OR v_row.generation <> p_generation
     OR v_row.lease_until <= clock_timestamp()
  THEN
    RETURN jobs._lease_result('lost_lease', v_row, NULL, NULL, NULL, NULL);
  END IF;

  UPDATE jobs.tasks
  SET lease_until = clock_timestamp() + interval '30 seconds'
  WHERE id = p_task_id AND project_id = p_project_id
  RETURNING * INTO v_row;
  RETURN jobs._lease_result(
    'extended', v_row,
    jobs._lease_token(v_row.id, v_row.project_id, v_row.lease_owner,
                      v_row.generation, v_row.lease_until),
    NULL, NULL, NULL
  );
END;
$$;
REVOKE ALL ON FUNCTION jobs.heartbeat(uuid, uuid, text, bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION jobs.heartbeat(uuid, uuid, text, bigint) TO oo_jobs_runtime;
