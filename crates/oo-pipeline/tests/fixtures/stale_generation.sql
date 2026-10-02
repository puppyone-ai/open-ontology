\set ON_ERROR_STOP on
SELECT _oo.adopt_target_generation(
  'shard_stale',
  '11111111-1111-4111-8111-111111111111'::uuid
);
SELECT fixture_synth.put_receipt(
  'shard_stale',
  '11111111-1111-4111-8111-111111111111'::uuid,
  99,
  'batch-stale',
  repeat('e', 64),
  'pg-commit-lsn/v1',
  'src_main',
  42
);
