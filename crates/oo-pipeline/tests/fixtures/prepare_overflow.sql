\set ON_ERROR_STOP on
SELECT _oo.adopt_target_generation(
  'shard_overflow',
  '11111111-1111-4111-8111-111111111111'::uuid
);
SELECT _oo.fail_target_generation(
  'shard_overflow',
  '11111111-1111-4111-8111-111111111111'::uuid,
  1
);
