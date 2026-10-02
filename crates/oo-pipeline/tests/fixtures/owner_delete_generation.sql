\set ON_ERROR_STOP on
SET ROLE oo_target_owner;
DELETE FROM _oo.target_generations WHERE shard_id = 'shard_clock';
