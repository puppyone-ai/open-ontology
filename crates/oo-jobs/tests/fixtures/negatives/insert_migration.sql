\set ON_ERROR_STOP on
SET ROLE oo_jobs_runtime;
INSERT INTO jobs.schema_migrations (version, checksum, phase, filename)
VALUES ('jobs-evil/1', repeat('d', 64), 'expand', 'evil.sql');
