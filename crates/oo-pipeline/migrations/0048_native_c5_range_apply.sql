CREATE FUNCTION _oo.c5_key_identity(p_key jsonb) RETURNS bytea
LANGUAGE plpgsql IMMUTABLE STRICT SET search_path=pg_catalog,_oo AS $$
DECLARE v jsonb; b bytea; result bytea := ''::bytea;
BEGIN
  IF jsonb_typeof(p_key)<>'array' OR jsonb_array_length(p_key)=0 THEN RAISE EXCEPTION 'C5 key'; END IF;
  FOR v IN SELECT value FROM jsonb_array_elements(p_key) LOOP
    IF v->>'kind'='int' THEN b := '\x00'::bytea || int8send((v->>'value')::bigint);
    ELSIF v->>'kind' IN ('text','bytes') AND v->>'value' ~ '^([0-9a-f]{2})*$' THEN
      b := CASE WHEN v->>'kind'='text' THEN '\x01'::bytea ELSE '\x02'::bytea END || decode(v->>'value','hex');
    ELSE RAISE EXCEPTION 'C5 key'; END IF;
    result := result || int4send(octet_length(b)) || b;
  END LOOP;
  RETURN result;
END $$;

CREATE FUNCTION _oo.c5_range_guard(p_shard text,p_owner uuid,p_fence bigint,p_scope jsonb,p_keys text[])
RETURNS text[] LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE s text:=p_scope->>'source_id'; ns text:=p_scope->>'schema'; t text:=p_scope->>'table';
 c text:=p_scope->>'chunk_id'; state _oo.c5_table; chunk _oo.c5_chunk; missing text[]; n bigint;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
  IF p_scope->>'protocol'<>'oo.pipeline.native-c5-range/v1' OR p_scope->>'source_engine'<>'mysql'
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

CREATE FUNCTION _oo.c5_range_commit(p_shard text,p_owner uuid,p_fence bigint,p_scope jsonb,p_keys text[],p_run uuid,p_batch text,p_payload text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE s text:=p_scope->>'source_id'; ns text:=p_scope->>'schema'; t text:=p_scope->>'table'; c text:=p_scope->>'chunk_id';
 scan _oo.c5_scan;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
  IF NOT EXISTS(SELECT FROM _oo.full_import_run r JOIN _oo.update_batch b ON b.run_id=r.run_id JOIN _oo.apply_receipts a
    ON a.source=b.source AND a.table_generation=b.physical_name AND a.txn_or_batch_id=b.batch_id AND a.payload_digest=b.payload_digest
    WHERE r.run_id=p_run AND r.state='awaiting_publish' AND r.source_id=s AND r.source_generation=(p_scope->>'source_generation')::bigint
      AND r.target_generation=p_fence AND r.input_digest=p_scope->>'arrow_sha256'
      AND b.schema_name=ns AND b.table_name=t AND b.batch_id=p_batch AND b.payload_digest=p_payload)
    THEN RAISE EXCEPTION 'C5 sealed Loader receipt required' USING ERRCODE='OO030'; END IF;
  PERFORM _oo.c5_range_guard_after_seal(p_scope);
  DELETE FROM _oo.c5_member WHERE source=s AND schema_name=ns AND table_name=t AND chunk_id=c;
  INSERT INTO _oo.c5_member(source,schema_name,table_name,chunk_id,key_identity,typed_key,loader_key)
    SELECT s,ns,t,c,_oo.c5_key_identity(k.value),k.value,p_keys[k.ordinality] FROM jsonb_array_elements(p_scope->'members') WITH ORDINALITY k;
  INSERT INTO _oo.c5_block_receipt VALUES(s,ns,t,c,(p_scope->>'source_generation')::bigint,p_scope,p_run,p_batch,p_payload,p_fence,clock_timestamp());
  SELECT * INTO scan FROM _oo.c5_scan WHERE source=s AND schema_name=ns AND table_name=t AND status='running' FOR UPDATE;
  UPDATE _oo.c5_chunk SET membership_revision=membership_revision+1,row_count=(p_scope->>'row_count')::bigint,
    arrow_sha256=p_scope->>'arrow_sha256',last_scope=p_scope,scanned_at=clock_timestamp(),last_scan_id=scan.scan_id
    WHERE source=s AND schema_name=ns AND table_name=t AND chunk_id=c;
  UPDATE _oo.c5_table SET source_generation=(p_scope->>'source_generation')::bigint,input_digest=p_scope->>'arrow_sha256'
    WHERE source=s AND schema_name=ns AND table_name=t;
  IF scan.scan_id IS NOT NULL THEN
    UPDATE _oo.c5_scan SET next_block=COALESCE((SELECT min(k.ordinality)-1 FROM jsonb_array_elements_text(scan.expected_chunks) WITH ORDINALITY k
      LEFT JOIN _oo.c5_chunk ch ON ch.source=s AND ch.schema_name=ns AND ch.table_name=t AND ch.chunk_id=k.value
      WHERE ch.last_scan_id IS DISTINCT FROM scan.scan_id),jsonb_array_length(scan.expected_chunks))
      WHERE source=s AND schema_name=ns AND table_name=t AND scan_id=scan.scan_id;
  END IF;
  UPDATE _oo.table_quality SET c5_mode='layered' WHERE schema_name=ns AND table_name=t;
END $$;

CREATE FUNCTION _oo.c5_range_guard_after_seal(p_scope jsonb) RETURNS void LANGUAGE plpgsql
SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
BEGIN
  IF NOT EXISTS(SELECT FROM _oo.c5_table t JOIN _oo.c5_chunk c USING(source,schema_name,table_name)
    WHERE t.source=p_scope->>'source_id' AND t.schema_name=p_scope->>'schema' AND t.table_name=p_scope->>'table'
    AND c.chunk_id=p_scope->>'chunk_id' AND c.membership_revision=(p_scope->>'prior_membership_revision')::bigint
    AND t.source_generation=(p_scope->>'parent_generation')::bigint AND t.schema_fingerprint=p_scope->>'schema_fingerprint'
    AND c.topology_revision=(p_scope->>'chunk_topology_revision')::bigint AND c.lo=p_scope->'lo' AND c.hi=p_scope->'hi')
    THEN RAISE EXCEPTION 'C5 stale commit' USING ERRCODE='OO010'; END IF;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_key_identity(jsonb),_oo.c5_range_guard_after_seal(jsonb) FROM PUBLIC,oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.c5_range_guard(text,uuid,bigint,jsonb,text[]),_oo.c5_range_commit(text,uuid,bigint,jsonb,text[],uuid,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_range_guard(text,uuid,bigint,jsonb,text[]),_oo.c5_range_commit(text,uuid,bigint,jsonb,text[],uuid,text,text) TO oo_pipeline_runtime;
