-- Correct the remaining JSON operator precedence in the OLAP signature.
-- The 0014 and 0015 bodies and checksums stay immutable.
CREATE OR REPLACE FUNCTION _oo.native_partition_guard(
  p_shard text, p_owner uuid, p_target_generation bigint,
  p_source text, p_schema text, p_table text,
  p_source_generation bigint, p_parent_generation bigint,
  p_scope jsonb, p_previous_ack_exists boolean, p_previous_signature text,
  p_new_keys text[]
) RETURNS text[] LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $np$
DECLARE
  v_prior _oo.native_partition_state%ROWTYPE;
  v_watermark bigint;
  v_keys text[] := ARRAY[]::text[];
  v_partition text := p_scope->>'partition_id';
  v_olap boolean := p_scope->>'source_engine' IN ('doris', 'starrocks');
  v_lower text := p_scope->'partition_range'->>'lower';
  v_upper text := p_scope->'partition_range'->>'upper';
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_target_generation);
  IF p_scope IS NULL OR jsonb_typeof(p_scope) <> 'object'
     OR p_scope->>'source_id' IS DISTINCT FROM p_source
     OR (p_scope->>'source_generation')::bigint IS DISTINCT FROM p_source_generation
     OR p_scope->>'logical_schema' IS DISTINCT FROM p_schema
     OR p_scope->>'logical_table' IS DISTINCT FROM p_table
     OR v_partition IS NULL OR v_partition = ''
     OR COALESCE(p_scope->>'table_epoch', '') = ''
     OR COALESCE(p_scope->>'schema_fingerprint', '') = ''
     OR p_scope->>'complete' IS DISTINCT FROM 'true'
     OR p_scope->>'mutations_pending' IS DISTINCT FROM 'false'
     OR p_new_keys IS NULL OR cardinality(p_new_keys) > 1000000
     OR p_scope->>'row_count' IS NULL
     OR cardinality(p_new_keys) IS DISTINCT FROM (p_scope->>'row_count')::bigint
     OR (cardinality(p_new_keys) > 0 AND COALESCE(p_scope->>'current_signature', '') = '')
     OR EXISTS (SELECT 1 FROM unnest(p_new_keys) AS key_text WHERE key_text IS NULL)
     OR (NOT COALESCE(v_olap, false) AND cardinality(p_new_keys) <> (SELECT count(DISTINCT key_text) FROM unnest(p_new_keys) AS key_text))
  THEN RAISE EXCEPTION 'partition scope is closed' USING ERRCODE = '23514'; END IF;
  IF v_olap THEN
    IF (SELECT count(*) FROM jsonb_object_keys(p_scope)) <> 21
       OR COALESCE((p_scope->>'table_epoch') ~ '^[0-9a-f]{64}$', false) IS NOT TRUE
       OR COALESCE((p_scope->>'schema_fingerprint') ~ '^[0-9a-f]{64}$', false) IS NOT TRUE
       OR COALESCE((p_scope->>'native_partition_id') ~ '^[1-9][0-9]*$', false) IS NOT TRUE
       OR COALESCE((p_scope->>'native_partition_version') ~ '^[1-9][0-9]*$', false) IS NOT TRUE
       OR p_scope->>'key_model' IS NULL
       OR p_scope->>'key_model' NOT IN ('duplicate','unique','primary')
       OR p_scope->>'routing_column' IS NULL
       OR v_lower IS NULL OR v_upper IS NULL
       OR v_lower !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
       OR v_upper !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
       OR v_lower >= v_upper
       OR p_scope->>'current_signature' IS NULL
       OR (p_scope->>'current_signature') !~ ('^' || (p_scope->>'native_partition_id') || '[|][0-9a-f]{64}$')
       OR EXISTS (SELECT 1 FROM unnest(p_new_keys) AS key_text WHERE key_text < v_lower OR key_text >= v_upper)
    THEN RAISE EXCEPTION 'partition route is closed' USING ERRCODE = '23514'; END IF;
  END IF;
  PERFORM pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended(
    p_source || chr(31) || p_schema || chr(31) || p_table, 0));
  SELECT watermark.source_generation INTO v_watermark
  FROM _oo.source_watermark AS watermark WHERE watermark.source = p_source FOR UPDATE;
  IF (p_parent_generation IS NULL AND FOUND)
     OR (p_parent_generation IS NOT NULL AND (NOT FOUND OR v_watermark <> p_parent_generation))
     OR p_source_generation <> COALESCE(p_parent_generation, 0) + 1
  THEN RAISE EXCEPTION 'partition source generation is stale' USING ERRCODE = 'OO010'; END IF;
  SELECT state.* INTO v_prior FROM _oo.native_partition_state AS state
  WHERE state.source = p_source AND state.schema_name = p_schema
    AND state.table_name = p_table AND state.partition_id = v_partition FOR UPDATE;
  IF FOUND IS DISTINCT FROM p_previous_ack_exists THEN
    RAISE EXCEPTION 'partition prior ACK mismatch' USING ERRCODE = 'OO010';
  END IF;
  IF FOUND THEN
    IF NOT v_prior.acknowledged
       OR v_prior.current_signature IS DISTINCT FROM p_previous_signature
       OR v_prior.table_epoch IS DISTINCT FROM p_scope->>'table_epoch'
       OR v_prior.schema_fingerprint IS DISTINCT FROM p_scope->>'schema_fingerprint'
       OR v_prior.partition_scope->>'schema_digest' IS DISTINCT FROM p_scope->>'schema_digest'
       OR v_prior.partition_scope->>'type_digest' IS DISTINCT FROM p_scope->>'type_digest'
    THEN RAISE EXCEPTION 'partition prior ACK mismatch' USING ERRCODE = 'OO010'; END IF;
    v_keys := v_prior.key_texts;
  ELSIF p_previous_signature IS NOT NULL THEN
    RAISE EXCEPTION 'partition prior ACK mismatch' USING ERRCODE = 'OO010';
  END IF;
  IF EXISTS (SELECT 1 FROM _oo.native_partition_state AS state
    WHERE state.source = p_source AND state.schema_name = p_schema
      AND state.table_name = p_table AND NOT state.acknowledged)
  THEN RAISE EXCEPTION 'partition prior ACK mismatch' USING ERRCODE = 'OO010'; END IF;
  IF EXISTS (SELECT 1 FROM _oo.native_partition_state AS state
    WHERE state.source = p_source AND state.schema_name = p_schema AND state.table_name = p_table
      AND state.partition_id <> v_partition
      AND (state.table_epoch IS DISTINCT FROM p_scope->>'table_epoch'
        OR state.schema_fingerprint IS DISTINCT FROM p_scope->>'schema_fingerprint'
        OR state.partition_scope->>'schema_digest' IS DISTINCT FROM p_scope->>'schema_digest'
        OR state.partition_scope->>'type_digest' IS DISTINCT FROM p_scope->>'type_digest'
        OR (v_olap AND (
          state.partition_scope->>'source_engine' IS DISTINCT FROM p_scope->>'source_engine'
          OR state.partition_scope->>'key_model' IS DISTINCT FROM p_scope->>'key_model'
          OR state.partition_scope->>'routing_column' IS DISTINCT FROM p_scope->>'routing_column'
          OR (state.partition_scope->'partition_range'->>'lower' < v_upper
              AND v_lower < state.partition_scope->'partition_range'->>'upper')
        ))
        OR state.key_texts && p_new_keys))
  THEN RAISE EXCEPTION 'partition epoch or key ownership conflict' USING ERRCODE = 'OO010'; END IF;
  RETURN v_keys;
END;
$np$;

REVOKE ALL ON FUNCTION _oo.native_partition_guard(text,uuid,bigint,text,text,text,bigint,bigint,jsonb,boolean,text,text[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_partition_guard(text,uuid,bigint,text,text,text,bigint,bigint,jsonb,boolean,text,text[]) TO oo_pipeline_runtime;
