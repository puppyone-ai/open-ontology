\set ON_ERROR_STOP on
-- Anonymous C1 bridge fixture, not a MySQL source or an invented publication row.
CREATE SCHEMA native_business_fixture;
CREATE TABLE native_business_fixture.saved(generation bigint PRIMARY KEY,task uuid,receipt jsonb,context jsonb);
CREATE FUNCTION native_business_fixture.load(g bigint,n bigint) RETURNS jsonb
LANGUAGE plpgsql AS $fixture$
DECLARE
 s text:='native_business_fixture'; own uuid:='11111111-1111-4111-8111-111111111111';
 project uuid:='22222222-2222-4222-8222-222222222222'; server_id text:='33333333-3333-4333-8333-333333333333';
 claimed jobs.lease_result; queued jobs.lease_result; fence bigint; epoch integer; prior uuid;
 cfg text:=repeat('a',64); typ text:=repeat('b',64); inp text:=repeat('c',64); global_schema text:=repeat('d',64); ddl text:=repeat('e',64);
 tables text[]:=ARRAY['customer','independent_counts']; cols text[]; pcs text[]; t text; c text; physical text; ns text; run uuid;
 count_rows bigint; rows jsonb; entry jsonb; entries jsonb:='[]'; receipts jsonb:='[]'; mappings jsonb:='[]';
 scope jsonb; source_instance jsonb; receipt jsonb; sha text; result jsonb; pos numeric:=10; i integer;
BEGIN
 IF n<1 OR n>100000 OR g<1 THEN RAISE EXCEPTION 'fixture bounds'; END IF;
 queued:=jobs.enqueue(project,'native_business_fixture',1,'{}','test',cfg,'load-'||g,clock_timestamp(),3);
 claimed:=jobs.claim(project,ARRAY['native_business_fixture'],'fixture');
 IF claimed.status<>'claimed' OR claimed.task_id<>queued.task_id THEN RAISE EXCEPTION 'fixture lease'; END IF;
 fence:=_oo.adopt_target_generation(s,own);
 PERFORM _oo.full_import_bind_session(s,own,fence);
 IF g=1 THEN
   source_instance:=_oo.native_source_instance_register(s,own,claimed.generation,s,'mysql','mysql.'||server_id,cfg,global_schema,claimed.task_id);
   epoch:=(source_instance->>'source_epoch')::integer;
   PERFORM _oo.native_c1_baseline_open(s,claimed.task_id,claimed.generation,epoch,'mysql.'||server_id,cfg,global_schema,server_id||':1-10',10);
   SELECT jsonb_agg(jsonb_build_object('schema','public','table','customer',
       'key',jsonb_build_object('id',jsonb_build_object('type','int','value',k::text)),
       'value',jsonb_build_object('id',jsonb_build_object('type','int','value',k::text),'secret',jsonb_build_object('type','int','value','777'))))
     INTO rows FROM generate_series(1,n) k;
   PERFORM _oo.native_c1_baseline_fill(s,claimed.task_id,claimed.generation,epoch,rows);
   PERFORM _oo.native_c1_baseline_fill(s,claimed.task_id,claimed.generation,epoch,jsonb_build_array(
       jsonb_build_object('schema','public','table','independent_counts','key',jsonb_build_object('n',jsonb_build_object('type','int','value',n::text)),
       'value',jsonb_build_object('n',jsonb_build_object('type','int','value',n::text),'secret',jsonb_build_object('type','int','value','777')))));
 ELSE
   SELECT task INTO STRICT prior FROM native_business_fixture.saved WHERE generation=g-1;
   SELECT source_epoch INTO STRICT epoch FROM _oo.source_instance WHERE source=s ORDER BY source_epoch DESC LIMIT 1;
   PERFORM _oo.native_c1_ack_seed(s,claimed.task_id,claimed.generation,epoch,prior,g-1);
 END IF;
 PERFORM _oo.native_c1_baseline_seal(s,claimed.task_id,claimed.generation,epoch,server_id||':1-10',10);
 FOREACH t IN ARRAY tables LOOP
   cols:=ARRAY[CASE WHEN t='customer' THEN 'id' ELSE 'n' END,'secret']; pcs:=ARRAY[]::text[];
   PERFORM _oo.register_physical_mapping(s,cfg,g,'namespace','public','','',_oo.pm_sha256(_oo.pm_payload(s,cfg,g,'n','public','','')));
   PERFORM _oo.register_physical_mapping(s,cfg,g,'table','public',t,'',_oo.pm_sha256(_oo.pm_payload(s,cfg,g,'t','public',t,'')));
   ns:=_oo.lookup_physical_mapping(s,cfg,g,'namespace','public','','');
   physical:=_oo.lookup_physical_mapping(s,cfg,g,'table','public',t,'');
   FOREACH c IN ARRAY cols LOOP
     PERFORM _oo.register_physical_mapping(s,cfg,g,'column','public',t,c,_oo.pm_sha256(_oo.pm_payload(s,cfg,g,'c','public',t,c)));
     pcs:=array_append(pcs,_oo.lookup_physical_mapping(s,cfg,g,'column','public',t,c));
   END LOOP;
   run:=_oo.full_import_open_run(s,own,fence,s,g,cfg,typ,inp,ddl,ARRAY['public'],ARRAY[t],ARRAY[physical]);
   PERFORM _oo.full_import_prepare_shadow(s,own,fence,run,0,s,cfg,g,'public',t,cols,
       ARRAY[CASE WHEN t='customer' THEN 23 ELSE 20 END,20]::bigint[],ARRAY[-1,-1],ARRAY[true,true]);
   IF t='customer' THEN
     EXECUTE format('INSERT INTO _oo.%I(_oo_pos,%I,%I) SELECT 0,k,777 FROM generate_series(1,%s) k',physical,pcs[1],pcs[2],n);
     count_rows:=n;
   ELSE
     EXECUTE format('INSERT INTO _oo.%I(_oo_pos,%I,%I) VALUES(0,%s,777)',physical,pcs[1],pcs[2],n);
     count_rows:=1;
   END IF;
   PERFORM _oo.full_import_write_batch(s,own,fence,run,0,s,physical,'baseline',cfg,cfg,typ,
       encode(sha256(convert_to('full-import/v1 hook none','UTF8')),'hex'),ddl,inp,pos,count_rows,0,
       ARRAY[]::text[],ARRAY[]::bigint[],ARRAY[]::integer[],ARRAY[]::bigint[],ARRAY[]::text[]);
   PERFORM _oo.full_import_note_table(s,own,fence,run,0,count_rows,cfg,s);
   PERFORM _oo.native_c1_baseline_bind_table(s,claimed.task_id,claimed.generation,epoch,s,own,fence,run,'public',t,cols,ARRAY[cols[1]]);
   PERFORM _oo.native_update_seal_quality(s,own,fence,run,s,g,'public',t,physical,'baseline',cfg,pos,
       'mysql-gtid/v1','mysql.'||server_id,'c1_cdc','native','T',true,ARRAY[]::text[],count_rows,'never',0,'full');
   PERFORM _oo.full_import_seal_run(s,own,fence,run,s,g,cfg,typ,inp,ddl,ARRAY['public'],ARRAY[t]);
   entries:=entries||jsonb_build_array(jsonb_build_object('schema','public','table',t,'physical_id',physical,'run_id',run));
   receipts:=receipts||jsonb_build_array(jsonb_build_object('schema','public','table',t,'physical_name',physical,'run_id',run,
       'accepted_rows',count_rows,'canonical_digest',cfg,'source_generation',g,'target_generation',fence,'applied_pos','10','rejects','[]'::jsonb));
   mappings:=mappings||jsonb_build_array(jsonb_build_object('dataset','public.'||t,'source',s,'config_digest',cfg,'generation',g));
   -- Only the declared aggregate column, never table SELECT, secret, or _oo_pos.
   EXECUTE format('GRANT SELECT(%I) ON _oo.%I TO %I',pcs[1],physical,CASE WHEN t='customer' THEN 'oo_r_clerk' ELSE 'biz_oracle' END);
 END LOOP;
 scope:=jsonb_build_object('config_digest',cfg,'schema_digest',global_schema,'lineage','mysql.'||server_id,
   'l0',server_id||':1-10','l_end',server_id||':1-10','complete',true,'consistency_grade','T','cross_table_consistent',true,
   'tables',jsonb_build_array(jsonb_build_object('schema','public','table','customer','schema_sha256',ddl),
     jsonb_build_object('schema','public','table','independent_counts','schema_sha256',ddl)));
 PERFORM _oo.native_c1_baseline_typed_commit(s,claimed.task_id,claimed.generation,epoch,fence,g,inp,scope,entries);
 queued:=jobs.complete_success(project,claimed.task_id,'fixture',claimed.generation);
 IF queued.status<>'succeeded' THEN RAISE EXCEPTION 'fixture jobs CAS'; END IF;
 PERFORM _oo.native_c1_baseline_ack(s,claimed.task_id,inp);
 receipt:=jsonb_build_object('protocol','oo.pipeline.native-publication-receipt/v1','run_id',run,
   'run_ids',(SELECT jsonb_agg(e->'run_id') FROM jsonb_array_elements(receipts) e),
   'source_id',s,'source_generation',g,'target_generation',fence,'position_codec','mysql-gtid/v1',
   'position_lineage','mysql.'||server_id,'source_epoch',epoch,'applied_pos','10',
   'consistency','transactional','consistency_grade','T','cross_table_consistent',true,'tables',receipts);
 -- JSON canonical hash is supplied by the Rust test before calling real publication.
 result:=jsonb_build_object('datasets',mappings,'publication',jsonb_build_object(
   'protocol','oo.pipeline.native-publication-receipt/v1','run_id',run,'receipt',receipt));
 INSERT INTO native_business_fixture.saved VALUES(g,claimed.task_id,receipt,result);
 RETURN result;
END $fixture$;
