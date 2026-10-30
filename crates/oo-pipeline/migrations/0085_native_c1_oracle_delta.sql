-- First Oracle delta retains the confirmed source epoch and seeds from its
-- immutable typed parent, not from a second source snapshot.
CREATE TABLE _oo.c1_typed_delta_seed (
  source text NOT NULL,
  task uuid NOT NULL,
  parent_task uuid NOT NULL,
  rollover_sha256 text NOT NULL,
  previous jsonb NOT NULL,
  seeded boolean NOT NULL DEFAULT false,
  PRIMARY KEY(source,task)
);
REVOKE ALL ON _oo.c1_typed_delta_seed FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c1_oracle_delta_parent(s text,t uuid,g bigint,p jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE a _oo.native_c1_baseline_typed%ROWTYPE; w _oo.source_watermark%ROWTYPE;
  parent uuid; r jsonb; b jsonb;
BEGIN
  IF p->>'protocol' IS DISTINCT FROM 'oo.jobs.oracle-buffer-previous/v1'
    OR p->>'engine' IS DISTINCT FROM 'oracle' OR p->>'source_id' IS DISTINCT FROM s
    OR p->>'source_generation' IS DISTINCT FROM '1'
    OR p->>'target_generation' IS DISTINCT FROM '1'
    OR p->>'checkpoint_kind' IS DISTINCT FROM 'buffer_consumption'
    OR p->>'task_id' IS NULL THEN
    RAISE EXCEPTION 'Oracle delta parent identity required' USING ERRCODE='OO030';
  END IF;
  parent:=(p->>'task_id')::uuid;
  IF parent=t OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=t AND generation=g
      AND state='running' AND lease_until>clock_timestamp()) THEN
    RAISE EXCEPTION 'Oracle delta live child lease required' USING ERRCODE='OO010';
  END IF;
  SELECT * INTO a FROM _oo.native_c1_baseline_typed
    WHERE source=s AND task_id=parent AND acknowledged_at IS NOT NULL;
  IF NOT FOUND OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=parent AND state='succeeded') THEN
    RAISE EXCEPTION 'Oracle parent ACK not confirmed' USING ERRCODE='OO010';
  END IF;
  r:=a.receipt; b:=r->'oracle_buffer';
  SELECT * INTO w FROM _oo.source_watermark WHERE source=s;
  IF NOT FOUND OR w.state IS DISTINCT FROM 'live'
    OR w.source_generation IS DISTINCT FROM 1 OR w.target_generation IS DISTINCT FROM 1
    OR w.source_epoch IS DISTINCT FROM (p->>'source_epoch')::integer
    OR w.applied_pos IS DISTINCT FROM (r->>'applied_pos')::numeric
    OR w.position_codec IS DISTINCT FROM 'oracle-capture-sequence/v1'
    OR a.request_digest IS DISTINCT FROM p->>'request_digest'
    OR r->>'source_generation' IS DISTINCT FROM '1'
    OR r->>'target_generation' IS DISTINCT FROM '1'
    OR r->'source_instance'->>'engine' IS DISTINCT FROM 'oracle'
    OR r->'source_instance'->>'measured_id' IS DISTINCT FROM p->>'measured_id'
    OR r->'source_instance'->>'config_digest' IS DISTINCT FROM p->>'config_digest'
    OR r->'source_instance'->>'schema_digest' IS DISTINCT FROM p->>'schema_digest'
    OR r->>'source_epoch' IS DISTINCT FROM p->>'source_epoch'
    OR r->>'l_end' IS DISTINCT FROM p->>'l_end'
    OR b->>'context_sha256' IS DISTINCT FROM p->'buffer_context'->>'sha256'
    OR b->>'bundle_sha256' IS DISTINCT FROM p->>'bundle_sha256'
    OR b->'final_cursor' IS DISTINCT FROM p->'buffer_cursor'
    OR b->>'source_offset_write' IS DISTINCT FROM 'false' THEN
    RAISE EXCEPTION 'Oracle delta parent authority drift' USING ERRCODE='OO030';
  END IF;
  -- shortcut: nonzero parent cuts need a cross-task cursor predecessor proof;
  -- refuse until that proof and its recovery tests are implemented.
  IF w.applied_pos<>0 OR p->'buffer_cursor'->>'transaction_ordinal' IS DISTINCT FROM '0'
    OR p->'buffer_cursor'->'transaction_id' IS DISTINCT FROM 'null'::jsonb THEN
    RAISE EXCEPTION 'Oracle nonzero parent cursor not supported' USING ERRCODE='OO030';
  END IF;
  RETURN r;
END $fn$;

CREATE FUNCTION _oo.c1_oracle_delta_register(s text,t uuid,g bigint,p jsonb,h text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE r jsonb; i _oo.source_instance%ROWTYPE; old _oo.c1_typed_delta_seed%ROWTYPE;
BEGIN
  PERFORM pg_advisory_xact_lock(hashtext('oo-source-instance'),hashtext(s));
  r:=_oo.c1_oracle_delta_parent(s,t,g,p);
  IF h IS NULL OR h !~ '^[0-9a-f]{64}$' THEN
    RAISE EXCEPTION 'Oracle rollover digest required' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO i FROM _oo.source_instance
    WHERE source=s AND source_epoch=(p->>'source_epoch')::integer FOR UPDATE;
  IF NOT FOUND OR i.engine<>'oracle' OR i.measured_id IS DISTINCT FROM p->>'measured_id'
    OR i.config_digest IS DISTINCT FROM p->>'config_digest'
    OR i.schema_digest IS DISTINCT FROM p->>'schema_digest'
    OR (i.lease_id IS DISTINCT FROM (p->>'task_id')::uuid AND i.lease_id IS DISTINCT FROM t)
    OR (i.lease_id=t AND (i.lease_generation IS NULL OR i.lease_generation>g)) THEN
    RAISE EXCEPTION 'Oracle source lease handoff conflict' USING ERRCODE='OO010';
  END IF;
  SELECT * INTO old FROM _oo.c1_typed_delta_seed WHERE source=s AND task=t;
  IF FOUND AND (old.previous<>p OR old.rollover_sha256<>h) THEN
    RAISE EXCEPTION 'Oracle rollover replay conflict' USING ERRCODE='OO030';
  END IF;
  INSERT INTO _oo.c1_typed_delta_seed(source,task,parent_task,rollover_sha256,previous)
    VALUES(s,t,(p->>'task_id')::uuid,h,p) ON CONFLICT DO NOTHING;
  UPDATE _oo.source_instance SET lease_id=t,lease_generation=g
    WHERE source=s AND source_epoch=i.source_epoch;
  RETURN jsonb_build_object('protocol','oo.pipeline.native-source-instance/v1',
    'source',s,'source_epoch',i.source_epoch,'engine',i.engine,'measured_id',i.measured_id,
    'config_digest',i.config_digest,'schema_digest',i.schema_digest,
    'shard_id',i.shard_id,'owner_token',i.owner_token,'lease_id',t,'lease_generation',g);
END $fn$;

CREATE FUNCTION _oo.c1_oracle_delta_seed(s text,t uuid,g bigint,e integer,p jsonb,h text,n bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE seed _oo.c1_typed_delta_seed%ROWTYPE; child _oo.c1_typed_table%ROWTYPE;
  parent _oo.c1_typed_table%ROWTYPE; checkpoint bigint; parent_count bigint; child_count bigint;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  PERFORM _oo.native_c1_baseline_v2_assert(s,t,g,e);
  PERFORM _oo.c1_oracle_delta_parent(s,t,g,p);
  SELECT * INTO seed FROM _oo.c1_typed_delta_seed WHERE source=s AND task=t FOR UPDATE;
  IF NOT FOUND OR seed.previous<>p OR seed.rollover_sha256<>h THEN
    RAISE EXCEPTION 'Oracle delta registration absent' USING ERRCODE='OO030';
  END IF;
  SELECT seq INTO checkpoint FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t;
  IF checkpoint IS NULL OR checkpoint IS DISTINCT FROM n THEN
    RAISE EXCEPTION 'Oracle delta durable resume mismatch' USING ERRCODE='OO030';
  END IF;
  SELECT count(*) INTO parent_count FROM _oo.c1_typed_table WHERE source=s AND task=seed.parent_task;
  SELECT count(*) INTO child_count FROM _oo.c1_typed_table WHERE source=s AND task=t;
  IF parent_count=0 OR parent_count<>child_count THEN
    RAISE EXCEPTION 'Oracle delta table coverage' USING ERRCODE='OO030';
  END IF;
  IF NOT seed.seeded THEN
    IF checkpoint<>0 OR EXISTS(SELECT 1 FROM _oo.c1_typed_receipt WHERE source=s AND task=t) THEN
      RAISE EXCEPTION 'Oracle delta seed must precede events' USING ERRCODE='OO030';
    END IF;
    FOR child IN SELECT * FROM _oo.c1_typed_table WHERE source=s AND task=t ORDER BY name LOOP
      SELECT * INTO parent FROM _oo.c1_typed_table
        WHERE source=s AND task=seed.parent_task AND name=child.name;
      IF NOT FOUND OR NOT parent.snapshot_complete OR parent.spec<>child.spec THEN
        RAISE EXCEPTION 'Oracle delta parent table mismatch' USING ERRCODE='OO030';
      END IF;
      EXECUTE format('INSERT INTO _oo.%I SELECT * FROM _oo.%I',child.physical,parent.physical);
      EXECUTE format('INSERT INTO _oo.%I SELECT * FROM _oo.%I',child.tomb,parent.tomb);
      PERFORM _oo.c1_typed_complete(s,t,g,e,child.name);
    END LOOP;
    UPDATE _oo.c1_typed_delta_seed SET seeded=true WHERE source=s AND task=t;
  END IF;
  RETURN jsonb_build_object('outcome','open','rollover_sha256',h,
    'seeded_from_request_digest',p->>'request_digest','resumed_sequence',checkpoint,'source_epoch',e);
END $fn$;

REVOKE ALL ON FUNCTION
  _oo.c1_oracle_delta_parent(text,uuid,bigint,jsonb),
  _oo.c1_oracle_delta_register(text,uuid,bigint,jsonb,text),
  _oo.c1_oracle_delta_seed(text,uuid,bigint,integer,jsonb,text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
  _oo.c1_oracle_delta_register(text,uuid,bigint,jsonb,text),
  _oo.c1_oracle_delta_seed(text,uuid,bigint,integer,jsonb,text,bigint) TO oo_pipeline_runtime;
