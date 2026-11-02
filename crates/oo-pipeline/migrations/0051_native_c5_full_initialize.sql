ALTER TABLE _oo.c5_chunk ADD COLUMN retired_at timestamptz;

CREATE FUNCTION _oo.c5_initialize_full(p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,p_fingerprint text,p_topology bigint,p_chunk text,p_keys text[])
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
   OR r.source_generation<>(SELECT source_generation FROM _oo.source_watermark WHERE source=p_source)
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
REVOKE ALL ON FUNCTION _oo.c5_initialize_full(text,uuid,bigint,text,text,text,text,bigint,text,text[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_initialize_full(text,uuid,bigint,text,text,text,text,bigint,text,text[]) TO oo_pipeline_runtime;

CREATE OR REPLACE FUNCTION _oo.c5_scan_begin(p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,p_scan text,p_expected jsonb)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE existing _oo.c5_scan;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
  PERFORM 1 FROM _oo.c5_table WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
  IF NOT FOUND OR jsonb_typeof(p_expected)<>'array' OR jsonb_array_length(p_expected)=0
    OR (SELECT count(DISTINCT value) FROM jsonb_array_elements(p_expected))<>jsonb_array_length(p_expected)
    OR EXISTS(SELECT FROM _oo.c5_chunk c WHERE c.source=p_source AND c.schema_name=p_schema AND c.table_name=p_table AND c.retired_at IS NULL
      AND NOT p_expected @> jsonb_build_array(c.chunk_id))
    THEN RAISE EXCEPTION 'C5 complete scan manifest required' USING ERRCODE='22023'; END IF;
  SELECT * INTO existing FROM _oo.c5_scan WHERE source=p_source AND schema_name=p_schema AND table_name=p_table AND status='running' FOR UPDATE;
  IF FOUND THEN
    IF existing.scan_id<>p_scan OR existing.expected_chunks<>p_expected THEN RAISE EXCEPTION 'C5 active scan differs' USING ERRCODE='OO010'; END IF;
    RETURN existing.next_block;
  END IF;
  INSERT INTO _oo.c5_scan(source,schema_name,table_name,scan_id,status,expected_chunks)
    VALUES(p_source,p_schema,p_table,p_scan,'running',p_expected);
  RETURN 0;
END $$;

CREATE OR REPLACE FUNCTION _oo.c5_observe(p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,p_generation bigint,p_observation jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE state _oo.c5_table; obs jsonb; ch _oo.c5_chunk; scan _oo.c5_scan; at_time timestamptz;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
  SELECT * INTO STRICT state FROM _oo.c5_table WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
  IF state.source_generation<>p_generation OR state.schema_fingerprint<>p_observation->>'schema_fingerprint'
    OR COALESCE((SELECT source_generation FROM _oo.source_watermark WHERE source=p_source),0)<>p_generation
    THEN RAISE EXCEPTION 'C5 stale observation' USING ERRCODE='OO010'; END IF;
  at_time:=to_timestamp((p_observation->>'checked_at_ms')::double precision/1000);
  IF at_time IS NULL OR at_time>clock_timestamp()+interval '5 minutes' THEN RAISE EXCEPTION 'C5 observation time'; END IF;
  IF p_observation->>'full_digest_scan_completed'='true' THEN
    IF jsonb_typeof(p_observation->'chunks')<>'array'
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

CREATE OR REPLACE FUNCTION _oo.c5_authority_export(p_source text,p_schema text,p_table text) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
 SELECT jsonb_build_object('source_id',t.source,'schema',t.schema_name,'table',t.table_name,'source_generation',t.source_generation,
   'chunk_topology_revision',t.topology_revision,'schema_fingerprint',t.schema_fingerprint,'input_digest',t.input_digest,
   'next_chunk_index',COALESCE((SELECT next_block FROM _oo.c5_scan s WHERE s.source=t.source AND s.schema_name=t.schema_name AND s.table_name=t.table_name AND status='running'),0),
   'chunks',COALESCE((SELECT jsonb_agg(jsonb_build_object('chunk_id',c.chunk_id,'lo',c.lo,'hi',c.hi,'membership_revision',c.membership_revision,
     'digest',CASE WHEN c.previous_digest_hi IS NULL THEN NULL ELSE jsonb_build_array(c.row_count,lpad(to_hex(c.previous_digest_hi::bigint),16,'0'),lpad(to_hex(c.previous_digest_lo::bigint),16,'0')) END,
     'arrow_sha256',c.arrow_sha256,'scanned_at_ms',floor(extract(epoch FROM c.scanned_at)*1000)::bigint) ORDER BY CASE WHEN c.chunk_id ~ '^[0-9]+$' THEN c.chunk_id::numeric END,c.chunk_id COLLATE "C")
     FROM _oo.c5_chunk c WHERE c.source=t.source AND c.schema_name=t.schema_name AND c.table_name=t.table_name AND c.retired_at IS NULL),'[]'::jsonb))
 FROM _oo.c5_table t WHERE t.source=p_source AND t.schema_name=p_schema AND t.table_name=p_table
$$;
