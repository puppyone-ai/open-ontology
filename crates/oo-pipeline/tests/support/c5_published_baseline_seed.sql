-- Owned synthetic sealed receipts; real mappings and target metadata.
CREATE SCHEMA fixture69;
CREATE FUNCTION fixture69.seed(p_table text,p_generation bigint,p_multipart boolean,p_empty boolean DEFAULT false) RETURNS void
LANGUAGE plpgsql AS $$
DECLARE source text:='shared_published'; physical text; cols text[]:=ARRAY[]::text[];
 names text[]:=CASE WHEN p_multipart THEN ARRAY['b','a','value'] ELSE ARRAY['id','value'] END;
 run uuid:=md5(p_table)::uuid; i integer; total integer:=CASE WHEN p_multipart THEN 3 ELSE 1 END;
 row_total bigint:=CASE WHEN p_empty THEN 0 WHEN p_multipart THEN 6 ELSE 2 END;
 parts jsonb:='[]'; chunks jsonb:='[]'; low jsonb:='null'; high jsonb; sha text;
BEGIN
 PERFORM _oo.register_physical_mapping(source,repeat('a',64),p_generation,'namespace','app','','',
  sha256(_oo.pm_payload(source,repeat('a',64),p_generation,'n','app','','')));
 PERFORM _oo.register_physical_mapping(source,repeat('a',64),p_generation,'table','app',p_table,'',
  sha256(_oo.pm_payload(source,repeat('a',64),p_generation,'t','app',p_table,'')));
 physical:=_oo.lookup_physical_mapping(source,repeat('a',64),p_generation,'table','app',p_table,'');
 FOR i IN 1..cardinality(names) LOOP
  PERFORM _oo.register_physical_mapping(source,repeat('a',64),p_generation,'column','app',p_table,names[i],
   sha256(_oo.pm_payload(source,repeat('a',64),p_generation,'c','app',p_table,names[i])));
  cols:=array_append(cols,_oo.lookup_physical_mapping(source,repeat('a',64),p_generation,'column','app',p_table,names[i]));
 END LOOP;
 IF p_multipart THEN
  EXECUTE format('CREATE TABLE _oo.%I (_oo_pos numeric,%I bigint NOT NULL,%I bigint NOT NULL,%I text)',physical,cols[1],cols[2],cols[3]);
  EXECUTE format('CREATE UNIQUE INDEX ON _oo.%I (%I,%I)',physical,cols[1],cols[2]);
  EXECUTE format('INSERT INTO _oo.%I VALUES (0,20,1,''v1''),(0,10,2,''v2''),(0,40,3,''v3''),(0,30,4,''v4''),(0,60,5,''v5''),(0,50,6,''v6'')',physical);
 ELSE
  -- The historical single Loader creates no unique index. Archive hydration
  -- must prove its complete pinned-key set instead of assuming an index exists.
  EXECUTE format('CREATE TABLE _oo.%I (_oo_pos numeric,%I bigint NOT NULL,%I text)',physical,cols[1],cols[2]);
  IF NOT p_empty THEN EXECUTE format('INSERT INTO _oo.%I VALUES (0,1,''v1''),(0,2,''v2'')',physical); END IF;
 END IF;
 EXECUTE format('ALTER TABLE _oo.%I OWNER TO oo_target_owner',physical);
 EXECUTE format('REVOKE SELECT ON _oo.%I FROM oo_target_owner',physical);
 FOR i IN 0..total-1 LOOP
  sha:=repeat((i+1)::text,64);
  IF p_multipart THEN
   high:=jsonb_build_array(jsonb_build_object('Int',(i+1)*2),jsonb_build_object('Int',(i*2+1)*10));
   parts:=parts||jsonb_build_array(jsonb_build_object('index',i,'total',total,'chunk_id',i,'rows',2,'arrow_sha256',sha));
   chunks:=chunks||jsonb_build_array(jsonb_build_object('table',p_table,'chunk_id',i,'lo',low,'hi',high,'rows',2,'bytes',200,'arrow_sha256',sha));
   low:=high;
  END IF;
  INSERT INTO _oo.apply_receipts(source,table_generation,txn_or_batch_id,config_version,config_digest,type_policy_version,
   type_digest,hook_digest,schema_digest,payload_digest,protocol_version,position_codec,position_lineage,source_epoch,
   applied_pos,partial_txn,cross_table_consistent,txn_complete,segment_bitmap,fence_shard,fence_generation)
  VALUES(source,physical,p_table||'.part.'||i,'config/v1',repeat('a',64),'type-policy/v1',repeat('b',64),
   repeat('c',64),repeat('d',64),sha,'batch/v1','snapshot-zero/v1','fixture',0,0,false,false,true,'','shared_published',1);
  INSERT INTO _oo.full_import_quality(run_id,physical_name,batch_id,kind,input_digest,accepted_rows,rejected_rows,payload_digest)
  VALUES(run,physical,p_table||'.part.'||i,'batch',repeat('e',64),CASE WHEN p_empty THEN 0 ELSE 2 END,0,sha);
 END LOOP;
 INSERT INTO _oo.full_import_run(run_id,shard_id,owner_token,target_generation,source_id,source_generation,
  config_digest,type_digest,input_digest,schema_digest,state,sealed_at,multipart_scope)
 VALUES(run,'shared_published','11111111-1111-4111-8111-111111111111',1,source,p_generation,
  repeat('a',64),repeat('b',64),repeat('e',64),repeat('d',64),'awaiting_publish',clock_timestamp(),
  CASE WHEN p_multipart THEN jsonb_build_object('protocol','oo.mysql.c5-multipart/v1','source_id',source,
   'source_generation',p_generation,'occurrence',p_table,'schema','app','table',p_table,
   'schema_sha256',repeat('d',64),'capture_config_sha256',repeat('f',64),'manifest_sha256',repeat('f',64),
   'chunks',chunks,'part_count',total,'parts',parts) ELSE NULL END);
 INSERT INTO _oo.full_import_table VALUES(run,0,'app',p_table,physical,true,row_total,0,repeat('e',64));
 IF p_multipart THEN
  INSERT INTO _oo.apply_receipts(source,table_generation,txn_or_batch_id,config_version,config_digest,type_policy_version,
   type_digest,hook_digest,schema_digest,payload_digest,protocol_version,position_codec,position_lineage,source_epoch,
   applied_pos,partial_txn,cross_table_consistent,txn_complete,segment_bitmap,fence_shard,fence_generation)
  VALUES(source,physical,p_table||'.final','config/v1',repeat('a',64),'type-policy/v1',repeat('b',64),
   repeat('c',64),repeat('d',64),repeat('f',64),'batch/v1','snapshot-zero/v1','fixture',0,0,false,false,true,'','shared_published',1);
  INSERT INTO _oo.full_import_quality(run_id,physical_name,batch_id,kind,input_digest,accepted_rows,rejected_rows,payload_digest)
  VALUES(run,physical,p_table||'.final','batch',repeat('e',64),0,0,repeat('f',64));
 END IF;
 INSERT INTO _oo.update_batch VALUES(source,'app',p_table,
  p_table||CASE WHEN p_multipart THEN '.final' ELSE '.part.0' END,
  CASE WHEN p_multipart THEN repeat('f',64) ELSE repeat('1',64) END,run,1,p_generation,physical,0,row_total);
 INSERT INTO _oo.table_freshness(schema_name,table_name,source,source_generation,target_generation,physical_name,applied_at,mode,state)
 VALUES('app',p_table,source,p_generation,1,physical,clock_timestamp(),'full','awaiting_publish');
END $$;

BEGIN;
SET LOCAL session_replication_role=replica;
SELECT fixture69.seed('single_old',3,false);
SELECT fixture69.seed('composite',7,true);
SELECT fixture69.seed('single_empty',1,false,true);
INSERT INTO _oo.source_watermark(source,consistency,consistency_grade,applied_pos,applied_at,state,cross_table_consistent,
 source_generation,target_generation,position_codec,position_lineage)
VALUES('shared_published','per_table','B',0,clock_timestamp(),'lagging',false,7,1,'snapshot-zero/v1','fixture');
COMMIT;
