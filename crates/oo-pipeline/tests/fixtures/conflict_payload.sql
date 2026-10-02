\set ON_ERROR_STOP on
DO $conflict$
DECLARE
  v_owner uuid := '33333333-3333-4333-8333-333333333333';
  v_generation bigint;
BEGIN
  v_generation := _oo.adopt_target_generation('shard_alpha', v_owner);
  PERFORM fixture_synth.put_receipt(
    'shard_alpha', v_owner, v_generation, 'batch-0001', repeat('f', 64),
    'pg-commit-lsn/v1', 'src_main', 42
  );
END
$conflict$;
