\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
SELECT _oo.adopt_target_generation(
  'shard_fail',
  '22222222-2222-4222-8222-222222222222'::uuid
);
