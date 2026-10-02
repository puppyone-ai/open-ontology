SELECT version, checksum, phase, filename, applied_at::text, applied_by
FROM jobs.schema_migrations
WHERE version IN ('jobs-bootstrap/1', 'jobs-control/1')
ORDER BY version;
