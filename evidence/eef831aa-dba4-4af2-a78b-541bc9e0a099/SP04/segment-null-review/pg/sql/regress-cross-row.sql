SELECT 'STAMP|' || target_commit_at::text || '|' || fence_generation::text || '|' || payload_digest FROM _oo.apply_receipts WHERE txn_or_batch_id = 'replay-batch';
