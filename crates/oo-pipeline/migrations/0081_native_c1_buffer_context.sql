-- Preserve 0080 receipts; bind the stronger producer context without a new
-- checkpoint/lease authority or a vendor-position interpretation.
ALTER FUNCTION _oo.c1_typed_open_capture(text,uuid,bigint,integer,jsonb)
 RENAME TO c1_typed_open_capture_0080;
CREATE FUNCTION _oo.c1_typed_open_capture(s text,t uuid,g bigint,e integer,p jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
BEGIN
  IF p ? 'context_sha256' AND (
    p->>'context_sha256' IS NULL OR p->>'context_sha256' !~ '^[0-9a-f]{64}$'
    OR p->>'typed_schema_sha256' IS NULL OR p->>'typed_schema_sha256' !~ '^[0-9a-f]{64}$'
    OR p->>'buffer_position_protocol' IS DISTINCT FROM 'oo.c1.oracle-buffer-position/v1'
    OR p->>'source_offset_write' IS DISTINCT FROM 'false'
    OR p->'source_identity'->>'engine' IS DISTINCT FROM 'oracle'
    OR NOT EXISTS(SELECT 1 FROM _oo.source_instance WHERE source=s AND source_epoch=e
      AND schema_digest=p->>'typed_schema_sha256' AND engine='oracle'
      AND measured_id=p->>'measured_id' AND lease_id=t AND lease_generation=g)) THEN
    RAISE EXCEPTION 'registered immutable buffer context required' USING ERRCODE='OO030';
  END IF;
  PERFORM _oo.c1_typed_open_capture_0080(s,t,g,e,p);
END $fn$;

CREATE FUNCTION _oo.c1_typed_assert_buffer_context(s text,t uuid,g bigint,e integer,p jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
DECLARE b _oo.c1_typed_capture_bundle%ROWTYPE;
BEGIN
  PERFORM _oo.native_c1_baseline_lock(s,t);
  PERFORM _oo.c1_typed_assert_engine(s,t,g,e,'oracle');
  SELECT * INTO b FROM _oo.c1_typed_capture_bundle WHERE source=s AND task=t;
  IF (b.metadata ? 'context_sha256' OR p ? 'context_sha256') AND (
    b.metadata->>'context_sha256' IS NULL
    OR p->>'context_sha256' IS DISTINCT FROM b.metadata->>'context_sha256'
    OR p->>'typed_schema_sha256' IS DISTINCT FROM b.metadata->>'typed_schema_sha256'
    OR p->'source_identity' IS DISTINCT FROM b.metadata->'source_identity'
    OR p->>'bundle_sha256' IS DISTINCT FROM b.bundle_sha256
    OR p->>'spool_sha256' IS DISTINCT FROM b.spool_sha256
    OR p->>'source_offset_write' IS DISTINCT FROM 'false'
    OR p->>'source_offset_advanced' IS DISTINCT FROM 'false'
    OR p->>'buffer_position_protocol' IS DISTINCT FROM 'oo.c1.oracle-buffer-position/v1') THEN
    RAISE EXCEPTION 'buffer context changed or checkpoint mode downgraded' USING ERRCODE='OO030';
  END IF;
END $fn$;

ALTER FUNCTION _oo.c1_typed_record_buffer(text,uuid,bigint,integer,bigint,text,jsonb)
 RENAME TO c1_typed_record_buffer_0080;
CREATE FUNCTION _oo.c1_typed_record_buffer(s text,t uuid,g bigint,e integer,p_seq bigint,p_digest text,p jsonb)
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
      IF NOT FOUND THEN RAISE EXCEPTION 'previous buffer transaction absent' USING ERRCODE='OO030'; END IF;
    END IF;
    IF v->'cursor_before'->>'transaction_id' IS DISTINCT FROM previous_id THEN
      RAISE EXCEPTION 'previous buffer transaction conflict' USING ERRCODE='OO030';
    END IF;
  END IF;
  PERFORM _oo.c1_typed_record_buffer_0080(s,t,g,e,p_seq,p_digest,p);
END $fn$;

-- A strict buffer run cannot bypass its context by using the vendor-pair API.
ALTER FUNCTION _oo.c1_typed_record_oracle_position(text,uuid,bigint,integer,bigint,text,jsonb)
 RENAME TO c1_typed_record_oracle_position_0079;
CREATE FUNCTION _oo.c1_typed_record_oracle_position(s text,t uuid,g bigint,e integer,p_seq bigint,p_digest text,p jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,_oo AS $fn$
BEGIN
  PERFORM _oo.c1_typed_assert_buffer_context(s,t,g,e,p);
  PERFORM _oo.c1_typed_record_oracle_position_0079(s,t,g,e,p_seq,p_digest,p);
END $fn$;

REVOKE ALL ON FUNCTION
 _oo.c1_typed_open_capture_0080(text,uuid,bigint,integer,jsonb),
 _oo.c1_typed_record_buffer_0080(text,uuid,bigint,integer,bigint,text,jsonb),
 _oo.c1_typed_record_oracle_position_0079(text,uuid,bigint,integer,bigint,text,jsonb) FROM PUBLIC,oo_pipeline_runtime;
REVOKE ALL ON FUNCTION
 _oo.c1_typed_open_capture(text,uuid,bigint,integer,jsonb),
 _oo.c1_typed_assert_buffer_context(text,uuid,bigint,integer,jsonb),
 _oo.c1_typed_record_buffer(text,uuid,bigint,integer,bigint,text,jsonb),
 _oo.c1_typed_record_oracle_position(text,uuid,bigint,integer,bigint,text,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
 _oo.c1_typed_open_capture(text,uuid,bigint,integer,jsonb),
 _oo.c1_typed_assert_buffer_context(text,uuid,bigint,integer,jsonb),
 _oo.c1_typed_record_buffer(text,uuid,bigint,integer,bigint,text,jsonb),
 _oo.c1_typed_record_oracle_position(text,uuid,bigint,integer,bigint,text,jsonb) TO oo_pipeline_runtime;
