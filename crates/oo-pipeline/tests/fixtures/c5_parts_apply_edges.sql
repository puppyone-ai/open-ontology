\set ON_ERROR_STOP on
\timing on
-- TARGET-ONLY. Include the existing real-claim stage rejection fixture.
BEGIN;
\set runtime c5_parts_test_runtime
\ir c5_parts_stage.sql
ROLLBACK;

CREATE FUNCTION parts_fixture.small_stage(ctx jsonb,rows jsonb) RETURNS void LANGUAGE plpgsql AS $$
DECLARE sc jsonb; box text; row jsonb; vals text; expr text; cols text; i integer; n bigint:=0;
 s text:=ctx->>'source'; t uuid:=(ctx->>'task')::uuid; g bigint:=(ctx->>'job_generation')::bigint;
BEGIN
 sc:=ctx->'header'||jsonb_build_object('part',jsonb_build_object('ordinal',0,'row_count',jsonb_array_length(rows),
   'arrow',jsonb_build_object('sha256',repeat('c',64))));
 box:=_oo.c5_parts_open(s,t,g,sc,ctx->'spec');
 IF _oo.c5_parts_begin(s,t,g,sc,repeat('d',64)) THEN
  expr:=_oo.c5_overlay_key_sql(to_regclass(format('_oo.%I',box)),
    ARRAY(SELECT 'c'||value FROM jsonb_array_elements_text(ctx->'spec'->'keys')),'x');
  SELECT string_agg(quote_ident('c'||v.pos),',' ORDER BY v.pos) INTO cols
   FROM generate_series(0,jsonb_array_length(ctx->'spec'->'columns')-1) v(pos);
  FOR row IN SELECT value FROM jsonb_array_elements(rows) LOOP
   vals:='';
   FOR i IN 0..jsonb_array_length(row)-1 LOOP
    vals:=vals||CASE WHEN i=0 THEN '' ELSE ',' END||format('%L::%s',row->>i,
     format_type((ctx->'types'->>i)::oid,(ctx->'mods'->>i)::integer));
   END LOOP;
   EXECUTE format('INSERT INTO _oo.%I(k,row_no,%s) SELECT %s,$1,%s FROM (VALUES(%s)) x(%s)',
    box,cols,expr,cols,vals,cols) USING n;
   n:=n+1;
  END LOOP;
  PERFORM _oo.c5_parts_finish_part(s,t,g,sc,repeat('d',64));
 END IF;
 PERFORM _oo.c5_parts_complete(s,t,g,repeat('b',64));
END $$;

-- Multiple physical components: replace the other chunk by an ordinary v1
-- overlay, then apply parts to the first chunk. Never copy the other delta.
BEGIN;
INSERT INTO parts_fixture.saved VALUES('other_open',parts_fixture.open('parts_large','parts_large'));
UPDATE parts_fixture.saved SET ctx=overlay_fixture.prepare(ctx,
 overlay_fixture.scope(ctx,'1','[["200000","2.5","4","other-updated"]]'))
 WHERE label='other_open';
SELECT overlay_fixture.insert_rows(ctx,'[["200000","2.5","4","other-updated"]]')
 FROM parts_fixture.saved WHERE label='other_open';
SELECT overlay_fixture.complete(ctx) FROM parts_fixture.saved WHERE label='other_open';
COMMIT;
BEGIN;
INSERT INTO parts_fixture.saved VALUES('regrow',parts_fixture.claim(parts_fixture.open('parts_large','parts_large'),10000));
COMMIT;
BEGIN;
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.stage(ctx,0,0) FROM parts_fixture.saved WHERE label='regrow';
SELECT _oo.c5_parts_complete(ctx->>'source',(ctx->>'task')::uuid,(ctx->>'job_generation')::bigint,repeat('b',64))
 FROM parts_fixture.saved WHERE label='regrow';
COMMIT;
BEGIN;
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='regrow';
-- Compare within one transaction: freshly renamed heap counters may still
-- contain unflushed activity from the previous transaction on this backend.
CREATE TEMP TABLE parts_heap_before ON COMMIT DROP AS
 SELECT relid,seq_scan,coalesce(idx_scan,0) AS idx_scan,n_tup_ins,n_tup_upd,n_tup_del
 FROM pg_stat_xact_user_tables WHERE relid IN (
  SELECT root_oid FROM _oo.c5_overlay WHERE run_id=(SELECT (ctx->>'run')::uuid FROM parts_fixture.saved WHERE label='other_open')
  UNION SELECT delta_oid FROM _oo.c5_overlay WHERE run_id=(SELECT (ctx->>'run')::uuid FROM parts_fixture.saved WHERE label='other_open'));
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.ok(parts_fixture.apply(ctx)=10001,'exact page boundary with empty terminal page')
 FROM parts_fixture.saved WHERE label='regrow';
RESET ROLE;
SELECT relname,seq_scan,idx_scan,n_tup_ins,n_tup_upd,n_tup_del FROM pg_stat_xact_user_tables
 WHERE relid IN (
  SELECT root_oid FROM _oo.c5_overlay WHERE run_id=(SELECT (ctx->>'run')::uuid FROM parts_fixture.saved WHERE label='regrow')
  UNION SELECT delta_oid FROM _oo.c5_overlay WHERE run_id=(SELECT (ctx->>'run')::uuid FROM parts_fixture.saved WHERE label='other_open'));
SELECT parts_fixture.ok(NOT EXISTS(
 SELECT FROM pg_stat_xact_user_tables after JOIN parts_heap_before before USING(relid)
 WHERE (after.seq_scan,coalesce(after.idx_scan,0),after.n_tup_ins,after.n_tup_upd,after.n_tup_del)
  IS DISTINCT FROM (before.seq_scan,before.idx_scan,before.n_tup_ins,before.n_tup_upd,before.n_tup_del)),
 'zero old root or other chunk scans and writes');
SELECT parts_fixture.ok(
 (SELECT delta_owner FROM _oo.c5_overlay_component WHERE physical_name=(SELECT ctx->>'physical' FROM parts_fixture.saved WHERE label='regrow')
   AND chunk_id='1')=(SELECT ctx->>'physical' FROM parts_fixture.saved WHERE label='other_open'),
 'other chunk retains same physical delta');
COMMIT;
BEGIN;
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='regrow';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.ok(_oo.c5_parts_commit(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
 ctx->'scope',(ctx->>'run')::uuid,ctx->>'batch',repeat('a',64))=0,'cross transaction exact receipt replay')
 FROM parts_fixture.saved WHERE label='regrow';
SELECT parts_fixture.reject(format('SELECT _oo.c5_parts_commit(%L,%L,%s,%L,%L,%L,%L)',
 ctx->>'source',ctx->>'owner',ctx->>'fence',ctx->'scope',ctx->>'run',ctx->>'batch',repeat('e',64)),
 'OO010','changed receipt replay refused') FROM parts_fixture.saved WHERE label='regrow';
COMMIT;

-- Even an accidental view SELECT grant cannot bypass the 0068 latch.
GRANT USAGE ON SCHEMA _oo,parts_fixture,overlay_fixture TO overlay_reader;
SELECT format('GRANT SELECT ON _oo.%I TO overlay_reader',ctx->>'physical')
 FROM parts_fixture.saved WHERE label='regrow' \gexec
SELECT ctx->>'physical' AS visible_physical FROM parts_fixture.saved WHERE label='regrow' \gset
SET ROLE overlay_reader;
SELECT overlay_fixture.ok(count(*)=0,'unpublished gated view reads zero for accidental grantee') FROM _oo.:"visible_physical";
RESET ROLE;
SELECT parts_fixture.ok(true,'0068 gate denies nonpublished reader even with SELECT');
SELECT parts_fixture.ok(NOT has_table_privilege('c5_parts_test_runtime','_oo.c5_parts_apply','SELECT,UPDATE')
 AND NOT has_table_privilege('c5_parts_test_runtime','_oo.c5_parts_capture','SELECT,UPDATE')
 AND NOT has_function_privilege('c5_parts_test_runtime','_oo.c5_parts_stage_bound(jsonb)','EXECUTE'),
 'private stage and apply metadata ACLs');

-- A sibling table advances the shared source high watermark, not this table.
BEGIN;
INSERT INTO parts_fixture.saved VALUES('sibling',parts_fixture.open('parts_large','parts_sibling'));
SELECT overlay_fixture.seal(ctx,0,false) FROM parts_fixture.saved WHERE label='sibling';
COMMIT;
BEGIN;
INSERT INTO parts_fixture.saved VALUES('shared_parent',
 parts_fixture.claim(parts_fixture.open('parts_large','parts_large'),1));
COMMIT;
BEGIN;
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.stage(ctx,0,0) FROM parts_fixture.saved WHERE label='shared_parent';
SELECT _oo.c5_parts_complete(ctx->>'source',(ctx->>'task')::uuid,(ctx->>'job_generation')::bigint,repeat('b',64))
 FROM parts_fixture.saved WHERE label='shared_parent';
COMMIT;
BEGIN;
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='shared_parent';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.ok(parts_fixture.apply(ctx)=2,'shared source high watermark versus older table parent CAS')
 FROM parts_fixture.saved WHERE label='shared_parent';
COMMIT;
SELECT parts_fixture.ok((SELECT o.parent_generation<(o.scope->>'parent_generation')::bigint
 AND _oo.c5_overlay_is_valid(o.physical_name) FROM _oo.c5_overlay o
 WHERE o.run_id=(SELECT (ctx->>'run')::uuid FROM parts_fixture.saved WHERE label='shared_parent')),
 'table parent remains independently receipted after sibling advancement');

-- New parts route with byte-exact text/binary composite keys and nullable cells.
BEGIN;
INSERT INTO parts_fixture.saved VALUES('composite_base',parts_fixture.open('parts_composite','parts_composite',
 '{"columns":[{"name":"name","oid":25,"typmod":-1,"nullable":false},
 {"name":"bin","oid":17,"typmod":-1,"nullable":false},
 {"name":"amount","oid":1700,"typmod":2490390,"nullable":true},
 {"name":"value","oid":25,"typmod":-1,"nullable":true}],"keys":[0,1]}'));
SELECT overlay_fixture.insert_rows(ctx,'[["B","\\x0000","1",null],["a","\\x00","2","delete"]]')
 FROM parts_fixture.saved WHERE label='composite_base';
SET CONSTRAINTS ALL IMMEDIATE;
SELECT overlay_fixture.seal(ctx,2,false) FROM parts_fixture.saved WHERE label='composite_base';
SELECT _oo.c5_initialize_full(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
 'parts_composite','app','parts_composite',repeat('a',64),1,'0',ARRAY['name','bin'])
 FROM parts_fixture.saved WHERE label='composite_base';
COMMIT;
BEGIN;
INSERT INTO parts_fixture.saved SELECT 'composite',
 parts_fixture.claim(parts_fixture.open('parts_composite','parts_composite',ctx->'spec'),3)
 FROM parts_fixture.saved WHERE label='composite_base';
COMMIT;
BEGIN;
SELECT parts_fixture.small_stage(ctx,
 '[["B","\\x0000","12345678901234567890.123456789012345678","updated"],["a","\\xff00",null,"new"],["z","\\x","-0.000000000000000001",null]]')
 FROM parts_fixture.saved WHERE label='composite';
COMMIT;
BEGIN;
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='composite';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.ok(parts_fixture.apply(ctx,512)=3,'parts text binary composite key IUD with 512-row pages')
 FROM parts_fixture.saved WHERE label='composite';
COMMIT;
SET ROLE oo_target_owner;
SELECT parts_fixture.ok(overlay_fixture.read_rows(ctx)=
 '[["B","\\x0000","12345678901234567890.123456789012345678","updated"],["a","\\xff00",null,"new"],["z","\\x","-0.000000000000000001",null]]'::jsonb,
 'composite typed values preserved byte-exact') FROM parts_fixture.saved WHERE label='composite';
RESET ROLE;

-- Prove that a lease can expire/reclaim without fabricating jobs state.
BEGIN;
INSERT INTO parts_fixture.saved SELECT 'expired',
 parts_fixture.claim(parts_fixture.open('parts_composite','parts_composite',ctx->'spec'),0,'0','c5-parts-expire')
 FROM parts_fixture.saved WHERE label='composite';
COMMIT;
BEGIN;
SELECT parts_fixture.small_stage(ctx,'[]') FROM parts_fixture.saved WHERE label='expired';
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='expired';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT _oo.c5_parts_guard(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,ctx->'scope')
 FROM parts_fixture.saved WHERE label='expired';
COMMIT;
BEGIN;
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='expired';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.reject(format('SELECT _oo.c5_parts_overlay_prepare(%L,%L,%s,%L,%L,%L::text[],%L::text[])',
 ctx->>'source',ctx->>'owner',ctx->>'fence',ctx->>'run',ctx->'scope','{name,bin,amount,value}','{name,bin}'),
 'OO010','separate transaction CAS refused') FROM parts_fixture.saved WHERE label='expired';
COMMIT;
SELECT pg_sleep(31);
BEGIN;
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='expired';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.reject(format('SELECT _oo.c5_parts_guard(%L,%L,%s,%L)',
 ctx->>'source',ctx->>'owner',ctx->>'fence',ctx->'scope'),'OO010','naturally expired real lease refused')
 FROM parts_fixture.saved WHERE label='expired';
COMMIT;
SELECT parts_fixture.ok((jobs.claim_selected((ctx->>'project')::uuid,(ctx->>'task')::uuid,
 ARRAY['native_capture'],'c5-parts-reclaimed')).status='claimed','real expired claim reclaimed')
 FROM parts_fixture.saved WHERE label='expired';
BEGIN;
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='expired';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.reject(format('SELECT _oo.c5_parts_guard(%L,%L,%s,%L)',
 ctx->>'source',ctx->>'owner',ctx->>'fence',ctx->'scope'),'OO010','reclaimed generation cannot reuse pinned CAS')
 FROM parts_fixture.saved WHERE label='expired';
COMMIT;
SELECT jobs.cancel((ctx->>'project')::uuid,(ctx->>'task')::uuid,'c5-parts-reclaimed',j.generation)
 FROM parts_fixture.saved JOIN jobs.tasks j ON j.id=(ctx->>'task')::uuid WHERE label='expired';
BEGIN;
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='expired';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.reject(format('SELECT _oo.c5_parts_guard(%L,%L,%s,%L)',
 ctx->>'source',ctx->>'owner',ctx->>'fence',ctx->'scope'),'OO010','real cancelled claim refused')
 FROM parts_fixture.saved WHERE label='expired';
COMMIT;
-- A fresh task can reuse the still-open abandoned run, but a reclaimed
-- generation cannot reuse a prepared CAS even within the SAME transaction.
BEGIN;
INSERT INTO parts_fixture.saved SELECT 'generation_switch',
 parts_fixture.claim(ctx,0,'0','c5-parts-expire-again') FROM parts_fixture.saved WHERE label='expired';
COMMIT;
BEGIN;
SELECT parts_fixture.small_stage(ctx,'[]') FROM parts_fixture.saved WHERE label='generation_switch';
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='generation_switch';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT _oo.c5_parts_overlay_prepare(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
 (ctx->>'run')::uuid,ctx->'scope',ARRAY['name','bin','amount','value'],ARRAY['name','bin'])
 FROM parts_fixture.saved WHERE label='generation_switch';
SELECT pg_sleep(31);
SELECT overlay_fixture.ok((jobs.claim_selected((ctx->>'project')::uuid,(ctx->>'task')::uuid,
 ARRAY['native_capture'],'c5-parts-new-generation')).status='claimed','genuine same-transaction reclaim')
 FROM parts_fixture.saved WHERE label='generation_switch';
SELECT parts_fixture.reject(format('SELECT _oo.c5_parts_copy_page(%L,%L,%s,%L,%L,NULL,10000)',
 ctx->>'source',ctx->>'owner',ctx->>'fence',ctx->>'run',ctx->'scope'),
 'OO010','same transaction changed real job generation refused') FROM parts_fixture.saved WHERE label='generation_switch';
ROLLBACK;
SELECT parts_fixture.ok(true,'same transaction real generation takeover refused and rolled back');
SELECT parts_fixture.ok(NOT EXISTS(SELECT FROM jw_native.pending_acks)
 AND NOT EXISTS(SELECT FROM jobs.tasks WHERE state='succeeded')
 AND NOT EXISTS(SELECT FROM _oo.full_import_run WHERE state='published'),'no fake source ACK job success or publication');
\echo 'PASS: target-only edges, real lease expiry/reclaim, visibility, exact-page and composite keys'
