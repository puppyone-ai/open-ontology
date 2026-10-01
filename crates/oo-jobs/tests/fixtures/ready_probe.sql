SELECT set_config('jobs.checksum_expand', :'checksum_expand', false);
SELECT (schema_version_present AND checksum = current_setting('jobs.checksum_expand')) AS ready
FROM jobs.service_readiness;
