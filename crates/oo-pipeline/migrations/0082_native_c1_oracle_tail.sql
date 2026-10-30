-- Extend existing C1 seal/commit/publication. Ordinals are buffer positions, never SCNs.
CREATE OR REPLACE FUNCTION _oo.native_position_closed(
  p_codec text,
  p_lineage text,
  p_pos numeric,
  p_epoch integer
)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
SET search_path = pg_catalog
AS $$
  SELECT CASE
    WHEN p_codec = 'oracle-capture-sequence/v1' THEN
      p_lineage ~ '^oracle\.[1-9][0-9]{0,19}\.(0|[1-9][0-9]{0,19})\.[0-9a-f]{32}$'
      AND p_pos IS NOT NULL AND p_pos >= 0 AND p_pos = trunc(p_pos) AND p_pos <= 1000000
      AND (p_epoch IS NULL OR p_epoch >= 0)
    WHEN p_codec IN (
      'pg-commit-lsn/v1',
      'mysql-binlog-pos/v1',
      'batch-cursor-us/v1',
      'snapshot-zero/v1',
      'partition-signature/v1',
      'mssql-ct-version/v1'
    ) THEN true
    WHEN p_codec = 'mariadb-gtid/v1'
      AND p_lineage ~ '^d(0|[1-9][0-9]{0,9})\.s([1-9][0-9]{0,9})$'
      AND substring(p_lineage from '^d([0-9]+)')::numeric <= 4294967295
      AND substring(p_lineage from '\.s([0-9]+)$')::numeric BETWEEN 1 AND 4294967295
      AND p_pos IS NOT NULL
      AND _oo.native_pos_text(p_pos) ~ '^(0|[1-9][0-9]{0,19})$'
      AND (p_epoch IS NULL OR p_epoch >= 0)
    THEN true
    WHEN p_codec = 'mysql-gtid/v1'
      AND p_lineage ~ '^mysql\.[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
      AND p_pos IS NOT NULL
      AND _oo.native_pos_text(p_pos) ~ '^[1-9][0-9]{0,19}$'
      AND (p_epoch IS NULL OR p_epoch >= 0)
    THEN true
    WHEN p_codec = 'mongo-cluster-time/v1'
      AND p_lineage ~ '^[a-z][a-z0-9_.:-]{0,127}$'
      AND position(',' in p_lineage) = 0
      AND p_pos IS NOT NULL
      AND _oo.native_pos_text(p_pos) ~ '^[1-9][0-9]{0,37}$'
      AND (p_epoch IS NULL OR p_epoch >= 0)
    THEN true
    ELSE false
  END;
$$;

ALTER TABLE _oo.source_watermark DROP CONSTRAINT source_watermark_codec_check;
ALTER TABLE _oo.source_watermark ADD CONSTRAINT source_watermark_codec_check CHECK (
 position_codec='partition-signature/v1' OR
 (source_epoch IS NULL AND position_codec IN ('pg-commit-lsn/v1','mysql-binlog-pos/v1','batch-cursor-us/v1','snapshot-zero/v1','mssql-ct-version/v1')) OR
 (position_codec IN ('mariadb-gtid/v1','mysql-gtid/v1','mongo-cluster-time/v1','oracle-capture-sequence/v1')
  AND _oo.native_position_closed(position_codec,position_lineage,applied_pos,COALESCE(source_epoch,0))));
CREATE FUNCTION _oo.c1_oracle_buffer_summary(s text,t uuid)
RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
 SELECT jsonb_build_object('checkpoint_kind','buffer_consumption','context_sha256',b.metadata->>'context_sha256',
 'bundle_sha256',b.bundle_sha256,'spool_sha256',b.spool_sha256,'source_identity',b.metadata->'source_identity',
 'typed_schema_sha256',b.metadata->>'typed_schema_sha256','final_cursor',COALESCE(p.evidence->'buffer_position'->'cursor_after',
 jsonb_build_object('transaction_ordinal','0','transaction_id',NULL)), 'commit_scn',p.commit_scn::text,
 'connector_offsets_sha256',b.connector_offsets_sha256,'connector_history_sha256',b.connector_history_sha256,
 'source_offset_write',false)
 FROM _oo.c1_typed_capture_bundle b LEFT JOIN _oo.c1_typed_buffer_position p
 ON p.source=b.source AND p.task=b.task AND p.apply_ordinal=b.final_apply_ordinal
 WHERE b.source=s AND b.task=t AND b.metadata ? 'context_sha256'
$$;
REVOKE ALL ON FUNCTION _oo.c1_oracle_buffer_summary(text,uuid) FROM PUBLIC,oo_pipeline_runtime;
CREATE OR REPLACE FUNCTION _oo.c1_typed_seal(s text,t uuid,g bigint,e integer,p_end text,p_pos numeric,p_tables text[])
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE; r _oo.c1_typed_receipt%ROWTYPE;
  ack_seq bigint; tables text[]; incomplete boolean; buffer jsonb;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  v:=_oo.native_c1_baseline_v2_assert(s,t,g,e);
  SELECT array_agg(name ORDER BY name),bool_or(NOT snapshot_complete) INTO tables,incomplete
    FROM _oo.c1_typed_table WHERE source=s AND task=t;
  IF tables IS NULL OR tables IS DISTINCT FROM p_tables OR incomplete THEN
    RAISE EXCEPTION 'typed_snapshot_incomplete' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO r FROM _oo.c1_typed_receipt WHERE source=s AND task=t ORDER BY seq DESC LIMIT 1;
  SELECT seq INTO ack_seq FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t;
  IF p_pos IS NULL OR p_pos<>COALESCE(r.position,v.l0_pos)
    OR ack_seq IS DISTINCT FROM COALESCE(r.seq,0)
    OR (v.engine='mysql' AND p_end IS DISTINCT FROM substring(v.measured_id from 7)||':1-'||_oo.native_pos_text(p_pos)) THEN
    RAISE EXCEPTION 'typed_barrier' USING ERRCODE='OO030';
  END IF;
  IF v.engine='oracle' THEN
    buffer:=_oo.c1_oracle_buffer_summary(s,t);
    IF buffer IS NULL OR buffer->>'context_sha256' IS NULL
      OR (buffer->'final_cursor'->>'transaction_ordinal')::numeric IS DISTINCT FROM p_pos
      OR p_end IS DISTINCT FROM 'oracle-buffer:'||(buffer->>'context_sha256')||':'||_oo.native_pos_text(p_pos)
      OR NOT EXISTS(SELECT 1 FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t AND final_apply_ordinal=p_pos) THEN
      RAISE EXCEPTION 'exact complete Oracle buffer cut required' USING ERRCODE='OO030';
    END IF;
    UPDATE _oo.native_c1_baseline_run SET l0='oracle-buffer:'||(buffer->>'context_sha256')||':0' WHERE source=s AND task_id=t;
  ELSIF v.engine<>'mysql' THEN RAISE EXCEPTION 'unsupported C1 engine' USING ERRCODE='OO030'; END IF;
  UPDATE _oo.native_c1_baseline_v2_format SET sealed=true WHERE source=s AND task_id=t;
  UPDATE _oo.native_c1_baseline_run SET applied_pos=p_pos,l_end=p_pos,state='awaiting_publish',sealed_at=clock_timestamp()
    WHERE source=s AND task_id=t;
  RETURN jsonb_build_object('outcome','sealed','l_end',p_end,'applied_pos',_oo.native_pos_text(p_pos)) || CASE WHEN buffer IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('source_engine','oracle','buffer',buffer) END;
END $fn$;

-- A position-complete initial snapshot may have no new transaction timestamp.
-- Preserve NULL, without inventing source as_of or writing a source heartbeat.
CREATE OR REPLACE FUNCTION _oo.native_c1_baseline_typed_commit(
 p_source text,p_task uuid,p_lease_generation bigint,p_epoch integer,
 p_target_generation bigint,p_source_generation bigint,p_request_digest text,p_scope jsonb,p_entries jsonb
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $bridge$
DECLARE s _oo.native_c1_baseline_run%ROWTYPE; i _oo.source_instance%ROWTYPE;
 e jsonb; r _oo.full_import_run%ROWTYPE; receipt jsonb; committed timestamptz; n integer; prior_key text:=''; codec text; buffer jsonb; expected_end text;
BEGIN
 SELECT * INTO s FROM _oo.native_c1_baseline_run WHERE source=p_source AND task_id=p_task FOR UPDATE;
 SELECT source_committed_at INTO committed FROM _oo.native_c1_baseline_txn_time
  WHERE source=p_source AND task_id=p_task AND applied_pos=s.l_end;
 IF s.engine='mysql' AND committed IS NULL AND s.l_end>s.l0_pos THEN RAISE EXCEPTION 'actual L_end binlog commit time required' USING ERRCODE='OO030'; END IF;
 IF s.engine='oracle' THEN
  buffer:=_oo.c1_oracle_buffer_summary(p_source,p_task);codec:='oracle-capture-sequence/v1';
  expected_end:='oracle-buffer:'||(buffer->>'context_sha256')||':'||_oo.native_pos_text(s.l_end);
  IF buffer IS NULL OR p_scope->>'protocol' IS DISTINCT FROM 'oo.source-admission.c1-oracle-buffer-scope/v1'
   OR p_scope->'buffer' IS DISTINCT FROM buffer OR p_scope->>'engine' IS DISTINCT FROM 'oracle'
   OR (buffer->'final_cursor'->>'transaction_ordinal')::numeric IS DISTINCT FROM s.l_end THEN
   RAISE EXCEPTION 'closed Oracle buffer scope required' USING ERRCODE='OO030'; END IF;
 ELSIF s.engine='mysql' THEN
  codec:='mysql-gtid/v1';expected_end:=substring(s.measured_id from 7)||':1-'||_oo.native_pos_text(s.l_end);
 ELSE RAISE EXCEPTION 'unsupported baseline engine' USING ERRCODE='OO030'; END IF;
 SELECT * INTO i FROM _oo.source_instance WHERE source=p_source AND source_epoch=p_epoch;
 IF s.source IS NULL OR i.source IS NULL OR s.state<>'awaiting_publish' OR s.l_end<>s.applied_pos
  OR s.source_epoch<>p_epoch OR s.lease_generation<>p_lease_generation
  OR i.lease_id<>p_task OR i.lease_generation<>p_lease_generation
  OR s.config_digest<>i.config_digest OR s.schema_digest<>i.schema_digest
  OR (p_scope->>'config_digest') IS DISTINCT FROM s.config_digest OR (p_scope->>'schema_digest') IS DISTINCT FROM s.schema_digest
  OR (p_scope->>'lineage') IS DISTINCT FROM s.measured_id OR (p_scope->>'l0') IS DISTINCT FROM s.l0
  OR (p_scope->>'l_end') IS DISTINCT FROM expected_end
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
   codec,s.measured_id,p_epoch,s.l_end,false,true,true,NULL,NULL,segment_bitmap,fence_shard,fence_generation
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
   AND w.source_generation=p_source_generation AND w.position_codec=codec
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
 IF buffer IS NOT NULL THEN receipt:=receipt||jsonb_build_object('position_codec',codec,'oracle_buffer',buffer); END IF;
 INSERT INTO _oo.native_c1_baseline_typed(source,task_id,request_digest,receipt) VALUES(p_source,p_task,p_request_digest,receipt);
 RETURN receipt;
END
$bridge$;
REVOKE ALL ON FUNCTION _oo.native_c1_baseline_typed_commit(text,uuid,bigint,integer,bigint,bigint,text,jsonb,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_c1_baseline_typed_commit(text,uuid,bigint,integer,bigint,bigint,text,jsonb,jsonb) TO oo_pipeline_runtime;

CREATE OR REPLACE FUNCTION _oo.publish_native_c1_collection(p_anchor uuid,p_sha text,p_receipt jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $publish$
DECLARE b _oo.native_c1_baseline_typed%ROWTYPE; w _oo.source_watermark%ROWTYPE;
 r _oo.full_import_run%ROWTYPE; t _oo.full_import_table%ROWTYPE; e jsonb; q record;
 n integer; changed integer; rejected bigint; accepted bigint; count_rejects bigint;
 v_source text:=p_receipt->>'source_id'; source_gen bigint:=(p_receipt->>'source_generation')::bigint;
 target_gen bigint:=(p_receipt->>'target_generation')::bigint; pos numeric:=(p_receipt->>'applied_pos')::numeric;
BEGIN
 IF p_anchor IS NULL OR p_sha IS NULL OR p_sha !~ '^[0-9a-f]{64}$'
  OR p_receipt->>'protocol' IS DISTINCT FROM 'oo.pipeline.native-publication-receipt/v1'
  OR p_receipt->>'run_id' IS DISTINCT FROM p_anchor::text
  OR jsonb_typeof(p_receipt->'tables') IS DISTINCT FROM 'array'
  OR jsonb_typeof(p_receipt->'run_ids') IS DISTINCT FROM 'array'
  OR p_receipt->>'position_codec' IS NULL OR p_receipt->>'position_codec' NOT IN ('mysql-gtid/v1','oracle-capture-sequence/v1')
  OR p_receipt->>'consistency' IS DISTINCT FROM 'transactional'
  OR p_receipt->>'consistency_grade' IS DISTINCT FROM 'T'
  OR p_receipt->>'cross_table_consistent' IS DISTINCT FROM 'true'
 THEN RAISE EXCEPTION 'closed C1 publication receipt' USING ERRCODE='OO030'; END IF;
 n:=jsonb_array_length(p_receipt->'tables');
 IF n<1 OR n>32 OR jsonb_array_length(p_receipt->'run_ids')<>n
  OR (SELECT count(DISTINCT value) FROM jsonb_array_elements(p_receipt->'run_ids'))<>n
  OR (SELECT count(DISTINCT (value->>'schema',value->>'table')) FROM jsonb_array_elements(p_receipt->'tables'))<>n
  OR NOT p_receipt->'run_ids' @> jsonb_build_array(p_anchor::text)
 THEN RAISE EXCEPTION 'complete C1 publication vector required' USING ERRCODE='OO030'; END IF;
 SELECT * INTO w FROM _oo.source_watermark WHERE source=v_source FOR UPDATE;
 IF NOT FOUND OR w.source_generation IS DISTINCT FROM source_gen OR w.target_generation IS DISTINCT FROM target_gen
  OR w.applied_pos IS DISTINCT FROM pos OR w.position_codec IS DISTINCT FROM p_receipt->>'position_codec'
  OR w.position_lineage IS DISTINCT FROM p_receipt->>'position_lineage'
  OR w.source_epoch IS DISTINCT FROM (p_receipt->>'source_epoch')::integer
  OR w.consistency<>'transactional' OR w.consistency_grade<>'T' OR NOT w.cross_table_consistent
 THEN RAISE EXCEPTION 'C1 current source cut required' USING ERRCODE='OO030'; END IF;
 SELECT bt.* INTO b FROM _oo.native_c1_baseline_typed bt JOIN jobs.tasks j ON j.id=bt.task_id
  WHERE bt.source=v_source AND bt.acknowledged_at IS NOT NULL AND j.state='succeeded'
  AND (bt.receipt->>'source_generation')::bigint=source_gen
  AND (bt.receipt->>'target_generation')::bigint=target_gen
  AND (bt.receipt->>'source_epoch')::integer=w.source_epoch AND (bt.receipt->>'applied_pos')::numeric=pos;
 IF NOT FOUND OR jsonb_array_length(b.receipt->'entries')<>n THEN
  RAISE EXCEPTION 'complete acknowledged typed C1 cut required' USING ERRCODE='OO030'; END IF;
 IF p_receipt->>'position_codec'='oracle-capture-sequence/v1' THEN
  IF b.receipt->'source_instance'->>'engine' IS DISTINCT FROM 'oracle'
   OR b.receipt->'oracle_buffer' IS NULL OR p_receipt->'oracle_buffer' IS DISTINCT FROM b.receipt->'oracle_buffer'
   OR (b.receipt->'oracle_buffer'->'final_cursor'->>'transaction_ordinal')::numeric IS DISTINCT FROM pos THEN
   RAISE EXCEPTION 'Oracle acknowledged buffer identity required' USING ERRCODE='OO030'; END IF;
 ELSIF b.receipt->'source_instance'->>'engine' IS DISTINCT FROM 'mysql' OR p_receipt ? 'oracle_buffer' THEN
  RAISE EXCEPTION 'C1 engine/codec conflict' USING ERRCODE='OO030'; END IF;
 IF w.state='live' THEN
  IF (SELECT count(*) FROM _oo.native_publication_receipt p JOIN _oo.full_import_run rr USING(run_id)
   WHERE p.source_id=v_source AND p.source_generation=source_gen AND p.target_generation=target_gen
   AND p.receipt_sha256=p_sha AND p.table_vector=p_receipt->'tables' AND rr.state='published'
   AND p_receipt->'run_ids' @> jsonb_build_array(p.run_id::text))<>n THEN
   RAISE EXCEPTION 'C1 publication replay conflict' USING ERRCODE='OO030'; END IF;
  RETURN jsonb_build_object('outcome','replay','run_id',p_anchor,'tables',n,'receipt_sha256',p_sha);
 END IF;
 IF w.state<>'lagging' THEN RAISE EXCEPTION 'C1 cut is not awaiting publication' USING ERRCODE='OO030'; END IF;
 FOR e IN SELECT value FROM jsonb_array_elements(p_receipt->'tables') LOOP
  IF NOT p_receipt->'run_ids' @> jsonb_build_array(e->>'run_id') OR NOT EXISTS(
   SELECT 1 FROM jsonb_array_elements(b.receipt->'entries') x WHERE x->>'run_id'=e->>'run_id'
    AND x->>'schema'=e->>'schema' AND x->>'table'=e->>'table' AND x->>'physical_id'=e->>'physical_name')
  THEN RAISE EXCEPTION 'C1 publication table identity' USING ERRCODE='OO030'; END IF;
  SELECT * INTO r FROM _oo.full_import_run WHERE run_id=(e->>'run_id')::uuid FOR UPDATE;
  IF NOT FOUND OR r.state<>'awaiting_publish' OR r.source_id<>v_source OR r.source_generation<>source_gen
   OR r.target_generation<>target_gen OR (e->>'source_generation')::bigint IS DISTINCT FROM source_gen
   OR (e->>'target_generation')::bigint IS DISTINCT FROM target_gen OR (e->>'applied_pos')::numeric IS DISTINCT FROM pos
  THEN RAISE EXCEPTION 'C1 publication run differs' USING ERRCODE='OO030'; END IF;
  SELECT * INTO t FROM _oo.full_import_table WHERE run_id=r.run_id AND schema_ident=e->>'schema' AND table_ident=e->>'table' FOR UPDATE;
  IF NOT FOUND OR NOT t.exhausted OR t.physical_name IS DISTINCT FROM e->>'physical_name'
   OR t.applied_rows IS DISTINCT FROM (e->>'accepted_rows')::bigint OR t.canonical_digest IS DISTINCT FROM e->>'canonical_digest'
   OR t.applied_pos<>pos OR (SELECT count(*) FROM _oo.full_import_table WHERE run_id=r.run_id)<>1
  THEN RAISE EXCEPTION 'C1 publication sealed table differs' USING ERRCODE='OO030'; END IF;
  SELECT coalesce(sum(accepted_rows),0),coalesce(sum(rejected_rows),0) INTO accepted,rejected
   FROM _oo.full_import_quality WHERE run_id=r.run_id AND physical_name=t.physical_name AND kind='batch';
  SELECT count(*) INTO count_rejects FROM _oo.full_import_quality WHERE run_id=r.run_id AND physical_name=t.physical_name AND kind='reject';
  IF accepted<>t.applied_rows OR rejected<>count_rejects OR jsonb_typeof(e->'rejects') IS DISTINCT FROM 'array'
   OR jsonb_array_length(e->'rejects')<>count_rejects THEN
   RAISE EXCEPTION 'C1 publication quality differs' USING ERRCODE='OO030'; END IF;
  FOR q IN SELECT * FROM _oo.full_import_quality WHERE run_id=r.run_id AND physical_name=t.physical_name AND kind='reject' LOOP
   IF q.reason<>'zero_date' OR q.key_digest IS NULL OR q.attnum IS NULL OR q.row_index IS NULL OR q.value_len IS NULL
    OR NOT EXISTS(SELECT 1 FROM pg_attribute a WHERE a.attrelid=to_regclass(format('_oo.%I',t.physical_name))
     AND a.attnum=q.attnum+1 AND a.atttypid IN(1082,1114,1184) AND NOT a.attnotnull)
    OR NOT EXISTS(SELECT 1 FROM jsonb_array_elements(e->'rejects') x WHERE x->>'reason'=q.reason
     AND (x->>'attnum')::integer=q.attnum AND (x->>'row_index')::bigint=q.row_index
     AND (x->>'value_len')::bigint=q.value_len AND x->>'key_digest'=q.key_digest)
   THEN RAISE EXCEPTION 'C1 zero-date quality review differs' USING ERRCODE='OO030'; END IF;
  END LOOP;
  IF NOT EXISTS(SELECT 1 FROM _oo.table_freshness f WHERE f.source=v_source AND f.source_generation=source_gen
   AND f.target_generation=target_gen AND f.schema_name=t.schema_ident AND f.table_name=t.table_ident
   AND f.physical_name=t.physical_name AND f.last_pos=pos AND f.mode='full' AND f.state='awaiting_publish')
  THEN RAISE EXCEPTION 'C1 freshness vector differs' USING ERRCODE='OO030'; END IF;
 END LOOP;
 FOR e IN SELECT value FROM jsonb_array_elements(p_receipt->'run_ids') LOOP
  INSERT INTO _oo.native_publication_receipt(run_id,receipt_sha256,source_id,source_generation,target_generation,
   position_codec,applied_pos,consistency,consistency_grade,cross_table_consistent,table_vector,publication_xid)
  VALUES((e#>>'{}')::uuid,p_sha,v_source,source_gen,target_gen,p_receipt->>'position_codec',pos,'transactional','T',true,p_receipt->'tables',pg_current_xact_id());
  UPDATE _oo.full_import_run SET state='published' WHERE run_id=(e#>>'{}')::uuid AND state='awaiting_publish';
  IF NOT FOUND THEN RAISE EXCEPTION 'C1 publication raced' USING ERRCODE='OO030'; END IF;
 END LOOP;
 UPDATE _oo.table_freshness SET state='live',applied_at=clock_timestamp() WHERE source=v_source
  AND source_generation=source_gen AND target_generation=target_gen AND state='awaiting_publish';
 GET DIAGNOSTICS changed=ROW_COUNT;
 IF changed<>n THEN RAISE EXCEPTION 'C1 full source barrier differs' USING ERRCODE='OO030'; END IF;
 UPDATE _oo.source_watermark SET state='live',applied_at=clock_timestamp() WHERE source=v_source;
 RETURN jsonb_build_object('outcome','published','run_id',p_anchor,'source_id',v_source,'source_generation',source_gen,
  'target_generation',target_gen,'applied_pos',pos,'tables',n,'receipt_sha256',p_sha);
END
$publish$;
REVOKE ALL ON FUNCTION _oo.publish_native_c1_collection(uuid,text,jsonb) FROM PUBLIC,oo_pipeline_runtime;
DO $grant$
BEGIN
 IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_target_admin') THEN
  GRANT EXECUTE ON FUNCTION _oo.publish_native_c1_collection(uuid,text,jsonb) TO oo_target_admin;
 END IF;
END
$grant$;

-- The final source-facing ACK remains gated on the existing successful jobs CAS.
CREATE OR REPLACE FUNCTION _oo.native_c1_baseline_ack(p_source text,p_task uuid,p_request_digest text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $bridge$
DECLARE v _oo.native_c1_baseline_typed%ROWTYPE;
BEGIN
 SELECT * INTO v FROM _oo.native_c1_baseline_typed WHERE source=p_source AND task_id=p_task FOR UPDATE;
 IF NOT FOUND OR v.request_digest IS DISTINCT FROM p_request_digest
  OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=p_task AND state='succeeded')
  OR NOT EXISTS(SELECT 1 FROM _oo.source_watermark w WHERE w.source=p_source
   AND w.source_generation=(v.receipt->>'source_generation')::bigint
   AND w.target_generation=(v.receipt->>'target_generation')::bigint
   AND w.source_epoch=(v.receipt->>'source_epoch')::integer
   AND w.applied_pos=(v.receipt->>'applied_pos')::numeric)
 THEN RAISE EXCEPTION 'baseline ACK requires exact durable import and jobs CAS' USING ERRCODE='OO010'; END IF;
 UPDATE _oo.native_c1_baseline_typed SET acknowledged_at=COALESCE(acknowledged_at,clock_timestamp())
  WHERE source=p_source AND task_id=p_task;
 RETURN jsonb_build_object('protocol','oo.pipeline.native-c1-baseline-ack-result/v1','acknowledged',true)
 || CASE WHEN v.receipt->'source_instance'->>'engine'='oracle' THEN jsonb_build_object('source_engine','oracle',
 'buffer_ack',jsonb_build_object('protocol','oo.c1.oracle-buffer-final-ack/v1','checkpoint_kind','buffer_consumption',
 'context_sha256',v.receipt->'oracle_buffer'->>'context_sha256','bundle_sha256',v.receipt->'oracle_buffer'->>'bundle_sha256',
 'cursor',v.receipt->'oracle_buffer'->'final_cursor','source_offset_write',false,'control_task',p_task,'control_state','succeeded',
 'request_digest',p_request_digest)) ELSE '{}'::jsonb END;
END
$bridge$;
REVOKE ALL ON FUNCTION _oo.native_c1_baseline_ack(text,uuid,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_c1_baseline_ack(text,uuid,text) TO oo_pipeline_runtime;
