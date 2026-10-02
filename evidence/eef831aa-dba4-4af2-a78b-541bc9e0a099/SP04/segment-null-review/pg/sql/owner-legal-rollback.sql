BEGIN;
SET ROLE oo_target_owner;
SELECT pg_catalog.set_config('_oo.writer', 'apply-receipt', true);
INSERT INTO _oo.apply_receipts (source, table_generation, txn_or_batch_id, config_version, config_digest, type_policy_version, type_digest, hook_digest, schema_digest, payload_digest, protocol_version, position_codec, position_lineage, source_epoch, applied_pos, partial_txn, cross_table_consistent, txn_complete, segment_index, segment_count, segment_bitmap, fence_shard, fence_generation) VALUES ('warehouse', 'orders_g1', 'direct-legal', 'config/v1', repeat('a', 64), 'type-policy/v1', repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'batch/v1', 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, NULL, NULL, ''::bytea, 'ownerlegal', 1);
SELECT 'RESULT|owner-legal';
ROLLBACK;
