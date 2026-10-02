\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
SELECT version, checksum, phase, filename
FROM _oo.schema_migrations
ORDER BY version;
