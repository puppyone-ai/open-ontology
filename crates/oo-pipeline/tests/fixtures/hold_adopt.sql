\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
SET application_name = :'app_name';
SELECT _oo.adopt_target_generation(:'shard', :'owner'::uuid);
SELECT pg_catalog.pg_sleep(:sleep_seconds);
