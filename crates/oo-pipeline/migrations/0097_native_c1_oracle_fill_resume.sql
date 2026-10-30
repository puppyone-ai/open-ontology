-- Same-task TARGET fill recovery from an already immutable initial buffer.
-- No source SCN, vendor offset, generation allocation or parent-stage seed.
CREATE FUNCTION _oo.c1_oracle_fill_check(s text,t uuid,g bigint,e integer,p jsonb,n bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE r _oo.native_c1_baseline_run%ROWTYPE; i _oo.source_instance%ROWTYPE;
  b _oo.c1_typed_capture_bundle%ROWTYPE; tab _oo.c1_typed_table%ROWTYPE;
  cp bigint; last_seq bigint; receipt_count bigint; total bigint; got bigint;
  names text[]; wanted text[]; completed text[]:='{}'; entry jsonb; k bigint;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  SELECT * INTO r FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t;
  IF NOT FOUND OR r.engine<>'oracle' OR r.state NOT IN ('open','awaiting_publish')
    OR r.source_epoch<>e OR r.lease_generation<1 OR r.lease_generation>g
    OR r.l0_pos<>0 OR r.applied_pos<>0
    OR NOT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=t AND generation=g
      AND state='running' AND lease_until>clock_timestamp())
    OR EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=s AND source_generation>0)
    OR EXISTS(SELECT 1 FROM _oo.c1_typed_delta_seed WHERE source=s AND task=t)
    OR EXISTS(SELECT 1 FROM _oo.native_c1_baseline_typed WHERE source=s AND task_id=t AND acknowledged_at IS NOT NULL)
  THEN RAISE EXCEPTION 'Oracle initial fill live task required' USING ERRCODE='OO010'; END IF;
  SELECT * INTO i FROM _oo.source_instance WHERE source=s AND source_epoch=e;
  IF NOT FOUND OR i.engine<>'oracle' OR i.lease_id<>t OR i.lease_generation<>r.lease_generation
    OR i.measured_id<>r.measured_id OR i.config_digest<>r.config_digest OR i.schema_digest<>r.schema_digest
  THEN RAISE EXCEPTION 'Oracle initial fill instance drift' USING ERRCODE='OO010'; END IF;
  SELECT * INTO b FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t;
  IF NOT FOUND OR b.final_apply_ordinal<>0
    OR p->>'protocol' IS DISTINCT FROM 'oo.oracle.initial-fill/v1'
    OR p->>'source_id' IS DISTINCT FROM s
    OR p->>'config_digest' IS DISTINCT FROM r.config_digest
    OR p->>'typed_schema_sha256' IS DISTINCT FROM r.schema_digest
    OR p->>'bundle_sha256' IS DISTINCT FROM b.bundle_sha256
    OR p->'buffer_context'->>'sha256' IS DISTINCT FROM b.metadata->>'context_sha256'
    OR p->'buffer_index' IS DISTINCT FROM b.metadata->'buffer_index'
    OR p IS DISTINCT FROM b.metadata->'initial_fill'
    OR b.metadata->>'source_id' IS DISTINCT FROM s
    OR b.metadata->>'typed_schema_sha256' IS DISTINCT FROM r.schema_digest
    OR b.metadata->>'checkpoint_kind' IS DISTINCT FROM 'buffer_consumption'
    OR b.metadata->>'source_offset_write' IS DISTINCT FROM 'false'
    OR b.metadata ? 'rollover_sha256'
    OR jsonb_typeof(p->'batches') IS DISTINCT FROM 'array'
    OR jsonb_typeof(p->'tables') IS DISTINCT FROM 'object'
    OR (r.l0<>b.bundle_sha256 AND r.l0<>'oracle-buffer:'||(b.metadata->>'context_sha256')||':0')
  THEN RAISE EXCEPTION 'Oracle initial fill immutable binding drift' USING ERRCODE='OO030'; END IF;
  total:=(p->>'batch_count')::bigint;
  IF total IS NULL OR total NOT BETWEEN 1 AND 4096 OR jsonb_array_length(p->'batches')<>total
  THEN RAISE EXCEPTION 'Oracle initial fill bounded plan required' USING ERRCODE='OO030'; END IF;
  k:=0;
  FOR entry IN SELECT value FROM jsonb_array_elements(p->'batches') LOOP
    k:=k+1;
    IF (entry->>'sequence')::bigint IS DISTINCT FROM k OR entry->>'digest' IS NULL
      OR entry->>'digest' !~ '^[0-9a-f]{64}$'
    THEN RAISE EXCEPTION 'Oracle initial fill plan sequence' USING ERRCODE='OO030'; END IF;
  END LOOP;
  SELECT seq INTO cp FROM _oo.c1_typed_checkpoint WHERE source=s AND task=t;
  SELECT coalesce(max(seq),0),count(*) INTO last_seq,receipt_count
    FROM _oo.c1_typed_receipt WHERE source=s AND task=t;
  IF n IS NULL OR n<0 OR n>total OR cp IS DISTINCT FROM n
    OR last_seq<n OR last_seq>n+1 OR last_seq>total OR receipt_count<>last_seq
    OR EXISTS(SELECT 1 FROM _oo.c1_typed_receipt a WHERE a.source=s AND a.task=t
      AND (a.kind<>'fill' OR a.position<>0 OR a.seq>total
        OR a.digest IS DISTINCT FROM p->'batches'->(a.seq::integer-1)->>'digest'))
  THEN RAISE EXCEPTION 'Oracle initial fill durable checkpoint required' USING ERRCODE='OO030'; END IF;
  SELECT array_agg(name ORDER BY name) INTO names FROM _oo.c1_typed_table WHERE source=s AND task=t;
  SELECT array_agg(key ORDER BY key) INTO wanted FROM jsonb_object_keys(p->'tables') AS key;
  IF names IS NULL OR wanted IS NULL OR names IS DISTINCT FROM wanted
  THEN RAISE EXCEPTION 'Oracle initial fill table coverage' USING ERRCODE='OO030'; END IF;
  FOR tab IN SELECT * FROM _oo.c1_typed_table WHERE source=s AND task=t ORDER BY name LOOP
    entry:=p->'tables'->tab.name;
    IF (entry->>'last_sequence')::bigint NOT BETWEEN 1 AND total
      OR (entry->>'last_sequence') IS NULL OR (entry->>'rows') IS NULL OR (entry->>'rows')::bigint<0
    THEN RAISE EXCEPTION 'Oracle initial fill table plan' USING ERRCODE='OO030'; END IF;
    IF tab.snapshot_complete THEN
      IF (entry->>'last_sequence')::bigint>n
      THEN RAISE EXCEPTION 'Oracle incomplete fill marked complete' USING ERRCODE='OO030'; END IF;
      EXECUTE format('SELECT count(*) FROM _oo.%I',tab.physical) INTO got;
      IF got<>(entry->>'rows')::bigint
      THEN RAISE EXCEPTION 'Oracle complete fill row coverage' USING ERRCODE='OO030'; END IF;
      completed:=array_append(completed,tab.name);
    END IF;
  END LOOP;
  IF r.state='awaiting_publish' THEN
    IF n<>total OR last_seq<>n OR completed IS DISTINCT FROM names OR r.l_end IS DISTINCT FROM 0::numeric
      OR NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_v2_format WHERE source=s AND task_id=t AND sealed)
    THEN RAISE EXCEPTION 'Oracle initial fill not sealed' USING ERRCODE='OO030'; END IF;
  ELSIF NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_v2_format WHERE source=s AND task_id=t AND NOT sealed) THEN
    RAISE EXCEPTION 'Oracle initial fill state drift' USING ERRCODE='OO030';
  END IF;
  RETURN jsonb_build_object('outcome','open','source_epoch',e,'resumed_sequence',n,
    'buffer_context_sha256',b.metadata->>'context_sha256','bundle_sha256',b.bundle_sha256,
    'completed_tables',to_jsonb(completed));
END $fn$;

CREATE FUNCTION _oo.c1_oracle_fill_rebind(s text,t uuid,g bigint,e integer,p jsonb,n bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE proof jsonb; old_g bigint;
BEGIN
  proof:=_oo.c1_oracle_fill_check(s,t,g,e,p,n);
  SELECT lease_generation INTO old_g FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t FOR UPDATE;
  UPDATE _oo.source_instance SET lease_generation=g
    WHERE source=s AND source_epoch=e AND lease_id=t AND lease_generation=old_g;
  IF NOT FOUND THEN RAISE EXCEPTION 'Oracle initial fill rebind conflict' USING ERRCODE='OO010'; END IF;
  UPDATE _oo.native_c1_baseline_run SET lease_generation=g
    WHERE source=s AND task_id=t AND lease_generation=old_g;
  IF NOT FOUND THEN RAISE EXCEPTION 'Oracle initial fill rebind conflict' USING ERRCODE='OO010'; END IF;
  RETURN proof;
END $fn$;

CREATE FUNCTION _oo.c1_oracle_fill_open(s text,t uuid,g bigint,e integer,p jsonb,n bigint)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE proof jsonb;
BEGIN
  proof:=_oo.c1_oracle_fill_check(s,t,g,e,p,n);
  IF NOT EXISTS(SELECT 1 FROM _oo.native_c1_baseline_run WHERE source=s AND task_id=t AND lease_generation=g)
  THEN RAISE EXCEPTION 'Oracle initial fill requires same-task rebind' USING ERRCODE='OO010'; END IF;
  RETURN proof;
END $fn$;

ALTER FUNCTION _oo.c1_typed_complete(text,uuid,bigint,integer,text) RENAME TO c1_typed_complete_0097_base;
CREATE FUNCTION _oo.c1_typed_complete(s text,t uuid,g bigint,e integer,p_name text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE plan jsonb; seq bigint; tab _oo.c1_typed_table%ROWTYPE; got bigint;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  SELECT metadata->'initial_fill' INTO plan FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t;
  IF plan IS NOT NULL THEN
    SELECT cp.seq INTO seq FROM _oo.c1_typed_checkpoint cp WHERE cp.source=s AND cp.task=t;
    PERFORM _oo.c1_oracle_fill_open(s,t,g,e,plan,seq);
    IF NOT (plan->'tables' ? p_name) OR (plan->'tables'->p_name->>'last_sequence')::bigint>seq
    THEN RAISE EXCEPTION 'Oracle incomplete fill cannot complete' USING ERRCODE='OO030'; END IF;
    SELECT * INTO STRICT tab FROM _oo.c1_typed_table WHERE source=s AND task=t AND name=p_name;
    EXECUTE format('SELECT count(*) FROM _oo.%I',tab.physical) INTO got;
    IF got<>(plan->'tables'->p_name->>'rows')::bigint
    THEN RAISE EXCEPTION 'Oracle initial fill row coverage' USING ERRCODE='OO030'; END IF;
  END IF;
  PERFORM _oo.c1_typed_complete_0097_base(s,t,g,e,p_name);
END $fn$;

ALTER FUNCTION _oo.c1_typed_seal(text,uuid,bigint,integer,text,numeric,text[]) RENAME TO c1_typed_seal_0097_base;
CREATE FUNCTION _oo.c1_typed_seal(s text,t uuid,g bigint,e integer,p_end text,p_pos numeric,p_tables text[])
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE plan jsonb; seq bigint; proof jsonb; names text[]; state text; buffer jsonb;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  SELECT metadata->'initial_fill' INTO plan FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t;
  IF plan IS NOT NULL THEN
    SELECT cp.seq INTO seq FROM _oo.c1_typed_checkpoint cp WHERE cp.source=s AND cp.task=t;
    proof:=_oo.c1_oracle_fill_open(s,t,g,e,plan,seq);
    SELECT array_agg(key ORDER BY key) INTO names FROM jsonb_object_keys(plan->'tables') AS key;
    IF seq IS DISTINCT FROM (plan->>'batch_count')::bigint OR p_tables IS DISTINCT FROM names
      OR proof->'completed_tables' IS DISTINCT FROM to_jsonb(names) OR p_pos IS DISTINCT FROM 0::numeric
      OR p_end IS DISTINCT FROM 'oracle-buffer:'||(plan->'buffer_context'->>'sha256')||':0'
    THEN RAISE EXCEPTION 'Oracle initial snapshot incomplete' USING ERRCODE='OO030'; END IF;
    SELECT r.state INTO state FROM _oo.native_c1_baseline_run r WHERE source=s AND task_id=t;
    IF state='awaiting_publish' THEN
      buffer:=_oo.c1_oracle_buffer_summary(s,t);
      RETURN jsonb_build_object('outcome','sealed','l_end',p_end,'applied_pos','0',
        'source_engine','oracle','buffer',buffer);
    END IF;
  END IF;
  RETURN _oo.c1_typed_seal_0097_base(s,t,g,e,p_end,p_pos,p_tables);
END $fn$;

REVOKE ALL ON FUNCTION
 _oo.c1_oracle_fill_check(text,uuid,bigint,integer,jsonb,bigint),
 _oo.c1_typed_complete_0097_base(text,uuid,bigint,integer,text),
 _oo.c1_typed_seal_0097_base(text,uuid,bigint,integer,text,numeric,text[])
 FROM PUBLIC,oo_pipeline_runtime;
REVOKE ALL ON FUNCTION
 _oo.c1_oracle_fill_rebind(text,uuid,bigint,integer,jsonb,bigint),
 _oo.c1_oracle_fill_open(text,uuid,bigint,integer,jsonb,bigint),
 _oo.c1_typed_complete(text,uuid,bigint,integer,text),
 _oo.c1_typed_seal(text,uuid,bigint,integer,text,numeric,text[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
 _oo.c1_oracle_fill_rebind(text,uuid,bigint,integer,jsonb,bigint),
 _oo.c1_oracle_fill_open(text,uuid,bigint,integer,jsonb,bigint),
 _oo.c1_typed_complete(text,uuid,bigint,integer,text),
 _oo.c1_typed_seal(text,uuid,bigint,integer,text,numeric,text[]) TO oo_pipeline_runtime;
