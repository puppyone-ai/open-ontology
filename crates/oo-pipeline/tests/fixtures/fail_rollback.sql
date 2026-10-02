\set ON_ERROR_STOP on
SELECT _oo.adopt_target_generation(
  'shard_rb',
  '11111111-1111-4111-8111-111111111111'::uuid
);
BEGIN;
SELECT _oo.fail_target_generation(
  'shard_rb',
  '11111111-1111-4111-8111-111111111111'::uuid,
  1
);
ROLLBACK;
SELECT fixture_synth.put_receipt(
  'shard_rb',
  '11111111-1111-4111-8111-111111111111'::uuid,
  1,
  'batch-rb',
  repeat('e', 64),
  'pg-commit-lsn/v1',
  'src_main',
  42
);
