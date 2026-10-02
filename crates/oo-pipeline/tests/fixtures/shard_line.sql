\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
SELECT fixture_synth.shard_line(:'shard');
