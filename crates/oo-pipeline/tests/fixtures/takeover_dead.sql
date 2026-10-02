\set ON_ERROR_STOP on
DO $dead$
DECLARE
  v_owner uuid := '22222222-2222-4222-8222-222222222222';
  v_generation bigint;
  v_result text;
BEGIN
  v_generation := _oo.adopt_target_generation('shard_dead', v_owner);
  IF v_generation <> 2 THEN
    RAISE EXCEPTION 'takeover generation was %', v_generation;
  END IF;
  BEGIN
    PERFORM fixture_synth.put_receipt(
      'shard_dead', v_owner, 1, 'batch-dead', repeat('e', 64),
      'pg-commit-lsn/v1', 'src_main', 42
    );
    RAISE EXCEPTION 'stale generation committed';
  EXCEPTION
    WHEN SQLSTATE 'OO013' THEN
      NULL;
  END;
  v_result := fixture_synth.put_receipt(
    'shard_dead', v_owner, v_generation, 'batch-dead', repeat('e', 64),
    'pg-commit-lsn/v1', 'src_main', 42
  );
  IF v_result <> 'inserted' THEN
    RAISE EXCEPTION 'current generation was %', v_result;
  END IF;
END
$dead$;
