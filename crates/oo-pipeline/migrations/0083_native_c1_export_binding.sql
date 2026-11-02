-- Export metadata through the same least-privilege, live-lease boundary as
-- typed rows. The runtime never receives SELECT on private baseline tables.
CREATE FUNCTION _oo.c1_typed_export_binding(s text,t uuid,g bigint,e integer)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE r _oo.native_c1_baseline_run%ROWTYPE;
BEGIN
 PERFORM _oo.native_c1_baseline_lock(s,t);
 SELECT * INTO r FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
 IF NOT FOUND OR r.state<>'awaiting_publish' OR r.source_epoch<>e OR r.lease_generation<>g
  OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=t AND generation=g AND state='running' AND lease_until>clock_timestamp()) THEN
  RAISE EXCEPTION 'live typed export lease required' USING ERRCODE='OO010';
 END IF;
 RETURN jsonb_build_object('engine',r.engine,'l0',r.l0,'l_end',_oo.native_pos_text(r.l_end),
  'measured_id',r.measured_id,'config_digest',r.config_digest,'schema_digest',r.schema_digest,
  'oracle_buffer',CASE WHEN r.engine='oracle' THEN _oo.c1_oracle_buffer_summary(s,t) ELSE NULL END);
END $fn$;
REVOKE ALL ON FUNCTION _oo.c1_typed_export_binding(text,uuid,bigint,integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c1_typed_export_binding(text,uuid,bigint,integer) TO oo_pipeline_runtime;
