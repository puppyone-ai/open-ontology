\set ON_ERROR_STOP on
SELECT _oo.adopt_target_generation(
  'shard_owner',
  '11111111-1111-4111-8111-111111111111'::uuid
);
SELECT fixture_synth.put_receipt(
  'shard_owner',
  '22222222-2222-4222-8222-222222222222'::uuid,
  1,
  'batch-wrong-owner',
  repeat('e', 64),
  'pg-commit-lsn/v1',
  'src_main',
  42
);
