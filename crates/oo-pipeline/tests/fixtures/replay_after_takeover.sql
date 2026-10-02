\set ON_ERROR_STOP on
DO $take$
DECLARE
  v_owner uuid := '22222222-2222-4222-8222-222222222222';
  v_generation bigint;
  v_result text;
  v_line text;
BEGIN
  v_generation := _oo.adopt_target_generation('shard_alpha', v_owner);
  IF v_generation <> 2 THEN
    RAISE EXCEPTION 'takeover generation was %', v_generation;
  END IF;
  v_result := fixture_synth.put_receipt(
    'shard_alpha', v_owner, v_generation, 'batch-0001', repeat('e', 64),
    'pg-commit-lsn/v1', 'src_main', 42
  );
  IF v_result <> 'replay' THEN
    RAISE EXCEPTION 'later owner retry was %', v_result;
  END IF;
  v_line := fixture_synth.receipt_line('batch-0001');
  IF split_part(v_line, '|', 4) <> '1' THEN
    RAISE EXCEPTION 'retry rewrote fence generation %', v_line;
  END IF;
  IF fixture_synth.counts_for('batch-0001') <> '0|0|1' THEN
    RAISE EXCEPTION 'retry duplicated receipt rows';
  END IF;
  IF fixture_synth.shard_line('shard_alpha') <> '2|adopted|' || v_owner::text THEN
    RAISE EXCEPTION 'shard line %', fixture_synth.shard_line('shard_alpha');
  END IF;
END
$take$;
