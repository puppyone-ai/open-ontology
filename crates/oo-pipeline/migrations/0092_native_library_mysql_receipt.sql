-- Legacy MySQL ACKs omitted position_codec; derive only the existing MySQL GTID
-- codec from the verified live watermark/instance and exact acknowledged GTID end.
-- Explicit null/foreign codecs and Oracle receipts with a missing codec still fail.
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
 IF n IS NULL OR n<1 OR n>32 THEN RAISE EXCEPTION 'C1 library entries' USING ERRCODE='OO030'; END IF;
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
