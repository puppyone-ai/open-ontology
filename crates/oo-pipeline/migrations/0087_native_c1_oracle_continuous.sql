-- Additive upgrade: preserve installed 0080-0086 and their historical receipts.
-- Cross-task predecessors must be durable parent transactions, never invented ACKs.

CREATE OR REPLACE FUNCTION _oo.c1_oracle_delta_parent(s text,t uuid,g bigint,p jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE a _oo.native_c1_baseline_typed%ROWTYPE; w _oo.source_watermark%ROWTYPE;
  parent uuid; r jsonb; b jsonb;
BEGIN
  IF p->>'protocol' IS DISTINCT FROM 'oo.jobs.oracle-buffer-previous/v1'
    OR p->>'engine' IS DISTINCT FROM 'oracle' OR p->>'source_id' IS DISTINCT FROM s
    OR p->>'source_generation' IS NULL OR p->>'source_generation' !~ '^[1-9][0-9]{0,18}$'
    OR p->>'target_generation' IS NULL OR p->>'target_generation' !~ '^[1-9][0-9]{0,18}$'
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
    OR w.source_generation IS DISTINCT FROM (p->>'source_generation')::bigint
    OR w.target_generation IS DISTINCT FROM (p->>'target_generation')::bigint
    OR w.source_epoch IS DISTINCT FROM (p->>'source_epoch')::integer
    OR w.applied_pos IS DISTINCT FROM (r->>'applied_pos')::numeric
    OR w.position_codec IS DISTINCT FROM 'oracle-capture-sequence/v1'
    OR a.request_digest IS DISTINCT FROM p->>'request_digest'
    OR r->>'source_generation' IS DISTINCT FROM p->>'source_generation'
    OR r->>'target_generation' IS DISTINCT FROM p->>'target_generation'
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
  IF p->'buffer_cursor'->>'transaction_ordinal' IS DISTINCT FROM _oo.native_pos_text(w.applied_pos)
    OR (w.applied_pos=0 AND p->'buffer_cursor'->'transaction_id' IS DISTINCT FROM 'null'::jsonb)
    OR (w.applied_pos>0 AND NOT EXISTS(
      SELECT 1 FROM _oo.c1_typed_buffer_position bp JOIN _oo.c1_typed_checkpoint cp
      ON cp.source=bp.source AND cp.task=bp.task AND cp.seq=bp.seq
      WHERE bp.source=s AND bp.task=parent AND bp.apply_ordinal=w.applied_pos
      AND bp.transaction_id=p->'buffer_cursor'->>'transaction_id'
      AND bp.evidence->'buffer_position'->'cursor_after'=p->'buffer_cursor'
      AND bp.commit_scn::text=b->>'commit_scn')) THEN
    RAISE EXCEPTION 'Oracle parent cursor not durably confirmed' USING ERRCODE='OO030';
  END IF;
  RETURN r;
END $fn$;

CREATE OR REPLACE FUNCTION _oo.c1_typed_record_buffer_0080(s text,t uuid,g bigint,e integer,p_seq bigint,p_digest text,p jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE b _oo.c1_typed_capture_bundle%ROWTYPE; r _oo.c1_typed_receipt%ROWTYPE;
  old _oo.c1_typed_buffer_position%ROWTYPE; prev _oo.c1_typed_buffer_position%ROWTYPE; checkpoint jsonb;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  PERFORM _oo.c1_typed_assert_engine(s,t,g,e,'oracle');
  SELECT * INTO b FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t;
  IF NOT FOUND OR p->>'protocol' IS DISTINCT FROM 'oo.c1.oracle-buffer-cursor/v1'
    OR p->>'bundle_sha256' IS DISTINCT FROM b.bundle_sha256
    OR p->>'spool_sha256' IS DISTINCT FROM b.spool_sha256
    OR p->>'source_offset_advanced' IS DISTINCT FROM 'false' THEN
    RAISE EXCEPTION 'captured bundle binding conflict' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO r FROM _oo.c1_typed_receipt WHERE source=s AND task=t AND seq=p_seq AND digest=p_digest AND kind='cdc';
  checkpoint:=p->'checkpoint';
  IF NOT FOUND OR r.position::text IS DISTINCT FROM p->>'apply_ordinal'
    OR (checkpoint->>'application_position')::numeric IS DISTINCT FROM r.position
    OR r.position>b.final_apply_ordinal
    OR checkpoint->>'transaction_id' IS NULL
    OR checkpoint->>'commit_scn' IS NULL OR checkpoint->>'commit_scn' !~ '^[1-9][0-9]{0,37}$'
    OR checkpoint->>'event_sha256' IS NULL OR checkpoint->>'event_sha256' !~ '^[0-9a-f]{64}$'
    OR (checkpoint->>'event_count')::bigint NOT BETWEEN 1 AND 100000 THEN
    RAISE EXCEPTION 'captured complete transaction required' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO old FROM _oo.c1_typed_buffer_position WHERE source=s AND task=t AND seq=p_seq;
  IF FOUND THEN
    IF old.evidence<>p THEN RAISE EXCEPTION 'captured transaction replay conflict' USING ERRCODE='OO030'; END IF;
    RETURN;
  END IF;
  SELECT * INTO prev FROM _oo.c1_typed_buffer_position WHERE source=s AND task=t ORDER BY apply_ordinal DESC LIMIT 1;
  IF NOT FOUND AND EXISTS(SELECT 1 FROM _oo.c1_typed_delta_seed WHERE source=s AND task=t) THEN
    IF p_seq<>1 THEN RAISE EXCEPTION 'missing prior child transaction' USING ERRCODE='OO030'; END IF;
    SELECT bp.* INTO prev FROM _oo.c1_typed_delta_seed d
      JOIN _oo.c1_typed_buffer_position bp ON bp.source=d.source AND bp.task=d.parent_task
      JOIN _oo.native_c1_baseline_run run ON run.source=d.source AND run.task_id=d.task
      WHERE d.source=s AND d.task=t AND d.seeded
      AND bp.apply_ordinal=(d.previous->'buffer_cursor'->>'transaction_ordinal')::bigint
      AND bp.apply_ordinal=run.l0_pos
      AND bp.transaction_id=d.previous->'buffer_cursor'->>'transaction_id';
    IF NOT FOUND AND EXISTS(SELECT 1 FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t AND l0_pos<>0) THEN
      RAISE EXCEPTION 'durable parent predecessor absent' USING ERRCODE='OO030';
    END IF;
  END IF;
  IF r.position<>COALESCE(prev.apply_ordinal,0)+1 OR (checkpoint->>'commit_scn')::numeric<COALESCE(prev.commit_scn,b.snapshot_scn) THEN
    RAISE EXCEPTION 'buffer order or source SCN regression' USING ERRCODE='OO030';
  END IF;
  INSERT INTO _oo.c1_typed_buffer_position VALUES(s,t,p_seq,r.position::bigint,checkpoint->>'transaction_id',
    (checkpoint->>'commit_scn')::numeric,p);
END $fn$;

CREATE OR REPLACE FUNCTION _oo.c1_typed_record_buffer(s text,t uuid,g bigint,e integer,p_seq bigint,p_digest text,p jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE v jsonb; previous_id text; ordinal bigint; previous_ordinal bigint;
BEGIN
  PERFORM _oo.c1_typed_assert_buffer_context(s,t,g,e,p);
  IF p ? 'context_sha256' THEN
    v:=p->'buffer_position';
    IF v IS NULL OR v->>'protocol' IS DISTINCT FROM 'oo.c1.oracle-buffer-position/v1'
      OR v->>'checkpoint_kind' IS DISTINCT FROM 'buffer_consumption'
      OR v->>'source_offset_write' IS DISTINCT FROM 'false'
      OR v->>'context_sha256' IS DISTINCT FROM p->>'context_sha256'
      OR v->>'bundle_sha256' IS DISTINCT FROM p->>'bundle_sha256'
      OR v->>'spool_sha256' IS DISTINCT FROM p->>'spool_sha256'
      OR v->>'typed_batch_digest' IS DISTINCT FROM p_digest
      OR v->>'target_apply_ordinal' IS DISTINCT FROM p->>'apply_ordinal'
      OR v->'cursor_after'->>'transaction_ordinal' IS DISTINCT FROM p->>'apply_ordinal'
      OR v->'cursor_after'->>'transaction_id' IS DISTINCT FROM p->'checkpoint'->>'transaction_id'
      OR v->>'transaction_id' IS DISTINCT FROM p->'checkpoint'->>'transaction_id'
      OR v->>'commit_scn' IS DISTINCT FROM p->'checkpoint'->>'commit_scn'
      OR v->>'events_sha256' IS DISTINCT FROM p->'checkpoint'->>'event_sha256'
      OR v->>'event_count' IS DISTINCT FROM p->'checkpoint'->>'event_count'
      OR v->>'end_event_count' IS DISTINCT FROM v->>'event_count'
      OR v->>'event_order_first' IS DISTINCT FROM '1'
      OR v->>'event_order_last' IS DISTINCT FROM v->>'event_count'
      OR p->>'source_evidence_sha256' IS NULL OR p->>'source_evidence_sha256' !~ '^[0-9a-f]{64}$'
      OR v->'captured_event_ids'->>'sha256' IS NULL OR v->'captured_event_ids'->>'sha256' !~ '^[0-9a-f]{64}$'
      OR v->'end_evidence'->>'sha256' IS NULL OR v->'end_evidence'->>'sha256' !~ '^[0-9a-f]{64}$' THEN
      RAISE EXCEPTION 'complete pinned buffer transaction required' USING ERRCODE='OO030';
    END IF;
    ordinal:=(v->>'target_apply_ordinal')::bigint;
    previous_ordinal:=(v->'cursor_before'->>'transaction_ordinal')::bigint;
    IF previous_ordinal IS NULL OR previous_ordinal<>ordinal-1 THEN
      RAISE EXCEPTION 'noncontiguous buffer cursor' USING ERRCODE='OO030';
    END IF;
    IF previous_ordinal>0 THEN
      SELECT transaction_id INTO previous_id FROM _oo.c1_typed_buffer_position
        WHERE source=s AND task=t AND apply_ordinal=previous_ordinal;
      IF NOT FOUND AND p_seq=1 THEN
        SELECT bp.transaction_id INTO previous_id FROM _oo.c1_typed_delta_seed d
          JOIN _oo.c1_typed_buffer_position bp ON bp.source=d.source AND bp.task=d.parent_task
          JOIN _oo.native_c1_baseline_run run ON run.source=d.source AND run.task_id=d.task
          WHERE d.source=s AND d.task=t AND d.seeded AND run.l0_pos=previous_ordinal
          AND bp.apply_ordinal=previous_ordinal
          AND d.previous->'buffer_cursor'=v->'cursor_before'
          AND bp.transaction_id=v->'cursor_before'->>'transaction_id';
      END IF;
      IF previous_id IS NULL THEN RAISE EXCEPTION 'previous buffer transaction absent' USING ERRCODE='OO030'; END IF;
    END IF;
    IF v->'cursor_before'->>'transaction_id' IS DISTINCT FROM previous_id THEN
      RAISE EXCEPTION 'previous buffer transaction conflict' USING ERRCODE='OO030';
    END IF;
  END IF;
  PERFORM _oo.c1_typed_record_buffer_0080(s,t,g,e,p_seq,p_digest,p);
END $fn$;

CREATE OR REPLACE FUNCTION _oo.c1_oracle_delta_seed(s text,t uuid,g bigint,e integer,p jsonb,h text,n bigint)
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
    UPDATE _oo.native_c1_baseline_run SET
      l0='oracle-buffer:'||(SELECT metadata->>'context_sha256' FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t)||':'||(p->'buffer_cursor'->>'transaction_ordinal'),
      l0_pos=(p->'buffer_cursor'->>'transaction_ordinal')::numeric,
      applied_pos=(p->'buffer_cursor'->>'transaction_ordinal')::numeric
      WHERE source=s AND task_id=t;
    UPDATE _oo.c1_typed_delta_seed SET seeded=true WHERE source=s AND task=t;
  END IF;
  RETURN jsonb_build_object('outcome','open','rollover_sha256',h,
    'seeded_from_request_digest',p->>'request_digest','resumed_sequence',checkpoint,'source_epoch',e);
END $fn$;

CREATE OR REPLACE FUNCTION _oo.c1_typed_open_oracle(s text,t uuid,g bigint,e integer,p_offset text)
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
  IF old.engine<>'oracle' OR old.source_epoch<>e OR old.lease_generation>g
    OR old.measured_id<>i.measured_id OR old.config_digest<>i.config_digest OR old.schema_digest<>i.schema_digest
    OR old.state NOT IN ('open','awaiting_publish')
    OR (old.lease_generation<>g AND NOT EXISTS(SELECT 1 FROM _oo.c1_typed_delta_seed WHERE source=s AND task=t))
    OR (old.l0<>p_offset AND NOT EXISTS(SELECT 1 FROM _oo.c1_typed_capture_bundle b
      JOIN _oo.c1_typed_delta_seed d ON d.source=b.source AND d.task=b.task AND d.seeded
      WHERE b.source=s AND b.task=t AND b.bundle_sha256=p_offset
      AND old.l0='oracle-buffer:'||(b.metadata->>'context_sha256')||':'||_oo.native_pos_text(old.l0_pos))) THEN
    RAISE EXCEPTION 'Oracle baseline replay conflict' USING ERRCODE='OO010';
  END IF;
  UPDATE _oo.native_c1_baseline_run SET lease_generation=g WHERE source=s AND task_id=t;
  INSERT INTO _oo.native_c1_baseline_v2_format(source,task_id,format)
    VALUES(s,t,'chunked-typed/v2') ON CONFLICT DO NOTHING;
END $fn$;

CREATE OR REPLACE FUNCTION _oo.c1_typed_seal(s text,t uuid,g bigint,e integer,p_end text,p_pos numeric,p_tables text[])
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE; r _oo.c1_typed_receipt%ROWTYPE;
  ack_seq bigint; tables text[]; incomplete boolean; buffer jsonb;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  SELECT * INTO v FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
  IF v.state='awaiting_publish' AND v.engine='oracle' THEN
    PERFORM _oo.c1_oracle_delta_final_assert(s,t,g,e);
  ELSE
    v:=_oo.native_c1_baseline_v2_assert(s,t,g,e);
  END IF;
  SELECT array_agg(name ORDER BY name),bool_or(NOT snapshot_complete) INTO tables,incomplete
    FROM _oo.c1_typed_table WHERE source=s AND task=t;
  IF tables IS NULL OR tables IS DISTINCT FROM p_tables OR incomplete THEN
    RAISE EXCEPTION 'typed_snapshot_incomplete' USING ERRCODE='OO030';
  END IF;
  SELECT * INTO r FROM _oo.c1_typed_receipt WHERE source=s AND task=t ORDER BY seq DESC LIMIT 1;
  SELECT seq INTO ack_seq FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t;
  IF p_pos IS NULL OR p_pos<>COALESCE(r.position,v.l0_pos)
    OR ack_seq IS DISTINCT FROM COALESCE(r.seq,0)
    OR (v.engine='mysql' AND p_end IS DISTINCT FROM substring(v.measured_id from 7)||':1-'||_oo.native_pos_text(p_pos)) THEN
    RAISE EXCEPTION 'typed_barrier' USING ERRCODE='OO030';
  END IF;
  IF v.engine='oracle' THEN
    buffer:=_oo.c1_oracle_buffer_summary(s,t);
    IF buffer IS NULL OR buffer->>'context_sha256' IS NULL
      OR (buffer->'final_cursor'->>'transaction_ordinal')::numeric IS DISTINCT FROM p_pos
      OR p_end IS DISTINCT FROM 'oracle-buffer:'||(buffer->>'context_sha256')||':'||_oo.native_pos_text(p_pos)
      OR NOT EXISTS(SELECT 1 FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t AND final_apply_ordinal=p_pos) THEN
      RAISE EXCEPTION 'exact complete Oracle buffer cut required' USING ERRCODE='OO030';
    END IF;
    UPDATE _oo.native_c1_baseline_run SET l0='oracle-buffer:'||(buffer->>'context_sha256')||':'||_oo.native_pos_text(v.l0_pos) WHERE source=s AND task_id=t;
  ELSIF v.engine<>'mysql' THEN RAISE EXCEPTION 'unsupported C1 engine' USING ERRCODE='OO030'; END IF;
  UPDATE _oo.native_c1_baseline_v2_format SET sealed=true WHERE source=s AND task_id=t;
  UPDATE _oo.native_c1_baseline_run SET applied_pos=p_pos,l_end=p_pos,state='awaiting_publish',sealed_at=clock_timestamp()
    WHERE source=s AND task_id=t;
  RETURN jsonb_build_object('outcome','sealed','l_end',p_end,'applied_pos',_oo.native_pos_text(p_pos)) || CASE WHEN buffer IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('source_engine','oracle','buffer',buffer) END;
END $fn$;

CREATE FUNCTION _oo.c1_oracle_delta_sealed_resume(s text,t uuid,g bigint,e integer,p jsonb,h text,n bigint,tables jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE r _oo.native_c1_baseline_run%ROWTYPE; d _oo.c1_typed_delta_seed%ROWTYPE;
  actual jsonb; seq bigint; last_seq bigint;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  SELECT * INTO r FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
  IF NOT FOUND OR r.state<>'awaiting_publish' THEN RETURN NULL; END IF;
  PERFORM _oo.c1_oracle_delta_parent(s,t,g,p);
  PERFORM _oo.c1_oracle_delta_final_assert(s,t,g,e);
  SELECT * INTO d FROM _oo.c1_typed_delta_seed WHERE source=s AND task=t;
  SELECT jsonb_object_agg(name,spec) INTO actual FROM _oo.c1_typed_table WHERE source=s AND task=t;
  SELECT cp.seq INTO seq FROM _oo.c1_typed_checkpoint cp WHERE source=s AND task=t;
  SELECT max(a.seq) INTO last_seq FROM _oo.c1_typed_receipt a WHERE source=s AND task=t;
  IF d.previous IS DISTINCT FROM p OR d.rollover_sha256 IS DISTINCT FROM h
    OR seq IS DISTINCT FROM n OR seq IS DISTINCT FROM last_seq OR seq<1
    OR actual IS DISTINCT FROM tables THEN
    RAISE EXCEPTION 'Oracle sealed resume binding conflict' USING ERRCODE='OO030';
  END IF;
  RETURN jsonb_build_object('outcome','open','rollover_sha256',h,
    'seeded_from_request_digest',p->>'request_digest','resumed_sequence',seq,'source_epoch',e);
END $fn$;
REVOKE ALL ON FUNCTION _oo.c1_oracle_delta_sealed_resume(text,uuid,bigint,integer,jsonb,text,bigint,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.c1_oracle_delta_sealed_resume(text,uuid,bigint,integer,jsonb,text,bigint,jsonb) TO oo_pipeline_runtime;
