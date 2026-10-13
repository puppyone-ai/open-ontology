-- A reclaimed jobs task keeps its UUID and immutable payload. Rebind only an
-- unacknowledged seal to the newly mirrored live generation; COPY never runs.
CREATE FUNCTION _oo.native_partition_rebind(
  p_source text, p_schema text, p_table text, p_partition text,
  p_scope jsonb, p_run uuid, p_batch text, p_payload text,
  p_target_generation bigint, p_task uuid, p_job_generation bigint
) RETURNS text LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $np$
DECLARE
  v_state _oo.native_partition_state%ROWTYPE;
BEGIN
  SELECT state.* INTO v_state FROM _oo.native_partition_state AS state
  WHERE state.source = p_source AND state.schema_name = p_schema
    AND state.table_name = p_table AND state.partition_id = p_partition
  FOR UPDATE;
  IF NOT FOUND OR v_state.partition_scope IS DISTINCT FROM p_scope
    OR v_state.run_id IS DISTINCT FROM p_run
    OR v_state.batch_id IS DISTINCT FROM p_batch
    OR v_state.payload_digest IS DISTINCT FROM p_payload
    OR v_state.target_generation IS DISTINCT FROM p_target_generation
    OR v_state.job_task_id IS DISTINCT FROM p_task
    OR v_state.source_generation IS DISTINCT FROM (p_scope->>'source_generation')::bigint
  THEN RAISE EXCEPTION 'partition replay identity conflict' USING ERRCODE = 'OO010'; END IF;
  IF v_state.acknowledged THEN
    IF v_state.job_generation IS DISTINCT FROM p_job_generation
    THEN RAISE EXCEPTION 'partition already acknowledged' USING ERRCODE = 'OO010'; END IF;
    RETURN 'acknowledged';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM jobs.tasks AS task
    WHERE task.id = p_task AND task.state = 'running'
      AND task.generation = p_job_generation
      AND task.lease_until > clock_timestamp())
  THEN RAISE EXCEPTION 'partition replay lease lost' USING ERRCODE = 'OO010'; END IF;
  IF p_job_generation = v_state.job_generation THEN RETURN 'replay'; END IF;
  IF p_job_generation <= v_state.job_generation
  THEN RAISE EXCEPTION 'partition replay generation stale' USING ERRCODE = 'OO010'; END IF;
  UPDATE _oo.native_partition_state SET job_generation = p_job_generation
  WHERE source = p_source AND schema_name = p_schema
    AND table_name = p_table AND partition_id = p_partition;
  RETURN 'rebound';
END;
$np$;
REVOKE ALL ON FUNCTION _oo.native_partition_rebind(text,text,text,text,jsonb,uuid,text,text,bigint,uuid,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_partition_rebind(text,text,text,text,jsonb,uuid,text,text,bigint,uuid,bigint) TO oo_pipeline_runtime;
