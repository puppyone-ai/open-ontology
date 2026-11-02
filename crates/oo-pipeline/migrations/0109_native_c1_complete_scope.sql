-- Complete source membership and fill plans are not per-part resource budgets.
-- Preserve exact-cut, schema, receipt, digest, uniqueness and lease checks.
-- Existing versions/checksums remain immutable; normal typed installation applies this additive replacement.

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
 IF n<1 OR jsonb_array_length(p_receipt->'run_ids')<>n
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
  IF e ? 'storage_schema' OR e ? 'storage_table' THEN
   IF e->>'storage_schema' IS DISTINCT FROM _oo.c1_storage_ident(e->>'schema')
    OR e->>'storage_table' IS DISTINCT FROM _oo.c1_storage_ident(e->>'table') THEN
    RAISE EXCEPTION 'publication source/storage name mapping differs' USING ERRCODE='OO030'; END IF;
  END IF;
  SELECT * INTO r FROM _oo.full_import_run WHERE run_id=(e->>'run_id')::uuid FOR UPDATE;
  IF NOT FOUND OR r.state<>'awaiting_publish' OR r.source_id<>v_source OR r.source_generation<>source_gen
   OR r.target_generation<>target_gen OR (e->>'source_generation')::bigint IS DISTINCT FROM source_gen
   OR (e->>'target_generation')::bigint IS DISTINCT FROM target_gen OR (e->>'applied_pos')::numeric IS DISTINCT FROM pos
  THEN RAISE EXCEPTION 'C1 publication run differs' USING ERRCODE='OO030'; END IF;
  SELECT * INTO t FROM _oo.full_import_table WHERE run_id=r.run_id AND schema_ident=_oo.c1_storage_ident(e->>'schema') AND table_ident=_oo.c1_storage_ident(e->>'table') FOR UPDATE;
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


CREATE OR REPLACE FUNCTION _oo.native_library_c1_shard(p_source text) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
DECLARE w _oo.source_watermark%ROWTYPE; b _oo.native_c1_baseline_typed%ROWTYPE;
 p _oo.native_publication_receipt%ROWTYPE; r _oo.full_import_run%ROWTYPE;
 t _oo.full_import_table%ROWTYPE; base _oo.native_c1_baseline_run%ROWTYPE;
 receipt jsonb; entry jsonb; ack_entry jsonb; runs jsonb; n integer; sha text;
 storage_schema text; storage_table text; rejected jsonb; accepted bigint;
BEGIN
 SELECT * INTO STRICT w FROM _oo.source_watermark WHERE source=p_source;
 IF w.state<>'live' OR w.consistency<>'transactional' OR w.consistency_grade<>'T'
  OR NOT w.cross_table_consistent OR w.position_codec NOT IN ('mysql-gtid/v1','oracle-capture-sequence/v1') THEN
  RAISE EXCEPTION 'C1 library current T cut required' USING ERRCODE='OO030'; END IF;
 SELECT bt.* INTO STRICT b FROM _oo.native_c1_baseline_typed bt
  JOIN _oo.native_c1_baseline_run br ON br.source=bt.source AND br.task_id=bt.task_id
  JOIN jobs.tasks j ON j.id=bt.task_id AND j.generation=br.lease_generation
  WHERE bt.source=p_source AND bt.acknowledged_at IS NOT NULL AND j.state='succeeded'
   AND (bt.receipt->>'source_generation')::bigint=w.source_generation
   AND (bt.receipt->>'target_generation')::bigint=w.target_generation
   AND (bt.receipt->>'source_epoch')::integer=w.source_epoch
   AND (bt.receipt->>'applied_pos')::numeric=w.applied_pos;
 SELECT * INTO STRICT base FROM _oo.native_c1_baseline_run WHERE source=p_source AND task_id=b.task_id;
 IF base.state<>'awaiting_publish' OR base.sealed_at IS NULL
  OR base.applied_pos IS DISTINCT FROM w.applied_pos OR base.l_end IS DISTINCT FROM w.applied_pos
  OR base.source_epoch IS DISTINCT FROM w.source_epoch OR base.measured_id IS DISTINCT FROM w.position_lineage
  OR b.receipt->>'source_id' IS DISTINCT FROM p_source
  OR b.receipt->>'request_digest' IS DISTINCT FROM b.request_digest
  OR (CASE WHEN base.engine='mysql' THEN
      w.position_codec IS DISTINCT FROM 'mysql-gtid/v1'
      OR (b.receipt ? 'position_codec' AND b.receipt->>'position_codec' IS DISTINCT FROM w.position_codec)
      OR b.receipt->>'l_end' IS DISTINCT FROM substring(base.measured_id from 7)||':1-'||_oo.native_pos_text(base.l_end)
     ELSE b.receipt->>'position_codec' IS DISTINCT FROM w.position_codec END)
  OR b.receipt->'source_instance'->>'measured_id' IS DISTINCT FROM w.position_lineage
  OR b.receipt->'source_instance'->>'engine' IS DISTINCT FROM base.engine
  OR b.receipt->'source_instance'->>'config_digest' IS DISTINCT FROM base.config_digest
  OR b.receipt->'source_instance'->>'schema_digest' IS DISTINCT FROM base.schema_digest
  OR b.receipt->'source_instance'->>'lease_id' IS DISTINCT FROM b.task_id::text
  OR NOT EXISTS(SELECT 1 FROM _oo.source_instance i WHERE i.source=p_source
    AND i.source_epoch=w.source_epoch AND i.measured_id=w.position_lineage
    AND i.engine=base.engine AND i.config_digest=base.config_digest AND i.schema_digest=base.schema_digest)
 THEN RAISE EXCEPTION 'C1 library acknowledged identity differs' USING ERRCODE='OO030'; END IF;
 n:=jsonb_array_length(b.receipt->'entries');
 IF n IS NULL OR n<1 THEN RAISE EXCEPTION 'C1 library entries' USING ERRCODE='OO030'; END IF;
 SELECT * INTO STRICT p FROM _oo.native_publication_receipt
  WHERE run_id=(b.receipt->'entries'->0->>'run_id')::uuid;
 SELECT jsonb_agg(value->'run_id' ORDER BY ord) INTO runs
  FROM jsonb_array_elements(b.receipt->'entries') WITH ORDINALITY e(value,ord);
 IF jsonb_array_length(p.table_vector)<>n
  OR (SELECT count(DISTINCT value) FROM jsonb_array_elements(runs))<>n
  OR (SELECT count(DISTINCT (value->>'schema',value->>'table')) FROM jsonb_array_elements(p.table_vector))<>n
  OR (SELECT count(*) FROM _oo.table_freshness WHERE source=p_source)<>n
  OR (SELECT count(*) FROM _oo.native_publication_receipt WHERE source_id=p_source
      AND source_generation=w.source_generation AND target_generation=w.target_generation)<>n
 THEN RAISE EXCEPTION 'C1 library complete vector required' USING ERRCODE='OO030'; END IF;
 receipt:=jsonb_build_object('protocol','oo.pipeline.native-publication-receipt/v1',
  'run_id',p.run_id,'run_ids',runs,'source_id',p_source,'source_generation',w.source_generation,
  'target_generation',w.target_generation,'source_epoch',w.source_epoch,'position_codec',w.position_codec,
  'position_lineage',w.position_lineage,'applied_pos',_oo.native_pos_text(w.applied_pos),
  'consistency','transactional','consistency_grade','T','cross_table_consistent',true,'tables',p.table_vector);
 IF base.engine='oracle' AND w.position_codec='oracle-capture-sequence/v1' THEN
  IF b.receipt->'oracle_buffer'->>'checkpoint_kind' IS DISTINCT FROM 'buffer_consumption'
   OR b.receipt->'oracle_buffer'->'source_offset_write' IS DISTINCT FROM 'false'::jsonb
   OR (b.receipt->'oracle_buffer'->'final_cursor'->>'transaction_ordinal')::numeric IS DISTINCT FROM w.applied_pos THEN
   RAISE EXCEPTION 'C1 library Oracle buffer differs' USING ERRCODE='OO030'; END IF;
  receipt:=receipt||jsonb_build_object('oracle_buffer',b.receipt->'oracle_buffer');
 ELSIF base.engine<>'mysql' OR w.position_codec<>'mysql-gtid/v1' OR b.receipt ? 'oracle_buffer' THEN
  RAISE EXCEPTION 'C1 library engine differs' USING ERRCODE='OO030';
 END IF;
 sha:=encode(sha256(convert_to(_oo.native_library_canonical(receipt),'UTF8')),'hex');
 IF sha IS DISTINCT FROM p.receipt_sha256 THEN
  RAISE EXCEPTION 'C1 library original receipt digest differs' USING ERRCODE='OO030'; END IF;
 FOR entry IN SELECT value FROM jsonb_array_elements(p.table_vector) LOOP
  SELECT value INTO STRICT ack_entry FROM jsonb_array_elements(b.receipt->'entries')
   WHERE value->>'run_id'=entry->>'run_id';
  storage_schema:=coalesce(entry->>'storage_schema',entry->>'schema');
  storage_table:=coalesce(entry->>'storage_table',entry->>'table');
  IF entry->>'schema' IS DISTINCT FROM ack_entry->>'schema'
   OR entry->>'table' IS DISTINCT FROM ack_entry->>'table'
   OR storage_schema IS DISTINCT FROM coalesce(ack_entry->>'storage_schema',ack_entry->>'schema')
   OR storage_table IS DISTINCT FROM coalesce(ack_entry->>'storage_table',ack_entry->>'table')
   OR entry->>'physical_name' IS DISTINCT FROM ack_entry->>'physical_id'
   OR (entry->>'accepted_rows')::bigint IS DISTINCT FROM (ack_entry->>'row_count')::bigint
   OR (entry->>'source_generation')::bigint IS DISTINCT FROM w.source_generation
   OR (entry->>'target_generation')::bigint IS DISTINCT FROM w.target_generation
   OR (entry->>'applied_pos')::numeric IS DISTINCT FROM w.applied_pos THEN
   RAISE EXCEPTION 'C1 library source/storage binding differs' USING ERRCODE='OO030'; END IF;
  SELECT * INTO STRICT r FROM _oo.full_import_run WHERE run_id=(entry->>'run_id')::uuid;
  SELECT * INTO STRICT t FROM _oo.full_import_table WHERE run_id=r.run_id;
  IF r.source_id<>p_source OR r.source_generation<>w.source_generation OR r.target_generation<>w.target_generation
   OR r.state<>'published' OR r.config_digest<>base.config_digest
   OR NOT t.exhausted OR t.schema_ident IS DISTINCT FROM storage_schema OR t.table_ident IS DISTINCT FROM storage_table
   OR t.physical_name IS DISTINCT FROM entry->>'physical_name' OR t.applied_pos<>w.applied_pos
   OR t.applied_rows IS DISTINCT FROM (entry->>'accepted_rows')::bigint
   OR t.canonical_digest IS DISTINCT FROM entry->>'canonical_digest'
   OR NOT EXISTS(SELECT 1 FROM _oo.native_publication_receipt x WHERE x.run_id=r.run_id
    AND x.receipt_sha256=sha AND x.source_id=p_source AND x.source_generation=w.source_generation
    AND x.target_generation=w.target_generation AND x.position_codec=w.position_codec AND x.applied_pos=w.applied_pos
    AND x.consistency='transactional' AND x.consistency_grade='T' AND x.cross_table_consistent AND x.table_vector=p.table_vector)
   OR NOT EXISTS(SELECT 1 FROM _oo.table_freshness f WHERE f.source=p_source
    AND f.schema_name=storage_schema AND f.table_name=storage_table AND f.physical_name=t.physical_name
    AND f.source_generation=w.source_generation AND f.target_generation=w.target_generation
    AND f.last_pos=w.applied_pos AND f.state='live')
   OR NOT EXISTS(SELECT 1 FROM _oo.table_quality q WHERE q.schema_name=storage_schema AND q.table_name=storage_table
    AND NOT q.partial_txn_in_progress AND q.position_state='ok' AND q.budget_state='ok' AND q.capture_method='c1_cdc') THEN
   RAISE EXCEPTION 'C1 library target vector differs' USING ERRCODE='OO030'; END IF;
  SELECT coalesce(sum(accepted_rows),0) INTO accepted FROM _oo.full_import_quality
   WHERE run_id=r.run_id AND physical_name=t.physical_name AND kind='batch';
  SELECT coalesce(jsonb_agg(jsonb_build_object('reason',q.reason,'attnum',q.attnum,'row_index',q.row_index,
    'value_len',q.value_len,'key_digest',q.key_digest) ORDER BY q.row_index,q.attnum),'[]'::jsonb)
   INTO rejected FROM _oo.full_import_quality q WHERE q.run_id=r.run_id AND q.kind='reject';
  IF accepted IS DISTINCT FROM t.applied_rows OR rejected IS DISTINCT FROM entry->'rejects' THEN
   RAISE EXCEPTION 'C1 library quality proof differs' USING ERRCODE='OO030'; END IF;
 END LOOP;
 RETURN jsonb_build_object('source_id',p_source,'anchor_run',p.run_id,'receipt_sha256',sha,'receipt',receipt);
EXCEPTION WHEN no_data_found OR too_many_rows OR invalid_text_representation OR numeric_value_out_of_range OR invalid_parameter_value THEN
 RAISE EXCEPTION 'C1 library durable proof missing or ambiguous' USING ERRCODE='OO030';
END $body$;

CREATE OR REPLACE FUNCTION _oo.c1_oracle_fill_check_0099_base(s text,t uuid,g bigint,e integer,p jsonb,n bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE r _oo.native_c1_baseline_run%ROWTYPE; i _oo.source_instance%ROWTYPE;
  b _oo.c1_typed_capture_bundle%ROWTYPE; tab _oo.c1_typed_table%ROWTYPE;
  cp bigint; last_seq bigint; receipt_count bigint; total bigint; got bigint;
  names text[]; wanted text[]; completed text[]:='{}'; entry jsonb; k bigint;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  SELECT * INTO r FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
  IF NOT FOUND OR r.engine<>'oracle' OR r.state NOT IN ('open','awaiting_publish')
    OR r.source_epoch<>e OR r.lease_generation<1 OR r.lease_generation>g
    OR r.l0_pos<>0 OR r.applied_pos<>0
    OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=t AND generation=g
      AND state='running' AND lease_until>clock_timestamp())
    OR EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=s AND source_generation>0)
    OR EXISTS(SELECT 1 FROM _oo.c1_typed_delta_seed WHERE source=s AND task=t)
    OR EXISTS(SELECT 1 FROM _oo.native_c1_baseline_typed WHERE source=s AND task_id=t AND acknowledged_at IS NOT NULL)
  THEN RAISE EXCEPTION 'Oracle initial fill live task required' USING ERRCODE='OO010'; END IF;
  SELECT * INTO i FROM _oo.source_instance WHERE source=s AND source_epoch=e;
  IF NOT FOUND OR i.engine<>'oracle' OR i.lease_id<>t OR i.lease_generation<>r.lease_generation
    OR i.measured_id<>r.measured_id OR i.config_digest<>r.config_digest OR i.schema_digest<>r.schema_digest
  THEN RAISE EXCEPTION 'Oracle initial fill instance drift' USING ERRCODE='OO010'; END IF;
  SELECT * INTO b FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t;
  IF NOT FOUND OR b.final_apply_ordinal<>0
    OR p->>'protocol' IS DISTINCT FROM 'oo.oracle.initial-fill/v1'
    OR p->>'source_id' IS DISTINCT FROM s
    OR p->>'config_digest' IS DISTINCT FROM r.config_digest
    OR p->>'typed_schema_sha256' IS DISTINCT FROM r.schema_digest
    OR p->>'bundle_sha256' IS DISTINCT FROM b.bundle_sha256
    OR p->'buffer_context'->>'sha256' IS DISTINCT FROM b.metadata->>'context_sha256'
    OR p->'buffer_index' IS DISTINCT FROM b.metadata->'buffer_index'
    OR p IS DISTINCT FROM b.metadata->'initial_fill'
    OR b.metadata->>'source_id' IS DISTINCT FROM s
    OR b.metadata->>'typed_schema_sha256' IS DISTINCT FROM r.schema_digest
    OR b.metadata->>'checkpoint_kind' IS DISTINCT FROM 'buffer_consumption'
    OR b.metadata->>'source_offset_write' IS DISTINCT FROM 'false'
    OR b.metadata ? 'rollover_sha256'
    OR jsonb_typeof(p->'batches') IS DISTINCT FROM 'array'
    OR jsonb_typeof(p->'tables') IS DISTINCT FROM 'object'
    OR (r.l0<>b.bundle_sha256 AND r.l0<>'oracle-buffer:'||(b.metadata->>'context_sha256')||':0')
  THEN RAISE EXCEPTION 'Oracle initial fill immutable binding drift' USING ERRCODE='OO030'; END IF;
  total:=(p->>'batch_count')::bigint;
  IF total IS NULL OR total<1 OR jsonb_array_length(p->'batches')<>total
  THEN RAISE EXCEPTION 'Oracle initial fill bounded plan required' USING ERRCODE='OO030'; END IF;
  k:=0;
  FOR entry IN SELECT value FROM jsonb_array_elements(p->'batches') LOOP
    k:=k+1;
    IF (entry->>'sequence')::bigint IS DISTINCT FROM k OR entry->>'digest' IS NULL
      OR entry->>'digest' !~ '^[0-9a-f]{64}$'
    THEN RAISE EXCEPTION 'Oracle initial fill plan sequence' USING ERRCODE='OO030'; END IF;
  END LOOP;
  SELECT seq INTO cp FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t;
  SELECT coalesce(max(seq),0),count(*) INTO last_seq,receipt_count
    FROM _oo.c1_typed_receipt WHERE source=s AND task=t;
  IF n IS NULL OR n<0 OR n>total OR cp IS DISTINCT FROM n
    OR last_seq<n OR last_seq>n+1 OR last_seq>total OR receipt_count<>last_seq
    OR EXISTS(SELECT 1 FROM _oo.c1_typed_receipt a WHERE a.source=s AND a.task=t
      AND (a.kind<>'fill' OR a.position<>0 OR a.seq>total
        OR a.digest IS DISTINCT FROM p->'batches'->(a.seq::integer-1)->>'digest'))
  THEN RAISE EXCEPTION 'Oracle initial fill durable checkpoint required' USING ERRCODE='OO030'; END IF;
  SELECT array_agg(name ORDER BY name) INTO names FROM _oo.c1_typed_table WHERE source=s AND task=t;
  SELECT array_agg(key ORDER BY key) INTO wanted FROM jsonb_object_keys(p->'tables') AS key;
  IF names IS NULL OR wanted IS NULL OR names IS DISTINCT FROM wanted
  THEN RAISE EXCEPTION 'Oracle initial fill table coverage' USING ERRCODE='OO030'; END IF;
  FOR tab IN SELECT * FROM _oo.c1_typed_table WHERE source=s AND task=t ORDER BY name LOOP
    entry:=p->'tables'->tab.name;
    IF (entry->>'last_sequence')::bigint NOT BETWEEN 1 AND total
      OR (entry->>'last_sequence') IS NULL OR (entry->>'rows') IS NULL OR (entry->>'rows')::bigint<0
    THEN RAISE EXCEPTION 'Oracle initial fill table plan' USING ERRCODE='OO030'; END IF;
    IF tab.snapshot_complete THEN
      IF (entry->>'last_sequence')::bigint>n
      THEN RAISE EXCEPTION 'Oracle incomplete fill marked complete' USING ERRCODE='OO030'; END IF;
      EXECUTE format('SELECT count(*) FROM _oo.%I',tab.physical) INTO got;
      IF got<>(entry->>'rows')::bigint
      THEN RAISE EXCEPTION 'Oracle complete fill row coverage' USING ERRCODE='OO030'; END IF;
      completed:=array_append(completed,tab.name);
    END IF;
  END LOOP;
  IF r.state='awaiting_publish' THEN
    IF n<>total OR last_seq<>n OR completed IS DISTINCT FROM names OR r.l_end IS DISTINCT FROM 0::numeric
      OR NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_v2_format WHERE source=s AND task_id=t AND sealed)
    THEN RAISE EXCEPTION 'Oracle initial fill not sealed' USING ERRCODE='OO030'; END IF;
  ELSIF NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_v2_format WHERE source=s AND task_id=t AND NOT sealed) THEN
    RAISE EXCEPTION 'Oracle initial fill state drift' USING ERRCODE='OO030';
  END IF;
  RETURN jsonb_build_object('outcome','open','source_epoch',e,'resumed_sequence',n,
    'buffer_context_sha256',b.metadata->>'context_sha256','bundle_sha256',b.bundle_sha256,
    'completed_tables',to_jsonb(completed));
END $fn$;

CREATE OR REPLACE FUNCTION _oo.c1_oracle_mixed_check(s text,t uuid,g bigint,e integer,p jsonb,n bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE r _oo.native_c1_baseline_run%ROWTYPE; i _oo.source_instance%ROWTYPE;
 b _oo.c1_typed_capture_bundle%ROWTYPE; tab _oo.c1_typed_table%ROWTYPE;
 a _oo.c1_typed_receipt%ROWTYPE; bp _oo.c1_typed_buffer_position%ROWTYPE;
 cp bigint; last_seq bigint; receipt_count bigint; total bigint; fills bigint; final_pos bigint;
 k bigint:=0; entry jsonb; names text[]; wanted text[]; completed text[]:='{}'; got bigint;
BEGIN
 PERFORM _oo.native_c1_baseline_lock(s,t);
 SELECT * INTO r FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
 IF NOT FOUND OR r.engine<>'oracle' OR r.state NOT IN ('open','awaiting_publish')
  OR r.source_epoch<>e OR r.lease_generation<1 OR r.lease_generation>g OR r.l0_pos<>0
  OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=t AND generation=g AND state='running' AND lease_until>clock_timestamp())
  OR EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=s AND source_generation>0)
  OR EXISTS(SELECT 1 FROM _oo.c1_typed_delta_seed WHERE source=s AND task=t)
  OR EXISTS(SELECT 1 FROM _oo.native_c1_baseline_typed WHERE source=s AND task_id=t AND acknowledged_at IS NOT NULL)
 THEN RAISE EXCEPTION 'Oracle mixed live initial task required' USING ERRCODE='OO010'; END IF;
 SELECT * INTO i FROM _oo.source_instance WHERE source=s AND source_epoch=e;
 IF NOT FOUND OR i.engine<>'oracle' OR i.lease_id<>t OR i.lease_generation<>r.lease_generation
  OR i.measured_id<>r.measured_id OR i.config_digest<>r.config_digest OR i.schema_digest<>r.schema_digest
 THEN RAISE EXCEPTION 'Oracle mixed instance drift' USING ERRCODE='OO010'; END IF;
 SELECT * INTO b FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t;
 IF NOT FOUND OR p->>'protocol' IS DISTINCT FROM 'oo.oracle.initial-mixed/v1'
  OR p IS DISTINCT FROM b.metadata->'initial_fill'
  OR p->>'source_id' IS DISTINCT FROM s OR p->>'config_digest' IS DISTINCT FROM r.config_digest
  OR p->>'typed_schema_sha256' IS DISTINCT FROM r.schema_digest
  OR p->>'bundle_sha256' IS DISTINCT FROM b.bundle_sha256
  OR p->'buffer_context'->>'sha256' IS DISTINCT FROM b.metadata->>'context_sha256'
  OR p->'buffer_index' IS DISTINCT FROM b.metadata->'buffer_index'
  OR b.metadata->>'source_id' IS DISTINCT FROM s
  OR b.metadata->>'typed_schema_sha256' IS DISTINCT FROM r.schema_digest
  OR b.metadata->>'checkpoint_kind' IS DISTINCT FROM 'buffer_consumption'
  OR b.metadata->>'source_offset_write' IS DISTINCT FROM 'false'
  OR b.metadata ? 'rollover_sha256'
  OR jsonb_typeof(p->'batches') IS DISTINCT FROM 'array'
  OR jsonb_typeof(p->'tables') IS DISTINCT FROM 'object'
  OR (r.l0<>b.bundle_sha256 AND r.l0<>'oracle-buffer:'||(b.metadata->>'context_sha256')||':0')
 THEN RAISE EXCEPTION 'Oracle mixed immutable binding drift' USING ERRCODE='OO030'; END IF;
 total:=(p->>'batch_count')::bigint; fills:=(p->>'fill_batch_count')::bigint;
 final_pos:=(p->>'final_position')::bigint;
 IF total IS NULL OR total<2 OR fills IS NULL OR fills NOT BETWEEN 1 AND total-1
  OR final_pos IS DISTINCT FROM total-fills OR final_pos IS DISTINCT FROM b.final_apply_ordinal
  OR jsonb_array_length(p->'batches')<>total
  OR r.applied_pos IS DISTINCT FROM (CASE WHEN r.state='awaiting_publish' THEN final_pos ELSE 0 END)::numeric
 THEN RAISE EXCEPTION 'Oracle mixed bounded plan required' USING ERRCODE='OO030'; END IF;
 FOR entry IN SELECT value FROM jsonb_array_elements(p->'batches') LOOP
  k:=k+1;
  IF (entry->>'sequence')::bigint IS DISTINCT FROM k OR entry->>'digest' IS NULL
   OR entry->>'digest' !~ '^[0-9a-f]{64}$'
   OR (k<=fills AND (entry->>'kind' IS DISTINCT FROM 'fill' OR entry->>'position' IS DISTINCT FROM '0'
     OR entry->>'previous_position' IS DISTINCT FROM '0' OR entry ? 'source_evidence_sha256'))
   OR (k>fills AND (entry->>'kind' IS DISTINCT FROM 'cdc' OR entry->>'position' IS DISTINCT FROM (k-fills)::text
     OR entry->>'previous_position' IS DISTINCT FROM (k-fills-1)::text
     OR entry->>'source_evidence_sha256' IS NULL OR entry->>'source_evidence_sha256' !~ '^[0-9a-f]{64}$'))
  THEN RAISE EXCEPTION 'Oracle mixed explicit sequence/cursor plan' USING ERRCODE='OO030'; END IF;
 END LOOP;
 SELECT seq INTO cp FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t;
 SELECT coalesce(max(seq),0),count(*) INTO last_seq,receipt_count FROM _oo.c1_typed_receipt WHERE source=s AND task=t;
 IF n IS NULL OR n<0 OR n>total OR cp IS DISTINCT FROM n OR last_seq<n OR last_seq>n+1
  OR last_seq>total OR receipt_count<>last_seq
 THEN RAISE EXCEPTION 'Oracle mixed durable checkpoint required' USING ERRCODE='OO030'; END IF;
 FOR a IN SELECT * FROM _oo.c1_typed_receipt WHERE source=s AND task=t ORDER BY seq LOOP
  entry:=p->'batches'->(a.seq::integer-1);
  IF a.seq<1 OR a.seq>total OR a.digest IS DISTINCT FROM entry->>'digest'
   OR a.kind IS DISTINCT FROM entry->>'kind' OR _oo.native_pos_text(a.position) IS DISTINCT FROM entry->>'position'
  THEN RAISE EXCEPTION 'Oracle mixed receipt conflict' USING ERRCODE='OO030'; END IF;
  IF a.kind='cdc' THEN
   SELECT * INTO bp FROM _oo.c1_typed_buffer_position WHERE source=s AND task=t AND seq=a.seq;
   IF NOT FOUND OR bp.apply_ordinal::text IS DISTINCT FROM entry->>'position'
    OR bp.evidence->>'source_evidence_sha256' IS DISTINCT FROM entry->>'source_evidence_sha256'
    OR bp.evidence->>'context_sha256' IS DISTINCT FROM b.metadata->>'context_sha256'
    OR bp.evidence->>'bundle_sha256' IS DISTINCT FROM b.bundle_sha256
    OR bp.evidence#>>'{buffer_position,typed_batch_digest}' IS DISTINCT FROM a.digest
    OR bp.evidence#>>'{buffer_position,source_offset_write}' IS DISTINCT FROM 'false'
    OR bp.evidence#>>'{buffer_position,cursor_before,transaction_ordinal}' IS DISTINCT FROM entry->>'previous_position'
    OR bp.evidence#>>'{buffer_position,cursor_after,transaction_ordinal}' IS DISTINCT FROM entry->>'position'
   THEN RAISE EXCEPTION 'Oracle mixed captured END evidence conflict' USING ERRCODE='OO030'; END IF;
  END IF;
 END LOOP;
 SELECT array_agg(name ORDER BY name) INTO names FROM _oo.c1_typed_table WHERE source=s AND task=t;
 SELECT array_agg(key ORDER BY key) INTO wanted FROM jsonb_object_keys(p->'tables') AS key;
 IF names IS NULL OR wanted IS NULL OR names IS DISTINCT FROM wanted
 THEN RAISE EXCEPTION 'Oracle mixed table coverage' USING ERRCODE='OO030'; END IF;
 FOR tab IN SELECT * FROM _oo.c1_typed_table WHERE source=s AND task=t ORDER BY name LOOP
  entry:=p->'tables'->tab.name;
  IF entry->>'last_sequence' IS NULL OR (entry->>'last_sequence')::bigint NOT BETWEEN 1 AND fills
   OR entry->>'rows' IS NULL OR (entry->>'rows')::bigint<0
  THEN RAISE EXCEPTION 'Oracle mixed fill table plan' USING ERRCODE='OO030'; END IF;
  IF tab.snapshot_complete THEN
   IF (entry->>'last_sequence')::bigint>n
   THEN RAISE EXCEPTION 'Oracle mixed incomplete fill marker' USING ERRCODE='OO030'; END IF;
   -- CDC can legitimately change row count after the exact fill was completed.
   IF last_seq<=fills THEN
    EXECUTE format('SELECT count(*) FROM _oo.%I',tab.physical) INTO got;
    IF got<>(entry->>'rows')::bigint
    THEN RAISE EXCEPTION 'Oracle mixed fill row coverage' USING ERRCODE='OO030'; END IF;
   END IF;
   completed:=array_append(completed,tab.name);
  END IF;
 END LOOP;
 IF last_seq>fills AND completed IS DISTINCT FROM names
 THEN RAISE EXCEPTION 'Oracle mixed CDC before complete fill' USING ERRCODE='OO030'; END IF;
 IF r.state='awaiting_publish' THEN
  IF n<>total OR last_seq<>n OR completed IS DISTINCT FROM names OR r.l_end IS DISTINCT FROM final_pos::numeric
   OR NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_v2_format WHERE source=s AND task_id=t AND sealed)
  THEN RAISE EXCEPTION 'Oracle mixed unsealed final cut' USING ERRCODE='OO030'; END IF;
 ELSIF NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_v2_format WHERE source=s AND task_id=t AND NOT sealed) THEN
  RAISE EXCEPTION 'Oracle mixed state drift' USING ERRCODE='OO030';
 END IF;
 RETURN jsonb_build_object('outcome','open','source_epoch',e,'resumed_sequence',n,
  'buffer_context_sha256',b.metadata->>'context_sha256','bundle_sha256',b.bundle_sha256,
  'completed_tables',to_jsonb(completed));
END $fn$;

CREATE OR REPLACE FUNCTION _oo.c1_typed_record_buffer_0080(s text,t uuid,g bigint,e integer,p_seq bigint,p_digest text,p jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE b _oo.c1_typed_capture_bundle%ROWTYPE; r _oo.c1_typed_receipt%ROWTYPE;
  old _oo.c1_typed_buffer_position%ROWTYPE; prev _oo.c1_typed_buffer_position%ROWTYPE; checkpoint jsonb;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  PERFORM _oo.c1_typed_assert_engine(s,t,g,e,'oracle');
  SELECT * INTO b FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t;
  IF NOT FOUND OR p->>'protocol' IS DISTINCT FROM 'oo.c1.oracle-buffer-cursor/v1'
    OR p->>'bundle_sha256' IS DISTINCT FROM b.bundle_sha256
    OR p->>'spool_sha256' IS DISTINCT FROM b.spool_sha256
    OR p->>'source_offset_advanced' IS DISTINCT FROM 'false' THEN
    RAISE EXCEPTION 'captured bundle binding conflict' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO r FROM _oo.c1_typed_receipt WHERE source=s AND task=t AND seq=p_seq AND digest=p_digest AND kind='cdc';
  checkpoint:=p->'checkpoint';
  IF NOT FOUND OR r.position::text IS DISTINCT FROM p->>'apply_ordinal'
    OR (checkpoint->>'application_position')::numeric IS DISTINCT FROM r.position
    OR r.position>b.final_apply_ordinal
    OR checkpoint->>'transaction_id' IS NULL
    OR checkpoint->>'commit_scn' IS NULL OR checkpoint->>'commit_scn' !~ '^[1-9][0-9]{0,37}$'
    OR checkpoint->>'event_sha256' IS NULL OR checkpoint->>'event_sha256' !~ '^[0-9a-f]{64}$'
    OR (checkpoint->>'event_count')::bigint<1 THEN
    RAISE EXCEPTION 'captured complete transaction required' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO old FROM _oo.c1_typed_buffer_position WHERE source=s AND task=t AND seq=p_seq;
  IF FOUND THEN
    IF old.evidence<>p THEN RAISE EXCEPTION 'captured transaction replay conflict' USING ERRCODE='OO030'; END IF;
    RETURN;
  END IF;
  SELECT * INTO prev FROM _oo.c1_typed_buffer_position WHERE source=s AND task=t ORDER BY apply_ordinal DESC LIMIT 1;
  IF NOT FOUND AND EXISTS(SELECT 1 FROM _oo.c1_typed_delta_seed WHERE source=s AND task=t) THEN
    IF p_seq<>1 THEN RAISE EXCEPTION 'missing prior child transaction' USING ERRCODE='OO030'; END IF;
    SELECT bp.* INTO prev FROM _oo.c1_typed_delta_seed d
      JOIN _oo.c1_typed_buffer_position bp ON bp.source=d.source AND bp.task=d.parent_task
      JOIN _oo.native_c1_baseline_run run ON run.source=d.source AND run.task_id=d.task
      WHERE d.source=s AND d.task=t AND d.seeded
      AND bp.apply_ordinal=(d.previous->'buffer_cursor'->>'transaction_ordinal')::bigint
      AND bp.apply_ordinal=run.l0_pos
      AND bp.transaction_id=d.previous->'buffer_cursor'->>'transaction_id';
    IF NOT FOUND AND EXISTS(SELECT 1 FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t AND l0_pos<>0) THEN
      RAISE EXCEPTION 'durable parent predecessor absent' USING ERRCODE='OO030';
    END IF;
  END IF;
  IF r.position<>COALESCE(prev.apply_ordinal,0)+1 OR (checkpoint->>'commit_scn')::numeric<COALESCE(prev.commit_scn,b.snapshot_scn) THEN
    RAISE EXCEPTION 'buffer order or source SCN regression' USING ERRCODE='OO030';
  END IF;
  INSERT INTO _oo.c1_typed_buffer_position VALUES(s,t,p_seq,r.position::bigint,checkpoint->>'transaction_id',
    (checkpoint->>'commit_scn')::numeric,p);
END $fn$;
