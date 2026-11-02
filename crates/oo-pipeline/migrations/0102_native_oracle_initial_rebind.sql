-- Explicitly cancelled, never-published initial work may adopt a new live job.
-- Preserve the old task/staging and record the identity transition atomically.
CREATE TABLE _oo.oracle_initial_rebind (
  source text NOT NULL,
  source_epoch integer NOT NULL,
  from_lease_id uuid NOT NULL,
  to_lease_id uuid NOT NULL,
  to_generation bigint NOT NULL,
  previous_instance jsonb NOT NULL,
  rebound_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (source, to_lease_id)
);
REVOKE ALL ON _oo.oracle_initial_rebind FROM PUBLIC, oo_pipeline_runtime;

ALTER FUNCTION _oo.native_source_instance_register(text,uuid,bigint,text,text,text,text,text,uuid)
  RENAME TO native_source_instance_register_before_initial_rebind;

CREATE FUNCTION _oo.native_source_instance_register(
 p_shard text,p_owner uuid,p_lease_generation bigint,p_source text,p_engine text,
 p_measured_id text,p_config_digest text,p_schema_digest text,p_lease uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE old _oo.source_instance%ROWTYPE; new_project uuid;
BEGIN
  IF p_engine='oracle' THEN
    PERFORM pg_advisory_xact_lock(hashtext('oo-source-instance'),hashtext(p_source));
    SELECT * INTO old FROM _oo.source_instance
      WHERE source=p_source AND measured_id=p_measured_id FOR UPDATE;
    IF FOUND AND old.lease_id IS DISTINCT FROM p_lease THEN
      SELECT t.project_id INTO new_project
      FROM jobs.tasks t JOIN jw_native.occurrences o ON o.task_id=t.id
      JOIN jw_native.bindings b ON b.id=o.binding_id
      JOIN jw_native.source_progress q ON q.source_id=b.source_id
        AND q.target_database=b.target_database AND q.active_task_id=t.id
      WHERE t.id=p_lease AND t.generation=p_lease_generation AND t.state='running'
        AND t.lease_until>clock_timestamp() AND t.kind='native_capture'
        AND b.method='C1' AND b.source_id=p_source AND b.id=p_owner
        AND b.project_id=t.project_id AND b.target_database=current_database()
        AND o.request IS NULL AND q.source_generation IS NULL AND q.task_id IS NULL
      FOR UPDATE OF b,q;
      IF new_project IS NULL OR old.engine IS DISTINCT FROM 'oracle'
        OR old.config_digest IS DISTINCT FROM p_config_digest
        OR old.schema_digest IS DISTINCT FROM p_schema_digest
        OR old.shard_id IS DISTINCT FROM p_shard
        OR NOT EXISTS(SELECT 1 FROM jobs.tasks t WHERE t.id=old.lease_id
          AND t.project_id=new_project AND t.state='cancelled')
        OR EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=p_source)
        OR EXISTS(SELECT 1 FROM _oo.full_import_run WHERE source_id=p_source)
        OR EXISTS(SELECT 1 FROM _oo.native_c1_baseline_typed WHERE source=p_source)
        OR EXISTS(SELECT 1 FROM jw_native.pending_acks WHERE task_id IN (old.lease_id,p_lease))
      THEN RAISE EXCEPTION 'oracle_initial_rebind_guard' USING ERRCODE='OO030'; END IF;
      INSERT INTO _oo.oracle_initial_rebind
        (source,source_epoch,from_lease_id,to_lease_id,to_generation,previous_instance)
        VALUES(p_source,old.source_epoch,old.lease_id,p_lease,p_lease_generation,to_jsonb(old));
      UPDATE _oo.source_instance SET owner_token=p_owner,lease_id=p_lease,
        lease_generation=p_lease_generation
        WHERE source=p_source AND source_epoch=old.source_epoch;
    END IF;
  END IF;
  RETURN _oo.native_source_instance_register_before_initial_rebind(
    p_shard,p_owner,p_lease_generation,p_source,p_engine,p_measured_id,p_config_digest,p_schema_digest,p_lease);
END $fn$;
REVOKE ALL ON FUNCTION _oo.native_source_instance_register_before_initial_rebind(
  text,uuid,bigint,text,text,text,text,text,uuid) FROM PUBLIC,oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.native_source_instance_register(
  text,uuid,bigint,text,text,text,text,text,uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_source_instance_register(
  text,uuid,bigint,text,text,text,text,text,uuid) TO oo_pipeline_runtime;
