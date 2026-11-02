\set ON_ERROR_STOP on
-- Synthetic data only. Helpers are isolated from the product schema.
CREATE SCHEMA overlay_fixture;
CREATE FUNCTION overlay_fixture.ok(p boolean,p_message text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN IF p IS DISTINCT FROM true THEN RAISE EXCEPTION 'FAIL: %',p_message; END IF; END $$;
CREATE FUNCTION overlay_fixture.mapping(s text,t text,g bigint,l text,c text) RETURNS text LANGUAGE plpgsql AS $$
BEGIN
  PERFORM _oo.register_physical_mapping(s,repeat('a',64),g,l,'app',
    CASE WHEN l='namespace' THEN '' ELSE t END,c,
    sha256(_oo.pm_payload(s,repeat('a',64),g,
      CASE l WHEN 'namespace' THEN 'n' WHEN 'table' THEN 't' ELSE 'c' END,
      'app',CASE WHEN l='namespace' THEN '' ELSE t END,c)));
  RETURN _oo.lookup_physical_mapping(s,repeat('a',64),g,l,'app',CASE WHEN l='namespace' THEN '' ELSE t END,c);
END
$$;
CREATE FUNCTION overlay_fixture.open_table(s text,t text,
  types bigint[] DEFAULT ARRAY[20,25]::bigint[], mods integer[] DEFAULT ARRAY[-1,-1])
RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE own uuid:='11111111-1111-4111-8111-111111111111'; fence bigint; gen bigint; physical text;
  cols text[]:=ARRAY[]::text[]; pcs text[]:=ARRAY[]::text[]; keys text[]; run uuid; i integer;
BEGIN
  fence:=_oo.adopt_target_generation(s,own);
  PERFORM _oo.full_import_bind_session(s,own,fence);
  SELECT COALESCE((SELECT source_generation FROM _oo.source_watermark WHERE source=s),0)+1 INTO gen;
  PERFORM overlay_fixture.mapping(s,t,gen,'namespace','');
  physical:=overlay_fixture.mapping(s,t,gen,'table','');
  FOR i IN 1..cardinality(types) LOOP
    cols:=array_append(cols,CASE WHEN i=cardinality(types) THEN 'value' ELSE 'k'||i END);
    pcs:=array_append(pcs,overlay_fixture.mapping(s,t,gen,'column',cols[i]));
  END LOOP;
  keys:=cols[1:cardinality(cols)-1];
  run:=_oo.full_import_open_run(s,own,fence,s,gen,repeat('a',64),repeat('a',64),repeat('b',64),
    repeat('a',64),ARRAY['app'],ARRAY[t],ARRAY[physical]);
  PERFORM _oo.full_import_prepare_shadow(s,own,fence,run,0,s,repeat('a',64),gen,'app',t,
    cols,types,mods,array_fill(true,ARRAY[cardinality(types)]));
  RETURN jsonb_build_object('source',s,'table',t,'owner',own,'fence',fence,'generation',gen,
    'physical',physical,'run',run,'columns',to_jsonb(cols),'pcs',to_jsonb(pcs),'keys',to_jsonb(keys),
    'types',to_jsonb(types),'mods',to_jsonb(mods),'batch','b'||gen);
END $$;

CREATE FUNCTION overlay_fixture.insert_rows(ctx jsonb,rows jsonb) RETURNS void LANGUAGE plpgsql AS $$
DECLARE row jsonb; cols text; vals text; i integer;
BEGIN
  SELECT string_agg(format('%I',value),',' ORDER BY ordinality) INTO cols
    FROM jsonb_array_elements_text(ctx->'pcs') WITH ORDINALITY;
  FOR row IN SELECT value FROM jsonb_array_elements(rows) LOOP
    vals:='';
    FOR i IN 0..jsonb_array_length(row)-1 LOOP
      IF i>0 THEN vals:=vals||','; END IF;
      vals:=vals||format('%L::%s',row->>i,format_type((ctx->'types'->>i)::oid,(ctx->'mods'->>i)::integer));
    END LOOP;
    EXECUTE format('INSERT INTO _oo.%I("_oo_pos",%s) VALUES(0,%s)',ctx->>'physical',cols,vals);
  END LOOP;
END $$;

CREATE FUNCTION overlay_fixture.scope(ctx jsonb,chunk text,rows jsonb,overrides jsonb DEFAULT '{}')
RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE row jsonb; members jsonb:='[]'; key jsonb; i integer; kind oid; ch _oo.c5_chunk;
BEGIN
  SELECT * INTO ch FROM _oo.c5_chunk WHERE source=ctx->>'source' AND schema_name='app'
    AND table_name=ctx->>'table' AND chunk_id=chunk;
  FOR row IN SELECT value FROM jsonb_array_elements(rows) LOOP
    key:='[]';
    FOR i IN 0..jsonb_array_length(ctx->'keys')-1 LOOP
      kind:=(ctx->'types'->>i)::oid;
      key:=key||jsonb_build_array(CASE WHEN kind IN (20,21,23) THEN jsonb_build_object('kind','int','value',(row->>i)::bigint)
        WHEN kind=17 THEN jsonb_build_object('kind','bytes','value',encode((row->>i)::bytea,'hex'))
        ELSE jsonb_build_object('kind','text','value',encode(convert_to(row->>i,'UTF8'),'hex')) END);
    END LOOP;
    members:=members||jsonb_build_array(key);
  END LOOP;
  RETURN jsonb_build_object('protocol','oo.pipeline.native-c5-range/v1','source_engine','mysql',
    'capture_method','c5_diff','source_id',ctx->>'source','schema','app','table',ctx->>'table',
    'schema_fingerprint',repeat('a',64),'chunk_topology_revision',COALESCE(ch.topology_revision,1),
    'chunk_id',chunk,'lo',COALESCE(ch.lo,'null'),'hi',COALESCE(ch.hi,'null'),
    'prior_membership_revision',COALESCE(ch.membership_revision,0),
    'parent_generation',(ctx->>'generation')::bigint-1,'source_generation',(ctx->>'generation')::bigint,
    'arrow_sha256',repeat('b',64),'row_count',jsonb_array_length(rows),'delete_detection','reconcile',
    'position_codec','snapshot-zero/v1','applied_pos','0','members',members)||overrides;
END $$;

CREATE FUNCTION overlay_fixture.prepare(ctx jsonb,scope jsonb,keys_override text[] DEFAULT NULL)
RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE loader text[]; inherited bigint;
BEGIN
  SELECT COALESCE(array_agg(_oo.c5_overlay_loader_key(value) ORDER BY ordinality),ARRAY[]::text[])
    INTO loader FROM jsonb_array_elements(scope->'members') WITH ORDINALITY;
  loader:=COALESCE(keys_override,loader);
  inherited:=_oo.c5_overlay_prepare(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
    (ctx->>'run')::uuid,scope,loader,
    ARRAY(SELECT jsonb_array_elements_text(ctx->'columns')),ARRAY(SELECT jsonb_array_elements_text(ctx->'keys')));
  RETURN ctx||jsonb_build_object('scope',scope,'loader',to_jsonb(loader),'inherited',inherited);
END $$;

CREATE FUNCTION overlay_fixture.seal(ctx jsonb,n bigint,is_overlay boolean) RETURNS void LANGUAGE plpgsql AS $$
DECLARE s text:=ctx->>'source'; own uuid:=(ctx->>'owner')::uuid; fence bigint:=(ctx->>'fence')::bigint;
  run uuid:=(ctx->>'run')::uuid; dig text:=repeat('a',64); missing text[]:=ARRAY[]::text[];
BEGIN
  IF is_overlay THEN
    SELECT COALESCE(array_agg(m.loader_key),ARRAY[]::text[]) INTO missing FROM _oo.c5_member m
      WHERE m.source=s AND m.schema_name='app' AND m.table_name=ctx->>'table' AND m.chunk_id=ctx->'scope'->>'chunk_id'
        AND NOT EXISTS(SELECT FROM jsonb_array_elements(ctx->'scope'->'members') k WHERE _oo.c5_key_identity(k.value)=m.key_identity);
  END IF;
  PERFORM _oo.full_import_write_batch(s,own,fence,run,0,s,ctx->>'physical',ctx->>'batch',dig,dig,dig,
    encode(sha256(convert_to('full-import/v1 hook none','UTF8')),'hex'),dig,repeat('b',64),0,n,0,
    ARRAY[]::text[],ARRAY[]::bigint[],ARRAY[]::integer[],ARRAY[]::bigint[],ARRAY[]::text[]);
  PERFORM _oo.full_import_note_table(s,own,fence,run,0,n,dig,s);
  PERFORM _oo.native_update_seal_quality(s,own,fence,run,s,(ctx->>'generation')::bigint,'app',ctx->>'table',
    ctx->>'physical',ctx->>'batch',dig,0,'snapshot-zero/v1','fixture','c5_diff','reconcile','B',false,missing,n,'ok',0,
    CASE WHEN is_overlay THEN 'incremental' ELSE 'full' END);
  PERFORM _oo.full_import_seal_run(s,own,fence,run,s,(ctx->>'generation')::bigint,dig,dig,repeat('b',64),dig,
    ARRAY['app'],ARRAY[ctx->>'table']);
END $$;

CREATE FUNCTION overlay_fixture.complete(ctx jsonb) RETURNS void LANGUAGE plpgsql AS $$
DECLARE s text:=ctx->>'source'; own uuid:=(ctx->>'owner')::uuid; fence bigint:=(ctx->>'fence')::bigint;
  run uuid:=(ctx->>'run')::uuid; n bigint;
BEGIN
  SET CONSTRAINTS ALL IMMEDIATE;
  n:=_oo.c5_overlay_finish(s,own,fence,run);
  PERFORM overlay_fixture.ok(n=(ctx->>'inherited')::bigint+jsonb_array_length(ctx->'scope'->'members'),'inherited + delta');
  PERFORM overlay_fixture.ok(_oo.native_update_shadow_count(ctx->>'physical')=n,'bounded shadow count');
  PERFORM overlay_fixture.ok(NOT _oo.c5_overlay_is_valid(ctx->>'physical'),'unsealed view is not admissible');
  PERFORM overlay_fixture.seal(ctx,n,true);
  PERFORM overlay_fixture.ok(NOT _oo.c5_overlay_is_valid(ctx->>'physical'),'block receipt required');
  PERFORM _oo.c5_range_commit(s,own,fence,ctx->'scope',ARRAY(SELECT jsonb_array_elements_text(ctx->'loader')),
    run,ctx->>'batch',repeat('a',64));
  PERFORM overlay_fixture.ok(_oo.c5_overlay_is_valid(ctx->>'physical'),'sealed receipted overlay valid');
  PERFORM overlay_fixture.ok(_oo.c5_overlay_row_count(ctx->>'physical')=n,'sealed lineage row count');
END $$;

CREATE FUNCTION overlay_fixture.load_range(s text,t text,chunk text,rows jsonb,
  types bigint[] DEFAULT ARRAY[20,25]::bigint[],mods integer[] DEFAULT ARRAY[-1,-1]) RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE ctx jsonb;
BEGIN
  ctx:=overlay_fixture.open_table(s,t,types,mods);
  ctx:=overlay_fixture.prepare(ctx,overlay_fixture.scope(ctx,chunk,rows));
  PERFORM overlay_fixture.insert_rows(ctx,rows);
  PERFORM overlay_fixture.complete(ctx);
  RETURN ctx;
END $$;

CREATE FUNCTION overlay_fixture.read_rows(ctx jsonb) RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE expr text; result jsonb;
BEGIN
  SELECT string_agg(format('r.%I::text',value),',' ORDER BY ordinality) INTO expr
    FROM jsonb_array_elements_text(ctx->'pcs') WITH ORDINALITY;
  EXECUTE format('SELECT COALESCE(jsonb_agg(x ORDER BY x::text COLLATE "C"),''[]'') FROM
    (SELECT jsonb_build_array(%s) x FROM _oo.%I r) q',expr,ctx->>'physical') INTO result;
  RETURN result;
END $$;

CREATE TABLE overlay_fixture.saved(label text PRIMARY KEY,ctx jsonb);
