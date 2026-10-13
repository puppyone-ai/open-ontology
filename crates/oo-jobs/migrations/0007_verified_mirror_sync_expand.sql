-- Target owner mirrors only a live control claim supplied by the pinned Loader.
-- Ordinary target runtime keeps SELECT-only access to jobs.tasks.
CREATE FUNCTION jobs.mirror_sync(p_task uuid, p_control jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, jobs AS $mirror$
DECLARE
  v_old jobs.tasks%ROWTYPE;
  v_new jobs.tasks%ROWTYPE;
BEGIN
  IF p_task IS NULL OR p_control IS NULL OR jsonb_typeof(p_control) <> 'object'
  THEN RAISE EXCEPTION 'mirror claim required' USING ERRCODE = '22023'; END IF;
  v_new := jsonb_populate_record(NULL::jobs.tasks, p_control);
  IF v_new.id IS DISTINCT FROM p_task OR v_new.state <> 'running'
     OR v_new.lease_owner IS NULL OR v_new.lease_until <= clock_timestamp()
     OR v_new.generation < 1 OR v_new.attempt < 1
  THEN RAISE EXCEPTION 'live mirror claim required' USING ERRCODE = 'OO010'; END IF;
  SELECT * INTO v_old FROM jobs.tasks WHERE id = p_task FOR UPDATE;
  IF NOT FOUND THEN
    INSERT INTO jobs.tasks (
      id, project_id, kind, payload_version, payload, config_version,
      artifact_digest, dedupe_key, state, run_at, attempt, max_attempts,
      lease_owner, lease_until, generation, last_error, created_at, updated_at
    ) VALUES (
      v_new.id, v_new.project_id, v_new.kind, v_new.payload_version, v_new.payload,
      v_new.config_version, v_new.artifact_digest, v_new.dedupe_key,
      v_new.state, v_new.run_at, v_new.attempt, v_new.max_attempts,
      v_new.lease_owner, v_new.lease_until, v_new.generation,
      v_new.last_error, v_new.created_at, v_new.updated_at
    );
  ELSIF v_old.project_id IS DISTINCT FROM v_new.project_id
     OR v_old.kind IS DISTINCT FROM v_new.kind
     OR v_old.payload_version IS DISTINCT FROM v_new.payload_version
     OR v_old.payload IS DISTINCT FROM v_new.payload
     OR v_old.config_version IS DISTINCT FROM v_new.config_version
     OR v_old.artifact_digest IS DISTINCT FROM v_new.artifact_digest
     OR v_old.dedupe_key IS DISTINCT FROM v_new.dedupe_key
     OR v_old.max_attempts IS DISTINCT FROM v_new.max_attempts
     OR v_old.created_at IS DISTINCT FROM v_new.created_at
     OR v_old.state <> 'running'
  THEN RAISE EXCEPTION 'mirror claim differs' USING ERRCODE = 'OO010';
  ELSIF v_new.generation > v_old.generation THEN
    PERFORM jobs.mirror_rebind(p_task, p_control);
  ELSIF v_new.generation = v_old.generation
    AND v_new.attempt = v_old.attempt
    AND v_new.lease_owner = v_old.lease_owner
  THEN
    UPDATE jobs.tasks SET lease_until = v_new.lease_until,
      run_at = v_new.run_at, last_error = v_new.last_error
    WHERE id = p_task;
  ELSE
    RAISE EXCEPTION 'mirror generation differs' USING ERRCODE = 'OO010';
  END IF;
  RETURN jsonb_build_object('task_id',p_task,'generation',v_new.generation,
    'attempt',v_new.attempt,'mirrored',true);
END;
$mirror$;
REVOKE ALL ON FUNCTION jobs.mirror_sync(uuid,jsonb) FROM PUBLIC;
DO $grant$ BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'oo_target_owner') THEN
    GRANT EXECUTE ON FUNCTION jobs.mirror_sync(uuid,jsonb) TO oo_target_owner;
  END IF;
END $grant$;
