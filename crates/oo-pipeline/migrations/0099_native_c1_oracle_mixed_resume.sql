-- Same-task initial TARGET recovery: typed sequence and captured CDC ordinal differ.
-- The installed 0097 pure-fill contract and all source offsets remain unchanged.
CREATE FUNCTION _oo.c1_oracle_mixed_check(s text,t uuid,g bigint,e integer,p jsonb,n bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE r _oo.native_c1_baseline_run%ROWTYPE; i _oo.source_instance%ROWTYPE;
 b _oo.c1_typed_capture_bundle%ROWTYPE; tab _oo.c1_typed_table%ROWTYPE;
 a _oo.c1_typed_receipt%ROWTYPE; bp _oo.c1_typed_buffer_position%ROWTYPE;
 cp bigint; last_seq bigint; receipt_count bigint; total bigint; fills bigint; final_pos bigint;
 k bigint:=0; entry jsonb; names text[]; wanted text[]; completed text[]:='{}'; got bigint;
BEGIN
 PERFORM _oo.native_c1_baseline_lock(s,t);
 SELECT * INTO r FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
 IF NOT FOUND OR r.engine<>'oracle' OR r.state NOT IN ('open','awaiting_publish')
  OR r.source_epoch<>e OR r.lease_generation<1 OR r.lease_generation>g OR r.l0_pos<>0
  OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=t AND generation=g AND state='running' AND lease_until>clock_timestamp())
  OR EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=s AND source_generation>0)
  OR EXISTS(SELECT 1 FROM _oo.c1_typed_delta_seed WHERE source=s AND task=t)
  OR EXISTS(SELECT 1 FROM _oo.native_c1_baseline_typed WHERE source=s AND task_id=t AND acknowledged_at IS NOT NULL)
 THEN RAISE EXCEPTION 'Oracle mixed live initial task required' USING ERRCODE='OO010'; END IF;
 SELECT * INTO i FROM _oo.source_instance WHERE source=s AND source_epoch=e;
 IF NOT FOUND OR i.engine<>'oracle' OR i.lease_id<>t OR i.lease_generation<>r.lease_generation
  OR i.measured_id<>r.measured_id OR i.config_digest<>r.config_digest OR i.schema_digest<>r.schema_digest
 THEN RAISE EXCEPTION 'Oracle mixed instance drift' USING ERRCODE='OO010'; END IF;
 SELECT * INTO b FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t;
 IF NOT FOUND OR p->>'protocol' IS DISTINCT FROM 'oo.oracle.initial-mixed/v1'
  OR p IS DISTINCT FROM b.metadata->'initial_fill'
  OR p->>'source_id' IS DISTINCT FROM s OR p->>'config_digest' IS DISTINCT FROM r.config_digest
  OR p->>'typed_schema_sha256' IS DISTINCT FROM r.schema_digest
  OR p->>'bundle_sha256' IS DISTINCT FROM b.bundle_sha256
  OR p->'buffer_context'->>'sha256' IS DISTINCT FROM b.metadata->>'context_sha256'
  OR p->'buffer_index' IS DISTINCT FROM b.metadata->'buffer_index'
  OR b.metadata->>'source_id' IS DISTINCT FROM s
  OR b.metadata->>'typed_schema_sha256' IS DISTINCT FROM r.schema_digest
  OR b.metadata->>'checkpoint_kind' IS DISTINCT FROM 'buffer_consumption'
  OR b.metadata->>'source_offset_write' IS DISTINCT FROM 'false'
  OR b.metadata ? 'rollover_sha256'
  OR jsonb_typeof(p->'batches') IS DISTINCT FROM 'array'
  OR jsonb_typeof(p->'tables') IS DISTINCT FROM 'object'
  OR (r.l0<>b.bundle_sha256 AND r.l0<>'oracle-buffer:'||(b.metadata->>'context_sha256')||':0')
 THEN RAISE EXCEPTION 'Oracle mixed immutable binding drift' USING ERRCODE='OO030'; END IF;
 total:=(p->>'batch_count')::bigint; fills:=(p->>'fill_batch_count')::bigint;
 final_pos:=(p->>'final_position')::bigint;
 IF total IS NULL OR total NOT BETWEEN 2 AND 4096 OR fills IS NULL OR fills NOT BETWEEN 1 AND total-1
  OR final_pos IS DISTINCT FROM total-fills OR final_pos IS DISTINCT FROM b.final_apply_ordinal
  OR jsonb_array_length(p->'batches')<>total
  OR r.applied_pos IS DISTINCT FROM (CASE WHEN r.state='awaiting_publish' THEN final_pos ELSE 0 END)::numeric
 THEN RAISE EXCEPTION 'Oracle mixed bounded plan required' USING ERRCODE='OO030'; END IF;
 FOR entry IN SELECT value FROM jsonb_array_elements(p->'batches') LOOP
  k:=k+1;
  IF (entry->>'sequence')::bigint IS DISTINCT FROM k OR entry->>'digest' IS NULL
   OR entry->>'digest' !~ '^[0-9a-f]{64}$'
   OR (k<=fills AND (entry->>'kind' IS DISTINCT FROM 'fill' OR entry->>'position' IS DISTINCT FROM '0'
     OR entry->>'previous_position' IS DISTINCT FROM '0' OR entry ? 'source_evidence_sha256'))
   OR (k>fills AND (entry->>'kind' IS DISTINCT FROM 'cdc' OR entry->>'position' IS DISTINCT FROM (k-fills)::text
     OR entry->>'previous_position' IS DISTINCT FROM (k-fills-1)::text
     OR entry->>'source_evidence_sha256' IS NULL OR entry->>'source_evidence_sha256' !~ '^[0-9a-f]{64}$'))
  THEN RAISE EXCEPTION 'Oracle mixed explicit sequence/cursor plan' USING ERRCODE='OO030'; END IF;
 END LOOP;
 SELECT seq INTO cp FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t;
 SELECT coalesce(max(seq),0),count(*) INTO last_seq,receipt_count FROM _oo.c1_typed_receipt WHERE source=s AND task=t;
 IF n IS NULL OR n<0 OR n>total OR cp IS DISTINCT FROM n OR last_seq<n OR last_seq>n+1
  OR last_seq>total OR receipt_count<>last_seq
 THEN RAISE EXCEPTION 'Oracle mixed durable checkpoint required' USING ERRCODE='OO030'; END IF;
 FOR a IN SELECT * FROM _oo.c1_typed_receipt WHERE source=s AND task=t ORDER BY seq LOOP
  entry:=p->'batches'->(a.seq::integer-1);
  IF a.seq<1 OR a.seq>total OR a.digest IS DISTINCT FROM entry->>'digest'
   OR a.kind IS DISTINCT FROM entry->>'kind' OR _oo.native_pos_text(a.position) IS DISTINCT FROM entry->>'position'
  THEN RAISE EXCEPTION 'Oracle mixed receipt conflict' USING ERRCODE='OO030'; END IF;
  IF a.kind='cdc' THEN
   SELECT * INTO bp FROM _oo.c1_typed_buffer_position WHERE source=s AND task=t AND seq=a.seq;
   IF NOT FOUND OR bp.apply_ordinal::text IS DISTINCT FROM entry->>'position'
    OR bp.evidence->>'source_evidence_sha256' IS DISTINCT FROM entry->>'source_evidence_sha256'
    OR bp.evidence->>'context_sha256' IS DISTINCT FROM b.metadata->>'context_sha256'
    OR bp.evidence->>'bundle_sha256' IS DISTINCT FROM b.bundle_sha256
    OR bp.evidence#>>'{buffer_position,typed_batch_digest}' IS DISTINCT FROM a.digest
    OR bp.evidence#>>'{buffer_position,source_offset_write}' IS DISTINCT FROM 'false'
    OR bp.evidence#>>'{buffer_position,cursor_before,transaction_ordinal}' IS DISTINCT FROM entry->>'previous_position'
    OR bp.evidence#>>'{buffer_position,cursor_after,transaction_ordinal}' IS DISTINCT FROM entry->>'position'
   THEN RAISE EXCEPTION 'Oracle mixed captured END evidence conflict' USING ERRCODE='OO030'; END IF;
  END IF;
 END LOOP;
 SELECT array_agg(name ORDER BY name) INTO names FROM _oo.c1_typed_table WHERE source=s AND task=t;
 SELECT array_agg(key ORDER BY key) INTO wanted FROM jsonb_object_keys(p->'tables') AS key;
 IF names IS NULL OR wanted IS NULL OR names IS DISTINCT FROM wanted
 THEN RAISE EXCEPTION 'Oracle mixed table coverage' USING ERRCODE='OO030'; END IF;
 FOR tab IN SELECT * FROM _oo.c1_typed_table WHERE source=s AND task=t ORDER BY name LOOP
  entry:=p->'tables'->tab.name;
  IF entry->>'last_sequence' IS NULL OR (entry->>'last_sequence')::bigint NOT BETWEEN 1 AND fills
   OR entry->>'rows' IS NULL OR (entry->>'rows')::bigint<0
  THEN RAISE EXCEPTION 'Oracle mixed fill table plan' USING ERRCODE='OO030'; END IF;
  IF tab.snapshot_complete THEN
   IF (entry->>'last_sequence')::bigint>n
   THEN RAISE EXCEPTION 'Oracle mixed incomplete fill marker' USING ERRCODE='OO030'; END IF;
   -- CDC can legitimately change row count after the exact fill was completed.
   IF last_seq<=fills THEN
    EXECUTE format('SELECT count(*) FROM _oo.%I',tab.physical) INTO got;
    IF got<>(entry->>'rows')::bigint
    THEN RAISE EXCEPTION 'Oracle mixed fill row coverage' USING ERRCODE='OO030'; END IF;
   END IF;
   completed:=array_append(completed,tab.name);
  END IF;
 END LOOP;
 IF last_seq>fills AND completed IS DISTINCT FROM names
 THEN RAISE EXCEPTION 'Oracle mixed CDC before complete fill' USING ERRCODE='OO030'; END IF;
 IF r.state='awaiting_publish' THEN
  IF n<>total OR last_seq<>n OR completed IS DISTINCT FROM names OR r.l_end IS DISTINCT FROM final_pos::numeric
   OR NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_v2_format WHERE source=s AND task_id=t AND sealed)
  THEN RAISE EXCEPTION 'Oracle mixed unsealed final cut' USING ERRCODE='OO030'; END IF;
 ELSIF NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_v2_format WHERE source=s AND task_id=t AND NOT sealed) THEN
  RAISE EXCEPTION 'Oracle mixed state drift' USING ERRCODE='OO030';
 END IF;
 RETURN jsonb_build_object('outcome','open','source_epoch',e,'resumed_sequence',n,
  'buffer_context_sha256',b.metadata->>'context_sha256','bundle_sha256',b.bundle_sha256,
  'completed_tables',to_jsonb(completed));
END $fn$;

ALTER FUNCTION _oo.c1_oracle_fill_check(text,uuid,bigint,integer,jsonb,bigint) RENAME TO c1_oracle_fill_check_0099_base;
CREATE FUNCTION _oo.c1_oracle_fill_check(s text,t uuid,g bigint,e integer,p jsonb,n bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
BEGIN
 IF p->>'protocol'='oo.oracle.initial-mixed/v1' THEN
  RETURN _oo.c1_oracle_mixed_check(s,t,g,e,p,n);
 END IF;
 RETURN _oo.c1_oracle_fill_check_0099_base(s,t,g,e,p,n);
END $fn$;

ALTER FUNCTION _oo.c1_typed_begin(text,uuid,bigint,integer,bigint,text,numeric,numeric,text) RENAME TO c1_typed_begin_0099_base;
CREATE FUNCTION _oo.c1_typed_begin(s text,t uuid,g bigint,e integer,p_seq bigint,p_digest text,p_prev numeric,p_pos numeric,p_kind text)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE plan jsonb; item jsonb;
BEGIN
 PERFORM _oo.native_c1_baseline_lock(s,t);
 SELECT metadata->'initial_fill' INTO plan FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t;
 IF plan->>'protocol'='oo.oracle.initial-mixed/v1' THEN
  item:=plan->'batches'->(p_seq::integer-1);
  IF p_seq IS NULL OR p_seq<1 OR p_seq>(plan->>'batch_count')::bigint
   OR item->>'digest' IS DISTINCT FROM p_digest OR item->>'kind' IS DISTINCT FROM p_kind
   OR item->>'position' IS DISTINCT FROM _oo.native_pos_text(p_pos)
   OR item->>'previous_position' IS DISTINCT FROM _oo.native_pos_text(p_prev)
   OR (p_kind='cdc' AND EXISTS(SELECT 1 FROM _oo.c1_typed_table WHERE source=s AND task=t AND NOT snapshot_complete))
  THEN RAISE EXCEPTION 'Oracle mixed immutable batch or incomplete fill' USING ERRCODE='OO030'; END IF;
 END IF;
 RETURN _oo.c1_typed_begin_0099_base(s,t,g,e,p_seq,p_digest,p_prev,p_pos,p_kind);
END $fn$;

ALTER FUNCTION _oo.c1_typed_seal(text,uuid,bigint,integer,text,numeric,text[]) RENAME TO c1_typed_seal_0099_base;
CREATE FUNCTION _oo.c1_typed_seal(s text,t uuid,g bigint,e integer,p_end text,p_pos numeric,p_tables text[])
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE plan jsonb; seq bigint; proof jsonb; names text[]; state text; buffer jsonb;
BEGIN
 PERFORM _oo.native_c1_baseline_lock(s,t);
 SELECT metadata->'initial_fill' INTO plan FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t;
 IF plan->>'protocol'='oo.oracle.initial-mixed/v1' THEN
  SELECT cp.seq INTO seq FROM _oo.c1_typed_checkpoint cp WHERE cp.source=s AND cp.task=t;
  proof:=_oo.c1_oracle_fill_open(s,t,g,e,plan,seq);
  SELECT array_agg(key ORDER BY key) INTO names FROM jsonb_object_keys(plan->'tables') AS key;
  IF seq IS DISTINCT FROM (plan->>'batch_count')::bigint OR p_tables IS DISTINCT FROM names
   OR proof->'completed_tables' IS DISTINCT FROM to_jsonb(names)
   OR p_pos IS DISTINCT FROM (plan->>'final_position')::numeric
   OR p_end IS DISTINCT FROM 'oracle-buffer:'||(plan->'buffer_context'->>'sha256')||':'||(plan->>'final_position')
  THEN RAISE EXCEPTION 'Oracle mixed final cut incomplete' USING ERRCODE='OO030'; END IF;
  SELECT r.state INTO state FROM _oo.native_c1_baseline_run r WHERE source=s AND task_id=t;
  IF state='awaiting_publish' THEN
   buffer:=_oo.c1_oracle_buffer_summary(s,t);
   RETURN jsonb_build_object('outcome','sealed','l_end',p_end,'applied_pos',_oo.native_pos_text(p_pos),
    'source_engine','oracle','buffer',buffer);
  END IF;
  RETURN _oo.c1_typed_seal_0097_base(s,t,g,e,p_end,p_pos,p_tables);
 END IF;
 RETURN _oo.c1_typed_seal_0099_base(s,t,g,e,p_end,p_pos,p_tables);
END $fn$;

REVOKE ALL ON FUNCTION
 _oo.c1_oracle_mixed_check(text,uuid,bigint,integer,jsonb,bigint),
 _oo.c1_oracle_fill_check(text,uuid,bigint,integer,jsonb,bigint),
 _oo.c1_oracle_fill_check_0099_base(text,uuid,bigint,integer,jsonb,bigint),
 _oo.c1_typed_begin_0099_base(text,uuid,bigint,integer,bigint,text,numeric,numeric,text),
 _oo.c1_typed_seal_0099_base(text,uuid,bigint,integer,text,numeric,text[])
 FROM PUBLIC,oo_pipeline_runtime;
REVOKE ALL ON FUNCTION
 _oo.c1_typed_begin(text,uuid,bigint,integer,bigint,text,numeric,numeric,text),
 _oo.c1_typed_seal(text,uuid,bigint,integer,text,numeric,text[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
 _oo.c1_typed_begin(text,uuid,bigint,integer,bigint,text,numeric,numeric,text),
 _oo.c1_typed_seal(text,uuid,bigint,integer,text,numeric,text[]) TO oo_pipeline_runtime;
