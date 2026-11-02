-- native-c5-overlay/v1
-- prepare_shadow -> overlay_prepare -> COPY -> SET CONSTRAINTS ALL IMMEDIATE
-- -> overlay_finish -> count/quality/receipt -> seal -> range_commit, in ONE tx.
-- Only changed-range rows are stored. No parent SELECT/count/INSERT in prepare
-- or finish. Publication and catalogue view admission are separate migrations.

CREATE TABLE _oo.c5_overlay (
  physical_name text PRIMARY KEY CHECK (physical_name ~ '^pt_[0-9a-f]{60}$'),
  run_id uuid NOT NULL UNIQUE REFERENCES _oo.full_import_run,
  scope jsonb NOT NULL,
  loader_keys text[] NOT NULL,
  column_idents text[] NOT NULL,
  key_idents text[] NOT NULL,
  child_columns text[] NOT NULL,
  parent_columns text[] NOT NULL,
  parent_name text,
  parent_oid oid,
  parent_generation bigint NOT NULL,
  root_name text,
  root_oid oid,
  root_generation bigint NOT NULL DEFAULT 0,
  root_columns text[] NOT NULL DEFAULT ARRAY[]::text[],
  delta_name text NOT NULL UNIQUE CHECK (delta_name ~ '^pd_[0-9a-f]{60}$'),
  delta_oid oid NOT NULL UNIQUE,
  view_oid oid UNIQUE,
  view_definition text,
  view_sha256 text,
  inherited_rows bigint NOT NULL CHECK (inherited_rows >= 0),
  replaced_rows bigint NOT NULL CHECK (replaced_rows >= 0),
  delta_rows bigint CHECK (delta_rows >= 0),
  overlay_depth integer NOT NULL CHECK (overlay_depth = 1),
  prepared_xid xid8 NOT NULL,
  CHECK ((view_oid IS NULL AND view_definition IS NULL AND view_sha256 IS NULL AND delta_rows IS NULL)
      OR (view_oid IS NOT NULL AND view_definition IS NOT NULL AND view_sha256 ~ '^[0-9a-f]{64}$' AND delta_rows IS NOT NULL)),
  CHECK ((parent_name IS NULL AND parent_oid IS NULL AND parent_generation = 0)
      OR (parent_name ~ '^pt_[0-9a-f]{60}$' AND parent_oid IS NOT NULL AND parent_generation > 0))
);
CREATE TABLE _oo.c5_overlay_exclusion (
  physical_name text NOT NULL REFERENCES _oo.c5_overlay,
  key_identity bytea NOT NULL,
  PRIMARY KEY (physical_name, key_identity)
);
CREATE TABLE _oo.c5_overlay_component (
  physical_name text NOT NULL REFERENCES _oo.c5_overlay,
  chunk_id text NOT NULL,
  delta_owner text NOT NULL REFERENCES _oo.c5_overlay,
  exclusion_owner text NOT NULL REFERENCES _oo.c5_overlay,
  PRIMARY KEY(physical_name,chunk_id)
);
REVOKE ALL ON _oo.c5_overlay_component FROM PUBLIC,oo_pipeline_runtime;
CREATE INDEX c5_member_chunk_lookup ON _oo.c5_member(source,schema_name,table_name,chunk_id);
REVOKE ALL ON _oo.c5_overlay, _oo.c5_overlay_exclusion FROM PUBLIC, oo_pipeline_runtime;

-- Older single-full initialization did not persist which columns defined its
-- membership. Never infer that authority from caller-supplied column names.
CREATE TABLE _oo.c5_overlay_baseline_key (
  source text NOT NULL, schema_name text NOT NULL, table_name text NOT NULL,
  physical_name text NOT NULL, source_generation bigint NOT NULL, key_idents text[] NOT NULL,
  PRIMARY KEY(source,schema_name,table_name)
);
REVOKE ALL ON _oo.c5_overlay_baseline_key FROM PUBLIC,oo_pipeline_runtime;
ALTER FUNCTION _oo.c5_initialize_full(text,uuid,bigint,text,text,text,text,bigint,text,text[])
  RENAME TO c5_initialize_full_before_overlay;
REVOKE ALL ON FUNCTION _oo.c5_initialize_full_before_overlay(text,uuid,bigint,text,text,text,text,bigint,text,text[])
  FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_initialize_full(p_shard text,p_owner uuid,p_fence bigint,p_source text,
  p_schema text,p_table text,p_fingerprint text,p_topology bigint,p_chunk text,p_keys text[])
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo SET bytea_output='hex' AS $$
BEGIN
  PERFORM _oo.c5_initialize_full_before_overlay(p_shard,p_owner,p_fence,p_source,p_schema,p_table,
    p_fingerprint,p_topology,p_chunk,p_keys);
  INSERT INTO _oo.c5_overlay_baseline_key
    SELECT p_source,p_schema,p_table,physical_name,source_generation,p_keys FROM _oo.table_freshness
      WHERE source=p_source AND schema_name=p_schema AND table_name=p_table
    ON CONFLICT(source,schema_name,table_name) DO UPDATE
      SET physical_name=EXCLUDED.physical_name,source_generation=EXCLUDED.source_generation,key_idents=EXCLUDED.key_idents;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_initialize_full(text,uuid,bigint,text,text,text,text,bigint,text,text[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_initialize_full(text,uuid,bigint,text,text,text,text,bigint,text,text[]) TO oo_pipeline_runtime;

-- Not a general SQL evaluator: identifiers below come only from stored mappings
-- and pg_attribute. The resulting view uses builtins, not private functions
-- whose EXECUTE privileges would leak into publication.
CREATE FUNCTION _oo.c5_overlay_key_sql(p_relation oid, p_columns text[], p_alias text)
RETURNS text LANGUAGE plpgsql STABLE SET search_path=pg_catalog,_oo AS $$
DECLARE col text; kind oid; piece text; result text := '';
BEGIN
  IF cardinality(p_columns) NOT BETWEEN 1 AND 64 OR p_columns IS NULL
    OR array_position(p_columns,NULL) IS NOT NULL THEN
    RAISE EXCEPTION 'C5 key columns' USING ERRCODE='22023';
  END IF;
  FOREACH col IN ARRAY p_columns LOOP
    SELECT atttypid INTO STRICT kind FROM pg_attribute
      WHERE attrelid=p_relation AND attname=col AND attnum>0 AND NOT attisdropped;
    IF kind IN (20,21,23) THEN
      piece := format('decode(''00'',''hex'') || int8send(%I.%I::bigint)',p_alias,col);
    ELSIF kind IN (25,1043) THEN
      piece := format('decode(''01'',''hex'') || convert_to(%I.%I::text,''UTF8'')',p_alias,col);
    ELSIF kind=17 THEN
      piece := format('decode(''02'',''hex'') || %I.%I',p_alias,col);
    ELSE
      -- bpchar trims trailing spaces on ::text; numeric/date keys need a
      -- different typed range protocol. Refuse rather than alias identities.
      RAISE EXCEPTION 'C5 overlay unsupported key type %',kind USING ERRCODE='22023';
    END IF;
    IF result<>'' THEN result:=result||' || '; END IF;
    result:=result||format('(int4send(octet_length(%s)) || (%s))',piece,piece);
  END LOOP;
  RETURN '('||result||')';
END $$;

CREATE FUNCTION _oo.c5_overlay_loader_key(p_key jsonb)
RETURNS text LANGUAGE plpgsql IMMUTABLE STRICT SET search_path=pg_catalog,_oo AS $$
DECLARE v jsonb; piece text; result text := ''; i integer := 0;
BEGIN
  PERFORM _oo.c5_key_identity(p_key);
  FOR v IN SELECT value FROM jsonb_array_elements(p_key) LOOP
    IF v->>'kind'='int' THEN piece:=((v->>'value')::bigint)::text;
    ELSIF v->>'kind'='text' THEN
      piece:=convert_from(decode(v->>'value','hex'),'UTF8');
      IF piece='' THEN RAISE EXCEPTION 'C5 empty loader text key' USING ERRCODE='22023'; END IF;
    ELSE piece:=chr(92)||'x'||(v->>'value');
    END IF;
    IF jsonb_array_length(p_key)>1 AND strpos(piece,chr(31))>0 THEN
      RAISE EXCEPTION 'C5 composite key contains loader separator' USING ERRCODE='22023';
    END IF;
    IF i>0 THEN result:=result||chr(31); END IF;
    result:=result||CASE WHEN jsonb_array_length(p_key)=1 THEN piece
      ELSE octet_length(convert_to(piece,'UTF8'))::text||':'||piece END;
    i:=i+1;
  END LOOP;
  RETURN result;
END $$;

CREATE FUNCTION _oo.c5_overlay_key_compare(p_left jsonb,p_right jsonb)
RETURNS integer LANGUAGE plpgsql IMMUTABLE STRICT SET search_path=pg_catalog,_oo AS $$
DECLARE i integer; l jsonb; r jsonb; cmp integer;
BEGIN
  PERFORM _oo.c5_key_identity(p_left); PERFORM _oo.c5_key_identity(p_right);
  IF jsonb_array_length(p_left)<>jsonb_array_length(p_right) THEN
    RAISE EXCEPTION 'C5 range key width' USING ERRCODE='22023';
  END IF;
  FOR i IN 0..jsonb_array_length(p_left)-1 LOOP
    l:=p_left->i; r:=p_right->i;
    IF l->>'kind'<>r->>'kind' THEN RAISE EXCEPTION 'C5 range key kind' USING ERRCODE='22023'; END IF;
    IF l->>'kind'='int' THEN
      cmp:=CASE WHEN (l->>'value')::bigint<(r->>'value')::bigint THEN -1
        WHEN (l->>'value')::bigint>(r->>'value')::bigint THEN 1 ELSE 0 END;
    ELSIF l->>'kind'='bytes' THEN
      cmp:=CASE WHEN decode(l->>'value','hex')<decode(r->>'value','hex') THEN -1
        WHEN decode(l->>'value','hex')>decode(r->>'value','hex') THEN 1 ELSE 0 END;
    ELSE
      RAISE EXCEPTION 'C5 text order belongs to the admitted MySQL source, not PostgreSQL' USING ERRCODE='22023';
    END IF;
    IF cmp<>0 THEN RETURN cmp; END IF;
  END LOOP;
  RETURN 0;
END $$;

CREATE FUNCTION _oo.c5_overlay_relation_ok(p_physical text)
RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
  SELECT EXISTS (
    SELECT FROM _oo.c5_overlay o
    JOIN pg_class v ON v.oid=o.view_oid AND v.relname=o.physical_name
    JOIN pg_namespace n ON n.oid=v.relnamespace AND n.nspname='_oo'
    JOIN pg_class d ON d.oid=o.delta_oid AND d.relname=o.delta_name AND d.relnamespace=n.oid
    JOIN _oo.full_import_table t ON t.run_id=o.run_id AND t.physical_name=o.physical_name
    WHERE o.physical_name=p_physical AND o.delta_rows IS NOT NULL
      AND v.relkind='v' AND d.relkind='r' AND d.relpersistence='p'
      AND v.relowner='oo_target_owner'::regrole AND d.relowner=v.relowner
      AND pg_relation_is_updatable(v.oid,false)=0
      AND NOT EXISTS(SELECT FROM pg_trigger WHERE tgrelid=v.oid AND NOT tgisinternal)
      AND NOT EXISTS(SELECT FROM pg_rewrite WHERE ev_class=v.oid AND rulename<>'_RETURN')
      AND pg_get_viewdef(v.oid,false)=o.view_definition
      AND encode(sha256(convert_to(pg_get_viewdef(v.oid,false),'UTF8')),'hex')=o.view_sha256
      AND NOT EXISTS(SELECT FROM pg_attribute da LEFT JOIN pg_attribute va
        ON va.attrelid=v.oid AND va.attnum=da.attnum AND NOT va.attisdropped
        WHERE da.attrelid=d.oid AND da.attnum>0 AND NOT da.attisdropped
          AND (da.attname,da.atttypid,da.atttypmod,da.attcollation)
            IS DISTINCT FROM (va.attname,va.atttypid,va.atttypmod,va.attcollation))
  );
$$;

-- Receipt validation is nonrecursive. A component uses only its private heap;
-- it never reads its historical public view or that view's other components.
CREATE FUNCTION _oo.c5_overlay_receipted(p_physical text)
RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
  SELECT EXISTS(SELECT FROM _oo.c5_overlay o
    JOIN pg_class d ON d.oid=o.delta_oid AND d.relname=o.delta_name AND d.relkind='r' AND d.relpersistence='p'
    JOIN pg_namespace ns ON ns.oid=d.relnamespace AND ns.nspname='_oo'
    JOIN _oo.full_import_run r ON r.run_id=o.run_id
    JOIN _oo.full_import_table t ON t.run_id=r.run_id AND t.physical_name=o.physical_name
    JOIN _oo.update_batch b ON b.run_id=t.run_id AND b.physical_name=t.physical_name
    JOIN _oo.apply_receipts a ON a.source=b.source AND a.table_generation=b.physical_name
      AND a.txn_or_batch_id=b.batch_id AND a.payload_digest=b.payload_digest
    JOIN _oo.c5_block_receipt c ON c.run_id=t.run_id AND c.source=b.source
      AND c.schema_name=t.schema_ident AND c.table_name=t.table_ident AND c.scope=o.scope
      AND c.batch_id=b.batch_id AND c.payload_digest=b.payload_digest
    WHERE o.physical_name=p_physical AND d.relowner='oo_target_owner'::regrole
      AND o.view_oid IS NOT NULL AND o.delta_rows IS NOT NULL
      AND (SELECT count(*) FROM pg_trigger tr WHERE tr.tgrelid=d.oid AND tr.tgenabled='O'
        AND tr.tgfoid='_oo.full_import_guard_shadow()'::regprocedure
        AND tr.tgname IN ('full_import_shadow_insert','full_import_shadow_mutate',
          'full_import_shadow_truncate','full_import_shadow_commit'))=4
      AND r.state IN ('awaiting_publish','published') AND r.sealed_at IS NOT NULL
      AND t.exhausted AND t.canonical_digest IS NOT NULL
      AND t.applied_rows=o.inherited_rows+o.delta_rows AND b.row_count=t.applied_rows
      AND b.source=r.source_id AND b.source_generation=r.source_generation AND b.target_generation=r.target_generation
      AND b.schema_name=t.schema_ident AND b.table_name=t.table_ident
      AND c.chunk_id=o.scope->>'chunk_id' AND c.source_generation=r.source_generation AND c.target_generation=r.target_generation
      AND a.config_digest=r.config_digest AND a.type_digest=r.type_digest AND a.schema_digest=r.schema_digest
      AND a.fence_shard=r.shard_id AND a.fence_generation=r.target_generation
      AND r.input_digest=o.scope->>'arrow_sha256'
      AND r.source_id=o.scope->>'source_id' AND r.source_generation=(o.scope->>'source_generation')::bigint
      AND t.schema_ident=o.scope->>'schema' AND t.table_ident=o.scope->>'table'
      AND o.delta_rows=(o.scope->>'row_count')::bigint);
$$;
REVOKE ALL ON FUNCTION _oo.c5_overlay_receipted(text) FROM PUBLIC,oo_pipeline_runtime;

-- Boolean metadata only: validate the frozen root + actual component set, not
-- a chain of parent views. Repeated updates do not consume a depth budget.
CREATE FUNCTION _oo.c5_overlay_is_valid(p_physical text)
RETURNS boolean LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE o _oo.c5_overlay; r _oo.full_import_run; root_rows bigint:=0;
  component record; n integer:=0; total numeric; parent_rows bigint;
BEGIN
  SELECT * INTO o FROM _oo.c5_overlay WHERE physical_name=p_physical;
  IF NOT FOUND OR NOT _oo.c5_overlay_relation_ok(p_physical)
    OR NOT _oo.c5_overlay_receipted(p_physical) THEN RETURN false; END IF;
  SELECT * INTO STRICT r FROM _oo.full_import_run WHERE run_id=o.run_id;
  IF o.root_name IS NOT NULL THEN
    IF to_regclass(format('_oo.%I',o.root_name)) IS DISTINCT FROM o.root_oid
      OR NOT EXISTS(SELECT FROM pg_class WHERE oid=o.root_oid AND relkind='r'
        AND relpersistence='p' AND relowner='oo_target_owner'::regrole)
      OR (SELECT count(*) FROM pg_trigger tr WHERE tr.tgrelid=o.root_oid AND tr.tgenabled='O'
        AND tr.tgfoid='_oo.full_import_guard_shadow()'::regprocedure
        AND tr.tgname IN ('full_import_shadow_insert','full_import_shadow_mutate',
          'full_import_shadow_truncate','full_import_shadow_commit'))<>4 THEN RETURN false; END IF;
    SELECT t.applied_rows INTO root_rows FROM _oo.full_import_table t JOIN _oo.full_import_run pr USING(run_id)
      WHERE t.physical_name=o.root_name AND t.schema_ident=o.scope->>'schema' AND t.table_ident=o.scope->>'table'
        AND pr.source_id=r.source_id AND pr.source_generation=o.root_generation
        AND pr.config_digest=r.config_digest AND pr.type_digest=r.type_digest AND pr.schema_digest=r.schema_digest
        AND pr.state IN ('awaiting_publish','published') AND t.exhausted AND t.canonical_digest IS NOT NULL
        AND EXISTS(SELECT FROM _oo.apply_receipts a WHERE a.source=pr.source_id AND a.table_generation=t.physical_name
          AND a.config_digest=pr.config_digest AND a.type_digest=pr.type_digest AND a.schema_digest=pr.schema_digest
          AND a.fence_shard=pr.shard_id AND a.fence_generation=pr.target_generation);
    IF NOT FOUND THEN RETURN false; END IF;
  ELSIF o.root_oid IS NOT NULL OR o.root_generation<>0 THEN RETURN false;
  END IF;
  IF o.parent_name IS NOT NULL THEN
    SELECT t.applied_rows INTO parent_rows FROM _oo.full_import_table t JOIN _oo.full_import_run pr USING(run_id)
      WHERE t.physical_name=o.parent_name AND t.schema_ident=o.scope->>'schema' AND t.table_ident=o.scope->>'table'
        AND pr.source_id=r.source_id AND pr.source_generation=o.parent_generation
        AND pr.config_digest=r.config_digest AND pr.type_digest=r.type_digest AND pr.schema_digest=r.schema_digest
        AND pr.state IN ('awaiting_publish','published') AND t.exhausted;
    IF NOT FOUND OR parent_rows<>o.inherited_rows+o.replaced_rows THEN RETURN false; END IF;
  ELSIF o.inherited_rows<>0 OR o.replaced_rows<>0 THEN RETURN false;
  END IF;
  total:=root_rows;
  FOR component IN SELECT c.*,d.delta_rows,e.replaced_rows,d.scope AS ds,e.scope AS es,
      d.root_oid AS dr,e.root_oid AS er,d.key_idents AS dk,e.key_idents AS ek
    FROM _oo.c5_overlay_component c JOIN _oo.c5_overlay d ON d.physical_name=c.delta_owner
    JOIN _oo.c5_overlay e ON e.physical_name=c.exclusion_owner WHERE c.physical_name=p_physical LOOP
    n:=n+1;
    IF n>4096 OR NOT _oo.c5_overlay_receipted(component.delta_owner)
      OR NOT _oo.c5_overlay_receipted(component.exclusion_owner)
      OR component.dr IS DISTINCT FROM o.root_oid OR component.er IS DISTINCT FROM o.root_oid
      OR component.dk IS DISTINCT FROM o.key_idents OR component.ek IS DISTINCT FROM o.key_idents
      OR component.ds->>'source_id' IS DISTINCT FROM r.source_id OR component.es->>'source_id' IS DISTINCT FROM r.source_id
      OR component.ds->>'schema' IS DISTINCT FROM o.scope->>'schema' OR component.es->>'schema' IS DISTINCT FROM o.scope->>'schema'
      OR component.ds->>'table' IS DISTINCT FROM o.scope->>'table' OR component.es->>'table' IS DISTINCT FROM o.scope->>'table'
      OR component.ds->>'chunk_id' IS DISTINCT FROM component.chunk_id OR component.es->>'chunk_id' IS DISTINCT FROM component.chunk_id
      OR component.ds->'chunk_topology_revision' IS DISTINCT FROM o.scope->'chunk_topology_revision'
      OR component.es->'chunk_topology_revision' IS DISTINCT FROM o.scope->'chunk_topology_revision'
      OR component.ds->'lo' IS DISTINCT FROM component.es->'lo' OR component.ds->'hi' IS DISTINCT FROM component.es->'hi'
    THEN RETURN false; END IF;
    total:=total-component.replaced_rows+component.delta_rows;
  END LOOP;
  RETURN n>0 AND total=o.inherited_rows+o.delta_rows AND EXISTS(
    SELECT FROM _oo.c5_overlay_component WHERE physical_name=p_physical
      AND chunk_id=o.scope->>'chunk_id' AND delta_owner=p_physical);
END $$;
GRANT EXECUTE ON FUNCTION _oo.c5_overlay_is_valid(text) TO PUBLIC;

CREATE FUNCTION _oo.c5_overlay_row_count(p_physical text)
RETURNS bigint LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
  SELECT o.inherited_rows+o.delta_rows FROM _oo.c5_overlay o
    WHERE o.physical_name=p_physical AND _oo.c5_overlay_is_valid(p_physical);
$$;
GRANT EXECUTE ON FUNCTION _oo.c5_overlay_row_count(text) TO PUBLIC;

CREATE FUNCTION _oo.c5_overlay_prepare(
  p_shard text,p_owner uuid,p_fence bigint,p_run uuid,
  p_scope jsonb,p_keys text[],p_column_idents text[],p_key_idents text[]
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE
  r _oo.full_import_run; child _oo.full_import_table; f _oo.table_freshness;
  parent_run _oo.full_import_run; parent_table _oo.full_import_table;
  chunk _oo.c5_chunk; prior _oo.c5_overlay; old _oo.c5_overlay;
  child_oid oid; parent_oid oid; parent_attributes oid;
  parent_cols text[]:=ARRAY[]::text[]; child_cols text[]:=ARRAY[]::text[];
  pc text; cc text; ca pg_attribute; pa pg_attribute; i integer; k jsonb; v jsonb; trusted_keys text[];
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
  SELECT * INTO old FROM _oo.c5_overlay WHERE run_id=p_run;
  IF FOUND THEN
    IF old.scope IS DISTINCT FROM p_scope OR old.loader_keys IS DISTINCT FROM p_keys
      OR old.column_idents IS DISTINCT FROM p_column_idents OR old.key_idents IS DISTINCT FROM p_key_idents
      OR old.prepared_xid<>pg_current_xact_id() THEN
      RAISE EXCEPTION 'C5 overlay prepare replay differs' USING ERRCODE='OO010';
    END IF;
    RETURN old.inherited_rows;
  END IF;
  -- Locks the source watermark before any quality writer can advance it.
  PERFORM _oo.c5_range_guard(p_shard,p_owner,p_fence,p_scope,p_keys);
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
  FOR k IN SELECT value FROM jsonb_array_elements(p_scope->'members')
    UNION ALL SELECT typed_key FROM _oo.c5_member WHERE source=r.source_id AND schema_name=child.schema_ident
      AND table_name=child.table_ident AND chunk_id=chunk.chunk_id LOOP
    IF jsonb_array_length(k)<>cardinality(p_key_idents) THEN
      RAISE EXCEPTION 'C5 typed key width' USING ERRCODE='22023';
    END IF;
    FOR i IN 1..cardinality(p_key_idents) LOOP
      SELECT * INTO STRICT ca FROM pg_attribute WHERE attrelid=child_oid
        AND attname=child_cols[array_position(p_column_idents,p_key_idents[i])];
      v:=k->(i-1);
      IF v->>'kind' IS DISTINCT FROM (CASE WHEN ca.atttypid IN (20,21,23) THEN 'int'
        WHEN ca.atttypid IN (25,1043) THEN 'text' ELSE 'bytes' END) THEN
        RAISE EXCEPTION 'C5 typed key kind' USING ERRCODE='22023';
      END IF;
    END LOOP;
    PERFORM _oo.c5_overlay_loader_key(k);
    IF NOT has_text_key AND ((p_scope->'lo'<>'null'::jsonb AND _oo.c5_overlay_key_compare(k,p_scope->'lo')<=0)
      OR (p_scope->'hi'<>'null'::jsonb AND _oo.c5_overlay_key_compare(k,p_scope->'hi')>0)) THEN
      RAISE EXCEPTION 'C5 member outside source (lo, hi] range' USING ERRCODE='22023';
    END IF;
  END LOOP;
  IF EXISTS(SELECT FROM jsonb_array_elements(p_scope->'members') WITH ORDINALITY m
      WHERE convert_to(_oo.c5_overlay_loader_key(m.value),'UTF8') IS DISTINCT FROM convert_to(p_keys[m.ordinality],'UTF8'))
    OR EXISTS(SELECT FROM _oo.c5_member m WHERE m.source=r.source_id AND m.schema_name=child.schema_ident
      AND m.table_name=child.table_ident AND m.chunk_id=chunk.chunk_id
      AND (m.key_identity IS DISTINCT FROM _oo.c5_key_identity(m.typed_key)
        OR convert_to(m.loader_key,'UTF8') IS DISTINCT FROM convert_to(_oo.c5_overlay_loader_key(m.typed_key),'UTF8'))) THEN
    RAISE EXCEPTION 'C5 loader key does not match typed identity' USING ERRCODE='22023';
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
    VALUES(child.physical_name,p_run,p_scope,p_keys,p_column_idents,p_key_idents,child_cols,parent_cols,
      f.physical_name,parent_oid,gen,root_name,root_oid,root_generation,root_columns,
      'pd_'||substr(child.physical_name,4),child_oid,inherited,chunk.row_count,1,pg_current_xact_id());
  INSERT INTO _oo.c5_overlay_component
    SELECT child.physical_name,c.chunk_id,c.delta_owner,c.exclusion_owner FROM _oo.c5_overlay_component c
      WHERE c.physical_name=f.physical_name AND c.chunk_id<>chunk.chunk_id;
  SELECT c.exclusion_owner INTO exclusion_owner FROM _oo.c5_overlay_component c
    WHERE c.physical_name=f.physical_name AND c.chunk_id=chunk.chunk_id;
  IF exclusion_owner IS NULL THEN
    exclusion_owner:=child.physical_name;
    INSERT INTO _oo.c5_overlay_exclusion
      SELECT child.physical_name,key_identity FROM _oo.c5_member WHERE source=r.source_id
        AND schema_name=child.schema_ident AND table_name=child.table_ident AND chunk_id=chunk.chunk_id
      UNION SELECT child.physical_name,_oo.c5_key_identity(value) FROM jsonb_array_elements(p_scope->'members');
  END IF;
  INSERT INTO _oo.c5_overlay_component VALUES(child.physical_name,chunk.chunk_id,child.physical_name,exclusion_owner);
  -- Bound the number of stable chunks, not the number of updates/generations.
  IF (SELECT count(*) FROM _oo.c5_overlay_component WHERE physical_name=child.physical_name)>4096 THEN
    RAISE EXCEPTION 'C5 overlay component budget exhausted (4096)' USING ERRCODE='54001';
  END IF;
  RETURN inherited;
END $$;

CREATE FUNCTION _oo.c5_overlay_finish(p_shard text,p_owner uuid,p_fence bigint,p_run uuid)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE o _oo.c5_overlay; r _oo.full_import_run; keys text[]; expression text;
  delta_count bigint; matched bigint; unique_count bigint; projection text:='p."_oo_pos"';
  delta_projection text:='d."_oo_pos"'; statement text:=''; i integer; view_id oid; component record; exclusions text[];
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
  SELECT * INTO STRICT r FROM _oo.full_import_run WHERE run_id=p_run FOR UPDATE;
  SELECT * INTO STRICT o FROM _oo.c5_overlay WHERE run_id=p_run FOR UPDATE;
  IF r.state<>'open' OR r.shard_id IS DISTINCT FROM p_shard OR r.owner_token IS DISTINCT FROM p_owner
    OR r.target_generation IS DISTINCT FROM p_fence OR o.prepared_xid<>pg_current_xact_id() THEN
    RAISE EXCEPTION 'C5 overlay finish binding' USING ERRCODE='OO030';
  END IF;
  IF o.view_oid IS NOT NULL THEN
    IF NOT _oo.c5_overlay_relation_ok(o.physical_name) THEN
      RAISE EXCEPTION 'C5 overlay view changed' USING ERRCODE='OO030';
    END IF;
    RETURN o.inherited_rows+o.delta_rows;
  END IF;
  IF to_regclass(format('_oo.%I',o.physical_name)) IS DISTINCT FROM o.delta_oid THEN
    RAISE EXCEPTION 'C5 delta relation changed' USING ERRCODE='OO030';
  END IF;
  SELECT array_agg(o.child_columns[array_position(o.column_idents,k)] ORDER BY ord)
    INTO keys FROM unnest(o.key_idents) WITH ORDINALITY x(k,ord);
  expression:=_oo.c5_overlay_key_sql(o.delta_oid,keys,'d');
  EXECUTE format('SELECT count(*),count(DISTINCT %s),count(*) FILTER (WHERE %s IN
    (SELECT _oo.c5_key_identity(value) FROM jsonb_array_elements($1))) FROM _oo.%I d',
    expression,expression,o.physical_name)
    INTO delta_count,unique_count,matched USING o.scope->'members';
  IF delta_count<>(o.scope->>'row_count')::bigint OR delta_count<>unique_count OR matched<>delta_count THEN
    RAISE EXCEPTION 'C5 delta keys/count differ from range' USING ERRCODE='OO030';
  END IF;
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
      (SELECT FROM _oo.c5_overlay_exclusion e WHERE e.physical_name=ANY(%L::text[]) AND e.key_identity=%s)',
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
  RETURN o.inherited_rows+delta_count;
END $$;

CREATE OR REPLACE FUNCTION _oo.native_update_shadow_count(p_physical text)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE o _oo.c5_overlay; n bigint; r _oo.full_import_run;
BEGIN
  IF p_physical IS NULL OR NOT _oo.full_import_physical_ok(p_physical) OR p_physical !~ '^pt_' THEN
    RAISE EXCEPTION 'physical mapping name is closed' USING ERRCODE='23514';
  END IF;
  SELECT run.* INTO r FROM _oo.full_import_run run JOIN _oo.full_import_table t USING(run_id)
    WHERE t.physical_name=p_physical AND run.state='open';
  IF NOT FOUND THEN RAISE EXCEPTION 'unpublished shadow is not open' USING ERRCODE='OO031'; END IF;
  SELECT * INTO o FROM _oo.c5_overlay WHERE physical_name=p_physical;
  IF FOUND THEN
    PERFORM _oo.full_import_assert_bound(r.shard_id,r.owner_token,r.target_generation);
    IF o.view_oid IS NULL THEN
      EXECUTE format('SELECT count(*) FROM _oo.%I',p_physical) INTO n;
    ELSE
      IF NOT _oo.c5_overlay_relation_ok(p_physical) THEN
        RAISE EXCEPTION 'C5 overlay view changed' USING ERRCODE='OO030';
      END IF;
      n:=o.delta_rows;
    END IF;
    RETURN o.inherited_rows+n;
  END IF;
  EXECUTE format('SELECT count(*) FROM _oo.%I',p_physical) INTO n;
  RETURN n;
END $$;

-- Do not permit the ordinary sealing path to certify an unfinished overlay.
CREATE FUNCTION _oo.c5_overlay_seal_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
DECLARE o _oo.c5_overlay;
BEGIN
  IF NEW.state='awaiting_publish' AND OLD.state='open' THEN
    SELECT * INTO o FROM _oo.c5_overlay WHERE run_id=NEW.run_id;
    IF FOUND AND (NOT _oo.c5_overlay_relation_ok(o.physical_name)
      OR NOT EXISTS(SELECT FROM _oo.full_import_table t JOIN _oo.update_batch b USING(run_id)
        JOIN _oo.apply_receipts a ON a.source=b.source AND a.table_generation=b.physical_name
          AND a.txn_or_batch_id=b.batch_id AND a.payload_digest=b.payload_digest
        WHERE t.run_id=NEW.run_id AND t.exhausted AND t.physical_name=o.physical_name
          AND t.applied_rows=o.inherited_rows+o.delta_rows AND b.row_count=t.applied_rows)) THEN
      RAISE EXCEPTION 'C5 finished overlay and counted receipt required' USING ERRCODE='OO030';
    END IF;
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER c5_overlay_seal_guard BEFORE UPDATE ON _oo.full_import_run
  FOR EACH ROW EXECUTE FUNCTION _oo.c5_overlay_seal_guard();

REVOKE ALL ON FUNCTION _oo.c5_overlay_key_sql(oid,text[],text),_oo.c5_overlay_loader_key(jsonb),
  _oo.c5_overlay_key_compare(jsonb,jsonb),_oo.c5_overlay_seal_guard() FROM PUBLIC,oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.c5_overlay_prepare(text,uuid,bigint,uuid,jsonb,text[],text[],text[]),
  _oo.c5_overlay_finish(text,uuid,bigint,uuid),_oo.c5_overlay_relation_ok(text) FROM PUBLIC,oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.c5_overlay_prepare(text,uuid,bigint,uuid,jsonb,text[],text[],text[]),
  _oo.c5_overlay_finish(text,uuid,bigint,uuid) TO oo_pipeline_runtime;

-- Extend, do not replace, 0065's global/table generation, pending-membership
-- and digest-scan revision guards. No historical function body is copied here.
ALTER FUNCTION _oo.c5_range_guard(text,uuid,bigint,jsonb,text[]) RENAME TO c5_range_guard_before_overlay;
REVOKE ALL ON FUNCTION _oo.c5_range_guard_before_overlay(text,uuid,bigint,jsonb,text[]) FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_range_guard(p_shard text,p_owner uuid,p_fence bigint,p_scope jsonb,p_keys text[])
RETURNS text[] LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
  IF p_scope IS NULL OR jsonb_typeof(p_scope) IS DISTINCT FROM 'object' OR p_keys IS NULL
    OR array_position(p_keys,NULL) IS NOT NULL OR (cardinality(p_keys)>0 AND array_lower(p_keys,1)<>1) THEN
    RAISE EXCEPTION 'C5 closed scope and loader keys required' USING ERRCODE='22023';
  END IF;
  RETURN _oo.c5_range_guard_before_overlay(p_shard,p_owner,p_fence,p_scope,p_keys);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_range_guard(text,uuid,bigint,jsonb,text[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_range_guard(text,uuid,bigint,jsonb,text[]) TO oo_pipeline_runtime;

ALTER FUNCTION _oo.c5_range_commit(text,uuid,bigint,jsonb,text[],uuid,text,text) RENAME TO c5_range_commit_before_overlay;
REVOKE ALL ON FUNCTION _oo.c5_range_commit_before_overlay(text,uuid,bigint,jsonb,text[],uuid,text,text) FROM PUBLIC,oo_pipeline_runtime;
CREATE FUNCTION _oo.c5_range_commit(p_shard text,p_owner uuid,p_fence bigint,p_scope jsonb,p_keys text[],p_run uuid,p_batch text,p_payload text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $$
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_fence);
  IF EXISTS(SELECT FROM _oo.c5_overlay WHERE run_id=p_run) AND NOT EXISTS(
    SELECT FROM _oo.c5_overlay o JOIN _oo.full_import_run r USING(run_id)
    WHERE o.run_id=p_run AND o.scope=p_scope AND o.loader_keys=p_keys AND r.state IN ('awaiting_publish','published')
      AND r.shard_id=p_shard AND ((r.owner_token=p_owner AND r.target_generation=p_fence) OR EXISTS(
        SELECT FROM _oo.c5_block_receipt b WHERE b.run_id=p_run AND b.scope=p_scope
          AND b.batch_id=p_batch AND b.payload_digest=p_payload
          AND b.source_generation=r.source_generation AND b.target_generation=r.target_generation))
      AND _oo.c5_overlay_relation_ok(o.physical_name)) THEN
    RAISE EXCEPTION 'C5 overlay receipt scope differs' USING ERRCODE='OO030';
  END IF;
  PERFORM _oo.c5_range_commit_before_overlay(p_shard,p_owner,p_fence,p_scope,p_keys,p_run,p_batch,p_payload);
END $$;
REVOKE ALL ON FUNCTION _oo.c5_range_commit(text,uuid,bigint,jsonb,text[],uuid,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c5_range_commit(text,uuid,bigint,jsonb,text[],uuid,text,text) TO oo_pipeline_runtime;
