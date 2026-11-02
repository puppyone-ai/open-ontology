-- Metadata-only bootstrap. Pending membership is not an empty member set.
CREATE TABLE _oo.c5_multipart_baseline (
 source text NOT NULL, schema_name text NOT NULL, table_name text NOT NULL,
 run_id uuid NOT NULL REFERENCES _oo.full_import_run(run_id),
 source_generation bigint NOT NULL CHECK(source_generation>0),
 physical_name text NOT NULL, config_digest text NOT NULL,
 schema_digest text NOT NULL, type_digest text NOT NULL,
 key_columns text[] NOT NULL, key_kinds text[] NOT NULL,
 selected_columns text[] NOT NULL, projection_sha256 text NOT NULL,
 PRIMARY KEY(source,schema_name,table_name),
 FOREIGN KEY(source,schema_name,table_name) REFERENCES _oo.c5_table,
 CHECK(cardinality(key_columns)>0 AND cardinality(key_columns)=cardinality(key_kinds))
);
REVOKE ALL ON _oo.c5_multipart_baseline FROM PUBLIC,oo_pipeline_runtime;
CREATE TABLE _oo.c5_multipart_hydration (
 source text NOT NULL, schema_name text NOT NULL, table_name text NOT NULL, chunk_id text NOT NULL,
 run_id uuid NOT NULL REFERENCES _oo.full_import_run(run_id),
 arrow_sha256 text NOT NULL, row_count bigint NOT NULL, members_sha256 text NOT NULL,
 seed_digest jsonb,
 PRIMARY KEY(source,schema_name,table_name,chunk_id),
 FOREIGN KEY(source,schema_name,table_name,chunk_id) REFERENCES _oo.c5_chunk
);
REVOKE ALL ON _oo.c5_multipart_hydration FROM PUBLIC,oo_pipeline_runtime;
ALTER TABLE _oo.c5_chunk ADD COLUMN baseline_membership_pending boolean NOT NULL DEFAULT false;
ALTER TABLE _oo.c5_chunk ADD COLUMN last_digest_scan_id text, ADD COLUMN digest_receipt jsonb;
ALTER TABLE _oo.c5_table ADD COLUMN multipart_parent jsonb;
ALTER TABLE _oo.c5_scan ADD COLUMN scan_layers jsonb, ADD COLUMN scan_observed_at_ms bigint;

CREATE FUNCTION _oo.c5_invalidate_multipart_digest() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
BEGIN
 IF EXISTS(SELECT FROM _oo.c5_multipart_baseline WHERE source=NEW.source
  AND schema_name=NEW.schema_name AND table_name=NEW.table_name)
  AND (NEW.membership_revision IS DISTINCT FROM OLD.membership_revision
   OR NEW.arrow_sha256 IS DISTINCT FROM OLD.arrow_sha256 OR NEW.row_count IS DISTINCT FROM OLD.row_count
   OR NEW.lo IS DISTINCT FROM OLD.lo OR NEW.hi IS DISTINCT FROM OLD.hi
   OR NEW.retired_at IS DISTINCT FROM OLD.retired_at OR NEW.baseline_membership_pending)
 THEN
  NEW.last_digest_scan_id:=NULL; NEW.digest_receipt:=NULL;
  NEW.previous_digest_hi:=NULL; NEW.previous_digest_lo:=NULL;
  NEW.current_digest_hi:=NULL; NEW.current_digest_lo:=NULL;
  UPDATE _oo.c5_table SET last_full_digest_scan_completed_at=NULL
   WHERE source=NEW.source AND schema_name=NEW.schema_name AND table_name=NEW.table_name;
  UPDATE _oo.table_quality SET last_full_digest_scan_completed_at=NULL
   WHERE schema_name=NEW.schema_name AND table_name=NEW.table_name;
 END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_invalidate_multipart_digest() FROM PUBLIC,oo_pipeline_runtime;
CREATE TRIGGER c5_invalidate_multipart_digest BEFORE UPDATE ON _oo.c5_chunk
 FOR EACH ROW EXECUTE FUNCTION _oo.c5_invalidate_multipart_digest();

CREATE FUNCTION _oo.c5_mysql_bound(p_bound jsonb,p_kinds text[]) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE STRICT SET search_path=pg_catalog,_oo AS $$
DECLARE v jsonb; raw jsonb; kind text; result jsonb:='[]'; encoded text; i integer:=0;
BEGIN
 IF p_bound='null'::jsonb THEN RETURN p_bound; END IF;
 IF jsonb_typeof(p_bound) IS DISTINCT FROM 'array' OR cardinality(p_kinds)<1
  OR jsonb_array_length(p_bound)<>cardinality(p_kinds)
 THEN RAISE EXCEPTION 'C5 multipart bound width' USING ERRCODE='22023'; END IF;
 FOR v IN SELECT value FROM jsonb_array_elements(p_bound) LOOP
  i:=i+1; kind:=p_kinds[i];
  IF jsonb_typeof(v) IS DISTINCT FROM 'object' THEN
   RAISE EXCEPTION 'C5 multipart typed bound' USING ERRCODE='22023';
  END IF;
  IF (SELECT count(*) FROM jsonb_object_keys(v))<>1 OR NOT v ? kind
  THEN RAISE EXCEPTION 'C5 multipart typed bound' USING ERRCODE='22023'; END IF;
  raw:=v->kind;
  IF kind='Int' THEN
   IF jsonb_typeof(raw) IS DISTINCT FROM 'number' OR raw::text !~ '^-?(0|[1-9][0-9]*)$'
   THEN RAISE EXCEPTION 'C5 multipart integer bound' USING ERRCODE='22023'; END IF;
   result:=result||jsonb_build_array(jsonb_build_object('kind','int','value',(raw::text)::bigint));
  ELSIF kind IN ('Text','Bytes') THEN
   IF jsonb_typeof(raw) IS DISTINCT FROM 'array' THEN
    RAISE EXCEPTION 'C5 multipart byte bound' USING ERRCODE='22023';
   END IF;
   IF EXISTS(SELECT FROM jsonb_array_elements(raw) b WHERE jsonb_typeof(b) IS DISTINCT FROM 'number'
     OR b::text !~ '^(0|[1-9][0-9]{0,2})$' OR (b::text)::numeric>255)
   THEN RAISE EXCEPTION 'C5 multipart byte bound' USING ERRCODE='22023'; END IF;
   SELECT coalesce(string_agg(lpad(to_hex((b.value::text)::integer),2,'0'),'' ORDER BY b.ordinality),'')
    INTO encoded FROM jsonb_array_elements(raw) WITH ORDINALITY b;
   IF kind='Text' THEN PERFORM convert_from(decode(encoded,'hex'),'UTF8'); END IF;
   result:=result||jsonb_build_array(jsonb_build_object('kind',lower(kind),'value',encoded));
  ELSE RAISE EXCEPTION 'C5 multipart unsupported key type' USING ERRCODE='22023';
  END IF;
 END LOOP;
 RETURN result;
END $$;

CREATE FUNCTION _oo.c5_multipart_topology(p_manifest jsonb,p_kinds text[]) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE STRICT SET search_path=pg_catalog,_oo AS $$
DECLARE c jsonb; part jsonb; lo jsonb; hi jsonb; previous jsonb:='null'; result jsonb:='[]';
 n integer; i integer:=0; j integer; comparison integer; a bigint; b bigint; ba bytea; bb bytea;
BEGIN
 IF jsonb_typeof(p_manifest->'chunks') IS DISTINCT FROM 'array'
  OR jsonb_typeof(p_manifest->'parts') IS DISTINCT FROM 'array'
 THEN RAISE EXCEPTION 'C5 multipart manifest arrays' USING ERRCODE='22023'; END IF;
 n:=jsonb_array_length(p_manifest->'chunks');
 IF n<1 OR jsonb_array_length(p_manifest->'parts')<>n
  OR p_manifest->'part_count' IS DISTINCT FROM to_jsonb(n)
  OR cardinality(p_kinds)<1 OR EXISTS(SELECT FROM unnest(p_kinds) k WHERE k IS NULL OR k NOT IN ('Int','Text','Bytes'))
 THEN RAISE EXCEPTION 'C5 multipart manifest count' USING ERRCODE='22023'; END IF;
 FOR c IN SELECT value FROM jsonb_array_elements(p_manifest->'chunks') LOOP
  part:=p_manifest->'parts'->i;
  IF jsonb_typeof(c) IS DISTINCT FROM 'object' OR jsonb_typeof(part) IS DISTINCT FROM 'object'
  THEN RAISE EXCEPTION 'C5 multipart closed part' USING ERRCODE='22023'; END IF;
  IF (SELECT count(*) FROM jsonb_object_keys(c))<>7
   OR NOT c ?& ARRAY['table','chunk_id','lo','hi','rows','bytes','arrow_sha256']
  THEN RAISE EXCEPTION 'C5 multipart closed chunk' USING ERRCODE='22023'; END IF;
  IF (SELECT count(*) FROM jsonb_object_keys(part))<>5
   OR NOT part ?& ARRAY['index','total','chunk_id','rows','arrow_sha256']
   OR c->'chunk_id' IS DISTINCT FROM to_jsonb(i) OR part->'index' IS DISTINCT FROM to_jsonb(i)
   OR part->'chunk_id' IS DISTINCT FROM to_jsonb(i) OR part->'total' IS DISTINCT FROM to_jsonb(n)
   OR c->>'table' IS DISTINCT FROM p_manifest->>'table'
   OR c->'rows' IS DISTINCT FROM part->'rows' OR c->'arrow_sha256' IS DISTINCT FROM part->'arrow_sha256'
   OR coalesce(c->>'rows','') !~ '^[1-9][0-9]*$' OR (c->>'rows')::numeric>100000
   OR jsonb_typeof(c->'rows') IS DISTINCT FROM 'number'
   OR coalesce(c->>'bytes','') !~ '^[1-9][0-9]*$' OR jsonb_typeof(c->'bytes') IS DISTINCT FROM 'number'
   OR coalesce(c->>'arrow_sha256','') !~ '^[0-9a-f]{64}$'
  THEN RAISE EXCEPTION 'C5 multipart ordered parts' USING ERRCODE='22023'; END IF;
  lo:=_oo.c5_mysql_bound(c->'lo',p_kinds); hi:=_oo.c5_mysql_bound(c->'hi',p_kinds);
  IF lo IS DISTINCT FROM previous OR hi IS NULL OR hi='null'::jsonb OR hi=lo
   OR EXISTS(SELECT FROM jsonb_array_elements(result) prior WHERE prior->'hi'=hi)
  THEN RAISE EXCEPTION 'C5 multipart discontinuous bounds' USING ERRCODE='22023'; END IF;
  -- Numeric/binary order is portable. Text order remains the sealed source order,
  -- never a PostgreSQL collation claim; pending membership blocks its use.
  IF lo<>'null'::jsonb AND NOT 'Text'=ANY(p_kinds) THEN
   comparison:=0;
   FOR j IN 0..cardinality(p_kinds)-1 LOOP
    IF p_kinds[j+1]='Int' THEN
     a:=(lo->j->>'value')::bigint; b:=(hi->j->>'value')::bigint;
     comparison:=CASE WHEN a<b THEN -1 WHEN a>b THEN 1 ELSE 0 END;
    ELSE
     ba:=decode(lo->j->>'value','hex'); bb:=decode(hi->j->>'value','hex');
     comparison:=CASE WHEN ba<bb THEN -1 WHEN ba>bb THEN 1 ELSE 0 END;
    END IF;
    EXIT WHEN comparison<>0;
   END LOOP;
   IF comparison<>-1 THEN RAISE EXCEPTION 'C5 multipart nonincreasing bounds' USING ERRCODE='22023'; END IF;
  END IF;
  result:=result||jsonb_build_array(jsonb_build_object('chunk_id',i::text,'lo',lo,
   'hi',CASE WHEN i=n-1 THEN 'null'::jsonb ELSE hi END,
   'row_count',(c->>'rows')::bigint,'arrow_sha256',c->>'arrow_sha256'));
  previous:=hi; i:=i+1;
 END LOOP;
 RETURN result;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_mysql_bound(jsonb,text[]),_oo.c5_multipart_topology(jsonb,text[]) FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_initialize_multipart_if_absent(
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
     AND a.fence_shard=r.shard_id AND a.fence_generation=r.target_generation
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
REVOKE ALL ON FUNCTION _oo.c5_initialize_multipart_if_absent(text,uuid,bigint,text,text,text,text,bigint,text[],bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_initialize_multipart_if_absent(text,uuid,bigint,text,text,text,text,bigint,text[],bigint) TO oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_multipart_hydration_manifest(p_shard text,p_owner uuid,p_fence bigint,
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
    AND a.fence_generation=r.target_generation AND a.fence_shard=r.shard_id
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
REVOKE ALL ON FUNCTION _oo.c5_multipart_hydration_manifest(text,uuid,bigint,text,text,text,text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_multipart_hydration_manifest(text,uuid,bigint,text,text,text,text,bigint) TO oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_hydrate_multipart(p_shard text,p_owner uuid,p_fence bigint,
 p_source text,p_schema text,p_table text,p_chunk text,p_generation bigint,p_arrow_sha256 text,p_members jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE authority jsonb; b _oo.c5_multipart_baseline; saved _oo.c5_multipart_hydration;
 member jsonb; component jsonb; loader text; piece text; i integer; key_sha text;
BEGIN
 authority:=_oo.c5_multipart_hydration_manifest(p_shard,p_owner,p_fence,p_source,p_schema,p_table,p_chunk,p_generation);
 SELECT * INTO STRICT b FROM _oo.c5_multipart_baseline WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
 IF p_arrow_sha256 IS DISTINCT FROM authority->>'arrow_sha256'
  OR jsonb_typeof(p_members) IS DISTINCT FROM 'array'
 THEN RAISE EXCEPTION 'C5 hydration part identity differs' USING ERRCODE='OO030'; END IF;
 IF jsonb_array_length(p_members) IS DISTINCT FROM (authority->>'row_count')::bigint
  OR jsonb_array_length(p_members) NOT BETWEEN 1 AND 100000 OR octet_length(p_members::text)>67108864
  OR (SELECT count(DISTINCT _oo.c5_key_identity(value)) FROM jsonb_array_elements(p_members))<>jsonb_array_length(p_members)
 THEN RAISE EXCEPTION 'C5 hydration complete unique membership required' USING ERRCODE='OO030'; END IF;
 key_sha:=encode(sha256(convert_to(p_members::text,'UTF8')),'hex');
 SELECT * INTO saved FROM _oo.c5_multipart_hydration
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=p_chunk;
 IF FOUND THEN
  IF saved.run_id IS DISTINCT FROM b.run_id OR saved.arrow_sha256 IS DISTINCT FROM p_arrow_sha256
   OR saved.members_sha256 IS DISTINCT FROM key_sha OR saved.row_count IS DISTINCT FROM jsonb_array_length(p_members)
   OR authority->'baseline_membership_pending' IS DISTINCT FROM 'false'::jsonb
   OR (SELECT count(*) FROM _oo.c5_member WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=p_chunk)<>saved.row_count
   OR EXISTS(SELECT FROM jsonb_array_elements(p_members) k WHERE NOT EXISTS(
    SELECT FROM _oo.c5_member m WHERE m.source=p_source AND m.schema_name=p_schema AND m.table_name=p_table
     AND m.chunk_id=p_chunk AND m.key_identity=_oo.c5_key_identity(k.value) AND m.typed_key=k.value))
  THEN RAISE EXCEPTION 'C5 hydration replay differs' USING ERRCODE='OO010'; END IF;
  RETURN jsonb_build_object('hydrated',true,'replay',true,'chunk_id',p_chunk,'rows',saved.row_count,
   'data_gen',false,'apply',false,'ack',false,'receipt',NULL);
 END IF;
 IF authority->'baseline_membership_pending' IS DISTINCT FROM 'true'::jsonb
  OR EXISTS(SELECT FROM _oo.c5_member WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=p_chunk)
  OR EXISTS(SELECT FROM jsonb_array_elements(p_members) k JOIN _oo.c5_member m ON m.key_identity=_oo.c5_key_identity(k.value)
   WHERE m.source=p_source AND m.schema_name=p_schema AND m.table_name=p_table)
 THEN RAISE EXCEPTION 'C5 hydration existing membership differs' USING ERRCODE='OO010'; END IF;
 FOR member IN SELECT value FROM jsonb_array_elements(p_members) LOOP
  IF jsonb_array_length(member)<>cardinality(b.key_kinds) THEN
   RAISE EXCEPTION 'C5 hydration key width' USING ERRCODE='22023';
  END IF;
  loader:=''; i:=0;
  FOR component IN SELECT value FROM jsonb_array_elements(member) LOOP
   i:=i+1;
   IF component->>'kind' IS DISTINCT FROM lower(b.key_kinds[i]) THEN
    RAISE EXCEPTION 'C5 hydration key type' USING ERRCODE='22023';
   END IF;
   piece:=CASE b.key_kinds[i] WHEN 'Int' THEN ((component->>'value')::bigint)::text
    WHEN 'Text' THEN convert_from(decode(component->>'value','hex'),'UTF8')
    WHEN 'Bytes' THEN E'\\x'||(component->>'value') END;
   IF piece IS NULL OR (cardinality(b.key_kinds)>1 AND strpos(piece,chr(31))>0) THEN
    RAISE EXCEPTION 'C5 hydration loader key encoding' USING ERRCODE='22023';
   END IF;
   IF i>1 THEN loader:=loader||chr(31); END IF;
   loader:=loader||CASE WHEN cardinality(b.key_kinds)=1 THEN piece ELSE octet_length(piece)::text||':'||piece END;
  END LOOP;
  INSERT INTO _oo.c5_member VALUES(p_source,p_schema,p_table,p_chunk,_oo.c5_key_identity(member),member,loader);
 END LOOP;
 INSERT INTO _oo.c5_multipart_hydration(source,schema_name,table_name,chunk_id,run_id,arrow_sha256,row_count,members_sha256)
 VALUES(p_source,p_schema,p_table,p_chunk,b.run_id,p_arrow_sha256,jsonb_array_length(p_members),key_sha);
 UPDATE _oo.c5_chunk SET baseline_membership_pending=false
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=p_chunk;
 RETURN jsonb_build_object('hydrated',true,'replay',false,'chunk_id',p_chunk,'rows',jsonb_array_length(p_members),
  'data_gen',false,'apply',false,'ack',false,'receipt',NULL);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_hydrate_multipart(text,uuid,bigint,text,text,text,text,bigint,text,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_hydrate_multipart(text,uuid,bigint,text,text,text,text,bigint,text,jsonb) TO oo_pipeline_runtime;

-- Only the native loader calls this after independently hashing verified Arrow
-- cells using the qualified source-compatible codec; no JSON digest is accepted.
CREATE FUNCTION _oo.c5_seed_multipart_digest(p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,
 p_chunk text,p_generation bigint,p_arrow text,p_projection text,p_digest jsonb) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE authority jsonb; h _oo.c5_multipart_hydration; c _oo.c5_chunk; receipt jsonb;
BEGIN
 authority:=_oo.c5_multipart_hydration_manifest(p_shard,p_owner,p_fence,p_source,p_schema,p_table,p_chunk,p_generation);
 SELECT * INTO STRICT h FROM _oo.c5_multipart_hydration WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=p_chunk;
 SELECT * INTO STRICT c FROM _oo.c5_chunk WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=p_chunk FOR UPDATE;
 IF c.baseline_membership_pending OR c.membership_revision<>1 OR p_arrow IS DISTINCT FROM h.arrow_sha256
  OR p_arrow IS DISTINCT FROM authority->>'arrow_sha256'
  OR p_projection IS DISTINCT FROM authority->'baseline'->>'projection_sha256'
  OR jsonb_typeof(p_digest) IS DISTINCT FROM 'array'
 THEN RAISE EXCEPTION 'C5 archive digest scope differs' USING ERRCODE='OO030'; END IF;
 IF jsonb_array_length(p_digest)<>3 OR p_digest->0 IS DISTINCT FROM to_jsonb(h.row_count)
  OR coalesce(p_digest->>1,'') !~ '^[0-9a-f]{16}$' OR coalesce(p_digest->>2,'') !~ '^[0-9a-f]{16}$'
  OR EXISTS(SELECT FROM jsonb_array_elements(authority->'columns') col
   WHERE (col->>'target_oid')::bigint NOT IN (16,17,20,21,23,25,1042,1043,1700,1082,1114,1184)
    OR ((col->>'target_oid')::bigint=1700 AND ((col->>'target_typmod')::integer<4
     OR (((col->>'target_typmod')::integer-4)>>16) NOT BETWEEN 1 AND 38
     OR (((col->>'target_typmod')::integer-4)&65535) NOT BETWEEN 0 AND 30)))
 THEN RAISE EXCEPTION 'C5 archive digest type differs' USING ERRCODE='OO030'; END IF;
 receipt:=jsonb_build_object('chunk_id',c.chunk_id,'lo',c.lo,'hi',c.hi,'membership_revision',c.membership_revision,
  'arrow_sha256',c.arrow_sha256,'row_count',c.row_count,'digest',p_digest,'digest_codec','oo.c5.arrow-sha256-xor/v1',
  'projection_sha256',p_projection);
 IF (h.seed_digest IS NOT NULL AND h.seed_digest IS DISTINCT FROM p_digest)
  OR (c.digest_receipt IS NOT NULL AND c.digest_receipt IS DISTINCT FROM receipt)
 THEN RAISE EXCEPTION 'C5 archive digest replay differs' USING ERRCODE='OO010'; END IF;
 UPDATE _oo.c5_multipart_hydration SET seed_digest=p_digest
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=p_chunk;
 UPDATE _oo.c5_chunk SET digest_receipt=receipt,
  previous_digest_hi=('x'||(p_digest->>1))::bit(64),previous_digest_lo=('x'||(p_digest->>2))::bit(64),
  current_digest_hi=('x'||(p_digest->>1))::bit(64),current_digest_lo=('x'||(p_digest->>2))::bit(64)
 WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=p_chunk;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_seed_multipart_digest(text,uuid,bigint,text,text,text,text,bigint,text,text,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_seed_multipart_digest(text,uuid,bigint,text,text,text,text,bigint,text,text,jsonb) TO oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_digest_receipt_current(c _oo.c5_chunk) RETURNS boolean
LANGUAGE sql IMMUTABLE SET search_path=pg_catalog,_oo AS $$
 SELECT coalesce(NOT c.baseline_membership_pending
  AND c.digest_receipt->'chunk_id'=to_jsonb(c.chunk_id)
  AND c.digest_receipt->'membership_revision'=to_jsonb(c.membership_revision)
  AND c.digest_receipt->'arrow_sha256'=to_jsonb(c.arrow_sha256)
  AND c.digest_receipt->'row_count'=to_jsonb(c.row_count)
  AND c.digest_receipt->'digest'->0=to_jsonb(c.row_count)
  AND c.digest_receipt->'lo'=c.lo AND c.digest_receipt->'hi'=c.hi,false)
$$;
CREATE FUNCTION _oo.c5_digest_scan_prefix(p_source text,p_schema text,p_table text,p_scan text,p_expected jsonb)
RETURNS bigint LANGUAGE sql STABLE SET search_path=pg_catalog,_oo AS $$
 SELECT coalesce(min(k.ordinality)-1,jsonb_array_length(p_expected))
 FROM jsonb_array_elements_text(p_expected) WITH ORDINALITY k
 LEFT JOIN _oo.c5_chunk c ON c.source=p_source AND c.schema_name=p_schema AND c.table_name=p_table
  AND c.chunk_id=k.value AND c.retired_at IS NULL
 WHERE c.last_digest_scan_id IS DISTINCT FROM p_scan OR NOT _oo.c5_digest_receipt_current(c)
$$;
REVOKE ALL ON FUNCTION _oo.c5_digest_receipt_current(_oo.c5_chunk),
 _oo.c5_digest_scan_prefix(text,text,text,text,jsonb) FROM PUBLIC,oo_pipeline_runtime;

ALTER FUNCTION _oo.c5_scan_begin(text,uuid,bigint,text,text,text,text,jsonb) RENAME TO c5_scan_begin_before_multipart;
REVOKE ALL ON FUNCTION _oo.c5_scan_begin_before_multipart(text,uuid,bigint,text,text,text,text,jsonb) FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_scan_begin(p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,p_scan text,p_expected jsonb)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE expected jsonb; scan _oo.c5_scan; prefix bigint;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 IF NOT EXISTS(SELECT FROM _oo.c5_multipart_baseline WHERE source=p_source AND schema_name=p_schema AND table_name=p_table)
 THEN RETURN _oo.c5_scan_begin_before_multipart(p_shard,p_owner,p_fence,p_source,p_schema,p_table,p_scan,p_expected); END IF;
 PERFORM 1 FROM _oo.source_watermark WHERE source=p_source FOR UPDATE;
 PERFORM 1 FROM _oo.c5_table WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 SELECT jsonb_agg(chunk_id ORDER BY chunk_id::numeric) INTO expected FROM _oo.c5_chunk
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND retired_at IS NULL;
 IF p_scan IS NULL OR p_scan !~ '^[A-Za-z0-9_.:-]{1,128}$' OR p_expected IS DISTINCT FROM expected
 THEN RAISE EXCEPTION 'C5 multipart complete ordered scan required' USING ERRCODE='22023'; END IF;
 SELECT * INTO scan FROM _oo.c5_scan WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND scan_id=p_scan FOR UPDATE;
 IF FOUND AND scan.status='completed' THEN
  prefix:=_oo.c5_digest_scan_prefix(p_source,p_schema,p_table,p_scan,p_expected);
  IF scan.expected_chunks IS DISTINCT FROM p_expected OR prefix<>jsonb_array_length(p_expected)
  THEN RAISE EXCEPTION 'C5 completed scan changed; new scan ID required' USING ERRCODE='OO010'; END IF;
  RETURN prefix;
 END IF;
 PERFORM _oo.c5_scan_begin_before_multipart(p_shard,p_owner,p_fence,p_source,p_schema,p_table,p_scan,p_expected);
 prefix:=_oo.c5_digest_scan_prefix(p_source,p_schema,p_table,p_scan,p_expected);
 UPDATE _oo.c5_scan SET next_block=prefix
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND scan_id=p_scan;
 RETURN prefix;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_scan_begin(text,uuid,bigint,text,text,text,text,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_scan_begin(text,uuid,bigint,text,text,text,text,jsonb) TO oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_scan_context(p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,
 p_scan text,p_generation bigint,p_layers jsonb,p_observed bigint) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE scan _oo.c5_scan; high bigint;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 SELECT source_generation INTO STRICT high FROM _oo.source_watermark WHERE source=p_source FOR UPDATE;
 SELECT * INTO STRICT scan FROM _oo.c5_scan WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND scan_id=p_scan FOR UPDATE;
 IF high IS DISTINCT FROM p_generation OR jsonb_typeof(p_layers) IS DISTINCT FROM 'array'
  OR p_observed IS NULL OR p_observed<0
 THEN RAISE EXCEPTION 'C5 scan context scope' USING ERRCODE='22023'; END IF;
 IF jsonb_array_length(p_layers)<>4 OR EXISTS(SELECT FROM jsonb_array_elements(p_layers) v WHERE jsonb_typeof(v)<>'boolean')
  OR to_timestamp(p_observed::double precision/1000)>clock_timestamp()+interval '5 minutes'
  OR to_timestamp(p_observed::double precision/1000)<scan.started_at-interval '5 minutes'
  OR (scan.scan_layers IS NOT NULL AND (scan.scan_layers IS DISTINCT FROM p_layers OR scan.scan_observed_at_ms IS DISTINCT FROM p_observed))
 THEN RAISE EXCEPTION 'C5 scan context differs' USING ERRCODE='OO010'; END IF;
 UPDATE _oo.c5_scan SET scan_layers=p_layers,scan_observed_at_ms=p_observed
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND scan_id=p_scan;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_scan_context(text,uuid,bigint,text,text,text,text,bigint,jsonb,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_scan_context(text,uuid,bigint,text,text,text,text,bigint,jsonb,bigint) TO oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_observation_receipt_guard(p_shard text,p_owner uuid,p_fence bigint,p_scope jsonb,p_receipt jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 IF p_receipt->>'protocol' IS DISTINCT FROM 'oo.pipeline.native-update-result/v1'
  OR NOT EXISTS(SELECT FROM _oo.c5_block_receipt b JOIN _oo.full_import_run r USING(run_id)
   JOIN _oo.full_import_table t ON t.run_id=r.run_id AND t.schema_ident=b.schema_name AND t.table_ident=b.table_name
   JOIN _oo.apply_receipts a ON a.source=b.source AND a.table_generation=t.physical_name
    AND a.txn_or_batch_id=b.batch_id AND a.payload_digest=b.payload_digest
   WHERE b.scope=p_scope AND b.source=p_receipt->>'source_id'
    AND b.schema_name=p_receipt->>'logical_schema' AND b.table_name=p_receipt->>'logical_table'
    AND b.run_id::text=p_receipt->>'run_id' AND to_jsonb(b.source_generation)=p_receipt->'source_generation'
    AND to_jsonb(b.target_generation)=p_receipt->'target_generation' AND b.payload_digest=p_receipt->>'payload_digest'
    AND t.physical_name=p_receipt->>'physical_name' AND r.source_generation=b.source_generation
    AND r.target_generation=b.target_generation AND r.input_digest=p_scope->>'arrow_sha256'
    AND r.state IN ('awaiting_publish','published') AND r.sealed_at IS NOT NULL AND t.exhausted
    AND a.config_digest=r.config_digest AND a.type_digest=r.type_digest AND a.schema_digest=r.schema_digest)
 THEN RAISE EXCEPTION 'C5 observation Loader receipt differs' USING ERRCODE='OO030'; END IF;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_observation_receipt_guard(text,uuid,bigint,jsonb,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_observation_receipt_guard(text,uuid,bigint,jsonb,jsonb) TO oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_observe_partial(p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,
 p_generation bigint,p_observation jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE state _oo.c5_table; baseline _oo.c5_multipart_baseline; f _oo.table_freshness;
 scan _oo.c5_scan; ch _oo.c5_chunk; obs jsonb:=p_observation->'chunk';
 high bigint; prefix bigint; position bigint; at_time timestamptz; expected jsonb; complete boolean;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 SELECT source_generation INTO STRICT high FROM _oo.source_watermark WHERE source=p_source FOR UPDATE;
 SELECT * INTO STRICT f FROM _oo.table_freshness WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 SELECT * INTO STRICT state FROM _oo.c5_table WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 SELECT * INTO STRICT baseline FROM _oo.c5_multipart_baseline WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
 IF high IS DISTINCT FROM p_generation OR state.source_generation IS DISTINCT FROM f.source_generation
  OR state.source_generation>high OR state.source_generation<baseline.source_generation
  OR f.state NOT IN ('awaiting_publish','live')
  OR p_observation->>'schema_fingerprint' IS DISTINCT FROM state.schema_fingerprint
  OR p_observation->'chunk_topology_revision' IS DISTINCT FROM to_jsonb(state.topology_revision)
 THEN RAISE EXCEPTION 'C5 partial observation scope differs' USING ERRCODE='OO010'; END IF;
 PERFORM _oo.c5_authority_export(p_source,p_schema,p_table);
 IF jsonb_typeof(obs) IS DISTINCT FROM 'object' OR jsonb_typeof(obs->'digest') IS DISTINCT FROM 'array'
  OR jsonb_typeof(p_observation->'checked_at_ms') IS DISTINCT FROM 'number'
  OR coalesce(p_observation->>'checked_at_ms','') !~ '^[0-9]+$'
 THEN RAISE EXCEPTION 'C5 partial observation shape' USING ERRCODE='22023'; END IF;
 IF jsonb_array_length(obs->'digest')<>3 OR jsonb_typeof(obs->'digest'->0) IS DISTINCT FROM 'number'
  OR coalesce(obs->'digest'->>0,'') !~ '^[0-9]+$'
  OR coalesce(obs->'digest'->>1,'') !~ '^[0-9a-f]{16}$'
  OR coalesce(obs->'digest'->>2,'') !~ '^[0-9a-f]{16}$'
  OR coalesce(obs->>'digest_codec','') NOT IN ('mysql-md5-xor/v1','oo.c5.arrow-sha256-xor/v1')
  OR obs->>'projection_sha256' IS DISTINCT FROM baseline.projection_sha256
 THEN RAISE EXCEPTION 'C5 partial observation digest' USING ERRCODE='22023'; END IF;
 IF obs->>'digest_codec'='oo.c5.arrow-sha256-xor/v1' AND NOT EXISTS(
  SELECT FROM _oo.c5_multipart_hydration h WHERE h.source=p_source AND h.schema_name=p_schema AND h.table_name=p_table
   AND h.chunk_id=obs->>'chunk_id' AND h.run_id=baseline.run_id AND h.seed_digest IS NOT NULL)
 THEN RAISE EXCEPTION 'C5 qualified archive digest proof required' USING ERRCODE='OO030'; END IF;
 IF p_observation->'scan_id'='null'::jsonb AND p_observation->'expected_chunks'='null'::jsonb THEN
  IF p_observation->'full_digest_scan_completed'='true'::jsonb OR EXISTS(
   SELECT FROM _oo.c5_scan WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND status='running')
  THEN RAISE EXCEPTION 'C5 observation scan context required' USING ERRCODE='OO010'; END IF;
 ELSE
  SELECT * INTO STRICT scan FROM _oo.c5_scan WHERE source=p_source AND schema_name=p_schema
   AND table_name=p_table AND scan_id=p_observation->>'scan_id' FOR UPDATE;
 END IF;
 at_time:=to_timestamp((p_observation->>'checked_at_ms')::double precision/1000);
 IF at_time>clock_timestamp()+interval '5 minutes' OR at_time<scan.started_at-interval '5 minutes'
 THEN RAISE EXCEPTION 'C5 partial observation time' USING ERRCODE='22023'; END IF;
 SELECT jsonb_agg(chunk_id ORDER BY chunk_id::numeric) INTO expected FROM _oo.c5_chunk
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND retired_at IS NULL;
 IF scan.scan_id IS NOT NULL AND scan.expected_chunks IS DISTINCT FROM expected THEN
  RAISE EXCEPTION 'C5 partial observation scan topology differs' USING ERRCODE='OO010';
 END IF;
 IF scan.scan_id IS NOT NULL AND p_observation ? 'expected_chunks'
  AND p_observation->'expected_chunks' IS DISTINCT FROM scan.expected_chunks THEN
  RAISE EXCEPTION 'C5 partial observation expected chunks differ' USING ERRCODE='OO010';
 END IF;
 SELECT * INTO STRICT ch FROM _oo.c5_chunk WHERE source=p_source AND schema_name=p_schema
  AND table_name=p_table AND chunk_id=obs->>'chunk_id' AND retired_at IS NULL FOR UPDATE;
 IF ch.baseline_membership_pending OR obs->'lo' IS DISTINCT FROM ch.lo OR obs->'hi' IS DISTINCT FROM ch.hi
  OR obs->'membership_revision' IS DISTINCT FROM to_jsonb(ch.membership_revision)
  OR obs->'arrow_sha256' IS DISTINCT FROM to_jsonb(ch.arrow_sha256)
  OR obs->'row_count' IS DISTINCT FROM to_jsonb(ch.row_count)
  OR obs->'digest'->0 IS DISTINCT FROM to_jsonb(ch.row_count)
 THEN RAISE EXCEPTION 'C5 partial observation differs from confirmed membership' USING ERRCODE='OO030'; END IF;
 IF _oo.c5_digest_receipt_current(ch) THEN
  IF ch.digest_receipt->'digest' IS DISTINCT FROM obs->'digest'
   OR ch.digest_receipt->'digest_codec' IS DISTINCT FROM obs->'digest_codec'
   OR ch.digest_receipt->'projection_sha256' IS DISTINCT FROM obs->'projection_sha256'
  THEN RAISE EXCEPTION 'C5 nochange observation digest differs' USING ERRCODE='OO030'; END IF;
 ELSE
  IF NOT EXISTS(SELECT FROM _oo.c5_multipart_hydration h
    WHERE h.source=p_source AND h.schema_name=p_schema AND h.table_name=p_table AND h.chunk_id=ch.chunk_id
     AND ch.membership_revision=1 AND h.arrow_sha256=ch.arrow_sha256 AND h.row_count=ch.row_count
     AND h.run_id=baseline.run_id)
   AND NOT EXISTS(SELECT FROM _oo.c5_block_receipt b JOIN _oo.full_import_run r ON r.run_id=b.run_id
    JOIN _oo.full_import_table ft ON ft.run_id=r.run_id AND ft.schema_ident=p_schema AND ft.table_ident=p_table
    JOIN _oo.apply_receipts a ON a.source=b.source AND a.table_generation=ft.physical_name
     AND a.txn_or_batch_id=b.batch_id AND a.payload_digest=b.payload_digest
    WHERE b.source=p_source AND b.schema_name=p_schema AND b.table_name=p_table AND b.chunk_id=ch.chunk_id
     AND b.scope=ch.last_scope AND b.scope->>'arrow_sha256'=ch.arrow_sha256
     AND b.scope->'row_count'=to_jsonb(ch.row_count)
     AND b.scope->'prior_membership_revision'=to_jsonb(ch.membership_revision-1)
     AND b.source_generation=r.source_generation AND b.source_generation<=state.source_generation
     AND r.state IN ('awaiting_publish','published') AND r.sealed_at IS NOT NULL
     AND r.input_digest=ch.arrow_sha256 AND r.target_generation=b.target_generation
     AND a.fence_generation=b.target_generation AND a.config_digest=r.config_digest
     AND a.type_digest=r.type_digest AND a.schema_digest=r.schema_digest)
  THEN RAISE EXCEPTION 'C5 partial observation block receipt required' USING ERRCODE='OO030'; END IF;
 END IF;
 IF scan.scan_id IS NOT NULL THEN
  prefix:=_oo.c5_digest_scan_prefix(p_source,p_schema,p_table,scan.scan_id,scan.expected_chunks);
  SELECT ordinality-1 INTO STRICT position FROM jsonb_array_elements_text(scan.expected_chunks) WITH ORDINALITY k
   WHERE k.value=ch.chunk_id;
  IF position>prefix OR (scan.status='completed' AND prefix<>jsonb_array_length(scan.expected_chunks)) THEN
   RAISE EXCEPTION 'C5 partial observation skips unconfirmed prefix' USING ERRCODE='OO010';
  END IF;
 END IF;
 UPDATE _oo.c5_chunk SET digest_receipt=jsonb_build_object('chunk_id',ch.chunk_id,
   'lo',ch.lo,'hi',ch.hi,'membership_revision',ch.membership_revision,'arrow_sha256',ch.arrow_sha256,
   'row_count',ch.row_count,'digest',obs->'digest','digest_codec',obs->>'digest_codec',
   'projection_sha256',obs->>'projection_sha256'),
  previous_digest_hi=('x'||(obs->'digest'->>1))::bit(64),previous_digest_lo=('x'||(obs->'digest'->>2))::bit(64),
  current_digest_hi=('x'||(obs->'digest'->>1))::bit(64),current_digest_lo=('x'||(obs->'digest'->>2))::bit(64),
  scanned_at=greatest(ch.scanned_at,at_time),last_digest_scan_id=coalesce(scan.scan_id,ch.last_digest_scan_id)
 WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=ch.chunk_id;
 IF scan.scan_id IS NULL THEN
  RETURN jsonb_build_object('next_chunk_index',0,'full_digest_scan_completed',false,
   'data_gen',false,'apply',false,'ack',false,'receipt',NULL);
 END IF;
 prefix:=_oo.c5_digest_scan_prefix(p_source,p_schema,p_table,scan.scan_id,scan.expected_chunks);
 complete:=prefix=jsonb_array_length(scan.expected_chunks);
 UPDATE _oo.c5_scan SET next_block=prefix,status=CASE WHEN complete THEN 'completed' ELSE 'running' END,
  completed_at=CASE WHEN complete THEN coalesce(completed_at,clock_timestamp()) ELSE NULL END
 WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND scan_id=scan.scan_id;
 IF complete THEN
  UPDATE _oo.c5_table SET last_full_digest_scan_completed_at=(SELECT completed_at FROM _oo.c5_scan s
   WHERE s.source=p_source AND s.schema_name=p_schema AND s.table_name=p_table AND s.scan_id=scan.scan_id)
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
  UPDATE _oo.table_quality q SET c5_mode='layered',last_full_digest_scan_completed_at=c.last_full_digest_scan_completed_at
  FROM _oo.c5_table c WHERE c.source=p_source AND c.schema_name=p_schema AND c.table_name=p_table
   AND q.schema_name=p_schema AND q.table_name=p_table;
 END IF;
 RETURN jsonb_build_object('next_chunk_index',prefix,'full_digest_scan_completed',complete,
  'data_gen',false,'apply',false,'ack',false,'receipt',NULL);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_observe_partial(text,uuid,bigint,text,text,text,bigint,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_observe_partial(text,uuid,bigint,text,text,text,bigint,jsonb) TO oo_pipeline_runtime;

ALTER FUNCTION _oo.c5_range_guard(text,uuid,bigint,jsonb,text[]) RENAME TO c5_range_guard_before_multipart;
REVOKE ALL ON FUNCTION _oo.c5_range_guard_before_multipart(text,uuid,bigint,jsonb,text[]) FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_range_guard(p_shard text,p_owner uuid,p_fence bigint,p_scope jsonb,p_keys text[])
RETURNS text[] LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE s text:=p_scope->>'source_id'; ns text:=p_scope->>'schema'; tbl text:=p_scope->>'table';
 state _oo.c5_table; f _oo.table_freshness; ch _oo.c5_chunk; b _oo.c5_multipart_baseline;
 high bigint; missing text[];
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 SELECT * INTO b FROM _oo.c5_multipart_baseline WHERE source=s AND schema_name=ns AND table_name=tbl;
 IF NOT FOUND THEN RETURN _oo.c5_range_guard_before_multipart(p_shard,p_owner,p_fence,p_scope,p_keys); END IF;
 IF jsonb_typeof(p_scope) IS DISTINCT FROM 'object' OR jsonb_typeof(p_scope->'members') IS DISTINCT FROM 'array'
 THEN RAISE EXCEPTION 'C5 multipart range scope' USING ERRCODE='22023'; END IF;
 IF (SELECT count(*) FROM jsonb_object_keys(p_scope))<>20
  OR NOT p_scope ?& ARRAY['protocol','source_engine','capture_method','source_id','schema','table','schema_fingerprint',
   'chunk_topology_revision','chunk_id','lo','hi','prior_membership_revision','parent_generation','source_generation',
   'arrow_sha256','row_count','delete_detection','position_codec','applied_pos','members']
  OR p_scope->>'protocol' IS DISTINCT FROM 'oo.pipeline.native-c5-range/v1'
  OR p_scope->>'source_engine' IS DISTINCT FROM 'mysql' OR p_scope->>'capture_method' IS DISTINCT FROM 'c5_diff'
  OR p_scope->>'delete_detection' IS DISTINCT FROM 'reconcile' OR p_scope->>'position_codec' IS DISTINCT FROM 'snapshot-zero/v1'
  OR p_scope->>'applied_pos' IS DISTINCT FROM '0' OR coalesce(p_scope->>'arrow_sha256','') !~ '^[0-9a-f]{64}$'
  OR p_keys IS NULL OR cardinality(p_keys)>100000
  OR p_scope->'row_count' IS DISTINCT FROM to_jsonb(cardinality(p_keys))
  OR jsonb_array_length(p_scope->'members')<>cardinality(p_keys)
 THEN RAISE EXCEPTION 'C5 multipart range scope' USING ERRCODE='22023'; END IF;
 SELECT source_generation INTO STRICT high FROM _oo.source_watermark WHERE source=s FOR UPDATE;
 SELECT * INTO STRICT f FROM _oo.table_freshness WHERE source=s AND schema_name=ns AND table_name=tbl FOR UPDATE;
 SELECT * INTO STRICT state FROM _oo.c5_table WHERE source=s AND schema_name=ns AND table_name=tbl FOR UPDATE;
 IF p_scope->'parent_generation' IS DISTINCT FROM to_jsonb(high)
  OR p_scope->'source_generation' IS DISTINCT FROM to_jsonb(high+1)
  OR state.source_generation IS DISTINCT FROM f.source_generation OR state.source_generation>high
  OR state.source_generation<b.source_generation OR state.schema_fingerprint IS DISTINCT FROM p_scope->>'schema_fingerprint'
  OR p_scope->'chunk_topology_revision' IS DISTINCT FROM to_jsonb(state.topology_revision)
 THEN RAISE EXCEPTION 'C5 multipart range parent differs' USING ERRCODE='OO010'; END IF;
 PERFORM _oo.c5_authority_export(s,ns,tbl);
 IF EXISTS(SELECT FROM _oo.c5_multipart_baseline WHERE source=p_scope->>'source_id'
  AND schema_name=p_scope->>'schema' AND table_name=p_scope->>'table')
  AND (EXISTS(SELECT FROM _oo.c5_chunk WHERE source=p_scope->>'source_id' AND schema_name=p_scope->>'schema'
   AND table_name=p_scope->>'table' AND retired_at IS NULL AND baseline_membership_pending)
   OR NOT EXISTS(SELECT FROM _oo.c5_chunk WHERE source=p_scope->>'source_id' AND schema_name=p_scope->>'schema'
    AND table_name=p_scope->>'table' AND chunk_id=p_scope->>'chunk_id' AND retired_at IS NULL))
 THEN RAISE EXCEPTION 'C5 multipart baseline membership pending' USING ERRCODE='OO030'; END IF;
 SELECT * INTO STRICT ch FROM _oo.c5_chunk WHERE source=s AND schema_name=ns AND table_name=tbl
  AND chunk_id=p_scope->>'chunk_id' AND retired_at IS NULL FOR UPDATE;
 IF ch.topology_revision<>state.topology_revision OR ch.lo IS DISTINCT FROM p_scope->'lo'
  OR ch.hi IS DISTINCT FROM p_scope->'hi' OR p_scope->'prior_membership_revision' IS DISTINCT FROM to_jsonb(ch.membership_revision)
 THEN RAISE EXCEPTION 'C5 multipart range chunk differs' USING ERRCODE='OO010'; END IF;
 IF EXISTS(SELECT FROM unnest(p_keys) k WHERE k IS NULL)
  OR (SELECT count(DISTINCT k) FROM unnest(p_keys) k)<>cardinality(p_keys)
  OR (SELECT count(DISTINCT _oo.c5_key_identity(value)) FROM jsonb_array_elements(p_scope->'members'))<>cardinality(p_keys)
  OR EXISTS(SELECT FROM jsonb_array_elements(p_scope->'members') m WHERE jsonb_array_length(m)<>cardinality(b.key_kinds))
  OR EXISTS(SELECT FROM jsonb_array_elements(p_scope->'members') m,
   LATERAL jsonb_array_elements(m) WITH ORDINALITY k WHERE k.value->>'kind' IS DISTINCT FROM lower(b.key_kinds[k.ordinality]))
  OR EXISTS(SELECT FROM _oo.c5_member m JOIN jsonb_array_elements(p_scope->'members') k ON m.key_identity=_oo.c5_key_identity(k.value)
   WHERE m.source=s AND m.schema_name=ns AND m.table_name=tbl AND m.chunk_id<>ch.chunk_id)
 THEN RAISE EXCEPTION 'C5 multipart range membership differs' USING ERRCODE='22023'; END IF;
 SELECT coalesce(array_agg(m.loader_key),ARRAY[]::text[]) INTO missing FROM _oo.c5_member m
  WHERE m.source=s AND m.schema_name=ns AND m.table_name=tbl AND m.chunk_id=ch.chunk_id
   AND NOT EXISTS(SELECT FROM jsonb_array_elements(p_scope->'members') k WHERE m.key_identity=_oo.c5_key_identity(k.value));
 UPDATE _oo.c5_table SET multipart_parent=jsonb_build_object('xid',pg_current_xact_id()::text,
  'global_generation',high,'table_generation',state.source_generation,'physical_name',f.physical_name,
  'input_digest',state.input_digest,'scope_sha256',encode(sha256(convert_to(p_scope::text,'UTF8')),'hex'),
  'keys_sha256',encode(sha256(convert_to(to_jsonb(p_keys)::text,'UTF8')),'hex'))
 WHERE source=s AND schema_name=ns AND table_name=tbl;
 RETURN missing;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_range_guard(text,uuid,bigint,jsonb,text[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_range_guard(text,uuid,bigint,jsonb,text[]) TO oo_pipeline_runtime;

ALTER FUNCTION _oo.c5_range_guard_after_seal(jsonb) RENAME TO c5_range_guard_after_seal_before_multipart;
REVOKE ALL ON FUNCTION _oo.c5_range_guard_after_seal_before_multipart(jsonb) FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_range_guard_after_seal(p_scope jsonb) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE s text:=p_scope->>'source_id'; ns text:=p_scope->>'schema'; tbl text:=p_scope->>'table';
 state _oo.c5_table; f _oo.table_freshness; ch _oo.c5_chunk; parent jsonb; high bigint;
BEGIN
 IF NOT EXISTS(SELECT FROM _oo.c5_multipart_baseline WHERE source=s AND schema_name=ns AND table_name=tbl) THEN
  PERFORM _oo.c5_range_guard_after_seal_before_multipart(p_scope); RETURN;
 END IF;
 SELECT source_generation INTO STRICT high FROM _oo.source_watermark WHERE source=s FOR UPDATE;
 SELECT * INTO STRICT f FROM _oo.table_freshness WHERE source=s AND schema_name=ns AND table_name=tbl FOR UPDATE;
 SELECT * INTO STRICT state FROM _oo.c5_table WHERE source=s AND schema_name=ns AND table_name=tbl FOR UPDATE;
 SELECT * INTO STRICT ch FROM _oo.c5_chunk WHERE source=s AND schema_name=ns AND table_name=tbl
  AND chunk_id=p_scope->>'chunk_id' AND retired_at IS NULL FOR UPDATE;
 parent:=state.multipart_parent;
 IF parent->>'xid' IS DISTINCT FROM pg_current_xact_id()::text
  OR parent->'global_generation' IS DISTINCT FROM p_scope->'parent_generation'
  OR parent->'table_generation' IS DISTINCT FROM to_jsonb(state.source_generation)
  OR parent->>'input_digest' IS DISTINCT FROM state.input_digest
  OR parent->>'scope_sha256' IS DISTINCT FROM encode(sha256(convert_to(p_scope::text,'UTF8')),'hex')
  OR p_scope->'source_generation' IS DISTINCT FROM to_jsonb(high)
  OR p_scope->'source_generation' IS DISTINCT FROM to_jsonb(f.source_generation)
  OR state.source_generation>(p_scope->>'parent_generation')::bigint
  OR p_scope->>'schema_fingerprint' IS DISTINCT FROM state.schema_fingerprint
  OR p_scope->'chunk_topology_revision' IS DISTINCT FROM to_jsonb(state.topology_revision)
  OR ch.topology_revision<>state.topology_revision
  OR p_scope->'prior_membership_revision' IS DISTINCT FROM to_jsonb(ch.membership_revision)
  OR p_scope->'lo' IS DISTINCT FROM ch.lo OR p_scope->'hi' IS DISTINCT FROM ch.hi
  OR NOT EXISTS(SELECT FROM _oo.full_import_run r JOIN _oo.full_import_table ft USING(run_id)
   WHERE r.source_id=s AND r.source_generation=state.source_generation AND r.input_digest=state.input_digest
    AND r.state IN ('awaiting_publish','published') AND ft.physical_name=parent->>'physical_name'
    AND ft.schema_ident=ns AND ft.table_ident=tbl AND ft.exhausted)
  OR NOT EXISTS(SELECT FROM _oo.full_import_run r JOIN _oo.full_import_table ft USING(run_id)
   WHERE r.source_id=s AND r.source_generation=f.source_generation AND r.target_generation=f.target_generation
    AND r.input_digest=p_scope->>'arrow_sha256' AND r.state='awaiting_publish'
    AND ft.physical_name=f.physical_name AND ft.schema_ident=ns AND ft.table_ident=tbl AND ft.exhausted)
 THEN RAISE EXCEPTION 'C5 multipart sealed parent differs' USING ERRCODE='OO010'; END IF;
 IF EXISTS(SELECT FROM _oo.c5_multipart_baseline WHERE source=p_scope->>'source_id'
  AND schema_name=p_scope->>'schema' AND table_name=p_scope->>'table')
  AND (EXISTS(SELECT FROM _oo.c5_chunk WHERE source=p_scope->>'source_id' AND schema_name=p_scope->>'schema'
   AND table_name=p_scope->>'table' AND retired_at IS NULL AND baseline_membership_pending)
   OR NOT EXISTS(SELECT FROM _oo.c5_chunk WHERE source=p_scope->>'source_id' AND schema_name=p_scope->>'schema'
    AND table_name=p_scope->>'table' AND chunk_id=p_scope->>'chunk_id' AND retired_at IS NULL))
 THEN RAISE EXCEPTION 'C5 multipart baseline membership pending' USING ERRCODE='OO030'; END IF;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_range_guard_after_seal(jsonb) FROM PUBLIC,oo_pipeline_runtime;

ALTER FUNCTION _oo.c5_range_commit(text,uuid,bigint,jsonb,text[],uuid,text,text) RENAME TO c5_range_commit_before_multipart;
REVOKE ALL ON FUNCTION _oo.c5_range_commit_before_multipart(text,uuid,bigint,jsonb,text[],uuid,text,text) FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_range_commit(p_shard text,p_owner uuid,p_fence bigint,p_scope jsonb,p_keys text[],p_run uuid,p_batch text,p_payload text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE state _oo.c5_table;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 SELECT c.* INTO state FROM _oo.c5_table c JOIN _oo.c5_multipart_baseline b USING(source,schema_name,table_name)
  WHERE c.source=p_scope->>'source_id' AND c.schema_name=p_scope->>'schema' AND c.table_name=p_scope->>'table';
 IF FOUND AND NOT EXISTS(SELECT FROM _oo.c5_block_receipt r WHERE r.source=state.source
  AND r.schema_name=state.schema_name AND r.table_name=state.table_name AND r.chunk_id=p_scope->>'chunk_id'
  AND r.source_generation=(p_scope->>'source_generation')::bigint) THEN
  IF state.multipart_parent->>'xid' IS DISTINCT FROM pg_current_xact_id()::text
   OR state.multipart_parent->>'keys_sha256' IS DISTINCT FROM encode(sha256(convert_to(to_jsonb(p_keys)::text,'UTF8')),'hex')
  THEN RAISE EXCEPTION 'C5 multipart commit keys differ' USING ERRCODE='OO010'; END IF;
 END IF;
 PERFORM _oo.c5_range_commit_before_multipart(p_shard,p_owner,p_fence,p_scope,p_keys,p_run,p_batch,p_payload);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_range_commit(text,uuid,bigint,jsonb,text[],uuid,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_range_commit(text,uuid,bigint,jsonb,text[],uuid,text,text) TO oo_pipeline_runtime;

ALTER FUNCTION _oo.c5_observe(text,uuid,bigint,text,text,text,bigint,jsonb) RENAME TO c5_observe_before_multipart;
REVOKE ALL ON FUNCTION _oo.c5_observe_before_multipart(text,uuid,bigint,text,text,text,bigint,jsonb) FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_observe(p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,p_generation bigint,p_observation jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE chunk jsonb;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 IF EXISTS(SELECT FROM _oo.c5_multipart_baseline WHERE source=p_source AND schema_name=p_schema AND table_name=p_table) THEN
  IF EXISTS(SELECT FROM _oo.c5_chunk WHERE source=p_source AND schema_name=p_schema
   AND table_name=p_table AND retired_at IS NULL AND baseline_membership_pending)
  THEN RAISE EXCEPTION 'C5 multipart baseline membership pending' USING ERRCODE='OO030'; END IF;
  IF jsonb_typeof(p_observation->'chunks') IS DISTINCT FROM 'array'
   OR jsonb_array_length(p_observation->'chunks')=0
  THEN RAISE EXCEPTION 'C5 multipart confirmed observations required' USING ERRCODE='22023'; END IF;
  FOR chunk IN SELECT value FROM jsonb_array_elements(p_observation->'chunks') LOOP
   PERFORM _oo.c5_observe_partial(p_shard,p_owner,p_fence,p_source,p_schema,p_table,p_generation,
    p_observation||jsonb_build_object('chunk',chunk));
  END LOOP;
  RETURN;
 END IF;
 PERFORM _oo.c5_observe_before_multipart(p_shard,p_owner,p_fence,p_source,p_schema,p_table,p_generation,p_observation);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_observe(text,uuid,bigint,text,text,text,bigint,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_observe(text,uuid,bigint,text,text,text,bigint,jsonb) TO oo_pipeline_runtime;

ALTER FUNCTION _oo.c5_authority_export(text,text,text) RENAME TO c5_authority_export_before_multipart;
REVOKE ALL ON FUNCTION _oo.c5_authority_export_before_multipart(text,text,text) FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_authority_export(p_source text,p_schema text,p_table text) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE value jsonb; baseline _oo.c5_multipart_baseline; f _oo.table_freshness; high bigint; chunks jsonb;
BEGIN
 value:=_oo.c5_authority_export_before_multipart(p_source,p_schema,p_table);
 SELECT * INTO baseline FROM _oo.c5_multipart_baseline
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
 IF NOT FOUND THEN RETURN value; END IF;
 SELECT * INTO STRICT f FROM _oo.table_freshness
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
 SELECT source_generation INTO STRICT high FROM _oo.source_watermark WHERE source=p_source;
 IF value IS NULL OR (value->>'source_generation')::bigint IS DISTINCT FROM f.source_generation
  OR f.source_generation>high OR f.source_generation<baseline.source_generation
  OR f.state NOT IN ('awaiting_publish','live')
  OR (f.source_generation=baseline.source_generation AND f.physical_name IS DISTINCT FROM baseline.physical_name)
  OR NOT EXISTS(SELECT FROM _oo.full_import_run r JOIN _oo.full_import_table ft USING(run_id)
   WHERE r.source_id=p_source AND r.source_generation=f.source_generation AND r.target_generation=f.target_generation
    AND r.state IN ('awaiting_publish','published') AND r.sealed_at IS NOT NULL
    AND r.input_digest=value->>'input_digest' AND ft.schema_ident=p_schema AND ft.table_ident=p_table
    AND ft.physical_name=f.physical_name AND ft.exhausted)
 THEN RAISE EXCEPTION 'C5 multipart authority table differs' USING ERRCODE='OO010'; END IF;
 SELECT jsonb_agg(c.value||jsonb_build_object('baseline_membership_pending',ch.baseline_membership_pending,
  'row_count',ch.row_count,'digest',CASE WHEN _oo.c5_digest_receipt_current(ch) THEN ch.digest_receipt->'digest' ELSE 'null'::jsonb END,
  'digest_codec',CASE WHEN _oo.c5_digest_receipt_current(ch) THEN ch.digest_receipt->>'digest_codec' END,
  'projection_sha256',CASE WHEN _oo.c5_digest_receipt_current(ch) THEN ch.digest_receipt->>'projection_sha256' END,
  'last_digest_scan_id',ch.last_digest_scan_id) ORDER BY c.ordinality)
  INTO chunks FROM jsonb_array_elements(value->'chunks') WITH ORDINALITY c
  JOIN _oo.c5_chunk ch ON ch.source=p_source AND ch.schema_name=p_schema AND ch.table_name=p_table
   AND ch.chunk_id=c.value->>'chunk_id' AND ch.retired_at IS NULL;
 RETURN value||jsonb_build_object('source_generation',high,'table_source_generation',f.source_generation,
  'table_physical_name',f.physical_name,'baseline',to_jsonb(baseline),'selected_columns',to_jsonb(baseline.selected_columns),
  'next_chunk_index',coalesce((SELECT _oo.c5_digest_scan_prefix(p_source,p_schema,p_table,s.scan_id,s.expected_chunks)
   FROM _oo.c5_scan s WHERE s.source=p_source AND s.schema_name=p_schema AND s.table_name=p_table AND s.status='running'),0),
  'scan_id',(SELECT s.scan_id FROM _oo.c5_scan s WHERE s.source=p_source AND s.schema_name=p_schema AND s.table_name=p_table AND s.status='running'),
  'expected_chunks',(SELECT s.expected_chunks FROM _oo.c5_scan s WHERE s.source=p_source AND s.schema_name=p_schema AND s.table_name=p_table AND s.status='running'),
  'scan_layers',(SELECT s.scan_layers FROM _oo.c5_scan s WHERE s.source=p_source AND s.schema_name=p_schema AND s.table_name=p_table AND s.status='running'),
  'scan_observed_at_ms',(SELECT s.scan_observed_at_ms FROM _oo.c5_scan s WHERE s.source=p_source AND s.schema_name=p_schema AND s.table_name=p_table AND s.status='running'),
  'chunks',chunks);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_authority_export(text,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_authority_export(text,text,text) TO oo_pipeline_runtime;
