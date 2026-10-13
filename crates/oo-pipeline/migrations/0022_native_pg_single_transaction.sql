-- A complete one-table PostgreSQL transaction also has an authoritative T scope.
-- Keep every 0019 receipt, order, fence and watermark check unchanged.
-- The first complete CDC transaction may follow a source-bound snapshot-zero baseline.
CREATE OR REPLACE FUNCTION _oo.native_pg_group_prior(
  p_shard text, p_owner uuid, p_generation bigint, p_source text
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $gp$
DECLARE
  v_generation bigint;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_generation);
  SELECT source_generation INTO v_generation
  FROM _oo.source_watermark
  WHERE source=p_source AND (
    position_codec='pg-commit-lsn/v1' OR (
      position_codec='snapshot-zero/v1' AND source_generation=1
      AND applied_pos=0 AND consistency='per_table'
      AND consistency_grade='S1' AND NOT cross_table_consistent
    )
  ) FOR UPDATE;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'prior PostgreSQL watermark is absent' USING ERRCODE='OO030';
  END IF;
  RETURN v_generation;
END;
$gp$;
REVOKE ALL ON FUNCTION _oo.native_pg_group_prior(text,uuid,bigint,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_pg_group_prior(text,uuid,bigint,text) TO oo_pipeline_runtime;

CREATE FUNCTION _oo.native_pg_prior_is_snapshot(
  p_shard text, p_owner uuid, p_generation bigint, p_source text
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $ps$
DECLARE v_codec text;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_generation);
  SELECT position_codec INTO v_codec FROM _oo.source_watermark WHERE source=p_source FOR UPDATE;
  IF NOT FOUND THEN RAISE EXCEPTION 'prior PostgreSQL watermark is absent' USING ERRCODE='OO030'; END IF;
  RETURN v_codec='snapshot-zero/v1';
END;
$ps$;
REVOKE ALL ON FUNCTION _oo.native_pg_prior_is_snapshot(text,uuid,bigint,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_pg_prior_is_snapshot(text,uuid,bigint,text) TO oo_pipeline_runtime;

-- Only a completed, source-bound snapshot vector can bridge snapshot-zero to CDC.
CREATE FUNCTION _oo.native_pg_baseline_receipts(
  p_shard text, p_owner uuid, p_generation bigint, p_source text,
  p_config_digest text, p_entries jsonb
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $gb$
DECLARE
  v_watermark _oo.source_watermark%ROWTYPE;
  v_entry jsonb;
  v_count integer := 0;
  v_key text;
  v_prior text;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_generation);
  SELECT * INTO v_watermark FROM _oo.source_watermark WHERE source=p_source FOR UPDATE;
  IF NOT FOUND THEN RAISE EXCEPTION 'prior PostgreSQL watermark is absent' USING ERRCODE='OO030'; END IF;
  IF v_watermark.position_codec='pg-commit-lsn/v1' THEN RETURN false; END IF;
  IF v_watermark.position_codec<>'snapshot-zero/v1' OR v_watermark.source_generation<>1
    OR v_watermark.applied_pos<>0 OR v_watermark.consistency<>'per_table'
    OR v_watermark.consistency_grade<>'S1' OR v_watermark.cross_table_consistent
    OR p_config_digest IS NULL OR p_config_digest !~ '^[0-9a-f]{64}$'
    OR jsonb_typeof(p_entries) IS DISTINCT FROM 'array'
    OR jsonb_array_length(p_entries) NOT BETWEEN 1 AND 32
  THEN RAISE EXCEPTION 'snapshot predecessor is not sealed' USING ERRCODE='OO030'; END IF;
  FOR v_entry IN SELECT value FROM jsonb_array_elements(p_entries) AS item(value) LOOP
    v_key := (v_entry->>'logical_schema') || chr(31) || (v_entry->>'logical_table');
    IF v_key IS NULL OR (v_prior IS NOT NULL AND v_prior >= v_key)
      OR v_entry->>'source_id' IS DISTINCT FROM p_source
      OR (v_entry->>'source_generation')::bigint IS DISTINCT FROM 1
      OR v_entry->>'source_bound' IS DISTINCT FROM 'true'
      OR v_entry->'producer_trust'->>'trusted' IS DISTINCT FROM 'true'
      OR v_entry->>'load_state' IS NULL
      OR v_entry->>'load_state' NOT IN ('awaiting_publish','replay')
      OR NOT EXISTS (
        SELECT 1 FROM _oo.table_freshness f
        JOIN _oo.update_batch b ON b.source=f.source AND b.schema_name=f.schema_name
          AND b.table_name=f.table_name AND b.source_generation=f.source_generation
          AND b.target_generation=f.target_generation AND b.physical_name=f.physical_name
        JOIN _oo.full_import_run r ON r.run_id=b.run_id
        JOIN _oo.apply_receipts a ON a.source=f.source AND a.table_generation=f.physical_name
          AND a.txn_or_batch_id=b.batch_id AND a.payload_digest=b.payload_digest
        WHERE f.source=p_source AND f.schema_name=v_entry->>'logical_schema'
          AND f.table_name=v_entry->>'logical_table' AND f.source_generation=1
          AND f.target_generation=(v_entry->>'target_generation')::bigint
          AND f.physical_name=v_entry->>'physical_name' AND f.last_pos=0
          AND b.run_id=(v_entry->>'run_id')::uuid
          AND b.payload_digest=v_entry->>'payload_digest'
          AND r.config_digest=p_config_digest AND r.source_id=p_source
          AND r.source_generation=1 AND r.target_generation=f.target_generation
          AND a.applied_pos=0 AND a.position_codec='snapshot-zero/v1'
          AND a.txn_complete AND NOT a.partial_txn
      )
    THEN RAISE EXCEPTION 'snapshot predecessor receipts are incomplete' USING ERRCODE='OO030'; END IF;
    v_prior := v_key;
    v_count := v_count + 1;
  END LOOP;
  IF v_count<>(SELECT count(*) FROM _oo.table_freshness WHERE source=p_source AND source_generation=1)
    OR v_watermark.target_generation<>(SELECT max((value->>'target_generation')::bigint)
       FROM jsonb_array_elements(p_entries) AS item(value))
  THEN RAISE EXCEPTION 'snapshot predecessor table set is incomplete' USING ERRCODE='OO030'; END IF;
  RETURN true;
END;
$gb$;
REVOKE ALL ON FUNCTION _oo.native_pg_baseline_receipts(text,uuid,bigint,text,text,jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_pg_baseline_receipts(text,uuid,bigint,text,text,jsonb) TO oo_pipeline_runtime;

CREATE OR REPLACE FUNCTION _oo.native_transaction_commit(
  p_shard text, p_owner uuid, p_generation bigint, p_source text,
  p_xid bigint, p_end_lsn numeric, p_scope_digest text,
  p_entries jsonb, p_job uuid
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $ng$
DECLARE
  v_entry jsonb;
  v_prior text;
  v_key text;
  v_count integer := 0;
  v_watermark _oo.source_watermark%ROWTYPE;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard,p_owner,p_generation);
  IF p_source IS NULL OR p_source = '' OR p_xid IS NULL OR p_xid <= 0
     OR p_end_lsn IS NULL OR p_end_lsn <= 0 OR p_job IS NULL
     OR p_scope_digest IS NULL OR p_scope_digest !~ '^[0-9a-f]{64}$'
     OR jsonb_typeof(p_entries) IS DISTINCT FROM 'array'
     OR jsonb_array_length(p_entries) NOT BETWEEN 1 AND 32
  THEN RAISE EXCEPTION 'transaction group is closed' USING ERRCODE='23514'; END IF;
  FOR v_entry IN SELECT value FROM jsonb_array_elements(p_entries) AS item(value) LOOP
    IF jsonb_typeof(v_entry) <> 'object'
       OR (SELECT count(*) FROM jsonb_object_keys(v_entry)) <> 8
       OR v_entry->>'logical_schema' IS NULL OR v_entry->>'logical_table' IS NULL
       OR v_entry->>'source_generation' IS NULL OR v_entry->>'target_generation' IS NULL
       OR v_entry->>'physical_name' IS NULL OR v_entry->>'batch_id' IS NULL
       OR v_entry->>'payload_digest' IS NULL OR v_entry->>'run_id' IS NULL
    THEN RAISE EXCEPTION 'transaction group is closed' USING ERRCODE='23514'; END IF;
    v_key := (v_entry->>'logical_schema') || chr(31) || (v_entry->>'logical_table');
    IF v_prior IS NOT NULL AND v_prior >= v_key THEN
      RAISE EXCEPTION 'transaction group order is closed' USING ERRCODE='23514';
    END IF;
    v_prior := v_key;
    v_count := v_count + 1;
    IF NOT EXISTS (SELECT 1 FROM _oo.update_batch AS batch
      WHERE batch.source=p_source AND batch.schema_name=(v_entry->>'logical_schema')
        AND batch.table_name=(v_entry->>'logical_table')
        AND batch.batch_id=(v_entry->>'batch_id')
        AND batch.run_id=(v_entry->>'run_id')::uuid
        AND batch.payload_digest=(v_entry->>'payload_digest')
        AND batch.source_generation=(v_entry->>'source_generation')::bigint
        AND batch.target_generation=(v_entry->>'target_generation')::bigint
        AND batch.physical_name=(v_entry->>'physical_name'))
       OR NOT EXISTS (SELECT 1 FROM _oo.apply_receipts AS receipt
      WHERE receipt.source=p_source AND receipt.table_generation=(v_entry->>'physical_name')
        AND receipt.txn_or_batch_id=(v_entry->>'batch_id')
        AND receipt.payload_digest=(v_entry->>'payload_digest')
        AND receipt.applied_pos=p_end_lsn AND receipt.txn_complete AND NOT receipt.partial_txn
        AND receipt.position_codec='snapshot-zero/v1'
        AND NOT receipt.cross_table_consistent)
       OR NOT EXISTS (SELECT 1 FROM _oo.table_freshness AS fresh
      WHERE fresh.source=p_source AND fresh.schema_name=(v_entry->>'logical_schema')
        AND fresh.table_name=(v_entry->>'logical_table')
        AND fresh.source_generation=(v_entry->>'source_generation')::bigint
        AND fresh.target_generation=(v_entry->>'target_generation')::bigint
        AND fresh.physical_name=(v_entry->>'physical_name') AND fresh.last_pos=p_end_lsn)
    THEN RAISE EXCEPTION 'transaction receipt coverage is absent' USING ERRCODE='OO030'; END IF;
  END LOOP;
  IF v_count <> jsonb_array_length(p_entries) THEN
    RAISE EXCEPTION 'transaction group is closed' USING ERRCODE='23514';
  END IF;
  SELECT * INTO v_watermark FROM _oo.source_watermark WHERE source=p_source FOR UPDATE;
  IF NOT FOUND OR v_watermark.consistency <> 'transactional'
     OR v_watermark.consistency_grade <> 'T' OR NOT v_watermark.cross_table_consistent
     OR v_watermark.applied_pos <> p_end_lsn OR v_watermark.target_generation <> p_generation
     OR v_watermark.source_generation <> (
       SELECT min((item.value->>'source_generation')::bigint)
       FROM jsonb_array_elements(p_entries) AS item(value))
  THEN RAISE EXCEPTION 'transaction watermark is incomplete' USING ERRCODE='OO030'; END IF;
  INSERT INTO _oo.native_transaction_group
    (source,xid,end_lsn,scope_digest,entries,job_task_id,target_generation)
  VALUES (p_source,p_xid,p_end_lsn,p_scope_digest,p_entries,p_job,p_generation);
END;
$ng$;
