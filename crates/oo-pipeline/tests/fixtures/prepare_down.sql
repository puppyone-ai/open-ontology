\set ON_ERROR_STOP on
SELECT _oo.adopt_target_generation(
  'shard_down',
  '11111111-1111-4111-8111-111111111111'::uuid
);
