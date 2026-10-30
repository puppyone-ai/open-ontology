\set ON_ERROR_STOP on
\timing on
-- TARGET-ONLY native result envelope reconstructed from actual SQL receipts.
-- The observation digest is synthetic, NOT a source-admission measurement.
UPDATE parts_fixture.saved saved SET ctx=ctx||jsonb_build_object('loader_receipt',(
 SELECT jsonb_build_object('protocol','oo.pipeline.native-update-result/v1','receipt','inserted',
  'source_id',b.source,'source_engine','mysql','logical_schema',b.schema_name,'logical_table',b.table_name,
  'run_id',b.run_id::text,'source_generation',b.source_generation,'target_generation',b.target_generation,
  'physical_name',b.physical_name,'payload_digest',b.payload_digest,'row_count',b.row_count,
  'tombstone_count',b.tombstone_count,
  'jobs_lease_receipt',jsonb_build_object('id',j.id::text,'generation',j.generation,
   'project_id',j.project_id::text,'owner',j.lease_owner,'state',j.state,
   'claim','jobs.claim','target_fence_certified',false,'inserted',false))
 FROM _oo.update_batch b JOIN jobs.tasks j ON j.id=(saved.ctx->>'task')::uuid
 WHERE b.run_id=(saved.ctx->>'run')::uuid))
 WHERE label='hydrated_text';
BEGIN;
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='hydrated_text';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT parts_fixture.ok(_oo.c5_parts_rebind(ctx->>'source',(ctx->>'task')::uuid,
 (ctx->'loader_receipt'->'jobs_lease_receipt'->>'generation')::bigint)
 = (ctx->'loader_receipt'->>'tombstone_count')::bigint,
 'committed replay rebind returns actual deleted row count')
 FROM parts_fixture.saved WHERE label='hydrated_text';
SELECT _oo.c5_parts_observation_receipt_guard(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
 ctx->'scope',ctx->'loader_receipt') FROM parts_fixture.saved WHERE label='hydrated_text';
SELECT parts_fixture.ok(true,'parts observation accepts actual stored block and native Loader receipt');
DO $$
DECLARE c jsonb; receipt jsonb; command text;
BEGIN
 SELECT ctx INTO c FROM parts_fixture.saved WHERE label='hydrated_text'; receipt:=c->'loader_receipt';
 command:=format('SELECT _oo.c5_parts_observation_receipt_guard(%L,%L,%s,%L,%%L)',
  c->>'source',c->>'owner',c->>'fence',c->'scope');
 PERFORM parts_fixture.reject(format(command,jsonb_set(receipt,'{payload_digest}',to_jsonb(repeat('e',64)))),
  'OO030','observation wrong payload refused');
 PERFORM parts_fixture.reject(format(command,jsonb_set(receipt,'{row_count}',c->'scope'->'row_count')),
  'OO030','observation cannot substitute range count for total table count');
 PERFORM parts_fixture.reject(format(command,jsonb_set(receipt,'{tombstone_count}','0')),
  'OO030','observation false tombstone count refused');
 PERFORM parts_fixture.reject(format(command,jsonb_set(receipt,'{jobs_lease_receipt,generation}','999')),
  'OO030','observation wrong real job generation refused');
 PERFORM parts_fixture.reject(format(command,jsonb_set(receipt,'{jobs_lease_receipt,id}',to_jsonb(gen_random_uuid()::text))),
  'OO030','observation foreign parts task refused');
 PERFORM parts_fixture.reject(format(command,jsonb_set(receipt,'{source_generation}','999')),
  'OO030','observation forged source generation refused');
 PERFORM parts_fixture.reject(format(command,jsonb_set(receipt,'{physical_name}','"pt_foreign"')),
  'OO030','observation foreign shadow refused');
 PERFORM parts_fixture.reject(format(command,receipt-'receipt'),'OO030','observation missing Loader receipt refused');
END $$;
RESET ROLE;
DO $$
DECLARE c jsonb;
BEGIN
 SELECT ctx INTO c FROM parts_fixture.saved WHERE label='hydrated_text';
 BEGIN
  DELETE FROM _oo.c5_block_receipt WHERE run_id=(c->>'run')::uuid;
  PERFORM _oo.c5_parts_observation_receipt_guard(c->>'source',(c->>'owner')::uuid,(c->>'fence')::bigint,c->'scope',c->'loader_receipt');
  RAISE EXCEPTION 'missing block receipt accepted';
 EXCEPTION WHEN SQLSTATE 'OO030' THEN RAISE NOTICE 'PASS: missing actual parts block receipt refused'; END;
 BEGIN
  -- Native receipts are immutable even to the fixture administrator.
  -- Point the block at a nonexistent batch instead of disabling their guard.
  UPDATE _oo.c5_block_receipt SET batch_id=batch_id||'.missing' WHERE run_id=(c->>'run')::uuid;
  PERFORM _oo.c5_parts_observation_receipt_guard(c->>'source',(c->>'owner')::uuid,(c->>'fence')::bigint,c->'scope',c->'loader_receipt');
  RAISE EXCEPTION 'missing native receipt accepted';
 EXCEPTION WHEN SQLSTATE 'OO030' THEN RAISE NOTICE 'PASS: missing actual native receipt refused'; END;
END $$;
COMMIT;

-- Real 0065/0073 observe_partial accepts the new scope already stored by commit.
UPDATE parts_fixture.saved saved SET ctx=ctx||jsonb_build_object('observation',(
 SELECT jsonb_build_object('schema_fingerprint',t.schema_fingerprint,'chunk_topology_revision',t.topology_revision,
  'checked_at_ms',floor(extract(epoch FROM clock_timestamp())*1000)::bigint,
  'scan_id',NULL,'expected_chunks',NULL,'full_digest_scan_completed',false,
  'chunk',jsonb_build_object('chunk_id',ch.chunk_id,'lo',ch.lo,'hi',ch.hi,
   'membership_revision',ch.membership_revision,'arrow_sha256',ch.arrow_sha256,'row_count',ch.row_count,
   'digest',jsonb_build_array(ch.row_count,repeat('0',16),repeat('1',16)),
   'digest_codec','mysql-md5-xor/v1','projection_sha256',baseline.projection_sha256))
 FROM _oo.c5_table t JOIN _oo.c5_chunk ch USING(source,schema_name,table_name)
  JOIN _oo.c5_multipart_baseline baseline USING(source,schema_name,table_name)
 WHERE t.source=ctx->>'source' AND t.schema_name='app' AND t.table_name=ctx->>'table' AND ch.chunk_id='0'))
 WHERE label='hydrated_text';
BEGIN;
SELECT parts_fixture.bind(ctx) FROM parts_fixture.saved WHERE label='hydrated_text';
SET LOCAL ROLE c5_parts_test_runtime;
SELECT _oo.c5_parts_observation_receipt_guard(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
 ctx->'scope',ctx->'loader_receipt') FROM parts_fixture.saved WHERE label='hydrated_text';
SELECT parts_fixture.ok(_oo.c5_observe_partial(ctx->>'source',(ctx->>'owner')::uuid,(ctx->>'fence')::bigint,
 ctx->>'source','app',ctx->>'table',(ctx->>'generation')::bigint,ctx->'observation')->'data_gen'='false'::jsonb,
 'existing observe_partial accepts receipted parts scope without generation or ACK')
 FROM parts_fixture.saved WHERE label='hydrated_text';
COMMIT;
SELECT parts_fixture.ok(NOT EXISTS(SELECT FROM jw_native.pending_acks)
 AND NOT EXISTS(SELECT FROM _oo.full_import_run WHERE state='published'),
 'parts observation does not publish or acknowledge');
\echo 'PASS: parts observation receipt guard and unchanged observe_partial target-only'
