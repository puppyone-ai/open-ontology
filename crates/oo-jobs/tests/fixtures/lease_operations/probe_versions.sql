\ir prelude.sql
SELECT coalesce(string_agg(version || '|' || checksum, E'\n' ORDER BY version), '')
FROM jobs.schema_migrations;
