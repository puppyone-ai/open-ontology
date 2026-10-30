\set ON_ERROR_STOP on
\timing on
-- TARGET-ONLY synthetic typed rows. No source-admission claim.
CREATE FUNCTION parts_fixture.reject(statement text,code text,name text) RETURNS void LANGUAGE plpgsql AS $$
DECLARE got text;
BEGIN
 BEGIN
  EXECUTE statement;
 EXCEPTION WHEN OTHERS THEN GET STACKED DIAGNOSTICS got=RETURNED_SQLSTATE;
 END;
 PERFORM parts_fixture.ok(got IS NOT DISTINCT FROM code,name||' ['||coalesce(got,'ACCEPTED')||']');
END $$;

BEGIN;
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.stage(ctx,0,0) FROM parts_fixture.saved WHERE label='large';
COMMIT;
BEGIN;
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='large';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.reject(format('SELECT _oo.c5_parts_guard(%L,%L,%s,%L)',
 ctx->>'source',ctx->>'owner',ctx->>'fence',ctx->'scope'),'OO030','incomplete stage refused')
 FROM parts_fixture.saved WHERE label='large';
COMMIT;

BEGIN;
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.stage(ctx,1,1) FROM parts_fixture.saved WHERE label='large';
SELECT _oo.c5_parts_complete(ctx->>'source',(ctx->>'task')::uuid,(ctx->>'job_generation')::bigint,repeat('b',64))
 FROM parts_fixture.saved WHERE label='large';
SELECT parts_fixture.stage(ctx,0,1) FROM parts_fixture.saved WHERE label='large';
COMMIT;

-- Wrong scope, fence, owner, native claim and parent. Subtransactions leave
-- the original stage and job state intact; jobs are NEVER hand-updated.
BEGIN;
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='large';
SET LOCAL ROLE c5_parts_test_runtime;
DO $$
DECLARE c jsonb; sc jsonb; statement text;
BEGIN
 SELECT ctx INTO c FROM parts_fixture.saved WHERE label='large'; sc:=c->'scope';
 statement:=format('SELECT _oo.c5_parts_guard(%L,%L,%s,%%L)',c->>'source',c->>'owner',c->>'fence');
 PERFORM parts_fixture.reject(format(statement,sc||'{"extra":true}'::jsonb),'22023','extra scope field refused');
 PERFORM parts_fixture.reject(format(statement,jsonb_set(sc,'{arrow_sha256}',to_jsonb(repeat('e',64)))),'OO030','index SHA mismatch refused');
 PERFORM parts_fixture.reject(format(statement,jsonb_set(sc,'{row_count}','100000')),'OO030','total rows mismatch refused');
 PERFORM parts_fixture.reject(format(statement,jsonb_set(sc,'{parent_generation}','0')),'OO030','stage range mismatch refused');
 PERFORM parts_fixture.reject(format('SELECT _oo.c5_parts_guard(%L,%L,%s,%L)',
   c->>'source',c->>'owner',(c->>'fence')::bigint+1,sc),'OO013','wrong target fence refused');
END $$;
RESET ROLE;
DO $$
DECLARE c jsonb; name text; sc jsonb;
BEGIN
 SELECT ctx INTO c FROM parts_fixture.saved WHERE label='large';
 BEGIN
  UPDATE _oo.c5_table SET source_generation=999 WHERE source='parts_large';
  PERFORM _oo.c5_parts_guard(c->>'source',(c->>'owner')::uuid,(c->>'fence')::bigint,c->'scope');
  RAISE EXCEPTION 'wrong parent accepted';
 EXCEPTION WHEN SQLSTATE 'OO010' THEN RAISE NOTICE 'PASS: wrong current parent refused'; END;
 BEGIN
  UPDATE jw_native.source_progress SET active_task_id=NULL WHERE source_id='parts_large';
  PERFORM _oo.c5_parts_guard(c->>'source',(c->>'owner')::uuid,(c->>'fence')::bigint,c->'scope');
  RAISE EXCEPTION 'wrong current source progress accepted';
 EXCEPTION WHEN SQLSTATE 'OO010' THEN RAISE NOTICE 'PASS: noncurrent sourceProgress refused'; END;
 BEGIN
  UPDATE jw_native.bindings SET method='C4' WHERE source_id='parts_large';
  PERFORM _oo.c5_parts_guard(c->>'source',(c->>'owner')::uuid,(c->>'fence')::bigint,c->'scope');
  RAISE EXCEPTION 'non-C5 job accepted';
 EXCEPTION WHEN SQLSTATE 'OO010' THEN RAISE NOTICE 'PASS: non-C5 native job refused'; END;
 BEGIN
  UPDATE _oo.c5_chunk SET baseline_membership_pending=true WHERE source='parts_large' AND chunk_id='1';
  PERFORM _oo.c5_parts_guard(c->>'source',(c->>'owner')::uuid,(c->>'fence')::bigint,c->'scope');
  RAISE EXCEPTION 'pending membership accepted';
 EXCEPTION WHEN SQLSTATE 'OO010' THEN RAISE NOTICE 'PASS: pending other chunk refused'; END;
 BEGIN
  INSERT INTO _oo.c5_member VALUES('parts_large','app','parts_large','1',
   _oo.c5_key_identity('[{"kind":"int","value":4}]'),'[{"kind":"int","value":4}]','4');
  PERFORM _oo.c5_parts_guard(c->>'source',(c->>'owner')::uuid,(c->>'fence')::bigint,c->'scope');
  RAISE EXCEPTION 'overlapping key accepted';
 EXCEPTION WHEN SQLSTATE '22023' THEN RAISE NOTICE 'PASS: other chunk overlap refused'; END;
END $$;
COMMIT;

-- Guard/prepare/copy and final metadata must share a single transaction.
BEGIN;
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='large';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT _oo.c5_parts_guard(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,ctx->'scope')
 FROM parts_fixture.saved WHERE label='large';
ROLLBACK;
SELECT parts_fixture.ok(NOT EXISTS(SELECT FROM _oo.c5_parts_apply),'rolled back CAS leaves durable stage reusable');

BEGIN;
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='large';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.ok(_oo.c5_parts_guard(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,ctx->'scope')=1,
 'guard counts every deleted key without arrays') FROM parts_fixture.saved WHERE label='large';
SELECT parts_fixture.ok(_oo.c5_parts_overlay_prepare(ctx->>'source',(ctx->>'owner')::uuid,
 (ctx->>'fence')::bigint,(ctx->>'run')::uuid,ctx->'scope',
 ARRAY(SELECT jsonb_array_elements_text(ctx->'columns')),ARRAY['id'])=1,'one unchanged inherited row')
 FROM parts_fixture.saved WHERE label='large';
SELECT parts_fixture.reject(format('SELECT _oo.c5_parts_reconcile(%L,%L,%s,%L,%L)',
 ctx->>'source',ctx->>'owner',ctx->>'fence',ctx->>'run',ctx->'scope'),
 'OO030','reconcile before complete copy refused') FROM parts_fixture.saved WHERE label='large';
SELECT parts_fixture.reject(format('SELECT _oo.c5_parts_copy_page(%L,%L,%s,%L,%L,NULL,10001)',
 ctx->>'source',ctx->>'owner',ctx->>'fence',ctx->>'run',ctx->'scope'),
 'OO030','oversized page refused') FROM parts_fixture.saved WHERE label='large';
-- Outlive the real 30-second lease while a different backend heartbeats it.
SELECT pg_sleep(32);
SELECT parts_fixture.ok(_oo.c5_parts_guard(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,ctx->'scope')=1,
 'independent heartbeat keeps a greater-than-30-second atomic transaction live')
 FROM parts_fixture.saved WHERE label='large';
SELECT parts_fixture.reject(format('SELECT _oo.c5_parts_copy_page(%L,%L,%s,%L,%L,%L,10000)',
 ctx->>'source',ctx->>'owner',ctx->>'fence',ctx->>'run',ctx->'scope','00'),'OO010','skipped cursor refused')
 FROM parts_fixture.saved WHERE label='large';
SELECT parts_fixture.reject(format('SELECT _oo.c5_parts_commit(%L,%L,%s,%L,%L,%L,%L)',
 ctx->>'source',ctx->>'owner',ctx->>'fence',ctx->'scope',ctx->>'run',ctx->>'batch',repeat('a',64)),
 'OO010','commit before seal refused') FROM parts_fixture.saved WHERE label='large';
DO $$
DECLARE c jsonb; result jsonb; k text; n bigint:=0;
BEGIN
 SELECT ctx INTO c FROM parts_fixture.saved WHERE label='large';
 LOOP
  result:=_oo.c5_parts_copy_page(c->>'source',(c->>'owner')::uuid,(c->>'fence')::bigint,
    (c->>'run')::uuid,c->'scope',k,10000);
  PERFORM overlay_fixture.ok(result=_oo.c5_parts_copy_page(c->>'source',(c->>'owner')::uuid,(c->>'fence')::bigint,
    (c->>'run')::uuid,c->'scope',k,10000),'exact page replay');
  n:=n+(result->>'copied_rows')::bigint; k:=result->>'last_key';
  EXIT WHEN (result->>'copied_rows')::bigint<10000;
 END LOOP;
 PERFORM parts_fixture.ok(n=100001,'100001 staged rows copied by bounded keyset pages with replay');
 PERFORM parts_fixture.ok(_oo.c5_parts_reconcile(c->>'source',(c->>'owner')::uuid,(c->>'fence')::bigint,
  (c->>'run')::uuid,c->'scope')=100001,'100001 exact typed cells including float NaN signed-zero decimal NULL');
END $$;
RESET ROLE;
-- Privileged fault injection into this private synthetic stage only. No
-- production trigger is disabled, and every injected change is rolled back.
DO $$
DECLARE c jsonb; physical text; test integer;
BEGIN
 SELECT ctx INTO c FROM parts_fixture.saved WHERE label='large';
 SELECT p.physical INTO physical FROM _oo.c5_parts_capture p WHERE source='parts_large';
 FOR test IN 1..4 LOOP
  BEGIN
   IF test=1 THEN EXECUTE format('UPDATE _oo.%I SET c1=0::float8 WHERE c0=3',physical);
   ELSIF test=2 THEN EXECUTE format('UPDATE _oo.%I SET c2=c2+0.000000000000000001 WHERE c0=3',physical);
   ELSIF test=3 THEN EXECUTE format('UPDATE _oo.%I SET c3='''' WHERE c0=5',physical);
   ELSE EXECUTE format('DELETE FROM _oo.%I WHERE c0=3',physical);
   END IF;
   PERFORM _oo.c5_parts_reconcile(c->>'source',(c->>'owner')::uuid,(c->>'fence')::bigint,(c->>'run')::uuid,c->'scope');
   RAISE EXCEPTION 'typed corruption accepted %',test;
  EXCEPTION WHEN SQLSTATE 'OO030' THEN RAISE NOTICE 'PASS: exact typed corruption refused case %',test; END;
 END LOOP;
END $$;
SET LOCAL ROLE c5_parts_test_runtime;
SET CONSTRAINTS ALL IMMEDIATE;
SELECT parts_fixture.ok(_oo.c5_parts_overlay_finish(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
 (ctx->>'run')::uuid,ctx->'scope')=100002,'flat finish inherited plus delta')
 FROM parts_fixture.saved WHERE label='large';
SELECT parts_fixture.seal(ctx,100002) FROM parts_fixture.saved WHERE label='large';
SELECT parts_fixture.ok(NOT _oo.c5_overlay_is_valid(ctx->>'physical'),'generic receipt gate rejects before parts commit')
 FROM parts_fixture.saved WHERE label='large';
SELECT parts_fixture.ok(_oo.c5_parts_commit(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
 ctx->'scope',(ctx->>'run')::uuid,ctx->>'batch',repeat('a',64))=1,'sealed atomic membership and per-key deletion')
 FROM parts_fixture.saved WHERE label='large';
SELECT parts_fixture.ok(_oo.c5_parts_commit(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
 ctx->'scope',(ctx->>'run')::uuid,ctx->>'batch',repeat('a',64))=1,'commit receipt replay')
 FROM parts_fixture.saved WHERE label='large';
COMMIT;

SELECT parts_fixture.ok(_oo.c5_overlay_is_valid(ctx->>'physical') AND
 _oo.c5_overlay_row_count(ctx->>'physical')=100002,'generic receipts and 0068 visibility validity')
 FROM parts_fixture.saved WHERE label='large';
SELECT parts_fixture.ok((SELECT tombstone_count FROM _oo.update_batch WHERE source='parts_large' AND source_generation=2)=1
 AND (SELECT count(*) FROM _oo.update_tombstone WHERE source='parts_large' AND key_text='2')=1,'real tombstone count and deleted key');
SELECT parts_fixture.ok((SELECT next_block=1 AND status='running' AND completed_at IS NULL
 FROM _oo.c5_scan WHERE source='parts_large' AND scan_id='parts-target-only-scan')
 AND (SELECT c5_mode='layered' AND last_full_digest_scan_completed_at IS NULL
 FROM _oo.table_quality WHERE schema_name='app' AND table_name='parts_large'),
 'original scan prefix and layered quality without fake full digest completion');
SELECT format('SELECT parts_fixture.ok(count(*)=4,''old root unchanged'') FROM _oo.%I',ctx->>'physical')
 FROM parts_fixture.saved WHERE label='base' \gexec
SELECT format('SET ROLE oo_target_owner; SELECT count(*)=100002 AS complete_rows FROM _oo.%I; RESET ROLE;',ctx->>'physical')
 FROM parts_fixture.saved WHERE label='large' \gexec
SELECT parts_fixture.ok((SELECT cardinality(loader_keys)=0 AND overlay_depth=1 FROM _oo.c5_overlay WHERE scope->>'source_id'='parts_large'),
 'no giant loader keys or nested overlay');
SELECT parts_fixture.ok(NOT EXISTS(SELECT FROM jw_native.pending_acks) AND
 NOT EXISTS(SELECT FROM _oo.full_import_run WHERE state='published'),'no publication or job ACK');

-- A zero-row replacement deletes all 100001 keys, not a historical budget.
BEGIN;
INSERT INTO parts_fixture.saved VALUES('empty',parts_fixture.claim(parts_fixture.open('parts_large','parts_large'),0));
COMMIT;
BEGIN;
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.stage(ctx,0,0) FROM parts_fixture.saved WHERE label='empty';
SELECT _oo.c5_parts_complete(ctx->>'source',(ctx->>'task')::uuid,(ctx->>'job_generation')::bigint,repeat('b',64))
 FROM parts_fixture.saved WHERE label='empty';
COMMIT;
BEGIN;
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='empty';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.ok(parts_fixture.apply(ctx)=1,'empty range keeps unchanged chunk only')
 FROM parts_fixture.saved WHERE label='empty';
COMMIT;
SELECT parts_fixture.ok((SELECT tombstone_count FROM _oo.update_batch WHERE source='parts_large' AND source_generation=3)=100001
 AND (SELECT count(*) FROM _oo.update_tombstone WHERE source='parts_large' AND source_generation=3)=100001,
 '100001 individual tombstones no arrays');
SELECT parts_fixture.ok((SELECT count(*) FROM _oo.c5_member WHERE source='parts_large')=1,
 'empty replacement complete membership');
SELECT parts_fixture.ok(_oo.c5_overlay_is_valid(ctx->>'physical'),'generic receipts valid after massive delete')
 FROM parts_fixture.saved WHERE label='empty';
\echo 'PASS: growing-range core SQL target-only'
