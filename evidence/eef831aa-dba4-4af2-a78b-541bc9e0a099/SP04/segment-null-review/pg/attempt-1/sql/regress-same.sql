SELECT 'ADOPT|' || _oo.adopt_target_generation('a099replay', '11111111-1111-4111-8111-111111111111'::uuid)::text;
SELECT 'RESULT|' || _oo.commit_apply_receipt('a099replay', '11111111-1111-4111-8111-111111111111'::uuid, 1, 'warehouse', 'orders_g1', 'replay-batch', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, 0, 1, '\x01'::bytea);
SELECT 'STAMP|' || target_commit_at::text || '|' || fence_generation::text || '|' || payload_digest FROM _oo.apply_receipts WHERE txn_or_batch_id = 'replay-batch';

SELECT 'RESULT|' || _oo.commit_apply_receipt('a099replay', '11111111-1111-4111-8111-111111111111'::uuid, 1, 'warehouse', 'orders_g1', 'replay-batch', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, 0, 1, '\x01'::bytea);
SELECT 'STAMP|' || target_commit_at::text || '|' || fence_generation::text || '|' || payload_digest FROM _oo.apply_receipts WHERE txn_or_batch_id = 'replay-batch';
