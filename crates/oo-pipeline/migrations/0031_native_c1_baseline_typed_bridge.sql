-- Snapshot fills always have position zero; L0 belongs to the stream barrier.
CREATE OR REPLACE FUNCTION _oo.native_c1_baseline_fill(
  p_source text,p_task uuid,p_generation bigint,p_epoch integer,p_rows jsonb
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $c1$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE; e jsonb; n integer:=0; skipped integer:=0;
BEGIN
  v:=_oo.native_c1_baseline_assert_lease(p_source,p_task,p_generation,p_epoch);
  IF jsonb_typeof(p_rows)<>'array' THEN RAISE EXCEPTION 'invalid C1 snapshot batch' USING ERRCODE='OO030'; END IF;
  FOR e IN SELECT value FROM jsonb_array_elements(p_rows) LOOP
    IF e->>'schema' IS NULL OR e->>'table' IS NULL OR jsonb_typeof(e->'key') IS NULL
       OR jsonb_typeof(e->'value')<>'object' THEN RAISE EXCEPTION 'invalid C1 snapshot row' USING ERRCODE='OO030'; END IF;
    INSERT INTO _oo.native_c1_baseline_row(source,task_id,logical_schema,logical_table,row_key,row_value,deleted,applied_pos,txn_id)
    VALUES(p_source,p_task,e->>'schema',e->>'table',e->'key',e->'value',false,0,'snapshot')
    ON CONFLICT(source,task_id,logical_schema,logical_table,row_key) DO NOTHING;
    IF FOUND THEN n:=n+1; ELSE skipped:=skipped+1; END IF;
  END LOOP;
  RETURN jsonb_build_object('outcome','filled','inserted',n,'skipped_cdc_or_tombstone',skipped);
END
$c1$;


-- A sealed CDC-first stage is the source of the typed bootstrap, never a
-- published table. Verify the complete typed row set before retaining positions.
CREATE FUNCTION _oo.native_c1_baseline_bind_table(
 p_source text,p_task uuid,p_lease_generation bigint,p_epoch integer,
 p_shard text,p_owner uuid,p_target_generation bigint,p_run uuid,
 p_schema text,p_table text,p_columns text[],p_keys text[]
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $bridge$
DECLARE s _oo.native_c1_baseline_run%ROWTYPE; r _oo.full_import_run%ROWTYPE;
 physical text; col text; mapped text; typ text; oid oid; key_join text:='';
 mismatch text:=''; key_columns text:=''; expected text; n bigint; actual bigint; bad boolean;
BEGIN
 SELECT * INTO s FROM _oo.native_c1_baseline_run WHERE source=p_source AND task_id=p_task FOR UPDATE;
 IF NOT FOUND OR s.state<>'awaiting_publish' OR s.l_end<>s.applied_pos
  OR s.lease_generation<>p_lease_generation OR s.source_epoch<>p_epoch
  OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=p_task AND generation=p_lease_generation
    AND state='running' AND lease_until>clock_timestamp())
 THEN RAISE EXCEPTION 'sealed baseline and live lease required' USING ERRCODE='OO010'; END IF;
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_target_generation);
 SELECT * INTO r FROM _oo.full_import_run WHERE run_id=p_run;
 IF NOT FOUND OR r.source_id<>p_source OR r.shard_id<>p_shard OR r.owner_token<>p_owner
  OR r.target_generation<>p_target_generation OR r.state<>'open'
  OR cardinality(p_keys)<1 OR cardinality(p_columns)<1
 THEN RAISE EXCEPTION 'typed baseline run identity' USING ERRCODE='OO010'; END IF;
 SELECT physical_name INTO STRICT physical FROM _oo.full_import_table
  WHERE run_id=p_run AND schema_ident=p_schema AND table_ident=p_table;
 FOREACH col IN ARRAY p_columns LOOP
  mapped:=_oo.lookup_physical_mapping(p_source,r.config_digest,r.source_generation,'column',p_schema,p_table,col);
  SELECT a.atttypid,pg_catalog.format_type(a.atttypid,a.atttypmod) INTO oid,typ
   FROM pg_catalog.pg_attribute a WHERE a.attrelid=to_regclass(format('_oo.%I',physical)) AND a.attname=mapped AND NOT a.attisdropped;
  IF oid IS NULL OR oid NOT IN (16,17,20,21,23,25,114,700,701,1042,1043,1082,1114,1184,1186,1700,3802)
  THEN RAISE EXCEPTION 'baseline column type' USING ERRCODE='OO030'; END IF;
  expected:=CASE WHEN oid=17 THEN format('decode(s.row_value->%L->>''value'',''hex'')',col)
   WHEN oid IN (114,3802) THEN format('(s.row_value->%L->>''value'')::jsonb',col)
   ELSE format('(s.row_value->%L->>''value'')::%s',col,typ) END;
  mismatch:=mismatch || CASE WHEN mismatch='' THEN '' ELSE ' OR ' END || format(
   '(s.row_value->%L IS NULL OR CASE WHEN s.row_value->%L->>''type''=''null'' THEN t.%I IS NOT NULL ELSE %s IS DISTINCT FROM %s END)',
   col,col,mapped,CASE WHEN oid IN (114,3802) THEN format('t.%I::jsonb',mapped) ELSE format('t.%I',mapped) END,expected);
  IF col=ANY(p_keys) THEN
   key_columns:=key_columns || CASE WHEN key_columns='' THEN '' ELSE ',' END || format('%I',mapped);
   key_join:=key_join || CASE WHEN key_join='' THEN '' ELSE ' AND ' END || format('t.%I IS NOT DISTINCT FROM %s',mapped,expected);
  END IF;
 END LOOP;
 IF key_join='' OR NOT p_keys<@p_columns THEN RAISE EXCEPTION 'baseline keys' USING ERRCODE='OO030'; END IF;
 SELECT count(*) INTO n FROM _oo.native_c1_baseline_row WHERE source=p_source AND task_id=p_task
  AND logical_schema=p_schema AND logical_table=p_table AND NOT deleted;
 EXECUTE format('SELECT count(*) FROM _oo.%I',physical) INTO actual;
 IF actual<>n THEN RAISE EXCEPTION 'baseline typed row count' USING ERRCODE='OO030'; END IF;
 EXECUTE format('SELECT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_row s LEFT JOIN _oo.%I t ON %s WHERE s.source=$1 AND s.task_id=$2 AND s.logical_schema=$3 AND s.logical_table=$4 AND NOT s.deleted AND (t._oo_pos IS NULL OR t._oo_pos<>s.applied_pos OR %s))',physical,key_join,mismatch)
 INTO bad USING p_source,p_task,p_schema,p_table;
 IF bad THEN RAISE EXCEPTION 'baseline typed row mismatch' USING ERRCODE='OO030'; END IF;
 SET CONSTRAINTS ALL IMMEDIATE;
 IF NOT EXISTS(SELECT 1 FROM pg_catalog.pg_constraint WHERE conrelid=to_regclass(format('_oo.%I',physical)) AND contype='p') THEN
  EXECUTE format('ALTER TABLE _oo.%I ADD PRIMARY KEY (%s)',physical,key_columns);
 END IF;
 RETURN n;
END
$bridge$;
REVOKE ALL ON FUNCTION _oo.native_c1_baseline_bind_table(text,uuid,bigint,integer,text,uuid,bigint,uuid,text,text,text[],text[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_c1_baseline_bind_table(text,uuid,bigint,integer,text,uuid,bigint,uuid,text,text,text[],text[]) TO oo_pipeline_runtime;

CREATE TABLE _oo.native_c1_baseline_typed (
 source text NOT NULL, task_id uuid NOT NULL, request_digest text NOT NULL,
 receipt jsonb NOT NULL, PRIMARY KEY(source,task_id),
 FOREIGN KEY(source,task_id) REFERENCES _oo.native_c1_baseline_run(source,task_id)
);
REVOKE ALL ON _oo.native_c1_baseline_typed FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.native_c1_baseline_typed_receipt(p_source text,p_task uuid)
RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $$ SELECT jsonb_build_object('request_digest',request_digest,'receipt',receipt)
 FROM _oo.native_c1_baseline_typed WHERE source=p_source AND task_id=p_task $$;
REVOKE ALL ON FUNCTION _oo.native_c1_baseline_typed_receipt(text,uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_c1_baseline_typed_receipt(text,uuid) TO oo_pipeline_runtime;

CREATE FUNCTION _oo.native_c1_baseline_typed_commit(
 p_source text,p_task uuid,p_lease_generation bigint,p_epoch integer,
 p_target_generation bigint,p_source_generation bigint,p_request_digest text,p_scope jsonb,p_entries jsonb
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo
AS $bridge$
DECLARE s _oo.native_c1_baseline_run%ROWTYPE; i _oo.source_instance%ROWTYPE;
 e jsonb; r _oo.full_import_run%ROWTYPE; receipt jsonb; prior_key text:='';
BEGIN
 SELECT * INTO s FROM _oo.native_c1_baseline_run WHERE source=p_source AND task_id=p_task FOR UPDATE;
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
 END LOOP;
 IF NOT EXISTS(SELECT 1 FROM _oo.source_watermark w WHERE w.source=p_source
   AND w.applied_pos=s.l_end AND w.target_generation=p_target_generation
   AND w.source_generation=p_source_generation AND w.position_codec='mysql-gtid/v1'
   AND w.position_lineage=s.measured_id AND w.consistency_grade='T' AND w.cross_table_consistent)
 THEN RAISE EXCEPTION 'baseline watermark barrier' USING ERRCODE='OO010'; END IF;
 UPDATE _oo.source_watermark SET source_epoch=p_epoch WHERE source=p_source;
 receipt:=jsonb_build_object('protocol','oo.pipeline.native-c1-baseline-result/v1',
  'request_digest',p_request_digest,'source_id',p_source,'source_generation',p_source_generation,'target_generation',p_target_generation,
  'load_state','awaiting_publish','acknowledged',false,'l0',s.l0,'l_end',p_scope->>'l_end',
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

ALTER TABLE _oo.native_c1_baseline_typed ADD COLUMN acknowledged_at timestamptz;
CREATE FUNCTION _oo.native_c1_baseline_ack(p_source text,p_task uuid,p_request_digest text)
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
 RETURN jsonb_build_object('protocol','oo.pipeline.native-c1-baseline-ack-result/v1','acknowledged',true);
END
$bridge$;
REVOKE ALL ON FUNCTION _oo.native_c1_baseline_ack(text,uuid,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_c1_baseline_ack(text,uuid,text) TO oo_pipeline_runtime;
