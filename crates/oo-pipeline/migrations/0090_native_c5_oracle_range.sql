-- Oracle SCN-bound integer ranges. Existing MySQL functions remain unchanged.
CREATE TABLE _oo.c5_oracle_binding (
  source text NOT NULL, schema_name text NOT NULL, table_name text NOT NULL,
  source_schema text NOT NULL, source_table text NOT NULL, identity text NOT NULL,
  baseline_run uuid NOT NULL REFERENCES _oo.full_import_run,
  snapshot_scn numeric(38,0) NOT NULL CHECK(snapshot_scn>0),
  manifest_sha256 text NOT NULL CHECK(manifest_sha256 ~ '^[0-9a-f]{64}$'),
  key_ident text NOT NULL,
  PRIMARY KEY(source,schema_name,table_name)
);
CREATE TABLE _oo.c5_oracle_checked (
  source text NOT NULL, schema_name text NOT NULL, table_name text NOT NULL, chunk_id text NOT NULL,
  snapshot_scn numeric(38,0) NOT NULL, manifest_sha256 text NOT NULL,
  PRIMARY KEY(source,schema_name,table_name,chunk_id),
  FOREIGN KEY(source,schema_name,table_name,chunk_id) REFERENCES _oo.c5_chunk
);
REVOKE ALL ON _oo.c5_oracle_binding,_oo.c5_oracle_checked FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_oracle_ident(p_name text) RETURNS text
LANGUAGE plpgsql STABLE STRICT SET search_path=pg_catalog,_oo AS $$
DECLARE n text:=normalize(p_name,NFC);
BEGIN
 IF n='' OR octet_length(n)>128 THEN RAISE EXCEPTION 'Oracle source identifier' USING ERRCODE='22023'; END IF;
 IF octet_length(n)<=63 AND left(n,2)<>'q_' AND _oo.pm_logical_ok(n) THEN RETURN n; END IF;
 RETURN 'q_'||encode(convert_to(n,'UTF8'),'hex');
END $$;
REVOKE ALL ON FUNCTION _oo.c5_oracle_ident(text) FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_oracle_context(p_source text,p_schema text,p_table text,p_context jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE b _oo.c5_oracle_binding; scn numeric(38,0);
BEGIN
 IF jsonb_typeof(p_context) IS DISTINCT FROM 'object'
  OR (SELECT count(*) FROM jsonb_object_keys(p_context))<>5
  OR NOT p_context ?& ARRAY['identity','scn','manifest_sha256','source_schema','source_table']
  OR EXISTS(SELECT FROM jsonb_each(p_context) e WHERE jsonb_typeof(e.value)<>'string')
  OR coalesce(p_context->>'scn','') !~ '^[1-9][0-9]{0,37}$'
  OR coalesce(p_context->>'manifest_sha256','') !~ '^[0-9a-f]{64}$'
  OR coalesce(p_context->>'identity','')='' OR octet_length(p_context->>'identity')>1024
 THEN RAISE EXCEPTION 'Oracle C5 snapshot context' USING ERRCODE='22023'; END IF;
 SELECT * INTO STRICT b FROM _oo.c5_oracle_binding
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 scn:=(p_context->>'scn')::numeric;
 IF b.identity IS DISTINCT FROM p_context->>'identity'
  OR b.source_schema IS DISTINCT FROM p_context->>'source_schema'
  OR b.source_table IS DISTINCT FROM p_context->>'source_table'
  OR p_schema IS DISTINCT FROM _oo.c5_oracle_ident(p_context->>'source_schema')
  OR p_table IS DISTINCT FROM _oo.c5_oracle_ident(p_context->>'source_table')
  OR scn<b.snapshot_scn
  OR (scn=b.snapshot_scn AND b.manifest_sha256 IS DISTINCT FROM p_context->>'manifest_sha256')
 THEN RAISE EXCEPTION 'Oracle C5 source identity or SCN differs' USING ERRCODE='OO010'; END IF;
 IF scn>b.snapshot_scn THEN
  IF EXISTS(SELECT FROM _oo.c5_chunk c LEFT JOIN _oo.c5_oracle_checked k
    USING(source,schema_name,table_name,chunk_id)
    WHERE c.source=p_source AND c.schema_name=p_schema AND c.table_name=p_table AND c.retired_at IS NULL
     AND (k.snapshot_scn IS DISTINCT FROM b.snapshot_scn OR k.manifest_sha256 IS DISTINCT FROM b.manifest_sha256))
  THEN RAISE EXCEPTION 'Oracle C5 previous snapshot unfinished' USING ERRCODE='OO030'; END IF;
  UPDATE _oo.c5_oracle_binding SET snapshot_scn=scn,manifest_sha256=p_context->>'manifest_sha256'
   WHERE source=p_source AND schema_name=p_schema AND table_name=p_table;
 END IF;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_oracle_context(text,text,text,jsonb) FROM PUBLIC,oo_pipeline_runtime;

-- Administrator bootstrap is pinned to an actual published full-import run.
-- Membership is read from target typed keys, never supplied by the caller.
CREATE FUNCTION _oo.c5_oracle_initialize(
 p_source text,p_run uuid,p_context jsonb,p_fingerprint text,p_key text,p_chunks jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE r _oo.full_import_run; t _oo.full_import_table; b _oo.c5_oracle_binding;
 ns text; tbl text; key_name text; physical_key text; fence bigint; ch jsonb; prior jsonb:='null';
 n integer:=0; row_total bigint; actual bigint; scn numeric(38,0);
BEGIN
 IF coalesce(p_fingerprint,'') !~ '^[0-9a-f]{64}$' OR jsonb_typeof(p_chunks) IS DISTINCT FROM 'array'
  OR jsonb_array_length(p_chunks) NOT BETWEEN 1 AND 4096
  OR jsonb_typeof(p_context) IS DISTINCT FROM 'object'
  OR coalesce(p_context->>'scn','') !~ '^[1-9][0-9]{0,37}$'
  OR coalesce(p_context->>'manifest_sha256','') !~ '^[0-9a-f]{64}$'
  OR coalesce(p_context->>'identity','')='' OR octet_length(p_context->>'identity')>1024
  OR (SELECT count(*) FROM jsonb_object_keys(p_context))<>5
  OR NOT p_context ?& ARRAY['identity','scn','manifest_sha256','source_schema','source_table']
  OR EXISTS(SELECT FROM jsonb_each(p_context) e WHERE jsonb_typeof(e.value)<>'string')
 THEN RAISE EXCEPTION 'Oracle C5 initialization context' USING ERRCODE='22023'; END IF;
 ns:=_oo.c5_oracle_ident(p_context->>'source_schema'); tbl:=_oo.c5_oracle_ident(p_context->>'source_table');
 key_name:=_oo.c5_oracle_ident(p_key); scn:=(p_context->>'scn')::numeric;
 SELECT * INTO STRICT r FROM _oo.full_import_run WHERE run_id=p_run AND source_id=p_source;
 PERFORM 1 FROM _oo.target_generations WHERE shard_id=r.shard_id FOR UPDATE;
 PERFORM 1 FROM _oo.source_watermark WHERE source=p_source FOR UPDATE;
 SELECT * INTO STRICT t FROM _oo.full_import_table WHERE run_id=p_run AND schema_ident=ns AND table_ident=tbl;
 SELECT * INTO b FROM _oo.c5_oracle_binding WHERE source=p_source AND schema_name=ns AND table_name=tbl;
 IF FOUND THEN
  IF b.baseline_run<>p_run OR b.identity<>p_context->>'identity' OR b.key_ident<>key_name
   OR b.source_schema<>p_context->>'source_schema' OR b.source_table<>p_context->>'source_table'
   OR b.snapshot_scn<>scn OR b.manifest_sha256<>p_context->>'manifest_sha256'
   OR (SELECT schema_fingerprint FROM _oo.c5_table WHERE source=p_source AND schema_name=ns AND table_name=tbl) IS DISTINCT FROM p_fingerprint
   OR (SELECT jsonb_agg(jsonb_build_object('chunk_id',chunk_id,'lo',lo,'hi',hi) ORDER BY chunk_id::integer)
       FROM _oo.c5_chunk WHERE source=p_source AND schema_name=ns AND table_name=tbl AND retired_at IS NULL) IS DISTINCT FROM p_chunks
  THEN RAISE EXCEPTION 'Oracle C5 initialization replay differs' USING ERRCODE='OO010'; END IF;
  RETURN jsonb_build_object('outcome','replay','source_generation',r.source_generation);
 END IF;
 IF r.state<>'published' OR NOT t.exhausted OR t.canonical_digest IS NULL
  OR (SELECT count(*) FROM _oo.full_import_table WHERE run_id=p_run)<>1
  OR NOT EXISTS(SELECT FROM _oo.source_watermark w WHERE w.source=p_source
    AND w.source_generation=r.source_generation AND w.target_generation=r.target_generation AND w.state='live'
    AND w.position_codec='snapshot-zero/v1' AND w.position_lineage='oracle.snapshot'
    AND w.consistency_grade='S0' AND NOT w.cross_table_consistent)
  OR NOT EXISTS(SELECT FROM _oo.table_freshness f WHERE f.source=p_source AND f.schema_name=ns AND f.table_name=tbl
    AND f.source_generation=r.source_generation AND f.physical_name=t.physical_name AND f.mode='full' AND f.state='live')
  OR EXISTS(SELECT FROM _oo.c5_table WHERE source=p_source AND schema_name=ns AND table_name=tbl)
 THEN RAISE EXCEPTION 'Oracle C5 published single full baseline required' USING ERRCODE='OO030'; END IF;
 physical_key:=_oo.lookup_physical_mapping(p_source,r.config_digest,r.source_generation,'column',ns,tbl,key_name);
 IF NOT EXISTS(SELECT FROM pg_attribute WHERE attrelid=to_regclass(format('_oo.%I',t.physical_name))
   AND attname=physical_key AND attnum>0 AND NOT attisdropped AND attnotnull AND atttypid IN (20,21,23))
 THEN RAISE EXCEPTION 'Oracle C5 integer nonnull key required' USING ERRCODE='22023'; END IF;
 FOR ch IN SELECT value FROM jsonb_array_elements(p_chunks) LOOP
  IF (SELECT count(*) FROM jsonb_object_keys(ch))<>3 OR ch->>'chunk_id' IS DISTINCT FROM n::text
   OR ch->'lo' IS DISTINCT FROM prior OR NOT ch ?& ARRAY['chunk_id','lo','hi']
   OR (ch->'hi'='null'::jsonb AND n<>jsonb_array_length(p_chunks)-1)
   OR (ch->'hi'<>'null'::jsonb AND (jsonb_array_length(ch->'hi')<>1
     OR ch->'hi'->0->>'kind'<>'int' OR jsonb_typeof(ch->'hi'->0->'value')<>'number'))
   OR (prior<>'null'::jsonb AND ch->'hi'<>'null'::jsonb
     AND _oo.c5_overlay_key_compare(prior,ch->'hi')>=0)
  THEN RAISE EXCEPTION 'Oracle C5 closed integer topology' USING ERRCODE='22023'; END IF;
  prior:=ch->'hi'; n:=n+1;
 END LOOP;
 IF prior<>'null'::jsonb THEN RAISE EXCEPTION 'Oracle C5 unbounded final range required' USING ERRCODE='22023'; END IF;
 fence:=_oo.adopt_target_generation(r.shard_id,r.owner_token);
 PERFORM _oo.full_import_bind_session(r.shard_id,r.owner_token,fence);
 PERFORM _oo.c5_initialize_full(r.shard_id,r.owner_token,fence,p_source,ns,tbl,p_fingerprint,1,'0',ARRAY[key_name]);
 INSERT INTO _oo.c5_oracle_binding VALUES(p_source,ns,tbl,p_context->>'source_schema',p_context->>'source_table',
  p_context->>'identity',p_run,scn,p_context->>'manifest_sha256',key_name);
 FOR ch IN SELECT value FROM jsonb_array_elements(p_chunks) LOOP
  INSERT INTO _oo.c5_chunk(source,schema_name,table_name,chunk_id,topology_revision,lo,hi,membership_revision)
   VALUES(p_source,ns,tbl,ch->>'chunk_id',1,ch->'lo',ch->'hi',1)
   ON CONFLICT(source,schema_name,table_name,chunk_id) DO UPDATE SET lo=EXCLUDED.lo,hi=EXCLUDED.hi;
 END LOOP;
 UPDATE _oo.c5_member m SET chunk_id=ch.chunk_id FROM _oo.c5_chunk ch
  WHERE m.source=p_source AND m.schema_name=ns AND m.table_name=tbl
   AND ch.source=m.source AND ch.schema_name=m.schema_name AND ch.table_name=m.table_name AND ch.retired_at IS NULL
   AND (ch.lo='null'::jsonb OR _oo.c5_overlay_key_compare(m.typed_key,ch.lo)>0)
   AND (ch.hi='null'::jsonb OR _oo.c5_overlay_key_compare(m.typed_key,ch.hi)<=0);
 UPDATE _oo.c5_chunk ch SET row_count=(SELECT count(*) FROM _oo.c5_member m WHERE m.source=ch.source
    AND m.schema_name=ch.schema_name AND m.table_name=ch.table_name AND m.chunk_id=ch.chunk_id),arrow_sha256=NULL
  WHERE ch.source=p_source AND ch.schema_name=ns AND ch.table_name=tbl;
 SELECT sum(row_count) INTO row_total FROM _oo.c5_chunk WHERE source=p_source AND schema_name=ns AND table_name=tbl;
 SELECT count(*) INTO actual FROM _oo.c5_member WHERE source=p_source AND schema_name=ns AND table_name=tbl;
 IF row_total<>t.applied_rows OR actual<>t.applied_rows THEN RAISE EXCEPTION 'Oracle C5 membership count' USING ERRCODE='OO030'; END IF;
 INSERT INTO _oo.c5_oracle_checked SELECT source,schema_name,table_name,chunk_id,scn,p_context->>'manifest_sha256'
  FROM _oo.c5_chunk WHERE source=p_source AND schema_name=ns AND table_name=tbl;
 RETURN jsonb_build_object('outcome','initialized','source_generation',r.source_generation,'chunks',n,'rows',actual);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_oracle_initialize(text,uuid,jsonb,text,text,jsonb) FROM PUBLIC,oo_pipeline_runtime;

ALTER FUNCTION _oo.c5_range_guard(text,uuid,bigint,jsonb,text[]) RENAME TO c5_range_guard_before_oracle;
REVOKE ALL ON FUNCTION _oo.c5_range_guard_before_oracle(text,uuid,bigint,jsonb,text[]) FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_range_guard(p_shard text,p_owner uuid,p_fence bigint,p_scope jsonb,p_keys text[])
RETURNS text[] LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE s text:=p_scope->>'source_id'; ns text:=p_scope->>'schema'; tbl text:=p_scope->>'table';
 state _oo.c5_table; ch _oo.c5_chunk; missing text[]; high bigint;
BEGIN
 IF p_scope->>'source_engine' IS DISTINCT FROM 'oracle' THEN
  IF EXISTS(SELECT FROM _oo.c5_oracle_binding WHERE source=s AND schema_name=ns AND table_name=tbl) THEN
   RAISE EXCEPTION 'Oracle C5 source cannot use another engine' USING ERRCODE='OO010';
  END IF;
  RETURN _oo.c5_range_guard_before_oracle(p_shard,p_owner,p_fence,p_scope,p_keys);
 END IF;
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 IF jsonb_typeof(p_scope) IS DISTINCT FROM 'object' OR (SELECT count(*) FROM jsonb_object_keys(p_scope))<>21
  OR NOT p_scope ?& ARRAY['protocol','source_engine','capture_method','source_id','schema','table','schema_fingerprint',
    'chunk_topology_revision','chunk_id','lo','hi','prior_membership_revision','parent_generation','source_generation',
    'arrow_sha256','row_count','delete_detection','position_codec','applied_pos','members','oracle_snapshot']
  OR p_scope->>'protocol' IS DISTINCT FROM 'oo.pipeline.native-c5-range/v1'
  OR p_scope->>'capture_method' IS DISTINCT FROM 'c5_diff' OR p_scope->>'delete_detection' IS DISTINCT FROM 'reconcile'
  OR p_scope->>'position_codec' IS DISTINCT FROM 'snapshot-zero/v1' OR p_scope->>'applied_pos' IS DISTINCT FROM '0'
  OR coalesce(p_scope->>'schema_fingerprint','') !~ '^[0-9a-f]{64}$'
  OR coalesce(p_scope->>'arrow_sha256','') !~ '^[0-9a-f]{64}$'
  OR p_keys IS NULL OR cardinality(p_keys)>100000 OR array_position(p_keys,NULL) IS NOT NULL
  OR p_scope->'row_count' IS DISTINCT FROM to_jsonb(cardinality(p_keys))
  OR jsonb_typeof(p_scope->'members') IS DISTINCT FROM 'array'
  OR jsonb_array_length(p_scope->'members')<>cardinality(p_keys)
 THEN RAISE EXCEPTION 'Oracle C5 closed range scope' USING ERRCODE='22023'; END IF;
 SELECT source_generation INTO STRICT high FROM _oo.source_watermark WHERE source=s FOR UPDATE;
 SELECT * INTO STRICT state FROM _oo.c5_table WHERE source=s AND schema_name=ns AND table_name=tbl FOR UPDATE;
 SELECT * INTO STRICT ch FROM _oo.c5_chunk WHERE source=s AND schema_name=ns AND table_name=tbl
  AND chunk_id=p_scope->>'chunk_id' AND retired_at IS NULL FOR UPDATE;
 IF p_scope->'parent_generation' IS DISTINCT FROM to_jsonb(high)
  OR p_scope->'source_generation' IS DISTINCT FROM to_jsonb(high+1)
  OR state.source_generation<>high OR state.schema_fingerprint IS DISTINCT FROM p_scope->>'schema_fingerprint'
  OR p_scope->'chunk_topology_revision' IS DISTINCT FROM to_jsonb(state.topology_revision)
  OR ch.topology_revision<>state.topology_revision OR ch.lo IS DISTINCT FROM p_scope->'lo' OR ch.hi IS DISTINCT FROM p_scope->'hi'
  OR p_scope->'prior_membership_revision' IS DISTINCT FROM to_jsonb(ch.membership_revision)
 THEN RAISE EXCEPTION 'Oracle C5 stale parent or membership' USING ERRCODE='OO010'; END IF;
 PERFORM _oo.c5_oracle_context(s,ns,tbl,p_scope->'oracle_snapshot');
 IF EXISTS(SELECT FROM jsonb_array_elements(p_scope->'members') WITH ORDINALITY k
   WHERE jsonb_array_length(k.value)<>1 OR k.value->0->>'kind'<>'int'
    OR _oo.c5_overlay_loader_key(k.value) IS DISTINCT FROM p_keys[k.ordinality]
    OR (ch.lo<>'null'::jsonb AND _oo.c5_overlay_key_compare(k.value,ch.lo)<=0)
    OR (ch.hi<>'null'::jsonb AND _oo.c5_overlay_key_compare(k.value,ch.hi)>0))
  OR (SELECT count(DISTINCT k) FROM unnest(p_keys) k)<>cardinality(p_keys)
  OR EXISTS(SELECT FROM _oo.c5_member m JOIN jsonb_array_elements(p_scope->'members') k
    ON m.key_identity=_oo.c5_key_identity(k.value)
    WHERE m.source=s AND m.schema_name=ns AND m.table_name=tbl AND m.chunk_id<>ch.chunk_id)
 THEN RAISE EXCEPTION 'Oracle C5 typed membership or bounds' USING ERRCODE='22023'; END IF;
 SELECT coalesce(array_agg(m.loader_key),ARRAY[]::text[]) INTO missing FROM _oo.c5_member m
  WHERE m.source=s AND m.schema_name=ns AND m.table_name=tbl AND m.chunk_id=ch.chunk_id
   AND NOT EXISTS(SELECT FROM jsonb_array_elements(p_scope->'members') k WHERE m.key_identity=_oo.c5_key_identity(k.value));
 RETURN missing;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_range_guard(text,uuid,bigint,jsonb,text[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_range_guard(text,uuid,bigint,jsonb,text[]) TO oo_pipeline_runtime;

ALTER FUNCTION _oo.c5_range_commit(text,uuid,bigint,jsonb,text[],uuid,text,text) RENAME TO c5_range_commit_before_oracle;
REVOKE ALL ON FUNCTION _oo.c5_range_commit_before_oracle(text,uuid,bigint,jsonb,text[],uuid,text,text) FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_range_commit(p_shard text,p_owner uuid,p_fence bigint,p_scope jsonb,p_keys text[],p_run uuid,p_batch text,p_payload text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
BEGIN
 PERFORM _oo.c5_range_commit_before_oracle(p_shard,p_owner,p_fence,p_scope,p_keys,p_run,p_batch,p_payload);
 IF p_scope->>'source_engine'='oracle' THEN
  UPDATE _oo.c5_oracle_checked SET snapshot_scn=(p_scope->'oracle_snapshot'->>'scn')::numeric,
   manifest_sha256=p_scope->'oracle_snapshot'->>'manifest_sha256'
   WHERE source=p_scope->>'source_id' AND schema_name=p_scope->>'schema' AND table_name=p_scope->>'table'
    AND chunk_id=p_scope->>'chunk_id';
  IF NOT FOUND THEN RAISE EXCEPTION 'Oracle C5 chunk not initialized' USING ERRCODE='OO030'; END IF;
 END IF;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_range_commit(text,uuid,bigint,jsonb,text[],uuid,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_range_commit(text,uuid,bigint,jsonb,text[],uuid,text,text) TO oo_pipeline_runtime;

-- Unchanged ranges are checked against actual target cells, not asserted by hash.
CREATE FUNCTION _oo.c5_oracle_check_unchanged(p_source text,p_context jsonb,p_chunk text,p_rows jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo SET bytea_output='hex' AS $$
DECLARE ns text:=_oo.c5_oracle_ident(p_context->>'source_schema'); tbl text:=_oo.c5_oracle_ident(p_context->>'source_table');
 b _oo.c5_oracle_binding; f _oo.table_freshness; r _oo.full_import_run; ch _oo.c5_chunk;
 col record; projection text:=''; key_col text; actual jsonb;
BEGIN
 PERFORM 1 FROM _oo.source_watermark WHERE source=p_source FOR UPDATE;
 PERFORM _oo.c5_oracle_context(p_source,ns,tbl,p_context);
 SELECT * INTO STRICT b FROM _oo.c5_oracle_binding WHERE source=p_source AND schema_name=ns AND table_name=tbl;
 SELECT * INTO STRICT ch FROM _oo.c5_chunk WHERE source=p_source AND schema_name=ns AND table_name=tbl AND chunk_id=p_chunk AND retired_at IS NULL;
 SELECT * INTO STRICT f FROM _oo.table_freshness WHERE source=p_source AND schema_name=ns AND table_name=tbl;
 SELECT run.* INTO STRICT r FROM _oo.full_import_run run JOIN _oo.full_import_table t USING(run_id)
  WHERE run.source_id=p_source AND run.source_generation=f.source_generation AND t.physical_name=f.physical_name AND t.exhausted;
 IF jsonb_typeof(p_rows) IS DISTINCT FROM 'array' OR jsonb_array_length(p_rows)>100000
  OR jsonb_array_length(p_rows)<>ch.row_count OR r.state NOT IN ('awaiting_publish','published')
 THEN RAISE EXCEPTION 'Oracle C5 unchanged range shape' USING ERRCODE='22023'; END IF;
 key_col:=_oo.lookup_physical_mapping(p_source,r.config_digest,r.source_generation,'column',ns,tbl,b.key_ident);
 FOR col IN SELECT attname,atttypid FROM pg_attribute WHERE attrelid=to_regclass(format('_oo.%I',f.physical_name))
  AND attnum>0 AND NOT attisdropped AND attname<>'_oo_pos' ORDER BY attnum LOOP
  IF projection<>'' THEN projection:=projection||','; END IF;
  projection:=projection||CASE WHEN col.atttypid=17 THEN format('encode(t.%I,''hex'')',col.attname) ELSE format('t.%I::text',col.attname) END;
 END LOOP;
 EXECUTE format('SELECT coalesce(jsonb_agg(cells ORDER BY key),''[]''::jsonb) FROM
  (SELECT t.%I AS key,jsonb_build_array(%s) AS cells FROM _oo.%I t
   WHERE ($1 IS NULL OR t.%I>$1) AND ($2 IS NULL OR t.%I<=$2)) q',
  key_col,projection,f.physical_name,key_col,key_col) INTO actual
  USING (ch.lo->0->>'value')::bigint,(ch.hi->0->>'value')::bigint;
 IF actual IS DISTINCT FROM p_rows THEN RAISE EXCEPTION 'Oracle C5 unchanged cells differ' USING ERRCODE='OO030'; END IF;
 UPDATE _oo.c5_oracle_checked SET snapshot_scn=b.snapshot_scn,manifest_sha256=b.manifest_sha256
  WHERE source=p_source AND schema_name=ns AND table_name=tbl AND chunk_id=p_chunk;
 RETURN jsonb_build_object('checked',true,'data_gen',false,'rows',ch.row_count,'chunk_id',p_chunk);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_oracle_check_unchanged(text,jsonb,text,jsonb) FROM PUBLIC,oo_pipeline_runtime;

ALTER FUNCTION _oo.publish_native_per_table_collection(uuid,text,jsonb) RENAME TO publish_native_per_table_collection_before_oracle;
REVOKE ALL ON FUNCTION _oo.publish_native_per_table_collection_before_oracle(uuid,text,jsonb) FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.publish_native_per_table_collection(p_anchor uuid,p_sha text,p_receipt jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
BEGIN
 PERFORM 1 FROM _oo.source_watermark WHERE source=p_receipt->>'source_id' FOR UPDATE;
 IF EXISTS(SELECT FROM _oo.c5_oracle_binding b JOIN _oo.c5_chunk c USING(source,schema_name,table_name)
   LEFT JOIN _oo.c5_oracle_checked k USING(source,schema_name,table_name,chunk_id)
   WHERE b.source=p_receipt->>'source_id' AND c.retired_at IS NULL
    AND (k.snapshot_scn IS DISTINCT FROM b.snapshot_scn OR k.manifest_sha256 IS DISTINCT FROM b.manifest_sha256))
 THEN RAISE EXCEPTION 'Oracle C5 complete SCN reconciliation required before publication' USING ERRCODE='OO030'; END IF;
 RETURN _oo.publish_native_per_table_collection_before_oracle(p_anchor,p_sha,p_receipt);
END $$;
REVOKE ALL ON FUNCTION _oo.publish_native_per_table_collection(uuid,text,jsonb) FROM PUBLIC,oo_pipeline_runtime;
DO $grant$
BEGIN
 IF EXISTS(SELECT FROM pg_roles WHERE rolname='oo_target_admin') THEN
  REVOKE ALL ON FUNCTION _oo.publish_native_per_table_collection_before_oracle(uuid,text,jsonb) FROM oo_target_admin;
  GRANT EXECUTE ON FUNCTION _oo.publish_native_per_table_collection(uuid,text,jsonb),
   _oo.c5_oracle_initialize(text,uuid,jsonb,text,text,jsonb),
   _oo.c5_oracle_check_unchanged(text,jsonb,text,jsonb) TO oo_target_admin;
 END IF;
END $grant$;
