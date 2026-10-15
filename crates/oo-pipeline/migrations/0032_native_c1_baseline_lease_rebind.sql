-- A retry retains the same immutable source cut and epoch. Only jobs.claim's
-- newer generation for this exact task may adopt its private baseline stage.
CREATE FUNCTION _oo.native_c1_baseline_rebind(p_source text,p_task uuid,p_generation bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $rebind$
DECLARE s _oo.native_c1_baseline_run%ROWTYPE; i _oo.source_instance%ROWTYPE;
BEGIN
 SELECT * INTO s FROM _oo.native_c1_baseline_run WHERE source=p_source AND task_id=p_task FOR UPDATE;
 IF NOT FOUND THEN RETURN NULL; END IF;
 SELECT * INTO i FROM _oo.source_instance WHERE source=p_source AND source_epoch=s.source_epoch FOR UPDATE;
 IF i.source IS NULL OR i.lease_id IS DISTINCT FROM p_task OR i.config_digest<>s.config_digest
  OR i.schema_digest<>s.schema_digest OR i.measured_id<>s.measured_id
  OR s.lease_generation>p_generation OR i.lease_generation>p_generation
  OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=p_task AND state='running'
   AND generation=p_generation AND lease_until>clock_timestamp())
  OR EXISTS(SELECT 1 FROM _oo.native_c1_baseline_typed WHERE source=p_source AND task_id=p_task AND acknowledged_at IS NOT NULL)
 THEN RAISE EXCEPTION 'baseline retry requires exact current jobs claim' USING ERRCODE='OO010'; END IF;
 UPDATE _oo.native_c1_baseline_run SET lease_generation=p_generation WHERE source=p_source AND task_id=p_task;
 UPDATE _oo.source_instance SET lease_generation=p_generation WHERE source=p_source AND source_epoch=s.source_epoch AND lease_id=p_task;
 RETURN jsonb_build_object('l0',s.l0,'l0_pos',_oo.native_pos_text(s.l0_pos),
  'applied_pos',_oo.native_pos_text(s.applied_pos),'l_end_pos',CASE WHEN s.l_end IS NULL THEN NULL ELSE _oo.native_pos_text(s.l_end) END,
  'l_end',CASE WHEN s.l_end IS NULL THEN NULL ELSE substring(s.measured_id from 7)||':1-'||_oo.native_pos_text(s.l_end) END,
  'state',s.state,'schema_digest',s.schema_digest,'config_digest',s.config_digest,'source_epoch',s.source_epoch);
END
$rebind$;
REVOKE ALL ON FUNCTION _oo.native_c1_baseline_rebind(text,uuid,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_c1_baseline_rebind(text,uuid,bigint) TO oo_pipeline_runtime;
