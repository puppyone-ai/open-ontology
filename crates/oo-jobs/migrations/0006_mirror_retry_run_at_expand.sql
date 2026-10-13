-- A real control retry changes run_at. Keep the 0005 audit-preserving
-- rebind and immutable identity checks; copy only this legal schedule field.
CREATE OR REPLACE FUNCTION jobs.mirror_rebind(p_task uuid, p_control jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, jobs AS $mirror$
DECLARE
  v_old jobs.tasks%ROWTYPE;
  v_new jobs.tasks%ROWTYPE;
BEGIN
  IF p_task IS NULL OR p_control IS NULL OR jsonb_typeof(p_control) <> 'object'
  THEN RAISE EXCEPTION 'mirror claim required' USING ERRCODE = '22023'; END IF;
  v_new := jsonb_populate_record(NULL::jobs.tasks, p_control);
  SELECT * INTO v_old FROM jobs.tasks WHERE id = p_task FOR UPDATE;
  IF NOT FOUND OR v_old.state <> 'running' OR v_old.lease_until > clock_timestamp()
     OR v_new.id IS DISTINCT FROM p_task
     OR v_new.project_id IS DISTINCT FROM v_old.project_id
     OR v_new.kind IS DISTINCT FROM v_old.kind
     OR v_new.payload_version IS DISTINCT FROM v_old.payload_version
     OR v_new.payload IS DISTINCT FROM v_old.payload
     OR v_new.config_version IS DISTINCT FROM v_old.config_version
     OR v_new.artifact_digest IS DISTINCT FROM v_old.artifact_digest
     OR v_new.dedupe_key IS DISTINCT FROM v_old.dedupe_key
     OR v_new.max_attempts IS DISTINCT FROM v_old.max_attempts
     OR v_new.created_at IS DISTINCT FROM v_old.created_at
     OR v_new.state <> 'running' OR v_new.lease_owner IS NULL
     OR v_new.lease_until <= clock_timestamp()
     OR v_new.generation <= v_old.generation
     OR v_new.attempt <= v_old.attempt OR v_new.attempt > v_old.max_attempts
  THEN RAISE EXCEPTION 'mirror claim differs or is stale' USING ERRCODE = 'OO010'; END IF;
  PERFORM set_config('jobs.mirror_rebind_task', p_task::text, true);
  UPDATE jobs.tasks SET generation = v_new.generation, attempt = v_new.attempt,
    lease_owner = v_new.lease_owner, lease_until = v_new.lease_until,
    last_error = v_new.last_error, run_at = v_new.run_at
  WHERE id = p_task;
  PERFORM set_config('jobs.mirror_rebind_task', '', true);
  RETURN jsonb_build_object('task_id', p_task, 'generation', v_new.generation,
    'attempt', v_new.attempt, 'rebound', true);
END;
$mirror$;
