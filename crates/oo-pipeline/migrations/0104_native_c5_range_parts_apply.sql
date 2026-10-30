-- native-c5-range-parts-apply/v1. Install in a transaction after 0103.
-- Requires C5 through exact 0075, 0103 stage, real
-- jobs.loader_hold (jobs 0004), and co-located jobs/jw_native catalogs readable
-- by oo_target_owner. Oracle 0090 and MySQL 0100 identity are checked when
-- present, not required. Partially installed MySQL identity fails closed.
-- The installing principal grants only the existing final lease-lock API;
-- all new objects remain owned by oo_target_owner (no jobs table write grant).
SET LOCAL ROLE oo_jobs_owner;
GRANT EXECUTE ON FUNCTION jobs.loader_hold(uuid,bigint) TO oo_target_owner;
SET LOCAL ROLE oo_target_owner;
-- Durable stage precedes this ONE transaction: guard, prepare_shadow/prepare,
-- copy_page (heartbeat between pages), reconcile, SET CONSTRAINTS ALL IMMEDIATE,
-- finish, ordinary batch/note/seal_quality(empty tombstones)/seal_run, commit.
-- "commit" below writes metadata, not COMMIT, publication, or jobs ACK.
-- after_key/last_key are lowercase hex of stage.k; NULL starts the stream.
-- copied_rows is the page count. The last request may be replayed exactly.
ALTER TABLE _oo.update_batch ALTER COLUMN tombstone_count TYPE bigint;

CREATE TABLE _oo.c5_parts_apply (
 source text NOT NULL, task uuid NOT NULL, scope jsonb NOT NULL,
 shard text NOT NULL, owner uuid NOT NULL, fence bigint NOT NULL,
 job_generation bigint NOT NULL, prepared_xid xid8 NOT NULL,
 parent_generation bigint NOT NULL, parent_physical text NOT NULL, parent_digest text NOT NULL,
 deleted_rows bigint NOT NULL CHECK(deleted_rows>=0),
 run_id uuid UNIQUE REFERENCES _oo.full_import_run,
 copied_rows bigint NOT NULL DEFAULT 0, last_key bytea,
 previous_after bytea, previous_limit integer, previous_rows bigint,
 PRIMARY KEY(source,task),
 FOREIGN KEY(source,task) REFERENCES _oo.c5_parts_capture
);
REVOKE ALL ON _oo.c5_parts_apply FROM PUBLIC,oo_pipeline_runtime;

-- A JSON value for ONE typed key, never a JSON collection of range members.
CREATE FUNCTION _oo.c5_parts_typed_sql(p_spec jsonb,p_alias text) RETURNS text
LANGUAGE plpgsql IMMUTABLE SET search_path=pg_catalog,_oo AS $$
DECLARE k jsonb; c jsonb; n integer; col text; piece text; result text:='';
BEGIN
 FOR k IN SELECT value FROM jsonb_array_elements(p_spec->'keys') LOOP
  n:=(k#>>'{}')::integer; c:=p_spec->'columns'->n; col:=format('%I.%I',p_alias,'c'||n);
  IF (c->>'oid')::oid IN (20,21,23) THEN
   piece:=format('jsonb_build_object(''kind'',''int'',''value'',%s::bigint)',col);
  ELSIF (c->>'oid')::oid IN (25,1043) THEN
   piece:=format('jsonb_build_object(''kind'',''text'',''value'',encode(convert_to(%s::text,''UTF8''),''hex''))',col);
  ELSIF (c->>'oid')::oid=17 THEN
   piece:=format('jsonb_build_object(''kind'',''bytes'',''value'',encode(%s,''hex''))',col);
  ELSE RAISE EXCEPTION 'C5 parts key type' USING ERRCODE='22023';
  END IF;
  result:=result||CASE WHEN result='' THEN '' ELSE ',' END||piece;
 END LOOP;
 IF result='' THEN RAISE EXCEPTION 'C5 parts keys required' USING ERRCODE='22023'; END IF;
 RETURN 'jsonb_build_array('||result||')';
END $$;

CREATE FUNCTION _oo.c5_parts_stage_bound(p_scope jsonb) RETURNS _oo.c5_parts_capture
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE c _oo.c5_parts_capture; g bigint;
BEGIN
 IF jsonb_typeof(p_scope) IS DISTINCT FROM 'object'
  OR (SELECT count(*) FROM jsonb_object_keys(p_scope))<>20
  OR NOT p_scope ?& ARRAY['protocol','source_engine','capture_method','source_id','schema','table',
    'schema_fingerprint','chunk_topology_revision','chunk_id','lo','hi','prior_membership_revision',
    'parent_generation','source_generation','arrow_sha256','row_count','delete_detection',
    'position_codec','applied_pos','parts_task']
  OR p_scope->>'protocol' IS DISTINCT FROM 'oo.pipeline.native-c5-range-parts/v1'
  OR p_scope->>'source_engine' IS DISTINCT FROM 'mysql'
  OR p_scope->>'capture_method' IS DISTINCT FROM 'c5_diff'
  OR p_scope->>'delete_detection' IS DISTINCT FROM 'reconcile'
  OR p_scope->>'position_codec' IS DISTINCT FROM 'snapshot-zero/v1'
  OR p_scope->>'applied_pos' IS DISTINCT FROM '0'
  OR jsonb_typeof(p_scope->'parts_task') IS DISTINCT FROM 'string'
  OR coalesce(p_scope->>'arrow_sha256','') !~ '^[0-9a-f]{64}$'
 THEN RAISE EXCEPTION 'C5 parts closed scope' USING ERRCODE='22023'; END IF;
 SELECT * INTO STRICT c FROM _oo.c5_parts_capture
  WHERE source=p_scope->>'source_id' AND task=(p_scope->>'parts_task')::uuid FOR SHARE;
 SELECT generation INTO g FROM jobs.tasks WHERE id=c.task;
 PERFORM _oo.c5_parts_live(c.source,c.task,g);
 PERFORM _oo.c5_parts_identity_check(c.source,c.task,g);
 IF NOT c.complete OR c.next_part<>c.expected_parts OR c.copied_rows<>c.expected_rows
  OR p_scope-ARRAY['protocol','capture_method','arrow_sha256','row_count','delete_detection',
     'position_codec','applied_pos','parts_task'] IS DISTINCT FROM c.header->'range'
  OR p_scope->>'arrow_sha256' IS DISTINCT FROM c.header#>>'{parts_index,sha256}'
  OR p_scope->'row_count' IS DISTINCT FROM to_jsonb(c.expected_rows)
  OR c.header->'total_rows' IS DISTINCT FROM to_jsonb(c.expected_rows)
  OR c.header->'part_count' IS DISTINCT FROM to_jsonb(c.expected_parts)
  OR NOT EXISTS(SELECT FROM pg_class cl JOIN pg_namespace ns ON ns.oid=cl.relnamespace
    WHERE cl.oid=to_regclass(format('_oo.%I',c.physical)) AND ns.nspname='_oo'
     AND cl.relname='c5p_'||md5(c.source||':'||c.task::text)
     AND cl.relkind='r' AND cl.relpersistence='p' AND cl.relowner='oo_target_owner'::regrole)
 THEN RAISE EXCEPTION 'C5 parts complete stage binding' USING ERRCODE='OO030'; END IF;
 RETURN c;
END $$;

CREATE FUNCTION _oo.c5_parts_identity_check(p_source text,p_task uuid,p_generation bigint)
RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE identity_table oid:=to_regclass('_oo.mysql_connection_current');
 job_function oid:=to_regprocedure('_oo.mysql_connection_job(text,uuid,bigint)');
 schema_function oid:=to_regprocedure('_oo.mysql_connection_schema(text)'); digest text;
BEGIN
 IF identity_table IS NULL AND job_function IS NULL AND schema_function IS NULL THEN RETURN NULL; END IF;
 IF identity_table IS NULL OR job_function IS NULL OR schema_function IS NULL
 THEN RAISE EXCEPTION 'C5 parts incomplete MySQL identity installation' USING ERRCODE='OO030'; END IF;
 EXECUTE 'SELECT _oo.mysql_connection_job($1,$2,$3)' USING p_source,p_task,p_generation;
 EXECUTE 'SELECT _oo.mysql_connection_schema($1)' INTO digest USING p_source;
 RETURN digest;
END $$;

CREATE FUNCTION _oo.c5_parts_apply_bound(p_shard text,p_owner uuid,p_fence bigint,p_scope jsonb,p_run uuid)
RETURNS _oo.c5_parts_apply LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE a _oo.c5_parts_apply; c _oo.c5_parts_capture;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 c:=_oo.c5_parts_stage_bound(p_scope);
 SELECT * INTO STRICT a FROM _oo.c5_parts_apply WHERE source=c.source AND task=c.task FOR UPDATE;
 IF (a.shard,a.owner,a.fence,a.scope,a.prepared_xid) IS DISTINCT FROM
    (p_shard,p_owner,p_fence,p_scope,pg_current_xact_id())
  OR (p_run IS NOT NULL AND a.run_id IS DISTINCT FROM p_run)
 THEN RAISE EXCEPTION 'C5 parts transaction binding' USING ERRCODE='OO010'; END IF;
 PERFORM _oo.c5_parts_live(c.source,c.task,a.job_generation);
 RETURN a;
END $$;

CREATE FUNCTION _oo.c5_parts_guard(p_shard text,p_owner uuid,p_fence bigint,p_scope jsonb)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE c _oo.c5_parts_capture; a _oo.c5_parts_apply; state _oo.c5_table; ch _oo.c5_chunk;
 f _oo.table_freshness; high bigint; g bigint; n bigint; deleted bigint; invalid boolean;
 expression text; typed text; key_cols text[]; kinds text[]; k jsonb; trusted_keys text[]; stage_keys text[];
 has_text boolean; physical oid; parent_attrs oid; parent_run _oo.full_import_run; col record;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 c:=_oo.c5_parts_stage_bound(p_scope);
 -- No task row lock here: a different connection must be able to heartbeat.
 SELECT generation INTO STRICT g FROM jobs.tasks WHERE id=c.task;
 PERFORM 1 FROM jw_native.source_progress
  WHERE source_id=c.source AND target_database=current_database() AND active_task_id=c.task;
 IF NOT FOUND THEN RAISE EXCEPTION 'C5 parts current source progress' USING ERRCODE='OO010'; END IF;
 SELECT source_generation INTO STRICT high FROM _oo.source_watermark WHERE source=c.source FOR UPDATE;
 SELECT * INTO STRICT f FROM _oo.table_freshness
  WHERE source=c.source AND schema_name=p_scope->>'schema' AND table_name=p_scope->>'table' FOR UPDATE;
 SELECT * INTO STRICT state FROM _oo.c5_table
  WHERE source=c.source AND schema_name=f.schema_name AND table_name=f.table_name FOR UPDATE;
 SELECT * INTO STRICT ch FROM _oo.c5_chunk WHERE source=c.source AND schema_name=f.schema_name
  AND table_name=f.table_name AND chunk_id=p_scope->>'chunk_id' AND retired_at IS NULL FOR UPDATE;
 IF p_scope->'parent_generation' IS DISTINCT FROM to_jsonb(high)
  OR p_scope->'source_generation' IS DISTINCT FROM to_jsonb(high+1)
  OR state.source_generation IS DISTINCT FROM f.source_generation OR state.source_generation>high
  OR f.state NOT IN ('awaiting_publish','live')
  OR state.schema_fingerprint IS DISTINCT FROM p_scope->>'schema_fingerprint'
  OR p_scope->'chunk_topology_revision' IS DISTINCT FROM to_jsonb(state.topology_revision)
  OR ch.topology_revision<>state.topology_revision
  OR (ch.lo,ch.hi) IS DISTINCT FROM (p_scope->'lo',p_scope->'hi')
  OR p_scope->'prior_membership_revision' IS DISTINCT FROM to_jsonb(ch.membership_revision)
  OR EXISTS(SELECT FROM _oo.c5_chunk WHERE source=c.source AND schema_name=f.schema_name
    AND table_name=f.table_name AND retired_at IS NULL AND baseline_membership_pending)
 THEN RAISE EXCEPTION 'C5 parts parent or chunk differs' USING ERRCODE='OO010'; END IF;
 -- MySQL-only installations need not install Oracle. An installed Oracle
 -- binding still forbids entering this route, exactly as in 0090.
 IF to_regclass('_oo.c5_oracle_binding') IS NOT NULL THEN
  EXECUTE 'SELECT EXISTS(SELECT FROM _oo.c5_oracle_binding
    WHERE source=$1 AND schema_name=$2 AND table_name=$3)'
   INTO invalid USING c.source,f.schema_name,f.table_name;
  IF invalid THEN RAISE EXCEPTION 'C5 parts Oracle binding cannot use MySQL' USING ERRCODE='OO010'; END IF;
 END IF;
 SELECT * INTO a FROM _oo.c5_parts_apply WHERE source=c.source AND task=c.task;
 IF FOUND THEN
  a:=_oo.c5_parts_apply_bound(p_shard,p_owner,p_fence,p_scope,NULL);
  IF (a.parent_generation,a.parent_physical,a.parent_digest) IS DISTINCT FROM
     (state.source_generation,f.physical_name,state.input_digest)
  THEN RAISE EXCEPTION 'C5 parts parent replay differs' USING ERRCODE='OO010'; END IF;
  RETURN a.deleted_rows;
 END IF;
 SELECT r.* INTO STRICT parent_run FROM _oo.full_import_run r JOIN _oo.full_import_table ft USING(run_id)
  WHERE r.source_id=c.source AND r.source_generation=f.source_generation
   AND r.target_generation=f.target_generation AND r.input_digest=state.input_digest
   AND r.state IN ('awaiting_publish','published') AND r.sealed_at IS NOT NULL
   AND ft.physical_name=f.physical_name AND ft.schema_ident=f.schema_name AND ft.table_ident=f.table_name
   AND ft.exhausted AND ft.canonical_digest IS NOT NULL;
 parent_attrs:=coalesce((SELECT delta_oid FROM _oo.c5_overlay WHERE physical_name=f.physical_name),
   to_regclass(format('_oo.%I',f.physical_name)));
 SELECT coalesce(
   (SELECT key_idents FROM _oo.c5_overlay WHERE physical_name=f.physical_name),
   (SELECT key_idents FROM _oo.c5_overlay_baseline_key WHERE source=c.source AND schema_name=f.schema_name
     AND table_name=f.table_name AND physical_name=f.physical_name AND source_generation=f.source_generation),
   (SELECT key_columns FROM _oo.c5_multipart_baseline WHERE source=c.source AND schema_name=f.schema_name
     AND table_name=f.table_name AND physical_name=f.physical_name AND source_generation=f.source_generation))
  INTO trusted_keys;
 SELECT array_agg(c.spec->'columns'->(value::integer)->>'name' ORDER BY ordinality) INTO stage_keys
  FROM jsonb_array_elements_text(c.spec->'keys') WITH ORDINALITY;
 IF trusted_keys IS NULL OR trusted_keys IS DISTINCT FROM stage_keys
  OR (EXISTS(SELECT FROM _oo.c5_overlay WHERE physical_name=f.physical_name)
    AND NOT _oo.c5_overlay_is_valid(f.physical_name))
 THEN RAISE EXCEPTION 'C5 parts sealed key authority differs' USING ERRCODE='OO030'; END IF;
 FOR col IN SELECT value AS v,ordinality::integer-1 AS i
   FROM jsonb_array_elements(c.spec->'columns') WITH ORDINALITY LOOP
  IF NOT EXISTS(SELECT FROM pg_attribute pa JOIN _oo.physical_mappings m
    ON m.physical_name=pa.attname AND m.source=c.source AND m.config_digest=parent_run.config_digest
     AND m.run_generation=f.source_generation AND m.level='c'
     AND m.logical_namespace=f.schema_name AND m.logical_table=f.table_name AND m.logical_column=col.v->>'name'
    WHERE pa.attrelid=parent_attrs AND pa.attnum=col.i+2 AND NOT pa.attisdropped
     AND pa.atttypid=(col.v->>'oid')::oid AND pa.atttypmod=(col.v->>'typmod')::integer
     AND pa.attnotnull=NOT (col.v->>'nullable')::boolean)
  THEN RAISE EXCEPTION 'C5 parts stage source schema differs' USING ERRCODE='23514'; END IF;
 END LOOP;
 IF (SELECT count(*) FROM pg_attribute WHERE attrelid=parent_attrs AND attnum>0 AND NOT attisdropped)
    <>jsonb_array_length(c.spec->'columns')+1
 THEN RAISE EXCEPTION 'C5 parts stage source width differs' USING ERRCODE='23514'; END IF;
 SELECT array_agg('c'||value ORDER BY ordinality),
   array_agg(CASE WHEN (c.spec->'columns'->(value::integer)->>'oid')::oid IN (20,21,23) THEN 'int'
     WHEN (c.spec->'columns'->(value::integer)->>'oid')::oid IN (25,1043) THEN 'text' ELSE 'bytes' END ORDER BY ordinality)
  INTO key_cols,kinds FROM jsonb_array_elements_text(c.spec->'keys') WITH ORDINALITY;
 has_text:='text'=ANY(kinds);
 IF has_text AND (ch.lo<>'null'::jsonb OR ch.hi<>'null'::jsonb) AND NOT EXISTS(
   SELECT FROM _oo.c5_multipart_baseline b JOIN _oo.c5_multipart_hydration h
    ON h.source=b.source AND h.schema_name=b.schema_name AND h.table_name=b.table_name AND h.run_id=b.run_id
   WHERE b.source=c.source AND b.schema_name=f.schema_name AND b.table_name=f.table_name
    AND b.key_columns=stage_keys AND h.chunk_id=ch.chunk_id
    AND b.physical_name=coalesce((SELECT root_name FROM _oo.c5_overlay WHERE physical_name=f.physical_name),f.physical_name))
 THEN RAISE EXCEPTION 'C5 parts bounded text requires source topology hydration' USING ERRCODE='OO030'; END IF;
 physical:=to_regclass(format('_oo.%I',c.physical));
 EXECUTE format('LOCK TABLE _oo.%I IN SHARE MODE',c.physical);
 expression:=_oo.c5_overlay_key_sql(physical,key_cols,'x');
 typed:=_oo.c5_parts_typed_sql(c.spec,'x');
 IF expression IS DISTINCT FROM c.key_sql
  OR (SELECT count(*) FROM _oo.c5_parts_receipt WHERE source=c.source AND task=c.task)<>c.expected_parts
  OR (SELECT coalesce(sum(rows),0) FROM _oo.c5_parts_receipt WHERE source=c.source AND task=c.task)<>c.expected_rows
 THEN RAISE EXCEPTION 'C5 parts receipts or typed key expression' USING ERRCODE='OO030'; END IF;
 FOR k IN SELECT value FROM jsonb_array_elements(jsonb_build_array(ch.lo,ch.hi)) WHERE value<>'null'::jsonb LOOP
  PERFORM _oo.c5_key_identity(k);
  IF jsonb_array_length(k)<>cardinality(kinds) OR EXISTS(
   SELECT FROM jsonb_array_elements(k) WITH ORDINALITY v WHERE v.value->>'kind' IS DISTINCT FROM kinds[v.ordinality])
  THEN RAISE EXCEPTION 'C5 parts bound key shape' USING ERRCODE='22023'; END IF;
 END LOOP;
 EXECUTE format('SELECT count(*),coalesce(bool_or(x.k IS DISTINCT FROM %s
   OR _oo.c5_overlay_loader_key(%s) IS NULL),false) FROM _oo.%I x',expression,typed,c.physical) INTO n,invalid;
 IF n<>c.expected_rows OR invalid THEN RAISE EXCEPTION 'C5 parts typed membership' USING ERRCODE='OO030'; END IF;
 IF NOT has_text THEN
  IF ch.lo<>'null'::jsonb AND ch.hi<>'null'::jsonb AND _oo.c5_overlay_key_compare(ch.lo,ch.hi)>=0
  THEN RAISE EXCEPTION 'C5 parts inverted range' USING ERRCODE='22023'; END IF;
  EXECUTE format('SELECT EXISTS(SELECT FROM _oo.%I x WHERE
    ($1<>''null''::jsonb AND _oo.c5_overlay_key_compare(%s,$1)<=0) OR
    ($2<>''null''::jsonb AND _oo.c5_overlay_key_compare(%s,$2)>0))',c.physical,typed,typed)
   INTO invalid USING ch.lo,ch.hi;
  IF invalid THEN RAISE EXCEPTION 'C5 parts member outside range' USING ERRCODE='22023'; END IF;
 END IF;
 IF EXISTS(SELECT FROM _oo.c5_member m WHERE m.source=c.source AND m.schema_name=f.schema_name
   AND m.table_name=f.table_name AND m.chunk_id=ch.chunk_id
   AND (m.key_identity IS DISTINCT FROM _oo.c5_key_identity(m.typed_key)
    OR convert_to(m.loader_key,'UTF8') IS DISTINCT FROM convert_to(_oo.c5_overlay_loader_key(m.typed_key),'UTF8')
    OR jsonb_array_length(m.typed_key)<>cardinality(kinds)
    OR EXISTS(SELECT FROM jsonb_array_elements(m.typed_key) WITH ORDINALITY v
      WHERE v.value->>'kind' IS DISTINCT FROM kinds[v.ordinality])
    OR (NOT has_text AND ((ch.lo<>'null'::jsonb AND _oo.c5_overlay_key_compare(m.typed_key,ch.lo)<=0)
     OR (ch.hi<>'null'::jsonb AND _oo.c5_overlay_key_compare(m.typed_key,ch.hi)>0)))))
 THEN RAISE EXCEPTION 'C5 parts prior typed membership' USING ERRCODE='22023'; END IF;
 EXECUTE format('SELECT EXISTS(SELECT FROM _oo.c5_member m JOIN _oo.%I x ON x.k=m.key_identity
   WHERE m.source=$1 AND m.schema_name=$2 AND m.table_name=$3 AND m.chunk_id<>$4)',c.physical)
  INTO invalid USING c.source,f.schema_name,f.table_name,ch.chunk_id;
 IF invalid THEN RAISE EXCEPTION 'C5 parts overlapping membership' USING ERRCODE='22023'; END IF;
 EXECUTE format('SELECT count(*) FROM _oo.c5_member m WHERE m.source=$1 AND m.schema_name=$2
   AND m.table_name=$3 AND m.chunk_id=$4 AND NOT EXISTS(SELECT FROM _oo.%I x WHERE x.k=m.key_identity)',c.physical)
  INTO deleted USING c.source,f.schema_name,f.table_name,ch.chunk_id;
 INSERT INTO _oo.c5_parts_apply(source,task,scope,shard,owner,fence,job_generation,prepared_xid,
   parent_generation,parent_physical,parent_digest,deleted_rows)
  VALUES(c.source,c.task,p_scope,p_shard,p_owner,p_fence,g,pg_current_xact_id(),
    state.source_generation,f.physical_name,state.input_digest,deleted);
 RETURN deleted;
END $$;

-- Preserve the flat 0066 root/component and strict mapping checks.
CREATE FUNCTION _oo.c5_parts_overlay_prepare(
  p_shard text,p_owner uuid,p_fence bigint,p_run uuid,
  p_scope jsonb,p_column_idents text[],p_key_idents text[]
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE
  capture _oo.c5_parts_capture; identity_schema text;
  r _oo.full_import_run; child _oo.full_import_table; f _oo.table_freshness;
  parent_run _oo.full_import_run; parent_table _oo.full_import_table;
  chunk _oo.c5_chunk; prior _oo.c5_overlay; old _oo.c5_overlay;
  child_oid oid; parent_oid oid; parent_attributes oid;
  parent_cols text[]:=ARRAY[]::text[]; child_cols text[]:=ARRAY[]::text[];
  pc text; cc text; ca pg_attribute; pa pg_attribute; i integer; k jsonb; trusted_keys text[];
  parent_rows bigint:=0; member_count bigint; inherited bigint; gen bigint:=0;
  root_name text; root_oid oid; root_generation bigint:=0; root_columns text[]:=ARRAY[]::text[];
  exclusion_owner text; has_text_key boolean:=false; source_order_proven boolean:=false;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
  SELECT * INTO STRICT r FROM _oo.full_import_run WHERE run_id=p_run FOR UPDATE;
  SELECT * INTO STRICT child FROM _oo.full_import_table WHERE run_id=p_run;
  IF r.state<>'open' OR r.shard_id IS DISTINCT FROM p_shard OR r.owner_token IS DISTINCT FROM p_owner
    OR r.target_generation IS DISTINCT FROM p_fence OR r.source_id IS DISTINCT FROM p_scope->>'source_id'
    OR r.source_generation IS DISTINCT FROM (p_scope->>'source_generation')::bigint
    OR r.input_digest IS DISTINCT FROM p_scope->>'arrow_sha256'
    OR child.schema_ident IS DISTINCT FROM p_scope->>'schema' OR child.table_ident IS DISTINCT FROM p_scope->>'table'
    OR p_column_idents IS NULL OR cardinality(p_column_idents) NOT BETWEEN 1 AND 1024
    OR p_key_idents IS NULL OR cardinality(p_key_idents) NOT BETWEEN 1 AND 64
    OR array_lower(p_column_idents,1)<>1 OR array_lower(p_key_idents,1)<>1
    OR array_position(p_column_idents,NULL) IS NOT NULL OR array_position(p_key_idents,NULL) IS NOT NULL
    OR (SELECT count(DISTINCT x) FROM unnest(p_column_idents) x)<>cardinality(p_column_idents)
    OR (SELECT count(DISTINCT x) FROM unnest(p_key_idents) x)<>cardinality(p_key_idents)
    OR NOT p_key_idents<@p_column_idents THEN
    RAISE EXCEPTION 'C5 overlay run binding' USING ERRCODE='22023';
  END IF;
  capture:=_oo.c5_parts_stage_bound(p_scope);
  identity_schema:=_oo.c5_parts_identity_check(capture.source,capture.task,
    (SELECT generation FROM jobs.tasks WHERE id=capture.task));
  IF p_column_idents IS DISTINCT FROM ARRAY(SELECT value->>'name' FROM jsonb_array_elements(capture.spec->'columns'))
    OR p_key_idents IS DISTINCT FROM ARRAY(SELECT capture.spec->'columns'->(value::integer)->>'name'
      FROM jsonb_array_elements_text(capture.spec->'keys'))
    OR (identity_schema IS NOT NULL AND identity_schema IS DISTINCT FROM r.schema_digest) THEN
    RAISE EXCEPTION 'C5 parts schema projection differs' USING ERRCODE='23514';
  END IF;
  SELECT * INTO old FROM _oo.c5_overlay WHERE run_id=p_run;
  IF FOUND THEN
    IF old.scope IS DISTINCT FROM p_scope OR old.loader_keys IS DISTINCT FROM ARRAY[]::text[]
      OR old.column_idents IS DISTINCT FROM p_column_idents OR old.key_idents IS DISTINCT FROM p_key_idents
      OR old.prepared_xid<>pg_current_xact_id() THEN
      RAISE EXCEPTION 'C5 overlay prepare replay differs' USING ERRCODE='OO010';
    END IF;
    PERFORM _oo.c5_parts_apply_bound(p_shard,p_owner,p_fence,p_scope,p_run);
    RETURN old.inherited_rows;
  END IF;
  -- Locks the source watermark before any quality writer can advance it.
  PERFORM _oo.c5_parts_guard(p_shard,p_owner,p_fence,p_scope);
  SELECT * INTO STRICT chunk FROM _oo.c5_chunk
    WHERE source=r.source_id AND schema_name=child.schema_ident AND table_name=child.table_ident
      AND chunk_id=p_scope->>'chunk_id' AND retired_at IS NULL FOR UPDATE;
  SELECT * INTO f FROM _oo.table_freshness
    WHERE source=r.source_id AND schema_name=child.schema_ident AND table_name=child.table_ident FOR UPDATE;
  IF FOUND THEN
    SELECT run.* INTO STRICT parent_run FROM _oo.full_import_run run JOIN _oo.full_import_table t USING(run_id)
      WHERE t.physical_name=f.physical_name AND t.schema_ident=child.schema_ident AND t.table_ident=child.table_ident
        AND run.source_id=r.source_id AND run.source_generation=f.source_generation AND t.exhausted
        AND t.canonical_digest IS NOT NULL AND run.state IN ('awaiting_publish','published');
    SELECT * INTO STRICT parent_table FROM _oo.full_import_table
      WHERE run_id=parent_run.run_id AND physical_name=f.physical_name;
    IF parent_run.config_digest<>r.config_digest OR parent_run.type_digest<>r.type_digest
      OR parent_run.schema_digest<>r.schema_digest OR f.source_generation>(p_scope->>'parent_generation')::bigint
      OR NOT EXISTS(SELECT FROM _oo.apply_receipts a WHERE a.source=r.source_id
        AND a.table_generation=f.physical_name AND a.config_digest=parent_run.config_digest
        AND a.type_digest=parent_run.type_digest AND a.schema_digest=parent_run.schema_digest)
      OR (SELECT source_generation FROM _oo.c5_table WHERE source=r.source_id
        AND schema_name=child.schema_ident AND table_name=child.table_ident) IS DISTINCT FROM f.source_generation THEN
      RAISE EXCEPTION 'C5 sealed table baseline differs' USING ERRCODE='OO030';
    END IF;
    gen:=f.source_generation; parent_rows:=parent_table.applied_rows;
    parent_oid:=to_regclass(format('_oo.%I',f.physical_name));
    SELECT * INTO prior FROM _oo.c5_overlay WHERE physical_name=f.physical_name;
    IF FOUND THEN
      IF NOT _oo.c5_overlay_is_valid(f.physical_name) THEN
        RAISE EXCEPTION 'C5 parent view is not registered' USING ERRCODE='OO030';
      END IF;
      IF prior.key_idents IS DISTINCT FROM p_key_idents THEN
        RAISE EXCEPTION 'C5 overlay key columns changed' USING ERRCODE='22023';
      END IF;
      parent_attributes:=prior.delta_oid;
      root_name:=prior.root_name; root_oid:=prior.root_oid;
      root_generation:=prior.root_generation; root_columns:=prior.root_columns;
    ELSE
      IF NOT EXISTS(SELECT FROM pg_class WHERE oid=parent_oid AND relkind='r'
        AND relpersistence='p' AND relowner='oo_target_owner'::regrole) THEN
        RAISE EXCEPTION 'C5 parent is not a sealed heap' USING ERRCODE='OO030';
      END IF;
      parent_attributes:=parent_oid;
      root_name:=f.physical_name; root_oid:=parent_oid; root_generation:=gen;
      SELECT key_idents INTO trusted_keys FROM _oo.c5_overlay_baseline_key
        WHERE source=r.source_id AND schema_name=child.schema_ident AND table_name=child.table_ident
          AND physical_name=f.physical_name AND source_generation=f.source_generation;
      IF trusted_keys IS NULL AND to_regclass('_oo.c5_multipart_baseline') IS NOT NULL THEN
        EXECUTE 'SELECT key_columns FROM _oo.c5_multipart_baseline
          WHERE source=$1 AND schema_name=$2 AND table_name=$3 AND physical_name=$4 AND source_generation=$5'
          INTO trusted_keys USING r.source_id,child.schema_ident,child.table_ident,f.physical_name,f.source_generation;
      END IF;
      IF trusted_keys IS DISTINCT FROM p_key_idents THEN
        RAISE EXCEPTION 'C5 sealed baseline key authority required; explicit rebaseline required' USING ERRCODE='OO030';
      END IF;
    END IF;
  ELSIF (p_scope->>'parent_generation')::bigint<>0 THEN
    RAISE EXCEPTION 'C5 sealed table baseline required' USING ERRCODE='OO030';
  END IF;
  IF child.physical_name IS DISTINCT FROM _oo.lookup_physical_mapping(r.source_id,r.config_digest,r.source_generation,
    'table',child.schema_ident,child.table_ident,'') THEN
    RAISE EXCEPTION 'C5 child mapping differs' USING ERRCODE='23514';
  END IF;
  IF gen>0 AND f.physical_name IS DISTINCT FROM _oo.lookup_physical_mapping(r.source_id,r.config_digest,gen,
    'table',child.schema_ident,child.table_ident,'') THEN
    RAISE EXCEPTION 'C5 parent mapping differs' USING ERRCODE='23514';
  END IF;
  child_oid:=to_regclass(format('_oo.%I',child.physical_name));
  IF NOT EXISTS(SELECT FROM pg_class WHERE oid=child_oid AND relkind='r' AND relpersistence='p'
    AND relowner='oo_target_owner'::regrole) THEN
    RAISE EXCEPTION 'C5 fresh delta heap required' USING ERRCODE='OO030';
  END IF;
  FOR i IN 1..cardinality(p_column_idents) LOOP
    cc:=_oo.lookup_physical_mapping(r.source_id,r.config_digest,r.source_generation,
      'column',child.schema_ident,child.table_ident,p_column_idents[i]);
    SELECT * INTO STRICT ca FROM pg_attribute WHERE attrelid=child_oid AND attname=cc AND attnum>0 AND NOT attisdropped;
    IF cc !~ '^pc_[0-9a-f]{60}$' OR ca.attnum<>i+1 THEN
      RAISE EXCEPTION 'C5 ordered child mapping' USING ERRCODE='23514';
    END IF;
    IF gen>0 THEN
      pc:=_oo.lookup_physical_mapping(r.source_id,r.config_digest,gen,
        'column',child.schema_ident,child.table_ident,p_column_idents[i]);
      SELECT * INTO STRICT pa FROM pg_attribute WHERE attrelid=parent_oid AND attname=pc AND attnum>0 AND NOT attisdropped;
      IF pc !~ '^pc_[0-9a-f]{60}$' OR (pa.atttypid,pa.atttypmod,pa.attcollation,pa.attnum)
        IS DISTINCT FROM (ca.atttypid,ca.atttypmod,ca.attcollation,ca.attnum)
        OR ca.attnotnull IS DISTINCT FROM (SELECT attnotnull FROM pg_attribute WHERE attrelid=parent_attributes AND attname=pc) THEN
        RAISE EXCEPTION 'C5 strict parent child schema differs' USING ERRCODE='23514';
      END IF;
      parent_cols:=array_append(parent_cols,pc);
    END IF;
    child_cols:=array_append(child_cols,cc);
    IF p_column_idents[i]=ANY(p_key_idents) AND (NOT ca.attnotnull OR ca.atttypid NOT IN (20,21,23,25,1043,17)) THEN
      RAISE EXCEPTION 'C5 unsupported or nullable key' USING ERRCODE='22023';
    END IF;
    IF p_column_idents[i]=ANY(p_key_idents) AND ca.atttypid IN (25,1043) THEN has_text_key:=true; END IF;
  END LOOP;
  IF (SELECT count(*) FROM pg_attribute WHERE attrelid=child_oid AND attnum>0 AND NOT attisdropped)<>cardinality(child_cols)+1
    OR (gen>0 AND (SELECT count(*) FROM pg_attribute WHERE attrelid=parent_oid AND attnum>0 AND NOT attisdropped)<>cardinality(child_cols)+1)
    OR EXISTS(SELECT FROM _oo.physical_mappings m WHERE m.source=r.source_id AND m.config_digest=r.config_digest
      AND m.run_generation IN (gen,r.source_generation) AND m.level='c' AND m.logical_namespace=child.schema_ident
      AND m.logical_table=child.table_ident AND NOT m.logical_column=ANY(p_column_idents)) THEN
    RAISE EXCEPTION 'C5 incomplete column mapping' USING ERRCODE='23514';
  END IF;
  FOR k IN SELECT value FROM jsonb_array_elements(jsonb_build_array(p_scope->'lo',p_scope->'hi'))
    WHERE value<>'null'::jsonb LOOP
    PERFORM _oo.c5_key_identity(k);
    IF jsonb_array_length(k)<>cardinality(p_key_idents) THEN
      RAISE EXCEPTION 'C5 bound key width' USING ERRCODE='22023';
    END IF;
    FOR i IN 1..cardinality(p_key_idents) LOOP
      SELECT * INTO STRICT ca FROM pg_attribute WHERE attrelid=child_oid
        AND attname=child_cols[array_position(p_column_idents,p_key_idents[i])];
      IF k->(i-1)->>'kind' IS DISTINCT FROM (CASE WHEN ca.atttypid IN (20,21,23) THEN 'int'
        WHEN ca.atttypid IN (25,1043) THEN 'text' ELSE 'bytes' END) THEN
        RAISE EXCEPTION 'C5 bound key kind' USING ERRCODE='22023';
      END IF;
      IF k->(i-1)->>'kind'='text' THEN PERFORM convert_from(decode(k->(i-1)->>'value','hex'),'UTF8'); END IF;
    END LOOP;
  END LOOP;
  -- MySQL text order (including case/accent-insensitive collations) cannot be
  -- reconstructed from UTF8 bytes. Only the sealed source topology + hydrated
  -- membership can authorize bounded text scopes. Rust still must require the
  -- entire scope to equal the authenticated source admission; this is not a
  -- replacement source-proof API. Storage comparisons are identity-only.
  IF has_text_key AND (p_scope->'lo'<>'null'::jsonb OR p_scope->'hi'<>'null'::jsonb) THEN
    IF to_regclass('_oo.c5_multipart_hydration') IS NOT NULL THEN
      EXECUTE 'SELECT EXISTS(SELECT FROM _oo.c5_multipart_baseline b JOIN _oo.c5_multipart_hydration h
        ON h.source=b.source AND h.schema_name=b.schema_name AND h.table_name=b.table_name AND h.run_id=b.run_id
        WHERE b.source=$1 AND b.schema_name=$2 AND b.table_name=$3 AND b.physical_name=$4
          AND b.key_columns=$5 AND h.chunk_id=$6)'
        INTO source_order_proven USING r.source_id,child.schema_ident,child.table_ident,root_name,p_key_idents,chunk.chunk_id;
    END IF;
    IF NOT source_order_proven THEN
      RAISE EXCEPTION 'C5 bounded text requires sealed source topology and hydrated membership' USING ERRCODE='OO030';
    END IF;
  END IF;
  IF NOT has_text_key AND p_scope->'lo'<>'null'::jsonb AND p_scope->'hi'<>'null'::jsonb
    AND _oo.c5_overlay_key_compare(p_scope->'lo',p_scope->'hi')>=0 THEN
    RAISE EXCEPTION 'C5 inverted or zero-width range' USING ERRCODE='22023';
  END IF;
  SELECT count(*) INTO member_count FROM _oo.c5_member WHERE source=r.source_id
    AND schema_name=child.schema_ident AND table_name=child.table_ident AND chunk_id=chunk.chunk_id;
  IF member_count<>chunk.row_count OR chunk.row_count<0 OR parent_rows<chunk.row_count
    OR parent_rows<>(SELECT COALESCE(sum(row_count),0) FROM _oo.c5_chunk WHERE source=r.source_id
      AND schema_name=child.schema_ident AND table_name=child.table_ident AND retired_at IS NULL) THEN
    RAISE EXCEPTION 'C5 trusted chunk coverage differs' USING ERRCODE='OO030';
  END IF;
  -- The only row count on prepare is the new, empty heap, never the parent.
  EXECUTE format('SELECT count(*) FROM _oo.%I',child.physical_name) INTO member_count;
  IF member_count<>0 OR child.applied_rows<>0 OR child.exhausted THEN
    RAISE EXCEPTION 'C5 prepare requires empty shadow' USING ERRCODE='OO030';
  END IF;
  inherited:=parent_rows-chunk.row_count;
  IF root_name=f.physical_name THEN root_columns:=parent_cols; END IF;
  INSERT INTO _oo.c5_overlay(physical_name,run_id,scope,loader_keys,column_idents,key_idents,child_columns,parent_columns,
    parent_name,parent_oid,parent_generation,root_name,root_oid,root_generation,root_columns,
    delta_name,delta_oid,inherited_rows,replaced_rows,overlay_depth,prepared_xid)
    VALUES(child.physical_name,p_run,p_scope,ARRAY[]::text[],p_column_idents,p_key_idents,child_cols,parent_cols,
      f.physical_name,parent_oid,gen,root_name,root_oid,root_generation,root_columns,
      'pd_'||substr(child.physical_name,4),child_oid,inherited,chunk.row_count,1,pg_current_xact_id());
  INSERT INTO _oo.c5_overlay_component
    SELECT child.physical_name,c.chunk_id,c.delta_owner,c.exclusion_owner FROM _oo.c5_overlay_component c
      WHERE c.physical_name=f.physical_name AND c.chunk_id<>chunk.chunk_id;
  SELECT c.exclusion_owner INTO exclusion_owner FROM _oo.c5_overlay_component c
    WHERE c.physical_name=f.physical_name AND c.chunk_id=chunk.chunk_id;
  IF exclusion_owner IS NULL THEN
    exclusion_owner:=child.physical_name;
    EXECUTE format('INSERT INTO _oo.c5_overlay_exclusion
      SELECT $1,key_identity FROM _oo.c5_member WHERE source=$2
        AND schema_name=$3 AND table_name=$4 AND chunk_id=$5
      UNION SELECT $1,k FROM _oo.%I',capture.physical)
      USING child.physical_name,r.source_id,child.schema_ident,child.table_ident,chunk.chunk_id;
  END IF;
  INSERT INTO _oo.c5_overlay_component VALUES(child.physical_name,chunk.chunk_id,child.physical_name,exclusion_owner);
  -- Bound the number of stable chunks, not the number of updates/generations.
  IF (SELECT count(*) FROM _oo.c5_overlay_component WHERE physical_name=child.physical_name)>4096 THEN
    RAISE EXCEPTION 'C5 overlay component budget exhausted (4096)' USING ERRCODE='54001';
  END IF;
  UPDATE _oo.c5_parts_apply SET run_id=p_run WHERE source=capture.source AND task=capture.task;
  RETURN inherited;
END $$;

CREATE FUNCTION _oo.c5_parts_copy_page(p_shard text,p_owner uuid,p_fence bigint,p_run uuid,
 p_scope jsonb,p_after_key text,p_page_rows integer) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE a _oo.c5_parts_apply; c _oo.c5_parts_capture; o _oo.c5_overlay;
 after_bytes bytea; cols text; n bigint; last bytea;
BEGIN
 a:=_oo.c5_parts_apply_bound(p_shard,p_owner,p_fence,p_scope,p_run);
 SELECT * INTO STRICT c FROM _oo.c5_parts_capture WHERE source=a.source AND task=a.task;
 SELECT * INTO STRICT o FROM _oo.c5_overlay WHERE run_id=p_run;
 IF p_page_rows IS NULL OR p_page_rows NOT BETWEEN 1 AND 10000
  OR (p_after_key IS NOT NULL AND p_after_key !~ '^([0-9a-f]{2})+$')
  OR o.scope IS DISTINCT FROM p_scope OR o.view_oid IS NOT NULL
  OR o.prepared_xid<>pg_current_xact_id()
  OR to_regclass(format('_oo.%I',o.physical_name)) IS DISTINCT FROM o.delta_oid
  OR NOT EXISTS(SELECT FROM _oo.full_import_run WHERE run_id=p_run AND state='open'
    AND shard_id=p_shard AND owner_token=p_owner AND target_generation=p_fence)
 THEN RAISE EXCEPTION 'C5 parts copy page binding' USING ERRCODE='OO030'; END IF;
 after_bytes:=decode(p_after_key,'hex');
 IF a.previous_limit IS NOT NULL AND after_bytes IS NOT DISTINCT FROM a.previous_after THEN
  IF p_page_rows<>a.previous_limit THEN RAISE EXCEPTION 'C5 parts page replay changed' USING ERRCODE='OO010'; END IF;
  RETURN jsonb_build_object('copied_rows',a.previous_rows,'last_key',encode(a.last_key,'hex'));
 END IF;
 IF after_bytes IS DISTINCT FROM a.last_key
 THEN RAISE EXCEPTION 'C5 parts page cursor skips rows' USING ERRCODE='OO010'; END IF;
 SELECT string_agg(format('%I',col),',' ORDER BY ord) INTO cols
  FROM unnest(o.child_columns) WITH ORDINALITY v(col,ord);
 EXECUTE format('WITH page AS MATERIALIZED (
   SELECT * FROM _oo.%I %s ORDER BY k LIMIT $2),
   copied AS (INSERT INTO _oo.%I("_oo_pos",%s) SELECT 0,%s FROM page RETURNING 1)
   SELECT (SELECT count(*) FROM copied),(SELECT k FROM page ORDER BY k DESC LIMIT 1)',
   c.physical,CASE WHEN after_bytes IS NULL THEN '' ELSE 'WHERE k>$1' END,
   o.physical_name,cols,c.columns_sql) INTO n,last USING after_bytes,p_page_rows;
 IF a.copied_rows+n>c.expected_rows THEN RAISE EXCEPTION 'C5 parts copy overflow' USING ERRCODE='OO030'; END IF;
 last:=coalesce(last,a.last_key);
 UPDATE _oo.c5_parts_apply SET copied_rows=copied_rows+n,last_key=last,
  previous_after=after_bytes,previous_limit=p_page_rows,previous_rows=n WHERE source=a.source AND task=a.task;
 RETURN jsonb_build_object('copied_rows',n,'last_key',encode(last,'hex'));
END $$;

CREATE FUNCTION _oo.c5_parts_reconcile(p_shard text,p_owner uuid,p_fence bigint,p_run uuid,p_scope jsonb)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE a _oo.c5_parts_apply; c _oo.c5_parts_capture; o _oo.c5_overlay;
 keys text[]; expr text; dc text; sc text; n bigint; invalid bigint; unique_keys bigint; physical text;
BEGIN
 a:=_oo.c5_parts_apply_bound(p_shard,p_owner,p_fence,p_scope,p_run);
 SELECT * INTO STRICT c FROM _oo.c5_parts_capture WHERE source=a.source AND task=a.task;
 SELECT * INTO STRICT o FROM _oo.c5_overlay WHERE run_id=p_run;
 IF a.copied_rows<>c.expected_rows OR o.scope IS DISTINCT FROM p_scope
  OR o.prepared_xid<>pg_current_xact_id()
  OR NOT EXISTS(SELECT FROM _oo.full_import_run WHERE run_id=p_run AND state='open'
    AND shard_id=p_shard AND owner_token=p_owner AND target_generation=p_fence)
 THEN RAISE EXCEPTION 'C5 parts reconciliation binding' USING ERRCODE='OO030'; END IF;
 physical:=CASE WHEN o.view_oid IS NULL THEN o.physical_name ELSE o.delta_name END;
 IF to_regclass(format('_oo.%I',physical)) IS DISTINCT FROM o.delta_oid
 THEN RAISE EXCEPTION 'C5 parts delta relation changed' USING ERRCODE='OO030'; END IF;
 SELECT array_agg(o.child_columns[array_position(o.column_idents,k)] ORDER BY ord)
  INTO keys FROM unnest(o.key_idents) WITH ORDINALITY v(k,ord);
 expr:=_oo.c5_overlay_key_sql(o.delta_oid,keys,'d');
 SELECT string_agg(format('d.%I',col),',' ORDER BY ord),
   string_agg(format('s.%I','c'||(ord-1)),',' ORDER BY ord)
  INTO dc,sc FROM unnest(o.child_columns) WITH ORDINALITY v(col,ord);
 -- record_send retains float sign/NaN bits, numeric scale and NULL markers.
 -- Only the new heap and private stage participate; no old parent scan.
 EXECUTE format('SELECT count(*),count(DISTINCT d.k),
   count(*) FILTER(WHERE d.k IS NULL OR s.k IS NULL OR d.pos IS DISTINCT FROM 0
     OR d.cells IS DISTINCT FROM record_send(ROW(%s)))
   FROM (SELECT %s AS k,"_oo_pos" AS pos,record_send(ROW(%s)) AS cells FROM _oo.%I d) d
   FULL JOIN _oo.%I s ON s.k=d.k',sc,expr,dc,physical,c.physical) INTO n,unique_keys,invalid;
 IF n<>c.expected_rows OR unique_keys<>n OR invalid<>0
 THEN RAISE EXCEPTION 'C5 parts exact typed cells differ' USING ERRCODE='OO030'; END IF;
 RETURN n;
END $$;

-- Same 0075 flat indexed exclusion, then the 0068 visibility latch.
CREATE FUNCTION _oo.c5_parts_overlay_finish(p_shard text,p_owner uuid,p_fence bigint,p_run uuid,p_scope jsonb)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE definition text; gate_name text; o _oo.c5_overlay; r _oo.full_import_run; keys text[]; expression text;
  delta_count bigint; projection text:='p."_oo_pos"';
  delta_projection text:='d."_oo_pos"'; statement text:=''; i integer; view_id oid; component record; exclusions text[];
BEGIN
  PERFORM _oo.c5_parts_apply_bound(p_shard,p_owner,p_fence,p_scope,p_run);
  SELECT * INTO STRICT r FROM _oo.full_import_run WHERE run_id=p_run FOR UPDATE;
  SELECT * INTO STRICT o FROM _oo.c5_overlay WHERE run_id=p_run FOR UPDATE;
  IF r.state<>'open' OR r.shard_id IS DISTINCT FROM p_shard OR r.owner_token IS DISTINCT FROM p_owner
    OR r.target_generation IS DISTINCT FROM p_fence OR o.prepared_xid<>pg_current_xact_id()
    OR o.scope IS DISTINCT FROM p_scope THEN
    RAISE EXCEPTION 'C5 overlay finish binding' USING ERRCODE='OO030';
  END IF;
  IF o.view_oid IS NOT NULL THEN
    IF NOT o.visibility_gate OR NOT _oo.c5_overlay_relation_ok(o.physical_name) THEN
      RAISE EXCEPTION 'C5 overlay view changed' USING ERRCODE='OO030';
    END IF;
    RETURN o.inherited_rows+o.delta_rows;
  END IF;
  IF to_regclass(format('_oo.%I',o.physical_name)) IS DISTINCT FROM o.delta_oid THEN
    RAISE EXCEPTION 'C5 delta relation changed' USING ERRCODE='OO030';
  END IF;
  delta_count:=_oo.c5_parts_reconcile(p_shard,p_owner,p_fence,p_run,p_scope);
  IF o.root_name IS NOT NULL THEN
    IF to_regclass(format('_oo.%I',o.root_name)) IS DISTINCT FROM o.root_oid THEN
      RAISE EXCEPTION 'C5 root relation changed' USING ERRCODE='OO030';
    END IF;
    FOR i IN 1..cardinality(o.child_columns) LOOP
      projection:=projection||format(',p.%I AS %I',o.root_columns[i],o.child_columns[i]);
    END LOOP;
    SELECT array_agg(o.root_columns[array_position(o.column_idents,k)] ORDER BY ord)
      INTO keys FROM unnest(o.key_idents) WITH ORDINALITY x(k,ord);
    SELECT array_agg(c.exclusion_owner ORDER BY c.chunk_id COLLATE "C") INTO exclusions
      FROM _oo.c5_overlay_component c WHERE c.physical_name=o.physical_name;
    expression:=_oo.c5_overlay_key_sql(o.root_oid,keys,'p');
    statement:=format('SELECT %s FROM _oo.%I p WHERE NOT EXISTS
      (SELECT FROM _oo.c5_overlay_exclusion e WHERE e.physical_name=ANY(%L::text[]) AND e.key_identity=%s OFFSET 0)',
      projection,o.root_name,exclusions,expression);
  END IF;
  FOR component IN SELECT d.* FROM _oo.c5_overlay_component c JOIN _oo.c5_overlay d ON d.physical_name=c.delta_owner
    WHERE c.physical_name=o.physical_name ORDER BY c.chunk_id COLLATE "C" LOOP
    delta_projection:='d."_oo_pos"';
    FOR i IN 1..cardinality(o.child_columns) LOOP
      delta_projection:=delta_projection||format(',d.%I AS %I',component.child_columns[i],o.child_columns[i]);
    END LOOP;
    IF statement<>'' THEN statement:=statement||' UNION ALL '; END IF;
    statement:=statement||format('SELECT %s FROM _oo.%I d',delta_projection,component.delta_name);
  END LOOP;
  -- Even a single-component rootless view is intrinsically read-only.
  statement:=statement||format(' UNION ALL SELECT %s FROM _oo.%I d WHERE false',delta_projection,component.delta_name);
  -- ALTER fails if COPY's deferred fence triggers have not been flushed.
  -- Their original guards remain attached to pd_; no open run names that heap.
  EXECUTE format('ALTER TABLE _oo.%I RENAME TO %I',o.physical_name,o.delta_name);
  EXECUTE format('REVOKE ALL ON _oo.%I FROM PUBLIC,oo_pipeline_runtime',o.delta_name);
  EXECUTE format('CREATE VIEW _oo.%I AS %s',o.physical_name,statement);
  EXECUTE format('REVOKE ALL ON _oo.%I FROM PUBLIC,oo_pipeline_runtime',o.physical_name);
  view_id:=to_regclass(format('_oo.%I',o.physical_name));
  IF EXISTS(SELECT FROM pg_attribute d FULL JOIN pg_attribute v
      ON v.attrelid=view_id AND v.attnum=d.attnum AND v.attnum>0 AND NOT v.attisdropped
      WHERE d.attrelid=o.delta_oid AND d.attnum>0 AND NOT d.attisdropped
        AND (d.attname,d.atttypid,d.atttypmod,d.attcollation) IS DISTINCT FROM (v.attname,v.atttypid,v.atttypmod,v.attcollation)) THEN
    RAISE EXCEPTION 'C5 view changed strict schema types' USING ERRCODE='23514';
  END IF;
  UPDATE _oo.c5_overlay SET view_oid=view_id,view_definition=pg_get_viewdef(view_id,false),
    view_sha256=encode(sha256(convert_to(pg_get_viewdef(view_id,false),'UTF8')),'hex'),delta_rows=delta_count
    WHERE physical_name=o.physical_name;
  SELECT * INTO STRICT o FROM _oo.c5_overlay WHERE run_id=p_run;
    gate_name:='pv_'||substr(o.physical_name,4);
    -- This routine is only an ACL latch; EXECUTE never grants data or mutation.
    EXECUTE format('CREATE FUNCTION _oo.%I() RETURNS boolean LANGUAGE sql IMMUTABLE
      SET search_path=pg_catalog AS %L',gate_name,'SELECT false');
    EXECUTE format('REVOKE ALL ON FUNCTION _oo.%I() FROM PUBLIC,oo_pipeline_runtime',gate_name);
    definition:=regexp_replace(o.view_definition,';[[:space:]]*$','');
    EXECUTE format('CREATE OR REPLACE VIEW _oo.%I WITH (security_barrier=true) AS
      SELECT gated.* FROM (%s) gated
      WHERE _oo.c5_overlay_visible(%L::text,CURRENT_USER)',
      o.physical_name,definition,o.physical_name);
    UPDATE _oo.c5_overlay SET visibility_gate=true,
      visibility_gate_oid=to_regprocedure(format('_oo.%I()',gate_name)),
      view_definition=pg_get_viewdef(o.view_oid,false),
      view_sha256=encode(sha256(convert_to(pg_get_viewdef(o.view_oid,false),'UTF8')),'hex')
      WHERE physical_name=o.physical_name;
  RETURN o.inherited_rows+delta_count;
END $$;

CREATE FUNCTION _oo.c5_parts_commit(p_shard text,p_owner uuid,p_fence bigint,p_scope jsonb,
 p_run uuid,p_batch text,p_payload text) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE c _oo.c5_parts_capture; a _oo.c5_parts_apply; o _oo.c5_overlay;
 state _oo.c5_table; ch _oo.c5_chunk; f _oo.table_freshness; high bigint;
 receipt _oo.c5_block_receipt; scan _oo.c5_scan; n bigint; typed text; held record;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 c:=_oo.c5_parts_stage_bound(p_scope);
 SELECT * INTO STRICT a FROM _oo.c5_parts_apply WHERE source=c.source AND task=c.task FOR UPDATE;
 PERFORM _oo.c5_parts_live(c.source,c.task,a.job_generation);
 SELECT * INTO STRICT o FROM _oo.c5_overlay WHERE run_id=p_run;
 SELECT * INTO receipt FROM _oo.c5_block_receipt WHERE source=c.source
  AND schema_name=p_scope->>'schema' AND table_name=p_scope->>'table'
  AND chunk_id=p_scope->>'chunk_id' AND source_generation=(p_scope->>'source_generation')::bigint;
 IF FOUND THEN
  IF (receipt.scope,receipt.run_id,receipt.batch_id,receipt.payload_digest) IS DISTINCT FROM
     (p_scope,p_run,p_batch,p_payload) OR a.scope IS DISTINCT FROM p_scope OR a.run_id IS DISTINCT FROM p_run
    OR NOT _oo.c5_overlay_is_valid(o.physical_name)
    OR (SELECT count(*) FROM _oo.update_tombstone WHERE source=c.source
      AND schema_name=p_scope->>'schema' AND table_name=p_scope->>'table'
      AND source_generation=receipt.source_generation AND batch_id=p_batch)<>a.deleted_rows
    OR NOT EXISTS(SELECT FROM _oo.update_batch WHERE run_id=p_run AND batch_id=p_batch
      AND payload_digest=p_payload AND tombstone_count=a.deleted_rows)
  THEN RAISE EXCEPTION 'C5 parts receipt replay differs' USING ERRCODE='OO010'; END IF;
  RETURN a.deleted_rows;
 END IF;
 a:=_oo.c5_parts_apply_bound(p_shard,p_owner,p_fence,p_scope,p_run);
 SELECT source_generation INTO STRICT high FROM _oo.source_watermark WHERE source=c.source FOR UPDATE;
 SELECT * INTO STRICT state FROM _oo.c5_table
  WHERE source=c.source AND schema_name=p_scope->>'schema' AND table_name=p_scope->>'table' FOR UPDATE;
 SELECT * INTO STRICT ch FROM _oo.c5_chunk WHERE source=c.source AND schema_name=state.schema_name
  AND table_name=state.table_name AND chunk_id=p_scope->>'chunk_id' AND retired_at IS NULL FOR UPDATE;
 SELECT * INTO STRICT f FROM _oo.table_freshness
  WHERE source=c.source AND schema_name=state.schema_name AND table_name=state.table_name FOR UPDATE;
 IF (state.source_generation,state.input_digest) IS DISTINCT FROM (a.parent_generation,a.parent_digest)
  OR o.parent_generation IS DISTINCT FROM a.parent_generation OR o.parent_name IS DISTINCT FROM a.parent_physical
  OR o.scope IS DISTINCT FROM p_scope OR o.prepared_xid<>pg_current_xact_id()
  OR p_scope->'source_generation' IS DISTINCT FROM to_jsonb(high)
  OR f.source_generation<>high OR f.target_generation<>p_fence OR f.physical_name<>o.physical_name
  OR f.state<>'awaiting_publish' OR f.mode<>'incremental'
  OR state.source_generation>(p_scope->>'parent_generation')::bigint
  OR state.schema_fingerprint IS DISTINCT FROM p_scope->>'schema_fingerprint'
  OR p_scope->'chunk_topology_revision' IS DISTINCT FROM to_jsonb(state.topology_revision)
  OR ch.topology_revision<>state.topology_revision
  OR p_scope->'prior_membership_revision' IS DISTINCT FROM to_jsonb(ch.membership_revision)
  OR (ch.lo,ch.hi) IS DISTINCT FROM (p_scope->'lo',p_scope->'hi')
  OR EXISTS(SELECT FROM _oo.c5_chunk WHERE source=c.source AND schema_name=state.schema_name
    AND table_name=state.table_name AND retired_at IS NULL AND baseline_membership_pending)
  OR a.copied_rows<>c.expected_rows OR o.delta_rows IS DISTINCT FROM c.expected_rows
  OR NOT o.visibility_gate OR NOT _oo.c5_overlay_relation_ok(o.physical_name)
 THEN RAISE EXCEPTION 'C5 parts sealed parent CAS differs' USING ERRCODE='OO010'; END IF;
 IF NOT EXISTS(SELECT FROM _oo.full_import_run r JOIN _oo.full_import_table ft USING(run_id)
   JOIN _oo.update_batch b ON b.run_id=r.run_id AND b.physical_name=ft.physical_name
   JOIN _oo.apply_receipts ar ON ar.source=b.source AND ar.table_generation=b.physical_name
     AND ar.txn_or_batch_id=b.batch_id AND ar.payload_digest=b.payload_digest
   JOIN _oo.full_import_quality q ON q.run_id=r.run_id AND q.physical_name=ft.physical_name
     AND q.batch_id=b.batch_id AND q.kind='batch'
   WHERE r.run_id=p_run AND r.state='awaiting_publish' AND r.sealed_at IS NOT NULL AND r.retired_at IS NULL
    AND r.source_id=c.source AND r.source_generation=high AND r.shard_id=p_shard
    AND r.owner_token=p_owner AND r.target_generation=p_fence AND r.input_digest=p_scope->>'arrow_sha256'
    AND ft.schema_ident=state.schema_name AND ft.table_ident=state.table_name
    AND ft.physical_name=o.physical_name AND ft.exhausted AND ft.canonical_digest IS NOT NULL
    AND ft.applied_pos=0 AND ft.applied_rows=o.inherited_rows+o.delta_rows
    AND b.source=c.source AND b.schema_name=state.schema_name AND b.table_name=state.table_name
    AND b.batch_id=p_batch AND b.payload_digest=p_payload AND b.source_generation=high
    AND b.target_generation=p_fence AND b.row_count=ft.applied_rows AND b.tombstone_count=0
    AND ar.config_digest=r.config_digest AND ar.type_digest=r.type_digest AND ar.schema_digest=r.schema_digest
    AND ar.fence_shard=p_shard AND ar.fence_generation=p_fence
    AND ar.position_codec='snapshot-zero/v1' AND ar.applied_pos=0
    AND ar.txn_complete AND NOT ar.partial_txn AND NOT ar.cross_table_consistent
    AND q.input_digest=r.input_digest AND q.payload_digest=p_payload
    AND q.accepted_rows=ft.applied_rows AND q.rejected_rows=0)
  OR (SELECT count(*) FROM _oo.update_batch WHERE run_id=p_run)<>1
  OR EXISTS(SELECT FROM _oo.full_import_quality WHERE run_id=p_run AND kind='reject')
  OR EXISTS(SELECT FROM _oo.update_tombstone WHERE source=c.source
    AND schema_name=state.schema_name AND table_name=state.table_name AND source_generation=high)
 THEN RAISE EXCEPTION 'C5 parts real sealed Loader receipts required' USING ERRCODE='OO030'; END IF;
 -- Anti-join the changed chunk only; preserve every deletion without arrays.
 EXECUTE format('INSERT INTO _oo.update_tombstone(source,schema_name,table_name,source_generation,key_text,batch_id)
   SELECT m.source,m.schema_name,m.table_name,$5,m.loader_key,$6 FROM _oo.c5_member m
   WHERE m.source=$1 AND m.schema_name=$2 AND m.table_name=$3 AND m.chunk_id=$4
    AND NOT EXISTS(SELECT FROM _oo.%I x WHERE x.k=m.key_identity)',c.physical)
  USING c.source,state.schema_name,state.table_name,ch.chunk_id,high,p_batch;
 GET DIAGNOSTICS n=ROW_COUNT;
 IF n<>a.deleted_rows THEN RAISE EXCEPTION 'C5 parts deleted membership changed' USING ERRCODE='OO010'; END IF;
 UPDATE _oo.update_batch SET tombstone_count=n WHERE run_id=p_run AND batch_id=p_batch;
 DELETE FROM _oo.c5_member WHERE source=c.source AND schema_name=state.schema_name
  AND table_name=state.table_name AND chunk_id=ch.chunk_id;
 typed:=_oo.c5_parts_typed_sql(c.spec,'x');
 EXECUTE format('INSERT INTO _oo.c5_member(source,schema_name,table_name,chunk_id,key_identity,typed_key,loader_key)
   SELECT $1,$2,$3,$4,x.k,%s,_oo.c5_overlay_loader_key(%s) FROM _oo.%I x',typed,typed,c.physical)
  USING c.source,state.schema_name,state.table_name,ch.chunk_id;
 GET DIAGNOSTICS n=ROW_COUNT;
 IF n<>c.expected_rows THEN RAISE EXCEPTION 'C5 parts committed membership count' USING ERRCODE='OO030'; END IF;
 INSERT INTO _oo.c5_block_receipt VALUES(c.source,state.schema_name,state.table_name,ch.chunk_id,high,
   p_scope,p_run,p_batch,p_payload,p_fence,clock_timestamp());
 SELECT * INTO scan FROM _oo.c5_scan WHERE source=c.source AND schema_name=state.schema_name
  AND table_name=state.table_name AND status='running' FOR UPDATE;
 UPDATE _oo.c5_chunk SET membership_revision=membership_revision+1,row_count=c.expected_rows,
  arrow_sha256=p_scope->>'arrow_sha256',last_scope=p_scope,scanned_at=clock_timestamp(),last_scan_id=scan.scan_id
  WHERE source=c.source AND schema_name=state.schema_name AND table_name=state.table_name AND chunk_id=ch.chunk_id;
 UPDATE _oo.c5_table SET source_generation=high,input_digest=p_scope->>'arrow_sha256'
  WHERE source=c.source AND schema_name=state.schema_name AND table_name=state.table_name;
 IF scan.scan_id IS NOT NULL THEN
  UPDATE _oo.c5_scan SET next_block=coalesce((SELECT min(k.ordinality)-1
    FROM jsonb_array_elements_text(scan.expected_chunks) WITH ORDINALITY k
    LEFT JOIN _oo.c5_chunk chunk ON chunk.source=c.source AND chunk.schema_name=state.schema_name
      AND chunk.table_name=state.table_name AND chunk.chunk_id=k.value
    WHERE chunk.last_scan_id IS DISTINCT FROM scan.scan_id),jsonb_array_length(scan.expected_chunks))
   WHERE source=c.source AND schema_name=state.schema_name AND table_name=state.table_name AND scan_id=scan.scan_id;
 END IF;
 UPDATE _oo.table_quality SET c5_mode='layered' WHERE schema_name=state.schema_name AND table_name=state.table_name;
 IF NOT _oo.c5_overlay_is_valid(o.physical_name)
 THEN RAISE EXCEPTION 'C5 parts generic overlay receipt invalid' USING ERRCODE='OO030'; END IF;
 -- Lock only at the end, after unbounded work. Heartbeats remain possible
 -- during staging, paging, reconciliation and membership insertion.
 SELECT * INTO held FROM jobs.loader_hold(c.task,a.job_generation);
 IF held.state IS DISTINCT FROM 'running' OR held.live IS DISTINCT FROM true
 THEN RAISE EXCEPTION 'C5 parts final live lease required' USING ERRCODE='OO010'; END IF;
 PERFORM _oo.c5_parts_live(c.source,c.task,a.job_generation);
 RETURN a.deleted_rows;
END $$;

-- Recover COMMIT-before-ACK under a new real claim of the SAME task.
-- This is not private-stage/unfinished-apply adoption and never rewrites data.
CREATE FUNCTION _oo.c5_parts_rebind(p_source text,p_task uuid,p_generation bigint) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE staged _oo.c5_parts_apply; capture _oo.c5_parts_capture; held record;
BEGIN
 PERFORM _oo.c5_parts_live(p_source,p_task,p_generation);
 SELECT * INTO STRICT staged FROM _oo.c5_parts_apply WHERE source=p_source AND task=p_task FOR UPDATE;
 capture:=_oo.c5_parts_stage_bound(staged.scope);
 IF p_generation<staged.job_generation OR staged.run_id IS NULL OR NOT EXISTS(
  SELECT FROM _oo.c5_overlay o JOIN _oo.full_import_run r USING(run_id)
   JOIN _oo.full_import_table ft USING(run_id)
   JOIN _oo.c5_block_receipt b ON b.run_id=r.run_id AND b.scope=o.scope
   JOIN _oo.update_batch batch ON batch.run_id=r.run_id AND batch.source=b.source
    AND batch.schema_name=b.schema_name AND batch.table_name=b.table_name
    AND batch.batch_id=b.batch_id AND batch.payload_digest=b.payload_digest
   JOIN _oo.apply_receipts ar ON ar.source=batch.source AND ar.table_generation=batch.physical_name
    AND ar.txn_or_batch_id=batch.batch_id AND ar.payload_digest=batch.payload_digest
  WHERE r.run_id=staged.run_id AND r.source_id=p_source AND r.shard_id=staged.shard
   AND r.owner_token=staged.owner AND r.target_generation=staged.fence
   AND r.source_generation=(staged.scope->>'source_generation')::bigint
   AND r.input_digest=staged.scope->>'arrow_sha256' AND r.sealed_at IS NOT NULL AND r.retired_at IS NULL
   AND r.state IN ('awaiting_publish','published') AND ft.exhausted AND ft.physical_name=o.physical_name
   AND ft.schema_ident=staged.scope->>'schema' AND ft.table_ident=staged.scope->>'table'
   AND o.scope=staged.scope AND o.delta_rows=capture.expected_rows AND staged.copied_rows=capture.expected_rows
   AND b.source=p_source AND b.chunk_id=staged.scope->>'chunk_id'
   AND b.source_generation=r.source_generation AND b.target_generation=r.target_generation
   AND batch.physical_name=o.physical_name AND batch.row_count=o.inherited_rows+o.delta_rows
   AND batch.tombstone_count=staged.deleted_rows
   AND ar.config_digest=r.config_digest AND ar.type_digest=r.type_digest AND ar.schema_digest=r.schema_digest
   AND ar.fence_shard=r.shard_id AND ar.fence_generation=r.target_generation
   AND ar.position_codec='snapshot-zero/v1' AND ar.applied_pos=0
   AND ar.txn_complete AND NOT ar.partial_txn AND NOT ar.cross_table_consistent
   AND _oo.c5_overlay_is_valid(o.physical_name))
 THEN RAISE EXCEPTION 'C5 parts rebind requires committed exact receipts' USING ERRCODE='OO030'; END IF;
 SELECT * INTO held FROM jobs.loader_hold(p_task,p_generation);
 IF held.state IS DISTINCT FROM 'running' OR held.live IS DISTINCT FROM true
 THEN RAISE EXCEPTION 'C5 parts rebind live lease required' USING ERRCODE='OO010'; END IF;
 PERFORM _oo.c5_parts_live(p_source,p_task,p_generation);
 UPDATE _oo.c5_parts_apply SET job_generation=p_generation WHERE source=p_source AND task=p_task;
 RETURN staged.deleted_rows;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_parts_rebind(text,uuid,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_parts_rebind(text,uuid,bigint) TO oo_pipeline_runtime;

-- Observation is a later transaction, not a second apply. Reuse 0065's
-- protocol-agnostic stored receipt join, then require the private parts origin.
CREATE FUNCTION _oo.c5_parts_observation_receipt_guard(p_shard text,p_owner uuid,p_fence bigint,
 p_scope jsonb,p_receipt jsonb) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE c _oo.c5_parts_capture; staged _oo.c5_parts_apply;
BEGIN
 PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
 c:=_oo.c5_parts_stage_bound(p_scope);
 SELECT * INTO STRICT staged FROM _oo.c5_parts_apply WHERE source=c.source AND task=c.task;
 PERFORM _oo.c5_parts_live(c.source,c.task,staged.job_generation);
 IF staged.scope IS DISTINCT FROM p_scope OR staged.shard IS DISTINCT FROM p_shard
  OR jsonb_typeof(p_receipt) IS DISTINCT FROM 'object'
  OR p_receipt->'jobs_lease_receipt'->>'id' IS DISTINCT FROM c.task::text
  OR p_receipt->'jobs_lease_receipt'->'generation' IS DISTINCT FROM to_jsonb(staged.job_generation)
  OR p_receipt->>'run_id' IS DISTINCT FROM staged.run_id::text
  OR coalesce(p_receipt->>'receipt','') NOT IN ('inserted','replay')
 THEN RAISE EXCEPTION 'C5 parts observation lease or scope differs' USING ERRCODE='OO030'; END IF;
 PERFORM _oo.c5_observation_receipt_guard(p_shard,p_owner,p_fence,p_scope,p_receipt);
 IF NOT EXISTS(
  SELECT FROM _oo.c5_overlay o JOIN _oo.c5_block_receipt block USING(run_id)
   JOIN _oo.update_batch batch ON batch.run_id=block.run_id AND batch.source=block.source
    AND batch.schema_name=block.schema_name AND batch.table_name=block.table_name
    AND batch.batch_id=block.batch_id AND batch.payload_digest=block.payload_digest
   JOIN _oo.c5_chunk ch ON ch.source=block.source AND ch.schema_name=block.schema_name
    AND ch.table_name=block.table_name AND ch.chunk_id=block.chunk_id
   JOIN _oo.apply_receipts ar ON ar.source=batch.source AND ar.table_generation=batch.physical_name
    AND ar.txn_or_batch_id=batch.batch_id AND ar.payload_digest=batch.payload_digest
  WHERE o.run_id=staged.run_id AND o.scope=p_scope AND block.scope=p_scope
   AND o.delta_rows=c.expected_rows AND staged.copied_rows=c.expected_rows
   AND batch.physical_name=o.physical_name AND batch.row_count=o.inherited_rows+o.delta_rows
   AND batch.tombstone_count=staged.deleted_rows
   AND p_receipt->'row_count'=to_jsonb(batch.row_count)
   AND p_receipt->'tombstone_count'=to_jsonb(batch.tombstone_count)
   AND ch.retired_at IS NULL AND NOT ch.baseline_membership_pending
   AND ch.last_scope=p_scope AND ch.membership_revision=(p_scope->>'prior_membership_revision')::bigint+1
   AND ch.row_count=c.expected_rows AND ch.arrow_sha256=p_scope->>'arrow_sha256'
   AND ar.fence_shard=staged.shard AND ar.fence_generation=block.target_generation
   AND ar.position_codec='snapshot-zero/v1' AND ar.applied_pos=0
   AND ar.txn_complete AND NOT ar.partial_txn AND NOT ar.cross_table_consistent
   AND _oo.c5_overlay_is_valid(o.physical_name))
 THEN RAISE EXCEPTION 'C5 parts observation stored receipts differ' USING ERRCODE='OO030'; END IF;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_parts_observation_receipt_guard(text,uuid,bigint,jsonb,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_parts_observation_receipt_guard(text,uuid,bigint,jsonb,jsonb) TO oo_pipeline_runtime;

REVOKE ALL ON FUNCTION _oo.c5_parts_typed_sql(jsonb,text),_oo.c5_parts_stage_bound(jsonb),
 _oo.c5_parts_identity_check(text,uuid,bigint),
 _oo.c5_parts_apply_bound(text,uuid,bigint,jsonb,uuid) FROM PUBLIC,oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.c5_parts_guard(text,uuid,bigint,jsonb),
 _oo.c5_parts_overlay_prepare(text,uuid,bigint,uuid,jsonb,text[],text[]),
 _oo.c5_parts_copy_page(text,uuid,bigint,uuid,jsonb,text,integer),
 _oo.c5_parts_reconcile(text,uuid,bigint,uuid,jsonb),
 _oo.c5_parts_overlay_finish(text,uuid,bigint,uuid,jsonb),
 _oo.c5_parts_commit(text,uuid,bigint,jsonb,uuid,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_parts_guard(text,uuid,bigint,jsonb),
 _oo.c5_parts_overlay_prepare(text,uuid,bigint,uuid,jsonb,text[],text[]),
 _oo.c5_parts_copy_page(text,uuid,bigint,uuid,jsonb,text,integer),
 _oo.c5_parts_reconcile(text,uuid,bigint,uuid,jsonb),
 _oo.c5_parts_overlay_finish(text,uuid,bigint,uuid,jsonb),
 _oo.c5_parts_commit(text,uuid,bigint,jsonb,uuid,text,text) TO oo_pipeline_runtime;
