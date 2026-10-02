\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
\pset fieldsep '|'
SELECT shard_id, generation::text, state, owner_token::text, lock_key1::text, lock_key2::text
FROM _oo.target_generations
ORDER BY shard_id;
