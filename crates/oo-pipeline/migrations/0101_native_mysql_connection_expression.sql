-- Preserve installed 0100; disambiguate JSON extraction before key deletion.
CREATE OR REPLACE FUNCTION _oo.mysql_connection_rebind(s text,b uuid,p uuid,old_manifest text,old_registration text,
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
  OR ((d->'admission')-'secret_ref') IS DISTINCT FROM ((c.document->'admission')-'secret_ref')
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
