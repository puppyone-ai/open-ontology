-- Owned target-only fixture. Real jobs enqueue/claim; all fixture metadata and
-- migration DDL are rolled back by the caller. No source capture/admission claim.
\set ON_ERROR_STOP on
SELECT task_id::text AS task FROM jobs.enqueue(
 '7f6acd86-a344-4453-831d-541282a59911','native_capture',1,'{}',
 'c5-parts-stage-test',repeat('a',64),'owned-stage-only',clock_timestamp(),3) \gset
INSERT INTO jw_native.bindings(id,project_id,source_id,method,cycle,period_ms,
 enabled,manifest_sha,pin,manifest,target_database)
VALUES('6eaec529-9dd8-4a08-94eb-5511cc567111','7f6acd86-a344-4453-831d-541282a59911',
 'c5_parts_stage_test','C5','pull',1000,false,repeat('a',64),1,'{}',current_database());
INSERT INTO jw_native.occurrences(binding_id,pin,task_id,manifest_sha,scheduled_at,manifest)
VALUES('6eaec529-9dd8-4a08-94eb-5511cc567111',1,:'task'::uuid,repeat('a',64),clock_timestamp(),'{}');
INSERT INTO jw_native.source_progress(source_id,target_database,active_task_id)
VALUES('c5_parts_stage_test',current_database(),:'task'::uuid);
SELECT set_config('oo_test.task',:'task',true);
SET LOCAL ROLE :"runtime";
DO $test$
DECLARE
 t uuid:=current_setting('oo_test.task')::uuid; s text:='c5_parts_stage_test';
 claim jobs.lease_result; scope jsonb; second jsonb; changed jsonb; spec jsonb;
 box text; result jsonb; refusals integer:=0;
BEGIN
 claim:=jobs.claim_selected('7f6acd86-a344-4453-831d-541282a59911',t,
  ARRAY['native_capture'],'c5-parts-stage-test');
 IF claim.status<>'claimed' THEN RAISE EXCEPTION 'real claim required'; END IF;
 scope:=jsonb_build_object('protocol','oo.pipeline.native-c5-range-part/v1',
  'range',jsonb_build_object('source_engine','mysql','source_id',s,'schema','fixture','table','items',
    'schema_fingerprint',repeat('a',64),'chunk_topology_revision',1,'chunk_id','0',
    'lo',NULL,'hi',NULL,'prior_membership_revision',1,'parent_generation',1,'source_generation',2),
  'parts_index',jsonb_build_object('path','/private/parts.jsonl','sha256',repeat('b',64),'bytes',800),
  'snapshot',jsonb_build_object('kind','owned-stage-fixture','value','one'),
  'part_count',2,'total_rows',3,
  'part',jsonb_build_object('ordinal',0,'row_count',2,
    'arrow',jsonb_build_object('sha256',repeat('c',64))));
 spec:='{"columns":[{"name":"id","oid":20,"typmod":-1,"nullable":false},
   {"name":"amount","oid":1700,"typmod":2490390,"nullable":true}],"keys":[0]}'::jsonb;
 box:=_oo.c5_parts_open(s,t,claim.generation,scope,spec);
 IF box IS DISTINCT FROM _oo.c5_parts_open(s,t,claim.generation,scope,spec)
 THEN RAISE EXCEPTION 'open replay differs'; END IF;
 IF has_table_privilege(current_user,format('_oo.%I',box),'SELECT')
  OR has_table_privilege(current_user,'_oo.c5_parts_capture','UPDATE')
 THEN RAISE EXCEPTION 'private stage privilege leak'; END IF;
 BEGIN
  PERFORM _oo.c5_parts_begin(s,t,claim.generation+1,scope,repeat('d',64));
  RAISE EXCEPTION 'stale lease accepted';
 EXCEPTION WHEN SQLSTATE 'OO010' THEN refusals:=refusals+1; END;
 second:=jsonb_set(scope,'{part}',jsonb_build_object('ordinal',1,'row_count',1,
  'arrow',jsonb_build_object('sha256',repeat('e',64))));
 BEGIN
  PERFORM _oo.c5_parts_begin(s,t,claim.generation,second,repeat('f',64));
  RAISE EXCEPTION 'out of order accepted';
 EXCEPTION WHEN SQLSTATE 'OO030' THEN refusals:=refusals+1; END;
 changed:=jsonb_set(scope,'{snapshot,value}','"different"');
 BEGIN
  PERFORM _oo.c5_parts_open(s,t,claim.generation,changed,spec);
  RAISE EXCEPTION 'different snapshot accepted';
 EXCEPTION WHEN SQLSTATE 'OO030' THEN refusals:=refusals+1; END;
 IF NOT _oo.c5_parts_begin(s,t,claim.generation,scope,repeat('d',64))
 THEN RAISE EXCEPTION 'first part skipped'; END IF;
 EXECUTE format('INSERT INTO _oo.%I VALUES
  (int4send(9)||decode(''00'',''hex'')||int8send(1),0,1,12345678901234567890.123456789012345678),
  (int4send(9)||decode(''00'',''hex'')||int8send(2),1,2,NULL)',box);
 PERFORM _oo.c5_parts_finish_part(s,t,claim.generation,scope,repeat('d',64));
 IF _oo.c5_parts_begin(s,t,claim.generation,scope,repeat('d',64))
 THEN RAISE EXCEPTION 'replay recopies rows'; END IF;
 BEGIN
  PERFORM _oo.c5_parts_complete(s,t,claim.generation,repeat('b',64));
  RAISE EXCEPTION 'partial capture completed';
 EXCEPTION WHEN SQLSTATE 'OO030' THEN refusals:=refusals+1; END;
 BEGIN
  PERFORM _oo.c5_parts_begin(s,t,claim.generation,scope,repeat('a',64));
  RAISE EXCEPTION 'different admission accepted';
 EXCEPTION WHEN SQLSTATE 'OO030' THEN refusals:=refusals+1; END;
 BEGIN
  PERFORM _oo.c5_parts_begin(s,t,claim.generation,second,repeat('f',64));
  EXECUTE format('INSERT INTO _oo.%I VALUES
   (int4send(9)||decode(''00'',''hex'')||int8send(1),0,1,1)',box);
  PERFORM _oo.c5_parts_finish_part(s,t,claim.generation,second,repeat('f',64));
  RAISE EXCEPTION 'duplicate key across parts accepted';
 EXCEPTION WHEN unique_violation THEN refusals:=refusals+1; END;
 PERFORM _oo.c5_parts_begin(s,t,claim.generation,second,repeat('f',64));
 EXECUTE format('INSERT INTO _oo.%I VALUES
  (int4send(9)||decode(''00'',''hex'')||int8send(3),0,3,-0.000000000000000001)',box);
 PERFORM _oo.c5_parts_finish_part(s,t,claim.generation,second,repeat('f',64));
 result:=_oo.c5_parts_complete(s,t,claim.generation,repeat('b',64));
 IF result->'published'<>'false'::jsonb OR result->'acknowledged'<>'false'::jsonb
  OR result->'rows'<>'3'::jsonb OR result->'parts'<>'2'::jsonb
 THEN RAISE EXCEPTION 'incorrect stage completion'; END IF;
 RAISE NOTICE 'PASS: % refusals; durable two-part private stage, replay and role isolation',refusals;
END $test$;
RESET ROLE;
DO $test$
DECLARE c _oo.c5_parts_capture; rows jsonb;
BEGIN
 SELECT * INTO STRICT c FROM _oo.c5_parts_capture
  WHERE source='c5_parts_stage_test' AND task=current_setting('oo_test.task')::uuid;
 EXECUTE format('SELECT jsonb_agg(jsonb_build_array(c0::text,c1::text) ORDER BY c0) FROM _oo.%I',c.physical)
  INTO rows;
 IF rows IS DISTINCT FROM
  '[["1","12345678901234567890.123456789012345678"],["2",null],["3","-0.000000000000000001"]]'::jsonb
  OR EXISTS(SELECT FROM _oo.source_watermark WHERE source='c5_parts_stage_test')
  OR EXISTS(SELECT FROM _oo.full_import_run WHERE source_id='c5_parts_stage_test')
  OR EXISTS(SELECT FROM jw_native.pending_acks WHERE task_id=c.task)
 THEN RAISE EXCEPTION 'typed stage mismatch or premature public effect'; END IF;
END $test$;
