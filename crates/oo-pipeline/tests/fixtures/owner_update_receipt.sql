\set ON_ERROR_STOP on
SET ROLE oo_target_owner;
UPDATE _oo.apply_receipts
SET payload_digest = repeat('a', 64)
WHERE txn_or_batch_id = 'batch-0001';
