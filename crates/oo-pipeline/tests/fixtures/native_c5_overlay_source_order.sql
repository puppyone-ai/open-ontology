-- SQL-level source-order contract, not a live MySQL collation qualification.
-- Synthetic part receipts use the real multipart initializer + hydration APIs.
-- The order a < e-acute < Z deliberately disagrees with UTF8 byte order.
DO $$
DECLARE ctx jsonb; s text:='source_order'; t text:='case_keys'; own uuid; fence bigint; run uuid; physical text;
  dig text:=repeat('a',64); e text:=convert_from(decode('c3a9','hex'),'UTF8');
  parts jsonb; chunks jsonb; manifest jsonb; i integer; sha text; rows jsonb;
BEGIN
  ctx:=overlay_fixture.open_table(s,t,ARRAY[25,20,25]::bigint[],ARRAY[-1,-1,-1]);
  own:=(ctx->>'owner')::uuid; fence:=(ctx->>'fence')::bigint; run:=(ctx->>'run')::uuid; physical:=ctx->>'physical';
  rows:=jsonb_build_array(jsonb_build_array('a','1','old a'),jsonb_build_array(e,'2','old accent'),jsonb_build_array('Z','3','keep Z'));
  PERFORM overlay_fixture.insert_rows(ctx,rows);
  SET CONSTRAINTS ALL IMMEDIATE;
  EXECUTE format('CREATE UNIQUE INDEX ON _oo.%I(%I,%I)',physical,ctx->'pcs'->>0,ctx->'pcs'->>1);
  parts:=jsonb_build_array(
    jsonb_build_object('index',0,'total',2,'chunk_id',0,'rows',2,'arrow_sha256',repeat('1',64)),
    jsonb_build_object('index',1,'total',2,'chunk_id',1,'rows',1,'arrow_sha256',repeat('2',64)));
  chunks:=jsonb_build_array(
    jsonb_build_object('table',t,'chunk_id',0,'lo','null'::jsonb,
      'hi','[{"Text":[195,169]},{"Int":2}]'::jsonb,'rows',2,'bytes',200,'arrow_sha256',repeat('1',64)),
    jsonb_build_object('table',t,'chunk_id',1,'lo','[{"Text":[195,169]},{"Int":2}]'::jsonb,
      'hi','[{"Text":[90]},{"Int":3}]'::jsonb,'rows',1,'bytes',100,'arrow_sha256',repeat('2',64)));
  manifest:=jsonb_build_object('protocol','oo.mysql.c5-multipart/v1','source_id',s,'source_generation',1,
    'occurrence','case_fixture','schema','app','table',t,'schema_sha256',dig,
    'capture_config_sha256',dig,'manifest_sha256',dig,'chunks',chunks,'part_count',2,'parts',parts);
  UPDATE _oo.full_import_run SET multipart_scope=manifest WHERE run_id=run;
  FOR i IN 0..1 LOOP
    sha:=repeat((i+1)::text,64);
    PERFORM _oo.full_import_write_batch(s,own,fence,run,0,s,physical,'case_fixture.part.'||i,sha,dig,dig,
      encode(sha256(convert_to('full-import/v1 hook none','UTF8')),'hex'),dig,repeat('b',64),0,
      CASE WHEN i=0 THEN 2 ELSE 1 END,0,
      ARRAY[]::text[],ARRAY[]::bigint[],ARRAY[]::integer[],ARRAY[]::bigint[],ARRAY[]::text[]);
  END LOOP;
  PERFORM overlay_fixture.ok(_oo.c5_multipart_complete(s,own,fence,run)=3,'multipart part ACK coverage');
  PERFORM _oo.full_import_write_batch(s,own,fence,run,0,s,physical,'case_fixture.final',repeat('3',64),dig,dig,
    encode(sha256(convert_to('full-import/v1 hook none','UTF8')),'hex'),dig,repeat('b',64),0,0,0,
    ARRAY[]::text[],ARRAY[]::bigint[],ARRAY[]::integer[],ARRAY[]::bigint[],ARRAY[]::text[]);
  PERFORM _oo.full_import_note_table(s,own,fence,run,0,3,dig,s);
  PERFORM _oo.native_update_seal_quality(s,own,fence,run,s,1,'app',t,physical,'case_fixture.final',repeat('3',64),
    0,'snapshot-zero/v1','fixture','c5_diff','reconcile','B',false,ARRAY[]::text[],3,'ok',0,'full');
  SET CONSTRAINTS ALL IMMEDIATE;
  PERFORM _oo.full_import_seal_run(s,own,fence,run,s,1,dig,dig,repeat('b',64),dig,ARRAY['app'],ARRAY[t]);
  PERFORM _oo.c5_initialize_multipart_if_absent(s,own,fence,s,'app',t,dig,1,ARRAY['k1','k2'],1);
  PERFORM _oo.c5_hydrate_multipart(s,own,fence,s,'app',t,'0',1,repeat('1',64),
    '[[{"kind":"text","value":"61"},{"kind":"int","value":1}],[{"kind":"text","value":"c3a9"},{"kind":"int","value":2}]]');
  PERFORM _oo.c5_hydrate_multipart(s,own,fence,s,'app',t,'1',1,repeat('2',64),
    '[[{"kind":"text","value":"5a"},{"kind":"int","value":3}]]');
  INSERT INTO overlay_fixture.saved VALUES('source_order_base',ctx);
END $$;

-- Advance the global source on ANOTHER table after hydrating the baseline.
-- No case_keys mapping exists at source generation 2.
BEGIN;
INSERT INTO overlay_fixture.saved SELECT 'source_order_other',overlay_fixture.open_table('source_order','sibling');
SELECT overlay_fixture.insert_rows(ctx,'[["9","other table"]]') FROM overlay_fixture.saved WHERE label='source_order_other';
SET CONSTRAINTS ALL IMMEDIATE;
SELECT overlay_fixture.seal(ctx,1,false) FROM overlay_fixture.saved WHERE label='source_order_other';
COMMIT;

BEGIN;
INSERT INTO overlay_fixture.saved SELECT 'source_order_first',
  overlay_fixture.load_range('source_order','case_keys','0',
    jsonb_build_array(jsonb_build_array('A','1','new case'),
      jsonb_build_array(convert_from(decode('c389','hex'),'UTF8'),'2','new accent')),
    ARRAY[25,20,25]::bigint[],ARRAY[-1,-1,-1]);
COMMIT;
SELECT overlay_fixture.ok(parent_generation=1 AND scope->'parent_generation'='2'::jsonb,
  'sealed table generation differs from source global parent')
  FROM _oo.c5_overlay WHERE physical_name=(SELECT ctx->>'physical' FROM overlay_fixture.saved WHERE label='source_order_first');
BEGIN;
INSERT INTO overlay_fixture.saved SELECT 'source_order_second',
  overlay_fixture.load_range('source_order','case_keys','1','[["z","3","new z"]]',
    ARRAY[25,20,25]::bigint[],ARRAY[-1,-1,-1]);
COMMIT;
SELECT overlay_fixture.ok(_oo.c5_overlay_row_count(ctx->>'physical')=3,'source-order composed count')
  FROM overlay_fixture.saved WHERE label='source_order_second';
SELECT overlay_fixture.ok(overlay_fixture.read_rows(ctx)=
  jsonb_build_array(jsonb_build_array('A','1','new case'),jsonb_build_array('z','3','new z'),
    jsonb_build_array(convert_from(decode('c389','hex'),'UTF8'),'2','new accent')),'case/accent/composite identities preserved')
  FROM overlay_fixture.saved WHERE label='source_order_second';

DO $$
DECLARE ctx jsonb; sc jsonb;
BEGIN
  BEGIN
    ctx:=overlay_fixture.open_table('unproved_text','unproved',ARRAY[25,25]::bigint[],ARRAY[-1,-1]);
    sc:=overlay_fixture.scope(ctx,'0','[["ASCII","not enough proof"]]',
      '{"hi":[{"kind":"text","value":"7a"}]}'::jsonb);
    PERFORM overlay_fixture.prepare(ctx,sc);
    RAISE EXCEPTION 'unproven text order unexpectedly accepted';
  EXCEPTION WHEN SQLSTATE 'OO030' THEN NULL; END;
END $$;
\echo 'overlay mixed-case accent composite source-order contract passed (not MySQL E2E)'
