-- Extend the existing source registry, not a second lease authority.
ALTER TABLE _oo.source_instance DROP CONSTRAINT source_instance_engine_check;
ALTER TABLE _oo.source_instance ADD CONSTRAINT source_instance_engine_check
  CHECK(engine IN ('mariadb','mysql','mongodb','oracle'));
ALTER TABLE _oo.native_c1_baseline_run DROP CONSTRAINT native_c1_baseline_run_engine_check;
ALTER TABLE _oo.native_c1_baseline_run ADD CONSTRAINT native_c1_baseline_run_engine_check CHECK(engine IN ('mysql','oracle'));
ALTER FUNCTION _oo.native_source_instance_register(text,uuid,bigint,text,text,text,text,text,uuid)
  RENAME TO native_source_instance_register_legacy;

CREATE FUNCTION _oo.native_source_instance_register(
 p_shard text,p_owner uuid,p_lease_generation bigint,p_source text,p_engine text,
 p_measured_id text,p_config_digest text,p_schema_digest text,p_lease uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE old _oo.source_instance%ROWTYPE; epoch integer;
BEGIN
  IF p_engine IS DISTINCT FROM 'oracle' THEN
    RETURN _oo.native_source_instance_register_legacy(p_shard,p_owner,p_lease_generation,p_source,p_engine,
      p_measured_id,p_config_digest,p_schema_digest,p_lease);
  END IF;
  IF p_source IS NULL OR p_source !~ '^[a-z][a-z0-9_]{0,62}$'
    OR p_shard IS NULL OR p_shard !~ '^[a-z][a-z0-9_.:-]{0,127}$' OR p_owner IS NULL
    OR p_measured_id IS NULL OR p_measured_id !~ '^oracle\.[1-9][0-9]{0,19}\.(0|[1-9][0-9]{0,19})\.[0-9a-f]{32}$'
    OR p_config_digest IS NULL OR p_config_digest !~ '^[0-9a-f]{64}$'
    OR p_schema_digest IS NULL OR p_schema_digest !~ '^[0-9a-f]{64}$' THEN
    RAISE EXCEPTION 'closed Oracle source identity' USING ERRCODE='23514';
  END IF;
  IF p_lease IS NULL OR p_lease_generation IS NULL OR p_lease_generation<1
    OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=p_lease AND generation=p_lease_generation
      AND state='running' AND lease_until>clock_timestamp()) THEN
    RAISE EXCEPTION 'live source lease required' USING ERRCODE='OO010';
  END IF;
  PERFORM pg_advisory_xact_lock(hashtext('oo-source-instance'),hashtext(p_source));
  SELECT * INTO old FROM _oo.source_instance WHERE source=p_source AND measured_id=p_measured_id FOR UPDATE;
  IF FOUND THEN
    IF old.engine<>'oracle' OR old.config_digest<>p_config_digest OR old.schema_digest<>p_schema_digest
      OR old.shard_id<>p_shard OR old.owner_token<>p_owner OR old.lease_id<>p_lease
      OR old.lease_generation<>p_lease_generation THEN
      RAISE EXCEPTION 'source instance context conflict' USING ERRCODE='OO010';
    END IF;
    epoch:=old.source_epoch;
  ELSE
    IF EXISTS(SELECT 1 FROM _oo.source_instance i JOIN jobs.tasks j ON j.id=i.lease_id
      WHERE i.source=p_source AND j.state='running' AND j.lease_until>clock_timestamp()) THEN
      RAISE EXCEPTION 'source already has a live writer' USING ERRCODE='OO010';
    END IF;
    SELECT COALESCE(max(source_epoch),0)+1 INTO epoch FROM _oo.source_instance WHERE source=p_source;
    INSERT INTO _oo.source_instance(source,source_epoch,engine,measured_id,config_digest,schema_digest,
      shard_id,owner_token,lease_id,lease_generation)
      VALUES(p_source,epoch,'oracle',p_measured_id,p_config_digest,p_schema_digest,p_shard,p_owner,p_lease,p_lease_generation);
  END IF;
  RETURN jsonb_build_object('protocol','oo.pipeline.native-source-instance/v1','source',p_source,
    'engine','oracle','measured_id',p_measured_id,'source_epoch',epoch,'config_digest',p_config_digest,
    'schema_digest',p_schema_digest,'shard_id',p_shard,'owner_token',p_owner,'lease_id',p_lease,
    'replay',old.source IS NOT NULL);
END $fn$;

CREATE FUNCTION _oo.c1_typed_open_oracle(s text,t uuid,g bigint,e integer,p_offset text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE i _oo.source_instance%ROWTYPE; old _oo.native_c1_baseline_run%ROWTYPE;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  SELECT * INTO i FROM _oo.source_instance WHERE source=s AND source_epoch=e;
  IF NOT FOUND OR i.engine<>'oracle' OR i.lease_id<>t OR i.lease_generation<>g
    OR p_offset IS NULL OR p_offset !~ '^[0-9a-f]{64}$'
    OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=t AND generation=g AND state='running' AND lease_until>clock_timestamp()) THEN
    RAISE EXCEPTION 'Oracle registered live lease required' USING ERRCODE='OO010';
  END IF;
  INSERT INTO _oo.native_c1_baseline_run(source,task_id,lease_generation,source_epoch,engine,
    measured_id,config_digest,schema_digest,l0,l0_pos,applied_pos,state)
    VALUES(s,t,g,e,'oracle',i.measured_id,i.config_digest,i.schema_digest,p_offset,0,0,'open') ON CONFLICT DO NOTHING;
  SELECT * INTO old FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
  IF old.engine<>'oracle' OR old.l0<>p_offset OR old.source_epoch<>e OR old.lease_generation<>g THEN
    RAISE EXCEPTION 'Oracle baseline replay conflict' USING ERRCODE='OO010';
  END IF;
  INSERT INTO _oo.native_c1_baseline_v2_format(source,task_id,format)
    VALUES(s,t,'chunked-typed/v2') ON CONFLICT DO NOTHING;
END $fn$;

-- Existing guard and lock order, with the stored engine compared instead of
-- a hard-coded MySQL literal. Legacy open entry points still only open MySQL.
CREATE OR REPLACE FUNCTION _oo.native_c1_baseline_v2_assert(p_source text,p_task uuid,p_generation bigint,p_epoch integer)
RETURNS _oo.native_c1_baseline_run LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE;
 s ALIAS FOR p_source; t ALIAS FOR p_task; g ALIAS FOR p_generation; e ALIAS FOR p_epoch;
BEGIN
  SELECT * INTO v FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
  IF NOT FOUND OR v.engine NOT IN ('mysql','oracle') OR v.state<>'open'
    OR v.lease_generation<>g OR v.source_epoch<>e
    OR NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_v2_format WHERE source=s AND task_id=t AND NOT sealed)
    OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=t AND state='running' AND generation=g AND lease_until>clock_timestamp())
    OR NOT EXISTS(SELECT 1 FROM _oo.source_instance WHERE source=s AND source_epoch=e
      AND engine=v.engine AND measured_id=v.measured_id AND config_digest=v.config_digest
      AND schema_digest=v.schema_digest AND lease_id=t AND lease_generation=g) THEN
    RAISE EXCEPTION 'C1 live source lease required' USING ERRCODE='OO010';
  END IF;
  RETURN v;
END $fn$;

CREATE TABLE _oo.c1_typed_source_position(
 source text NOT NULL,task uuid NOT NULL,seq bigint NOT NULL,apply_ordinal numeric(38,0) NOT NULL,
 engine text NOT NULL CHECK(engine='oracle'),transaction_id text NOT NULL,
 commit_scn numeric(38,0) NOT NULL,opaque_before text NOT NULL,opaque_after text NOT NULL,
 evidence jsonb NOT NULL,
 PRIMARY KEY(source,task,seq),UNIQUE(source,task,transaction_id),
 FOREIGN KEY(source,task,seq) REFERENCES _oo.c1_typed_receipt(source,task,seq)
);
REVOKE ALL ON _oo.c1_typed_source_position FROM PUBLIC,oo_pipeline_runtime;

ALTER FUNCTION _oo.c1_typed_ack(text,uuid,bigint,integer,bigint,text)
RENAME TO c1_typed_ack_legacy;
CREATE FUNCTION _oo.c1_typed_ack(s text,t uuid,g bigint,e integer,p_seq bigint,p_digest text)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  v:=_oo.native_c1_baseline_v2_assert(s,t,g,e);
  IF v.engine='oracle' AND EXISTS(SELECT 1 FROM _oo.c1_typed_receipt WHERE source=s AND task=t AND seq=p_seq AND kind='cdc')
    AND NOT EXISTS(SELECT 1 FROM _oo.c1_typed_source_position WHERE source=s AND task=t AND seq=p_seq) THEN
    RAISE EXCEPTION 'Oracle complete opaque checkpoint absent' USING ERRCODE='OO030';
  END IF;
  RETURN _oo.c1_typed_ack_legacy(s,t,g,e,p_seq,p_digest);
END $fn$;
REVOKE ALL ON FUNCTION _oo.c1_typed_ack_legacy(text,uuid,bigint,integer,bigint,text) FROM PUBLIC,oo_pipeline_runtime;
REVOKE ALL ON FUNCTION _oo.c1_typed_ack(text,uuid,bigint,integer,bigint,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c1_typed_ack(text,uuid,bigint,integer,bigint,text) TO oo_pipeline_runtime;

CREATE FUNCTION _oo.c1_typed_checkpoint_evidence(s text,t uuid,g bigint,e integer,p_seq bigint,p_digest text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE; result jsonb;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  v:=_oo.native_c1_baseline_v2_assert(s,t,g,e);
  IF NOT EXISTS(SELECT 1 FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t AND seq=p_seq)
    OR NOT EXISTS(SELECT 1 FROM _oo.c1_typed_receipt WHERE source=s AND task=t AND seq=p_seq AND digest=p_digest) THEN
    RAISE EXCEPTION 'durable checkpoint required' USING ERRCODE='OO010';
  END IF;
  SELECT jsonb_build_object('engine',engine,'apply_ordinal',apply_ordinal::text,
    'transaction_id',transaction_id,'commit_scn',commit_scn::text,
    'opaque_before_sha256',opaque_before,'opaque_after_sha256',opaque_after)
    INTO result FROM _oo.c1_typed_source_position WHERE source=s AND task=t AND seq=p_seq;
  RETURN result;
END $fn$;
REVOKE ALL ON FUNCTION _oo.c1_typed_checkpoint_evidence(text,uuid,bigint,integer,bigint,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c1_typed_checkpoint_evidence(text,uuid,bigint,integer,bigint,text) TO oo_pipeline_runtime;

CREATE FUNCTION _oo.c1_typed_assert_engine(s text,t uuid,g bigint,e integer,p_engine text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE;
BEGIN
  v:=_oo.native_c1_baseline_v2_assert(s,t,g,e);
  IF v.engine IS DISTINCT FROM p_engine THEN RAISE EXCEPTION 'source engine mismatch' USING ERRCODE='OO010'; END IF;
END $fn$;

CREATE FUNCTION _oo.c1_typed_record_oracle_position(s text,t uuid,g bigint,e integer,p_seq bigint,p_digest text,p jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE; r _oo.c1_typed_receipt%ROWTYPE;
  old _oo.c1_typed_source_position%ROWTYPE; prev _oo.c1_typed_source_position%ROWTYPE;
  n bigint; scn numeric; before_sha text; after_sha text;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  v:=_oo.native_c1_baseline_v2_assert(s,t,g,e);
  IF v.engine<>'oracle' OR p->>'protocol' IS DISTINCT FROM 'oo.c1.oracle-logminer-position/v1'
    OR p->>'engine' IS DISTINCT FROM 'oracle' OR p->>'transaction_id' IS NULL
    OR p->>'transaction_id' !~ '^[A-Za-z0-9_$#-]{1,128}$'
    OR p->>'commit_scn' IS NULL OR p->>'commit_scn' !~ '^[1-9][0-9]{0,37}$'
    OR p->>'events_sha256' IS NULL OR p->>'events_sha256' !~ '^[0-9a-f]{64}$' THEN
    RAISE EXCEPTION 'Oracle complete transaction required' USING ERRCODE='OO030';
  END IF;
  n:=(p->>'event_count')::bigint;scn:=(p->>'commit_scn')::numeric;
  before_sha:=p#>>'{offset_before,sha256}';after_sha:=p#>>'{offset_after,sha256}';
  IF n IS NULL OR n NOT BETWEEN 1 AND 100000 OR (p->>'end_event_count')::bigint IS DISTINCT FROM n
    OR (p->>'event_order_first')::bigint IS DISTINCT FROM 1 OR (p->>'event_order_last')::bigint IS DISTINCT FROM n
    OR before_sha IS NULL OR before_sha !~ '^[0-9a-f]{64}$' OR after_sha IS NULL OR after_sha !~ '^[0-9a-f]{64}$'
    OR before_sha=after_sha THEN RAISE EXCEPTION 'Oracle opaque checkpoint required' USING ERRCODE='OO030'; END IF;
  SELECT * INTO r FROM _oo.c1_typed_receipt WHERE source=s AND task=t AND seq=p_seq AND digest=p_digest AND kind='cdc';
  IF NOT FOUND THEN RAISE EXCEPTION 'typed target receipt required' USING ERRCODE='OO030'; END IF;
  SELECT * INTO old FROM _oo.c1_typed_source_position WHERE source=s AND task=t AND seq=p_seq;
  IF FOUND THEN
    IF old.evidence<>p THEN RAISE EXCEPTION 'source checkpoint replay conflict' USING ERRCODE='OO030'; END IF;
    RETURN;
  END IF;
  SELECT * INTO prev FROM _oo.c1_typed_source_position WHERE source=s AND task=t ORDER BY seq DESC LIMIT 1;
  IF before_sha<>COALESCE(prev.opaque_after,v.l0) OR (prev.commit_scn IS NOT NULL AND scn<prev.commit_scn) THEN
    RAISE EXCEPTION 'opaque checkpoint chain or source position regression' USING ERRCODE='OO030';
  END IF;
  INSERT INTO _oo.c1_typed_source_position VALUES(s,t,p_seq,r.position,'oracle',p->>'transaction_id',scn,before_sha,after_sha,p);
END $fn$;

REVOKE ALL ON FUNCTION
 _oo.native_source_instance_register(text,uuid,bigint,text,text,text,text,text,uuid),
 _oo.c1_typed_assert_engine(text,uuid,bigint,integer,text),
 _oo.c1_typed_open_oracle(text,uuid,bigint,integer,text),
 _oo.c1_typed_record_oracle_position(text,uuid,bigint,integer,bigint,text,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
 _oo.native_source_instance_register(text,uuid,bigint,text,text,text,text,text,uuid),
 _oo.c1_typed_assert_engine(text,uuid,bigint,integer,text),
 _oo.c1_typed_open_oracle(text,uuid,bigint,integer,text),
 _oo.c1_typed_record_oracle_position(text,uuid,bigint,integer,bigint,text,jsonb) TO oo_pipeline_runtime;
