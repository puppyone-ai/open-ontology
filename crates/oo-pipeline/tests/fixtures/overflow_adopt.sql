\set ON_ERROR_STOP on
SELECT _oo.adopt_target_generation(
  'shard_overflow',
  '22222222-2222-4222-8222-222222222222'::uuid
);
