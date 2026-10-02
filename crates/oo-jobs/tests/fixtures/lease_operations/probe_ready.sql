\ir prelude.sql
SELECT schema_version_present::text || '|' || checksum
FROM jobs.lease_operations_readiness;
