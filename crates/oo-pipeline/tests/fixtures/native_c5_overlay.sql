\ir native_c5_overlay_helpers.sql
-- Small baseline with actual production fences, typed shadow and Loader receipt.
BEGIN;
INSERT INTO overlay_fixture.saved SELECT 'base',overlay_fixture.open_table('shared','orders');
SELECT overlay_fixture.insert_rows(ctx,'[["1","old"],["2","delete"],["3","keep"],["4","keep4"]]') FROM overlay_fixture.saved WHERE label='base';
SET CONSTRAINTS ALL IMMEDIATE;
SELECT overlay_fixture.seal(ctx,4,false) FROM overlay_fixture.saved WHERE label='base';
SELECT _oo.c5_initialize_full(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
  'shared','app','orders',repeat('a',64),1,'0',ARRAY['k1']) FROM overlay_fixture.saved WHERE label='base';
-- Fixture topology: partition the already initialized membership, not customer data.
UPDATE _oo.c5_chunk SET hi='[{"kind":"int","value":2}]',row_count=2 WHERE source='shared' AND table_name='orders';
INSERT INTO _oo.c5_chunk(source,schema_name,table_name,chunk_id,topology_revision,lo,hi,membership_revision,row_count,arrow_sha256)
  VALUES('shared','app','orders','1',1,'[{"kind":"int","value":2}]','null',1,2,repeat('b',64));
UPDATE _oo.c5_member SET chunk_id='1' WHERE source='shared' AND table_name='orders' AND loader_key IN ('3','4');
COMMIT;

-- Real COPY, update and delete the first range; inherit two rows from table gen 1.
-- Shared-source global/table generation regression uses multipart in source_order.sql.
BEGIN;
INSERT INTO overlay_fixture.saved SELECT 'update',overlay_fixture.open_table('shared','orders');
UPDATE overlay_fixture.saved SET ctx=overlay_fixture.prepare(ctx,
  overlay_fixture.scope(ctx,'0','[["1","updated"]]')) WHERE label='update';
SELECT ctx->>'physical' AS physical,ctx->'pcs'->>0 AS key_col,ctx->'pcs'->>1 AS val_col
  FROM overlay_fixture.saved WHERE label='update' \gset
SET LOCAL ROLE oo_pipeline_runtime;
COPY _oo.:"physical"("_oo_pos",:"key_col",:"val_col") FROM STDIN;
0	1	updated
\.
RESET ROLE;
SELECT overlay_fixture.ok(
  (SELECT jsonb_agg(cells) FROM _oo.c5_delta_read_cells(
    (ctx->>'run')::uuid,ARRAY(SELECT jsonb_array_elements_text(ctx->'pcs'))) AS r(cells))
    ='[["1","updated"]]'::jsonb,
  'delta reconciliation excludes inherited rows')
FROM overlay_fixture.saved WHERE label='update';
DO $$
DECLARE ctx jsonb;
BEGIN
  SELECT saved.ctx INTO ctx FROM overlay_fixture.saved saved WHERE label='update';
  BEGIN
    PERFORM _oo.c5_delta_read_cells((ctx->>'run')::uuid,ARRAY['not_a_bound_column']);
    RAISE EXCEPTION 'wrong delta columns accepted';
  EXCEPTION WHEN SQLSTATE 'OO030' THEN NULL;
  END;
END $$;
SELECT overlay_fixture.complete(ctx) FROM overlay_fixture.saved WHERE label='update';
COMMIT;
DO $$
DECLARE ctx jsonb;
BEGIN
  SELECT saved.ctx INTO ctx FROM overlay_fixture.saved saved WHERE label='update';
  BEGIN
    PERFORM _oo.c5_delta_read_cells((ctx->>'run')::uuid,
      ARRAY(SELECT jsonb_array_elements_text(ctx->'pcs')));
    RAISE EXCEPTION 'closed delta accepted';
  EXCEPTION WHEN SQLSTATE 'OO030' THEN NULL;
  END;
END $$;
SELECT overlay_fixture.ok(overlay_fixture.read_rows(ctx)='[["1","updated"],["3","keep"],["4","keep4"]]'::jsonb,'update+delete+inherit') FROM overlay_fixture.saved WHERE label='update';
SELECT overlay_fixture.ok(overlay_fixture.read_rows(ctx)='[["1","old"],["2","delete"],["3","keep"],["4","keep4"]]'::jsonb,'old baseline unchanged') FROM overlay_fixture.saved WHERE label='base';
SELECT overlay_fixture.ok(parent_generation=1 AND inherited_rows=2 AND replaced_rows=2 AND delta_rows=1,'table baseline metadata') FROM _oo.c5_overlay;

-- Empty changed range must delete its previous membership, not fall back to full.
BEGIN;
INSERT INTO overlay_fixture.saved SELECT 'empty',overlay_fixture.load_range('shared','orders','0','[]');
COMMIT;
SELECT overlay_fixture.ok(overlay_fixture.read_rows(ctx)='[["3","keep"],["4","keep4"]]'::jsonb,'empty range deletes') FROM overlay_fixture.saved WHERE label='empty';
BEGIN;
INSERT INTO overlay_fixture.saved SELECT 'empty_again',overlay_fixture.load_range('shared','orders','0','[]');
COMMIT;
SELECT overlay_fixture.ok(overlay_fixture.read_rows(ctx)='[["3","keep"],["4","keep4"]]'::jsonb,'already empty range') FROM overlay_fixture.saved WHERE label='empty_again';

-- No-change observation creates no storage generation or new receipt.
DO $$
DECLARE n bigint; receipts bigint; observation jsonb; fence bigint; now_ms bigint;
BEGIN
  SELECT count(*) INTO n FROM _oo.c5_overlay;
  SELECT count(*) INTO receipts FROM _oo.c5_block_receipt;
  fence:=_oo.adopt_target_generation('shared','11111111-1111-4111-8111-111111111111');
  PERFORM _oo.full_import_bind_session('shared','11111111-1111-4111-8111-111111111111',fence);
  SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint INTO now_ms;
  SELECT jsonb_build_object('schema_fingerprint',repeat('a',64),'checked_at_ms',now_ms,
    'full_digest_scan_completed',true,'chunks',jsonb_agg(jsonb_build_object('chunk_id',chunk_id,
      'lo',lo,'hi',hi,'membership_revision',membership_revision,'arrow_sha256',arrow_sha256,
      'digest',jsonb_build_array(row_count,'0123456789abcdef','fedcba9876543210'))))
    INTO observation FROM _oo.c5_chunk WHERE source='shared' AND table_name='orders' AND retired_at IS NULL;
  PERFORM _oo.c5_observe('shared','11111111-1111-4111-8111-111111111111',fence,'shared','app','orders',
    (SELECT source_generation FROM _oo.source_watermark WHERE source='shared'),observation);
  PERFORM overlay_fixture.ok(n=(SELECT count(*) FROM _oo.c5_overlay),'nochange no storage');
  PERFORM overlay_fixture.ok(receipts=(SELECT count(*) FROM _oo.c5_block_receipt),'nochange no apply receipt');
END $$;

-- Composite int/text/binary identity, UTF8 lengths, quotes and non-default bytea
-- output. All values remain data; generated SQL never interpolates loader text.
BEGIN;
SET LOCAL bytea_output='escape';
INSERT INTO overlay_fixture.saved SELECT 'composite',overlay_fixture.load_range('composite','mixed','0',
  jsonb_build_array(jsonb_build_array('-7',convert_from(decode('c3a9','hex'),'UTF8')||''' ; DROP TABLE x; --',chr(92)||'x00ff','v1')),
  ARRAY[20,25,17,25]::bigint[],ARRAY[-1,-1,-1,-1]);
COMMIT;
BEGIN;
SET LOCAL bytea_output='escape';
INSERT INTO overlay_fixture.saved SELECT 'composite_update',overlay_fixture.load_range('composite','mixed','0',
  jsonb_build_array(jsonb_build_array('-7',convert_from(decode('c3a9','hex'),'UTF8')||''' ; DROP TABLE x; --',chr(92)||'x00ff','v2')),
  ARRAY[20,25,17,25]::bigint[],ARRAY[-1,-1,-1,-1]);
COMMIT;
SELECT overlay_fixture.ok(_oo.c5_overlay_row_count(ctx->>'physical')=1,'composite update once') FROM overlay_fixture.saved WHERE label='composite_update';

-- Exact numeric and varchar typmods must survive UNION ALL.
BEGIN;
INSERT INTO overlay_fixture.saved SELECT 'typed',overlay_fixture.load_range('typed','typed','0',
  '[["key","123.45"]]',ARRAY[1043,1700]::bigint[],ARRAY[16,655366]);
COMMIT;
BEGIN;
INSERT INTO overlay_fixture.saved SELECT 'typed_update',overlay_fixture.load_range('typed','typed','0',
  '[["key","456.78"]]',ARRAY[1043,1700]::bigint[],ARRAY[16,655366]);
COMMIT;

\echo 'overlay basic lifecycle passed'

CREATE FUNCTION overlay_fixture.expect_error(statement text,expected text) RETURNS void LANGUAGE plpgsql AS $$
DECLARE code text;
BEGIN
  BEGIN
    EXECUTE statement;
  EXCEPTION WHEN OTHERS THEN GET STACKED DIAGNOSTICS code=RETURNED_SQLSTATE;
  END;
  PERFORM overlay_fixture.ok(code=expected,'expected '||expected||', got '||COALESCE(code,'success'));
END $$;

-- Structural registration, privileges and publication read-only behavior.
CREATE VIEW overlay_fixture.fake AS SELECT 1 AS id;
SELECT overlay_fixture.ok(NOT _oo.c5_overlay_is_valid('fake') AND _oo.c5_overlay_row_count('fake') IS NULL,'unregistered view denied');
SELECT overlay_fixture.ok(NOT has_table_privilege('oo_pipeline_runtime','_oo.c5_overlay','SELECT,INSERT,UPDATE,DELETE'),'registry private');
SELECT overlay_fixture.ok(NOT has_table_privilege('oo_pipeline_runtime','_oo.c5_overlay_exclusion','SELECT,INSERT,UPDATE,DELETE'),'exclusions private');
SELECT overlay_fixture.ok(NOT has_function_privilege('oo_pipeline_runtime',
  '_oo.c5_overlay_relation_ok(text)','EXECUTE'),'open structural validator private');
SELECT overlay_fixture.ok(bool_and(p.prosecdef AND p.provolatile='s'
  AND 'search_path=pg_catalog, _oo'=ANY(p.proconfig) AND p.proowner='oo_target_owner'::regrole),'closed metadata validators')
  FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
  WHERE n.nspname='_oo' AND p.proname IN ('c5_overlay_is_valid','c5_overlay_row_count');
GRANT USAGE ON SCHEMA _oo,overlay_fixture TO overlay_reader;
GRANT EXECUTE ON FUNCTION overlay_fixture.ok(boolean,text),overlay_fixture.expect_error(text,text) TO overlay_reader;
SELECT format('GRANT SELECT ON _oo.%I TO overlay_reader',ctx->>'physical')
  FROM overlay_fixture.saved WHERE label='update' \gexec
SELECT ctx->>'physical' AS physical,ctx->'pcs'->>0 AS key_col,ctx->'pcs'->>1 AS val_col
  FROM overlay_fixture.saved WHERE label='update' \gset
SET ROLE overlay_reader;
SELECT overlay_fixture.ok(count(*)=3,'reader SELECT exact immutable old version') FROM _oo.:"physical";
SELECT overlay_fixture.ok(_oo.c5_overlay_is_valid(:'physical'),'reader boolean metadata');
SELECT overlay_fixture.expect_error(format('DELETE FROM _oo.%I',:'physical'),'55000');
RESET ROLE;
DO $$
DECLARE o _oo.c5_overlay; definition text; n bigint;
BEGIN
  PERFORM set_config('search_path','pg_catalog,_oo',true);
  SELECT * INTO STRICT o FROM _oo.c5_overlay WHERE physical_name=(SELECT ctx->>'physical' FROM overlay_fixture.saved WHERE label='update');
  definition:=o.view_definition;
  EXECUTE format('CREATE OR REPLACE VIEW _oo.%I AS SELECT * FROM _oo.%I',o.physical_name,o.delta_name);
  PERFORM overlay_fixture.ok(NOT _oo.c5_overlay_is_valid(o.physical_name) AND _oo.c5_overlay_row_count(o.physical_name) IS NULL,'tampered view denied');
  EXECUTE format('CREATE OR REPLACE VIEW _oo.%I AS %s',o.physical_name,definition);
  PERFORM overlay_fixture.ok(_oo.c5_overlay_is_valid(o.physical_name),'restored exact definition');
  PERFORM overlay_fixture.ok(NOT has_table_privilege('overlay_reader',o.delta_oid,'SELECT'),'delta stays private');
  PERFORM overlay_fixture.ok(NOT has_table_privilege('oo_pipeline_runtime',o.delta_oid,'INSERT,UPDATE,DELETE,TRUNCATE'),'delta runtime writes revoked');
  BEGIN
    EXECUTE format('INSERT INTO _oo.%I SELECT * FROM _oo.%I',o.delta_name,o.delta_name);
    RAISE EXCEPTION 'mutation unexpectedly accepted';
  EXCEPTION WHEN SQLSTATE 'OO031' THEN NULL;
  END;
END $$;

-- Every refusal is rolled back by the subtransaction, including mappings,
-- registry, DDL, receipts and quality/freshness changes.
DO $$
DECLARE ctx jsonb; sc jsonb; old_count bigint; generation bigint; old_ptr text;
BEGIN
  SELECT count(*) INTO old_count FROM _oo.c5_overlay;
  SELECT source_generation INTO generation FROM _oo.source_watermark WHERE source='shared';
  SELECT physical_name INTO old_ptr FROM _oo.table_freshness WHERE source='shared' AND table_name='orders';
  BEGIN
    ctx:=overlay_fixture.open_table('shared','orders');
    sc:=overlay_fixture.scope(ctx,'0','[["1","x"]]');
    PERFORM overlay_fixture.prepare(ctx,sc,ARRAY['01']);
    RAISE EXCEPTION 'noncanonical key unexpectedly accepted';
  EXCEPTION WHEN SQLSTATE '22023' THEN NULL; END;
  BEGIN
    ctx:=overlay_fixture.open_table('shared','orders',ARRAY[23,25]::bigint[],ARRAY[-1,-1]);
    PERFORM overlay_fixture.prepare(ctx,overlay_fixture.scope(ctx,'0','[["1","x"]]'));
    RAISE EXCEPTION 'type change unexpectedly accepted';
  EXCEPTION WHEN SQLSTATE '23514' THEN NULL; END;
  BEGIN
    ctx:=overlay_fixture.open_table('shared','orders');
    PERFORM overlay_fixture.prepare(ctx,overlay_fixture.scope(ctx,'0','[["5","outside"]]'));
    RAISE EXCEPTION 'out of range key unexpectedly accepted';
  EXCEPTION WHEN SQLSTATE '22023' THEN NULL; END;
  BEGIN
    ctx:=overlay_fixture.open_table('shared','orders');
    ctx:=overlay_fixture.prepare(ctx,overlay_fixture.scope(ctx,'0','[["1","x"]]'));
    PERFORM overlay_fixture.insert_rows(ctx,'[["1","x"],["1","duplicate"]]');
    SET CONSTRAINTS ALL IMMEDIATE;
    PERFORM _oo.c5_overlay_finish('shared',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,(ctx->>'run')::uuid);
    RAISE EXCEPTION 'duplicate delta unexpectedly accepted';
  EXCEPTION WHEN SQLSTATE 'OO030' THEN NULL; END;
  BEGIN
    ctx:=overlay_fixture.open_table('shared','orders');
    ctx:=overlay_fixture.prepare(ctx,overlay_fixture.scope(ctx,'0','[["1","x"]]'));
    PERFORM overlay_fixture.insert_rows(ctx,'[["2","wrong key"]]');
    SET CONSTRAINTS ALL IMMEDIATE;
    PERFORM _oo.c5_overlay_finish('shared',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,(ctx->>'run')::uuid);
    RAISE EXCEPTION 'wrong delta key unexpectedly accepted';
  EXCEPTION WHEN SQLSTATE 'OO030' THEN NULL; END;
  BEGIN
    ctx:=overlay_fixture.open_table('shared','orders');
    ctx:=overlay_fixture.prepare(ctx,overlay_fixture.scope(ctx,'0','[]'));
    PERFORM overlay_fixture.seal(ctx,(ctx->>'inherited')::bigint,true);
    RAISE EXCEPTION 'unfinished overlay unexpectedly sealed';
  EXCEPTION WHEN SQLSTATE 'OO030' THEN NULL; END;
  BEGIN
    ctx:=overlay_fixture.open_table('shared','orders');
    ctx:=overlay_fixture.prepare(ctx,overlay_fixture.scope(ctx,'0','[]'));
    PERFORM _oo.c5_overlay_finish('shared','22222222-2222-4222-8222-222222222222',
      (ctx->>'fence')::bigint,(ctx->>'run')::uuid);
    RAISE EXCEPTION 'wrong owner unexpectedly accepted';
  EXCEPTION WHEN SQLSTATE 'OO013' THEN NULL; END;
  BEGIN
    ctx:=overlay_fixture.open_table('shared','orders');
    sc:=overlay_fixture.scope(ctx,'0','[]')||'{"prior_membership_revision":0}';
    PERFORM overlay_fixture.prepare(ctx,sc);
    RAISE EXCEPTION 'stale revision unexpectedly accepted';
  EXCEPTION WHEN SQLSTATE 'OO010' THEN NULL; END;
  BEGIN
    ctx:=overlay_fixture.open_table('shared','orders');
    ctx:=overlay_fixture.prepare(ctx,overlay_fixture.scope(ctx,'0','[]'));
    PERFORM overlay_fixture.complete(ctx);
    RAISE EXCEPTION 'injected failure after seal' USING ERRCODE='ZX001';
  EXCEPTION WHEN SQLSTATE 'ZX001' THEN NULL; END;
  PERFORM overlay_fixture.ok(old_count=(SELECT count(*) FROM _oo.c5_overlay),'all failed overlays rolled back');
  PERFORM overlay_fixture.ok(generation=(SELECT source_generation FROM _oo.source_watermark WHERE source='shared'),'failed source generation unchanged');
  PERFORM overlay_fixture.ok(old_ptr=(SELECT physical_name FROM _oo.table_freshness WHERE source='shared' AND table_name='orders'),'failed freshness unchanged');
END $$;

-- An exact sealed receipt remains replayable under a newly adopted fence.
DO $$
DECLARE ctx jsonb; own uuid:='22222222-2222-4222-8222-222222222222'; fence bigint; n bigint;
BEGIN
  SELECT saved.ctx INTO STRICT ctx FROM overlay_fixture.saved saved WHERE label='empty_again';
  SELECT count(*) INTO n FROM _oo.c5_block_receipt;
  PERFORM _oo.native_update_release_fence('shared');
  fence:=_oo.adopt_target_generation('shared',own);
  PERFORM _oo.full_import_bind_session('shared',own,fence);
  PERFORM _oo.c5_range_commit('shared',own,fence,ctx->'scope',ARRAY(SELECT jsonb_array_elements_text(ctx->'loader')),
    (ctx->>'run')::uuid,ctx->>'batch',repeat('a',64));
  PERFORM overlay_fixture.ok(n=(SELECT count(*) FROM _oo.c5_block_receipt),'takeover replay does not duplicate receipt');
  PERFORM overlay_fixture.ok(_oo.c5_overlay_is_valid(ctx->>'physical'),'takeover preserves immutable view');
  BEGIN
    PERFORM _oo.c5_range_commit('shared',own,fence,ctx->'scope',ARRAY['forged'],
      (ctx->>'run')::uuid,ctx->>'batch',repeat('a',64));
    RAISE EXCEPTION 'altered replay keys unexpectedly accepted';
  EXCEPTION WHEN SQLSTATE 'OO030' THEN NULL; END;
END $$;

\ir native_c5_overlay_source_order.sql
\ir native_c5_overlay_scale.sql
