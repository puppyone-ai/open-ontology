-- The scheduled worker checks the latest acknowledged C4 sidecar before it
-- accepts a quiet cycle. Keep that read to the two columns used by the check.
GRANT SELECT (current_signature, partition_scope)
ON _oo.native_partition_state TO oo_jobs_runtime;
