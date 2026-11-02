-- Synthetic receipts against the actual 0000..0065 schema, not a source E2E.
BEGIN;
CREATE SCHEMA fixture;
CREATE TABLE fixture.checks(message text);
CREATE FUNCTION fixture.check(ok boolean,message text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN
 IF ok IS DISTINCT FROM true THEN RAISE EXCEPTION 'assertion: %',message; END IF;
 INSERT INTO fixture.checks VALUES(message);
END $$;
CREATE FUNCTION fixture.refuses(statement text,message text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN
 BEGIN
  EXECUTE statement;
 EXCEPTION WHEN OTHERS THEN
  IF SQLERRM NOT LIKE message||'%' THEN RAISE EXCEPTION 'expected %, got %',message,SQLERRM; END IF;
  INSERT INTO fixture.checks VALUES(message); RETURN;
 END;
 RAISE EXCEPTION 'expected refusal: %',message;
END $$;

-- Only fixture construction bypasses write triggers. Tested functions below
-- use the real fence/session/receipt checks, with triggers enabled.
SET LOCAL session_replication_role=replica;
DO $seed$
DECLARE physical text; id_col text; value_col text; run uuid:='22222222-2222-4222-8222-222222222222';
 chunks jsonb:='[]'; parts jsonb:='[]'; i integer; prior jsonb:='null'; hi jsonb; sha text;
BEGIN
 PERFORM _oo.register_physical_mapping('shared',repeat('a',64),3,'namespace','app','','',
  sha256(_oo.pm_payload('shared',repeat('a',64),3,'n','app','','')));
 PERFORM _oo.register_physical_mapping('shared',repeat('a',64),3,'table','app','older','',
  sha256(_oo.pm_payload('shared',repeat('a',64),3,'t','app','older','')));
 PERFORM _oo.register_physical_mapping('shared',repeat('a',64),3,'column','app','older','id',
  sha256(_oo.pm_payload('shared',repeat('a',64),3,'c','app','older','id')));
 PERFORM _oo.register_physical_mapping('shared',repeat('a',64),3,'column','app','older','value',
  sha256(_oo.pm_payload('shared',repeat('a',64),3,'c','app','older','value')));
 physical:=_oo.lookup_physical_mapping('shared',repeat('a',64),3,'table','app','older','');
 id_col:=_oo.lookup_physical_mapping('shared',repeat('a',64),3,'column','app','older','id');
 value_col:=_oo.lookup_physical_mapping('shared',repeat('a',64),3,'column','app','older','value');
 EXECUTE format('CREATE TABLE _oo.%I (_oo_pos numeric,%I bigint NOT NULL,%I text)',physical,id_col,value_col);
 EXECUTE format('ALTER TABLE _oo.%I OWNER TO oo_target_owner',physical);
 EXECUTE format('CREATE UNIQUE INDEX ON _oo.%I (%I)',physical,id_col);
 EXECUTE format('INSERT INTO _oo.%I SELECT 0,i,''value-''||i FROM generate_series(1,6) i',physical);
 -- Any baseline data SELECT by the initializer will now fail with 42501.
 EXECUTE format('REVOKE SELECT ON _oo.%I FROM oo_target_owner',physical);
 FOR i IN 0..2 LOOP
  hi:=jsonb_build_array(jsonb_build_object('Int',(i+1)*2));
  sha:=repeat((i+1)::text,64);
  chunks:=chunks||jsonb_build_array(jsonb_build_object('table','older','chunk_id',i,'lo',prior,'hi',hi,
   'rows',2,'bytes',200,'arrow_sha256',sha));
  parts:=parts||jsonb_build_array(jsonb_build_object('index',i,'total',3,'chunk_id',i,'rows',2,'arrow_sha256',sha));
  INSERT INTO _oo.apply_receipts(source,table_generation,txn_or_batch_id,config_version,config_digest,
   type_policy_version,type_digest,hook_digest,schema_digest,payload_digest,protocol_version,
   position_codec,position_lineage,source_epoch,applied_pos,partial_txn,cross_table_consistent,txn_complete,
   segment_bitmap,fence_shard,fence_generation)
  VALUES('shared',physical,'fixture.part.'||i,'config/v1',repeat('a',64),'type-policy/v1',repeat('b',64),
   repeat('c',64),repeat('d',64),sha,'batch/v1','snapshot-zero/v1','fixture',0,0,false,false,true,'','shared',1);
  INSERT INTO _oo.full_import_quality(run_id,physical_name,batch_id,kind,input_digest,accepted_rows,rejected_rows,payload_digest)
  VALUES(run,physical,'fixture.part.'||i,'batch',repeat('e',64),2,0,sha);
  prior:=hi;
 END LOOP;
 INSERT INTO _oo.full_import_run(run_id,shard_id,owner_token,target_generation,source_id,source_generation,
  config_digest,type_digest,input_digest,schema_digest,state,sealed_at,multipart_scope)
 VALUES(run,'shared','11111111-1111-4111-8111-111111111111',1,'shared',3,
  repeat('a',64),repeat('b',64),repeat('e',64),repeat('d',64),'awaiting_publish',clock_timestamp(),
  jsonb_build_object('protocol','oo.mysql.c5-multipart/v1','source_id','shared','source_generation',3,
   'occurrence','fixture','schema','app','table','older','schema_sha256',repeat('d',64),
   'capture_config_sha256',repeat('f',64),'manifest_sha256',repeat('f',64),'chunks',chunks,'part_count',3,'parts',parts));
 INSERT INTO _oo.full_import_table VALUES(run,0,'app','older',physical,true,6,0,repeat('e',64));
 INSERT INTO _oo.apply_receipts(source,table_generation,txn_or_batch_id,config_version,config_digest,
  type_policy_version,type_digest,hook_digest,schema_digest,payload_digest,protocol_version,
  position_codec,position_lineage,source_epoch,applied_pos,partial_txn,cross_table_consistent,txn_complete,
  segment_bitmap,fence_shard,fence_generation)
 VALUES('shared',physical,'fixture.final','config/v1',repeat('a',64),'type-policy/v1',repeat('b',64),
  repeat('c',64),repeat('d',64),repeat('f',64),'batch/v1','snapshot-zero/v1','fixture',0,0,false,false,true,'','shared',1);
 INSERT INTO _oo.full_import_quality(run_id,physical_name,batch_id,kind,input_digest,accepted_rows,rejected_rows,payload_digest)
 VALUES(run,physical,'fixture.final','batch',repeat('e',64),0,0,repeat('f',64));
 INSERT INTO _oo.update_batch VALUES('shared','app','older','fixture.final',repeat('f',64),run,1,3,physical,0,6);
 INSERT INTO _oo.table_freshness(schema_name,table_name,source,source_generation,target_generation,physical_name,applied_at,mode,state)
 VALUES('app','older','shared',3,1,physical,clock_timestamp(),'full','awaiting_publish');
 INSERT INTO _oo.source_watermark(source,consistency,consistency_grade,applied_pos,applied_at,state,
  cross_table_consistent,source_generation,target_generation,position_codec,position_lineage)
 VALUES('shared','per_table','B',0,clock_timestamp(),'lagging',false,7,1,'snapshot-zero/v1','fixture');
END $seed$;
SET LOCAL session_replication_role=origin;

CREATE FUNCTION fixture.initialize(high bigint DEFAULT 7,keys text[] DEFAULT ARRAY['id']) RETURNS jsonb
LANGUAGE sql AS $$
 SELECT _oo.c5_initialize_multipart_if_absent('shared','11111111-1111-4111-8111-111111111111',1,
  'shared','app','older',repeat('a',64),1,keys,high)
$$;

-- END SYNTHETIC BASELINE SETUP
SELECT fixture.refuses('SELECT fixture.initialize()','this backend does not hold the target lock');
SET LOCAL ROLE oo_pipeline_runtime;
SELECT _oo.adopt_target_generation('shared','11111111-1111-4111-8111-111111111111');
SELECT _oo.full_import_bind_session('shared','11111111-1111-4111-8111-111111111111',1);
RESET ROLE;
SELECT fixture.refuses('SELECT fixture.initialize(3)','C5 multipart source CAS differs');
SELECT fixture.refuses('SELECT fixture.initialize(7,ARRAY[''value''])','query returned no rows');
SELECT fixture.refuses('SELECT _oo.c5_initialize_full(''shared'',''11111111-1111-4111-8111-111111111111'',1,''shared'',''app'',''older'',repeat(''a'',64),1,''0'',ARRAY[''id''])',
 'C5 multipart baseline unsupported');

-- Each corruption is isolated in a subtransaction and must leave C5 empty.
DO $invalid$
DECLARE saved jsonb; invalid jsonb; changes jsonb;
BEGIN
 SELECT multipart_scope INTO saved FROM _oo.full_import_run;
 FOR changes IN SELECT value FROM jsonb_array_elements('[
  {"schema_sha256":"wrong"},{"source_generation":7},{"part_count":4},{"protocol":"wrong"}
 ]') LOOP
  BEGIN
   PERFORM set_config('session_replication_role','replica',true);
   UPDATE _oo.full_import_run SET multipart_scope=saved||changes;
   PERFORM set_config('session_replication_role','origin',true);
   PERFORM fixture.refuses('SELECT fixture.initialize()','C5 multipart');
   RAISE EXCEPTION 'undo' USING ERRCODE='Z0001';
  EXCEPTION WHEN SQLSTATE 'Z0001' THEN NULL;
  END;
 END LOOP;
 BEGIN
  PERFORM set_config('session_replication_role','replica',true);
  UPDATE _oo.apply_receipts SET payload_digest=repeat('0',64) WHERE txn_or_batch_id='fixture.part.1';
  PERFORM set_config('session_replication_role','origin',true);
  PERFORM fixture.refuses('SELECT fixture.initialize()','C5 multipart part receipt differs');
  RAISE EXCEPTION 'undo' USING ERRCODE='Z0001';
 EXCEPTION WHEN SQLSTATE 'Z0001' THEN NULL;
 END;
 BEGIN
  PERFORM set_config('session_replication_role','replica',true);
  DELETE FROM _oo.apply_receipts WHERE txn_or_batch_id='fixture.part.1';
  PERFORM set_config('session_replication_role','origin',true);
  PERFORM fixture.refuses('SELECT fixture.initialize()','C5 multipart receipt coverage differs');
  RAISE EXCEPTION 'undo' USING ERRCODE='Z0001';
 EXCEPTION WHEN SQLSTATE 'Z0001' THEN NULL;
 END;
 BEGIN
  PERFORM set_config('session_replication_role','replica',true);
  UPDATE _oo.full_import_quality SET accepted_rows=1 WHERE batch_id='fixture.final';
  PERFORM set_config('session_replication_role','origin',true);
  PERFORM fixture.refuses('SELECT fixture.initialize()','C5 multipart final receipt differs');
  RAISE EXCEPTION 'undo' USING ERRCODE='Z0001';
 EXCEPTION WHEN SQLSTATE 'Z0001' THEN NULL;
 END;
 BEGIN
  PERFORM set_config('session_replication_role','replica',true);
  DELETE FROM _oo.update_batch;
  PERFORM set_config('session_replication_role','origin',true);
  PERFORM fixture.refuses('SELECT fixture.initialize()','C5 multipart final receipt differs');
  RAISE EXCEPTION 'undo' USING ERRCODE='Z0001';
 EXCEPTION WHEN SQLSTATE 'Z0001' THEN NULL;
 END;
 PERFORM fixture.check(NOT EXISTS(SELECT FROM _oo.c5_table),'failed initializers are atomic');
END $invalid$;

SET LOCAL ROLE oo_pipeline_runtime;
SELECT _oo.c5_initialize_multipart_if_absent('shared','11111111-1111-4111-8111-111111111111',1,
 'shared','app','older',repeat('a',64),1,ARRAY['id'],7);
RESET ROLE;
SELECT fixture.check((SELECT count(*)=3 FROM _oo.c5_chunk),'three bounded chunks, not collapsed');
SELECT fixture.check((SELECT bool_and(baseline_membership_pending) FROM _oo.c5_chunk),'membership starts unknown');
SELECT fixture.check(NOT EXISTS(SELECT FROM _oo.c5_member),'zero eager membership inserts');
SELECT fixture.check((SELECT source_generation=3 FROM _oo.c5_table),'table generation stays B=3');
SELECT fixture.check((SELECT source_generation=7 FROM _oo.source_watermark),'global watermark stays H=7');
SELECT fixture.check((SELECT count(*)=4 FROM _oo.apply_receipts),'three part plus final ACK unchanged');
SELECT fixture.check((SELECT count(*)=1 FROM _oo.full_import_run),'no new data run');
SELECT fixture.check((SELECT lo='null' AND hi='[{"kind":"int","value":2}]' FROM _oo.c5_chunk WHERE chunk_id='0'),'first negative infinity');
SELECT fixture.check((SELECT lo='[{"kind":"int","value":2}]' AND hi='[{"kind":"int","value":4}]' FROM _oo.c5_chunk WHERE chunk_id='1'),'exact interior boundaries');
SELECT fixture.check((SELECT lo='[{"kind":"int","value":4}]' AND hi='null' FROM _oo.c5_chunk WHERE chunk_id='2'),'last positive infinity');
SELECT fixture.check((fixture.initialize()->>'created')='false','if-absent replay');
SELECT fixture.check(_oo.c5_authority_export('shared','app','older')->'source_generation'='7','export source CAS H');
SELECT fixture.check(_oo.c5_authority_export('shared','app','older')->'table_source_generation'='3','export physical B');
SELECT fixture.check(_oo.c5_authority_export('shared','app','older')->'selected_columns'='["id","value"]','trusted ordered projection');
SELECT fixture.check(_oo.c5_authority_export('shared','app','older')->'chunks'->0->'digest'='null','no fabricated source digest');
SELECT fixture.check(NOT has_table_privilege('oo_pipeline_runtime','_oo.c5_multipart_baseline','INSERT,UPDATE,DELETE'),'runtime has no baseline DML');
SELECT fixture.check(NOT has_function_privilege('oo_pipeline_runtime','_oo.c5_range_guard_before_multipart(text,uuid,bigint,jsonb,text[])','EXECUTE'),'old guard cannot bypass pending');
SELECT fixture.check(NOT has_function_privilege('oo_pipeline_runtime','_oo.c5_initialize_full_single(text,uuid,bigint,text,text,text,text,bigint,text,text[])','EXECUTE'),'0058 old helper remains private');
SELECT fixture.refuses($q$SELECT _oo.c5_range_guard('shared','11111111-1111-4111-8111-111111111111',1,
 '{"source_id":"shared","schema":"app","table":"older","chunk_id":"0"}',ARRAY[]::text[])$q$,
 'C5 multipart range scope');
SELECT fixture.refuses($q$SELECT _oo.c5_range_guard_after_seal(
 '{"source_id":"shared","schema":"app","table":"older","chunk_id":"0"}')$q$,
 'C5 multipart sealed parent differs');
SELECT fixture.refuses($q$SELECT _oo.c5_observe('shared','11111111-1111-4111-8111-111111111111',1,
 'shared','app','older',7,'{}')$q$,'C5 multipart baseline membership pending');

-- Typed conversion never applies PG text collation; exact source bytes survive.
SELECT fixture.check(_oo.c5_mysql_bound('[{"Int":-2},{"Text":[90,195,169]},{"Bytes":[0,255]}]',ARRAY['Int','Text','Bytes'])
 ='[{"kind":"int","value":-2},{"kind":"text","value":"5ac3a9"},{"kind":"bytes","value":"00ff"}]','mixed typed key bytes preserved');
SELECT fixture.refuses($q$SELECT _oo.c5_mysql_bound('[{"Int":9223372036854775808}]',ARRAY['Int'])$q$,'value');
SELECT fixture.refuses($q$SELECT _oo.c5_mysql_bound('[{"Int":1,"Text":[]}]',ARRAY['Int'])$q$,'C5 multipart typed bound');
SELECT fixture.refuses($q$SELECT _oo.c5_mysql_bound('[{"Bytes":[256]}]',ARRAY['Bytes'])$q$,'C5 multipart byte bound');
SELECT fixture.refuses($q$SELECT _oo.c5_mysql_bound('[{"Int":1}]',ARRAY['Text'])$q$,'C5 multipart typed bound');
SELECT fixture.refuses($q$SELECT _oo.c5_mysql_bound('[]',ARRAY['Int'])$q$,'C5 multipart bound width');
SELECT fixture.refuses($q$SELECT _oo.c5_multipart_topology(
 jsonb_set((SELECT multipart_scope FROM _oo.full_import_run),'{chunks,1,lo}','[{"Int":3}]'),ARRAY['Int'])$q$,
 'C5 multipart discontinuous bounds');
SELECT fixture.refuses($q$SELECT _oo.c5_multipart_topology(
 jsonb_set((SELECT multipart_scope FROM _oo.full_import_run),'{chunks,1,hi}','[{"Int":1}]'),ARRAY['Int'])$q$,
 'C5 multipart nonincreasing bounds');

SELECT fixture.refuses($q$SELECT _oo.c5_scan_begin('shared','11111111-1111-4111-8111-111111111111',1,
 'shared','app','older','scan1','["1","0","2"]')$q$,'C5 multipart complete ordered scan required');
SELECT fixture.check(_oo.c5_scan_begin('shared','11111111-1111-4111-8111-111111111111',1,
 'shared','app','older','scan1','["0","1","2"]')=0,'scan starts with no confirmed prefix');
-- Simulate the old commit cursor advance without a digest observation.
UPDATE _oo.c5_chunk SET last_scan_id='scan1' WHERE chunk_id='0';
UPDATE _oo.c5_scan SET next_block=1;
SELECT fixture.check(_oo.c5_authority_export('shared','app','older')->'next_chunk_index'='0','export cannot skip applied-only prefix');
SELECT fixture.check(_oo.c5_scan_begin('shared','11111111-1111-4111-8111-111111111111',1,
 'shared','app','older','scan1','["0","1","2"]')=0,'begin_scan repairs applied-only cursor');

CREATE FUNCTION fixture.observation(chunk text) RETURNS jsonb LANGUAGE sql AS $$
 SELECT jsonb_build_object('scan_id','scan1','schema_fingerprint',repeat('a',64),'chunk_topology_revision',1,
  'checked_at_ms',floor(extract(epoch FROM clock_timestamp())*1000)::bigint,
  'chunk',jsonb_build_object('chunk_id',c.chunk_id,'lo',c.lo,'hi',c.hi,'membership_revision',c.membership_revision,
   'arrow_sha256',c.arrow_sha256,'row_count',c.row_count,'digest',jsonb_build_array(c.row_count,'0000000000000001','0000000000000002'),
   'digest_codec','mysql-md5-xor/v1','projection_sha256',b.projection_sha256))
 FROM _oo.c5_chunk c JOIN _oo.c5_multipart_baseline b USING(source,schema_name,table_name) WHERE c.chunk_id=chunk
$$;
CREATE FUNCTION fixture.observe(chunk text) RETURNS jsonb LANGUAGE sql AS $$
 SELECT _oo.c5_observe_partial('shared','11111111-1111-4111-8111-111111111111',1,
 'shared','app','older',7,fixture.observation(chunk))
$$;
SELECT fixture.refuses($q$SELECT fixture.observe('0')$q$,'C5 partial observation differs from confirmed membership');
DO $missing_proof$
BEGIN
 BEGIN
  UPDATE _oo.c5_chunk SET baseline_membership_pending=false WHERE chunk_id='0';
  PERFORM fixture.refuses($q$SELECT fixture.observe('0')$q$,'C5 partial observation block receipt required');
  RAISE EXCEPTION 'undo' USING ERRCODE='Z0001';
 EXCEPTION WHEN SQLSTATE 'Z0001' THEN NULL;
 END;
END $missing_proof$;
CREATE FUNCTION fixture.hydrate(chunk text,members jsonb) RETURNS jsonb LANGUAGE sql AS $$
 SELECT _oo.c5_hydrate_multipart('shared','11111111-1111-4111-8111-111111111111',1,
  'shared','app','older',chunk,7,arrow_sha256,members) FROM _oo.c5_chunk WHERE chunk_id=chunk
$$;
SELECT fixture.refuses($q$SELECT _oo.c5_hydrate_multipart('shared','11111111-1111-4111-8111-111111111111',1,
 'shared','app','older','0',7,repeat('0',64),'[[{"kind":"int","value":1}],[{"kind":"int","value":2}]]')$q$,
 'C5 hydration part identity differs');
SELECT fixture.refuses($q$SELECT fixture.hydrate('0','[[{"kind":"int","value":1}]]')$q$,
 'C5 hydration complete unique membership required');
SELECT fixture.refuses($q$SELECT fixture.hydrate('0','[[{"kind":"int","value":1}],[{"kind":"int","value":1}]]')$q$,
 'C5 hydration complete unique membership required');
SELECT fixture.refuses($q$SELECT fixture.hydrate('0','[[{"kind":"text","value":"31"}],[{"kind":"text","value":"32"}]]')$q$,
 'C5 hydration key type');
SELECT fixture.check(fixture.hydrate('0','[[{"kind":"int","value":1}],[{"kind":"int","value":2}]]')->'hydrated'='true',
 'receipted first part hydrated successfully');
SELECT fixture.check((SELECT count(*)=2 FROM _oo.c5_member),'hydrate only bounded first part');
SELECT fixture.check((SELECT count(*)=2 FROM _oo.c5_chunk WHERE baseline_membership_pending),'other hydration progress untouched');
SELECT fixture.check(fixture.hydrate('0','[[{"kind":"int","value":1}],[{"kind":"int","value":2}]]')->'replay'='true',
 'hydration exact replay');
SELECT fixture.refuses($q$SELECT fixture.hydrate('0','[[{"kind":"int","value":1}],[{"kind":"int","value":3}]]')$q$,
 'C5 hydration replay differs');
SELECT fixture.refuses($q$SELECT fixture.hydrate('1','[[{"kind":"int","value":1}],[{"kind":"int","value":3}]]')$q$,
 'C5 hydration existing membership differs');
SELECT fixture.check(fixture.hydrate('1','[[{"kind":"int","value":3}],[{"kind":"int","value":4}]]')->'hydrated'='true',
 'second part hydrated');
SELECT fixture.check(fixture.hydrate('2','[[{"kind":"int","value":5}],[{"kind":"int","value":6}]]')->'hydrated'='true',
 'last part hydrated');
SELECT fixture.check((SELECT count(*)=6 AND count(DISTINCT loader_key)=6 FROM _oo.c5_member),'exact bounded membership seeded');
SELECT fixture.check(NOT EXISTS(SELECT FROM _oo.c5_chunk WHERE baseline_membership_pending),'all pending memberships complete');
SELECT fixture.check((SELECT count(*)=4 FROM _oo.apply_receipts),'hydration does not manufacture apply receipts');
SELECT fixture.check(_oo.c5_authority_export('shared','app','older')->'chunks'->0->'digest'='null','hydration does not guess source digest');
-- Exercise the real guard -> after-seal -> commit functions with B=3, H=7.
-- The candidate receipt is fixture-owned; rolling it back preserves the base
-- for the independent partial-observation checks that follow.
DO $shared_generation$
DECLARE scope jsonb; missing text[]; parent text; child text:='pt_'||repeat('8',60);
 run uuid:='88888888-8888-4888-8888-888888888888'; succeeded boolean:=false;
BEGIN
 scope:=jsonb_build_object('protocol','oo.pipeline.native-c5-range/v1','source_engine','mysql','capture_method','c5_diff',
  'source_id','shared','schema','app','table','older','schema_fingerprint',repeat('a',64),'chunk_topology_revision',1,
  'chunk_id','0','lo','null'::jsonb,'hi','[{"kind":"int","value":2}]'::jsonb,
  'prior_membership_revision',1,'parent_generation',7,'source_generation',8,'arrow_sha256',repeat('6',64),
  'row_count',2,'delete_detection','reconcile','position_codec','snapshot-zero/v1','applied_pos','0',
  'members','[[{"kind":"int","value":0}],[{"kind":"int","value":2}]]'::jsonb);
 BEGIN
  missing:=_oo.c5_range_guard('shared','11111111-1111-4111-8111-111111111111',1,scope,ARRAY['0','2']);
  PERFORM fixture.check(missing=ARRAY['1'],'shared source range detects exact removed member');
  PERFORM fixture.refuses(format('SELECT _oo.c5_range_guard_after_seal(%L::jsonb)',scope),'C5 multipart sealed parent differs');
  SELECT physical_name INTO parent FROM _oo.table_freshness;
  EXECUTE format('CREATE TABLE _oo.%I (id bigint)',child);
  EXECUTE format('ALTER TABLE _oo.%I OWNER TO oo_target_owner',child);
  PERFORM set_config('session_replication_role','replica',true);
  INSERT INTO _oo.full_import_run(run_id,shard_id,owner_token,target_generation,source_id,source_generation,
   config_digest,type_digest,input_digest,schema_digest,state,sealed_at)
  VALUES(run,'shared','11111111-1111-4111-8111-111111111111',1,'shared',8,repeat('a',64),repeat('b',64),repeat('6',64),
   repeat('d',64),'awaiting_publish',clock_timestamp());
  INSERT INTO _oo.full_import_table VALUES(run,0,'app','older',child,true,6,0,repeat('e',64));
  INSERT INTO _oo.apply_receipts(source,table_generation,txn_or_batch_id,config_version,config_digest,
   type_policy_version,type_digest,hook_digest,schema_digest,payload_digest,protocol_version,
   position_codec,position_lineage,source_epoch,applied_pos,partial_txn,cross_table_consistent,txn_complete,
   segment_bitmap,fence_shard,fence_generation)
  VALUES('shared',child,'fixture.changed','config/v1',repeat('a',64),'type-policy/v1',repeat('b',64),
   repeat('c',64),repeat('d',64),repeat('9',64),'batch/v1','snapshot-zero/v1','fixture',0,0,false,false,true,'','shared',1);
  INSERT INTO _oo.update_batch VALUES('shared','app','older','fixture.changed',repeat('9',64),run,1,8,child,1,6);
  UPDATE _oo.table_freshness SET source_generation=8,physical_name=child,mode='incremental';
  UPDATE _oo.source_watermark SET source_generation=8;
  PERFORM set_config('session_replication_role','origin',true);
  PERFORM _oo.c5_range_guard_after_seal(scope);
  PERFORM fixture.refuses(format('SELECT _oo.c5_range_commit(''shared'',''11111111-1111-4111-8111-111111111111'',1,%L::jsonb,
   ARRAY[''forged'',''2''],%L::uuid,''fixture.changed'',repeat(''9'',64))',scope,run),'C5 multipart commit keys differ');
  PERFORM _oo.c5_range_commit('shared','11111111-1111-4111-8111-111111111111',1,scope,ARRAY['0','2'],run,'fixture.changed',repeat('9',64));
  PERFORM _oo.c5_observation_receipt_guard('shared','11111111-1111-4111-8111-111111111111',1,scope,
   jsonb_build_object('protocol','oo.pipeline.native-update-result/v1','source_id','shared','logical_schema','app','logical_table','older',
    'run_id',run,'source_generation',8,'target_generation',1,'physical_name',child,'payload_digest',repeat('9',64)));
  PERFORM fixture.refuses(format('SELECT _oo.c5_observation_receipt_guard(''shared'',''11111111-1111-4111-8111-111111111111'',1,%L::jsonb,''{}'')',scope),
   'C5 observation Loader receipt differs');
  PERFORM fixture.check((SELECT source_generation=8 AND input_digest=repeat('6',64) FROM _oo.c5_table),'commit writes real new table generation');
  PERFORM fixture.check(_oo.c5_authority_export('shared','app','older')->'table_source_generation'='8','committed authority binds new table');
  PERFORM fixture.check(NOT EXISTS(SELECT FROM _oo.c5_member WHERE loader_key='1')
   AND EXISTS(SELECT FROM _oo.c5_member WHERE loader_key='0'),'commit reconciles hydrated membership');
  PERFORM fixture.check((SELECT last_digest_scan_id IS NULL FROM _oo.c5_chunk WHERE chunk_id='0'),'commit does not confirm digest');
  succeeded:=true;
  RAISE EXCEPTION 'undo' USING ERRCODE='Z0001';
 EXCEPTION WHEN SQLSTATE 'Z0001' THEN NULL;
 END;
 PERFORM fixture.check(succeeded,'B=3 H=7 guarded sealed commit to real table generation 8');
 PERFORM fixture.check((SELECT source_generation=3 FROM _oo.c5_table),'fixture rollback retains physical baseline');
END $shared_generation$;
SELECT fixture.refuses($q$SELECT fixture.observe('1')$q$,'C5 partial observation skips unconfirmed prefix');
SELECT fixture.check(fixture.observe('0')->'next_chunk_index'='1','confirmed first partial persists');
SELECT fixture.check((SELECT last_full_digest_scan_completed_at IS NULL FROM _oo.c5_table),'partial does not declare full scan');
SELECT fixture.check(fixture.observe('0')->'next_chunk_index'='1','partial replay is idempotent');
SELECT fixture.refuses($q$SELECT _oo.c5_observe_partial('shared','11111111-1111-4111-8111-111111111111',1,
 'shared','app','older',7,jsonb_set(fixture.observation('0'),'{chunk,digest,1}','"0000000000000003"'))$q$,
 'C5 nochange observation digest differs');
SELECT fixture.refuses($q$SELECT _oo.c5_observe_partial('shared','11111111-1111-4111-8111-111111111111',1,
 'shared','app','older',6,fixture.observation('0'))$q$,'C5 partial observation scope differs');
SELECT fixture.check(fixture.observe('1')->'next_chunk_index'='2','second partial persists');
SELECT fixture.check(fixture.observe('2')->'full_digest_scan_completed'='true','last confirmed partial closes full scan');
SELECT fixture.check((SELECT status='completed' AND next_block=3 FROM _oo.c5_scan),'durable full scan proof');
SELECT fixture.check((SELECT last_full_digest_scan_completed_at IS NOT NULL FROM _oo.c5_table),'full scan timestamp persists');
SELECT fixture.check((SELECT source_generation=3 FROM _oo.c5_table),'observations do not fake table generation');
SELECT fixture.check((SELECT source_generation=7 FROM _oo.source_watermark),'observations do not advance source');
SELECT fixture.check(fixture.observe('2')->'full_digest_scan_completed'='true','completion replay');
-- A newer membership cannot reuse an old source digest or completion prefix.
UPDATE _oo.c5_chunk SET membership_revision=membership_revision+1 WHERE chunk_id='0';
SELECT fixture.check(_oo.c5_authority_export('shared','app','older')->'chunks'->0->'digest'='null','stale digest masked after membership change');
SELECT fixture.check((SELECT last_full_digest_scan_completed_at IS NULL FROM _oo.c5_table),'membership change invalidates full scan proof');
SELECT fixture.refuses($q$SELECT _oo.c5_scan_begin('shared','11111111-1111-4111-8111-111111111111',1,
 'shared','app','older','scan1','["0","1","2"]')$q$,'C5 completed scan changed');
SELECT count(*) AS passed_assertions FROM fixture.checks;
ROLLBACK;
