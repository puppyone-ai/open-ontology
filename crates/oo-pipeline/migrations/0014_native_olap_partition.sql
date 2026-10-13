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
       OR COALESCE(p_scope->>'table_epoch' ~ '^[0-9a-f]{64}$', false) IS NOT TRUE
       OR COALESCE(p_scope->>'schema_fingerprint' ~ '^[0-9a-f]{64}$', false) IS NOT TRUE
       OR COALESCE(p_scope->>'native_partition_id' ~ '^[1-9][0-9]*$', false) IS NOT TRUE
       OR COALESCE(p_scope->>'native_partition_version' ~ '^[1-9][0-9]*$', false) IS NOT TRUE
       OR p_scope->>'key_model' IS NULL
       OR p_scope->>'key_model' NOT IN ('duplicate','unique','primary')
       OR p_scope->>'routing_column' IS NULL
       OR v_lower IS NULL OR v_upper IS NULL
       OR v_lower !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
       OR v_upper !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
       OR v_lower >= v_upper
       OR p_scope->>'current_signature' IS NULL
       OR p_scope->>'current_signature' !~ ('^' || p_scope->>'native_partition_id' || '[|][0-9a-f]{64}$')
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

-- Return only the target-owned prior ACK for the exact fenced table. The
-- loader passes this result back to native_partition_guard in the same txn.
CREATE FUNCTION _oo.native_partition_prior(
  p_shard text, p_owner uuid, p_target_generation bigint,
  p_source text, p_schema text, p_table text, p_partition text
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $np$
DECLARE
  v_prior _oo.native_partition_state%ROWTYPE;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_target_generation);
  SELECT state.* INTO v_prior FROM _oo.native_partition_state AS state
  WHERE state.source = p_source AND state.schema_name = p_schema
    AND state.table_name = p_table AND state.partition_id = p_partition FOR UPDATE;
  IF NOT FOUND THEN RETURN jsonb_build_object('exists', false, 'signature', NULL); END IF;
  IF NOT v_prior.acknowledged THEN
    RAISE EXCEPTION 'partition prior ACK mismatch' USING ERRCODE = 'OO010';
  END IF;
  RETURN jsonb_build_object('exists', true, 'signature', v_prior.current_signature);
END;
$np$;

REVOKE ALL ON FUNCTION _oo.native_partition_prior(text,uuid,bigint,text,text,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_partition_prior(text,uuid,bigint,text,text,text,text) TO oo_pipeline_runtime;

-- Copy the latest materialized table, even when another table advanced the
-- source watermark or the capture configuration changed. Column OIDs must match.
CREATE FUNCTION _oo.native_olap_copy_forward(
  p_shard text,
  p_owner uuid,
  p_target_generation bigint,
  p_source text,
  p_config_digest text,
  p_schema text,
  p_table text,
  p_parent_generation bigint,
  p_child_generation bigint,
  p_column_idents text[],
  p_tombstones text[],
  p_upsert_keys text[],
  p_key_idents text[],
  p_scope jsonb
)
RETURNS bigint
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $nu$
DECLARE
  v_parent text;
  v_parent_config text;
  v_parent_table_generation bigint;
  v_parent_column oid;
  v_child_column oid;
  v_parent_typmod integer;
  v_child_typmod integer;
  v_child text;
  v_parent_col text;
  v_child_col text;
  v_insert text := '"_oo_pos"';
  v_select text := 'row_number() OVER (ORDER BY parent_row."_oo_pos")';
  v_key text;
  v_piece text;
  v_index integer;
  v_count bigint;
  v_route text;
  v_lower date;
  v_upper date;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_target_generation);
  IF p_column_idents IS NULL OR cardinality(p_column_idents) < 1 THEN
    RAISE EXCEPTION 'closed shadow columns' USING ERRCODE = '22023';
  END IF;
  SELECT freshness.source_generation, run_row.config_digest, freshness.physical_name
    INTO v_parent_table_generation, v_parent_config, v_parent
  FROM _oo.table_freshness AS freshness
  JOIN _oo.full_import_table AS table_row
    ON table_row.physical_name = freshness.physical_name
    AND table_row.schema_ident = freshness.schema_name
    AND table_row.table_ident = freshness.table_name
  JOIN _oo.full_import_run AS run_row ON run_row.run_id = table_row.run_id
  WHERE freshness.source = p_source AND freshness.schema_name = p_schema
    AND freshness.table_name = p_table AND freshness.source_generation <= p_parent_generation
    AND run_row.source_id = p_source AND run_row.source_generation = freshness.source_generation
    AND run_row.state = 'awaiting_publish'
  FOR UPDATE OF freshness;
  IF NOT FOUND OR v_parent_table_generation IS NULL OR v_parent_table_generation < 1 THEN
    RAISE EXCEPTION 'materialized parent table is absent' USING ERRCODE = 'OO010';
  END IF;
  IF v_parent IS DISTINCT FROM _oo.lookup_physical_mapping(
    p_source, v_parent_config, v_parent_table_generation, 'table', p_schema, p_table, ''
  ) THEN RAISE EXCEPTION 'materialized parent mapping differs' USING ERRCODE = 'OO010'; END IF;
  v_child := _oo.lookup_physical_mapping(
    p_source, p_config_digest, p_child_generation, 'table', p_schema, p_table, ''
  );
  IF NOT _oo.full_import_physical_ok(v_parent) OR v_parent !~ '^pt_'
     OR NOT _oo.full_import_physical_ok(v_child) OR v_child !~ '^pt_'
  THEN
    RAISE EXCEPTION 'physical mapping name is closed' USING ERRCODE = '23514';
  END IF;
  FOR v_index IN 1..cardinality(p_column_idents) LOOP
    v_parent_col := _oo.lookup_physical_mapping(
      p_source, v_parent_config, v_parent_table_generation, 'column',
      p_schema, p_table, p_column_idents[v_index]
    );
    v_child_col := _oo.lookup_physical_mapping(
      p_source, p_config_digest, p_child_generation, 'column',
      p_schema, p_table, p_column_idents[v_index]
    );
    IF NOT _oo.full_import_physical_ok(v_parent_col) OR v_parent_col !~ '^pc_'
       OR NOT _oo.full_import_physical_ok(v_child_col) OR v_child_col !~ '^pc_'
    THEN
      RAISE EXCEPTION 'physical mapping name is closed' USING ERRCODE = '23514';
    END IF;
    SELECT a.atttypid, a.atttypmod INTO v_parent_column, v_parent_typmod
    FROM pg_catalog.pg_attribute AS a
    WHERE a.attrelid = pg_catalog.to_regclass(format('_oo.%I', v_parent))
      AND a.attname = v_parent_col AND a.attnum > 0 AND NOT a.attisdropped;
    IF NOT FOUND THEN RAISE EXCEPTION 'materialized parent column is absent' USING ERRCODE = 'OO010'; END IF;
    SELECT a.atttypid, a.atttypmod INTO v_child_column, v_child_typmod
    FROM pg_catalog.pg_attribute AS a
    WHERE a.attrelid = pg_catalog.to_regclass(format('_oo.%I', v_child))
      AND a.attname = v_child_col AND a.attnum > 0 AND NOT a.attisdropped;
    IF NOT FOUND OR v_parent_column IS DISTINCT FROM v_child_column
       OR v_parent_typmod IS DISTINCT FROM v_child_typmod
    THEN RAISE EXCEPTION 'materialized parent column type differs' USING ERRCODE = 'OO010'; END IF;
    v_insert := v_insert || ', ' || format('%I', v_child_col);
    v_select := v_select || ', parent_row.' || format('%I', v_parent_col);
  END LOOP;
  IF p_key_idents IS NULL OR cardinality(p_key_idents) < 1
     OR cardinality(p_key_idents) > cardinality(p_column_idents) THEN
    RAISE EXCEPTION 'closed key count' USING ERRCODE = '22023';
  END IF;
  IF p_scope IS NULL OR cardinality(p_key_idents) <> 1
     OR p_scope->>'routing_column' IS DISTINCT FROM p_key_idents[1]
     OR p_scope->>'source_engine' NOT IN ('doris', 'starrocks')
     OR COALESCE(p_scope->'partition_range'->>'lower' ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$', false) IS NOT TRUE
     OR COALESCE(p_scope->'partition_range'->>'upper' ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$', false) IS NOT TRUE
  THEN RAISE EXCEPTION 'closed partition route' USING ERRCODE = '23514'; END IF;
  -- StarRocks uses year 0000 as the unbounded lower range sentinel.
  v_lower := CASE WHEN p_scope->'partition_range'->>'lower' = '0000-01-01'
    THEN '-infinity'::date ELSE (p_scope->'partition_range'->>'lower')::date END;
  v_upper := (p_scope->'partition_range'->>'upper')::date;
  IF v_lower >= v_upper THEN RAISE EXCEPTION 'closed partition route' USING ERRCODE = '23514'; END IF;
  v_route := _oo.lookup_physical_mapping(
    p_source, v_parent_config, v_parent_table_generation, 'column',
    p_schema, p_table, p_key_idents[1]
  );
  IF NOT _oo.full_import_physical_ok(v_route) OR v_route !~ '^pc_'
  THEN RAISE EXCEPTION 'closed partition route' USING ERRCODE = '23514'; END IF;
  v_parent_column := NULL;
  SELECT a.atttypid INTO v_parent_column FROM pg_catalog.pg_attribute AS a
  WHERE a.attrelid = pg_catalog.to_regclass(format('_oo.%I', v_parent))
    AND a.attname = v_route AND a.attnum > 0 AND NOT a.attisdropped;
  IF v_parent_column IS DISTINCT FROM 1082::oid
  THEN RAISE EXCEPTION 'closed partition route' USING ERRCODE = '23514'; END IF;
  EXECUTE format('SELECT count(*) FROM _oo.%I WHERE %I IS NULL', v_parent, v_route) INTO v_count;
  IF v_count <> 0 THEN RAISE EXCEPTION 'null partition route' USING ERRCODE = '23514'; END IF;
  v_key := NULL;
  FOR v_index IN 1..cardinality(p_key_idents) LOOP
    IF p_key_idents[v_index] IS NULL OR NOT (p_key_idents[v_index] = ANY (p_column_idents)) THEN
      RAISE EXCEPTION 'closed key column' USING ERRCODE = '22023';
    END IF;
    v_parent_col := _oo.lookup_physical_mapping(
      p_source, v_parent_config, v_parent_table_generation, 'column',
      p_schema, p_table, p_key_idents[v_index]
    );
    IF NOT _oo.full_import_physical_ok(v_parent_col) OR v_parent_col !~ '^pc_' THEN
      RAISE EXCEPTION 'physical mapping name is closed' USING ERRCODE = '23514';
    END IF;
    IF cardinality(p_key_idents) = 1 THEN
      v_piece := format('parent_row.%I::text', v_parent_col);
    ELSE
      v_piece := format(
        'octet_length(parent_row.%I::text)::text || '':'' || parent_row.%I::text',
        v_parent_col, v_parent_col
      );
    END IF;
    IF v_key IS NULL THEN
      v_key := v_piece;
    ELSE
      v_key := v_key || ' || chr(31) || ' || v_piece;
    END IF;
  END LOOP;
  EXECUTE format(
    'INSERT INTO _oo.%I (%s) SELECT %s FROM _oo.%I AS parent_row WHERE parent_row.%I < $1 OR parent_row.%I >= $2',
    v_child,
    v_insert,
    v_select,
    v_parent,
    v_route,
    v_route
  )
  USING v_lower, v_upper;
  GET DIAGNOSTICS v_count = ROW_COUNT;
  IF v_count >= 1000000 THEN
    RAISE EXCEPTION 'copy-forward position budget' USING ERRCODE = '22003';
  END IF;
  RETURN v_count;
END;
$nu$;

REVOKE ALL ON FUNCTION _oo.native_olap_copy_forward(text,uuid,bigint,text,text,text,text,bigint,bigint,text[],text[],text[],text[],jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_olap_copy_forward(text,uuid,bigint,text,text,text,text,bigint,bigint,text[],text[],text[],text[],jsonb) TO oo_pipeline_runtime;

-- A PostgreSQL commit LSN may cover several tables. The source barrier stays
-- monotonic; equal LSN is legal only when this table has not consumed it.
CREATE FUNCTION _oo.native_pg_position_ahead(
  p_source text, p_schema text, p_table text, p_position numeric
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $pg$
DECLARE
  v_global _oo.source_watermark%ROWTYPE;
  v_table _oo.table_freshness%ROWTYPE;
BEGIN
  IF p_source IS NULL OR p_schema IS NULL OR p_table IS NULL
     OR p_position IS NULL OR p_position < 0 OR p_position > 99999999999999999999999999999999999999
  THEN RAISE EXCEPTION 'closed source position' USING ERRCODE = '22023'; END IF;
  SELECT * INTO v_global FROM _oo.source_watermark WHERE source = p_source FOR UPDATE;
  IF NOT FOUND THEN RETURN true; END IF;
  IF p_position > v_global.applied_pos THEN RETURN true; END IF;
  IF p_position < v_global.applied_pos OR v_global.position_codec <> 'pg-commit-lsn/v1'
  THEN RETURN false; END IF;
  SELECT * INTO v_table FROM _oo.table_freshness
  WHERE source = p_source AND schema_name = p_schema AND table_name = p_table FOR UPDATE;
  RETURN NOT FOUND OR v_table.last_pos IS NULL OR v_table.last_pos < p_position;
END;
$pg$;
REVOKE ALL ON FUNCTION _oo.native_pg_position_ahead(text,text,text,numeric) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_pg_position_ahead(text,text,text,numeric) TO oo_pipeline_runtime;

-- The capture count is the changed-row batch; the catalog count is the
-- materialized table. Bind both to the same sealed incremental target run.
CREATE FUNCTION _oo.native_pg_catalog_delta(
  p_run uuid, p_source text, p_schema text, p_table text, p_delta bigint
) RETURNS boolean LANGUAGE sql SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $pg$
  SELECT p_delta >= 0 AND p_delta <= table_row.applied_rows
     AND freshness.mode = 'incremental'
     AND freshness.physical_name = table_row.physical_name
     AND freshness.source_generation = run_row.source_generation
     AND count(batch_row.run_id) = 1
     AND min(batch_row.row_count) = table_row.applied_rows
  FROM _oo.full_import_run AS run_row
  JOIN _oo.full_import_table AS table_row ON table_row.run_id = run_row.run_id
  JOIN _oo.table_freshness AS freshness
    ON freshness.source = run_row.source_id
    AND freshness.schema_name = table_row.schema_ident
    AND freshness.table_name = table_row.table_ident
  LEFT JOIN _oo.update_batch AS batch_row
    ON batch_row.run_id = run_row.run_id
    AND batch_row.source = run_row.source_id
    AND batch_row.schema_name = table_row.schema_ident
    AND batch_row.table_name = table_row.table_ident
    AND batch_row.physical_name = table_row.physical_name
    AND batch_row.source_generation = run_row.source_generation
    AND batch_row.target_generation = run_row.target_generation
  WHERE run_row.run_id = p_run AND run_row.source_id = p_source
    AND run_row.state = 'awaiting_publish'
    AND table_row.schema_ident = p_schema AND table_row.table_ident = p_table
  GROUP BY table_row.applied_rows, freshness.mode, freshness.physical_name,
    table_row.physical_name, freshness.source_generation, run_row.source_generation;
$pg$;
REVOKE ALL ON FUNCTION _oo.native_pg_catalog_delta(uuid,text,text,text,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_pg_catalog_delta(uuid,text,text,text,bigint) TO oo_pipeline_runtime;

-- The current ACKed scope defines today's complete set. Earlier sealed
-- sidecars keep their historically observed set when a new partition appears.
CREATE OR REPLACE FUNCTION _oo.native_partition_vector(
  p_run uuid, p_source text, p_schema text, p_table text,
  p_expected text[], p_current_partition text, p_current_signature text,
  p_source_generation bigint, p_target_generation bigint
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $np$
DECLARE
  v_rows bigint;
  v_count bigint;
  v_ids text[];
  v_vector jsonb;
  v_current _oo.native_partition_state%ROWTYPE;
BEGIN
  IF p_expected IS NULL OR cardinality(p_expected) = 0 OR cardinality(p_expected) > 1024
     OR EXISTS (SELECT 1 FROM unnest(p_expected) AS id WHERE id IS NULL OR id = '')
     OR p_expected <> (SELECT array_agg(id ORDER BY id) FROM unnest(p_expected) AS id)
     OR cardinality(p_expected) <> (SELECT count(DISTINCT id) FROM unnest(p_expected) AS id)
  THEN RAISE EXCEPTION 'partition vector is incomplete' USING ERRCODE = 'OO030'; END IF;
  SELECT table_row.applied_rows INTO v_rows
  FROM _oo.full_import_table AS table_row
  JOIN _oo.full_import_run AS run_row ON run_row.run_id = table_row.run_id
  WHERE table_row.run_id = p_run AND table_row.schema_ident = p_schema
    AND table_row.table_ident = p_table AND run_row.source_id = p_source
    AND run_row.source_generation = p_source_generation
    AND run_row.target_generation = p_target_generation
    AND run_row.state = 'awaiting_publish';
  IF NOT FOUND THEN RAISE EXCEPTION 'sealed partition run is absent' USING ERRCODE = 'OO030'; END IF;
  SELECT state.* INTO v_current FROM _oo.native_partition_state AS state
  WHERE state.source = p_source AND state.schema_name = p_schema
    AND state.table_name = p_table AND state.partition_id = p_current_partition;
  IF NOT FOUND OR v_current.run_id <> p_run
     OR NOT v_current.acknowledged
     OR v_current.current_signature IS DISTINCT FROM p_current_signature
     OR v_current.source_generation <> p_source_generation
     OR v_current.target_generation <> p_target_generation
     OR v_current.observed_partition_ids <> p_expected
  THEN RAISE EXCEPTION 'current partition receipt is absent' USING ERRCODE = 'OO030'; END IF;
  SELECT count(*), array_agg(state.partition_id ORDER BY state.partition_id),
    COALESCE(sum(state.row_count), 0),
    jsonb_agg(jsonb_build_object(
      'partition_id', state.partition_id,
      'table_epoch', state.table_epoch,
      'schema_fingerprint', state.schema_fingerprint,
      'current_signature', state.current_signature,
      'previous_ack_exists', state.previous_ack_exists,
      'previous_signature', state.previous_signature,
      'partition_scope', state.partition_scope,
      'audit_digest', state.audit_digest,
      'consistency_grade', state.consistency_grade,
      'source_generation', state.source_generation,
      'target_generation', state.target_generation,
      'run_id', state.run_id,
      'batch_id', state.batch_id,
      'payload_digest', state.payload_digest,
      'row_count', state.row_count
    ) ORDER BY state.partition_id)
  INTO v_count, v_ids, v_rows, v_vector
  FROM _oo.native_partition_state AS state
  WHERE state.source = p_source AND state.schema_name = p_schema
    AND state.table_name = p_table;
  IF v_count <> cardinality(p_expected) OR v_ids <> p_expected
     OR v_rows <> (SELECT applied_rows FROM _oo.full_import_table
                   WHERE run_id = p_run AND schema_ident = p_schema AND table_ident = p_table)
     OR EXISTS (SELECT 1 FROM _oo.native_partition_state AS state
       WHERE state.source = p_source AND state.schema_name = p_schema AND state.table_name = p_table
         AND (state.table_epoch <> v_current.table_epoch
           OR state.schema_fingerprint <> v_current.schema_fingerprint
           OR state.partition_scope->>'schema_digest' IS DISTINCT FROM v_current.partition_scope->>'schema_digest'
           OR state.partition_scope->>'type_digest' IS DISTINCT FROM v_current.partition_scope->>'type_digest'))
     OR EXISTS (SELECT 1 FROM _oo.native_partition_state AS state
       WHERE state.source = p_source AND state.schema_name = p_schema
         AND state.table_name = p_table AND NOT state.acknowledged)
  THEN RAISE EXCEPTION 'partition vector is incomplete' USING ERRCODE = 'OO030'; END IF;
  RETURN v_vector;
END;
$np$;
