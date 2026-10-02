\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
SELECT schema_version_present AND checksum = :'checksum_expand'
FROM _oo.service_readiness;
