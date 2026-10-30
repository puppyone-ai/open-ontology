-- Explicit all-in-one stopped-runtime transport CAS. No old receipt or cursor is rewritten.
CREATE TABLE _oo.mysql_connection_current (
 source text PRIMARY KEY, binding uuid NOT NULL, project uuid NOT NULL, epoch integer NOT NULL,
 revision bigint NOT NULL CHECK(revision>0), registration_sha text NOT NULL,
 config_digest text NOT NULL, schema_digest text NOT NULL, measured_id text NOT NULL,
 document jsonb NOT NULL
);
CREATE TABLE _oo.mysql_connection_history (
 source text NOT NULL, revision bigint NOT NULL, registration_sha text NOT NULL,
 config_digest text NOT NULL, schema_digest text NOT NULL, epoch integer NOT NULL,
 document jsonb NOT NULL, evidence jsonb NOT NULL, recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(source,revision), UNIQUE(source,registration_sha)
);
REVOKE ALL ON _oo.mysql_connection_current,_oo.mysql_connection_history FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.mysql_connection_schema(s text) RETURNS text
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
 SELECT schema_digest FROM _oo.mysql_connection_current WHERE source=s
$f$;

CREATE FUNCTION _oo.mysql_connection_job(s text,t uuid,g bigint)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE c _oo.mysql_connection_current%ROWTYPE;
BEGIN
 SELECT * INTO c FROM _oo.mysql_connection_current WHERE source=s;
 IF NOT FOUND THEN RETURN; END IF;
 IF NOT EXISTS(
  SELECT 1 FROM jw_native.occurrences o JOIN jw_native.bindings b ON b.id=o.binding_id
  JOIN jw_native.source_progress p ON p.source_id=b.source_id AND p.target_database=b.target_database
  JOIN jobs.tasks j ON j.id=o.task_id
  WHERE o.task_id=t AND b.id=c.binding AND b.project_id=c.project AND b.source_id=s
   AND b.target_database=current_database() AND p.active_task_id=t
   AND j.project_id=c.project AND j.state='running' AND j.generation=g AND j.lease_until>clock_timestamp()
   AND o.manifest->'source_registration'=c.document->'registration'
   AND b.manifest->'source_registration'=c.document->'registration'
 ) THEN RAISE EXCEPTION 'mysql_identity_job_binding' USING ERRCODE='OO010'; END IF;
END $f$;

CREATE FUNCTION _oo.mysql_connection_bind(s text,t uuid,g bigint,e integer,b uuid,p uuid,d jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE i _oo.source_instance%ROWTYPE; old _oo.mysql_connection_current%ROWTYPE; r jsonb;
BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('oo-source-instance'),hashtext(s));
 SELECT * INTO STRICT i FROM _oo.source_instance WHERE source=s AND source_epoch=e FOR UPDATE;
 r:=d->'record';
 IF i.engine<>'mysql' OR i.lease_id<>t OR i.lease_generation<>g
  OR i.measured_id IS DISTINCT FROM 'mysql.'||(r->'physical_identity'->>'server_uuid')
  OR r->>'engine' IS DISTINCT FROM 'mysql' OR r->>'source_id' IS DISTINCT FROM s
  OR i.config_digest IS DISTINCT FROM d->>'config_digest' OR i.schema_digest IS DISTINCT FROM d->>'schema_digest'
  OR d->'admission'->>'source_id' IS DISTINCT FROM s OR d->'admission'->>'target_id' IS DISTINCT FROM s
  OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=t AND generation=g AND state='running' AND lease_until>clock_timestamp())
  OR NOT EXISTS(SELECT 1 FROM jw_native.bindings WHERE id=b AND project_id=p AND source_id=s
   AND target_database=current_database() AND manifest->'source_registration'=d->'registration') THEN
  RAISE EXCEPTION 'mysql_identity_initial_binding' USING ERRCODE='OO030';
 END IF;
 SELECT * INTO old FROM _oo.mysql_connection_current WHERE source=s FOR UPDATE;
 IF FOUND THEN
  IF old.binding<>b OR old.project<>p OR old.epoch<>e OR old.document IS DISTINCT FROM d THEN
   RAISE EXCEPTION 'mysql_identity_current_changed' USING ERRCODE='OO030'; END IF;
 ELSE
  IF EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=s)
   OR (r->>'connection_revision')::bigint<>1 OR (r->>'membership_revision')::bigint<>1
   OR r->>'previous_registration_sha256' IS NOT NULL THEN
   RAISE EXCEPTION 'mysql_identity_adoption_required' USING ERRCODE='OO030'; END IF;
  INSERT INTO _oo.mysql_connection_current VALUES(s,b,p,e,1,d->'registration'->'record'->>'sha256',
   i.config_digest,i.schema_digest,i.measured_id,d);
  INSERT INTO _oo.mysql_connection_history(source,revision,registration_sha,config_digest,schema_digest,epoch,document,evidence)
   VALUES(s,1,d->'registration'->'record'->>'sha256',i.config_digest,i.schema_digest,e,d,jsonb_build_object('initial_task',t));
 END IF;
 PERFORM _oo.mysql_connection_job(s,t,g);
 RETURN jsonb_build_object('source_id',s,'source_epoch',e,'connection_revision',(r->>'connection_revision')::bigint,
   'registration_sha256',d->'registration'->'record'->>'sha256','config_digest',i.config_digest);
END $f$;

CREATE FUNCTION _oo.mysql_connection_project_parent(s text,p jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE c _oo.mysql_connection_current%ROWTYPE;
BEGIN
 SELECT * INTO c FROM _oo.mysql_connection_current WHERE source=s;
 IF NOT FOUND THEN RETURN p; END IF;
 IF (p->>'source_epoch')::integer IS DISTINCT FROM c.epoch
  OR p->>'measured_id' IS DISTINCT FROM c.measured_id OR p->>'schema_digest' IS DISTINCT FROM c.schema_digest
  OR NOT EXISTS(SELECT 1 FROM _oo.mysql_connection_history h WHERE h.source=s AND h.epoch=c.epoch
     AND h.config_digest=p->>'config_digest' AND h.schema_digest=c.schema_digest AND h.revision<=c.revision)
  OR NOT EXISTS(SELECT 1 FROM _oo.source_instance i WHERE i.source=s AND i.source_epoch=c.epoch
     AND i.measured_id=c.measured_id AND i.schema_digest=c.schema_digest AND i.config_digest=c.config_digest) THEN
  RAISE EXCEPTION 'mysql_identity_parent_history' USING ERRCODE='OO030'; END IF;
 RETURN p||jsonb_build_object('config_digest',c.config_digest,'parent_config_digest',p->>'config_digest',
   'connection_revision',c.revision,'source_registration_sha256',c.registration_sha);
END $f$;

CREATE FUNCTION _oo.mysql_connection_rebind(s text,b uuid,p uuid,old_manifest text,old_registration text,
 d jsonb,new_manifest text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE c _oo.mysql_connection_current%ROWTYPE; r jsonb; old_r jsonb; w _oo.source_watermark%ROWTYPE;
 a _oo.native_c1_baseline_typed%ROWTYPE; i _oo.source_instance%ROWTYPE; proof jsonb;
BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('oo-source-instance'),hashtext(s));
 PERFORM 1 FROM jw_native.bindings WHERE source_id=s AND target_database=current_database() ORDER BY id FOR UPDATE;
 PERFORM 1 FROM jw_native.source_progress WHERE source_id=s AND target_database=current_database() FOR UPDATE;
 SELECT * INTO STRICT c FROM _oo.mysql_connection_current WHERE source=s FOR UPDATE;
 SELECT * INTO STRICT w FROM _oo.source_watermark WHERE source=s FOR UPDATE;
 SELECT * INTO STRICT i FROM _oo.source_instance WHERE source=s AND source_epoch=c.epoch FOR UPDATE;
 SELECT * INTO a FROM _oo.native_c1_baseline_typed WHERE source=s AND task_id=i.lease_id;
 r:=d->'record';old_r:=c.document->'record';
 IF old_manifest IS NULL OR old_manifest !~ '^[0-9a-f]{64}$'
  OR new_manifest IS NULL OR new_manifest !~ '^[0-9a-f]{64}$'
  OR old_registration IS NULL OR old_registration !~ '^[0-9a-f]{64}$'
  OR COALESCE(d->>'config_digest','') !~ '^[0-9a-f]{64}$'
  OR COALESCE(d->'registration'->'record'->>'sha256','') !~ '^[0-9a-f]{64}$' THEN
  RAISE EXCEPTION 'mysql_identity_digest' USING ERRCODE='OO030'; END IF;
 IF c.binding<>b OR c.project<>p OR c.registration_sha IS DISTINCT FROM old_registration
  OR NOT EXISTS(SELECT 1 FROM jw_native.bindings WHERE id=b AND project_id=p AND source_id=s
    AND target_database=current_database() AND NOT enabled AND manifest_sha=old_manifest
    AND manifest->'source_registration'=c.document->'registration')
  OR EXISTS(SELECT 1 FROM jw_native.bindings WHERE source_id=s AND target_database=current_database() AND enabled)
  OR EXISTS(SELECT 1 FROM jw_native.source_progress WHERE source_id=s AND target_database=current_database() AND active_task_id IS NOT NULL)
  OR EXISTS(SELECT 1 FROM jw_native.bindings x JOIN jw_native.occurrences o ON o.binding_id=x.id
    JOIN jobs.tasks j ON j.id=o.task_id LEFT JOIN jw_native.pending_acks k ON k.task_id=j.id
    WHERE x.source_id=s AND x.target_database=current_database()
      AND (j.state IN ('queued','running') OR k.task_id IS NOT NULL AND k.acknowledged_at IS NULL))
  OR w.state<>'live' OR w.source_epoch<>c.epoch OR i.config_digest<>c.config_digest OR i.schema_digest<>c.schema_digest
  OR a.task_id IS NULL OR a.acknowledged_at IS NULL
  OR (a.receipt->>'source_generation')::bigint<>w.source_generation
  OR (a.receipt->>'target_generation')::bigint<>w.target_generation
  OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=i.lease_id AND state='succeeded')
  OR NOT EXISTS(SELECT 1 FROM jw_native.source_progress q JOIN jw_native.pending_acks k ON k.task_id=q.task_id
    WHERE q.source_id=s AND q.target_database=current_database() AND q.task_id=i.lease_id
     AND q.source_generation=w.source_generation AND k.acknowledged_at IS NOT NULL
     AND k.ack_request->>'request_digest'=a.request_digest) THEN
  RAISE EXCEPTION 'mysql_identity_stop_drain_cas' USING ERRCODE='OO010'; END IF;
 IF r->>'previous_registration_sha256' IS DISTINCT FROM old_registration
  OR (r->>'connection_revision')::bigint IS DISTINCT FROM c.revision+1
  OR (r-ARRAY['connection_revision','connection_sha256','operator_config_sha256','observation','previous_registration_sha256'])
     IS DISTINCT FROM (old_r-ARRAY['connection_revision','connection_sha256','operator_config_sha256','observation','previous_registration_sha256'])
  OR r->>'connection_sha256' IS NOT DISTINCT FROM old_r->>'connection_sha256'
  OR d->'registration'->'registry' IS DISTINCT FROM c.document->'registration'->'registry'
  OR d->'invariant_config' IS DISTINCT FROM c.document->'invariant_config'
  OR d->>'schema_digest' IS DISTINCT FROM c.schema_digest
  OR (d->'admission'-'secret_ref') IS DISTINCT FROM (c.document->'admission'-'secret_ref')
  OR d->>'config_digest' IS NULL OR d->'registration'->'record'->>'sha256' IS NULL THEN
  RAISE EXCEPTION 'mysql_identity_transport_only' USING ERRCODE='OO030'; END IF;
 proof:=jsonb_build_object('protocol','oo.mysql.connection-rebind/v1','source_id',s,'binding',b,'project',p,
   'source_epoch',c.epoch,'source_generation',w.source_generation,'target_generation',w.target_generation,
   'applied_pos',_oo.native_pos_text(w.applied_pos),'parent_task',i.lease_id,'parent_request_digest',a.request_digest,
   'old_registration_sha256',c.registration_sha,'new_registration_sha256',d->'registration'->'record'->>'sha256',
   'old_config_digest',c.config_digest,'new_config_digest',d->>'config_digest',
   'old_manifest_sha256',old_manifest,'new_manifest_sha256',new_manifest,'connection_revision',c.revision+1,
   'source_offset_reset',false,'enabled',false);
 INSERT INTO _oo.mysql_connection_history(source,revision,registration_sha,config_digest,schema_digest,epoch,document,evidence)
  VALUES(s,c.revision+1,d->'registration'->'record'->>'sha256',d->>'config_digest',c.schema_digest,c.epoch,d,proof);
 UPDATE _oo.mysql_connection_current SET revision=c.revision+1,registration_sha=d->'registration'->'record'->>'sha256',
  config_digest=d->>'config_digest',document=d WHERE source=s;
 -- The mutable current instance records the REAL new admission config. Old run
 -- configs/receipts stay immutable; history is required by the parent guard.
 UPDATE _oo.source_instance SET config_digest=d->>'config_digest' WHERE source=s AND source_epoch=c.epoch;
 RETURN proof;
END $f$;

CREATE OR REPLACE FUNCTION _oo.c1_mysql_delta_parent(s text,t uuid,g bigint,p jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE parent uuid; expected jsonb; managed boolean;
BEGIN
 parent:=(p->>'task_id')::uuid;
 IF parent IS NULL OR parent=t OR NOT EXISTS(SELECT 1 FROM jobs.tasks
   WHERE id=t AND generation=g AND state='running' AND lease_until>clock_timestamp()) THEN
  RAISE EXCEPTION 'mysql_delta_live_child' USING ERRCODE='OO010'; END IF;
 SELECT EXISTS(SELECT 1 FROM _oo.mysql_connection_current WHERE source=s) INTO managed;
 IF managed THEN PERFORM _oo.mysql_connection_job(s,t,g); END IF;
 SELECT jsonb_build_object('task_id',r.task_id,'l_end',substring(r.measured_id from 7)||':1-'||_oo.native_pos_text(r.l_end),
   'measured_id',r.measured_id,'config_digest',r.config_digest,'schema_digest',r.schema_digest,
   'source_epoch',r.source_epoch,'source_generation',w.source_generation,'target_generation',w.target_generation,
   'request_digest',a.request_digest)
  INTO expected FROM _oo.native_c1_baseline_run r JOIN _oo.native_c1_baseline_typed a USING(source,task_id)
  JOIN _oo.source_watermark w ON w.source=r.source
  JOIN _oo.source_instance i ON i.source=r.source AND i.source_epoch=r.source_epoch
  JOIN jobs.tasks j ON j.id=r.task_id
  WHERE r.source=s AND r.task_id=parent AND r.engine='mysql' AND i.engine='mysql'
   AND a.acknowledged_at IS NOT NULL AND j.state='succeeded'
   AND a.request_digest=a.receipt->>'request_digest'
   AND (a.receipt->>'source_generation')::bigint=w.source_generation
   AND (a.receipt->>'target_generation')::bigint=w.target_generation
   AND w.state='live' AND w.source_epoch=r.source_epoch AND w.applied_pos=r.l_end
   AND i.measured_id=r.measured_id AND i.schema_digest=r.schema_digest
   AND (managed OR i.config_digest=r.config_digest);
 IF managed AND expected IS NOT NULL THEN expected:=_oo.mysql_connection_project_parent(s,expected); END IF;
 IF expected IS NULL OR expected IS DISTINCT FROM p OR NOT EXISTS(SELECT 1 FROM _oo.c1_typed_table WHERE source=s AND task=parent)
  OR EXISTS(SELECT 1 FROM _oo.c1_typed_table WHERE source=s AND task=parent AND NOT snapshot_complete) THEN
  RAISE EXCEPTION 'mysql_delta_parent_authority' USING ERRCODE='OO030'; END IF;
 RETURN expected;
END $f$;

REVOKE ALL ON FUNCTION _oo.mysql_connection_job(text,uuid,bigint),
 _oo.mysql_connection_schema(text),
 _oo.mysql_connection_bind(text,uuid,bigint,integer,uuid,uuid,jsonb),
 _oo.mysql_connection_project_parent(text,jsonb),
 _oo.mysql_connection_rebind(text,uuid,uuid,text,text,jsonb,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.mysql_connection_bind(text,uuid,bigint,integer,uuid,uuid,jsonb),
 _oo.mysql_connection_schema(text),
 _oo.mysql_connection_project_parent(text,jsonb),
 _oo.mysql_connection_rebind(text,uuid,uuid,text,text,jsonb,text) TO oo_pipeline_runtime;
