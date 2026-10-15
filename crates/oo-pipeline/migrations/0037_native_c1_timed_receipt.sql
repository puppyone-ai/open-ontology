-- New cuts bind the real binlog XID timestamp in the same transaction as CDC.
-- Existing immutable snapshot receipts and historical freshness are not changed.
CREATE TABLE _oo.native_c1_baseline_txn_time (
 source text NOT NULL, task_id uuid NOT NULL, applied_pos numeric(38,0) NOT NULL,
 source_committed_at timestamptz NOT NULL,
 PRIMARY KEY(source,task_id,applied_pos),
 FOREIGN KEY(source,task_id,applied_pos) REFERENCES _oo.native_c1_baseline_txn(source,task_id,applied_pos)
);
REVOKE ALL ON _oo.native_c1_baseline_txn_time FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.native_c1_baseline_apply_timed_txn(
 p_source text,p_task uuid,p_generation bigint,p_epoch integer,p_pos numeric,
 p_txn_id text,p_rows jsonb,p_payload_sha256 text,p_source_commit_timestamp text
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $timed$
DECLARE receipt jsonb; stamp timestamptz; old timestamptz;
BEGIN
 IF p_source_commit_timestamp IS NULL OR p_source_commit_timestamp !~ '^20[0-9]{2}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]{1,6})?Z$'
 THEN RAISE EXCEPTION 'binlog UTC commit timestamp required' USING ERRCODE='OO030'; END IF;
 stamp:=p_source_commit_timestamp::timestamptz;
 receipt:=_oo.native_c1_baseline_apply_txn(p_source,p_task,p_generation,p_epoch,p_pos,p_txn_id,p_rows,p_payload_sha256);
 SELECT source_committed_at INTO old FROM _oo.native_c1_baseline_txn_time
  WHERE source=p_source AND task_id=p_task AND applied_pos=p_pos;
 IF FOUND AND old IS DISTINCT FROM stamp THEN
  RAISE EXCEPTION 'C1 commit timestamp replay conflict' USING ERRCODE='OO030';
 END IF;
 INSERT INTO _oo.native_c1_baseline_txn_time VALUES(p_source,p_task,p_pos,stamp) ON CONFLICT DO NOTHING;
 RETURN receipt||jsonb_build_object('source_committed_at',stamp);
END
$timed$;
REVOKE ALL ON FUNCTION _oo.native_c1_baseline_apply_timed_txn(text,uuid,bigint,integer,numeric,text,jsonb,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_c1_baseline_apply_timed_txn(text,uuid,bigint,integer,numeric,text,jsonb,text,text) TO oo_pipeline_runtime;

CREATE OR REPLACE FUNCTION _oo.native_c1_baseline_typed_commit(
 p_source text,p_task uuid,p_lease_generation bigint,p_epoch integer,
 p_target_generation bigint,p_source_generation bigint,p_request_digest text,p_scope jsonb,p_entries jsonb
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $bridge$
DECLARE s _oo.native_c1_baseline_run%ROWTYPE; i _oo.source_instance%ROWTYPE;
 e jsonb; r _oo.full_import_run%ROWTYPE; receipt jsonb; committed timestamptz; n integer; prior_key text:='';
BEGIN
 SELECT * INTO s FROM _oo.native_c1_baseline_run WHERE source=p_source AND task_id=p_task FOR UPDATE;
 SELECT source_committed_at INTO committed FROM _oo.native_c1_baseline_txn_time
  WHERE source=p_source AND task_id=p_task AND applied_pos=s.l_end;
 IF committed IS NULL THEN RAISE EXCEPTION 'actual L_end binlog commit time required' USING ERRCODE='OO030'; END IF;
 SELECT * INTO i FROM _oo.source_instance WHERE source=p_source AND source_epoch=p_epoch;
 IF s.source IS NULL OR i.source IS NULL OR s.state<>'awaiting_publish' OR s.l_end<>s.applied_pos
  OR s.source_epoch<>p_epoch OR s.lease_generation<>p_lease_generation
  OR i.lease_id<>p_task OR i.lease_generation<>p_lease_generation
  OR s.config_digest<>i.config_digest OR s.schema_digest<>i.schema_digest
  OR (p_scope->>'config_digest') IS DISTINCT FROM s.config_digest OR (p_scope->>'schema_digest') IS DISTINCT FROM s.schema_digest
  OR (p_scope->>'lineage') IS DISTINCT FROM s.measured_id OR (p_scope->>'l0') IS DISTINCT FROM s.l0
  OR (p_scope->>'l_end') IS DISTINCT FROM (substring(s.measured_id from 7)||':1-'||_oo.native_pos_text(s.l_end))
  OR (p_scope->>'complete') IS DISTINCT FROM 'true' OR (p_scope->>'consistency_grade') IS DISTINCT FROM 'T'
  OR (p_scope->>'cross_table_consistent') IS DISTINCT FROM 'true'
  OR p_request_digest IS NULL OR p_request_digest !~ '^[0-9a-f]{64}$'
  OR jsonb_typeof(p_entries) IS DISTINCT FROM 'array' OR jsonb_array_length(p_entries)<1
  OR jsonb_typeof(p_scope->'tables') IS DISTINCT FROM 'array'
  OR jsonb_array_length(p_entries) IS DISTINCT FROM jsonb_array_length(p_scope->'tables')
 THEN RAISE EXCEPTION 'baseline complete source identity' USING ERRCODE='OO010'; END IF;
 IF NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=p_task AND generation=p_lease_generation
    AND state='running' AND lease_until>clock_timestamp())
 THEN RAISE EXCEPTION 'live baseline lease required' USING ERRCODE='OO010'; END IF;
 FOR e IN SELECT value FROM jsonb_array_elements(p_entries) LOOP
  IF e->>'schema'||'.'||(e->>'table')<=prior_key OR NOT EXISTS(
   SELECT 1 FROM jsonb_array_elements(p_scope->'tables') t
   WHERE t->>'schema'=e->>'schema' AND t->>'table'=e->>'table')
  THEN RAISE EXCEPTION 'baseline table vector' USING ERRCODE='OO030'; END IF;
  prior_key:=e->>'schema'||'.'||(e->>'table');
  SELECT * INTO r FROM _oo.full_import_run WHERE run_id=(e->>'run_id')::uuid;
  IF NOT FOUND OR r.source_id<>p_source OR r.source_generation<>p_source_generation
   OR r.target_generation<>p_target_generation OR r.config_digest<>i.config_digest
   OR NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p_scope->'tables') t WHERE t->>'schema'=e->>'schema'
     AND t->>'table'=e->>'table' AND t->>'schema_sha256'=r.schema_digest)
   OR r.owner_token<>i.owner_token OR r.shard_id<>i.shard_id OR r.state<>'awaiting_publish'
   OR NOT EXISTS(SELECT 1 FROM _oo.full_import_table t WHERE t.run_id=r.run_id
    AND t.schema_ident=e->>'schema' AND t.table_ident=e->>'table' AND t.physical_name=e->>'physical_id')
  THEN RAISE EXCEPTION 'baseline typed receipt' USING ERRCODE='OO010'; END IF;
  PERFORM set_config('_oo.writer','apply-receipt',true);
  INSERT INTO _oo.apply_receipts(source,table_generation,txn_or_batch_id,config_version,config_digest,
   type_policy_version,type_digest,hook_digest,schema_digest,payload_digest,protocol_version,
   position_codec,position_lineage,source_epoch,applied_pos,partial_txn,cross_table_consistent,
   txn_complete,segment_index,segment_count,segment_bitmap,fence_shard,fence_generation)
  SELECT source,table_generation,'native-c1:'||p_task::text,config_version,config_digest,
   type_policy_version,type_digest,hook_digest,schema_digest,payload_digest,protocol_version,
   'mysql-gtid/v1',s.measured_id,p_epoch,s.l_end,false,true,true,NULL,NULL,segment_bitmap,fence_shard,fence_generation
  FROM _oo.apply_receipts WHERE source=p_source AND table_generation=e->>'physical_id'
   AND config_digest=i.config_digest AND schema_digest=r.schema_digest
   AND fence_shard=i.shard_id AND fence_generation=p_target_generation
   AND applied_pos=s.l_end AND position_codec='snapshot-zero/v1' AND NOT partial_txn AND txn_complete;
  GET DIAGNOSTICS n=ROW_COUNT;
  IF n<>1 THEN RAISE EXCEPTION 'exact typed snapshot receipt required' USING ERRCODE='OO030'; END IF;
  UPDATE _oo.table_freshness SET source_committed_at=committed
   WHERE source=p_source AND source_generation=p_source_generation AND target_generation=p_target_generation
    AND physical_name=e->>'physical_id' AND schema_name=e->>'schema' AND table_name=e->>'table'
    AND state='awaiting_publish' AND last_pos=s.l_end;
  GET DIAGNOSTICS n=ROW_COUNT;
  IF n<>1 THEN RAISE EXCEPTION 'exact current freshness required' USING ERRCODE='OO030'; END IF;
 END LOOP;
 IF NOT EXISTS(SELECT 1 FROM _oo.source_watermark w WHERE w.source=p_source
   AND w.applied_pos=s.l_end AND w.target_generation=p_target_generation
   AND w.source_generation=p_source_generation AND w.position_codec='mysql-gtid/v1'
   AND w.position_lineage=s.measured_id AND w.consistency_grade='T' AND w.cross_table_consistent)
 THEN RAISE EXCEPTION 'baseline watermark barrier' USING ERRCODE='OO010'; END IF;
 UPDATE _oo.source_watermark SET source_epoch=p_epoch,source_committed_at=committed WHERE source=p_source;
 receipt:=jsonb_build_object('protocol','oo.pipeline.native-c1-baseline-result/v1',
  'request_digest',p_request_digest,'source_id',p_source,'source_generation',p_source_generation,'target_generation',p_target_generation,
  'source_committed_at',committed,'load_state','awaiting_publish','acknowledged',false,'l0',s.l0,'l_end',p_scope->>'l_end',
  'applied_pos',_oo.native_pos_text(s.l_end),'source_epoch',p_epoch,'entries',p_entries,
  'source_instance',jsonb_build_object('protocol','oo.pipeline.native-source-instance/v1',
   'source',p_source,'engine',i.engine,'measured_id',i.measured_id,'source_epoch',i.source_epoch,
   'config_digest',i.config_digest,'schema_digest',i.schema_digest,'shard_id',i.shard_id,
   'owner_token',i.owner_token,'lease_id',i.lease_id));
 INSERT INTO _oo.native_c1_baseline_typed(source,task_id,request_digest,receipt) VALUES(p_source,p_task,p_request_digest,receipt);
 RETURN receipt;
END
$bridge$;
REVOKE ALL ON FUNCTION _oo.native_c1_baseline_typed_commit(text,uuid,bigint,integer,bigint,bigint,text,jsonb,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_c1_baseline_typed_commit(text,uuid,bigint,integer,bigint,bigint,text,jsonb,jsonb) TO oo_pipeline_runtime;

