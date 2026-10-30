-- MySQL typed generations retain the confirmed epoch and copy the immutable
-- parent stage. Target checkpoint, not highest applied receipt, authorizes resume.
CREATE TABLE _oo.c1_mysql_delta_seed (
  source text NOT NULL, task uuid NOT NULL, parent_task uuid NOT NULL,
  previous jsonb NOT NULL, seeded boolean NOT NULL DEFAULT false,
  PRIMARY KEY(source,task)
);
REVOKE ALL ON _oo.c1_mysql_delta_seed FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c1_mysql_delta_parent(s text,t uuid,g bigint,p jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE parent uuid; expected jsonb;
BEGIN
  parent:=(p->>'task_id')::uuid;
  IF parent IS NULL OR parent=t OR NOT EXISTS(SELECT 1 FROM jobs.tasks
    WHERE id=t AND generation=g AND state='running' AND lease_until>clock_timestamp()) THEN
    RAISE EXCEPTION 'mysql_delta_live_child' USING ERRCODE='OO010';
  END IF;
  SELECT jsonb_build_object('task_id',r.task_id,'l_end',substring(r.measured_id from 7)||':1-'||_oo.native_pos_text(r.l_end),
    'measured_id',r.measured_id,'config_digest',r.config_digest,'schema_digest',r.schema_digest,
    'source_epoch',r.source_epoch,'source_generation',w.source_generation,
    'target_generation',w.target_generation,'request_digest',a.request_digest)
    INTO expected
    FROM _oo.native_c1_baseline_run r
    JOIN _oo.native_c1_baseline_typed a USING(source,task_id)
    JOIN _oo.source_watermark w ON w.source=r.source
    JOIN _oo.source_instance i ON i.source=r.source AND i.source_epoch=r.source_epoch
    JOIN jobs.tasks j ON j.id=r.task_id
    WHERE r.source=s AND r.task_id=parent AND r.engine='mysql' AND i.engine='mysql'
    AND a.acknowledged_at IS NOT NULL AND j.state='succeeded'
    AND a.request_digest=a.receipt->>'request_digest'
    AND (a.receipt->>'source_generation')::bigint=w.source_generation
    AND (a.receipt->>'target_generation')::bigint=w.target_generation
    AND w.state='live' AND w.source_epoch=r.source_epoch AND w.applied_pos=r.l_end
    AND i.measured_id=r.measured_id AND i.config_digest=r.config_digest AND i.schema_digest=r.schema_digest;
  IF expected IS NULL OR expected IS DISTINCT FROM p OR NOT EXISTS(
      SELECT 1 FROM _oo.c1_typed_table WHERE source=s AND task=parent)
    OR EXISTS(SELECT 1 FROM _oo.c1_typed_table WHERE source=s AND task=parent AND NOT snapshot_complete) THEN
    RAISE EXCEPTION 'mysql_delta_parent_authority' USING ERRCODE='OO030';
  END IF;
  RETURN expected;
END $fn$;

CREATE FUNCTION _oo.c1_mysql_delta_register(s text,t uuid,g bigint,p jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE i _oo.source_instance%ROWTYPE; prior _oo.c1_mysql_delta_seed%ROWTYPE;
BEGIN
  PERFORM pg_advisory_xact_lock(hashtext('oo-source-instance'),hashtext(s));
  PERFORM _oo.c1_mysql_delta_parent(s,t,g,p);
  SELECT * INTO i FROM _oo.source_instance WHERE source=s AND source_epoch=(p->>'source_epoch')::integer FOR UPDATE;
  IF NOT FOUND OR i.engine<>'mysql' OR i.measured_id IS DISTINCT FROM p->>'measured_id'
    OR i.config_digest IS DISTINCT FROM p->>'config_digest' OR i.schema_digest IS DISTINCT FROM p->>'schema_digest'
    OR (i.lease_id IS DISTINCT FROM (p->>'task_id')::uuid AND i.lease_id IS DISTINCT FROM t)
    OR (i.lease_id=t AND (i.lease_generation IS NULL OR i.lease_generation>g)) THEN
    RAISE EXCEPTION 'mysql_delta_source_lease' USING ERRCODE='OO010';
  END IF;
  SELECT * INTO prior FROM _oo.c1_mysql_delta_seed WHERE source=s AND task=t;
  IF FOUND AND prior.previous IS DISTINCT FROM p THEN
    RAISE EXCEPTION 'mysql_delta_parent_conflict' USING ERRCODE='OO030';
  END IF;
  INSERT INTO _oo.c1_mysql_delta_seed(source,task,parent_task,previous)
    VALUES(s,t,(p->>'task_id')::uuid,p) ON CONFLICT DO NOTHING;
  UPDATE _oo.source_instance SET lease_id=t,lease_generation=g WHERE source=s AND source_epoch=i.source_epoch;
  RETURN jsonb_build_object('protocol','oo.pipeline.native-source-instance/v1',
    'source',s,'source_epoch',i.source_epoch,'engine',i.engine,'measured_id',i.measured_id,
    'config_digest',i.config_digest,'schema_digest',i.schema_digest,'shard_id',i.shard_id,
    'owner_token',i.owner_token,'lease_id',t,'lease_generation',g);
END $fn$;

CREATE FUNCTION _oo.c1_mysql_delta_resume(s text,t uuid,g bigint,p jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE r _oo.native_c1_baseline_run%ROWTYPE; seed _oo.c1_mysql_delta_seed%ROWTYPE;
  ack_sequence bigint; highest bigint; receipts bigint; last_ack _oo.c1_typed_receipt%ROWTYPE;
  pos numeric; digest text;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  PERFORM _oo.c1_mysql_delta_parent(s,t,g,p);
  SELECT * INTO r FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
  IF NOT FOUND THEN RETURN NULL; END IF;
  SELECT * INTO seed FROM _oo.c1_mysql_delta_seed WHERE source=s AND task=t;
  IF NOT FOUND OR NOT seed.seeded OR seed.previous IS DISTINCT FROM p
    OR r.engine<>'mysql' OR r.source_epoch<>(p->>'source_epoch')::integer
    OR r.measured_id IS DISTINCT FROM p->>'measured_id' OR r.config_digest IS DISTINCT FROM p->>'config_digest'
    OR r.schema_digest IS DISTINCT FROM p->>'schema_digest' OR r.l0 IS DISTINCT FROM p->>'l_end'
    OR r.lease_generation>g OR r.state NOT IN ('open','awaiting_publish')
    OR EXISTS(SELECT 1 FROM _oo.native_c1_baseline_typed WHERE source=s AND task_id=t AND acknowledged_at IS NOT NULL)
    OR NOT EXISTS(SELECT 1 FROM _oo.c1_typed_table WHERE source=s AND task=t)
    OR EXISTS(SELECT 1 FROM _oo.c1_typed_table WHERE source=s AND task=t AND NOT snapshot_complete) THEN
    RAISE EXCEPTION 'mysql_delta_resume_identity' USING ERRCODE='OO030';
  END IF;
  SELECT c.seq INTO ack_sequence FROM _oo.c1_typed_checkpoint c WHERE source=s AND task=t;
  SELECT coalesce(max(a.seq),0),count(*) INTO highest,receipts FROM _oo.c1_typed_receipt a WHERE source=s AND task=t;
  IF ack_sequence IS NULL OR highest<>receipts OR highest<ack_sequence OR highest>ack_sequence+1
    OR EXISTS(SELECT 1 FROM _oo.c1_typed_receipt WHERE source=s AND task=t AND kind<>'cdc')
    OR (r.state='awaiting_publish' AND highest<>ack_sequence) THEN
    RAISE EXCEPTION 'mysql_delta_resume_sequence' USING ERRCODE='OO030';
  END IF;
  pos:=r.l0_pos;
  IF ack_sequence>0 THEN
    SELECT * INTO last_ack FROM _oo.c1_typed_receipt a WHERE source=s AND task=t AND a.seq=ack_sequence;
    IF NOT FOUND THEN RAISE EXCEPTION 'mysql_delta_checkpoint_absent' USING ERRCODE='OO030'; END IF;
    pos:=last_ack.position; digest:=last_ack.digest;
  END IF;
  RETURN jsonb_build_object('protocol','oo.mysql.typed-resume/v1','task_id',t,
    'source_epoch',r.source_epoch,'l0',r.l0,'snapshot_complete',true,
    'sequence',ack_sequence,'position',_oo.native_pos_text(pos),'digest',digest,
    'config_digest',r.config_digest,'schema_digest',r.schema_digest,'state',r.state,
    'typed_tables',(SELECT jsonb_object_agg(name,spec) FROM _oo.c1_typed_table WHERE source=s AND task=t));
END $fn$;

CREATE FUNCTION _oo.c1_mysql_delta_seed_rows(s text,t uuid,g bigint,e integer,p jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE seed _oo.c1_mysql_delta_seed%ROWTYPE; child _oo.c1_typed_table%ROWTYPE;
  parent _oo.c1_typed_table%ROWTYPE; parent_count bigint; child_count bigint;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  PERFORM _oo.native_c1_baseline_v2_assert(s,t,g,e);
  PERFORM _oo.c1_mysql_delta_parent(s,t,g,p);
  SELECT * INTO seed FROM _oo.c1_mysql_delta_seed WHERE source=s AND task=t FOR UPDATE;
  IF NOT FOUND OR seed.previous IS DISTINCT FROM p THEN
    RAISE EXCEPTION 'mysql_delta_registration' USING ERRCODE='OO030';
  END IF;
  SELECT count(*) INTO parent_count FROM _oo.c1_typed_table WHERE source=s AND task=seed.parent_task;
  SELECT count(*) INTO child_count FROM _oo.c1_typed_table WHERE source=s AND task=t;
  IF parent_count=0 OR parent_count<>child_count THEN
    RAISE EXCEPTION 'mysql_delta_coverage' USING ERRCODE='OO030';
  END IF;
  IF NOT seed.seeded THEN
    IF EXISTS(SELECT 1 FROM _oo.c1_typed_receipt WHERE source=s AND task=t)
      OR NOT EXISTS(SELECT 1 FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t AND seq=0) THEN
      RAISE EXCEPTION 'mysql_delta_seed_after_apply' USING ERRCODE='OO030';
    END IF;
    FOR child IN SELECT * FROM _oo.c1_typed_table WHERE source=s AND task=t ORDER BY name LOOP
      SELECT * INTO parent FROM _oo.c1_typed_table WHERE source=s AND task=seed.parent_task AND name=child.name;
      IF NOT FOUND OR NOT parent.snapshot_complete OR parent.spec<>child.spec THEN
        RAISE EXCEPTION 'mysql_delta_table_identity' USING ERRCODE='OO030';
      END IF;
      EXECUTE format('INSERT INTO _oo.%I SELECT * FROM _oo.%I',child.physical,parent.physical);
      EXECUTE format('INSERT INTO _oo.%I SELECT * FROM _oo.%I',child.tomb,parent.tomb);
      PERFORM _oo.c1_typed_complete(s,t,g,e,child.name);
    END LOOP;
    UPDATE _oo.c1_mysql_delta_seed SET seeded=true WHERE source=s AND task=t;
  END IF;
END $fn$;

CREATE FUNCTION _oo.c1_mysql_delta_seal(s text,t uuid,g bigint,e integer,p jsonb,ending text,pos numeric,tables text[])
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE r _oo.native_c1_baseline_run%ROWTYPE; names text[];
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  PERFORM _oo.c1_mysql_delta_resume(s,t,g,p);
  SELECT * INTO r FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
  IF NOT FOUND OR r.source_epoch<>e OR r.lease_generation<>g THEN
    RAISE EXCEPTION 'mysql_delta_seal_lease' USING ERRCODE='OO010';
  END IF;
  IF r.state='awaiting_publish' THEN
    SELECT array_agg(name ORDER BY name) INTO names FROM _oo.c1_typed_table WHERE source=s AND task=t;
    IF pos IS DISTINCT FROM r.l_end OR ending IS DISTINCT FROM substring(r.measured_id from 7)||':1-'||_oo.native_pos_text(r.l_end)
      OR names IS DISTINCT FROM tables THEN
      RAISE EXCEPTION 'mysql_delta_seal_replay' USING ERRCODE='OO030';
    END IF;
    RETURN jsonb_build_object('outcome','sealed','l_end',ending,'applied_pos',_oo.native_pos_text(pos));
  END IF;
  RETURN _oo.c1_typed_seal(s,t,g,e,ending,pos,tables);
END $fn$;

REVOKE ALL ON FUNCTION
 _oo.c1_mysql_delta_parent(text,uuid,bigint,jsonb),
 _oo.c1_mysql_delta_register(text,uuid,bigint,jsonb),
 _oo.c1_mysql_delta_resume(text,uuid,bigint,jsonb),
 _oo.c1_mysql_delta_seed_rows(text,uuid,bigint,integer,jsonb),
 _oo.c1_mysql_delta_seal(text,uuid,bigint,integer,jsonb,text,numeric,text[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
 _oo.c1_mysql_delta_register(text,uuid,bigint,jsonb),
 _oo.c1_mysql_delta_resume(text,uuid,bigint,jsonb),
 _oo.c1_mysql_delta_seed_rows(text,uuid,bigint,integer,jsonb),
 _oo.c1_mysql_delta_seal(text,uuid,bigint,integer,jsonb,text,numeric,text[]) TO oo_pipeline_runtime;
