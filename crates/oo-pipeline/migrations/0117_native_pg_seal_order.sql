-- PG-only order correction. Preserve 0042 format guard and all 0116 proof checks.
CREATE OR REPLACE FUNCTION _oo.pg_current_seal(s text,t uuid,g bigint,p_end numeric)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE r _oo.pg_current_run%ROWTYPE; entries jsonb; n bigint; last_lsn numeric; v_result jsonb;
BEGIN
 PERFORM _oo.native_c1_baseline_v2_assert(s,t,g,1);
 SELECT * INTO STRICT r FROM _oo.pg_current_run WHERE source=s AND task=t;
 SELECT count(*),max(end_lsn) INTO n,last_lsn FROM _oo.pg_current_tx WHERE source=s;
 IF n<>jsonb_array_length(r.boundary->'transactions') OR last_lsn IS DISTINCT FROM p_end
  OR EXISTS(SELECT 1 FROM _oo.c1_typed_table WHERE source=s AND task=t AND NOT snapshot_complete)
  OR (SELECT count(*) FROM _oo.c1_typed_table WHERE source=s AND task=t)<>jsonb_array_length(r.tables)
 THEN RAISE EXCEPTION 'PG capture incomplete' USING ERRCODE='OO030'; END IF;
 SELECT jsonb_agg(jsonb_build_object('name',name,'physical',physical,'spec',spec) ORDER BY name) INTO entries FROM _oo.c1_typed_table WHERE source=s AND task=t;
 v_result:=jsonb_build_object('protocol','oo.pipeline.pg-current-result/v1','load_state','staged',
  'source_id',s,'task',t,'bundle_sha256',r.bundle_sha,'source_identity',r.source_identity,'source_generation',1,
  'end_lsn',r.boundary->>'end_lsn','applied_pos',p_end::text,'transaction_count',n,'entries',entries,'acknowledged',false,'replay',false);
 UPDATE _oo.pg_current_run SET result=v_result, sealed_at=clock_timestamp() WHERE source=s;
 UPDATE _oo.native_c1_baseline_v2_format SET sealed=true WHERE source=s AND task_id=t;
 UPDATE _oo.native_c1_baseline_run SET state='awaiting_publish',l_end=p_end,applied_pos=p_end,sealed_at=clock_timestamp() WHERE source=s AND task_id=t;
 RETURN v_result;
END $f$;
