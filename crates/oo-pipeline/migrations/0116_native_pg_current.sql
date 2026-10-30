-- PostgreSQL captured parts use existing private typed tables and jobs leases.
-- No MySQL/Oracle path is changed except the additive PG engine guard.
ALTER TABLE _oo.source_instance DROP CONSTRAINT source_instance_engine_check;
ALTER TABLE _oo.source_instance ADD CONSTRAINT source_instance_engine_check
 CHECK(engine IN ('mariadb','mysql','mongodb','oracle','postgresql'));
ALTER TABLE _oo.native_c1_baseline_run DROP CONSTRAINT native_c1_baseline_run_engine_check;
ALTER TABLE _oo.native_c1_baseline_run ADD CONSTRAINT native_c1_baseline_run_engine_check
 CHECK(engine IN ('mysql','oracle','postgresql'));

CREATE TABLE _oo.pg_current_run (
 source text PRIMARY KEY, task uuid NOT NULL UNIQUE, bundle_sha text NOT NULL,
 source_identity jsonb NOT NULL, boundary jsonb NOT NULL, tables jsonb NOT NULL,
 result jsonb, sealed_at timestamptz,
 CHECK(bundle_sha ~ '^[0-9a-f]{64}$'), CHECK((result IS NULL)=(sealed_at IS NULL))
);
CREATE TABLE _oo.pg_current_tx (
 source text NOT NULL REFERENCES _oo.pg_current_run(source), ordinal bigint NOT NULL,
 scope jsonb NOT NULL, end_lsn numeric(38,0) NOT NULL, applied_events bigint NOT NULL,
 committed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(source,ordinal), UNIQUE(source,end_lsn)
);
REVOKE ALL ON _oo.pg_current_run,_oo.pg_current_tx FROM PUBLIC,oo_pipeline_runtime;

ALTER FUNCTION _oo.native_c1_baseline_v2_assert(text,uuid,bigint,integer)
 RENAME TO native_c1_baseline_v2_assert_0115;
CREATE FUNCTION _oo.native_c1_baseline_v2_assert(s text,t uuid,g bigint,e integer)
RETURNS _oo.native_c1_baseline_run LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE;
BEGIN
 SELECT * INTO v FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
 IF v.engine IS DISTINCT FROM 'postgresql' THEN RETURN _oo.native_c1_baseline_v2_assert_0115(s,t,g,e); END IF;
 IF v.state<>'open' OR v.lease_generation<>g OR v.source_epoch<>e
  OR NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_v2_format WHERE source=s AND task_id=t AND NOT sealed)
  OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=t AND state='running' AND generation=g AND lease_until>clock_timestamp())
  OR NOT EXISTS(SELECT 1 FROM _oo.source_instance WHERE source=s AND source_epoch=e AND engine='postgresql'
    AND measured_id=v.measured_id AND config_digest=v.config_digest AND schema_digest=v.schema_digest AND lease_id=t AND lease_generation=g)
 THEN RAISE EXCEPTION 'PG live lease required' USING ERRCODE='OO010'; END IF;
 RETURN v;
END $f$;

CREATE FUNCTION _oo.pg_current_open(s text,t uuid,g bigint,p jsonb,shard text,owner uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE old _oo.pg_current_run%ROWTYPE; measured text; cfg text; sch text;
BEGIN
 PERFORM _oo.native_c1_baseline_lock(s,t);
 IF NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=t AND generation=g AND state='running' AND lease_until>clock_timestamp())
  OR p->>'bundle_sha256' IS NULL OR p->>'bundle_sha256' !~ '^[0-9a-f]{64}$'
  OR p#>>'{source_identity,source_id}' IS DISTINCT FROM s OR p#>>'{source_identity,plugin}' IS DISTINCT FROM 'pgoutput'
  OR jsonb_typeof(p->'tables') IS DISTINCT FROM 'array' THEN RAISE EXCEPTION 'PG open identity' USING ERRCODE='OO010'; END IF;
 SELECT * INTO old FROM _oo.pg_current_run WHERE source=s FOR UPDATE;
 IF FOUND THEN
  IF old.task<>t OR old.bundle_sha<>p->>'bundle_sha256' OR old.source_identity<>p->'source_identity'
    OR old.boundary<>p->'boundary' OR old.tables<>p->'tables' THEN RAISE EXCEPTION 'PG immutable capture' USING ERRCODE='OO030'; END IF;
  IF old.result IS NOT NULL THEN RETURN old.result||jsonb_build_object('replay',true); END IF;
  UPDATE _oo.source_instance SET lease_generation=g WHERE source=s AND lease_id=t AND engine='postgresql';
  UPDATE _oo.native_c1_baseline_run SET lease_generation=g WHERE source=s AND task_id=t AND engine='postgresql';
  RETURN jsonb_build_object('outcome','open','replay',true);
 END IF;
 IF EXISTS(SELECT 1 FROM _oo.source_instance WHERE source=s) OR EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=s)
 THEN RAISE EXCEPTION 'PG foreign source' USING ERRCODE='OO030'; END IF;
 measured:='postgresql.'||(p#>>'{source_identity,system_identifier}')||'.'||(p#>>'{source_identity,database_oid}')||'.'||(p#>>'{source_identity,slot}');
 cfg:=p->>'bundle_sha256';sch:=p->>'schema_digest';
 INSERT INTO _oo.source_instance(source,source_epoch,engine,measured_id,config_digest,schema_digest,shard_id,owner_token,lease_id,lease_generation)
 VALUES(s,1,'postgresql',measured,cfg,sch,shard,owner,t,g);
 INSERT INTO _oo.native_c1_baseline_run(source,task_id,lease_generation,source_epoch,engine,measured_id,config_digest,schema_digest,l0,l0_pos,applied_pos,state)
 VALUES(s,t,g,1,'postgresql',measured,cfg,sch,p#>>'{boundary,original_slot_lsn}',0,0,'open');
 INSERT INTO _oo.native_c1_baseline_v2_format(source,task_id,format) VALUES(s,t,'chunked-typed/v2');
 INSERT INTO _oo.pg_current_run(source,task,bundle_sha,source_identity,boundary,tables)
 VALUES(s,t,cfg,p->'source_identity',p->'boundary',p->'tables');
 RETURN jsonb_build_object('outcome','open','replay',false);
END $f$;

-- Event values come from original pgoutput. An unchanged TOAST value is read
-- only from the already committed preceding target state, never the live source.
CREATE FUNCTION _oo.pg_current_event(s text,t uuid,g bigint,p_table text,p_kind text,p_old jsonb,p_new jsonb,p_pos numeric)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE r _oo.c1_typed_table%ROWTYPE; key_idx integer; old_key bigint; new_key bigint;
 row_old jsonb; vals text:=''; names text:=''; v jsonb; i integer:=0; col jsonb; typ text; value text; payload bytea; key_bytes bytea;
BEGIN
 PERFORM _oo.native_c1_baseline_v2_assert(s,t,g,1);
 IF p_kind NOT IN ('I','U','D') OR p_pos<=0 THEN RAISE EXCEPTION 'PG event' USING ERRCODE='OO030'; END IF;
 SELECT * INTO STRICT r FROM _oo.c1_typed_table WHERE source=s AND task=t AND name=p_table;
 key_idx:=(r.spec->'keys'->>0)::integer;
 IF jsonb_array_length(r.spec->'keys')<>1 OR r.spec->'columns'->key_idx->>'oid'<>'20' THEN RAISE EXCEPTION 'PG key' USING ERRCODE='OO030'; END IF;
 IF p_new IS NOT NULL THEN new_key:=(p_new->key_idx->>'value')::bigint; END IF;
 old_key:=CASE WHEN p_old IS NOT NULL THEN (p_old->key_idx->>'value')::bigint ELSE new_key END;
 IF old_key IS NULL THEN RAISE EXCEPTION 'PG null key' USING ERRCODE='OO030'; END IF;
 EXECUTE format('SELECT to_jsonb(x) FROM _oo.%I x WHERE c%s=$1',r.physical,key_idx) INTO row_old USING old_key;
 IF p_kind IN ('U','D') AND row_old IS NULL THEN RAISE EXCEPTION 'PG prior row absent' USING ERRCODE='OO030'; END IF;
 IF p_kind='I' AND row_old IS NOT NULL THEN RAISE EXCEPTION 'PG duplicate insert' USING ERRCODE='OO030'; END IF;
 IF p_kind='D' OR old_key IS DISTINCT FROM new_key THEN
  EXECUTE format('DELETE FROM _oo.%I WHERE c%s=$1',r.physical,key_idx) USING old_key;
  payload:=convert_to(old_key::text,'UTF8');key_bytes:=decode('6f6f2e63312e6b65792f763300','hex')||int4send(1)||decode('01','hex')||int4send(octet_length(payload))||payload;
  EXECUTE format('INSERT INTO _oo.%I(k,pos) VALUES($1,$2) ON CONFLICT(k) DO UPDATE SET pos=EXCLUDED.pos',r.tomb) USING key_bytes,p_pos;
 END IF;
 IF p_kind='D' THEN RETURN; END IF;
 IF new_key IS NULL OR jsonb_array_length(p_new)<>jsonb_array_length(r.spec->'columns') THEN RAISE EXCEPTION 'PG tuple' USING ERRCODE='OO030'; END IF;
 FOR col IN SELECT a.value FROM jsonb_array_elements(r.spec->'columns') a LOOP
  typ:=_oo.full_import_type_sql((col->>'oid')::bigint,(col->>'typmod')::integer);v:=p_new->i;
  IF v->>'kind'='unchanged' THEN
   IF row_old IS NULL OR NOT row_old ? ('c'||i) THEN RAISE EXCEPTION 'PG unchanged without prior' USING ERRCODE='OO030'; END IF;
   value:=row_old->>('c'||i);
  ELSIF v->>'kind'='null' THEN value:=NULL;
  ELSIF v->>'kind'='text' THEN value:=v->>'value';
  ELSE RAISE EXCEPTION 'PG value kind' USING ERRCODE='OO030'; END IF;
  IF value IS NULL AND (col->>'nullable')::boolean=false THEN RAISE EXCEPTION 'PG null' USING ERRCODE='OO030'; END IF;
  vals:=vals||format(',%L::%s',value,typ);names:=names||format(',c%s',i);i:=i+1;
 END LOOP;
 payload:=convert_to(new_key::text,'UTF8');key_bytes:=decode('6f6f2e63312e6b65792f763300','hex')||int4send(1)||decode('01','hex')||int4send(octet_length(payload))||payload;
 IF old_key IS DISTINCT FROM new_key THEN
  EXECUTE format('SELECT to_jsonb(x) FROM _oo.%I x WHERE c%s=$1',r.physical,key_idx) INTO row_old USING new_key;
  IF row_old IS NOT NULL THEN RAISE EXCEPTION 'PG new key exists' USING ERRCODE='OO030'; END IF;
 END IF;
 EXECUTE format('INSERT INTO _oo.%I(k,pos%s) VALUES($1,$2%s) ON CONFLICT(k) DO UPDATE SET pos=EXCLUDED.pos,%s',r.physical,names,vals,r.update_sql) USING key_bytes,p_pos;
 EXECUTE format('DELETE FROM _oo.%I WHERE k=$1',r.tomb) USING key_bytes;
END $f$;

CREATE FUNCTION _oo.pg_current_commit(s text,t uuid,g bigint,p_ordinal bigint,p_scope jsonb,p_end numeric,p_events bigint)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE n bigint; expected jsonb;
BEGIN
 PERFORM _oo.native_c1_baseline_v2_assert(s,t,g,1);
 SELECT count(*) INTO n FROM _oo.pg_current_tx WHERE source=s;
 SELECT boundary->'transactions'->(p_ordinal::integer-1) INTO expected FROM _oo.pg_current_run WHERE source=s AND task=t;
 IF p_end IS DISTINCT FROM ((p_scope->>'end_lsn')::pg_lsn-'0/0'::pg_lsn)
  OR p_ordinal<>n+1 OR expected IS DISTINCT FROM p_scope OR p_events<>(p_scope->>'inserts')::bigint+(p_scope->>'updates')::bigint+(p_scope->>'deletes')::bigint
  OR EXISTS(SELECT 1 FROM _oo.c1_typed_table WHERE source=s AND task=t AND NOT snapshot_complete)
 THEN RAISE EXCEPTION 'PG complete atomic transaction required' USING ERRCODE='OO030'; END IF;
 INSERT INTO _oo.pg_current_tx(source,ordinal,scope,end_lsn,applied_events) VALUES(s,p_ordinal,p_scope,p_end,p_events);
END $f$;

CREATE FUNCTION _oo.pg_current_seal(s text,t uuid,g bigint,p_end numeric)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE r _oo.pg_current_run%ROWTYPE; entries jsonb; n bigint; last_lsn numeric; v_result jsonb;
BEGIN
 PERFORM _oo.native_c1_baseline_v2_assert(s,t,g,1);
 SELECT * INTO STRICT r FROM _oo.pg_current_run WHERE source=s AND task=t;
 SELECT count(*),max(end_lsn) INTO n,last_lsn FROM _oo.pg_current_tx WHERE source=s;
 IF n<>jsonb_array_length(r.boundary->'transactions') OR last_lsn IS DISTINCT FROM p_end
  OR EXISTS(SELECT 1 FROM _oo.c1_typed_table WHERE source=s AND task=t AND NOT snapshot_complete)
  OR (SELECT count(*) FROM _oo.c1_typed_table WHERE source=s AND task=t)<>jsonb_array_length(r.tables)
 THEN RAISE EXCEPTION 'PG capture incomplete' USING ERRCODE='OO030'; END IF;
 SELECT jsonb_agg(jsonb_build_object('name',name,'physical',physical,'spec',spec) ORDER BY name) INTO entries FROM _oo.c1_typed_table WHERE source=s AND task=t;
 v_result:=jsonb_build_object('protocol','oo.pipeline.pg-current-result/v1','load_state','staged',
  'source_id',s,'task',t,'bundle_sha256',r.bundle_sha,'source_identity',r.source_identity,'source_generation',1,
  'end_lsn',r.boundary->>'end_lsn','applied_pos',p_end::text,'transaction_count',n,'entries',entries,'acknowledged',false,'replay',false);
 UPDATE _oo.pg_current_run SET result=v_result, sealed_at=clock_timestamp() WHERE source=s;
 UPDATE _oo.native_c1_baseline_run SET state='awaiting_publish',l_end=p_end,applied_pos=p_end,sealed_at=clock_timestamp() WHERE source=s AND task_id=t;
 UPDATE _oo.native_c1_baseline_v2_format SET sealed=true WHERE source=s AND task_id=t;
 RETURN v_result;
END $f$;

GRANT SELECT ON _oo.pg_current_run,_oo.pg_current_tx TO oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.pg_current_open(text,uuid,bigint,jsonb,text,uuid),_oo.pg_current_event(text,uuid,bigint,text,text,jsonb,jsonb,numeric),_oo.pg_current_commit(text,uuid,bigint,bigint,jsonb,numeric,bigint),_oo.pg_current_seal(text,uuid,bigint,numeric) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.pg_current_open(text,uuid,bigint,jsonb,text,uuid),_oo.pg_current_event(text,uuid,bigint,text,text,jsonb,jsonb,numeric),_oo.pg_current_commit(text,uuid,bigint,bigint,jsonb,numeric,bigint),_oo.pg_current_seal(text,uuid,bigint,numeric) TO oo_pipeline_runtime;
