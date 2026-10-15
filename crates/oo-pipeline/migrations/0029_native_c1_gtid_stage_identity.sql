-- Bind the private bootstrap stage positions to the registered MySQL UUID.
-- 0028 is immutable; this successor tightens its open/apply/seal boundary.
CREATE OR REPLACE FUNCTION _oo.native_c1_baseline_open(
  p_source text, p_task uuid, p_generation bigint, p_epoch integer,
  p_measured_id text, p_config_digest text, p_schema_digest text,
  p_l0 text, p_l0_pos numeric
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $c1$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE;
BEGIN
  IF p_measured_id !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
     OR p_l0 !~ ('^' || p_measured_id || ':[1-9][0-9]*-[1-9][0-9]*$')
     OR split_part(p_l0, ':', 2) !~ ('^[1-9][0-9]*-' || _oo.native_pos_text(p_l0_pos) || '$') THEN
    RAISE EXCEPTION 'MySQL L0 must be the measured server UUID GTID set and end at L0 position' USING ERRCODE='OO030';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM jobs.tasks t WHERE t.id=p_task AND t.state='running'
      AND t.generation=p_generation AND t.lease_until>clock_timestamp())
     OR NOT EXISTS (SELECT 1 FROM _oo.source_instance i WHERE i.source=p_source
       AND i.source_epoch=p_epoch AND i.engine='mysql' AND i.measured_id=p_measured_id
       AND i.config_digest=p_config_digest AND i.schema_digest=p_schema_digest
       AND i.lease_id=p_task AND i.lease_generation=p_generation) THEN
    RAISE EXCEPTION 'C1 baseline requires live lease and registered epoch' USING ERRCODE='OO010';
  END IF;
  INSERT INTO _oo.native_c1_baseline_run(source,task_id,lease_generation,source_epoch,engine,
    measured_id,config_digest,schema_digest,l0,l0_pos,applied_pos,state)
  VALUES(p_source,p_task,p_generation,p_epoch,'mysql',p_measured_id,p_config_digest,
    p_schema_digest,p_l0,p_l0_pos,p_l0_pos,'open')
  ON CONFLICT(source,task_id) DO NOTHING;
  SELECT * INTO v FROM _oo.native_c1_baseline_run WHERE source=p_source AND task_id=p_task FOR UPDATE;
  IF v.lease_generation<>p_generation OR v.source_epoch<>p_epoch OR v.measured_id<>p_measured_id
     OR v.config_digest<>p_config_digest OR v.schema_digest<>p_schema_digest
     OR v.l0<>p_l0 OR v.l0_pos<>p_l0_pos THEN
    RAISE EXCEPTION 'C1 baseline replay identity conflict' USING ERRCODE='OO030';
  END IF;
  RETURN jsonb_build_object('protocol','oo.pipeline.native-c1-baseline-stage/v1',
    'outcome',CASE WHEN v.state='open' THEN 'open' ELSE 'replay' END,
    'source',p_source,'task_id',p_task,'source_epoch',p_epoch,'measured_id',p_measured_id,
    'l0',v.l0,'applied_pos',v.applied_pos);
END
$c1$;

CREATE OR REPLACE FUNCTION _oo.native_c1_baseline_apply_txn(
  p_source text,p_task uuid,p_generation bigint,p_epoch integer,
  p_pos numeric,p_txn_id text,p_rows jsonb,p_payload_sha256 text
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $c1$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE; e jsonb; n integer:=0; old _oo.native_c1_baseline_txn%ROWTYPE;
BEGIN
  v:=_oo.native_c1_baseline_assert_lease(p_source,p_task,p_generation,p_epoch);
  IF p_pos<=v.l0_pos OR p_txn_id<>v.measured_id || ':' || _oo.native_pos_text(p_pos)
     OR jsonb_typeof(p_rows)<>'array' OR p_payload_sha256 !~ '^[0-9a-f]{64}$' THEN
    RAISE EXCEPTION 'invalid measured MySQL GTID transaction identity' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO old FROM _oo.native_c1_baseline_txn WHERE source=p_source AND task_id=p_task AND applied_pos=p_pos;
  IF FOUND THEN
    IF old.txn_id=p_txn_id AND old.payload_sha256=p_payload_sha256 THEN
      RETURN jsonb_build_object('outcome','replay','applied_pos',p_pos,'txn_id',p_txn_id);
    END IF;
    RAISE EXCEPTION 'C1 CDC position conflict' USING ERRCODE='OO030';
  END IF;
  IF p_pos<>v.applied_pos+1 THEN
    RAISE EXCEPTION 'MySQL GTID transaction is not the next measured UUID sequence' USING ERRCODE='OO030';
  END IF;
  INSERT INTO _oo.native_c1_baseline_txn VALUES(p_source,p_task,p_pos,p_txn_id,p_payload_sha256,clock_timestamp());
  FOR e IN SELECT value FROM jsonb_array_elements(p_rows) LOOP
    IF e->>'schema' IS NULL OR e->>'table' IS NULL OR jsonb_typeof(e->'key') IS NULL
       OR e->>'op' NOT IN ('upsert','delete') OR (e->>'op'='upsert' AND jsonb_typeof(e->'value')<>'object') THEN
      RAISE EXCEPTION 'invalid C1 CDC row' USING ERRCODE='OO030';
    END IF;
    INSERT INTO _oo.native_c1_baseline_row(source,task_id,logical_schema,logical_table,row_key,row_value,deleted,applied_pos,txn_id)
    VALUES(p_source,p_task,e->>'schema',e->>'table',e->'key',CASE WHEN e->>'op'='delete' THEN NULL ELSE e->'value' END,
      e->>'op'='delete',p_pos,p_txn_id)
    ON CONFLICT(source,task_id,logical_schema,logical_table,row_key) DO UPDATE SET
      row_value=EXCLUDED.row_value,deleted=EXCLUDED.deleted,applied_pos=EXCLUDED.applied_pos,txn_id=EXCLUDED.txn_id
      WHERE _oo.native_c1_baseline_row.applied_pos<EXCLUDED.applied_pos;
    n:=n+1;
  END LOOP;
  UPDATE _oo.native_c1_baseline_run SET applied_pos=p_pos WHERE source=p_source AND task_id=p_task;
  RETURN jsonb_build_object('outcome','applied','applied_pos',p_pos,'txn_id',p_txn_id,'events',n);
END
$c1$;

CREATE OR REPLACE FUNCTION _oo.native_c1_baseline_seal(
  p_source text,p_task uuid,p_generation bigint,p_epoch integer,p_l_end text,p_l_end_pos numeric
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $c1$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE;
BEGIN
  v:=_oo.native_c1_baseline_assert_lease(p_source,p_task,p_generation,p_epoch);
  IF p_l_end !~ ('^' || v.measured_id || ':[1-9][0-9]*-[1-9][0-9]*$')
     OR split_part(p_l_end, ':', 2) !~ ('^[1-9][0-9]*-' || _oo.native_pos_text(p_l_end_pos) || '$')
     OR p_l_end_pos<v.l0_pos OR p_l_end_pos<>v.applied_pos THEN
    RAISE EXCEPTION 'C1 final barrier is not the measured GTID set applied through L_end' USING ERRCODE='OO010';
  END IF;
  UPDATE _oo.native_c1_baseline_run SET l_end=p_l_end_pos,state='awaiting_publish',sealed_at=clock_timestamp()
    WHERE source=p_source AND task_id=p_task;
  RETURN jsonb_build_object('protocol','oo.pipeline.native-c1-baseline-stage/v1','outcome','sealed',
    'source',p_source,'task_id',p_task,'source_epoch',p_epoch,'measured_id',v.measured_id,
    'l0',v.l0,'l_end',p_l_end,'applied_pos',v.applied_pos,
    'rows',(SELECT count(*) FROM _oo.native_c1_baseline_row WHERE source=p_source AND task_id=p_task),
    'deleted',(SELECT count(*) FROM _oo.native_c1_baseline_row WHERE source=p_source AND task_id=p_task AND deleted));
END
$c1$;
