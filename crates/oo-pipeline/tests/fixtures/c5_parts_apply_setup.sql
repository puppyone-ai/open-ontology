\set ON_ERROR_STOP on
\timing on
-- TARGET-ONLY. Synthetic source metadata is not source admission evidence.
\ir native_c5_overlay_helpers.sql
CREATE SCHEMA parts_fixture;
CREATE TABLE parts_fixture.saved(label text PRIMARY KEY,ctx jsonb NOT NULL);
CREATE TABLE parts_fixture.passed(name text PRIMARY KEY);
CREATE FUNCTION parts_fixture.ok(p boolean,name text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN
 PERFORM overlay_fixture.ok(p,name);
 INSERT INTO parts_fixture.passed VALUES(name) ON CONFLICT DO NOTHING;
 RAISE NOTICE 'PASS: %',name;
END $$;

CREATE FUNCTION parts_fixture.spec() RETURNS jsonb LANGUAGE sql AS $$
 SELECT '{"columns":[{"name":"id","oid":20,"typmod":-1,"nullable":false},
 {"name":"float","oid":701,"typmod":-1,"nullable":true},
 {"name":"amount","oid":1700,"typmod":2490390,"nullable":true},
 {"name":"value","oid":25,"typmod":-1,"nullable":true}],"keys":[0]}'::jsonb
$$;

CREATE FUNCTION parts_fixture.open(s text,t text,spec jsonb DEFAULT parts_fixture.spec())
RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE owner uuid:='11111111-1111-4111-8111-111111111111'; fence bigint; gen bigint; phys text; run uuid;
 cols text[]; pcs text[]:='{}'; types bigint[]; mods integer[]; nn boolean[]; keys text[]; col text;
BEGIN
 fence:=_oo.adopt_target_generation(s,owner);
 PERFORM _oo.full_import_bind_session(s,owner,fence);
 gen:=coalesce((SELECT source_generation FROM _oo.source_watermark WHERE source=s),0)+1;
 SELECT array_agg(value->>'name' ORDER BY ordinality),
   array_agg((value->>'oid')::bigint ORDER BY ordinality),
   array_agg((value->>'typmod')::integer ORDER BY ordinality),
   array_agg(NOT (value->>'nullable')::boolean ORDER BY ordinality)
  INTO cols,types,mods,nn FROM jsonb_array_elements(spec->'columns') WITH ORDINALITY;
 SELECT array_agg(cols[value::integer+1] ORDER BY ordinality) INTO keys
  FROM jsonb_array_elements_text(spec->'keys') WITH ORDINALITY;
 PERFORM overlay_fixture.mapping(s,t,gen,'namespace','');
 phys:=overlay_fixture.mapping(s,t,gen,'table','');
 FOREACH col IN ARRAY cols LOOP
  pcs:=array_append(pcs,overlay_fixture.mapping(s,t,gen,'column',col));
 END LOOP;
 run:=_oo.full_import_open_run(s,owner,fence,s,gen,repeat('a',64),repeat('a',64),repeat('b',64),
  repeat('a',64),ARRAY['app'],ARRAY[t],ARRAY[phys]);
 PERFORM _oo.full_import_prepare_shadow(s,owner,fence,run,0,s,repeat('a',64),gen,'app',t,cols,types,mods,nn);
 RETURN jsonb_build_object('source',s,'table',t,'owner',owner,'fence',fence,'generation',gen,
  'physical',phys,'run',run,'columns',cols,'pcs',pcs,'keys',keys,'types',types,'mods',mods,'spec',spec,'batch','b'||gen);
END $$;

-- Enqueue/claim are genuine. Only the native occurrence fixture is synthetic.
CREATE FUNCTION parts_fixture.claim(ctx jsonb,n bigint,chunk text DEFAULT '0',worker text DEFAULT 'c5-parts-apply')
RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE project uuid:='7f6acd86-a344-4453-831d-541282a59912'; binding uuid:=gen_random_uuid();
 task uuid; claimed jobs.lease_result; ch _oo.c5_chunk; header jsonb; scope jsonb;
BEGIN
 SELECT task_id INTO task FROM jobs.enqueue(project,'native_capture',1,'{}','target-only-c5-parts',
  repeat('a',64),gen_random_uuid()::text,clock_timestamp(),3);
 INSERT INTO jw_native.bindings(id,project_id,source_id,method,cycle,period_ms,enabled,manifest_sha,pin,manifest,target_database)
  VALUES(binding,project,ctx->>'source','C5','pull',1000,false,repeat('a',64),1,'{}',current_database());
 INSERT INTO jw_native.occurrences(binding_id,pin,task_id,manifest_sha,scheduled_at,manifest)
  VALUES(binding,1,task,repeat('a',64),clock_timestamp(),'{}');
 INSERT INTO jw_native.source_progress(source_id,target_database,active_task_id)
  VALUES(ctx->>'source',current_database(),task)
  ON CONFLICT(source_id,target_database) DO UPDATE SET active_task_id=EXCLUDED.active_task_id;
 claimed:=jobs.claim_selected(project,task,ARRAY['native_capture'],worker);
 IF claimed.status<>'claimed' THEN RAISE EXCEPTION 'real claim required'; END IF;
 SELECT * INTO STRICT ch FROM _oo.c5_chunk WHERE source=ctx->>'source'
  AND schema_name='app' AND table_name=ctx->>'table' AND chunk_id=chunk;
 header:=jsonb_build_object('protocol','oo.pipeline.native-c5-range-part/v1',
  'range',jsonb_build_object('source_engine','mysql','source_id',ctx->>'source','schema','app','table',ctx->>'table',
   'schema_fingerprint',repeat('a',64),'chunk_topology_revision',ch.topology_revision,'chunk_id',ch.chunk_id,
   'lo',ch.lo,'hi',ch.hi,'prior_membership_revision',ch.membership_revision,
   'parent_generation',(ctx->>'generation')::bigint-1,'source_generation',(ctx->>'generation')::bigint),
  'parts_index',jsonb_build_object('path','/target-only/parts.jsonl','sha256',repeat('b',64),'bytes',800),
  'snapshot',jsonb_build_object('kind','target-only','value','not-source-admission'),
  'part_count',greatest(1,(n+59999)/60000),'total_rows',n);
 scope:=header->'range'||jsonb_build_object('protocol','oo.pipeline.native-c5-range-parts/v1',
  'capture_method','c5_diff','arrow_sha256',repeat('b',64),'row_count',n,'parts_task',task::text,
  'delete_detection','reconcile','position_codec','snapshot-zero/v1','applied_pos','0');
 RETURN ctx||jsonb_build_object('task',task,'job_generation',claimed.generation,'project',project,
  'header',header,'scope',scope);
END $$;

CREATE FUNCTION parts_fixture.stage(ctx jsonb,first_part bigint,last_part bigint) RETURNS void LANGUAGE plpgsql AS $$
DECLARE p bigint; n bigint; base bigint; scope jsonb; box text; t uuid:=(ctx->>'task')::uuid;
 g bigint:=(ctx->>'job_generation')::bigint; s text:=ctx->>'source'; total bigint:=(ctx->'scope'->>'row_count')::bigint;
BEGIN
 FOR p IN first_part..last_part LOOP
  base:=p*60000; n:=greatest(0,least(60000,total-base));
  scope:=ctx->'header'||jsonb_build_object('part',jsonb_build_object('ordinal',p,'row_count',n,
   'arrow',jsonb_build_object('sha256',repeat('c',64))));
  box:=_oo.c5_parts_open(s,t,g,scope,ctx->'spec');
  IF _oo.c5_parts_begin(s,t,g,scope,repeat('d',64)) THEN
   EXECUTE format('INSERT INTO _oo.%I(k,row_no,c0,c1,c2,c3)
    SELECT int4send(9)||decode(''00'',''hex'')||int8send(id),rn,id,
      CASE WHEN id%%3=0 THEN ''-0''::float8 WHEN id%%3=1 THEN ''NaN''::float8 ELSE 1.25 END,
      12345678901234567890.123456789012345678::numeric(38,18),
      CASE WHEN id%%5=0 THEN NULL ELSE ''new''||id::text END
    FROM (SELECT v-$1 AS rn,CASE WHEN v=1 THEN 1 ELSE v+1 END AS id
      FROM generate_series($1,$2) v) q',box) USING base+1,base+n;
   PERFORM _oo.c5_parts_finish_part(s,t,g,scope,repeat('d',64));
  END IF;
 END LOOP;
END $$;

CREATE FUNCTION parts_fixture.bind(ctx jsonb) RETURNS void LANGUAGE plpgsql AS $$
BEGIN
 PERFORM _oo.full_import_bind_session(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint);
END $$;

CREATE FUNCTION parts_fixture.apply(ctx jsonb,page_rows integer DEFAULT 10000) RETURNS bigint LANGUAGE plpgsql AS $$
DECLARE s text:=ctx->>'source'; own uuid:=(ctx->>'owner')::uuid; f bigint:=(ctx->>'fence')::bigint;
 r uuid:=(ctx->>'run')::uuid; sc jsonb:=ctx->'scope'; result jsonb; key text; n bigint:=0; inherited bigint; deleted bigint;
BEGIN
 deleted:=_oo.c5_parts_guard(s,own,f,sc);
 inherited:=_oo.c5_parts_overlay_prepare(s,own,f,r,sc,
   ARRAY(SELECT jsonb_array_elements_text(ctx->'columns')),ARRAY(SELECT jsonb_array_elements_text(ctx->'keys')));
 LOOP
  result:=_oo.c5_parts_copy_page(s,own,f,r,sc,key,page_rows);
  PERFORM overlay_fixture.ok(result=_oo.c5_parts_copy_page(s,own,f,r,sc,key,page_rows),'page replay no recopy');
  n:=n+(result->>'copied_rows')::bigint; key:=result->>'last_key';
  EXIT WHEN (result->>'copied_rows')::bigint<page_rows;
 END LOOP;
 PERFORM overlay_fixture.ok(n=(sc->>'row_count')::bigint,'all staged rows copied');
 PERFORM overlay_fixture.ok(_oo.c5_parts_reconcile(s,own,f,r,sc)=n,'exact typed delta');
 SET CONSTRAINTS ALL IMMEDIATE;
 PERFORM overlay_fixture.ok(_oo.c5_parts_overlay_finish(s,own,f,r,sc)=n+inherited,'flat total rows');
 PERFORM overlay_fixture.ok(_oo.c5_parts_overlay_finish(s,own,f,r,sc)=n+inherited,'finish replay');
 PERFORM overlay_fixture.ok(NOT _oo.c5_overlay_is_valid(ctx->>'physical'),'unsealed generic guard');
 -- Always empty tombstone array; 0104 writes the per-key deletion ledger.
 PERFORM parts_fixture.seal(ctx,n+inherited);
 PERFORM overlay_fixture.ok(NOT _oo.c5_overlay_is_valid(ctx->>'physical'),'unreceipted generic guard');
 PERFORM overlay_fixture.ok(_oo.c5_parts_commit(s,own,f,sc,r,ctx->>'batch',repeat('a',64))=deleted,'commit deletion count');
 PERFORM overlay_fixture.ok(_oo.c5_parts_commit(s,own,f,sc,r,ctx->>'batch',repeat('a',64))=deleted,'commit replay');
 RETURN n+inherited;
END $$;

-- Use the real quality API with incremental mode, without old helper's members.
CREATE FUNCTION parts_fixture.seal(ctx jsonb,n bigint) RETURNS void LANGUAGE plpgsql AS $$
DECLARE s text:=ctx->>'source'; own uuid:=(ctx->>'owner')::uuid; f bigint:=(ctx->>'fence')::bigint;
 r uuid:=(ctx->>'run')::uuid; dig text:=repeat('a',64);
BEGIN
 PERFORM _oo.full_import_write_batch(s,own,f,r,0,s,ctx->>'physical',ctx->>'batch',dig,dig,dig,
  encode(sha256(convert_to('full-import/v1 hook none','UTF8')),'hex'),dig,repeat('b',64),0,n,0,
  '{}'::text[],'{}'::bigint[],'{}'::integer[],'{}'::bigint[],'{}'::text[]);
 PERFORM _oo.full_import_note_table(s,own,f,r,0,n,dig,s);
 PERFORM _oo.native_update_seal_quality(s,own,f,r,s,(ctx->>'generation')::bigint,'app',ctx->>'table',
  ctx->>'physical',ctx->>'batch',dig,0,'snapshot-zero/v1','target-only','c5_diff','reconcile','B',false,
  '{}'::text[],n,'ok',0,'incremental');
 PERFORM _oo.full_import_seal_run(s,own,f,r,s,(ctx->>'generation')::bigint,dig,dig,repeat('b',64),dig,
  ARRAY['app'],ARRAY[ctx->>'table']);
END $$;

BEGIN;
INSERT INTO parts_fixture.saved VALUES('base',parts_fixture.open('parts_large','parts_large'));
SELECT overlay_fixture.insert_rows(ctx,
 '[["1","1.25","1.000000000000000001","old"],["2","0","2",null],["3","-0","3","old3"],["200000","2.5","4","keep"]]')
 FROM parts_fixture.saved WHERE label='base';
SET CONSTRAINTS ALL IMMEDIATE;
SELECT overlay_fixture.seal(ctx,4,false) FROM parts_fixture.saved WHERE label='base';
SELECT _oo.c5_initialize_full(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
 'parts_large','app','parts_large',repeat('a',64),1,'0',ARRAY['id'])
 FROM parts_fixture.saved WHERE label='base';
UPDATE _oo.c5_chunk SET hi='[{"kind":"int","value":150000}]',row_count=3 WHERE source='parts_large';
INSERT INTO _oo.c5_chunk(source,schema_name,table_name,chunk_id,topology_revision,lo,hi,membership_revision,row_count,arrow_sha256)
 VALUES('parts_large','app','parts_large','1',1,'[{"kind":"int","value":150000}]','null',1,1,repeat('b',64));
UPDATE _oo.c5_member SET chunk_id='1' WHERE source='parts_large' AND loader_key='200000';
SELECT _oo.c5_scan_begin(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
 'parts_large','app','parts_large','parts-target-only-scan','["0","1"]')
 FROM parts_fixture.saved WHERE label='base';
COMMIT;
BEGIN;
INSERT INTO parts_fixture.saved VALUES('large',parts_fixture.claim(parts_fixture.open('parts_large','parts_large'),100001));
COMMIT;
GRANT USAGE ON SCHEMA parts_fixture,overlay_fixture TO c5_parts_test_runtime;
GRANT SELECT,UPDATE ON parts_fixture.saved TO c5_parts_test_runtime;
GRANT SELECT,INSERT ON parts_fixture.passed TO c5_parts_test_runtime;
GRANT USAGE ON SCHEMA parts_fixture,overlay_fixture TO oo_target_owner;
GRANT SELECT ON parts_fixture.saved,overlay_fixture.saved TO oo_target_owner;
GRANT INSERT,UPDATE ON overlay_fixture.saved TO oo_target_owner;
GRANT SELECT,INSERT ON parts_fixture.passed TO oo_target_owner;
\echo 'TARGET-ONLY setup: real jobs enqueue/claim; no ACK or publication'
