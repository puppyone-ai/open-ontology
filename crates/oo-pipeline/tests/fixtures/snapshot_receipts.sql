\set ON_ERROR_STOP on
\pset tuples_only on
\pset format unaligned
\pset fieldsep '|'
SELECT txn_or_batch_id, payload_digest, position_codec, position_lineage,
       fence_generation, applied_pos::text, partial_txn::text, target_commit_at::text
FROM _oo.apply_receipts
ORDER BY txn_or_batch_id;
