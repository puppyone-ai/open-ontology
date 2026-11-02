BEGIN;
INSERT INTO overlay_fixture.saved SELECT 'legacy',
  overlay_fixture.load_range('legacy','orders','0','[["9","ungated"]]');
INSERT INTO overlay_fixture.saved SELECT 'base',overlay_fixture.open_table('visibility','orders');
SELECT overlay_fixture.insert_rows(ctx,'[["1","old"],["2","delete"],["3","keep"],["4","keep4"]]')
  FROM overlay_fixture.saved WHERE label='base';
SET CONSTRAINTS ALL IMMEDIATE;
SELECT overlay_fixture.seal(ctx,4,false) FROM overlay_fixture.saved WHERE label='base';
SELECT _oo.c5_initialize_full(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
  'visibility','app','orders',repeat('a',64),1,'0',ARRAY['k1'])
  FROM overlay_fixture.saved WHERE label='base';
UPDATE _oo.c5_chunk SET hi='[{"kind":"int","value":2}]',row_count=2 WHERE source='visibility';
INSERT INTO _oo.c5_chunk(source,schema_name,table_name,chunk_id,topology_revision,lo,hi,
  membership_revision,row_count,arrow_sha256)
VALUES('visibility','app','orders','1',1,'[{"kind":"int","value":2}]','null',1,2,repeat('b',64));
UPDATE _oo.c5_member SET chunk_id='1' WHERE source='visibility' AND loader_key IN ('3','4');
COMMIT;
