\set ON_ERROR_STOP on
SELECT _oo.adopt_target_generation(
  'shard_fail',
  '11111111-1111-4111-8111-111111111111'::uuid
);
SELECT _oo.fail_target_generation(
  'shard_fail',
  '11111111-1111-4111-8111-111111111111'::uuid,
  1
);
SELECT fixture_synth.put_receipt(
  'shard_fail',
  '11111111-1111-4111-8111-111111111111'::uuid,
  1,
  'batch-fail',
  repeat('e', 64),
  'pg-commit-lsn/v1',
  'src_main',
  42
);
