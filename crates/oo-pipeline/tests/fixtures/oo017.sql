\set ON_ERROR_STOP on
SELECT _oo.adopt_target_generation(
  'shard_oo017',
  '11111111-1111-4111-8111-111111111111'::uuid
);
SELECT _oo.adopt_target_generation(
  'shard_oo017',
  '22222222-2222-4222-8222-222222222222'::uuid
);
