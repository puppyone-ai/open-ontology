\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
SELECT generation::text || '|' || state
FROM _oo.target_generations
WHERE shard_id = 'shard_overflow';
