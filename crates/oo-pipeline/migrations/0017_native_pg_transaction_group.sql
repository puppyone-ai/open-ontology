-- Atomic multi-table PostgreSQL C1 attestation. A retry may replay only this
-- record, which is written in the same target transaction as every table.
CREATE TABLE _oo.native_transaction_group (
  source text NOT NULL,
  xid bigint NOT NULL CHECK (xid > 0),
  end_lsn numeric(38,0) NOT NULL CHECK (end_lsn > 0),
  scope_digest text NOT NULL CHECK (scope_digest ~ '^[0-9a-f]{64}$'),
  entries jsonb NOT NULL CHECK (jsonb_typeof(entries) = 'array'),
  job_task_id uuid NOT NULL,
  target_generation bigint NOT NULL,
  committed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (source, xid, end_lsn)
);
REVOKE ALL ON _oo.native_transaction_group FROM PUBLIC, oo_pipeline_runtime;

CREATE FUNCTION _oo.native_transaction_commit(
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
     OR jsonb_array_length(p_entries) NOT BETWEEN 2 AND 32
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
        AND receipt.cross_table_consistent)
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
  THEN RAISE EXCEPTION 'transaction watermark is incomplete' USING ERRCODE='OO030'; END IF;
  INSERT INTO _oo.native_transaction_group
    (source,xid,end_lsn,scope_digest,entries,job_task_id,target_generation)
  VALUES (p_source,p_xid,p_end_lsn,p_scope_digest,p_entries,p_job,p_generation);
END;
$ng$;

CREATE FUNCTION _oo.native_transaction_read(p_source text,p_xid bigint,p_end_lsn numeric)
RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $ng$
  SELECT to_jsonb(group_row) FROM _oo.native_transaction_group AS group_row
  WHERE source=p_source AND xid=p_xid AND end_lsn=p_end_lsn;
$ng$;

REVOKE ALL ON FUNCTION _oo.native_transaction_commit(text,uuid,bigint,text,bigint,numeric,text,jsonb,uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.native_transaction_read(text,bigint,numeric) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_transaction_commit(text,uuid,bigint,text,bigint,numeric,text,jsonb,uuid) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_transaction_read(text,bigint,numeric) TO oo_pipeline_runtime,oo_jobs_runtime;
