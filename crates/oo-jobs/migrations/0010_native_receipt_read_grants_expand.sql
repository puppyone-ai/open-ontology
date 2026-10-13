-- The worker verifies a sealed table receipt and its control CAS before ACK.
-- These are target reads only; Loader and target owner retain all writes.
GRANT SELECT (run_id, state, source_id, source_generation, target_generation)
  ON _oo.full_import_run TO oo_jobs_runtime;
GRANT SELECT (run_id, schema_ident, table_ident, physical_name, exhausted)
  ON _oo.full_import_table TO oo_jobs_runtime;
GRANT SELECT (source, table_generation, txn_or_batch_id, payload_digest, applied_pos, txn_complete)
  ON _oo.apply_receipts TO oo_jobs_runtime;
GRANT SELECT (source, schema_name, table_name, source_generation, last_pos, physical_name)
  ON _oo.table_freshness TO oo_jobs_runtime;
GRANT SELECT (position_codec, applied_pos)
  ON _oo.source_watermark TO oo_jobs_runtime;
