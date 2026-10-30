-- C5 cache adoption of an already sealed, affected-table-only full import.
-- Run as oo_target_owner after 0104. No source read, generation advance or publish.
-- Historical block receipts reference permanent identities, not replaceable cache.
CREATE TABLE _oo.c5_chunk_identity (
 source text NOT NULL, schema_name text NOT NULL, table_name text NOT NULL, chunk_id text NOT NULL,
 PRIMARY KEY(source,schema_name,table_name,chunk_id)
);
INSERT INTO _oo.c5_chunk_identity SELECT source,schema_name,table_name,chunk_id FROM _oo.c5_chunk;
REVOKE ALL ON _oo.c5_chunk_identity FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_keep_chunk_identity() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
BEGIN
 INSERT INTO _oo.c5_chunk_identity VALUES(NEW.source,NEW.schema_name,NEW.table_name,NEW.chunk_id)
  ON CONFLICT DO NOTHING;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_keep_chunk_identity() FROM PUBLIC,oo_pipeline_runtime;
CREATE TRIGGER c5_keep_chunk_identity AFTER INSERT ON _oo.c5_chunk
 FOR EACH ROW EXECUTE FUNCTION _oo.c5_keep_chunk_identity();
DO $$
DECLARE fk name;
BEGIN
 SELECT conname INTO STRICT fk FROM pg_constraint
  WHERE conrelid='_oo.c5_block_receipt'::regclass AND confrelid='_oo.c5_chunk'::regclass AND contype='f';
 EXECUTE format('ALTER TABLE _oo.c5_block_receipt DROP CONSTRAINT %I',fk);
 ALTER TABLE _oo.c5_block_receipt ADD CONSTRAINT c5_block_receipt_chunk_identity_fkey
  FOREIGN KEY(source,schema_name,table_name,chunk_id) REFERENCES _oo.c5_chunk_identity;
END $$;

CREATE TABLE _oo.c5_widening_rebase (
 archive_id uuid PRIMARY KEY, source text NOT NULL, schema_name text NOT NULL, table_name text NOT NULL,
 old_generation bigint NOT NULL, new_generation bigint NOT NULL CHECK(new_generation>old_generation),
 old_run uuid NOT NULL REFERENCES _oo.full_import_run, new_run uuid NOT NULL REFERENCES _oo.full_import_run,
 old_physical text NOT NULL, new_physical text NOT NULL,
 old_fingerprint text NOT NULL, new_fingerprint text NOT NULL,
 key_columns text[] NOT NULL, widened_columns integer NOT NULL CHECK(widened_columns>0),
 expected_source_generation bigint NOT NULL, task uuid NOT NULL, job_generation bigint NOT NULL,
 archived_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 UNIQUE(source,schema_name,table_name,new_generation)
);
REVOKE ALL ON _oo.c5_widening_rebase FROM PUBLIC,oo_pipeline_runtime;
DO $$
DECLARE rel text; pk text;
BEGIN
 FOREACH rel IN ARRAY ARRAY['c5_table','c5_multipart_baseline','c5_chunk','c5_multipart_hydration',
   'c5_scan','c5_member','c5_overlay_baseline_key','table_quality'] LOOP
  EXECUTE format('CREATE TABLE _oo.%I (
    archive_id uuid NOT NULL REFERENCES _oo.c5_widening_rebase,
    LIKE _oo.%I INCLUDING DEFAULTS INCLUDING CONSTRAINTS)', 'c5_widening_'||rel,rel);
  SELECT string_agg(quote_ident(a.attname),',' ORDER BY k.ord) INTO STRICT pk
   FROM pg_constraint c CROSS JOIN LATERAL unnest(c.conkey) WITH ORDINALITY k(num,ord)
   JOIN pg_attribute a ON a.attrelid=c.conrelid AND a.attnum=k.num
   WHERE c.conrelid=to_regclass('_oo.'||rel) AND c.contype='p';
  EXECUTE format('ALTER TABLE _oo.%I ADD PRIMARY KEY(archive_id,%s)','c5_widening_'||rel,pk);
  EXECUTE format('REVOKE ALL ON _oo.%I FROM PUBLIC,oo_pipeline_runtime','c5_widening_'||rel);
 END LOOP;
END $$;

CREATE FUNCTION _oo.c5_rebase_widening_if_needed(
 p_shard text,p_owner uuid,p_fence bigint,p_source text,p_schema text,p_table text,
 p_fingerprint text,p_expected_source_generation bigint,p_keys text[],p_task uuid,p_job_generation bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE state _oo.c5_table; fresh _oo.table_freshness; baseline _oo.c5_multipart_baseline;
 old_run _oo.full_import_run; new_run _oo.full_import_run;
 old_table _oo.full_import_table; new_table _oo.full_import_table;
 history _oo.c5_widening_rebase; high bigint; old_oid oid; new_oid oid;
 old_keys text[]; old_col record; new_col record; n integer:=0; wider integer:=0;
 new_name text; archive uuid:=gen_random_uuid(); rel text; preserved record; held record; initialized jsonb;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 PERFORM _oo.c5_parts_live(p_source,p_task,p_job_generation);
 PERFORM _oo.c5_parts_identity_check(p_source,p_task,p_job_generation);
 IF NOT EXISTS(SELECT FROM jw_native.occurrences occurrence WHERE occurrence.task_id=p_task
   AND occurrence.manifest#>>'{c5_state,schema}'=p_schema
   AND occurrence.manifest#>>'{c5_state,table}'=p_table
   AND occurrence.manifest#>>'{c5_state,schema_fingerprint}'=p_fingerprint
   AND occurrence.manifest#>'{c5_state,key_columns}'=to_jsonb(p_keys))
 THEN RAISE EXCEPTION 'C5 widening original occurrence scope differs' USING ERRCODE='OO030'; END IF;
 IF p_fingerprint IS NULL OR p_fingerprint !~ '^[0-9a-f]{64}$'
  OR p_keys IS NULL OR cardinality(p_keys) NOT BETWEEN 1 AND 64 OR array_lower(p_keys,1)<>1
  OR EXISTS(SELECT FROM unnest(p_keys) k WHERE k IS NULL OR k='')
  OR (SELECT count(DISTINCT k) FROM unnest(p_keys) k)<>cardinality(p_keys)
 THEN RAISE EXCEPTION 'C5 widening closed request' USING ERRCODE='22023'; END IF;
 SELECT source_generation INTO STRICT high FROM _oo.source_watermark WHERE source=p_source FOR UPDATE;
 IF high IS DISTINCT FROM p_expected_source_generation
 THEN RAISE EXCEPTION 'C5 widening source CAS' USING ERRCODE='OO010'; END IF;
 SELECT * INTO state FROM _oo.c5_table
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 IF NOT FOUND THEN RETURN jsonb_build_object('rebased',false,'reason','absent'); END IF;
 SELECT * INTO STRICT fresh FROM _oo.table_freshness
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 SELECT * INTO baseline FROM _oo.c5_multipart_baseline
  WHERE source=p_source AND schema_name=p_schema AND table_name=p_table FOR UPDATE;
 SELECT r.* INTO STRICT old_run FROM _oo.full_import_run r JOIN _oo.full_import_table ft USING(run_id)
  WHERE r.source_id=p_source AND r.source_generation=state.source_generation AND r.input_digest=state.input_digest
   AND ft.schema_ident=p_schema AND ft.table_ident=p_table AND ft.exhausted AND ft.canonical_digest IS NOT NULL
   AND r.state IN ('awaiting_publish','published') AND r.sealed_at IS NOT NULL AND r.retired_at IS NULL;
 SELECT * INTO STRICT old_table FROM _oo.full_import_table WHERE run_id=old_run.run_id
  AND schema_ident=p_schema AND table_ident=p_table;
 SELECT coalesce((SELECT key_idents FROM _oo.c5_overlay WHERE physical_name=old_table.physical_name),
   baseline.key_columns,
   (SELECT key_idents FROM _oo.c5_overlay_baseline_key WHERE source=p_source AND schema_name=p_schema
     AND table_name=p_table AND physical_name=old_table.physical_name AND source_generation=state.source_generation))
  INTO old_keys;
 IF old_keys IS DISTINCT FROM p_keys OR (baseline.run_id IS NOT NULL AND baseline.key_columns IS DISTINCT FROM p_keys)
 THEN RAISE EXCEPTION 'C5 widening key authority differs' USING ERRCODE='OO030'; END IF;
 SELECT * INTO history FROM _oo.c5_widening_rebase WHERE source=p_source AND schema_name=p_schema
  AND table_name=p_table AND new_generation=fresh.source_generation;
 IF FOUND AND (history.new_physical IS DISTINCT FROM fresh.physical_name
   OR history.new_fingerprint IS DISTINCT FROM p_fingerprint OR history.key_columns IS DISTINCT FROM p_keys)
 THEN RAISE EXCEPTION 'C5 widening replay differs' USING ERRCODE='OO010'; END IF;
 IF state.schema_fingerprint=p_fingerprint AND state.source_generation=fresh.source_generation THEN
  RETURN jsonb_build_object('rebased',false,'reason','unchanged','archive_id',history.archive_id);
 END IF;
 IF fresh.mode<>'full' OR fresh.state NOT IN ('awaiting_publish','live')
  OR fresh.source_generation<=state.source_generation OR fresh.source_generation>high
  OR state.schema_fingerprint=p_fingerprint
 THEN RAISE EXCEPTION 'C5 widening newer full table required' USING ERRCODE='OO010'; END IF;
 IF to_regclass('_oo.c5_oracle_binding') IS NOT NULL THEN
  EXECUTE 'SELECT EXISTS(SELECT FROM _oo.c5_oracle_binding WHERE source=$1 AND schema_name=$2 AND table_name=$3)'
   INTO preserved USING p_source,p_schema,p_table;
  IF preserved.exists THEN RAISE EXCEPTION 'C5 widening MySQL route required' USING ERRCODE='OO030'; END IF;
 END IF;
 SELECT r.* INTO STRICT new_run FROM _oo.full_import_run r JOIN _oo.full_import_table ft USING(run_id)
  WHERE r.source_id=p_source AND r.source_generation=fresh.source_generation
   AND r.target_generation=fresh.target_generation AND ft.physical_name=fresh.physical_name
   AND ft.schema_ident=p_schema AND ft.table_ident=p_table AND ft.exhausted AND ft.canonical_digest IS NOT NULL
   AND r.state IN ('awaiting_publish','published') AND r.sealed_at IS NOT NULL AND r.retired_at IS NULL;
 SELECT * INTO STRICT new_table FROM _oo.full_import_table WHERE run_id=new_run.run_id
  AND schema_ident=p_schema AND table_ident=p_table;
 IF new_run.shard_id<>p_shard OR old_run.shard_id<>p_shard
  OR new_run.config_digest IS DISTINCT FROM old_run.config_digest
  OR new_run.type_digest IS DISTINCT FROM old_run.type_digest
  OR new_table.applied_pos<>0
  OR (SELECT count(*) FROM _oo.full_import_table WHERE run_id=new_run.run_id)<>1
  OR EXISTS(SELECT FROM _oo.c5_overlay WHERE run_id=new_run.run_id)
 THEN RAISE EXCEPTION 'C5 widening full receipt identity differs' USING ERRCODE='OO030'; END IF;
 old_oid:=coalesce((SELECT delta_oid FROM _oo.c5_overlay WHERE physical_name=old_table.physical_name),
   to_regclass(format('_oo.%I',old_table.physical_name)));
 new_oid:=to_regclass(format('_oo.%I',new_table.physical_name));
 IF NOT EXISTS(SELECT FROM pg_class WHERE oid=old_oid AND relkind='r' AND relpersistence='p'
    AND relowner='oo_target_owner'::regrole)
  OR NOT EXISTS(SELECT FROM pg_class WHERE oid=new_oid AND relkind='r' AND relpersistence='p'
    AND relowner='oo_target_owner'::regrole)
 THEN RAISE EXCEPTION 'C5 widening typed owned heaps required' USING ERRCODE='OO030'; END IF;
 FOR preserved IN SELECT o.physical_name FROM _oo.c5_overlay o JOIN _oo.full_import_run r USING(run_id)
   WHERE r.source_id=p_source AND o.scope->>'schema'=p_schema AND o.scope->>'table'=p_table LOOP
  IF NOT _oo.c5_overlay_is_valid(preserved.physical_name)
  THEN RAISE EXCEPTION 'C5 widening prior overlay invalid' USING ERRCODE='OO030'; END IF;
 END LOOP;
 -- Resolve logical names through sealed per-generation mappings, never caller SQL.
 FOR old_col IN SELECT a.*,m.logical_column FROM pg_attribute a JOIN _oo.physical_mappings m
   ON m.physical_name=a.attname AND m.source=p_source AND m.config_digest=old_run.config_digest
    AND m.run_generation=old_run.source_generation AND m.level='c'
    AND m.logical_namespace=p_schema AND m.logical_table=p_table
   WHERE a.attrelid=old_oid AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum LOOP
  n:=n+1;
  new_name:=_oo.lookup_physical_mapping(p_source,new_run.config_digest,new_run.source_generation,
    'column',p_schema,p_table,old_col.logical_column);
  SELECT * INTO STRICT new_col FROM pg_attribute
   WHERE attrelid=new_oid AND attname=new_name AND attnum>0 AND NOT attisdropped;
  IF old_col.attnum<>n+1 OR new_col.attnum<>old_col.attnum
   OR (old_col.atttypid,old_col.attcollation,old_col.attnotnull)
      IS DISTINCT FROM (new_col.atttypid,new_col.attcollation,new_col.attnotnull)
  THEN RAISE EXCEPTION 'C5 widening names order type collation or nullability differs' USING ERRCODE='OO030'; END IF;
  IF new_col.atttypmod IS DISTINCT FROM old_col.atttypmod THEN
   IF old_col.logical_column=ANY(p_keys) OR old_col.atttypid<>1043
    OR old_col.atttypmod<=4 OR new_col.atttypmod<=old_col.atttypmod
   THEN RAISE EXCEPTION 'C5 widening only nonkey bounded varchar increase allowed' USING ERRCODE='OO030'; END IF;
   wider:=wider+1;
  END IF;
 END LOOP;
 IF wider=0 OR n=0
  OR n+1<>(SELECT count(*) FROM pg_attribute WHERE attrelid=old_oid AND attnum>0 AND NOT attisdropped)
  OR n+1<>(SELECT count(*) FROM pg_attribute WHERE attrelid=new_oid AND attnum>0 AND NOT attisdropped)
  OR n<>(SELECT count(*) FROM _oo.physical_mappings WHERE source=p_source AND config_digest=new_run.config_digest
    AND run_generation=new_run.source_generation AND level='c' AND logical_namespace=p_schema AND logical_table=p_table)
 THEN RAISE EXCEPTION 'C5 widening complete projection required' USING ERRCODE='OO030'; END IF;
 INSERT INTO _oo.c5_widening_rebase(archive_id,source,schema_name,table_name,old_generation,new_generation,
  old_run,new_run,old_physical,new_physical,old_fingerprint,new_fingerprint,key_columns,widened_columns,
  expected_source_generation,task,job_generation)
 VALUES(archive,p_source,p_schema,p_table,state.source_generation,fresh.source_generation,
  old_run.run_id,new_run.run_id,old_table.physical_name,new_table.physical_name,state.schema_fingerprint,p_fingerprint,
  p_keys,wider,high,p_task,p_job_generation);
 FOREACH rel IN ARRAY ARRAY['c5_table','c5_multipart_baseline','c5_chunk','c5_multipart_hydration',
   'c5_scan','c5_member','c5_overlay_baseline_key'] LOOP
  EXECUTE format('INSERT INTO _oo.%I SELECT $1,t.* FROM _oo.%I t
    WHERE source=$2 AND schema_name=$3 AND table_name=$4','c5_widening_'||rel,rel)
   USING archive,p_source,p_schema,p_table;
 END LOOP;
 INSERT INTO _oo.c5_widening_table_quality SELECT archive,q.* FROM _oo.table_quality q
  WHERE schema_name=p_schema AND table_name=p_table;
 FOREACH rel IN ARRAY ARRAY['c5_member','c5_multipart_hydration','c5_scan',
   'c5_multipart_baseline','c5_overlay_baseline_key','c5_chunk','c5_table'] LOOP
  EXECUTE format('DELETE FROM _oo.%I WHERE source=$1 AND schema_name=$2 AND table_name=$3',rel)
   USING p_source,p_schema,p_table;
 END LOOP;
 -- Existing API checks genuine single/multipart receipts; any failure rolls
 -- the archive and cache replacement back together. It does not hydrate keys.
 initialized:=_oo.c5_initialize_published_if_absent(p_shard,p_owner,p_fence,p_source,p_schema,p_table,
   p_fingerprint,1,p_keys,p_expected_source_generation);
 UPDATE _oo.table_quality SET c5_mode='layered',last_full_digest_scan_completed_at=NULL,recent_window_watermark=NULL
  WHERE schema_name=p_schema AND table_name=p_table;
 FOR preserved IN SELECT o.physical_name FROM _oo.c5_overlay o JOIN _oo.full_import_run r USING(run_id)
   WHERE r.source_id=p_source AND o.scope->>'schema'=p_schema AND o.scope->>'table'=p_table LOOP
  IF NOT _oo.c5_overlay_is_valid(preserved.physical_name)
  THEN RAISE EXCEPTION 'C5 widening changed prior overlay validity' USING ERRCODE='OO030'; END IF;
 END LOOP;
 SELECT * INTO held FROM jobs.loader_hold(p_task,p_job_generation);
 IF held.state IS DISTINCT FROM 'running' OR held.live IS DISTINCT FROM true
 THEN RAISE EXCEPTION 'C5 widening live claim lost' USING ERRCODE='OO010'; END IF;
 PERFORM _oo.c5_parts_live(p_source,p_task,p_job_generation);
 RETURN jsonb_build_object('rebased',true,'archive_id',archive,'old_generation',state.source_generation,
  'new_generation',fresh.source_generation,'widened_columns',wider,'initialized',initialized,
  'published',false,'acknowledged',false);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_rebase_widening_if_needed(text,uuid,bigint,text,text,text,text,bigint,text[],uuid,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_rebase_widening_if_needed(text,uuid,bigint,text,text,text,text,bigint,text[],uuid,bigint)
 TO oo_pipeline_runtime;
