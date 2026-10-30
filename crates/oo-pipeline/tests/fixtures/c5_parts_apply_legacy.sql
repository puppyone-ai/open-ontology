\set ON_ERROR_STOP on
-- TARGET-ONLY old v1 lifecycle on the exact same 0001..0104 installation.
BEGIN;
INSERT INTO parts_fixture.saved VALUES('v1',overlay_fixture.load_range('parts_legacy','parts_legacy','0',
 '[["1","old"],["2","delete"],["3","keep"]]'));
COMMIT;
BEGIN;
INSERT INTO parts_fixture.saved VALUES('v1_update',overlay_fixture.load_range('parts_legacy','parts_legacy','0',
 '[["1","updated"],["3","keep"],["4","insert"]]'));
COMMIT;
SET ROLE oo_target_owner;
SELECT parts_fixture.ok(overlay_fixture.read_rows(ctx)='[["1","updated"],["3","keep"],["4","insert"]]'::jsonb,
 'old v1 insert update delete regression') FROM parts_fixture.saved WHERE label='v1_update';
SELECT parts_fixture.ok(overlay_fixture.read_rows(ctx)='[["1","old"],["2","delete"],["3","keep"]]'::jsonb,
 'old v1 immutable previous generation') FROM parts_fixture.saved WHERE label='v1';
RESET ROLE;
SELECT parts_fixture.ok(_oo.c5_overlay_is_valid(ctx->>'physical'),'old v1 generic gated receipts')
 FROM parts_fixture.saved WHERE label='v1_update';
BEGIN;
INSERT INTO parts_fixture.saved VALUES('v1_empty',overlay_fixture.load_range('parts_legacy','parts_legacy','0','[]'));
COMMIT;
SELECT parts_fixture.ok(_oo.c5_overlay_row_count(ctx->>'physical')=0,'old v1 empty replacement')
 FROM parts_fixture.saved WHERE label='v1_empty';
BEGIN;
INSERT INTO parts_fixture.saved VALUES('v1_composite',overlay_fixture.load_range('parts_legacy_composite','parts_legacy_composite','0',
 '[["-1","B","\\x0000","value"],["2","a","\\xff00","other"]]',ARRAY[20,25,17,25]::bigint[],ARRAY[-1,-1,-1,-1]));
COMMIT;
SELECT parts_fixture.ok(_oo.c5_overlay_is_valid(ctx->>'physical') AND _oo.c5_overlay_row_count(ctx->>'physical')=2,
 'old v1 binary text composite keys') FROM parts_fixture.saved WHERE label='v1_composite';
SELECT parts_fixture.ok(position('100000' IN pg_get_functiondef('_oo.c5_delta_read_cells(uuid,text[])'::regprocedure))>0,
 'old 100000 reconciliation budget unchanged');
SELECT parts_fixture.ok(to_regclass('_oo.c5_oracle_binding') IS NULL OR
 (to_regprocedure('_oo.c5_oracle_initialize(text,uuid,jsonb,text,text,jsonb)') IS NOT NULL
 AND to_regprocedure('_oo.c5_range_guard_before_oracle(text,uuid,bigint,jsonb,text[])') IS NOT NULL),
 'Oracle legacy entrypoints retained when installed; not a new prerequisite');
-- Reuse the original 0065 synthetic multipart initialization/hydration test.
-- Run its reads as the data owner: 0068 deliberately closes all other readers.
SET ROLE oo_target_owner;
\ir native_c5_overlay_source_order.sql
RESET ROLE;
BEGIN;
INSERT INTO parts_fixture.saved VALUES('hydrated_text',
 parts_fixture.claim(parts_fixture.open('source_order','case_keys',
 '{"columns":[{"name":"k1","oid":25,"typmod":-1,"nullable":false},
 {"name":"k2","oid":20,"typmod":-1,"nullable":false},
 {"name":"value","oid":25,"typmod":-1,"nullable":false}],"keys":[0,1]}'),2));
COMMIT;
BEGIN;
SELECT parts_fixture.small_stage(ctx,'[["a","1","parts a"],["\u00e9","2","parts accent"]]')
 FROM parts_fixture.saved WHERE label='hydrated_text';
COMMIT;
BEGIN;
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='hydrated_text';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.ok(parts_fixture.apply(ctx)=3,'0065 hydrated multipart bounded text parts replacement')
 FROM parts_fixture.saved WHERE label='hydrated_text';
COMMIT;
SELECT parts_fixture.ok(_oo.c5_overlay_is_valid(ctx->>'physical'),'generic receipt for parts over hydrated multipart root')
 FROM parts_fixture.saved WHERE label='hydrated_text';
\echo 'PASS: targeted old v1 regression on full 0104 chain (no public publication)'
