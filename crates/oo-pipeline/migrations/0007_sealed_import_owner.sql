-- Owner and column identity for one sealed PostgreSQL import.
--
-- `full_import_sealed_identity` stays unchanged and does not grow a key.
-- oo_pipeline_runtime still cannot select the full-import tables.
-- These functions run as their owner. A missing seal, two seals, or a
-- different owner raises. The caller does not pass a physical name.

CREATE FUNCTION _oo.full_import_sealed_owner(
  p_source text,
  p_config_digest text,
  p_source_generation bigint,
  p_owner uuid
)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $owner$
DECLARE
  v_count integer;
  v_owner uuid;
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
  IF p_owner IS NULL THEN
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
  SELECT run_row.owner_token
    INTO v_owner
  FROM _oo.full_import_run AS run_row
  WHERE run_row.source_id = p_source
    AND run_row.config_digest = p_config_digest
    AND run_row.source_generation = p_source_generation
    AND run_row.state = 'awaiting_publish';
  IF v_owner IS DISTINCT FROM p_owner THEN
    RAISE EXCEPTION 'sealed import owner' USING ERRCODE = '42501';
  END IF;
  RETURN true;
END;
$owner$;

CREATE FUNCTION _oo.full_import_sealed_columns(
  p_source text,
  p_config_digest text,
  p_source_generation bigint
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $columns$
DECLARE
  v_count integer;
  v_columns jsonb;
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
  SELECT coalesce(jsonb_agg(jsonb_build_object(
      'logical_column', mapping.logical_column,
      'logical_namespace', mapping.logical_namespace,
      'logical_table', mapping.logical_table,
      'physical_name', mapping.physical_name
    ) ORDER BY mapping.logical_namespace, mapping.logical_table, mapping.logical_column), '[]'::jsonb)
    INTO v_columns
  FROM _oo.physical_mappings AS mapping
  WHERE mapping.source = p_source
    AND mapping.config_digest = p_config_digest
    AND mapping.run_generation = p_source_generation
    AND mapping.level = 'c';
  RETURN v_columns;
END;
$columns$;

REVOKE ALL ON FUNCTION _oo.full_import_sealed_owner(text, text, bigint, uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.full_import_sealed_columns(text, text, bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.full_import_sealed_owner(text, text, bigint, uuid) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.full_import_sealed_columns(text, text, bigint) TO oo_pipeline_runtime;

COMMENT ON FUNCTION _oo.full_import_sealed_owner(text, text, bigint, uuid) IS
  'True when the single awaiting_publish import is owned by p_owner. A different owner is sealed import owner.';

COMMENT ON FUNCTION _oo.full_import_sealed_columns(text, text, bigint) IS
  'Logical column bindings of one awaiting_publish import. The caller does not pass a physical name.';
