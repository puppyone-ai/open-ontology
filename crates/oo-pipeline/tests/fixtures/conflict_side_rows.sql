\set ON_ERROR_STOP on
DO $conflict$
DECLARE
  v_owner uuid := '33333333-3333-4333-8333-333333333333';
  v_generation bigint;
BEGIN
  v_generation := _oo.adopt_target_generation('shard_alpha', v_owner);
  PERFORM fixture_synth.conflict_with_side_rows('shard_alpha', v_owner, v_generation);
END
$conflict$;
