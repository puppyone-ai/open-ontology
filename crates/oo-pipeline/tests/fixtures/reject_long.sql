\set ON_ERROR_STOP on
SELECT _oo.commit_apply_receipt(
  'shard_alpha',
  '11111111-1111-4111-8111-111111111111'::uuid,
  1,
  repeat('a', 64),
  'orders_g1',
  'batch-long',
  repeat('a', 64),
  repeat('b', 64),
  repeat('c', 64),
  repeat('d', 64),
  repeat('e', 64),
  'snapshot-zero/v1',
  'src_main',
  0,
  0,
  false,
  true,
  true,
  NULL,
  NULL,
  ''::bytea
);
