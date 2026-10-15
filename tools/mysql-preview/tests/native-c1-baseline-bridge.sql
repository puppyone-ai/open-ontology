\set ON_ERROR_STOP on
BEGIN;
SET LOCAL ROLE oo_target_owner;
\ir ../../../crates/oo-pipeline/migrations/0031_native_c1_baseline_typed_bridge.sql
RESET ROLE;
DO $test$
DECLARE
 source text:='bridge_rollback_test_20261015'; owner uuid:=gen_random_uuid(); project uuid:=gen_random_uuid();
 task jobs.lease_result; claimed jobs.lease_result; target_generation bigint; epoch integer; run uuid;
 physical text; idcol text; notecol text; instance jsonb; result jsonb; n bigint;
 cfg text:=repeat('a',64); typ text:=repeat('b',64); input text:=repeat('c',64); global_schema text:=repeat('d',64); table_schema text:=repeat('e',64);
 uuid text:='11111111-2222-4333-8444-555555555555'; rows jsonb; scope jsonb; entries jsonb;
BEGIN
 task:=jobs.enqueue(project,'bridge_rollback',1,'{}','test',repeat('f',64),'bridge',clock_timestamp(),3);
 claimed:=jobs.claim(project,ARRAY['bridge_rollback'],'bridge-test');
 IF claimed.status<>'claimed' OR claimed.task_id<>task.task_id THEN RAISE EXCEPTION 'fixture claim'; END IF;
 target_generation:=_oo.adopt_target_generation(source,owner);
 PERFORM _oo.full_import_bind_session(source,owner,target_generation);
 instance:=_oo.native_source_instance_register(source,owner,claimed.generation,source,'mysql','mysql.'||uuid,cfg,global_schema,claimed.task_id);
 epoch:=(instance->>'source_epoch')::integer;
 PERFORM _oo.native_c1_baseline_open(source,claimed.task_id,claimed.generation,epoch,'mysql.'||uuid,cfg,global_schema,uuid||':1-10',10);
 rows:='[{"schema":"s","table":"items","key":{"id":{"type":"int","value":"1"}},"op":"upsert","value":{"id":{"type":"int","value":"1"},"note":{"type":"text","value":"updated"}}},{"schema":"s","table":"items","key":{"id":{"type":"int","value":"2"}},"op":"delete","value":null},{"schema":"s","table":"items","key":{"id":{"type":"int","value":"3"}},"op":"upsert","value":{"id":{"type":"int","value":"3"},"note":{"type":"text","value":"inserted"}}}]';
 PERFORM _oo.native_c1_baseline_apply_txn(source,claimed.task_id,claimed.generation,epoch,11,uuid||':11',rows,repeat('f',64));
 rows:='[{"schema":"s","table":"items","key":{"id":{"type":"int","value":"1"}},"value":{"id":{"type":"int","value":"1"},"note":{"type":"text","value":"old"}}},{"schema":"s","table":"items","key":{"id":{"type":"int","value":"2"}},"value":{"id":{"type":"int","value":"2"},"note":{"type":"text","value":"deleted"}}},{"schema":"s","table":"items","key":{"id":{"type":"int","value":"4"}},"value":{"id":{"type":"int","value":"4"},"note":{"type":"text","value":"snapshot"}}}]';
 PERFORM _oo.native_c1_baseline_fill(source,claimed.task_id,claimed.generation,epoch,rows);
 PERFORM _oo.native_c1_baseline_seal(source,claimed.task_id,claimed.generation,epoch,uuid||':1-11',11);
 PERFORM _oo.register_physical_mapping(source,cfg,1,'namespace','s','','',_oo.pm_sha256(_oo.pm_payload(source,cfg,1,'n','s','','')));
 physical:=_oo.register_physical_mapping(source,cfg,1,'table','s','items','',_oo.pm_sha256(_oo.pm_payload(source,cfg,1,'t','s','items','')));
 idcol:=_oo.register_physical_mapping(source,cfg,1,'column','s','items','id',_oo.pm_sha256(_oo.pm_payload(source,cfg,1,'c','s','items','id')));
 notecol:=_oo.register_physical_mapping(source,cfg,1,'column','s','items','note',_oo.pm_sha256(_oo.pm_payload(source,cfg,1,'c','s','items','note')));
 physical:=_oo.lookup_physical_mapping(source,cfg,1,'table','s','items','');
 idcol:=_oo.lookup_physical_mapping(source,cfg,1,'column','s','items','id');
 notecol:=_oo.lookup_physical_mapping(source,cfg,1,'column','s','items','note');
 run:=_oo.full_import_open_run(source,owner,target_generation,source,1,cfg,typ,input,table_schema,ARRAY['s'],ARRAY['items'],ARRAY[physical]);
 PERFORM _oo.full_import_prepare_shadow(source,owner,target_generation,run,0,source,cfg,1,'s','items',ARRAY['id','note'],ARRAY[20::bigint,25::bigint],ARRAY[-1,-1],ARRAY[true,true]);
 EXECUTE format('INSERT INTO _oo.%I (_oo_pos,%I,%I) VALUES (11,1,''updated''),(11,3,''inserted''),(0,4,''snapshot'')',physical,idcol,notecol);
 PERFORM _oo.full_import_write_batch(source,owner,target_generation,run,0,source,physical,'baseline',repeat('f',64),cfg,typ,encode(sha256(convert_to('full-import/v1 hook none','UTF8')),'hex'),table_schema,input,11,3,0,ARRAY[]::text[],ARRAY[]::bigint[],ARRAY[]::integer[],ARRAY[]::bigint[],ARRAY[]::text[]);
 PERFORM _oo.full_import_note_table(source,owner,target_generation,run,0,3,repeat('f',64),source);
 SET LOCAL ROLE oo_pipeline_runtime;
 n:=_oo.native_c1_baseline_bind_table(source,claimed.task_id,claimed.generation,epoch,source,owner,target_generation,run,'s','items',ARRAY['id','note'],ARRAY['id']);
 RESET ROLE;
 IF n<>3 THEN RAISE EXCEPTION 'wrong rows'; END IF;
 EXECUTE format('SELECT count(*) FROM _oo.%I WHERE (%I=4 AND _oo_pos=0) OR (%I IN (1,3) AND _oo_pos=11)',physical,idcol,idcol) INTO n;
 IF n<>3 THEN RAISE EXCEPTION 'snapshot or CDC positions lost'; END IF;
 -- Both an altered value and an expired lease must refuse atomically.
 BEGIN
  UPDATE _oo.native_c1_baseline_row SET row_value=jsonb_set(row_value,'{note,value}','"wrong"'::jsonb)
    WHERE _oo.native_c1_baseline_row.source='bridge_rollback_test_20261015' AND task_id=claimed.task_id
    AND row_key->'id'->>'value'='1';
  PERFORM _oo.native_c1_baseline_bind_table(source,claimed.task_id,claimed.generation,epoch,source,owner,target_generation,run,'s','items',ARRAY['id','note'],ARRAY['id']);
  RAISE EXCEPTION 'expected typed mismatch';
 EXCEPTION WHEN SQLSTATE 'OO030' THEN NULL; END;
 BEGIN
  PERFORM _oo.native_c1_baseline_bind_table(source,claimed.task_id,claimed.generation+1,epoch,source,owner,target_generation,run,'s','items',ARRAY['id','note'],ARRAY['id']);
  RAISE EXCEPTION 'expected lease refusal';
 EXCEPTION WHEN SQLSTATE 'OO010' THEN NULL; END;
 PERFORM _oo.native_update_seal_quality(source,owner,target_generation,run,source,1,'s','items',physical,'baseline',repeat('f',64),11,'mysql-gtid/v1','mysql.'||uuid,'c1_cdc','native','T',true,ARRAY[]::text[],3,'never',0,'full');
 PERFORM _oo.full_import_seal_run(source,owner,target_generation,run,source,1,cfg,typ,input,table_schema,ARRAY['s'],ARRAY['items']);
 scope:=jsonb_build_object('config_digest',cfg,'schema_digest',global_schema,'lineage','mysql.'||uuid,'l0',uuid||':1-10','l_end',uuid||':1-11','complete',true,'consistency_grade','T','cross_table_consistent',true,'tables',jsonb_build_array(jsonb_build_object('schema','s','table','items','schema_sha256',table_schema)));
 entries:=jsonb_build_array(jsonb_build_object('schema','s','table','items','physical_id',physical,'run_id',run));
 SET LOCAL ROLE oo_pipeline_runtime;
 result:=_oo.native_c1_baseline_typed_commit(source,claimed.task_id,claimed.generation,epoch,target_generation,1,repeat('f',64),scope,entries);
 RESET ROLE;
 IF result->>'load_state'<>'awaiting_publish' OR result->>'acknowledged'<>'false' THEN RAISE EXCEPTION 'bad typed receipt'; END IF;
 IF NOT EXISTS(SELECT 1 FROM _oo.source_watermark w WHERE w.source='bridge_rollback_test_20261015' AND w.source_epoch=epoch) THEN RAISE EXCEPTION 'epoch missing'; END IF;
 BEGIN
  PERFORM _oo.native_c1_baseline_ack(source,claimed.task_id,repeat('f',64));
  RAISE EXCEPTION 'expected pre-CAS ACK refusal';
 EXCEPTION WHEN SQLSTATE 'OO010' THEN NULL; END;
 task:=jobs.complete_success(project,claimed.task_id,'bridge-test',claimed.generation);
 IF task.status<>'succeeded' THEN RAISE EXCEPTION 'fixture completion CAS failed'; END IF;
 SET LOCAL ROLE oo_pipeline_runtime;
 result:=_oo.native_c1_baseline_ack(source,claimed.task_id,repeat('f',64));
 RESET ROLE;
 IF result->>'acknowledged'<>'true' THEN RAISE EXCEPTION 'post-CAS ACK failed'; END IF;
 RAISE NOTICE 'bridge positive, typed mismatch, wrong lease, row positions and tombstone tests passed';
END
$test$;
ROLLBACK;
