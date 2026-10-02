\set ON_ERROR_STOP on
SELECT fixture_synth.put_receipt(
  'shard_alpha',
  '11111111-1111-4111-8111-111111111111'::uuid,
  1,
  'batch-closed-codec',
  repeat('e', 64),
  'file-offset/v9',
  'src_main',
  0
);
