-- native-update/v1
-- Run as oo_target_owner after full-import/v1. This file does not rewrite
-- 0000-0009, does not create a publication, and does not write jobs.leases.
-- Freshness stays awaiting_publish. The source watermark stays lagging.
-- A control mirror is a later transaction and does not hold the target fence.

DO $nu$
BEGIN
  IF pg_catalog.to_regprocedure('_oo.full_import_assert_bound(text, uuid, bigint)') IS NULL THEN
    RAISE EXCEPTION 'native update requires full-import/v1'
      USING ERRCODE = 'OO002';
  END IF;
END
$nu$;

CREATE TABLE _oo.table_freshness (
  schema_name text NOT NULL,
  table_name text NOT NULL,
  source text NOT NULL,
  source_generation bigint NOT NULL,
  target_generation bigint NOT NULL,
  physical_name text NOT NULL,
  last_pos numeric(38,0),
  source_committed_at timestamptz,
  applied_at timestamptz NOT NULL,
  mode text NOT NULL,
  state text NOT NULL,
  PRIMARY KEY (schema_name, table_name),
  CONSTRAINT table_freshness_state_check CHECK (state = 'awaiting_publish'),
  CONSTRAINT table_freshness_mode_check CHECK (mode IN ('full', 'incremental'))
);

CREATE TABLE _oo.source_watermark (
  source text PRIMARY KEY,
  consistency text NOT NULL,
  consistency_grade text NOT NULL,
  applied_pos numeric(38,0) NOT NULL,
  source_committed_at timestamptz,
  applied_at timestamptz NOT NULL,
  state text NOT NULL,
  cross_table_consistent boolean NOT NULL,
  source_generation bigint NOT NULL,
  target_generation bigint NOT NULL,
  position_codec text NOT NULL,
  position_lineage text NOT NULL,
  CONSTRAINT source_watermark_state_check CHECK (state = 'lagging'),
  CONSTRAINT source_watermark_grade_check CHECK (consistency_grade IN ('T', 'S1', 'S0', 'B')),
  CONSTRAINT source_watermark_consistency_check CHECK (consistency IN ('transactional', 'per_table')),
  CONSTRAINT source_watermark_codec_check CHECK (
    position_codec IN (
      'pg-commit-lsn/v1',
      'mysql-binlog-pos/v1',
      'batch-cursor-us/v1',
      'snapshot-zero/v1'
    )
  )
);

CREATE TABLE _oo.table_quality (
  schema_name text NOT NULL,
  table_name text NOT NULL,
  rejects_open integer NOT NULL,
  rejects_by_column jsonb NOT NULL,
  reconcile_status text NOT NULL,
  reconcile_at timestamptz,
  reconcile_diff_rows bigint,
  unexplained_codes jsonb NOT NULL,
  hidden_columns integer NOT NULL,
  partial_txn_in_progress boolean NOT NULL,
  entity_check text NOT NULL,
  capture_method text NOT NULL,
  delete_detection text NOT NULL,
  position_state text NOT NULL,
  budget_state text NOT NULL,
  updated_at timestamptz NOT NULL,
  PRIMARY KEY (schema_name, table_name),
  CONSTRAINT table_quality_reconcile_check CHECK (reconcile_status IN ('ok', 'mismatch', 'stale', 'never')),
  CONSTRAINT table_quality_capture_check CHECK (
    capture_method IN ('c1_cdc', 'c2_change_feed', 'c3_cursor', 'c4_partition', 'c5_diff')
  ),
  CONSTRAINT table_quality_delete_check CHECK (
    delete_detection IN ('native', 'partition_replace', 'reconcile', 'none')
  ),
  CONSTRAINT table_quality_position_check CHECK (position_state IN ('ok', 'stale_risk', 'expired')),
  CONSTRAINT table_quality_budget_check CHECK (budget_state IN ('ok', 'exhausted')),
  CONSTRAINT table_quality_entity_check CHECK (entity_check IN ('ok', 'failed', 'n/a'))
);

CREATE TABLE _oo.update_batch (
  source text NOT NULL,
  schema_name text NOT NULL,
  table_name text NOT NULL,
  batch_id text NOT NULL,
  payload_digest text NOT NULL,
  run_id uuid NOT NULL,
  target_generation bigint NOT NULL,
  source_generation bigint NOT NULL,
  physical_name text NOT NULL,
  tombstone_count integer NOT NULL,
  row_count bigint NOT NULL,
  PRIMARY KEY (source, schema_name, table_name, batch_id),
  CONSTRAINT update_batch_digest_check CHECK (payload_digest ~ '^[0-9a-f]{64}$')
);

CREATE TABLE _oo.update_tombstone (
  source text NOT NULL,
  schema_name text NOT NULL,
  table_name text NOT NULL,
  source_generation bigint NOT NULL,
  key_text text NOT NULL,
  batch_id text NOT NULL,
  PRIMARY KEY (source, schema_name, table_name, source_generation, key_text)
);

CREATE TABLE _oo.control_mirror (
  source text NOT NULL,
  batch_id text NOT NULL,
  physical_name text NOT NULL,
  mirrored_at timestamptz NOT NULL,
  PRIMARY KEY (source, batch_id, physical_name)
);

CREATE FUNCTION _oo.native_update_release_fence(p_shard text)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $nu$
DECLARE
  v_key1 integer;
  v_key2 integer;
BEGIN
  PERFORM _oo._require_shard(p_shard);
  SELECT mapped.key1, mapped.key2
    INTO v_key1, v_key2
  FROM _oo.lock_key(p_shard) AS mapped;
  RETURN pg_catalog.pg_advisory_unlock(v_key1, v_key2);
END;
$nu$;

CREATE FUNCTION _oo.native_update_copy_forward(
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
  p_key_idents text[]
)
RETURNS bigint
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $nu$
DECLARE
  v_parent text;
  v_child text;
  v_parent_col text;
  v_child_col text;
  v_insert text := '"_oo_pos"';
  v_select text := 'row_number() OVER (ORDER BY parent_row."_oo_pos")';
  v_key text;
  v_piece text;
  v_index integer;
  v_count bigint;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_target_generation);
  IF p_column_idents IS NULL OR cardinality(p_column_idents) < 1 THEN
    RAISE EXCEPTION 'closed shadow columns' USING ERRCODE = '22023';
  END IF;
  v_parent := _oo.lookup_physical_mapping(
    p_source, p_config_digest, p_parent_generation, 'table', p_schema, p_table, ''
  );
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
      p_source, p_config_digest, p_parent_generation, 'column',
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
    v_insert := v_insert || ', ' || format('%I', v_child_col);
    v_select := v_select || ', parent_row.' || format('%I', v_parent_col);
  END LOOP;
  IF p_key_idents IS NULL OR cardinality(p_key_idents) < 1
     OR cardinality(p_key_idents) > cardinality(p_column_idents) THEN
    RAISE EXCEPTION 'closed key count' USING ERRCODE = '22023';
  END IF;
  v_key := NULL;
  FOR v_index IN 1..cardinality(p_key_idents) LOOP
    IF p_key_idents[v_index] IS NULL OR NOT (p_key_idents[v_index] = ANY (p_column_idents)) THEN
      RAISE EXCEPTION 'closed key column' USING ERRCODE = '22023';
    END IF;
    v_parent_col := _oo.lookup_physical_mapping(
      p_source, p_config_digest, p_parent_generation, 'column',
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
    'INSERT INTO _oo.%I (%s) SELECT %s FROM _oo.%I AS parent_row WHERE (%s) <> ALL ($1) AND (%s) <> ALL ($2)',
    v_child,
    v_insert,
    v_select,
    v_parent,
    v_key,
    v_key
  )
  USING COALESCE(p_tombstones, ARRAY[]::text[]), COALESCE(p_upsert_keys, ARRAY[]::text[]);
  GET DIAGNOSTICS v_count = ROW_COUNT;
  IF v_count >= 1000000 THEN
    RAISE EXCEPTION 'copy-forward position budget' USING ERRCODE = '22003';
  END IF;
  RETURN v_count;
END;
$nu$;

CREATE FUNCTION _oo.native_update_shadow_count(p_physical text)
RETURNS bigint
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $nu$
DECLARE
  v_count bigint;
BEGIN
  IF p_physical IS NULL OR NOT _oo.full_import_physical_ok(p_physical) OR p_physical !~ '^pt_' THEN
    RAISE EXCEPTION 'physical mapping name is closed' USING ERRCODE = '23514';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM _oo.full_import_table AS table_row
    JOIN _oo.full_import_run AS run_row ON run_row.run_id = table_row.run_id
    WHERE table_row.physical_name = p_physical
      AND run_row.state = 'open'
  ) THEN
    RAISE EXCEPTION 'unpublished shadow is not open' USING ERRCODE = 'OO031';
  END IF;
  EXECUTE format('SELECT count(*) FROM _oo.%I', p_physical) INTO v_count;
  RETURN v_count;
END;
$nu$;

CREATE FUNCTION _oo.native_update_read_cells(
  p_physical text,
  p_column_physical text[]
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $nu$
DECLARE
  v_cols text := '';
  v_index integer;
  v_key text;
  v_out jsonb;
BEGIN
  IF p_physical IS NULL OR NOT _oo.full_import_physical_ok(p_physical) OR p_physical !~ '^pt_' THEN
    RAISE EXCEPTION 'physical mapping name is closed' USING ERRCODE = '23514';
  END IF;
  IF p_column_physical IS NULL OR cardinality(p_column_physical) < 1 THEN
    RAISE EXCEPTION 'closed shadow columns' USING ERRCODE = '22023';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM _oo.full_import_table AS table_row
    JOIN _oo.full_import_run AS run_row ON run_row.run_id = table_row.run_id
    WHERE table_row.physical_name = p_physical
      AND run_row.state = 'open'
  ) THEN
    RAISE EXCEPTION 'unpublished shadow is not open' USING ERRCODE = 'OO031';
  END IF;
  FOR v_index IN 1..cardinality(p_column_physical) LOOP
    IF NOT _oo.full_import_physical_ok(p_column_physical[v_index])
       OR p_column_physical[v_index] !~ '^pc_'
    THEN
      RAISE EXCEPTION 'physical mapping name is closed' USING ERRCODE = '23514';
    END IF;
    IF v_cols <> '' THEN
      v_cols := v_cols || ', ';
    END IF;
    v_cols := v_cols || format('%I::text', p_column_physical[v_index]);
  END LOOP;
  v_key := format('%I', p_column_physical[1]);
  EXECUTE format(
    'SELECT COALESCE(jsonb_agg(jsonb_build_array(%s) ORDER BY child.%s), ''[]''::jsonb) FROM _oo.%I AS child',
    v_cols,
    v_key,
    p_physical
  )
  INTO v_out;
  RETURN v_out::text;
END;
$nu$;

CREATE FUNCTION _oo.native_update_seal_quality(
  p_shard text,
  p_owner uuid,
  p_target_generation bigint,
  p_run uuid,
  p_source text,
  p_source_generation bigint,
  p_schema text,
  p_table text,
  p_physical text,
  p_batch_id text,
  p_payload_digest text,
  p_applied_pos numeric,
  p_position_codec text,
  p_position_lineage text,
  p_capture_method text,
  p_delete_detection text,
  p_consistency_grade text,
  p_cross_table boolean,
  p_tombstones text[],
  p_row_count bigint,
  p_reconcile text,
  p_reconcile_diff bigint,
  p_mode text
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $nu$
DECLARE
  v_consistency text;
  v_index integer;
  v_existing text;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_target_generation);
  PERFORM _oo._require_digest(p_payload_digest, 'payload_digest');
  IF p_position_codec IS NULL OR p_position_codec NOT IN (
    'pg-commit-lsn/v1', 'mysql-binlog-pos/v1', 'batch-cursor-us/v1', 'snapshot-zero/v1'
  ) THEN
    RAISE EXCEPTION 'closed position codec' USING ERRCODE = '23514';
  END IF;
  IF p_position_lineage IS NULL OR p_position_lineage !~ '^[A-Za-z0-9_.:-]{1,128}$' THEN
    RAISE EXCEPTION 'closed position lineage' USING ERRCODE = '23514';
  END IF;
  IF p_capture_method IS NULL OR p_capture_method NOT IN (
    'c1_cdc', 'c2_change_feed', 'c3_cursor', 'c4_partition', 'c5_diff'
  ) THEN
    RAISE EXCEPTION 'closed capture method' USING ERRCODE = '23514';
  END IF;
  IF p_delete_detection IS NULL OR p_delete_detection NOT IN (
    'native', 'partition_replace', 'reconcile', 'none'
  ) THEN
    RAISE EXCEPTION 'closed delete detection' USING ERRCODE = '23514';
  END IF;
  IF p_consistency_grade IS NULL OR p_consistency_grade NOT IN ('T', 'S1', 'S0', 'B') THEN
    RAISE EXCEPTION 'closed consistency grade' USING ERRCODE = '23514';
  END IF;
  IF p_mode IS NULL OR p_mode NOT IN ('full', 'incremental') THEN
    RAISE EXCEPTION 'closed freshness mode' USING ERRCODE = '23514';
  END IF;
  IF p_reconcile IS NULL OR p_reconcile NOT IN ('ok', 'never') THEN
    RAISE EXCEPTION 'closed reconcile status' USING ERRCODE = '23514';
  END IF;
  IF p_consistency_grade = 'T' THEN
    v_consistency := 'transactional';
  ELSE
    v_consistency := 'per_table';
    IF p_cross_table THEN
      RAISE EXCEPTION 'closed consistency grade' USING ERRCODE = '23514';
    END IF;
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM _oo.apply_receipts AS receipt
    WHERE receipt.source = p_source
      AND receipt.table_generation = p_physical
      AND receipt.txn_or_batch_id = p_batch_id
      AND receipt.payload_digest = p_payload_digest
  ) THEN
    RAISE EXCEPTION 'applied coverage is absent' USING ERRCODE = 'OO030';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM _oo.full_import_run AS run_row
    WHERE run_row.run_id = p_run
      AND run_row.state = 'open'
      AND run_row.shard_id = p_shard
      AND run_row.owner_token = p_owner
      AND run_row.target_generation = p_target_generation
      AND run_row.source_id = p_source
      AND run_row.source_generation = p_source_generation
  ) THEN
    RAISE EXCEPTION 'full import run is not open' USING ERRCODE = 'OO031';
  END IF;
  INSERT INTO _oo.update_batch (
    source, schema_name, table_name, batch_id, payload_digest, run_id,
    target_generation, source_generation, physical_name, tombstone_count, row_count
  ) VALUES (
    p_source, p_schema, p_table, p_batch_id, p_payload_digest, p_run,
    p_target_generation, p_source_generation, p_physical,
    COALESCE(cardinality(p_tombstones), 0), p_row_count
  )
  ON CONFLICT (source, schema_name, table_name, batch_id) DO NOTHING;
  IF NOT FOUND THEN
    SELECT existing.payload_digest
      INTO v_existing
    FROM _oo.update_batch AS existing
    WHERE existing.source = p_source
      AND existing.schema_name = p_schema
      AND existing.table_name = p_table
      AND existing.batch_id = p_batch_id;
    IF v_existing IS DISTINCT FROM p_payload_digest THEN
      RAISE EXCEPTION 'apply receipt identity conflict' USING ERRCODE = 'OO010';
    END IF;
  END IF;
  IF p_tombstones IS NOT NULL THEN
    FOR v_index IN 1..cardinality(p_tombstones) LOOP
      INSERT INTO _oo.update_tombstone (
        source, schema_name, table_name, source_generation, key_text, batch_id
      ) VALUES (
        p_source, p_schema, p_table, p_source_generation, p_tombstones[v_index], p_batch_id
      )
      ON CONFLICT (source, schema_name, table_name, source_generation, key_text) DO NOTHING;
    END LOOP;
  END IF;
  INSERT INTO _oo.table_freshness (
    schema_name, table_name, source, source_generation, target_generation, physical_name,
    last_pos, source_committed_at, applied_at, mode, state
  ) VALUES (
    p_schema, p_table, p_source, p_source_generation, p_target_generation, p_physical,
    p_applied_pos, NULL, clock_timestamp(), p_mode, 'awaiting_publish'
  )
  ON CONFLICT (schema_name, table_name) DO UPDATE
    SET source = EXCLUDED.source,
        source_generation = EXCLUDED.source_generation,
        target_generation = EXCLUDED.target_generation,
        physical_name = EXCLUDED.physical_name,
        last_pos = EXCLUDED.last_pos,
        source_committed_at = EXCLUDED.source_committed_at,
        applied_at = EXCLUDED.applied_at,
        mode = EXCLUDED.mode,
        state = EXCLUDED.state;
  INSERT INTO _oo.source_watermark (
    source, consistency, consistency_grade, applied_pos, source_committed_at, applied_at,
    state, cross_table_consistent, source_generation, target_generation,
    position_codec, position_lineage
  ) VALUES (
    p_source, v_consistency, p_consistency_grade, p_applied_pos, NULL, clock_timestamp(),
    'lagging', p_cross_table, p_source_generation, p_target_generation,
    p_position_codec, p_position_lineage
  )
  ON CONFLICT (source) DO UPDATE
    SET consistency = EXCLUDED.consistency,
        consistency_grade = EXCLUDED.consistency_grade,
        applied_pos = EXCLUDED.applied_pos,
        source_committed_at = EXCLUDED.source_committed_at,
        applied_at = EXCLUDED.applied_at,
        state = EXCLUDED.state,
        cross_table_consistent = EXCLUDED.cross_table_consistent,
        source_generation = EXCLUDED.source_generation,
        target_generation = EXCLUDED.target_generation,
        position_codec = EXCLUDED.position_codec,
        position_lineage = EXCLUDED.position_lineage;
  INSERT INTO _oo.table_quality (
    schema_name, table_name, rejects_open, rejects_by_column, reconcile_status, reconcile_at,
    reconcile_diff_rows, unexplained_codes, hidden_columns, partial_txn_in_progress,
    entity_check, capture_method, delete_detection, position_state, budget_state, updated_at
  ) VALUES (
    p_schema, p_table, 0, '{}'::jsonb, p_reconcile,
    CASE WHEN p_reconcile = 'ok' THEN clock_timestamp() ELSE NULL END,
    p_reconcile_diff, '{}'::jsonb, 0, false,
    'n/a', p_capture_method, p_delete_detection, 'ok', 'ok', clock_timestamp()
  )
  ON CONFLICT (schema_name, table_name) DO UPDATE
    SET rejects_open = EXCLUDED.rejects_open,
        rejects_by_column = EXCLUDED.rejects_by_column,
        reconcile_status = EXCLUDED.reconcile_status,
        reconcile_at = EXCLUDED.reconcile_at,
        reconcile_diff_rows = EXCLUDED.reconcile_diff_rows,
        unexplained_codes = EXCLUDED.unexplained_codes,
        hidden_columns = EXCLUDED.hidden_columns,
        partial_txn_in_progress = EXCLUDED.partial_txn_in_progress,
        entity_check = EXCLUDED.entity_check,
        capture_method = EXCLUDED.capture_method,
        delete_detection = EXCLUDED.delete_detection,
        position_state = EXCLUDED.position_state,
        budget_state = EXCLUDED.budget_state,
        updated_at = EXCLUDED.updated_at;
END;
$nu$;

CREATE FUNCTION _oo.native_update_mirror(
  p_source text,
  p_batch_id text,
  p_physical text
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $nu$
DECLARE
  v_count integer;
BEGIN
  IF p_source IS NULL OR p_source !~ '^[a-z][a-z0-9_]{0,62}$' THEN
    RAISE EXCEPTION 'closed source identity' USING ERRCODE = '23514';
  END IF;
  IF p_batch_id IS NULL OR p_batch_id !~ '^[A-Za-z0-9_.:-]{1,128}$' THEN
    RAISE EXCEPTION 'closed batch identity' USING ERRCODE = '23514';
  END IF;
  IF p_physical IS NULL OR NOT _oo.full_import_physical_ok(p_physical) OR p_physical !~ '^pt_' THEN
    RAISE EXCEPTION 'physical mapping name is closed' USING ERRCODE = '23514';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM _oo.apply_receipts AS receipt
    WHERE receipt.source = p_source
      AND receipt.table_generation = p_physical
      AND receipt.txn_or_batch_id = p_batch_id
  ) THEN
    RAISE EXCEPTION 'apply receipt is absent' USING ERRCODE = 'OO030';
  END IF;
  INSERT INTO _oo.control_mirror (source, batch_id, physical_name, mirrored_at)
  VALUES (p_source, p_batch_id, p_physical, clock_timestamp())
  ON CONFLICT (source, batch_id, physical_name) DO NOTHING;
  GET DIAGNOSTICS v_count = ROW_COUNT;
  IF v_count = 0 THEN
    RETURN 'replay';
  END IF;
  RETURN 'inserted';
END;
$nu$;

CREATE FUNCTION _oo.native_update_read_batch(
  p_source text,
  p_schema text,
  p_table text,
  p_batch_id text
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $nu$
DECLARE
  v_out text;
BEGIN
  IF p_source IS NULL OR p_source !~ '^[a-z][a-z0-9_]{0,62}$' THEN
    RAISE EXCEPTION 'closed source identity' USING ERRCODE = '23514';
  END IF;
  IF p_batch_id IS NULL OR p_batch_id !~ '^[A-Za-z0-9_.:-]{1,128}$' THEN
    RAISE EXCEPTION 'closed batch identity' USING ERRCODE = '23514';
  END IF;
  SELECT json_build_object(
    'payload_digest', batch_row.payload_digest,
    'run_id', batch_row.run_id,
    'target_generation', batch_row.target_generation,
    'physical_name', batch_row.physical_name,
    'row_count', batch_row.row_count
  )::text
    INTO v_out
  FROM _oo.update_batch AS batch_row
  WHERE batch_row.source = p_source
    AND batch_row.schema_name = p_schema
    AND batch_row.table_name = p_table
    AND batch_row.batch_id = p_batch_id;
  RETURN v_out;
END;
$nu$;

REVOKE ALL ON TABLE _oo.table_freshness FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON TABLE _oo.source_watermark FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON TABLE _oo.table_quality FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON TABLE _oo.update_batch FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON TABLE _oo.update_tombstone FROM PUBLIC, oo_pipeline_runtime;
REVOKE ALL ON TABLE _oo.control_mirror FROM PUBLIC, oo_pipeline_runtime;

REVOKE ALL ON FUNCTION _oo.native_update_release_fence(text) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.native_update_copy_forward(text, uuid, bigint, text, text, text, text, bigint, bigint, text[], text[], text[], text[]) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.native_update_shadow_count(text) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.native_update_read_cells(text, text[]) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.native_update_seal_quality(text, uuid, bigint, uuid, text, bigint, text, text, text, text, text, numeric, text, text, text, text, text, boolean, text[], bigint, text, bigint, text) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.native_update_mirror(text, text, text) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.native_update_read_batch(text, text, text, text) FROM PUBLIC;

GRANT EXECUTE ON FUNCTION _oo.native_update_release_fence(text) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_update_copy_forward(text, uuid, bigint, text, text, text, text, bigint, bigint, text[], text[], text[], text[]) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_update_shadow_count(text) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_update_read_cells(text, text[]) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_update_seal_quality(text, uuid, bigint, uuid, text, bigint, text, text, text, text, text, numeric, text, text, text, text, text, boolean, text[], bigint, text, bigint, text) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_update_mirror(text, text, text) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_update_read_batch(text, text, text, text) TO oo_pipeline_runtime;
