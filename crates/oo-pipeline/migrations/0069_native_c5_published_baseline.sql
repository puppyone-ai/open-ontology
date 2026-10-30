-- native-c5-published-baseline/v1: append after 0068; do not rewrite 0065.
-- Source key order is an explicit pinned assertion, never PG index/column order.
ALTER TABLE _oo.c5_multipart_baseline ADD COLUMN baseline_kind text NOT NULL DEFAULT 'multipart'
 CHECK(baseline_kind IN ('single','multipart'));

CREATE FUNCTION _oo.c5_single_baseline_receipt(p_run uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE r _oo.full_import_run; t _oo.full_import_table; b _oo.update_batch;
BEGIN
 SELECT * INTO STRICT r FROM _oo.full_import_run WHERE run_id=p_run;
 SELECT * INTO STRICT t FROM _oo.full_import_table WHERE run_id=p_run;
 SELECT * INTO STRICT b FROM _oo.update_batch WHERE run_id=p_run;
 IF r.multipart_scope IS NOT NULL OR r.state NOT IN ('awaiting_publish','published')
  OR r.sealed_at IS NULL OR r.retired_at IS NOT NULL OR NOT t.exhausted
  OR t.applied_rows NOT BETWEEN 0 AND 100000 OR t.applied_pos IS DISTINCT FROM 0
  OR t.canonical_digest IS NULL OR b.source IS DISTINCT FROM r.source_id
  OR b.schema_name IS DISTINCT FROM t.schema_ident OR b.table_name IS DISTINCT FROM t.table_ident
  OR b.physical_name IS DISTINCT FROM t.physical_name OR b.source_generation IS DISTINCT FROM r.source_generation
  OR b.target_generation IS DISTINCT FROM r.target_generation OR b.row_count IS DISTINCT FROM t.applied_rows
  OR b.tombstone_count<>0
  OR (SELECT count(*) FROM _oo.apply_receipts WHERE source=r.source_id AND table_generation=t.physical_name)<>1
  OR (SELECT count(*) FROM _oo.full_import_quality WHERE run_id=p_run)<>1
  OR NOT EXISTS(SELECT FROM _oo.apply_receipts a JOIN _oo.full_import_quality q
   ON q.run_id=p_run AND q.physical_name=a.table_generation AND q.batch_id=a.txn_or_batch_id
   WHERE a.source=r.source_id AND a.table_generation=t.physical_name AND a.txn_or_batch_id=b.batch_id
    AND a.payload_digest=b.payload_digest AND q.payload_digest=b.payload_digest
    AND a.config_digest=r.config_digest AND a.type_digest=r.type_digest AND a.schema_digest=r.schema_digest
    AND a.fence_shard=r.shard_id AND a.fence_generation=r.target_generation
    AND a.position_codec='snapshot-zero/v1' AND a.applied_pos=0 AND a.txn_complete
    AND NOT a.partial_txn AND NOT a.cross_table_consistent
    AND q.kind='batch' AND q.accepted_rows=t.applied_rows AND q.rejected_rows=0 AND q.input_digest=r.input_digest)
 THEN RAISE EXCEPTION 'C5 sealed single baseline receipt differs' USING ERRCODE='OO030'; END IF;
 RETURN jsonb_build_object('run_id',r.run_id,'source_id',r.source_id,'schema',t.schema_ident,'table',t.table_ident,
  'source_generation',r.source_generation,'target_generation',r.target_generation,'physical_name',t.physical_name,
  'input_digest',r.input_digest,'schema_digest',r.schema_digest,'type_digest',r.type_digest,
  'config_digest',r.config_digest,'row_count',t.applied_rows,'batch_id',b.batch_id,'payload_digest',b.payload_digest,
  'type_policy_version','type-policy/v1');
END $$;
REVOKE ALL ON FUNCTION _oo.c5_single_baseline_receipt(uuid) FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_initialize_published_if_absent(
 p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,
 p_fingerprint text,p_topology bigint,p_keys text[],p_expected_source_generation bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE w _oo.source_watermark; f _oo.table_freshness; r _oo.full_import_run;
 t _oo.full_import_table; state _oo.c5_table; baseline _oo.c5_multipart_baseline;
 manifest jsonb; topology jsonb; part jsonb; ch jsonb;
 kinds text[]:=ARRAY[]::text[]; col text; kind oid; attrs smallint[]:=ARRAY[]::smallint[];
 att smallint; relation oid; rows bigint:=0; i integer:=0; already boolean;
 selected text[]; projection text; baseline_kind text; proof jsonb;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 IF p_fingerprint IS NULL OR p_fingerprint !~ '^[0-9a-f]{64}$' OR p_topology IS DISTINCT FROM 1
  OR p_expected_source_generation IS NULL OR p_expected_source_generation<1
  OR p_keys IS NULL OR cardinality(p_keys) NOT BETWEEN 1 AND 64
  OR EXISTS(SELECT FROM unnest(p_keys) k WHERE k IS NULL OR k='')
  OR (SELECT count(DISTINCT k) FROM unnest(p_keys) k)<>cardinality(p_keys)
 THEN RAISE EXCEPTION 'C5 multipart initialization scope' USING ERRCODE='22023'; END IF;
 SELECT * INTO STRICT w FROM _oo.source_watermark WHERE source=p_source FOR UPDATE;
 IF w.source_generation IS DISTINCT FROM p_expected_source_generation THEN
  RAISE EXCEPTION 'C5 multipart source CAS differs' USING ERRCODE='OO010';
 END IF;
 SELECT * INTO STRICT f FROM _oo.table_freshness
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 IF f.source_generation<1 OR f.source_generation>w.source_generation
  OR f.state NOT IN ('awaiting_publish','live')
 THEN RAISE EXCEPTION 'C5 multipart table freshness differs' USING ERRCODE='OO030'; END IF;
 SELECT * INTO state FROM _oo.c5_table
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 already:=FOUND;
 SELECT * INTO baseline FROM _oo.c5_multipart_baseline
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
 IF already THEN
  IF baseline.run_id IS NULL OR state.schema_fingerprint IS DISTINCT FROM p_fingerprint
   OR state.topology_revision IS DISTINCT FROM p_topology OR baseline.key_columns IS DISTINCT FROM p_keys
   OR state.source_generation IS DISTINCT FROM f.source_generation
  THEN RAISE EXCEPTION 'C5 multipart existing state differs' USING ERRCODE='OO010'; END IF;
  SELECT * INTO STRICT r FROM _oo.full_import_run WHERE run_id=baseline.run_id FOR SHARE;
 ELSE
  IF f.mode IS DISTINCT FROM 'full' THEN RAISE EXCEPTION 'C5 multipart full baseline required' USING ERRCODE='OO030'; END IF;
  SELECT run.* INTO STRICT r FROM _oo.full_import_run run JOIN _oo.full_import_table ft USING(run_id)
   WHERE run.source_id=p_source AND run.source_generation=f.source_generation
    AND ft.physical_name=f.physical_name AND ft.schema_ident=p_schema AND ft.table_ident=p_table
   FOR SHARE OF run;
 END IF;
 SELECT * INTO STRICT t FROM _oo.full_import_table WHERE run_id=r.run_id
  AND schema_ident=p_schema AND table_ident=p_table FOR SHARE;
 manifest:=r.multipart_scope;
 baseline_kind:=CASE WHEN manifest IS NULL THEN 'single' ELSE 'multipart' END;
 IF r.source_id IS DISTINCT FROM p_source OR r.state NOT IN ('awaiting_publish','published')
  OR r.sealed_at IS NULL OR r.retired_at IS NOT NULL OR NOT t.exhausted
  OR t.applied_pos IS DISTINCT FROM 0 OR t.canonical_digest IS NULL
  OR (SELECT count(*) FROM _oo.full_import_table WHERE run_id=r.run_id)<>1
  OR (manifest IS NOT NULL AND jsonb_typeof(manifest) IS DISTINCT FROM 'object')
 THEN RAISE EXCEPTION 'C5 multipart sealed baseline required' USING ERRCODE='OO030'; END IF;
 IF manifest IS NOT NULL THEN
 IF (SELECT count(*) FROM jsonb_object_keys(manifest))<>12
  OR NOT manifest ?& ARRAY['protocol','source_id','source_generation','occurrence','schema','table',
   'schema_sha256','capture_config_sha256','manifest_sha256','chunks','part_count','parts']
  OR manifest->>'protocol' IS DISTINCT FROM 'oo.mysql.c5-multipart/v1'
  OR manifest->>'source_id' IS DISTINCT FROM p_source OR manifest->>'schema' IS DISTINCT FROM p_schema
  OR manifest->>'table' IS DISTINCT FROM p_table OR manifest->'source_generation' IS DISTINCT FROM to_jsonb(r.source_generation)
  OR manifest->>'schema_sha256' IS DISTINCT FROM r.schema_digest
  OR coalesce(manifest->>'occurrence','') !~ '^[A-Za-z0-9_.:-]{1,110}$'
  OR coalesce(manifest->>'capture_config_sha256','') !~ '^[0-9a-f]{64}$'
  OR coalesce(manifest->>'manifest_sha256','') !~ '^[0-9a-f]{64}$'
 THEN RAISE EXCEPTION 'C5 multipart manifest identity differs' USING ERRCODE='OO030'; END IF;
 END IF;
 IF already AND (baseline.baseline_kind IS DISTINCT FROM baseline_kind OR baseline.source_generation IS DISTINCT FROM r.source_generation
  OR baseline.physical_name IS DISTINCT FROM t.physical_name OR baseline.config_digest IS DISTINCT FROM r.config_digest
  OR baseline.schema_digest IS DISTINCT FROM r.schema_digest OR baseline.type_digest IS DISTINCT FROM r.type_digest)
 THEN RAISE EXCEPTION 'C5 multipart baseline identity differs' USING ERRCODE='OO030'; END IF;
 IF NOT already AND (t.physical_name IS DISTINCT FROM f.physical_name OR r.target_generation IS DISTINCT FROM f.target_generation)
 THEN RAISE EXCEPTION 'C5 multipart physical baseline differs' USING ERRCODE='OO030'; END IF;
 relation:=to_regclass(format('_oo.%I',t.physical_name));
 IF relation IS NULL OR NOT EXISTS(SELECT FROM pg_class WHERE oid=relation AND relkind='r'
   AND relowner=(SELECT oid FROM pg_roles WHERE rolname='oo_target_owner'))
 THEN RAISE EXCEPTION 'C5 multipart baseline relation differs' USING ERRCODE='OO030'; END IF;
 FOREACH col IN ARRAY p_keys LOOP
  col:=_oo.lookup_physical_mapping(p_source,r.config_digest,r.source_generation,'column',p_schema,p_table,col);
  SELECT atttypid,attnum INTO STRICT kind,att FROM pg_attribute
   WHERE attrelid=relation AND attname=col AND NOT attisdropped AND attnotnull AND attnum>0;
  attrs:=array_append(attrs,att);
  kinds:=array_append(kinds,CASE WHEN kind IN (20,21,23) THEN 'Int'
   WHEN kind IN (25,1042,1043) THEN 'Text' WHEN kind=17 THEN 'Bytes' ELSE NULL END);
 END LOOP;
 IF array_position(kinds,NULL) IS NOT NULL OR (baseline_kind='multipart' AND NOT EXISTS(SELECT FROM pg_index ix
   WHERE ix.indrelid=relation AND ix.indisunique AND ix.indisvalid AND ix.indisready
    AND ix.indpred IS NULL AND ix.indexprs IS NULL AND ix.indnkeyatts=cardinality(attrs)
    AND ARRAY(SELECT unnest(ix.indkey) LIMIT ix.indnkeyatts) @> attrs
    AND ARRAY(SELECT unnest(ix.indkey) LIMIT ix.indnkeyatts) <@ attrs))
 THEN RAISE EXCEPTION 'C5 multipart stable typed keys required' USING ERRCODE='OO030'; END IF;
 IF already AND baseline.key_kinds IS DISTINCT FROM kinds THEN
  RAISE EXCEPTION 'C5 multipart key types differ' USING ERRCODE='OO030';
 END IF;
 SELECT array_agg(m.logical_column ORDER BY a.attnum),
  encode(sha256(convert_to('['||string_agg(to_json(m.logical_column)::text,',' ORDER BY a.attnum)||']','UTF8')),'hex')
 INTO selected,projection FROM pg_attribute a JOIN _oo.physical_mappings m
  ON m.physical_name=a.attname AND m.source=p_source AND m.config_digest=r.config_digest
   AND m.run_generation=r.source_generation AND m.level='c'
   AND m.logical_namespace=p_schema AND m.logical_table=p_table
 WHERE a.attrelid=relation AND a.attnum>0 AND NOT a.attisdropped;
 IF selected IS NULL OR cardinality(selected)<>(SELECT count(*) FROM pg_attribute
   WHERE attrelid=relation AND attnum>0 AND NOT attisdropped AND attname<>'_oo_pos')
  OR (already AND (baseline.selected_columns IS DISTINCT FROM selected OR baseline.projection_sha256 IS DISTINCT FROM projection))
 THEN RAISE EXCEPTION 'C5 multipart projection differs' USING ERRCODE='OO030'; END IF;
 IF baseline_kind='single' THEN
  proof:=_oo.c5_single_baseline_receipt(r.run_id);
  topology:=jsonb_build_array(jsonb_build_object('chunk_id','0','lo','null'::jsonb,'hi','null'::jsonb,
   'row_count',t.applied_rows,'arrow_sha256',r.input_digest));
 ELSE
 topology:=_oo.c5_multipart_topology(manifest,kinds);
 IF EXISTS(SELECT FROM _oo.full_import_quality q WHERE q.run_id=r.run_id AND q.kind='reject')
  OR (SELECT count(*) FROM _oo.full_import_quality q WHERE q.run_id=r.run_id AND q.kind='batch')<>jsonb_array_length(topology)+1
  OR (SELECT count(*) FROM _oo.apply_receipts WHERE source=p_source AND table_generation=t.physical_name)<>jsonb_array_length(topology)+1
 THEN RAISE EXCEPTION 'C5 multipart receipt coverage differs' USING ERRCODE='OO030'; END IF;
 -- finalize writes a zero-row ACK after all part ACKs, then seals update_batch.
 IF (SELECT count(*) FROM _oo.update_batch ub WHERE ub.run_id=r.run_id)<>1 OR NOT EXISTS(
  SELECT FROM _oo.update_batch ub JOIN _oo.apply_receipts a
   ON a.source=ub.source AND a.table_generation=ub.physical_name AND a.txn_or_batch_id=ub.batch_id AND a.payload_digest=ub.payload_digest
  JOIN _oo.full_import_quality q ON q.run_id=ub.run_id AND q.physical_name=ub.physical_name AND q.batch_id=ub.batch_id AND q.kind='batch'
  WHERE ub.run_id=r.run_id AND ub.source=p_source AND ub.schema_name=p_schema AND ub.table_name=p_table
   AND ub.source_generation=r.source_generation AND ub.target_generation=r.target_generation
   AND ub.physical_name=t.physical_name AND ub.row_count=t.applied_rows AND ub.tombstone_count=0
   AND q.accepted_rows=0 AND q.rejected_rows=0 AND q.input_digest=r.input_digest AND q.payload_digest=ub.payload_digest
   AND a.config_digest=r.config_digest AND a.type_digest=r.type_digest AND a.schema_digest=r.schema_digest
   AND a.fence_shard=r.shard_id AND a.fence_generation=r.target_generation
   AND a.position_codec='snapshot-zero/v1' AND a.applied_pos=0 AND a.txn_complete
   AND NOT a.partial_txn AND NOT a.cross_table_consistent
   AND NOT EXISTS(SELECT FROM jsonb_array_elements(manifest->'parts') p
    WHERE ub.batch_id=(manifest->>'occurrence')||'.part.'||(p->>'index')))
 THEN RAISE EXCEPTION 'C5 multipart final receipt differs' USING ERRCODE='OO030'; END IF;
 FOR part IN SELECT value FROM jsonb_array_elements(manifest->'parts') LOOP
  IF NOT EXISTS(SELECT FROM _oo.apply_receipts a JOIN _oo.full_import_quality q
    ON q.run_id=r.run_id AND q.physical_name=a.table_generation AND q.batch_id=a.txn_or_batch_id AND q.kind='batch'
    WHERE a.source=p_source AND a.table_generation=t.physical_name
     AND a.txn_or_batch_id=(manifest->>'occurrence')||'.part.'||i::text
     AND a.payload_digest=part->>'arrow_sha256' AND q.payload_digest=a.payload_digest
     AND a.config_digest=r.config_digest AND a.type_digest=r.type_digest AND a.schema_digest=r.schema_digest
     AND a.fence_shard=r.shard_id AND a.fence_generation=r.target_generation
     AND a.position_codec='snapshot-zero/v1' AND a.applied_pos=0 AND a.txn_complete
     AND NOT a.partial_txn AND NOT a.cross_table_consistent
     AND q.input_digest=r.input_digest AND q.accepted_rows=(part->>'rows')::bigint AND q.rejected_rows=0)
  THEN RAISE EXCEPTION 'C5 multipart part receipt differs' USING ERRCODE='OO030'; END IF;
  rows:=rows+(part->>'rows')::bigint; i:=i+1;
 END LOOP;
 IF rows<>t.applied_rows THEN RAISE EXCEPTION 'C5 multipart baseline row coverage differs' USING ERRCODE='OO030'; END IF;
 END IF;
 IF already THEN
  IF (SELECT count(*) FROM _oo.c5_chunk WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND retired_at IS NULL)<>jsonb_array_length(topology)
   OR EXISTS(SELECT FROM jsonb_array_elements(topology) planned WHERE NOT EXISTS(
    SELECT FROM _oo.c5_chunk c WHERE c.source=p_source AND c.schema_name=p_schema AND c.table_name=p_table
     AND c.retired_at IS NULL AND c.topology_revision=p_topology AND c.chunk_id=planned->>'chunk_id'
     AND c.lo=planned->'lo' AND c.hi=planned->'hi'))
  THEN RAISE EXCEPTION 'C5 multipart existing topology differs' USING ERRCODE='OO010'; END IF;
  PERFORM _oo.c5_authority_export(p_source,p_schema,p_table);
 ELSE
  INSERT INTO _oo.c5_table(source,schema_name,table_name,schema_fingerprint,topology_revision,source_generation,input_digest)
   VALUES(p_source,p_schema,p_table,p_fingerprint,1,r.source_generation,r.input_digest);
  INSERT INTO _oo.c5_multipart_baseline(source,schema_name,table_name,run_id,source_generation,physical_name,
   config_digest,schema_digest,type_digest,key_columns,key_kinds,selected_columns,projection_sha256,baseline_kind) VALUES(p_source,p_schema,p_table,r.run_id,r.source_generation,t.physical_name,
   r.config_digest,r.schema_digest,r.type_digest,p_keys,kinds,selected,projection,baseline_kind);
  FOR ch IN SELECT value FROM jsonb_array_elements(topology) LOOP
   INSERT INTO _oo.c5_chunk(source,schema_name,table_name,chunk_id,topology_revision,lo,hi,
    membership_revision,row_count,arrow_sha256,baseline_membership_pending)
   VALUES(p_source,p_schema,p_table,ch->>'chunk_id',1,ch->'lo',ch->'hi',1,(ch->>'row_count')::bigint,
    ch->>'arrow_sha256',true);
  END LOOP;
 END IF;
 RETURN jsonb_build_object('initialized',true,'created',NOT already,'receipt',NULL,
  'data_gen',false,'apply',false,'ack',false,'source_generation',w.source_generation,
  'table_source_generation',f.source_generation,'baseline_source_generation',r.source_generation,
  'chunks',jsonb_array_length(topology),'baseline_kind',baseline_kind);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_initialize_published_if_absent(text,uuid,bigint,text,text,text,text,bigint,text[],bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_initialize_published_if_absent(text,uuid,bigint,text,text,text,text,bigint,text[],bigint) TO oo_pipeline_runtime;

ALTER FUNCTION _oo.c5_multipart_hydration_manifest(text,uuid,bigint,text,text,text,text,bigint)
 RENAME TO c5_multipart_hydration_manifest_before_published;
REVOKE ALL ON FUNCTION _oo.c5_multipart_hydration_manifest_before_published(text,uuid,bigint,text,text,text,text,bigint)
 FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_multipart_hydration_manifest(p_shard text,p_owner uuid,p_fence bigint,
 p_source text,p_schema text,p_table text,p_chunk text,p_generation bigint) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE b _oo.c5_multipart_baseline; f _oo.table_freshness; s _oo.c5_table; c _oo.c5_chunk;
 high bigint; result jsonb; proof jsonb; columns jsonb; captured jsonb;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 SELECT * INTO STRICT b FROM _oo.c5_multipart_baseline
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
 IF b.baseline_kind='multipart' THEN
  result:=_oo.c5_multipart_hydration_manifest_before_published(p_shard,p_owner,p_fence,p_source,p_schema,p_table,p_chunk,p_generation);
  SELECT value INTO STRICT captured FROM jsonb_array_elements(result->'multipart_scope'->'chunks')
   WHERE value->>'chunk_id'=p_chunk;
  RETURN result||jsonb_build_object('baseline_protocol','oo.c5.published-baseline/v1','baseline_kind','multipart',
   'capture_hi',_oo.c5_mysql_bound(captured->'hi',b.key_kinds),'single_receipt',NULL);
 END IF;
 SELECT source_generation INTO STRICT high FROM _oo.source_watermark WHERE source=p_source FOR UPDATE;
 SELECT * INTO STRICT f FROM _oo.table_freshness WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 SELECT * INTO STRICT s FROM _oo.c5_table WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 SELECT * INTO STRICT c FROM _oo.c5_chunk WHERE source=p_source AND schema_name=p_schema AND table_name=p_table
  AND chunk_id=p_chunk AND retired_at IS NULL FOR UPDATE;
 proof:=_oo.c5_single_baseline_receipt(b.run_id);
 IF high IS DISTINCT FROM p_generation OR s.source_generation IS DISTINCT FROM b.source_generation
  OR f.source_generation IS DISTINCT FROM b.source_generation OR f.physical_name IS DISTINCT FROM b.physical_name
  OR f.mode IS DISTINCT FROM 'full' OR f.state NOT IN ('awaiting_publish','live') OR c.membership_revision<>1
  OR p_chunk IS DISTINCT FROM '0' OR c.lo IS DISTINCT FROM 'null'::jsonb OR c.hi IS DISTINCT FROM 'null'::jsonb
  OR proof->>'source_id' IS DISTINCT FROM p_source OR proof->>'schema' IS DISTINCT FROM p_schema
  OR proof->>'table' IS DISTINCT FROM p_table OR proof->'source_generation' IS DISTINCT FROM to_jsonb(b.source_generation)
  OR proof->>'physical_name' IS DISTINCT FROM b.physical_name
  OR proof->>'config_digest' IS DISTINCT FROM b.config_digest OR proof->>'schema_digest' IS DISTINCT FROM b.schema_digest
  OR proof->>'type_digest' IS DISTINCT FROM b.type_digest OR proof->>'input_digest' IS DISTINCT FROM c.arrow_sha256
  OR proof->'row_count' IS DISTINCT FROM to_jsonb(c.row_count)
 THEN RAISE EXCEPTION 'C5 hydration current single baseline differs' USING ERRCODE='OO010'; END IF;
 SELECT jsonb_agg(jsonb_build_object('name',m.logical_column,'target_oid',a.atttypid::bigint,
  'target_typmod',a.atttypmod,'nullable',NOT a.attnotnull) ORDER BY a.attnum) INTO columns
 FROM pg_attribute a JOIN _oo.physical_mappings m ON m.physical_name=a.attname
  AND m.source=p_source AND m.config_digest=b.config_digest AND m.run_generation=b.source_generation AND m.level='c'
  AND m.logical_namespace=p_schema AND m.logical_table=p_table
 WHERE a.attrelid=to_regclass(format('_oo.%I',b.physical_name)) AND a.attnum>0 AND NOT a.attisdropped;
 RETURN jsonb_build_object('baseline_protocol','oo.c5.published-baseline/v1','baseline_kind','single',
  'baseline',to_jsonb(b),'multipart_scope',NULL,'single_receipt',proof,'chunk_id',p_chunk,'capture_hi',NULL,
  'row_count',c.row_count,'arrow_sha256',c.arrow_sha256,'baseline_membership_pending',c.baseline_membership_pending,
  'columns',columns);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_multipart_hydration_manifest(text,uuid,bigint,text,text,text,text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_multipart_hydration_manifest(text,uuid,bigint,text,text,text,text,bigint) TO oo_pipeline_runtime;

ALTER FUNCTION _oo.c5_hydrate_multipart(text,uuid,bigint,text,text,text,text,bigint,text,jsonb)
 RENAME TO c5_hydrate_multipart_before_published;
REVOKE ALL ON FUNCTION _oo.c5_hydrate_multipart_before_published(text,uuid,bigint,text,text,text,text,bigint,text,jsonb)
 FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_hydrate_multipart(p_shard text,p_owner uuid,p_fence bigint,
 p_source text,p_schema text,p_table text,p_chunk text,p_generation bigint,p_arrow_sha256 text,p_members jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE authority jsonb; saved _oo.c5_multipart_hydration; replay boolean; result jsonb; key_sha text;
BEGIN
 authority:=_oo.c5_multipart_hydration_manifest(p_shard,p_owner,p_fence,p_source,p_schema,p_table,p_chunk,p_generation);
 IF jsonb_typeof(p_members) IS DISTINCT FROM 'array'
  OR p_arrow_sha256 IS DISTINCT FROM authority->>'arrow_sha256'
  OR jsonb_array_length(p_members) IS DISTINCT FROM (authority->>'row_count')::bigint
 THEN RAISE EXCEPTION 'C5 published hydration receipt differs' USING ERRCODE='OO030'; END IF;
 -- Compare against the actual captured endpoint, not the C5 null outer edge.
 -- Pinned source-key order selects the tuple; PG index/field order never does.
 IF authority->>'baseline_kind'='multipart' AND (
  jsonb_array_length(p_members)=0 OR authority->'capture_hi' IS NULL OR authority->'capture_hi'='null'::jsonb
  OR p_members->(jsonb_array_length(p_members)-1) IS DISTINCT FROM authority->'capture_hi')
 THEN RAISE EXCEPTION 'C5 retained source key order differs from sealed boundary' USING ERRCODE='OO030'; END IF;
 IF jsonb_array_length(p_members)>0 THEN
  result:=_oo.c5_hydrate_multipart_before_published(p_shard,p_owner,p_fence,p_source,p_schema,p_table,p_chunk,
   p_generation,p_arrow_sha256,p_members);
 ELSE
  IF authority->>'baseline_kind' IS DISTINCT FROM 'single' THEN
   RAISE EXCEPTION 'C5 empty multipart baseline unsupported' USING ERRCODE='OO030';
  END IF;
  key_sha:=encode(sha256(convert_to(p_members::text,'UTF8')),'hex');
  SELECT * INTO saved FROM _oo.c5_multipart_hydration
   WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=p_chunk;
  replay:=FOUND;
  IF EXISTS(SELECT FROM _oo.c5_member WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=p_chunk)
   OR (replay AND (saved.run_id::text IS DISTINCT FROM authority->'baseline'->>'run_id'
    OR saved.arrow_sha256 IS DISTINCT FROM p_arrow_sha256 OR saved.row_count<>0 OR saved.members_sha256 IS DISTINCT FROM key_sha
    OR authority->'baseline_membership_pending' IS DISTINCT FROM 'false'::jsonb))
   OR (NOT replay AND authority->'baseline_membership_pending' IS DISTINCT FROM 'true'::jsonb)
  THEN RAISE EXCEPTION 'C5 empty single hydration replay differs' USING ERRCODE='OO010'; END IF;
  IF NOT replay THEN
   INSERT INTO _oo.c5_multipart_hydration(source,schema_name,table_name,chunk_id,run_id,arrow_sha256,row_count,members_sha256)
    VALUES(p_source,p_schema,p_table,p_chunk,(authority->'baseline'->>'run_id')::uuid,p_arrow_sha256,0,key_sha);
   UPDATE _oo.c5_chunk SET baseline_membership_pending=false
    WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=p_chunk;
  END IF;
  result:=jsonb_build_object('hydrated',true,'replay',replay,'chunk_id',p_chunk,'rows',0,
   'data_gen',false,'apply',false,'ack',false,'receipt',NULL);
 END IF;
 RETURN result||jsonb_build_object('baseline_kind',authority->>'baseline_kind');
END $$;
REVOKE ALL ON FUNCTION _oo.c5_hydrate_multipart(text,uuid,bigint,text,text,text,text,bigint,text,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_hydrate_multipart(text,uuid,bigint,text,text,text,text,bigint,text,jsonb) TO oo_pipeline_runtime;

ALTER FUNCTION _oo.c5_authority_export(text,text,text) RENAME TO c5_authority_export_before_published;
REVOKE ALL ON FUNCTION _oo.c5_authority_export_before_published(text,text,text) FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_authority_export(p_source text,p_schema text,p_table text) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE result jsonb;
BEGIN
 result:=_oo.c5_authority_export_before_published(p_source,p_schema,p_table);
 IF result->'baseline' IS NOT NULL THEN
  result:=result||jsonb_build_object('baseline_kind',result->'baseline'->>'baseline_kind',
   'type_policy_version','type-policy/v1','type_digest',result->'baseline'->>'type_digest');
 END IF;
 RETURN result;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_authority_export(text,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_authority_export(text,text,text) TO oo_pipeline_runtime;
