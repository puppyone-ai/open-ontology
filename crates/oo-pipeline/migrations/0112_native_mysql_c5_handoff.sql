-- Explicit capture revision. No old offset, receipt, publication or epoch is rewritten.
CREATE TABLE _oo.mysql_c5_handoff (
 source text PRIMARY KEY, binding uuid NOT NULL, project uuid NOT NULL,
 plan_sha256 text NOT NULL, old_state jsonb NOT NULL, new_manifest_sha256 text NOT NULL,
 config_sha256 text NOT NULL, consent jsonb NOT NULL, source_probe jsonb NOT NULL,
 confirmed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 task uuid, source_epoch integer, l0 text
);
REVOKE ALL ON _oo.mysql_c5_handoff FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.mysql_handoff_inspect(s text,b uuid,p uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE binding record; progress record;
 w _oo.source_watermark%ROWTYPE; result jsonb;
BEGIN
 SELECT * INTO binding FROM jw_native.bindings WHERE id=b;
 SELECT * INTO progress FROM jw_native.source_progress
  WHERE source_id=s AND target_database=current_database();
 SELECT * INTO w FROM _oo.source_watermark WHERE source=s;
 IF binding.id IS NULL OR binding.source_id<>s OR binding.project_id<>p
  OR binding.target_database<>current_database() OR binding.method<>'C5'
  OR binding.manifest->>'method'<>'C5' OR binding.enabled
  OR progress.binding_id IS DISTINCT FROM b OR progress.active_task_id IS NOT NULL
  OR progress.source_generation IS NULL OR progress.source_generation<1
  OR w.source IS NULL OR w.state<>'live' OR w.position_codec<>'snapshot-zero/v1'
  OR w.source_generation<>progress.source_generation
  OR EXISTS(SELECT 1 FROM jw_native.bindings WHERE source_id=s
   AND target_database=current_database() AND enabled)
  OR EXISTS(SELECT 1 FROM jw_native.bindings x JOIN jw_native.occurrences o ON o.binding_id=x.id
    JOIN jobs.tasks j ON j.id=o.task_id LEFT JOIN jw_native.pending_acks a ON a.task_id=j.id
    WHERE x.source_id=s AND x.target_database=current_database()
     AND (j.state IN ('queued','running') OR a.task_id IS NOT NULL AND a.acknowledged_at IS NULL))
  OR NOT EXISTS(SELECT 1 FROM jobs.tasks j JOIN jw_native.watermarks k ON k.task_id=j.id
    WHERE j.id=progress.task_id AND j.state='succeeded' AND j.project_id=p
    AND k.binding_id=b AND k.source_generation=w.source_generation)
  OR EXISTS(SELECT 1 FROM _oo.mysql_c5_handoff WHERE source=s)
 THEN RAISE EXCEPTION 'mysql_handoff_stop_drain_published_required' USING ERRCODE='OO010'; END IF;
 SELECT jsonb_build_object('source_id',s,'binding',b,'project_id',p,
  'source_generation',w.source_generation,'target_generation',w.target_generation,
  'previous_task',progress.task_id,'watermark',to_jsonb(w),'manifest_sha256',binding.manifest_sha,
  'manifest',binding.manifest,'published_runs',
   (SELECT jsonb_agg(jsonb_build_object('run_id',r.run_id,'source_generation',r.source_generation,
      'target_generation',r.target_generation) ORDER BY r.run_id)
    FROM _oo.full_import_run r WHERE r.source_id=s AND r.state='published'))
 INTO result;
 IF result->'published_runs' IS NULL OR jsonb_array_length(result->'published_runs')=0 THEN
  RAISE EXCEPTION 'mysql_handoff_old_publication_required' USING ERRCODE='OO010'; END IF;
 RETURN result;
END $f$;

CREATE FUNCTION _oo.mysql_handoff_confirm(s text,b uuid,p uuid,plan text,old jsonb,
 newsha text,configsha text,consent jsonb,probe jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE actual jsonb;
BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('oo-source-instance'),hashtext(s));
 PERFORM 1 FROM jw_native.bindings WHERE source_id=s AND target_database=current_database() ORDER BY id FOR UPDATE;
 PERFORM 1 FROM jw_native.source_progress WHERE source_id=s AND target_database=current_database() FOR UPDATE;
 PERFORM 1 FROM _oo.source_watermark WHERE source=s FOR UPDATE;
 actual:=_oo.mysql_handoff_inspect(s,b,p);
 IF actual IS DISTINCT FROM old OR plan !~ '^[0-9a-f]{64}$' OR newsha !~ '^[0-9a-f]{64}$'
  OR configsha !~ '^[0-9a-f]{64}$'
  OR consent->>'protocol' IS DISTINCT FROM 'oo.mysql.c5-to-c1/v1'
  OR consent->>'source_id' IS DISTINCT FROM s OR consent->>'binding' IS DISTINCT FROM b::text
  OR consent->>'project_id' IS DISTINCT FROM p::text
  OR consent->>'baseline' IS DISTINCT FROM 'new_snapshot_plus_log_reconciliation'
  OR consent->>'prior_history' IS DISTINCT FROM 'not_recoverable_from_new_logs'
  OR probe->>'server_uuid' IS DISTINCT FROM consent->>'expected_server_uuid'
  OR probe->>'source_id' IS DISTINCT FROM s OR probe->>'business_rows_read' IS DISTINCT FROM '0'
  OR probe->'source_preflight'->>'log_bin' IS DISTINCT FROM '1'
 THEN RAISE EXCEPTION 'mysql_handoff_plan_changed' USING ERRCODE='OO030'; END IF;
 INSERT INTO _oo.mysql_c5_handoff(source,binding,project,plan_sha256,old_state,
  new_manifest_sha256,config_sha256,consent,source_probe)
 VALUES(s,b,p,plan,old,newsha,configsha,consent,probe);
 RETURN jsonb_build_object('protocol','oo.mysql.c5-to-c1/v1','source_id',s,'binding',b,'project_id',p,
  'capture_revision',2,'source_generation',old->'source_generation','baseline',consent->'baseline',
  'old_publication_preserved',true,'offset_reset',false,'enabled',false);
END $f$;

-- Worker resolves the transition from stored authority, never from a claimed prior GTID.
CREATE FUNCTION _oo.mysql_handoff_context(s text,b uuid,p uuid,t uuid,g bigint,base bigint,configsha text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE h _oo.mysql_c5_handoff%ROWTYPE;
BEGIN
 SELECT * INTO h FROM _oo.mysql_c5_handoff WHERE source=s;
 IF NOT FOUND OR base>(h.old_state->>'source_generation')::bigint THEN RETURN NULL; END IF;
 IF h.binding<>b OR h.project<>p OR h.config_sha256<>configsha
  OR base IS DISTINCT FROM (h.old_state->>'source_generation')::bigint
  OR h.task IS NOT NULL AND h.task<>t
  OR NOT EXISTS(SELECT 1 FROM jw_native.bindings x JOIN jw_native.occurrences o ON o.binding_id=x.id
    JOIN jobs.tasks j ON j.id=o.task_id JOIN jw_native.source_progress q
    ON q.source_id=x.source_id AND q.target_database=x.target_database
    WHERE x.id=b AND x.project_id=p AND x.source_id=s AND x.target_database=current_database()
     AND x.method='C1' AND x.manifest_sha=h.new_manifest_sha256
     AND o.manifest_sha=h.new_manifest_sha256 AND j.id=t AND j.generation=g
     AND j.state='running' AND j.lease_until>clock_timestamp() AND q.active_task_id=t
     AND q.source_generation=base)
 THEN RAISE EXCEPTION 'mysql_handoff_claim_identity' USING ERRCODE='OO030'; END IF;
 RETURN h.consent||jsonb_build_object('plan_sha256',h.plan_sha256,'parent_source_generation',base);
END $f$;

CREATE FUNCTION _oo.mysql_handoff_register(s text,t uuid,g bigint,configsha text,
 measured text,new_config text,new_schema text,owner uuid,boundary text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE h _oo.mysql_c5_handoff%ROWTYPE; old _oo.source_instance%ROWTYPE; instance jsonb;
BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('oo-source-instance'),hashtext(s));
 SELECT * INTO STRICT h FROM _oo.mysql_c5_handoff WHERE source=s FOR UPDATE;
 PERFORM _oo.mysql_handoff_context(s,h.binding,h.project,t,g,
  (h.old_state->>'source_generation')::bigint,configsha);
 IF measured IS DISTINCT FROM 'mysql.'||(h.consent->>'expected_server_uuid')
  OR boundary IS NULL OR boundary !~ ('^'||(h.consent->>'expected_server_uuid')||':1-[1-9][0-9]*$')
  OR h.l0 IS NOT NULL AND h.l0<>boundary
  OR NOT EXISTS(SELECT 1 FROM _oo.full_import_run WHERE source_id=s AND owner_token=owner AND state='published')
 THEN RAISE EXCEPTION 'mysql_handoff_boundary_identity' USING ERRCODE='OO030'; END IF;
 SELECT * INTO old FROM _oo.source_instance WHERE source=s ORDER BY source_epoch DESC LIMIT 1;
 -- Existing C1 epochs are not C5 history. Do not re-label or reset them.
 IF FOUND AND (old.lease_id<>t OR old.measured_id<>measured
   OR old.config_digest<>new_config OR old.schema_digest<>new_schema) THEN
  RAISE EXCEPTION 'mysql_handoff_existing_epoch_conflict' USING ERRCODE='OO030'; END IF;
 instance:=_oo.native_source_instance_register(s,owner,g,s,'mysql',measured,new_config,new_schema,t);
 UPDATE _oo.mysql_c5_handoff SET task=t,source_epoch=(instance->>'source_epoch')::integer,l0=boundary
  WHERE source=s;
 RETURN instance;
END $f$;

CREATE FUNCTION _oo.mysql_handoff_final_assert(s text,t uuid,g bigint,sg bigint) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE h _oo.mysql_c5_handoff%ROWTYPE; w _oo.source_watermark%ROWTYPE;
BEGIN
 SELECT * INTO w FROM _oo.source_watermark WHERE source=s FOR UPDATE;
 IF NOT FOUND OR w.position_codec<>'snapshot-zero/v1' THEN RETURN; END IF;
 SELECT * INTO h FROM _oo.mysql_c5_handoff WHERE source=s;
 IF NOT FOUND OR h.task IS DISTINCT FROM t OR sg<>(h.old_state->>'source_generation')::bigint+1
  OR to_jsonb(w) IS DISTINCT FROM h.old_state->'watermark'
  OR NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_run r WHERE r.source=s AND r.task_id=t
    AND r.lease_generation=g AND r.source_epoch=h.source_epoch AND r.l0=h.l0
    AND r.state='awaiting_publish' AND r.l_end=r.applied_pos)
 THEN RAISE EXCEPTION 'mysql_handoff_final_barrier' USING ERRCODE='OO030'; END IF;
END $f$;

REVOKE ALL ON FUNCTION _oo.mysql_handoff_inspect(text,uuid,uuid),
 _oo.mysql_handoff_confirm(text,uuid,uuid,text,jsonb,text,text,jsonb,jsonb),
 _oo.mysql_handoff_context(text,uuid,uuid,uuid,bigint,bigint,text),
 _oo.mysql_handoff_register(text,uuid,bigint,text,text,text,text,uuid,text),
 _oo.mysql_handoff_final_assert(text,uuid,bigint,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.mysql_handoff_inspect(text,uuid,uuid),
 _oo.mysql_handoff_confirm(text,uuid,uuid,text,jsonb,text,text,jsonb,jsonb),
 _oo.mysql_handoff_context(text,uuid,uuid,uuid,bigint,bigint,text),
 _oo.mysql_handoff_register(text,uuid,bigint,text,text,text,text,uuid,text),
 _oo.mysql_handoff_final_assert(text,uuid,bigint,bigint) TO oo_pipeline_runtime;
