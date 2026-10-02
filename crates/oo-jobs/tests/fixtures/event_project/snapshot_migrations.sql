SELECT version, checksum, phase, filename, applied_at::text, applied_by
FROM jobs.schema_migrations
ORDER BY version;
