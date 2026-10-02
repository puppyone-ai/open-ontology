\set ON_ERROR_STOP on
SET search_path = public;
DO $flow$
DECLARE
  v_owner uuid := '11111111-1111-4111-8111-111111111111';
  v_generation bigint;
  v_renewed bigint;
  v_line text;
  v_again text;
  v_year integer;
  v_result text;
BEGIN
  IF current_user <> 'oo_a072_runtime' OR session_user <> 'oo_a072_runtime' THEN
    RAISE EXCEPTION 'fence flow is not running as the runtime login';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = current_user AND rolsuper) THEN
    RAISE EXCEPTION 'runtime login is a superuser';
  END IF;

  v_generation := _oo.adopt_target_generation('shard_clock', v_owner);
  v_year := fixture_synth.adopted_year('shard_clock');
  IF v_year < 2024 THEN
    RAISE EXCEPTION 'adopt used a search_path clock, year %', v_year;
  END IF;
  IF fixture_synth.own_lock_count('shard_clock') <> 1 THEN
    RAISE EXCEPTION 'clock shard lock was not held once';
  END IF;

  v_generation := _oo.adopt_target_generation('shard_alpha', v_owner);
  IF v_generation <> 1 THEN
    RAISE EXCEPTION 'first generation was %', v_generation;
  END IF;
  v_renewed := _oo.adopt_target_generation('shard_alpha', v_owner);
  IF v_renewed <> 1 THEN
    RAISE EXCEPTION 'renewal changed generation to %', v_renewed;
  END IF;
  IF fixture_synth.own_lock_count('shard_alpha') <> 1 THEN
    RAISE EXCEPTION 'adopt stacked the advisory lock';
  END IF;
  PERFORM _oo.renew_target_generation('shard_alpha', v_owner, 1);
  PERFORM _oo.assert_target_fence('shard_alpha', v_owner, 1);

  v_result := fixture_synth.put_receipt(
    'shard_alpha', v_owner, 1, 'batch-0001', repeat('e', 64),
    'pg-commit-lsn/v1', 'src_main', 42
  );
  IF v_result <> 'inserted' THEN
    RAISE EXCEPTION 'expected inserted, got %', v_result;
  END IF;
  v_line := fixture_synth.receipt_line('batch-0001');
  PERFORM pg_catalog.pg_sleep(1);
  v_result := fixture_synth.put_receipt(
    'shard_alpha', v_owner, 1, 'batch-0001', repeat('e', 64),
    'pg-commit-lsn/v1', 'src_main', 42
  );
  IF v_result <> 'replay' THEN
    RAISE EXCEPTION 'expected replay, got %', v_result;
  END IF;
  v_again := fixture_synth.receipt_line('batch-0001');
  IF v_again IS DISTINCT FROM v_line THEN
    RAISE EXCEPTION 'replay changed the stored receipt';
  END IF;
  IF v_line NOT LIKE repeat('e', 64) || '|pg-commit-lsn/v1|src_main|1|%|42|false|true|true' THEN
    RAISE EXCEPTION 'receipt line %', v_line;
  END IF;
  IF abs(extract(epoch FROM (split_part(v_line, '|', 5)::timestamptz - pg_catalog.clock_timestamp()))) > 10 THEN
    RAISE EXCEPTION 'commit time is not the database clock';
  END IF;

  v_result := fixture_synth.commit_bundle(
    'shard_alpha', v_owner, 1, 'batch-bundle', repeat('e', 64)
  );
  IF v_result <> 'inserted' OR fixture_synth.counts_for('batch-bundle') <> '1|1|1' THEN
    RAISE EXCEPTION 'bundle counts %', fixture_synth.counts_for('batch-bundle');
  END IF;
  v_result := fixture_synth.commit_bundle(
    'shard_alpha', v_owner, 1, 'batch-bundle', repeat('e', 64)
  );
  IF v_result <> 'replay' OR fixture_synth.counts_for('batch-bundle') <> '1|1|1' THEN
    RAISE EXCEPTION 'bundle replay duplicated rows';
  END IF;

  v_result := fixture_synth.put_receipt(
    'shard_alpha', v_owner, 1, 'batch-receipt-only', repeat('e', 64),
    'pg-commit-lsn/v1', 'src_main', 42
  );
  IF v_result <> 'inserted' OR fixture_synth.counts_for('batch-receipt-only') <> '0|0|1' THEN
    RAISE EXCEPTION 'receipt-only gap was not observable';
  END IF;

  v_result := fixture_synth.put_receipt(
    'shard_alpha', v_owner, 1, 'batch-partial', repeat('e', 64),
    'snapshot-zero/v1', 'src_main', 0,
    true, false, false
  );
  IF v_result <> 'inserted'
     OR fixture_synth.receipt_line('batch-partial') NOT LIKE '%|snapshot-zero/v1|src_main|1|%|0|true|false|false'
  THEN
    RAISE EXCEPTION 'partial receipt %', fixture_synth.receipt_line('batch-partial');
  END IF;

  v_result := fixture_synth.put_receipt(
    'shard_alpha', v_owner, 1, 'batch-segment', repeat('e', 64),
    'batch-cursor-us/v1', 'src_main', 42,
    false, true, true,
    0, 1, '\x01'::bytea
  );
  IF v_result <> 'inserted' THEN
    RAISE EXCEPTION 'segment receipt was rejected';
  END IF;

  v_result := fixture_synth.put_receipt(
    'shard_alpha', v_owner, 1, 'batch-maxpos', repeat('e', 64),
    'mysql-binlog-pos/v1', 'src_alt',
    99999999999999999999999999999999999999
  );
  IF v_result <> 'inserted' THEN
    RAISE EXCEPTION 'max decimal position was rejected';
  END IF;
END
$flow$;
