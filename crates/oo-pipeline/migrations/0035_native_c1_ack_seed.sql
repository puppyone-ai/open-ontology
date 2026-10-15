-- Subsequent C1 cuts start from the exact acknowledged private source mirror.
-- The source reader then applies ordered binlog transactions, without a new snapshot.
CREATE FUNCTION _oo.native_c1_ack_seed(p_source text,p_task uuid,p_generation bigint,p_epoch integer,p_previous uuid,p_source_generation bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $seed$
DECLARE s _oo.native_c1_baseline_run%ROWTYPE; i _oo.source_instance%ROWTYPE;
 receipt jsonb; prior_task jobs.tasks%ROWTYPE; current_task jobs.tasks%ROWTYPE; result jsonb;
BEGIN
 SELECT * INTO s FROM _oo.native_c1_baseline_run WHERE source=p_source AND task_id=p_previous;
 SELECT t.receipt INTO receipt FROM _oo.native_c1_baseline_typed t
  WHERE t.source=p_source AND t.task_id=p_previous AND t.acknowledged_at IS NOT NULL;
 SELECT * INTO prior_task FROM jobs.tasks WHERE id=p_previous;
 SELECT * INTO current_task FROM jobs.tasks WHERE id=p_task;
 SELECT * INTO i FROM _oo.source_instance WHERE source=p_source AND source_epoch=p_epoch FOR UPDATE;
 IF receipt IS NULL OR s.source IS NULL OR i.source IS NULL OR p_task=p_previous
  OR prior_task.state IS DISTINCT FROM 'succeeded' OR current_task.state IS DISTINCT FROM 'running'
  OR prior_task.project_id IS DISTINCT FROM current_task.project_id
  OR current_task.generation IS DISTINCT FROM p_generation OR current_task.lease_until<=clock_timestamp()
  OR s.state<>'awaiting_publish' OR s.applied_pos<>s.l_end OR s.source_epoch<>p_epoch
  OR (receipt->>'source_generation')::bigint IS DISTINCT FROM p_source_generation
  OR i.measured_id<>s.measured_id OR i.config_digest<>s.config_digest OR i.schema_digest<>s.schema_digest
  OR i.lease_id NOT IN (p_previous,p_task)
  OR NOT EXISTS(SELECT 1 FROM _oo.source_watermark w WHERE w.source=p_source AND w.source_epoch=p_epoch
   AND w.source_generation=p_source_generation AND w.applied_pos=s.l_end
   AND w.target_generation=(receipt->>'target_generation')::bigint
   AND w.position_codec='mysql-gtid/v1' AND w.position_lineage=s.measured_id)
 THEN RAISE EXCEPTION 'C1 seed requires exact acknowledged predecessor and live successor' USING ERRCODE='OO010'; END IF;
 UPDATE _oo.source_instance SET lease_id=p_task,lease_generation=p_generation
  WHERE source=p_source AND source_epoch=p_epoch;
 result:=_oo.native_c1_baseline_open(p_source,p_task,p_generation,p_epoch,s.measured_id,s.config_digest,s.schema_digest,
  substring(s.measured_id from 7)||':1-'||_oo.native_pos_text(s.l_end),s.l_end);
 INSERT INTO _oo.native_c1_baseline_row(source,task_id,logical_schema,logical_table,row_key,row_value,deleted,applied_pos,txn_id)
  SELECT source,p_task,logical_schema,logical_table,row_key,row_value,deleted,applied_pos,txn_id
  FROM _oo.native_c1_baseline_row WHERE source=p_source AND task_id=p_previous
  ON CONFLICT DO NOTHING;
 RETURN result || jsonb_build_object('seeded',true,'previous_task',p_previous);
END
$seed$;
REVOKE ALL ON FUNCTION _oo.native_c1_ack_seed(text,uuid,bigint,integer,uuid,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_c1_ack_seed(text,uuid,bigint,integer,uuid,bigint) TO oo_pipeline_runtime;
