\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
SELECT _oo.schema_ready(:'checksum_expand');
SELECT schema_version_present FROM _oo.service_readiness;
