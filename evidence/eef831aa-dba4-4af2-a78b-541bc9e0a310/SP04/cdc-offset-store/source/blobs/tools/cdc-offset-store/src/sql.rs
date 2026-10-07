//! Static control-database statements. Names are schema-qualified.

pub const MIGRATION_SQL: &str = include_str!("../migrations/oo_cdc_offset_control_v1.sql");

pub const SESSION: &str = "\
SET LOCAL lock_timeout = '5s'; \
SET LOCAL statement_timeout = '15s'; \
SET LOCAL synchronous_commit = on; \
SET LOCAL search_path = pg_catalog";

pub const READ_MIGRATIONS: &str = "\
SELECT version, checksum, phase, filename \
FROM _oo.schema_migrations";

pub const READ_RECEIPTS: &str = "\
SELECT table_generation, config_version, config_digest, type_policy_version, type_digest, \
       hook_digest, schema_digest, payload_digest, protocol_version, position_codec, \
       position_lineage, source_epoch, applied_pos::text, partial_txn, cross_table_consistent, \
       txn_complete, segment_index, segment_count, encode(segment_bitmap, 'hex'), \
       fence_shard, fence_generation, \
       to_char(target_commit_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
FROM _oo.apply_receipts \
WHERE source = $1 AND txn_or_batch_id = $2 AND table_generation = ANY($3::text[])";

pub const INSERT_RECEIPT: &str = "\
INSERT INTO oo_cdc_offset.mirrored_receipt ( \
  source, table_generation, txn_or_batch_id, config_version, config_digest, type_policy_version, \
  type_digest, hook_digest, schema_digest, payload_digest, protocol_version, position_codec, \
  position_lineage, source_epoch, applied_pos, partial_txn, cross_table_consistent, txn_complete, \
  segment_index, segment_count, segment_bitmap, fence_shard, fence_generation, target_commit_at, \
  receipt_hash \
) VALUES ( \
  $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,($15::text)::numeric,$16,$17,$18,$19,$20,$21,$22,$23,($24::text)::timestamptz,$25 \
)";

pub const SELECT_MIRROR: &str = "\
SELECT receipt_hash FROM oo_cdc_offset.mirrored_receipt \
WHERE source = $1 AND table_generation = $2 AND txn_or_batch_id = $3";

pub const SELECT_WATERMARK: &str = "\
SELECT position_codec, position_lineage, source_epoch, applied_pos::text, applied_txn_id, \
       declared_next_txn, declared_next_pos::text \
FROM oo_cdc_offset.watermark \
WHERE source = $1 AND position_codec = $2 AND position_lineage = $3 AND source_epoch = $4";

pub const INSERT_WATERMARK: &str = "\
INSERT INTO oo_cdc_offset.watermark ( \
  source, position_codec, position_lineage, source_epoch, applied_pos, applied_txn_id, \
  declared_next_txn, declared_next_pos, binding_provenance \
) VALUES ($1,$2,$3,$4,($5::text)::numeric,$6,$7,($8::text)::numeric,'caller_declared_pending_real_normalizer')";

pub const CAS_WATERMARK: &str = "\
UPDATE oo_cdc_offset.watermark \
SET applied_pos = ($5::text)::numeric, applied_txn_id = $6, declared_next_txn = $7, declared_next_pos = ($8::text)::numeric \
WHERE source = $1 AND position_codec = $2 AND position_lineage = $3 AND source_epoch = $4 \
  AND applied_pos = ($9::text)::numeric AND applied_txn_id = $10";
