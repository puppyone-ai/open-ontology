\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
SELECT _oo.adopt_target_generation(:'shard', :'owner'::uuid);
