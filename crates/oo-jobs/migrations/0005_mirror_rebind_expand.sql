-- Preserve the original task and audit events while a verified control claim
-- advances across more than one expired target mirror generation.
CREATE OR REPLACE FUNCTION jobs.enforce_task_update()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog, jobs
AS $$
BEGIN
  IF NEW.id IS DISTINCT FROM OLD.id
     OR NEW.project_id IS DISTINCT FROM OLD.project_id
     OR NEW.kind IS DISTINCT FROM OLD.kind
     OR NEW.payload_version IS DISTINCT FROM OLD.payload_version
     OR NEW.payload IS DISTINCT FROM OLD.payload
     OR NEW.config_version IS DISTINCT FROM OLD.config_version
     OR NEW.artifact_digest IS DISTINCT FROM OLD.artifact_digest
     OR NEW.dedupe_key IS DISTINCT FROM OLD.dedupe_key
     OR NEW.max_attempts IS DISTINCT FROM OLD.max_attempts
     OR NEW.created_at IS DISTINCT FROM OLD.created_at
  THEN
    RAISE EXCEPTION 'immutable task columns' USING ERRCODE = '23514';
  END IF;

  IF current_user = 'oo_jobs_owner'
     AND current_setting('jobs.mirror_rebind_task', true) = OLD.id::text
     AND OLD.state = 'running' AND NEW.state = 'running'
     AND OLD.lease_until <= clock_timestamp() AND NEW.lease_until > clock_timestamp()
     AND NEW.generation > OLD.generation
     AND NEW.attempt > OLD.attempt AND NEW.attempt <= OLD.max_attempts
  THEN
    NEW.updated_at := clock_timestamp();
    RETURN NEW;
  END IF;

  IF NEW.generation < OLD.generation THEN
    RAISE EXCEPTION 'generation decreased' USING ERRCODE = '23514';
  END IF;

  IF NEW.attempt < OLD.attempt THEN
    RAISE EXCEPTION 'attempt decreased' USING ERRCODE = '23514';
  END IF;

  IF NEW.lease_owner IS DISTINCT FROM OLD.lease_owner
     AND NEW.lease_owner IS NOT NULL
     AND NEW.generation <= OLD.generation THEN
    RAISE EXCEPTION 'lease owner change requires a new generation' USING ERRCODE = '23514';
  END IF;

  IF OLD.state IN ('succeeded', 'failed', 'cancelled')
     AND NEW.state IS DISTINCT FROM OLD.state THEN
    RAISE EXCEPTION 'terminal task state is final' USING ERRCODE = '23514';
  END IF;

  IF OLD.state = 'queued' AND NEW.state = 'running' THEN
    IF NEW.generation <= OLD.generation OR NEW.attempt <> OLD.attempt + 1 THEN
      RAISE EXCEPTION 'claim requires a new generation and the next attempt' USING ERRCODE = '23514';
    END IF;
  ELSIF OLD.state = 'queued' AND NEW.state = 'queued' THEN
    IF NEW.attempt <> OLD.attempt THEN
      RAISE EXCEPTION 'queued update keeps the attempt' USING ERRCODE = '23514';
    END IF;
  ELSIF OLD.state = 'queued' AND NEW.state = 'cancelled' THEN
    IF NEW.attempt <> OLD.attempt OR NEW.last_error IS NOT NULL THEN
      RAISE EXCEPTION 'cancel from queued keeps the attempt' USING ERRCODE = '23514';
    END IF;
  ELSIF OLD.state = 'queued' AND NEW.state IN ('succeeded', 'failed') THEN
    RAISE EXCEPTION 'queued task cannot complete without running' USING ERRCODE = '23514';
  ELSIF OLD.state = 'running' AND NEW.state = 'running' THEN
    IF NEW.lease_owner IS NOT DISTINCT FROM OLD.lease_owner
       AND NEW.generation IS NOT DISTINCT FROM OLD.generation THEN
      IF NEW.attempt <> OLD.attempt THEN
        RAISE EXCEPTION 'heartbeat keeps the attempt' USING ERRCODE = '23514';
      END IF;
    ELSIF NEW.lease_owner IS DISTINCT FROM OLD.lease_owner THEN
      IF NEW.generation <= OLD.generation OR NEW.attempt <> OLD.attempt THEN
        RAISE EXCEPTION 'reclaim keeps the attempt and requires a new generation' USING ERRCODE = '23514';
      END IF;
    ELSE
      RAISE EXCEPTION 'generation change while running keeps the owner' USING ERRCODE = '23514';
    END IF;
  ELSIF OLD.state = 'running' AND NEW.state = 'queued' THEN
    IF OLD.attempt >= OLD.max_attempts THEN
      RAISE EXCEPTION 'attempts are exhausted' USING ERRCODE = '23514';
    END IF;
    IF NEW.attempt <> OLD.attempt OR NEW.generation < OLD.generation THEN
      RAISE EXCEPTION 'retry keeps the attempt' USING ERRCODE = '23514';
    END IF;
  ELSIF OLD.state = 'running' AND NEW.state IN ('succeeded', 'failed', 'cancelled') THEN
    IF NEW.attempt <> OLD.attempt OR NEW.generation < OLD.generation THEN
      RAISE EXCEPTION 'completion keeps the attempt' USING ERRCODE = '23514';
    END IF;
  ELSIF NEW.state IS DISTINCT FROM OLD.state THEN
    RAISE EXCEPTION 'task transition is not legal' USING ERRCODE = '23514';
  END IF;

  NEW.updated_at := clock_timestamp();
  RETURN NEW;
END;
$$;

CREATE FUNCTION jobs.mirror_rebind(p_task uuid, p_control jsonb)
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
     OR v_new.run_at IS DISTINCT FROM v_old.run_at
     OR v_new.state <> 'running' OR v_new.lease_owner IS NULL
     OR v_new.lease_until <= clock_timestamp()
     OR v_new.generation <= v_old.generation
     OR v_new.attempt <= v_old.attempt OR v_new.attempt > v_old.max_attempts
  THEN RAISE EXCEPTION 'mirror claim differs or is stale' USING ERRCODE = 'OO010'; END IF;
  PERFORM set_config('jobs.mirror_rebind_task', p_task::text, true);
  UPDATE jobs.tasks SET generation = v_new.generation, attempt = v_new.attempt,
    lease_owner = v_new.lease_owner, lease_until = v_new.lease_until,
    last_error = v_new.last_error
  WHERE id = p_task;
  PERFORM set_config('jobs.mirror_rebind_task', '', true);
  RETURN jsonb_build_object('task_id', p_task, 'generation', v_new.generation,
    'attempt', v_new.attempt, 'rebound', true);
END;
$mirror$;
REVOKE ALL ON FUNCTION jobs.mirror_rebind(uuid,jsonb) FROM PUBLIC;
DO $grant$ BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'oo_target_owner') THEN
    GRANT USAGE ON SCHEMA jobs TO oo_target_owner;
    GRANT EXECUTE ON FUNCTION jobs.mirror_rebind(uuid,jsonb) TO oo_target_owner;
  END IF;
END $grant$;
