\set ON_ERROR_STOP on
\timing on
-- TARGET-ONLY: 18 synthetic columns, 388 rows. Real Loader and jobs APIs.
CREATE SCHEMA widening_fixture;
CREATE TABLE widening_fixture.saved(label text PRIMARY KEY,ctx jsonb NOT NULL);
CREATE FUNCTION widening_fixture.spec(width integer) RETURNS jsonb LANGUAGE sql AS $$
 SELECT jsonb_build_object('columns',jsonb_build_array(
  jsonb_build_object('name','company_id','oid',1043,'typmod',9,'nullable',false),
  jsonb_build_object('name','param_id','oid',1043,'typmod',36,'nullable',false),
  jsonb_build_object('name','param_value','oid',1043,'typmod',width+4,'nullable',true))||
  (SELECT jsonb_agg(jsonb_build_object('name','c'||n,'oid',25,'typmod',-1,'nullable',true) ORDER BY n)
   FROM generate_series(3,17)n),'keys','[0,1]'::jsonb)
$$;
CREATE FUNCTION widening_fixture.rows(value text) RETURNS jsonb LANGUAGE sql AS $$
 SELECT jsonb_agg(jsonb_build_array('C01','p'||n,value)||to_jsonb(array_fill(NULL::text,ARRAY[15])) ORDER BY n)
 FROM generate_series(1,388)n
$$;

BEGIN;
INSERT INTO widening_fixture.saved VALUES('root',parts_fixture.open('widening','config_system_param',widening_fixture.spec(1000)));
SELECT overlay_fixture.insert_rows(ctx,widening_fixture.rows('root')) FROM widening_fixture.saved WHERE label='root';
SET CONSTRAINTS ALL IMMEDIATE;
SELECT overlay_fixture.seal(ctx,388,false) FROM widening_fixture.saved WHERE label='root';
SELECT _oo.c5_initialize_published_if_absent('widening',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
 'widening','app','config_system_param',repeat('a',64),1,ARRAY['company_id','param_id'],1)
 FROM widening_fixture.saved WHERE label='root';
SELECT _oo.c5_hydrate_multipart('widening',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
 'widening','app','config_system_param','0',1,repeat('b',64),
 overlay_fixture.scope(ctx,'0',widening_fixture.rows('root'))->'members')
 FROM widening_fixture.saved WHERE label='root';
COMMIT;

BEGIN;
INSERT INTO widening_fixture.saved VALUES('overlay',parts_fixture.open('widening','config_system_param',widening_fixture.spec(1000)));
UPDATE widening_fixture.saved SET ctx=overlay_fixture.prepare(ctx,overlay_fixture.scope(ctx,'0',widening_fixture.rows('old-overlay')))
 WHERE label='overlay';
SELECT overlay_fixture.insert_rows(ctx,widening_fixture.rows('old-overlay')) FROM widening_fixture.saved WHERE label='overlay';
SELECT overlay_fixture.complete(ctx) FROM widening_fixture.saved WHERE label='overlay';
SELECT _oo.c5_scan_begin('widening',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
 'widening','app','config_system_param','old-scan','["0"]') FROM widening_fixture.saved WHERE label='overlay';
SELECT _oo.c5_observe_partial('widening',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
 'widening','app','config_system_param',2,jsonb_build_object('schema_fingerprint',repeat('a',64),
 'chunk_topology_revision',1,'checked_at_ms',floor(extract(epoch FROM clock_timestamp())*1000)::bigint,
 'scan_id','old-scan','expected_chunks','["0"]'::jsonb,'chunk',jsonb_build_object(
 'chunk_id','0','lo',NULL,'hi',NULL,'membership_revision',2,'row_count',388,'arrow_sha256',repeat('b',64),
 'digest',jsonb_build_array(388,repeat('0',16),repeat('1',16)),'digest_codec','mysql-md5-xor/v1',
 'projection_sha256',(SELECT projection_sha256 FROM _oo.c5_multipart_baseline WHERE source='widening'))))
 FROM widening_fixture.saved WHERE label='overlay';
SELECT _oo.publish_native_per_table_collection((ctx->>'run')::uuid,repeat('d',64),
 jsonb_build_object('protocol','oo.pipeline.native-publication-receipt/v1','collection_mode','per_table',
 'run_id',ctx->>'run','run_ids',jsonb_build_array(ctx->>'run'),'source_id','widening',
 'source_generation',2,'target_generation',(ctx->>'fence')::bigint,'consistency','per_table','consistency_grade','B',
 'cross_table_consistent',false,'position_codec','snapshot-zero/v1','position_lineage','fixture','applied_pos',0,
 'tables',jsonb_build_array(jsonb_build_object('run_id',ctx->>'run','schema','app','table','config_system_param',
 'source_generation',2,'target_generation',(ctx->>'fence')::bigint,'physical_name',ctx->>'physical',
 'accepted_rows',388,'canonical_digest',repeat('a',64),'applied_pos',0,
 'capture_method','c5_diff','delete_detection','reconcile','consistency_grade','B'))))
 FROM widening_fixture.saved WHERE label='overlay';
COMMIT;
-- Model an already-open target publication gate; not a public runtime claim.
CREATE ROLE widening_reader NOLOGIN;
CREATE SCHEMA oo_publish_target;
CREATE TABLE oo_publish_target.fence(schema_name text,relation_name text,role_name text,state text);
GRANT USAGE ON SCHEMA oo_publish_target TO oo_target_owner;
GRANT SELECT ON oo_publish_target.fence TO oo_target_owner;
GRANT USAGE ON SCHEMA _oo,overlay_fixture TO widening_reader;
INSERT INTO oo_publish_target.fence SELECT '_oo',ctx->>'physical','widening_reader','open'
 FROM widening_fixture.saved WHERE label='overlay';
SELECT format('GRANT SELECT ON _oo.%I TO widening_reader; GRANT EXECUTE ON FUNCTION _oo.%I() TO widening_reader',
 ctx->>'physical','pv_'||substr(ctx->>'physical',4)) FROM widening_fixture.saved WHERE label='overlay' \gexec
SELECT ctx->>'physical' AS old_view FROM widening_fixture.saved WHERE label='overlay' \gset
SET ROLE widening_reader;
SELECT overlay_fixture.ok(count(*)=388,'old published overlay readable before rebase') FROM _oo.:"old_view";
RESET ROLE;

BEGIN;
INSERT INTO widening_fixture.saved VALUES('new',parts_fixture.open('widening','config_system_param',widening_fixture.spec(3000)));
SELECT overlay_fixture.insert_rows(ctx,widening_fixture.rows('new-full')) FROM widening_fixture.saved WHERE label='new';
SET CONSTRAINTS ALL IMMEDIATE;
SELECT overlay_fixture.seal(ctx,388,false) FROM widening_fixture.saved WHERE label='new';
UPDATE widening_fixture.saved SET ctx=parts_fixture.claim(ctx,388) WHERE label='new';
UPDATE jw_native.occurrences SET manifest=jsonb_build_object('c5_state',jsonb_build_object(
 'schema','app','table','config_system_param','schema_fingerprint',repeat('b',64),
 'key_columns',jsonb_build_array('company_id','param_id')))
 WHERE task_id=(SELECT (ctx->>'task')::uuid FROM widening_fixture.saved WHERE label='new');
COMMIT;
CREATE FUNCTION widening_fixture.rebase(c jsonb,fp text DEFAULT repeat('b',64)) RETURNS jsonb LANGUAGE sql AS $$
 SELECT _oo.c5_rebase_widening_if_needed('widening',(c->>'owner')::uuid,(c->>'fence')::bigint,
  'widening','app','config_system_param',fp,3,ARRAY['company_id','param_id'],(c->>'task')::uuid,(c->>'job_generation')::bigint)
$$;
GRANT USAGE ON SCHEMA widening_fixture,parts_fixture,overlay_fixture TO c5_parts_test_runtime;
GRANT SELECT ON widening_fixture.saved TO c5_parts_test_runtime;
BEGIN;
SELECT parts_fixture.bind(ctx) FROM widening_fixture.saved WHERE label='new';
CREATE TEMP TABLE invariants AS SELECT
 (SELECT jsonb_agg(to_jsonb(w)) FROM _oo.source_watermark w) watermark,
 (SELECT jsonb_agg(to_jsonb(p)) FROM _oo.native_publication_receipt p) publication,
 (SELECT jsonb_agg(to_jsonb(f)) FROM oo_publish_target.fence f) gate,
 (SELECT jsonb_agg(to_jsonb(h)) FROM _oo.native_library_head h) library;
DO $$
DECLARE c jsonb; col text; stmt text; test integer; refused integer:=0;
BEGIN
 SELECT ctx INTO c FROM widening_fixture.saved WHERE label='new'; col:=c->'pcs'->>2;
 FOR test IN 1..7 LOOP
  BEGIN
   IF test=1 THEN stmt:=format('ALTER TABLE _oo.%I ALTER COLUMN %I TYPE varchar(500)',c->>'physical',col);
   ELSIF test=2 THEN stmt:=format('ALTER TABLE _oo.%I ALTER COLUMN %I TYPE varchar(6)',c->>'physical',c->'pcs'->>0);
   ELSIF test=3 THEN stmt:=format('ALTER TABLE _oo.%I ALTER COLUMN %I SET NOT NULL',c->>'physical',col);
   ELSIF test=4 THEN stmt:=format('ALTER TABLE _oo.%I ALTER COLUMN %I TYPE text',c->>'physical',col);
   ELSIF test=5 THEN stmt:=format('ALTER TABLE _oo.%I ADD COLUMN unexpected text',c->>'physical');
   ELSIF test=6 THEN stmt:=format('UPDATE _oo.full_import_table SET exhausted=false,canonical_digest=NULL WHERE run_id=%L',c->>'run');
   ELSE stmt:=format('UPDATE jw_native.occurrences SET manifest=jsonb_set(manifest,''{c5_state,table}'',''"other_table"'') WHERE task_id=%L',c->>'task'); END IF;
   EXECUTE stmt;
   PERFORM widening_fixture.rebase(c);
   RAISE EXCEPTION 'accepted invalid widening %',test;
  EXCEPTION WHEN SQLSTATE 'OO030' OR SQLSTATE 'P0002' THEN
   refused:=refused+1; RAISE NOTICE 'PASS refused widening case %',test;
  END;
 END LOOP;
 PERFORM overlay_fixture.ok(refused=7,'narrow/key/nullable/type/add/incomplete/other occurrence table refused');
 PERFORM overlay_fixture.ok(NOT EXISTS(SELECT FROM _oo.c5_widening_rebase),'failed widening leaves no archives');
END $$;
SET LOCAL ROLE c5_parts_test_runtime;
SELECT widening_fixture.rebase(ctx) AS rebase_result FROM widening_fixture.saved WHERE label='new';
SELECT overlay_fixture.ok(widening_fixture.rebase(ctx)->'rebased'='false'::jsonb,'replay is no-op')
 FROM widening_fixture.saved WHERE label='new';
RESET ROLE;
SELECT overlay_fixture.ok(watermark=(SELECT jsonb_agg(to_jsonb(w)) FROM _oo.source_watermark w)
 AND publication=(SELECT jsonb_agg(to_jsonb(p)) FROM _oo.native_publication_receipt p)
 AND gate=(SELECT jsonb_agg(to_jsonb(f)) FROM oo_publish_target.fence f)
 AND library IS NOT DISTINCT FROM (SELECT jsonb_agg(to_jsonb(h)) FROM _oo.native_library_head h),
 'watermark native publication fence and library unchanged') FROM invariants;
SELECT overlay_fixture.ok((SELECT count(*) FROM _oo.c5_widening_c5_member)=388
 AND (SELECT count(*) FROM _oo.c5_member WHERE source='widening')=0
 AND (SELECT source_generation=3 AND schema_fingerprint=repeat('b',64) FROM _oo.c5_table WHERE source='widening')
 AND (SELECT baseline_membership_pending FROM _oo.c5_chunk WHERE source='widening' AND chunk_id='0'),
 '388 old members archived and newer full cache awaits normal hydration');
SELECT overlay_fixture.ok(_oo.c5_overlay_is_valid(:'old_view'),'prior published overlay generic validity after rebase');
COMMIT;
SET ROLE widening_reader;
SELECT overlay_fixture.ok(count(*)=388,'old published overlay STILL readable after committed rebase') FROM _oo.:"old_view";
RESET ROLE;
SELECT overlay_fixture.ok((SELECT count(*) FROM _oo.native_publication_receipt)=1
 AND (SELECT count(*) FROM _oo.c5_block_receipt)=1
 AND NOT EXISTS(SELECT FROM jobs.tasks WHERE state='succeeded')
 AND NOT EXISTS(SELECT FROM jw_native.pending_acks),'no fake job success ACK or new publication');
SELECT 'PASS target-only widening rebase: actual 388-row 18-column full, prior published overlay preserved' AS result;
