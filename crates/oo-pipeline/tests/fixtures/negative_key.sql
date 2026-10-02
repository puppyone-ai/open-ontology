\set ON_ERROR_STOP on
DO $neg$
DECLARE
  v_owner uuid := '11111111-1111-4111-8111-111111111111';
  v_key integer;
  v_generation bigint;
BEGIN
  v_key := fixture_synth.lock_key1('shard_0');
  IF v_key >= 0 THEN
    RAISE EXCEPTION 'shard_0 key1 was %', v_key;
  END IF;
  v_generation := _oo.adopt_target_generation('shard_0', v_owner);
  IF v_generation <> 1 OR fixture_synth.own_lock_count('shard_0') <> 1 THEN
    RAISE EXCEPTION 'negative key was not adopted once';
  END IF;
END
$neg$;
