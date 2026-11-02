-- native-c5-resumed-baseline/v1
-- Multipart recovery retains prior committed part ACKs while the sealed run adopts
-- a newer fence. Keep exact run/physical/part/hash/quality binding and require the
-- final ACK at the sealed fence; a part from a future or different shard is invalid.
CREATE OR REPLACE FUNCTION _oo.c5_initialize_published_if_absent(
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
     AND a.fence_shard=r.shard_id AND a.fence_generation BETWEEN 1 AND r.target_generation
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

-- Preserve the same historical-fence rule in archive hydration and the legacy initializer.
CREATE OR REPLACE FUNCTION _oo.c5_initialize_multipart_if_absent(
 p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,
 p_fingerprint text,p_topology bigint,p_keys text[],p_expected_source_generation bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE w _oo.source_watermark; f _oo.table_freshness; r _oo.full_import_run;
 t _oo.full_import_table; state _oo.c5_table; baseline _oo.c5_multipart_baseline;
 manifest jsonb; topology jsonb; part jsonb; ch jsonb;
 kinds text[]:=ARRAY[]::text[]; col text; kind oid; attrs smallint[]:=ARRAY[]::smallint[];
 att smallint; relation oid; rows bigint:=0; i integer:=0; already boolean;
 selected text[]; projection text;
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
 IF r.source_id IS DISTINCT FROM p_source OR r.state NOT IN ('awaiting_publish','published')
  OR r.sealed_at IS NULL OR r.retired_at IS NOT NULL OR NOT t.exhausted
  OR t.applied_pos IS DISTINCT FROM 0 OR t.canonical_digest IS NULL
  OR (SELECT count(*) FROM _oo.full_import_table WHERE run_id=r.run_id)<>1
  OR jsonb_typeof(manifest) IS DISTINCT FROM 'object'
 THEN RAISE EXCEPTION 'C5 multipart sealed baseline required' USING ERRCODE='OO030'; END IF;
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
 IF already AND (baseline.source_generation IS DISTINCT FROM r.source_generation
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
 IF array_position(kinds,NULL) IS NOT NULL OR NOT EXISTS(SELECT FROM pg_index ix
   WHERE ix.indrelid=relation AND ix.indisunique AND ix.indisvalid AND ix.indisready
    AND ix.indpred IS NULL AND ix.indexprs IS NULL AND ix.indnkeyatts=cardinality(attrs)
    AND ARRAY(SELECT unnest(ix.indkey) LIMIT ix.indnkeyatts)=attrs)
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
     AND a.fence_shard=r.shard_id AND a.fence_generation BETWEEN 1 AND r.target_generation
     AND a.position_codec='snapshot-zero/v1' AND a.applied_pos=0 AND a.txn_complete
     AND NOT a.partial_txn AND NOT a.cross_table_consistent
     AND q.input_digest=r.input_digest AND q.accepted_rows=(part->>'rows')::bigint AND q.rejected_rows=0)
  THEN RAISE EXCEPTION 'C5 multipart part receipt differs' USING ERRCODE='OO030'; END IF;
  rows:=rows+(part->>'rows')::bigint; i:=i+1;
 END LOOP;
 IF rows<>t.applied_rows THEN RAISE EXCEPTION 'C5 multipart baseline row coverage differs' USING ERRCODE='OO030'; END IF;
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
  INSERT INTO _oo.c5_multipart_baseline VALUES(p_source,p_schema,p_table,r.run_id,r.source_generation,t.physical_name,
   r.config_digest,r.schema_digest,r.type_digest,p_keys,kinds,selected,projection);
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
  'chunks',jsonb_array_length(topology));
END $$;

CREATE OR REPLACE FUNCTION _oo.c5_multipart_hydration_manifest_before_published(p_shard text,p_owner uuid,p_fence bigint,
 p_source text,p_schema text,p_table text,p_chunk text,p_generation bigint) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE b _oo.c5_multipart_baseline; f _oo.table_freshness; s _oo.c5_table;
 r _oo.full_import_run; t _oo.full_import_table; c _oo.c5_chunk; part jsonb; high bigint; columns jsonb;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 SELECT source_generation INTO STRICT high FROM _oo.source_watermark WHERE source=p_source FOR UPDATE;
 SELECT * INTO STRICT f FROM _oo.table_freshness WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 SELECT * INTO STRICT s FROM _oo.c5_table WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 SELECT * INTO STRICT b FROM _oo.c5_multipart_baseline WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
 SELECT * INTO STRICT c FROM _oo.c5_chunk WHERE source=p_source AND schema_name=p_schema AND table_name=p_table
  AND chunk_id=p_chunk AND retired_at IS NULL FOR UPDATE;
 IF high IS DISTINCT FROM p_generation OR s.source_generation IS DISTINCT FROM b.source_generation
  OR f.source_generation IS DISTINCT FROM b.source_generation OR f.physical_name IS DISTINCT FROM b.physical_name
  OR f.mode IS DISTINCT FROM 'full' OR f.state NOT IN ('awaiting_publish','live') OR c.membership_revision<>1
 THEN RAISE EXCEPTION 'C5 hydration current baseline required' USING ERRCODE='OO010'; END IF;
 SELECT * INTO STRICT r FROM _oo.full_import_run WHERE run_id=b.run_id FOR SHARE;
 SELECT * INTO STRICT t FROM _oo.full_import_table WHERE run_id=b.run_id AND schema_ident=p_schema AND table_ident=p_table FOR SHARE;
 SELECT value INTO STRICT part FROM jsonb_array_elements(r.multipart_scope->'parts')
  WHERE value->>'chunk_id'=p_chunk;
 IF r.state NOT IN ('awaiting_publish','published') OR r.sealed_at IS NULL OR r.retired_at IS NOT NULL
  OR r.source_id IS DISTINCT FROM p_source OR r.source_generation IS DISTINCT FROM b.source_generation
  OR r.schema_digest IS DISTINCT FROM b.schema_digest OR r.type_digest IS DISTINCT FROM b.type_digest
  OR r.config_digest IS DISTINCT FROM b.config_digest OR t.physical_name IS DISTINCT FROM b.physical_name
  OR NOT t.exhausted OR c.arrow_sha256 IS DISTINCT FROM part->>'arrow_sha256'
  OR c.row_count IS DISTINCT FROM (part->>'rows')::bigint
  OR NOT EXISTS(SELECT FROM _oo.apply_receipts a JOIN _oo.full_import_quality q
   ON q.run_id=b.run_id AND q.physical_name=a.table_generation AND q.batch_id=a.txn_or_batch_id AND q.kind='batch'
   WHERE a.source=p_source AND a.table_generation=b.physical_name
    AND a.txn_or_batch_id=(r.multipart_scope->>'occurrence')||'.part.'||(part->>'index')
    AND a.payload_digest=c.arrow_sha256 AND q.payload_digest=c.arrow_sha256
    AND a.config_digest=b.config_digest AND a.type_digest=b.type_digest AND a.schema_digest=b.schema_digest
    AND a.fence_generation BETWEEN 1 AND r.target_generation AND a.fence_shard=r.shard_id
    AND q.accepted_rows=c.row_count AND q.rejected_rows=0 AND q.input_digest=r.input_digest)
 THEN RAISE EXCEPTION 'C5 hydration sealed part receipt differs' USING ERRCODE='OO030'; END IF;
 SELECT jsonb_agg(jsonb_build_object('name',m.logical_column,'target_oid',a.atttypid::bigint,
  'target_typmod',a.atttypmod,'nullable',NOT a.attnotnull) ORDER BY a.attnum) INTO columns
 FROM pg_attribute a JOIN _oo.physical_mappings m ON m.physical_name=a.attname
  AND m.source=p_source AND m.config_digest=b.config_digest AND m.run_generation=b.source_generation AND m.level='c'
  AND m.logical_namespace=p_schema AND m.logical_table=p_table
 WHERE a.attrelid=to_regclass(format('_oo.%I',b.physical_name)) AND a.attnum>0 AND NOT a.attisdropped;
 RETURN jsonb_build_object('baseline',to_jsonb(b),'multipart_scope',r.multipart_scope,'chunk_id',p_chunk,
  'row_count',c.row_count,'arrow_sha256',c.arrow_sha256,'baseline_membership_pending',c.baseline_membership_pending,
  'columns',columns);
END $$;
