-- The production worker verifies exact target receipts and migration digests.
-- It never inserts or updates target task, watermark, or partition rows.
GRANT USAGE ON SCHEMA _oo TO oo_jobs_runtime;
GRANT SELECT (
  source, schema_name, table_name, partition_id, run_id, batch_id,
  payload_digest, job_task_id, job_generation, acknowledged
) ON _oo.native_partition_state TO oo_jobs_runtime;
GRANT SELECT (source, source_generation)
  ON _oo.source_watermark TO oo_jobs_runtime;
GRANT SELECT (version, checksum, filename)
  ON jobs.schema_migrations TO oo_jobs_runtime;
