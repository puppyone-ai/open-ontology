-- Keep the frozen 0024 23-argument quality seal callable for PostgreSQL.
-- MongoDB C1 additionally supplies the target-registered source instance epoch.
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
  p_mode text,
  p_source_epoch integer
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $epoch_seal$
BEGIN
  IF p_position_codec <> 'mongo-cluster-time/v1'
    OR p_source_epoch IS NULL OR p_source_epoch < 1
    OR NOT EXISTS (
      SELECT 1 FROM _oo.source_instance AS instance
      WHERE instance.source = p_source
        AND instance.source_epoch = p_source_epoch
        AND instance.engine = 'mongodb'
        AND instance.measured_id = p_position_lineage
        AND instance.shard_id = p_shard
        AND instance.owner_token = p_owner
    ) THEN
    RAISE EXCEPTION 'wrong source instance epoch' USING ERRCODE = 'OO010';
  END IF;

  PERFORM _oo.native_update_seal_quality(
    p_shard, p_owner, p_target_generation, p_run, p_source,
    p_source_generation, p_schema, p_table, p_physical, p_batch_id,
    p_payload_digest, p_applied_pos, p_position_codec, p_position_lineage,
    p_capture_method, p_delete_detection, p_consistency_grade,
    p_cross_table, p_tombstones, p_row_count, p_reconcile,
    p_reconcile_diff, p_mode
  );

  UPDATE _oo.source_watermark
  SET source_epoch = p_source_epoch
  WHERE source = p_source
    AND position_codec = 'mongo-cluster-time/v1'
    AND position_lineage = p_position_lineage
    AND applied_pos = p_applied_pos
    AND target_generation = p_target_generation
    AND source_generation = p_source_generation
    AND cross_table_consistent = false
    AND (source_epoch IS NULL OR source_epoch = p_source_epoch OR source_epoch < p_source_epoch);
  IF NOT FOUND THEN
    RAISE EXCEPTION 'source epoch or watermark mismatch' USING ERRCODE = 'OO010';
  END IF;
END;
$epoch_seal$;
