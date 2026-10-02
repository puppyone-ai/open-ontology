\ir prelude.sql
SELECT schema_version_present::text
FROM jobs.service_readiness;
