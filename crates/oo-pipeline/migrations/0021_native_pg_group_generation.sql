-- Keep the whole-source transaction generation distinct from each table's
-- independently advanced source generation. Historical group rows remain
-- nullable and retain their original attestation bytes.
ALTER TABLE _oo.native_transaction_group
  ADD COLUMN source_generation bigint CHECK (source_generation > 0);

CREATE FUNCTION _oo.native_transaction_commit_global(
  p_shard text, p_owner uuid, p_generation bigint, p_source text,
  p_xid bigint, p_end_lsn numeric, p_scope_digest text,
  p_entries jsonb, p_job uuid, p_source_generation bigint
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $ng$
DECLARE
  v_actual bigint;
  v_table_min bigint;
BEGIN
  IF p_source_generation IS NULL OR p_source_generation <= 0 THEN
    RAISE EXCEPTION 'transaction source generation is invalid' USING ERRCODE='OO030';
  END IF;
  PERFORM _oo.native_transaction_commit(
    p_shard,p_owner,p_generation,p_source,p_xid,p_end_lsn,
    p_scope_digest,p_entries,p_job);
  SELECT min((value->>'source_generation')::bigint) INTO v_table_min
  FROM jsonb_array_elements(p_entries) AS entry(value);
  SELECT source_generation INTO v_actual FROM _oo.source_watermark
  WHERE source=p_source AND applied_pos=p_end_lsn
    AND target_generation=p_generation AND consistency='transactional'
    AND consistency_grade='T' AND cross_table_consistent FOR UPDATE;
  IF NOT FOUND OR v_actual <> v_table_min THEN
    RAISE EXCEPTION 'transaction table generation differs' USING ERRCODE='OO030';
  END IF;
  UPDATE _oo.source_watermark SET source_generation=p_source_generation
  WHERE source=p_source AND applied_pos=p_end_lsn
    AND target_generation=p_generation AND source_generation=v_table_min;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'transaction source generation is not sealed' USING ERRCODE='OO030';
  END IF;
  UPDATE _oo.native_transaction_group SET source_generation=p_source_generation
  WHERE source=p_source AND xid=p_xid AND end_lsn=p_end_lsn
    AND job_task_id=p_job AND target_generation=p_generation
    AND source_generation IS NULL;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'transaction group generation is not sealed' USING ERRCODE='OO030';
  END IF;
END;
$ng$;

REVOKE ALL ON FUNCTION _oo.native_transaction_commit_global(text,uuid,bigint,text,bigint,numeric,text,jsonb,uuid,bigint) FROM PUBLIC;
REVOKE EXECUTE ON FUNCTION _oo.native_transaction_commit(text,uuid,bigint,text,bigint,numeric,text,jsonb,uuid) FROM oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_transaction_commit_global(text,uuid,bigint,text,bigint,numeric,text,jsonb,uuid,bigint) TO oo_pipeline_runtime;
