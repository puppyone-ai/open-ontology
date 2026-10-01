SELECT version, checksum, phase, filename
FROM jobs.schema_migrations
ORDER BY version;
