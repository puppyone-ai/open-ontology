-- Read the prior whole-source generation under the same adopted target fence.
-- The runtime receives one scalar and no table SELECT grant.
CREATE FUNCTION _oo.native_pg_group_prior(
  p_shard text, p_owner uuid, p_generation bigint, p_source text
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $gp$
DECLARE
  v_generation bigint;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_generation);
  SELECT source_generation INTO v_generation
  FROM _oo.source_watermark
  WHERE source=p_source AND position_codec='pg-commit-lsn/v1'
  FOR UPDATE;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'prior PostgreSQL watermark is absent' USING ERRCODE='OO030';
  END IF;
  RETURN v_generation;
END;
$gp$;
REVOKE ALL ON FUNCTION _oo.native_pg_group_prior(text,uuid,bigint,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_pg_group_prior(text,uuid,bigint,text) TO oo_pipeline_runtime;
