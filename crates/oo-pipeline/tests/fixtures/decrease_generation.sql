\set ON_ERROR_STOP on
SELECT fixture_synth.force_generation('shard_down', 4);
SELECT fixture_synth.force_generation('shard_down', 3);
