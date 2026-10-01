SELECT to_regclass('jobs._rollback_probe') IS NULL AS probe_absent;
SELECT version FROM jobs.schema_migrations ORDER BY version;
SELECT schema_version_present FROM jobs.service_readiness;
