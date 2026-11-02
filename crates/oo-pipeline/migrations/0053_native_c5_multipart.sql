ALTER TABLE _oo.full_import_run ADD COLUMN multipart_scope jsonb;
CREATE UNIQUE INDEX c5_multipart_occurrence ON _oo.full_import_run(source_id,(multipart_scope->>'occurrence')) WHERE multipart_scope IS NOT NULL;

CREATE FUNCTION _oo.c5_multipart_open(p_shard text,p_owner uuid,p_fence bigint,p_source text,p_generation bigint,p_config text,p_type text,p_input text,p_schema text,p_scope jsonb,p_physical text,p_columns text[],p_oids bigint[],p_typmods integer[],p_required boolean[])
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE r _oo.full_import_run; run uuid; item jsonb; i integer:=0;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 IF p_scope->>'protocol' IS DISTINCT FROM 'oo.mysql.c5-multipart/v1' OR p_scope->>'source_id' IS DISTINCT FROM p_source
  OR (p_scope->>'source_generation')::bigint IS DISTINCT FROM p_generation OR p_scope->>'schema_sha256' IS DISTINCT FROM p_schema
  OR jsonb_typeof(p_scope->'parts') IS DISTINCT FROM 'array' OR jsonb_array_length(p_scope->'parts')<1
  OR jsonb_array_length(p_scope->'parts') IS DISTINCT FROM (p_scope->>'part_count')::integer
 THEN RAISE EXCEPTION 'C5 complete multipart scope required'; END IF;
 FOR item IN SELECT value FROM jsonb_array_elements(p_scope->'parts') LOOP
  IF (item->>'index')::integer IS DISTINCT FROM i OR (item->>'chunk_id')::integer IS DISTINCT FROM i
   OR (item->>'total')::integer IS DISTINCT FROM jsonb_array_length(p_scope->'parts')
   OR item->>'arrow_sha256' !~ '^[0-9a-f]{64}$' THEN RAISE EXCEPTION 'C5 ordered parts required'; END IF;
  i:=i+1;
 END LOOP;
 SELECT * INTO r FROM _oo.full_import_run WHERE source_id=p_source AND multipart_scope->>'occurrence'=p_scope->>'occurrence' FOR UPDATE;
 IF FOUND THEN
  IF r.multipart_scope IS DISTINCT FROM p_scope OR r.config_digest<>p_config OR r.type_digest<>p_type OR r.input_digest<>p_input OR r.schema_digest<>p_schema
   OR r.source_generation<>p_generation OR r.shard_id<>p_shard OR r.owner_token<>p_owner OR r.state='retired'
   THEN RAISE EXCEPTION 'C5 candidate identity differs' USING ERRCODE='OO010'; END IF;
  IF r.state='open' THEN
   UPDATE _oo.full_import_run SET target_generation=p_fence WHERE run_id=r.run_id;
  END IF;
  RETURN jsonb_build_object('run_id',r.run_id,'state',r.state,'target_generation',CASE WHEN r.state='open' THEN p_fence ELSE r.target_generation END);
 END IF;
 IF EXISTS(SELECT FROM _oo.full_import_run WHERE source_id=p_source AND source_generation=p_generation AND multipart_scope->>'table'=p_scope->>'table' AND multipart_scope->>'schema'=p_scope->>'schema') THEN
  RAISE EXCEPTION 'C5 generation already has a candidate' USING ERRCODE='OO010'; END IF;
 run:=_oo.full_import_open_run(p_shard,p_owner,p_fence,p_source,p_generation,p_config,p_type,p_input,p_schema,ARRAY[p_scope->>'schema'],ARRAY[p_scope->>'table'],ARRAY[p_physical]);
 UPDATE _oo.full_import_run SET multipart_scope=p_scope WHERE run_id=run;
 PERFORM _oo.full_import_prepare_shadow(p_shard,p_owner,p_fence,run,0,p_source,p_config,p_generation,p_scope->>'schema',p_scope->>'table',p_columns,p_oids,p_typmods,p_required);
 RETURN jsonb_build_object('run_id',run,'state','open','target_generation',p_fence);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_multipart_open(text,uuid,bigint,text,bigint,text,text,text,text,jsonb,text,text[],bigint[],integer[],boolean[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_multipart_open(text,uuid,bigint,text,bigint,text,text,text,text,jsonb,text,text[],bigint[],integer[],boolean[]) TO oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_multipart_complete(p_shard text,p_owner uuid,p_fence bigint,p_run uuid)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE r _oo.full_import_run; t _oo.full_import_table; item jsonb; n bigint:=0; seen bigint;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 SELECT * INTO STRICT r FROM _oo.full_import_run WHERE run_id=p_run AND shard_id=p_shard AND owner_token=p_owner AND target_generation=p_fence AND state='open' FOR UPDATE;
 SELECT * INTO STRICT t FROM _oo.full_import_table WHERE run_id=p_run AND table_index=0;
 FOR item IN SELECT value FROM jsonb_array_elements(r.multipart_scope->'parts') LOOP
  IF NOT EXISTS(SELECT FROM _oo.apply_receipts WHERE source=r.source_id AND table_generation=t.physical_name
   AND txn_or_batch_id=(r.multipart_scope->>'occurrence')||'.part.'||(item->>'index') AND payload_digest=item->>'arrow_sha256')
  THEN RAISE EXCEPTION 'C5 part ACK missing' USING ERRCODE='OO030'; END IF;
  n:=n+(item->>'rows')::bigint;
 END LOOP;
 SELECT count(*) INTO seen FROM _oo.full_import_quality WHERE run_id=p_run AND kind='batch';
 IF seen<>jsonb_array_length(r.multipart_scope->'parts') OR t.applied_rows<>n THEN RAISE EXCEPTION 'C5 ACK coverage differs'; END IF;
 RETURN n;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_multipart_complete(text,uuid,bigint,uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_multipart_complete(text,uuid,bigint,uuid) TO oo_pipeline_runtime;
