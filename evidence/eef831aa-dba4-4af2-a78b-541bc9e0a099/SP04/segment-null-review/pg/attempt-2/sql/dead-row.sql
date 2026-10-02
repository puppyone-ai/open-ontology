SELECT fence_generation::text || '|' || target_commit_at::text || '|' || payload_digest FROM _oo.apply_receipts WHERE txn_or_batch_id = 'dead-batch';
