CREATE OR REPLACE FUNCTION _oo.c5_key_identity(p_key jsonb) RETURNS bytea
LANGUAGE plpgsql IMMUTABLE STRICT SET search_path=pg_catalog,_oo AS $$
DECLARE v jsonb; b bytea; result bytea := ''::bytea;
BEGIN
  IF jsonb_typeof(p_key)<>'array' OR jsonb_array_length(p_key)=0 THEN RAISE EXCEPTION 'C5 key'; END IF;
  FOR v IN SELECT value FROM jsonb_array_elements(p_key) LOOP
    IF jsonb_typeof(v) IS DISTINCT FROM 'object' OR (SELECT count(*) FROM jsonb_object_keys(v))<>2 OR NOT v ?& ARRAY['kind','value'] THEN RAISE EXCEPTION 'C5 closed key'; END IF;
    IF v->>'kind'='int' AND jsonb_typeof(v->'value')='number' AND v->>'value' ~ '^-?(0|[1-9][0-9]*)$' THEN b := '\x00'::bytea || int8send((v->>'value')::bigint);
    ELSIF v->>'kind' IN ('text','bytes') AND jsonb_typeof(v->'value')='string' AND v->>'value' ~ '^([0-9a-f]{2})*$' THEN
      b := CASE WHEN v->>'kind'='text' THEN '\x01'::bytea ELSE '\x02'::bytea END || decode(v->>'value','hex');
    ELSE RAISE EXCEPTION 'C5 key'; END IF;
    result := result || int4send(octet_length(b)) || b;
  END LOOP;
  RETURN result;
END $$;


CREATE OR REPLACE FUNCTION _oo.c5_initialize_full(p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,p_fingerprint text,p_topology bigint,p_chunk text,p_keys text[])
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE f _oo.table_freshness; r _oo.full_import_run; old _oo.c5_table; physical text; column_name text;
 typed text:='jsonb_build_array('; loader text:=''; piece text; kind oid; i integer; n bigint;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 IF p_fingerprint !~ '^[0-9a-f]{64}$' OR p_topology<1 OR p_chunk !~ '^[0-9]+$' OR cardinality(p_keys)<1
 THEN RAISE EXCEPTION 'C5 full initialization scope'; END IF;
 SELECT * INTO STRICT f FROM _oo.table_freshness WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 SELECT run.* INTO STRICT r FROM _oo.full_import_run run JOIN _oo.full_import_table t USING(run_id)
   WHERE run.source_id=p_source AND t.physical_name=f.physical_name AND t.exhausted
   AND run.state IN ('awaiting_publish','published') AND run.source_generation=f.source_generation;
 IF f.mode<>'full' OR NOT EXISTS(SELECT FROM _oo.apply_receipts a WHERE a.source=p_source AND a.table_generation=f.physical_name)
   OR r.source_generation IS DISTINCT FROM (SELECT source_generation FROM _oo.source_watermark WHERE source=p_source)
 THEN RAISE EXCEPTION 'C5 current sealed full read required' USING ERRCODE='OO030'; END IF;
 SELECT * INTO old FROM _oo.c5_table WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 IF FOUND THEN
   IF old.source_generation>=r.source_generation OR p_topology<>old.topology_revision+1
     OR EXISTS(SELECT FROM _oo.c5_chunk WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=p_chunk)
     OR EXISTS(SELECT FROM _oo.c5_scan WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND status='running')
   THEN RAISE EXCEPTION 'C5 topology needs new ID and newer full read' USING ERRCODE='OO010'; END IF;
 ELSE
   IF p_topology<>1 THEN RAISE EXCEPTION 'C5 initial topology'; END IF;
   INSERT INTO _oo.c5_table(source,schema_name,table_name,schema_fingerprint,topology_revision,source_generation)
     VALUES(p_source,p_schema,p_table,p_fingerprint,p_topology,r.source_generation);
 END IF;
 physical:=f.physical_name;
 FOR i IN 1..cardinality(p_keys) LOOP
   column_name:=_oo.lookup_physical_mapping(p_source,r.config_digest,r.source_generation,'column',p_schema,p_table,p_keys[i]);
   SELECT atttypid INTO STRICT kind FROM pg_attribute WHERE attrelid=to_regclass(format('_oo.%I',physical)) AND attname=column_name AND NOT attisdropped;
   IF i>1 THEN typed:=typed||','; loader:=loader||' || chr(31) || '; END IF;
   IF kind IN (20,21,23) THEN piece:=format('jsonb_build_object(''kind'',''int'',''value'',row.%I::bigint)',column_name);
   ELSIF kind IN (25,1042,1043) THEN piece:=format('jsonb_build_object(''kind'',''text'',''value'',encode(convert_to(row.%I::text,''UTF8''),''hex''))',column_name);
   ELSIF kind=17 THEN piece:=format('jsonb_build_object(''kind'',''bytes'',''value'',encode(row.%I,''hex''))',column_name);
   ELSE RAISE EXCEPTION 'C5 unsupported baseline key'; END IF;
   typed:=typed||piece;
   IF cardinality(p_keys)=1 THEN loader:=format('row.%I::text',column_name);
   ELSE loader:=loader||format('octet_length(row.%I::text)::text || '':'' || row.%I::text',column_name,column_name); END IF;
 END LOOP;
 typed:=typed||')';
 DELETE FROM _oo.c5_member WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
 UPDATE _oo.c5_chunk SET retired_at=clock_timestamp() WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND retired_at IS NULL;
 EXECUTE format('SELECT count(*) FROM _oo.%I',physical) INTO n;
 INSERT INTO _oo.c5_chunk(source,schema_name,table_name,chunk_id,topology_revision,lo,hi,membership_revision,row_count,arrow_sha256,scanned_at)
   VALUES(p_source,p_schema,p_table,p_chunk,p_topology,'null','null',1,n,r.input_digest,clock_timestamp());
 EXECUTE format('INSERT INTO _oo.c5_member(source,schema_name,table_name,chunk_id,key_identity,typed_key,loader_key) SELECT $1,$2,$3,$4,_oo.c5_key_identity(%s),%s,%s FROM _oo.%I row',typed,typed,loader,physical)
   USING p_source,p_schema,p_table,p_chunk;
 UPDATE _oo.c5_table SET schema_fingerprint=p_fingerprint,topology_revision=p_topology,source_generation=r.source_generation,input_digest=r.input_digest,
   last_full_digest_scan_completed_at=NULL WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
 UPDATE _oo.table_quality SET c5_mode='layered',last_full_digest_scan_completed_at=NULL WHERE schema_name=p_schema AND table_name=p_table;
END $$;

CREATE OR REPLACE FUNCTION _oo.c5_observe(p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,p_generation bigint,p_observation jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE state _oo.c5_table; obs jsonb; ch _oo.c5_chunk; scan _oo.c5_scan; at_time timestamptz;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
  SELECT * INTO STRICT state FROM _oo.c5_table WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
  IF state.source_generation IS DISTINCT FROM p_generation OR state.schema_fingerprint IS DISTINCT FROM p_observation->>'schema_fingerprint'
    OR COALESCE((SELECT source_generation FROM _oo.source_watermark WHERE source=p_source),0) IS DISTINCT FROM p_generation
    THEN RAISE EXCEPTION 'C5 stale observation' USING ERRCODE='OO010'; END IF;
  at_time:=to_timestamp((p_observation->>'checked_at_ms')::double precision/1000);
  IF at_time IS NULL OR at_time>clock_timestamp()+interval '5 minutes' THEN RAISE EXCEPTION 'C5 observation time'; END IF;
  IF p_observation->>'full_digest_scan_completed'='true' THEN
    IF jsonb_typeof(p_observation->'chunks') IS DISTINCT FROM 'array'
      OR jsonb_array_length(p_observation->'chunks')<>(SELECT count(*) FROM _oo.c5_chunk WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND retired_at IS NULL)
      OR (SELECT count(DISTINCT value->>'chunk_id') FROM jsonb_array_elements(p_observation->'chunks'))<>jsonb_array_length(p_observation->'chunks')
      THEN RAISE EXCEPTION 'C5 incomplete observation' USING ERRCODE='OO030'; END IF;
    SELECT * INTO scan FROM _oo.c5_scan WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND status='running' FOR UPDATE;
    IF scan.scan_id IS NOT NULL AND jsonb_array_length(scan.expected_chunks)<>jsonb_array_length(p_observation->'chunks') THEN
      RAISE EXCEPTION 'C5 pending scan blocks' USING ERRCODE='OO030'; END IF;
    FOR obs IN SELECT value FROM jsonb_array_elements(p_observation->'chunks') LOOP
      SELECT * INTO STRICT ch FROM _oo.c5_chunk WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=obs->>'chunk_id' AND retired_at IS NULL FOR UPDATE;
      IF ch.arrow_sha256 IS NULL OR ch.arrow_sha256 IS DISTINCT FROM obs->>'arrow_sha256'
        OR ch.membership_revision IS DISTINCT FROM (obs->>'membership_revision')::bigint
        OR ch.lo IS DISTINCT FROM obs->'lo' OR ch.hi IS DISTINCT FROM obs->'hi'
        OR ch.row_count IS DISTINCT FROM (obs->'digest'->>0)::bigint
        OR obs->'digest'->>1 !~ '^[0-9a-f]{16}$' OR obs->'digest'->>2 !~ '^[0-9a-f]{16}$'
        THEN RAISE EXCEPTION 'C5 observation differs from receipted block' USING ERRCODE='OO030'; END IF;
      UPDATE _oo.c5_chunk SET previous_digest_hi=('x'||(obs->'digest'->>1))::bit(64),previous_digest_lo=('x'||(obs->'digest'->>2))::bit(64),
        current_digest_hi=('x'||(obs->'digest'->>1))::bit(64),current_digest_lo=('x'||(obs->'digest'->>2))::bit(64),
        scanned_at=at_time,last_scan_id=scan.scan_id
        WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND chunk_id=ch.chunk_id;
    END LOOP;
    UPDATE _oo.c5_scan SET status='completed',completed_at=at_time,next_block=jsonb_array_length(expected_chunks)
      WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND status='running';
    UPDATE _oo.c5_table SET last_full_digest_scan_completed_at=at_time WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
  END IF;
  IF p_observation->>'recent_window_watermark_ms' IS NOT NULL THEN
    UPDATE _oo.c5_table SET recent_window_watermark=to_timestamp((p_observation->>'recent_window_watermark_ms')::double precision/1000)
      WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
  END IF;
  UPDATE _oo.table_quality q SET c5_mode='layered',last_full_digest_scan_completed_at=t.last_full_digest_scan_completed_at,
    recent_window_watermark=t.recent_window_watermark FROM _oo.c5_table t
    WHERE t.source=p_source AND t.schema_name=p_schema AND t.table_name=p_table AND q.schema_name=p_schema AND q.table_name=p_table;
END $$;


CREATE OR REPLACE FUNCTION _oo.c5_range_guard(p_shard text,p_owner uuid,p_fence bigint,p_scope jsonb,p_keys text[])
RETURNS text[] LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE s text:=p_scope->>'source_id'; ns text:=p_scope->>'schema'; t text:=p_scope->>'table';
 c text:=p_scope->>'chunk_id'; state _oo.c5_table; chunk _oo.c5_chunk; missing text[]; n bigint;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
  IF (SELECT count(*) FROM jsonb_object_keys(p_scope))<>20
    OR (SELECT bool_or(jsonb_typeof(p_scope->k) IS DISTINCT FROM 'string' OR p_scope->>k='') FROM unnest(ARRAY['protocol','source_engine','capture_method','source_id','schema','table','schema_fingerprint','chunk_id','arrow_sha256','delete_detection','position_codec','applied_pos']) k)
    OR (SELECT bool_or(jsonb_typeof(p_scope->k) IS DISTINCT FROM 'number') FROM unnest(ARRAY['chunk_topology_revision','prior_membership_revision','parent_generation','source_generation','row_count']) k)
    OR jsonb_typeof(p_scope->'members') IS DISTINCT FROM 'array'
    OR NOT p_scope ?& ARRAY['protocol','source_engine','capture_method','source_id','schema','table','schema_fingerprint','chunk_topology_revision','chunk_id','lo','hi','prior_membership_revision','parent_generation','source_generation','arrow_sha256','row_count','delete_detection','position_codec','applied_pos','members']
    OR p_scope->>'protocol'<>'oo.pipeline.native-c5-range/v1' OR p_scope->>'source_engine'<>'mysql'
    OR p_scope->>'capture_method'<>'c5_diff' OR p_scope->>'delete_detection'<>'reconcile'
    OR p_scope->>'position_codec'<>'snapshot-zero/v1' OR p_scope->>'applied_pos'<>'0'
    OR (p_scope->>'source_generation')::bigint<>(p_scope->>'parent_generation')::bigint+1
    OR jsonb_array_length(p_scope->'members')<>cardinality(p_keys)
    OR (p_scope->>'row_count')::bigint<>cardinality(p_keys)
    OR p_scope->>'arrow_sha256' !~ '^[0-9a-f]{64}$' OR p_scope->>'schema_fingerprint' !~ '^[0-9a-f]{64}$'
    OR (p_scope->>'prior_membership_revision')::bigint<0
    OR (p_scope->>'chunk_topology_revision')::bigint<1
    THEN RAISE EXCEPTION 'C5 scope' USING ERRCODE='22023'; END IF;
  SELECT source_generation INTO n FROM _oo.source_watermark WHERE source=s;
  IF COALESCE(n,0)<>(p_scope->>'parent_generation')::bigint THEN RAISE EXCEPTION 'C5 stale generation' USING ERRCODE='OO010'; END IF;
  IF (p_scope->>'parent_generation')::bigint>0 AND NOT EXISTS(SELECT FROM _oo.c5_table WHERE source=s AND schema_name=ns AND table_name=t) THEN
    RAISE EXCEPTION 'C5 baseline membership initialization required' USING ERRCODE='OO030';
  END IF;
  INSERT INTO _oo.c5_table(source,schema_name,table_name,schema_fingerprint,topology_revision,source_generation)
    VALUES(s,ns,t,p_scope->>'schema_fingerprint',(p_scope->>'chunk_topology_revision')::bigint,(p_scope->>'parent_generation')::bigint)
    ON CONFLICT DO NOTHING;
  SELECT * INTO STRICT state FROM _oo.c5_table WHERE source=s AND schema_name=ns AND table_name=t FOR UPDATE;
  IF state.schema_fingerprint<>p_scope->>'schema_fingerprint' OR state.topology_revision<>(p_scope->>'chunk_topology_revision')::bigint
    OR state.source_generation<>(p_scope->>'parent_generation')::bigint THEN RAISE EXCEPTION 'C5 scope state' USING ERRCODE='OO010'; END IF;
  INSERT INTO _oo.c5_chunk(source,schema_name,table_name,chunk_id,topology_revision,lo,hi)
    VALUES(s,ns,t,c,state.topology_revision,p_scope->'lo',p_scope->'hi') ON CONFLICT DO NOTHING;
  SELECT * INTO STRICT chunk FROM _oo.c5_chunk WHERE source=s AND schema_name=ns AND table_name=t AND chunk_id=c FOR UPDATE;
  IF chunk.topology_revision<>state.topology_revision OR chunk.lo IS DISTINCT FROM p_scope->'lo' OR chunk.hi IS DISTINCT FROM p_scope->'hi'
    OR chunk.membership_revision<>(p_scope->>'prior_membership_revision')::bigint
    THEN RAISE EXCEPTION 'C5 chunk scope' USING ERRCODE='OO010'; END IF;
  IF (SELECT count(DISTINCT _oo.c5_key_identity(value)) FROM jsonb_array_elements(p_scope->'members'))<>cardinality(p_keys)
    OR (SELECT count(DISTINCT k) FROM unnest(p_keys) k)<>cardinality(p_keys)
    OR EXISTS(SELECT FROM _oo.c5_member m JOIN jsonb_array_elements(p_scope->'members') k ON m.key_identity=_oo.c5_key_identity(k.value)
      WHERE m.source=s AND m.schema_name=ns AND m.table_name=t AND m.chunk_id<>c)
    THEN RAISE EXCEPTION 'C5 overlapping membership' USING ERRCODE='22023'; END IF;
  SELECT COALESCE(array_agg(m.loader_key),ARRAY[]::text[]) INTO missing FROM _oo.c5_member m
    WHERE m.source=s AND m.schema_name=ns AND m.table_name=t AND m.chunk_id=c
    AND NOT EXISTS(SELECT FROM jsonb_array_elements(p_scope->'members') k WHERE _oo.c5_key_identity(k.value)=m.key_identity);
  RETURN missing;
END $$;
