\set ON_ERROR_STOP on
-- More than 243 changed chunks and 32 successive updates must work without a
-- nested public-view dependency. The fixtures create genuine receipted versions.
DO $$
DECLARE ctx jsonb; sc jsonb; i integer; definition text; first_exclusions bigint; final_physical text;
BEGIN
  FOR i IN 0..243 LOOP
    ctx:=overlay_fixture.open_table('many_chunks','wide');
    sc:=overlay_fixture.scope(ctx,i::text,jsonb_build_array(jsonb_build_array(i::text,'v')),
      jsonb_build_object('lo',jsonb_build_array(jsonb_build_object('kind','int','value',i-1)),
        'hi',jsonb_build_array(jsonb_build_object('kind','int','value',i))));
    ctx:=overlay_fixture.prepare(ctx,sc);
    PERFORM overlay_fixture.insert_rows(ctx,jsonb_build_array(jsonb_build_array(i::text,'v')));
    PERFORM overlay_fixture.complete(ctx);
  END LOOP;
  SELECT count(*) INTO first_exclusions FROM _oo.c5_overlay_exclusion e JOIN _oo.c5_overlay o USING(physical_name)
    WHERE o.scope->>'source_id'='many_chunks';
  FOR i IN 1..40 LOOP
    ctx:=overlay_fixture.load_range('many_chunks','wide','0',jsonb_build_array(jsonb_build_array('0','changed'||i)));
  END LOOP;
  final_physical:=ctx->>'physical';
  PERFORM overlay_fixture.ok(_oo.c5_overlay_row_count(final_physical)=244,'244 chunk table after 284 generations');
  PERFORM overlay_fixture.ok((SELECT count(*) FROM _oo.c5_overlay_component WHERE physical_name=final_physical)=244,'one latest component per chunk');
  PERFORM overlay_fixture.ok(first_exclusions=(SELECT count(*) FROM _oo.c5_overlay_exclusion e JOIN _oo.c5_overlay o USING(physical_name)
    WHERE o.scope->>'source_id'='many_chunks'),'repeated chunk reuses frozen exclusions');
  PERFORM overlay_fixture.ok(NOT EXISTS(SELECT FROM pg_rewrite rw JOIN pg_depend dep ON dep.objid=rw.oid
      AND dep.classid='pg_rewrite'::regclass AND dep.refclassid='pg_class'::regclass
    JOIN pg_class parent ON parent.oid=dep.refobjid WHERE rw.ev_class=to_regclass(format('_oo.%I',final_physical))
      AND parent.relkind='v' AND parent.oid<>rw.ev_class),'view has no nested parent view');
  PERFORM overlay_fixture.ok(jsonb_array_length(overlay_fixture.read_rows(ctx))=244,'flat view real rows');
  INSERT INTO overlay_fixture.saved VALUES('many_chunks',ctx);
END $$;
\echo 'overlay 244-chunk and 284-generation flat composition passed'

-- Million-row root, generated locally. Bulk synthetic seed bypasses the
-- per-row shadow fence trigger ONLY during fixture creation; it is reenabled
-- before normal sealing and every measured overlay operation.
BEGIN;
INSERT INTO overlay_fixture.saved SELECT 'million',overlay_fixture.open_table('million','big');
SELECT format('ALTER TABLE _oo.%I DISABLE TRIGGER USER',ctx->>'physical') FROM overlay_fixture.saved WHERE label='million' \gexec
SELECT format('INSERT INTO _oo.%I("_oo_pos",%I,%I) SELECT 0,g,''seed'' FROM generate_series(1,1000002) g',
  ctx->>'physical',ctx->'pcs'->>0,ctx->'pcs'->>1) FROM overlay_fixture.saved WHERE label='million' \gexec
SELECT format('ALTER TABLE _oo.%I ENABLE TRIGGER USER',ctx->>'physical') FROM overlay_fixture.saved WHERE label='million' \gexec
SELECT overlay_fixture.seal(ctx,1000002,false) FROM overlay_fixture.saved WHERE label='million';
SELECT _oo.c5_initialize_full(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
  'million','app','big',repeat('a',64),1,'0',ARRAY['k1']) FROM overlay_fixture.saved WHERE label='million';
UPDATE _oo.c5_chunk SET hi='[{"kind":"int","value":2}]',row_count=2 WHERE source='million';
INSERT INTO _oo.c5_chunk(source,schema_name,table_name,chunk_id,topology_revision,lo,hi,membership_revision,row_count,arrow_sha256)
  VALUES('million','app','big','1',1,'[{"kind":"int","value":2}]','null',1,1000000,repeat('b',64));
UPDATE _oo.c5_member SET chunk_id='1' WHERE source='million' AND (typed_key->0->>'value')::bigint>=3;
COMMIT;
BEGIN;
INSERT INTO overlay_fixture.saved SELECT 'million_update',overlay_fixture.load_range('million','big','0','[["1","changed"]]');
SELECT overlay_fixture.ok(inherited_rows=1000000 AND delta_rows=1,'million inherited rows, only one stored delta')
  FROM _oo.c5_overlay WHERE scope->>'source_id'='million';
SELECT overlay_fixture.ok(seq_scan=0 AND coalesce(idx_scan,0)=0 AND n_tup_ins=0 AND n_tup_upd=0 AND n_tup_del=0,
  'no parent heap scans or writes during prepare/finish/count/seal') FROM pg_stat_xact_user_tables
  WHERE relid=to_regclass(format('_oo.%I',(SELECT ctx->>'physical' FROM overlay_fixture.saved WHERE label='million')));
COMMIT;
SELECT format('SELECT overlay_fixture.ok(count(*)=1000001,''million composed rows'') FROM _oo.%I',ctx->>'physical')
  FROM overlay_fixture.saved WHERE label='million_update' \gexec
SELECT format('SELECT overlay_fixture.ok(count(*)=1000002,''million original parent unchanged'') FROM _oo.%I',ctx->>'physical')
  FROM overlay_fixture.saved WHERE label='million' \gexec
\echo 'overlay million-row root with zero parent scan/copy during apply passed'
