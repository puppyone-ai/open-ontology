-- Read one sealed full import by source identity.
--
-- The caller supplies source id, config digest, and source generation.
-- The function does not accept a physical name, a SQL string, or a role change.
-- It does not change settings, copy rows, or read shadow attributes.
-- oo_pipeline_runtime cannot select the full-import tables. This function runs
-- as its owner and returns one jsonb document.

CREATE FUNCTION _oo.full_import_sealed_identity(
  p_source text,
  p_config_digest text,
  p_source_generation bigint
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $seal$
DECLARE
  v_count integer;
  v_run _oo.full_import_run%ROWTYPE;
  v_partial integer;
  v_tables jsonb;
  v_quality jsonb;
BEGIN
  IF p_source IS NULL OR p_source !~ '^[a-z][a-z0-9_]{0,62}$' THEN
    RAISE EXCEPTION 'sealed import identity is closed' USING ERRCODE = '22023';
  END IF;
  IF p_config_digest IS NULL OR p_config_digest !~ '^[0-9a-f]{64}$' THEN
    RAISE EXCEPTION 'sealed import identity is closed' USING ERRCODE = '22023';
  END IF;
  IF p_source_generation IS NULL OR p_source_generation < 1 THEN
    RAISE EXCEPTION 'sealed import identity is closed' USING ERRCODE = '22023';
  END IF;
  SELECT count(*)::integer
    INTO v_count
  FROM _oo.full_import_run AS run_row
  WHERE run_row.source_id = p_source
    AND run_row.config_digest = p_config_digest
    AND run_row.source_generation = p_source_generation
    AND run_row.state = 'awaiting_publish';
  IF v_count = 0 THEN
    RAISE EXCEPTION 'sealed import is absent' USING ERRCODE = 'P0002';
  END IF;
  IF v_count > 1 THEN
    RAISE EXCEPTION 'sealed import is ambiguous' USING ERRCODE = '21000';
  END IF;
  SELECT run_row.*
    INTO v_run
  FROM _oo.full_import_run AS run_row
  WHERE run_row.source_id = p_source
    AND run_row.config_digest = p_config_digest
    AND run_row.source_generation = p_source_generation
    AND run_row.state = 'awaiting_publish';
  SELECT count(*)::integer
    INTO v_partial
  FROM _oo.full_import_table AS table_row
  WHERE table_row.run_id = v_run.run_id
    AND NOT table_row.exhausted;
  IF v_partial <> 0 THEN
    RAISE EXCEPTION 'sealed import is partial' USING ERRCODE = 'P0002';
  END IF;
  SELECT coalesce(jsonb_agg(jsonb_build_object(
      'applied_rows', table_row.applied_rows,
      'canonical_digest', table_row.canonical_digest,
      'physical_name', table_row.physical_name,
      'schema_ident', table_row.schema_ident,
      'table_ident', table_row.table_ident,
      'table_index', table_row.table_index
    ) ORDER BY table_row.table_index), '[]'::jsonb)
    INTO v_tables
  FROM _oo.full_import_table AS table_row
  WHERE table_row.run_id = v_run.run_id;
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
    'quality', v_quality,
    'run_id', v_run.run_id,
    'schema_digest', v_run.schema_digest,
    'source_generation', v_run.source_generation,
    'source_id', v_run.source_id,
    'state', v_run.state,
    'tables', v_tables,
    'target_generation', v_run.target_generation,
    'type_digest', v_run.type_digest
  );
END;
$seal$;

REVOKE ALL ON FUNCTION _oo.full_import_sealed_identity(text, text, bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.full_import_sealed_identity(text, text, bigint) TO oo_pipeline_runtime;

COMMENT ON FUNCTION _oo.full_import_sealed_identity(text, text, bigint) IS
  'Read one awaiting_publish import. Zero rows are absent. Two rows are ambiguous. A table that is not exhausted is partial. The caller does not pass a physical name.';
