-- Read one native sealed import by run id.
--
-- 0006 and 0007 key only source, config digest, and source generation.
-- Five sealed tables can share that triple. This function keys the run id.
-- It does not change settings, read shadow values, or accept a SQL string.
-- oo_pipeline_runtime cannot select the full-import tables. This function runs
-- as its owner and returns one jsonb document of stored identity and attributes.

CREATE FUNCTION _oo.full_import_native_catalog(
  p_run_id uuid,
  p_source text,
  p_owner uuid,
  p_config_digest text,
  p_source_generation bigint,
  p_target_generation bigint,
  p_schema_digest text,
  p_type_digest text,
  p_input_digest text
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $native$
DECLARE
  v_run _oo.full_import_run%ROWTYPE;
  v_partial integer;
  v_hooks integer;
  v_hook text;
  v_tables jsonb := '[]'::jsonb;
  v_table _oo.full_import_table%ROWTYPE;
  v_namespace text;
  v_mapped_table text;
  v_rel oid;
  v_mapped integer;
  v_attributes integer;
  v_joined integer;
  v_columns jsonb;
  v_quality jsonb;
BEGIN
  IF p_run_id IS NULL
     OR p_source IS NULL OR p_source !~ '^[a-z][a-z0-9_]{0,62}$'
     OR p_owner IS NULL
     OR p_config_digest IS NULL OR p_config_digest !~ '^[0-9a-f]{64}$'
     OR p_schema_digest IS NULL OR p_schema_digest !~ '^[0-9a-f]{64}$'
     OR p_type_digest IS NULL OR p_type_digest !~ '^[0-9a-f]{64}$'
     OR p_input_digest IS NULL OR p_input_digest !~ '^[0-9a-f]{64}$'
     OR p_source_generation IS NULL OR p_source_generation < 1
     OR p_target_generation IS NULL OR p_target_generation < 1
  THEN
    RAISE EXCEPTION 'sealed import identity is closed' USING ERRCODE = '22023';
  END IF;
  SELECT run_row.*
    INTO v_run
  FROM _oo.full_import_run AS run_row
  WHERE run_row.run_id = p_run_id;
  IF NOT FOUND OR v_run.state <> 'awaiting_publish' THEN
    RAISE EXCEPTION 'sealed import is absent' USING ERRCODE = 'P0002';
  END IF;
  IF v_run.source_id <> p_source THEN
    RAISE EXCEPTION 'sealed import source' USING ERRCODE = '22023';
  END IF;
  IF v_run.owner_token <> p_owner THEN
    RAISE EXCEPTION 'sealed import owner' USING ERRCODE = '42501';
  END IF;
  IF v_run.config_digest <> p_config_digest
     OR v_run.schema_digest <> p_schema_digest
     OR v_run.type_digest <> p_type_digest
     OR v_run.input_digest <> p_input_digest
     OR v_run.source_generation <> p_source_generation
     OR v_run.target_generation <> p_target_generation
  THEN
    RAISE EXCEPTION 'sealed import digest' USING ERRCODE = '22023';
  END IF;
  SELECT count(*)::integer
    INTO v_partial
  FROM _oo.full_import_table AS table_row
  WHERE table_row.run_id = v_run.run_id
    AND NOT table_row.exhausted;
  IF v_partial <> 0 THEN
    RAISE EXCEPTION 'sealed import is partial' USING ERRCODE = 'P0002';
  END IF;
  SELECT count(*)::integer
    INTO v_partial
  FROM _oo.full_import_table AS table_row
  WHERE table_row.run_id = v_run.run_id;
  IF v_partial < 1 OR v_partial > 16 THEN
    RAISE EXCEPTION 'sealed import is partial' USING ERRCODE = 'P0002';
  END IF;
  SELECT count(DISTINCT receipt.hook_digest)::integer
    INTO v_hooks
  FROM _oo.apply_receipts AS receipt
  WHERE receipt.source = v_run.source_id
    AND receipt.config_digest = v_run.config_digest
    AND receipt.schema_digest = v_run.schema_digest
    AND receipt.type_digest = v_run.type_digest;
  IF v_hooks <> 1 THEN
    RAISE EXCEPTION 'sealed import digest' USING ERRCODE = '22023';
  END IF;
  SELECT min(receipt.hook_digest)
    INTO v_hook
  FROM _oo.apply_receipts AS receipt
  WHERE receipt.source = v_run.source_id
    AND receipt.config_digest = v_run.config_digest
    AND receipt.schema_digest = v_run.schema_digest
    AND receipt.type_digest = v_run.type_digest;
  FOR v_table IN
    SELECT table_row.*
    FROM _oo.full_import_table AS table_row
    WHERE table_row.run_id = v_run.run_id
    ORDER BY table_row.table_index
  LOOP
    IF v_table.canonical_digest IS NULL OR v_table.applied_rows < 1 THEN
      RAISE EXCEPTION 'sealed import is partial' USING ERRCODE = 'P0002';
    END IF;
    SELECT mapping.physical_name
      INTO v_namespace
    FROM _oo.physical_mappings AS mapping
    WHERE mapping.source = v_run.source_id
      AND mapping.config_digest = v_run.config_digest
      AND mapping.run_generation = v_run.source_generation
      AND mapping.level = 'n'
      AND mapping.logical_namespace = v_table.schema_ident
      AND mapping.logical_table = ''
      AND mapping.logical_column = '';
    IF NOT FOUND THEN
      RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'P0002';
    END IF;
    SELECT mapping.physical_name
      INTO v_mapped_table
    FROM _oo.physical_mappings AS mapping
    WHERE mapping.source = v_run.source_id
      AND mapping.config_digest = v_run.config_digest
      AND mapping.run_generation = v_run.source_generation
      AND mapping.level = 't'
      AND mapping.logical_namespace = v_table.schema_ident
      AND mapping.logical_table = v_table.table_ident
      AND mapping.logical_column = '';
    IF NOT FOUND OR v_mapped_table <> v_table.physical_name THEN
      RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'P0002';
    END IF;
    SELECT relation.oid
      INTO v_rel
    FROM pg_catalog.pg_class AS relation
    JOIN pg_catalog.pg_namespace AS namespace
      ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = '_oo'
      AND relation.relname = v_table.physical_name
      AND relation.relkind = 'r';
    IF NOT FOUND THEN
      RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'P0002';
    END IF;
    SELECT count(*)::integer
      INTO v_mapped
    FROM _oo.physical_mappings AS mapping
    WHERE mapping.source = v_run.source_id
      AND mapping.config_digest = v_run.config_digest
      AND mapping.run_generation = v_run.source_generation
      AND mapping.level = 'c'
      AND mapping.logical_namespace = v_table.schema_ident
      AND mapping.logical_table = v_table.table_ident;
    SELECT count(*)::integer
      INTO v_attributes
    FROM pg_catalog.pg_attribute AS attribute
    WHERE attribute.attrelid = v_rel
      AND attribute.attnum > 0
      AND NOT attribute.attisdropped
      AND attribute.attname <> '_oo_pos';
    SELECT count(*)::integer
      INTO v_joined
    FROM _oo.physical_mappings AS mapping
    JOIN pg_catalog.pg_attribute AS attribute
      ON attribute.attrelid = v_rel
     AND attribute.attname = mapping.physical_name
     AND attribute.attnum > 0
     AND NOT attribute.attisdropped
    WHERE mapping.source = v_run.source_id
      AND mapping.config_digest = v_run.config_digest
      AND mapping.run_generation = v_run.source_generation
      AND mapping.level = 'c'
      AND mapping.logical_namespace = v_table.schema_ident
      AND mapping.logical_table = v_table.table_ident;
    IF v_mapped < 1 OR v_mapped > 64 OR v_mapped <> v_attributes OR v_joined <> v_mapped THEN
      RAISE EXCEPTION 'physical mapping is absent' USING ERRCODE = 'P0002';
    END IF;
    SELECT coalesce(jsonb_agg(jsonb_build_object(
        'attnum', attribute.attnum,
        'atttypid', attribute.atttypid::bigint,
        'atttypmod', attribute.atttypmod,
        'logical_column', mapping.logical_column,
        'physical_name', mapping.physical_name
      ) ORDER BY attribute.attnum), '[]'::jsonb)
      INTO v_columns
    FROM _oo.physical_mappings AS mapping
    JOIN pg_catalog.pg_attribute AS attribute
      ON attribute.attrelid = v_rel
     AND attribute.attname = mapping.physical_name
     AND attribute.attnum > 0
     AND NOT attribute.attisdropped
    WHERE mapping.source = v_run.source_id
      AND mapping.config_digest = v_run.config_digest
      AND mapping.run_generation = v_run.source_generation
      AND mapping.level = 'c'
      AND mapping.logical_namespace = v_table.schema_ident
      AND mapping.logical_table = v_table.table_ident;
    v_tables := v_tables || jsonb_build_array(jsonb_build_object(
      'applied_rows', v_table.applied_rows,
      'canonical_digest', v_table.canonical_digest,
      'columns', v_columns,
      'physical_name', v_table.physical_name,
      'schema_ident', v_table.schema_ident,
      'stored_namespace', v_namespace,
      'table_ident', v_table.table_ident,
      'table_index', v_table.table_index
    ));
  END LOOP;
  SELECT coalesce(jsonb_agg(jsonb_build_object(
      'accepted_rows', quality_row.accepted_rows,
      'attnum', quality_row.attnum,
      'batch_id', quality_row.batch_id,
      'key_digest', quality_row.key_digest,
      'kind', quality_row.kind,
      'physical_name', quality_row.physical_name,
      'reason', quality_row.reason,
      'rejected_rows', quality_row.rejected_rows,
      'row_index', quality_row.row_index,
      'value_len', quality_row.value_len
    ) ORDER BY quality_row.quality_id), '[]'::jsonb)
    INTO v_quality
  FROM _oo.full_import_quality AS quality_row
  WHERE quality_row.run_id = v_run.run_id;
  RETURN jsonb_build_object(
    'config_digest', v_run.config_digest,
    'input_digest', v_run.input_digest,
    'owner', v_run.owner_token,
    'quality', v_quality,
    'run_id', v_run.run_id,
    'schema_digest', v_run.schema_digest,
    'source_generation', v_run.source_generation,
    'source_id', v_run.source_id,
    'state', v_run.state,
    'tables', v_tables,
    'target_generation', v_run.target_generation,
    'tool_hook_digest', v_hook,
    'type_digest', v_run.type_digest
  );
END;
$native$;

REVOKE ALL ON FUNCTION _oo.full_import_native_catalog(uuid, text, uuid, text, bigint, bigint, text, text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.full_import_native_catalog(uuid, text, uuid, text, bigint, bigint, text, text, text) TO oo_pipeline_runtime;

COMMENT ON FUNCTION _oo.full_import_native_catalog(uuid, text, uuid, text, bigint, bigint, text, text, text) IS
  'Read one awaiting_publish native import by run id. Attribute types come from pg_attribute. Shadow values are not read.';
