-- Library publication preserves the already published C1 receipt and its T cut.
CREATE FUNCTION _oo.native_library_c1_shard(p_source text) RETURNS jsonb
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
  OR b.receipt->>'position_codec' IS DISTINCT FROM w.position_codec
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
REVOKE ALL ON FUNCTION _oo.native_library_c1_shard(text) FROM PUBLIC,oo_pipeline_runtime;
DO $grant$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_target_admin') THEN
  GRANT EXECUTE ON FUNCTION _oo.native_library_c1_shard(text) TO oo_target_admin;
 END IF;
END $grant$;

CREATE FUNCTION _oo.native_library_check_shard(p_shard jsonb) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
BEGIN
 IF p_shard->'receipt'->>'consistency'='transactional' THEN
  PERFORM 1 FROM _oo.source_instance WHERE source=p_shard->>'source_id' FOR SHARE;
  -- Target owner may read the terminal control receipt, never acquire jobs write privileges.
  PERFORM 1 FROM _oo.native_c1_baseline_typed bt
   JOIN _oo.native_c1_baseline_run br ON br.source=bt.source AND br.task_id=bt.task_id
   JOIN jobs.tasks j ON j.id=bt.task_id WHERE bt.source=p_shard->>'source_id'
   ORDER BY bt.task_id FOR SHARE OF bt,br;
  PERFORM 1 FROM _oo.full_import_run r JOIN _oo.full_import_table t USING(run_id)
   WHERE r.source_id=p_shard->>'source_id' ORDER BY r.run_id FOR SHARE OF r,t;
  IF p_shard IS DISTINCT FROM _oo.native_library_c1_shard(p_shard->>'source_id') THEN
   RAISE EXCEPTION 'C1 library exact original shard required' USING ERRCODE='OO030'; END IF;
  PERFORM _oo.publish_native_c1_collection((p_shard->>'anchor_run')::uuid,p_shard->>'receipt_sha256',p_shard->'receipt');
 ELSE
  PERFORM _oo.publish_native_per_table_collection((p_shard->>'anchor_run')::uuid,p_shard->>'receipt_sha256',p_shard->'receipt');
 END IF;
END $body$;
REVOKE ALL ON FUNCTION _oo.native_library_check_shard(jsonb) FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.native_library_c1_current(p_shard jsonb) RETURNS boolean
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
BEGIN
 RETURN p_shard IS NOT DISTINCT FROM _oo.native_library_c1_shard(p_shard->>'source_id');
EXCEPTION WHEN SQLSTATE 'OO030' THEN RETURN false;
END $body$;
REVOKE ALL ON FUNCTION _oo.native_library_c1_current(jsonb) FROM PUBLIC,oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_library_c1_shard(text),
 _oo.native_library_check_shard(jsonb),_oo.native_library_c1_current(jsonb) TO oo_target_owner;

CREATE OR REPLACE FUNCTION _oo.publish_native_library(p_library jsonb,p_sha text,p_shards jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
DECLARE limits _oo.native_library_limits%ROWTYPE; head _oo.native_library_head%ROWTYPE;
 shard jsonb; member jsonb; existing _oo.native_library_publication%ROWTYPE;
 library_name text; n integer; total integer; source_name text; actual jsonb; expected jsonb; table_entry jsonb;
BEGIN
 IF jsonb_typeof(p_library->'manifest') IS DISTINCT FROM 'object'
  OR p_library->'manifest'->>'protocol' IS DISTINCT FROM 'oo.library.membership/v1'
  OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_library->'manifest') key)
   IS DISTINCT FROM ARRAY['library_id','protocol','sources']::text[]
  OR jsonb_typeof(p_library->'manifest'->'sources') IS DISTINCT FROM 'array'
  OR jsonb_typeof(p_shards) IS DISTINCT FROM 'array'
 THEN RAISE EXCEPTION 'closed library membership required' USING ERRCODE='OO030'; END IF;
 FOR member IN SELECT value FROM jsonb_array_elements(p_library->'manifest'->'sources') LOOP
  IF jsonb_typeof(member) IS DISTINCT FROM 'object'
   OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(member) key)
    IS DISTINCT FROM ARRAY['schema','source_id','tables']::text[]
   OR jsonb_typeof(member->'source_id') IS DISTINCT FROM 'string'
   OR jsonb_typeof(member->'schema') IS DISTINCT FROM 'string'
   OR coalesce(member->>'schema','')='' OR coalesce(member->>'source_id','')=''
   OR jsonb_typeof(member->'tables') IS DISTINCT FROM 'array'
   OR EXISTS(SELECT 1 FROM jsonb_array_elements(member->'tables') WHERE jsonb_typeof(value)<>'string' OR value#>>'{}'='')
  THEN RAISE EXCEPTION 'closed library source required' USING ERRCODE='OO030'; END IF;
 END LOOP;
 FOR shard IN SELECT value FROM jsonb_array_elements(p_shards) LOOP
  IF shard->'receipt'->>'consistency'='transactional' THEN CONTINUE; END IF;
  IF jsonb_typeof(shard->'receipt') IS DISTINCT FROM 'object'
   OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(shard->'receipt') key)
    IS DISTINCT FROM ARRAY['applied_pos','collection_mode','consistency','consistency_grade','cross_table_consistent','position_codec','position_lineage','protocol','run_id','run_ids','source_generation','source_id','tables','target_generation']::text[]
   OR jsonb_typeof(shard->'receipt'->'tables') IS DISTINCT FROM 'array'
  THEN RAISE EXCEPTION 'closed library source receipt required' USING ERRCODE='OO030'; END IF;
  FOR table_entry IN SELECT value FROM jsonb_array_elements(shard->'receipt'->'tables') LOOP
   IF jsonb_typeof(table_entry) IS DISTINCT FROM 'object'
    OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(table_entry-ARRAY['rejects','position_lineage']) key)
     IS DISTINCT FROM ARRAY['accepted_rows','applied_pos','canonical_digest','capture_method','consistency_grade','delete_detection','physical_name','run_id','schema','source_generation','table','target_generation']::text[]
   THEN RAISE EXCEPTION 'closed library table receipt required' USING ERRCODE='OO030'; END IF;
  END LOOP;
 END LOOP;

 IF jsonb_typeof(p_library) IS DISTINCT FROM 'object'
  OR p_library->>'protocol' IS DISTINCT FROM 'oo.pipeline.native-library-publication/v1'
  OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_library) key)
   IS DISTINCT FROM ARRAY['expected_tables','library_id','manifest','manifest_sha256','protocol','shards']::text[]
  OR jsonb_typeof(p_library->'library_id') IS DISTINCT FROM 'string'
  OR length(p_library->>'library_id') NOT BETWEEN 1 AND 128
  OR p_library->>'manifest_sha256' IS NULL OR p_library->>'manifest_sha256' !~ '^[0-9a-f]{64}$'
  OR p_sha IS NULL OR p_sha !~ '^[0-9a-f]{64}$'
  OR jsonb_typeof(p_library->'shards') IS DISTINCT FROM 'array'
  OR jsonb_typeof(p_library->'manifest') IS DISTINCT FROM 'object'
  OR jsonb_typeof(p_library->'manifest'->'sources') IS DISTINCT FROM 'array'
  OR jsonb_typeof(p_library->'expected_tables') IS DISTINCT FROM 'number'
  OR (p_library->>'expected_tables') !~ '^[1-9][0-9]*$'
  OR jsonb_typeof(p_shards) IS DISTINCT FROM 'array'
 THEN RAISE EXCEPTION 'closed library publication receipt required' USING ERRCODE='OO030'; END IF;
 library_name:=p_library->>'library_id';
 IF p_library->'manifest'->>'library_id' IS DISTINCT FROM library_name
  OR encode(sha256(convert_to(_oo.native_library_canonical(p_library->'manifest'),'UTF8')),'hex')<>p_library->>'manifest_sha256'
  OR encode(sha256(convert_to(_oo.native_library_canonical(jsonb_build_object('library',p_library,'shards',p_shards)),'UTF8')),'hex')<>p_sha
 THEN RAISE EXCEPTION 'library digest differs' USING ERRCODE='OO030'; END IF;
 PERFORM pg_advisory_xact_lock(6404202661);
 SELECT * INTO limits FROM _oo.native_library_limits FOR SHARE;
 n:=jsonb_array_length(p_shards);
 IF n<1 OR n>limits.max_shards OR (p_library->>'expected_tables')::numeric>limits.max_tables
  OR jsonb_array_length(p_library->'shards')<>n
  OR jsonb_array_length(p_library->'manifest'->'sources')<>n
  OR (SELECT count(DISTINCT value->>'source_id') FROM jsonb_array_elements(p_shards))<>n
  OR (SELECT count(DISTINCT value) FROM jsonb_array_elements(p_library->'shards'))<>n
  OR (SELECT count(DISTINCT value->>'source_id') FROM jsonb_array_elements(p_library->'manifest'->'sources'))<>n
 THEN RAISE EXCEPTION 'complete library membership required' USING ERRCODE='OO030'; END IF;
 SELECT * INTO head FROM _oo.native_library_head WHERE library_id=library_name FOR UPDATE;
 IF FOUND AND head.manifest_sha256<>p_library->>'manifest_sha256' THEN
  RAISE EXCEPTION 'registered library membership is immutable' USING ERRCODE='OO030'; END IF;
 FOR shard IN SELECT value FROM jsonb_array_elements(p_shards) ORDER BY value->>'source_id' COLLATE "C" LOOP
  IF jsonb_typeof(shard) IS DISTINCT FROM 'object'
   OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(shard) key)
    IS DISTINCT FROM ARRAY['anchor_run','receipt','receipt_sha256','source_id']::text[]
   OR shard->>'source_id' IS NULL OR shard->>'source_id'=''
   OR shard->>'anchor_run' IS DISTINCT FROM shard->'receipt'->>'run_id'
   OR shard->>'source_id' IS DISTINCT FROM shard->'receipt'->>'source_id'
   OR NOT (shard->'receipt'->>'collection_mode'='per_table' OR shard->'receipt'->>'consistency'='transactional')
   OR jsonb_typeof(shard->'receipt'->'tables') IS DISTINCT FROM 'array'
   OR shard->>'receipt_sha256' IS NULL
   OR encode(sha256(convert_to(_oo.native_library_canonical(shard->'receipt'),'UTF8')),'hex')<>shard->>'receipt_sha256'
   OR NOT p_library->'shards' @> jsonb_build_array(shard->>'source_id')
  THEN RAISE EXCEPTION 'closed library shard receipt required' USING ERRCODE='OO030'; END IF;
  source_name:=shard->>'source_id';
  SELECT value INTO member FROM jsonb_array_elements(p_library->'manifest'->'sources') WHERE value->>'source_id'=source_name;
  IF member IS NULL OR jsonb_typeof(member->'tables') IS DISTINCT FROM 'array'
   OR EXISTS(SELECT 1 FROM _oo.native_library_member WHERE source_id=source_name AND library_id<>library_name)
  THEN RAISE EXCEPTION 'library source membership differs' USING ERRCODE='OO030'; END IF;
  SELECT jsonb_agg(jsonb_build_array(coalesce(value->>'storage_schema',value->>'schema'),coalesce(value->>'storage_table',value->>'table')) ORDER BY coalesce(value->>'storage_schema',value->>'schema'),coalesce(value->>'storage_table',value->>'table'))
   INTO actual FROM jsonb_array_elements(shard->'receipt'->'tables');
  SELECT jsonb_agg(jsonb_build_array(member->>'schema',value#>>'{}') ORDER BY member->>'schema',value#>>'{}')
   INTO expected FROM jsonb_array_elements(member->'tables');
  IF actual IS DISTINCT FROM expected THEN
   RAISE EXCEPTION 'library manifest table coverage differs' USING ERRCODE='OO030'; END IF;
 END LOOP;
 SELECT sum(jsonb_array_length(value->'receipt'->'tables')) INTO total FROM jsonb_array_elements(p_shards);
 IF total IS DISTINCT FROM (p_library->>'expected_tables')::integer OR total>limits.max_tables THEN
  RAISE EXCEPTION 'library table ceiling or coverage differs' USING ERRCODE='OO030'; END IF;
 PERFORM 1 FROM _oo.source_watermark WHERE source IN (SELECT value->>'source_id' FROM jsonb_array_elements(p_shards)) ORDER BY source FOR UPDATE;
 PERFORM 1 FROM _oo.table_freshness WHERE source IN (SELECT value->>'source_id' FROM jsonb_array_elements(p_shards)) ORDER BY source,schema_name,table_name FOR UPDATE;
 SELECT * INTO existing FROM _oo.native_library_publication WHERE receipt_sha256=p_sha;
 IF FOUND AND (existing.library<>p_library OR existing.shards<>p_shards OR head.receipt_sha256 IS DISTINCT FROM p_sha) THEN
  RAISE EXCEPTION 'library replay conflicts or is superseded' USING ERRCODE='OO030'; END IF;
 FOR shard IN SELECT value FROM jsonb_array_elements(p_shards) ORDER BY value->>'source_id' COLLATE "C" LOOP
  PERFORM _oo.native_library_check_shard(shard);
 END LOOP;
 IF existing.receipt_sha256 IS NOT NULL THEN
  RETURN jsonb_build_object('outcome','replay','library_id',library_name,'receipt_sha256',p_sha,'tables',total,'state',head.state);
 END IF;
 INSERT INTO _oo.native_library_publication(receipt_sha256,library_id,library,shards,publication_xid)
 VALUES(p_sha,library_name,p_library,p_shards,pg_current_xact_id());
 INSERT INTO _oo.native_library_head VALUES(library_name,p_library->>'manifest_sha256',p_sha,'pending')
 ON CONFLICT(library_id) DO UPDATE SET receipt_sha256=EXCLUDED.receipt_sha256,state='pending';
 INSERT INTO _oo.native_library_member SELECT value->>'source_id',library_name FROM jsonb_array_elements(p_shards)
 ON CONFLICT(source_id) DO NOTHING;
 RETURN jsonb_build_object('outcome','published','library_id',library_name,'receipt_sha256',p_sha,'tables',total,'state','pending','ready',false);
EXCEPTION WHEN invalid_text_representation OR numeric_value_out_of_range THEN
 RAISE EXCEPTION 'closed library scalar required' USING ERRCODE='OO030';
END
$body$;


CREATE OR REPLACE VIEW _oo.native_library_source_status AS
 SELECT shard->>'source_id' AS source_id,shard->>'receipt_sha256' AS receipt_sha256,
 h.library_id,h.receipt_sha256 AS library_receipt_sha256,'superseded_by_library'::text AS status
 FROM _oo.native_library_head h JOIN _oo.native_library_publication p ON p.receipt_sha256=h.receipt_sha256
 CROSS JOIN LATERAL jsonb_array_elements(p.shards) shard;
REVOKE ALL ON _oo.native_library_source_status FROM PUBLIC,oo_pipeline_runtime;

CREATE OR REPLACE FUNCTION _oo.native_library_source_ready(p_source text,p_receipt text) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
 SELECT NOT EXISTS(SELECT 1 FROM _oo.native_library_member WHERE source_id=p_source)
 OR EXISTS(
  SELECT 1 FROM _oo.native_library_member m JOIN _oo.native_library_head h USING(library_id)
  JOIN _oo.native_library_publication p ON p.receipt_sha256=h.receipt_sha256
  WHERE m.source_id=p_source AND h.state='ready'
   AND EXISTS(SELECT 1 FROM jsonb_array_elements(p.shards) s WHERE s->>'source_id'=p_source AND s->>'receipt_sha256'=p_receipt)
   AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p.shards) s
    WHERE s->'receipt'->>'consistency'='transactional' AND NOT _oo.native_library_c1_current(s))
   AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p.shards) s LEFT JOIN _oo.source_watermark w ON w.source=s->>'source_id'
    WHERE w.source IS NULL OR w.state<>'live' OR w.source_generation IS DISTINCT FROM (s->'receipt'->>'source_generation')::bigint
     OR w.target_generation IS DISTINCT FROM (s->'receipt'->>'target_generation')::bigint)
   AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p.shards) s CROSS JOIN LATERAL jsonb_array_elements(s->'receipt'->'tables') t
    LEFT JOIN _oo.table_freshness f ON f.source=s->>'source_id' AND f.schema_name=coalesce(t->>'storage_schema',t->>'schema') AND f.table_name=coalesce(t->>'storage_table',t->>'table')
    WHERE f.source IS NULL OR f.state<>'live' OR f.source_generation IS DISTINCT FROM (t->>'source_generation')::bigint
     OR f.target_generation IS DISTINCT FROM (t->>'target_generation')::bigint OR f.physical_name IS DISTINCT FROM t->>'physical_name')
 )
$body$;

CREATE OR REPLACE FUNCTION _oo.native_library_mark_ready(p_library text,p_sha text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $body$
DECLARE publication _oo.native_library_publication%ROWTYPE; shard jsonb; missing boolean;
BEGIN
 PERFORM 1 FROM _oo.native_library_head WHERE library_id=p_library AND receipt_sha256=p_sha FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'current library pointer required' USING ERRCODE='OO030'; END IF;
 SELECT * INTO publication FROM _oo.native_library_publication WHERE receipt_sha256=p_sha;
 PERFORM 1 FROM _oo.source_watermark
  WHERE source IN (SELECT value->>'source_id' FROM jsonb_array_elements(publication.shards))
  ORDER BY source FOR UPDATE;
 PERFORM 1 FROM _oo.table_freshness
  WHERE source IN (SELECT value->>'source_id' FROM jsonb_array_elements(publication.shards))
  ORDER BY source,schema_name,table_name FOR UPDATE;
 FOR shard IN SELECT value FROM jsonb_array_elements(publication.shards) ORDER BY value->>'source_id' LOOP
  PERFORM _oo.native_library_check_shard(shard);
 END LOOP;
 IF to_regclass('oo_publish_target.fence') IS NULL THEN
  RAISE EXCEPTION 'library semantic fences absent' USING ERRCODE='OO030'; END IF;
 EXECUTE 'SELECT EXISTS(SELECT 1 FROM jsonb_array_elements($1) s CROSS JOIN LATERAL jsonb_array_elements(s->''receipt''->''tables'') t WHERE NOT EXISTS(SELECT 1 FROM oo_publish_target.fence f WHERE f.schema_name=''_oo'' AND f.relation_name=t->>''physical_name'' AND f.state=''open'' AND f.opened_pack_digest ~ ''^[0-9a-f]{64}$'' AND f.opened_version IS NOT NULL))' INTO missing USING publication.shards;
 IF missing THEN RAISE EXCEPTION 'all library fences must be open' USING ERRCODE='OO030'; END IF;
 UPDATE _oo.native_library_head SET state='ready' WHERE library_id=p_library;
END
$body$;
REVOKE ALL ON FUNCTION _oo.native_library_source_ready(text,text),_oo.native_library_mark_ready(text,text) FROM PUBLIC,oo_pipeline_runtime;
DO $grant$
BEGIN
 IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_target_admin') THEN
  GRANT EXECUTE ON FUNCTION _oo.native_library_source_ready(text,text),_oo.native_library_mark_ready(text,text) TO oo_target_admin;
 END IF;
END
$grant$;
