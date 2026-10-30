-- Retain all private stages and history. Only an unapplied, cancelled delta may
-- release its source lease back to the exact acknowledged parent.
CREATE TABLE _oo.c1_mysql_abandoned_delta (
 source text NOT NULL, task uuid NOT NULL, generation bigint NOT NULL,
 parent_task uuid NOT NULL, evidence jsonb NOT NULL,
 abandoned_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(source,task)
);
REVOKE ALL ON _oo.c1_mysql_abandoned_delta FROM PUBLIC,oo_pipeline_runtime;

CREATE FUNCTION _oo.c1_mysql_abandon_unapplied_delta(s text,t uuid,g bigint,b uuid,p uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $f$
DECLARE seed _oo.c1_mysql_delta_seed%ROWTYPE; i _oo.source_instance%ROWTYPE;
 w _oo.source_watermark%ROWTYPE; r _oo.native_c1_baseline_run%ROWTYPE;
 a _oo.native_c1_baseline_typed%ROWTYPE; parent_generation bigint; proof jsonb;
BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('oo-source-instance'),hashtext(s));
 PERFORM 1 FROM jw_native.bindings WHERE source_id=s AND target_database=current_database() ORDER BY id FOR UPDATE;
 PERFORM 1 FROM jw_native.source_progress WHERE source_id=s AND target_database=current_database() FOR UPDATE;
 SELECT * INTO seed FROM _oo.c1_mysql_delta_seed WHERE source=s AND task=t FOR UPDATE;
 IF NOT FOUND OR NOT seed.seeded THEN
  RAISE EXCEPTION 'mysql_abandon_delta_required' USING ERRCODE='OO030'; END IF;
 SELECT * INTO w FROM _oo.source_watermark WHERE source=s FOR UPDATE;
 SELECT * INTO i FROM _oo.source_instance WHERE source=s AND source_epoch=w.source_epoch FOR UPDATE;
 SELECT * INTO r FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t FOR UPDATE;
 SELECT * INTO a FROM _oo.native_c1_baseline_typed WHERE source=s AND task_id=seed.parent_task;
 SELECT generation INTO parent_generation FROM jobs.tasks
  WHERE id=seed.parent_task AND project_id=p AND state='succeeded';
 IF NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=t AND project_id=p AND generation=g AND state='cancelled')
  OR NOT EXISTS(SELECT 1 FROM jw_native.bindings WHERE id=b AND project_id=p AND source_id=s
   AND target_database=current_database() AND method='C1' AND NOT enabled
   AND manifest->>'c1_baseline_format'='typed-keys/v3')
  OR EXISTS(SELECT 1 FROM jw_native.bindings WHERE source_id=s AND target_database=current_database() AND enabled)
  OR NOT EXISTS(SELECT 1 FROM jw_native.occurrences WHERE task_id=t AND binding_id=b AND request IS NULL)
  OR NOT EXISTS(SELECT 1 FROM jw_native.source_progress q WHERE q.source_id=s AND q.target_database=current_database()
   AND q.active_task_id=t AND q.task_id=seed.parent_task AND q.binding_id=b AND q.source_generation=w.source_generation)
  OR EXISTS(SELECT 1 FROM jw_native.pending_acks WHERE task_id=t)
  OR w.source IS NULL OR w.state<>'live' OR i.source IS NULL OR i.engine<>'mysql'
  OR i.lease_id IS DISTINCT FROM t OR i.lease_generation IS DISTINCT FROM g
  OR r.source IS NULL OR r.engine<>'mysql' OR r.state<>'open' OR r.source_epoch<>w.source_epoch
  OR r.l0_pos<>w.applied_pos OR r.applied_pos<>r.l0_pos OR r.l_end IS NOT NULL
  OR r.measured_id IS DISTINCT FROM i.measured_id OR r.config_digest IS DISTINCT FROM i.config_digest
  OR r.schema_digest IS DISTINCT FROM i.schema_digest
  OR EXISTS(SELECT 1 FROM _oo.c1_typed_receipt WHERE source=s AND task=t)
  OR NOT EXISTS(SELECT 1 FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t AND seq=0)
  OR EXISTS(SELECT 1 FROM _oo.native_c1_baseline_typed WHERE source=s AND task_id=t)
  OR EXISTS(SELECT 1 FROM _oo.full_import_run WHERE source_id=s AND source_generation>w.source_generation AND state<>'retired')
  OR a.source IS NULL OR a.acknowledged_at IS NULL OR parent_generation IS NULL
  OR a.request_digest IS DISTINCT FROM seed.previous->>'request_digest'
  OR (a.receipt->>'source_generation')::bigint IS DISTINCT FROM w.source_generation
  OR (a.receipt->>'target_generation')::bigint IS DISTINCT FROM w.target_generation
  OR (a.receipt->>'source_epoch')::integer IS DISTINCT FROM w.source_epoch
  OR (a.receipt->>'applied_pos')::numeric IS DISTINCT FROM w.applied_pos
  OR NOT EXISTS(SELECT 1 FROM jw_native.pending_acks WHERE task_id=seed.parent_task
   AND acknowledged_at IS NOT NULL AND ack_request->>'request_digest'=a.request_digest)
 THEN RAISE EXCEPTION 'mysql_abandon_not_unapplied' USING ERRCODE='OO010'; END IF;
 proof:=jsonb_build_object('protocol','oo.mysql.unapplied-delta-abandon/v1',
  'source_id',s,'task_id',t,'lease_generation',g,'parent_task',seed.parent_task,
  'source_epoch',w.source_epoch,'source_generation',w.source_generation,'target_generation',w.target_generation,
  'applied_pos',_oo.native_pos_text(w.applied_pos),'source_offset_reset',false,
  'private_stages_preserved',true,'published_data_changed',false);
 INSERT INTO _oo.c1_mysql_abandoned_delta(source,task,generation,parent_task,evidence)
  VALUES(s,t,g,seed.parent_task,proof);
 UPDATE _oo.source_instance SET lease_id=seed.parent_task,lease_generation=parent_generation
  WHERE source=s AND source_epoch=w.source_epoch;
 RETURN proof;
END $f$;
REVOKE ALL ON FUNCTION _oo.c1_mysql_abandon_unapplied_delta(text,uuid,bigint,uuid,uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c1_mysql_abandon_unapplied_delta(text,uuid,bigint,uuid,uuid) TO oo_pipeline_runtime;
