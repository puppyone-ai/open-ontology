-- cdc-offset-store-v1 control mirror.
-- The adapter hashes this file before execution. Exact replay does not
-- execute it again. A different hash, owner, relation set, or grant set
-- is drift. This script does not create roles, does not touch a target
-- database, and does not repair an existing schema.

CREATE SCHEMA oo_cdc_offset;

REVOKE ALL ON SCHEMA oo_cdc_offset FROM PUBLIC;
GRANT USAGE ON SCHEMA oo_cdc_offset TO oo_cdc_offset_runtime;
REVOKE ALL ON SCHEMA public FROM oo_cdc_offset_runtime;

CREATE TABLE oo_cdc_offset.install_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    CONSTRAINT install_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT install_manifest_one CHECK (singleton),
    CONSTRAINT install_manifest_sha CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_cdc_offset.mirrored_receipt (
    source text NOT NULL,
    table_generation text NOT NULL,
    txn_or_batch_id text NOT NULL,
    config_version text NOT NULL,
    config_digest text NOT NULL,
    type_policy_version text NOT NULL,
    type_digest text NOT NULL,
    hook_digest text NOT NULL,
    schema_digest text NOT NULL,
    payload_digest text NOT NULL,
    protocol_version text NOT NULL,
    position_codec text NOT NULL,
    position_lineage text NOT NULL,
    source_epoch integer NOT NULL,
    applied_pos numeric(38, 0) NOT NULL,
    partial_txn boolean NOT NULL,
    cross_table_consistent boolean NOT NULL,
    txn_complete boolean NOT NULL,
    segment_index integer,
    segment_count integer,
    segment_bitmap bytea NOT NULL,
    fence_shard text NOT NULL,
    fence_generation bigint NOT NULL,
    target_commit_at timestamptz NOT NULL,
    receipt_hash text NOT NULL,
    CONSTRAINT mirrored_receipt_pk PRIMARY KEY (source, table_generation, txn_or_batch_id),
    CONSTRAINT mirrored_receipt_source_check CHECK (source ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT mirrored_receipt_table_generation_check CHECK (table_generation ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT mirrored_receipt_batch_check CHECK (txn_or_batch_id ~ '^[A-Za-z0-9_.:-]{1,128}$'),
    CONSTRAINT mirrored_receipt_config_version_check CHECK (config_version = 'config/v1'),
    CONSTRAINT mirrored_receipt_type_version_check CHECK (type_policy_version = 'type-policy/v1'),
    CONSTRAINT mirrored_receipt_protocol_check CHECK (protocol_version = 'batch/v1'),
    CONSTRAINT mirrored_receipt_config_digest_check CHECK (config_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT mirrored_receipt_type_digest_check CHECK (type_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT mirrored_receipt_hook_digest_check CHECK (hook_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT mirrored_receipt_schema_digest_check CHECK (schema_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT mirrored_receipt_payload_digest_check CHECK (payload_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT mirrored_receipt_codec_check CHECK (
        (
            position_codec IN (
                'pg-commit-lsn/v1',
                'mysql-binlog-pos/v1',
                'batch-cursor-us/v1',
                'snapshot-zero/v1'
            )
        )
        OR (
            position_codec = 'mariadb-gtid/v1'
            AND position_lineage ~ '^d(0|[1-9][0-9]{0,9})\.s([1-9][0-9]{0,9})$'
            AND substring(position_lineage from '^d([0-9]+)')::numeric <= 4294967295
            AND substring(position_lineage from '\.s([0-9]+)$')::numeric BETWEEN 1 AND 4294967295
            AND regexp_replace(applied_pos::text, '\.0+$', '') ~ '^(0|[1-9][0-9]{0,19})$'
        )
        OR (
            position_codec = 'mongo-cluster-time/v1'
            AND NOT cross_table_consistent
            AND position_lineage ~ '^[a-z][a-z0-9_.:-]{0,127}$'
            AND position(',' in position_lineage) = 0
            AND regexp_replace(applied_pos::text, '\.0+$', '') ~ '^[1-9][0-9]{0,37}$'
        )
    ),
    CONSTRAINT mirrored_receipt_lineage_check CHECK (position_lineage ~ '^[a-z][a-z0-9_.:-]{0,127}$'),
    CONSTRAINT mirrored_receipt_epoch_check CHECK (source_epoch >= 0),
    CONSTRAINT mirrored_receipt_pos_check CHECK (applied_pos >= 0),
    CONSTRAINT mirrored_receipt_completion_check CHECK (
        (
            partial_txn = true
            AND txn_complete = false
            AND cross_table_consistent = false
        )
        OR (
            partial_txn = false
            AND txn_complete = true
            AND cross_table_consistent = true
        )
        OR (
            partial_txn = false
            AND txn_complete = true
            AND cross_table_consistent = false
        )
    ),
    CONSTRAINT mirrored_receipt_segment_check CHECK (
        (
            segment_count IS NULL
            AND segment_index IS NULL
            AND octet_length(segment_bitmap) = 0
        )
        OR (
            segment_count >= 1
            AND segment_index >= 0
            AND segment_index < segment_count
            AND octet_length(segment_bitmap) BETWEEN 1 AND 8192
        )
    ),
    CONSTRAINT mirrored_receipt_fence_shard_check CHECK (fence_shard ~ '^[a-z][a-z0-9_.:-]{0,127}$'),
    CONSTRAINT mirrored_receipt_fence_generation_check CHECK (fence_generation >= 1),
    CONSTRAINT mirrored_receipt_hash_check CHECK (receipt_hash ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_cdc_offset.watermark (
    source text NOT NULL,
    position_codec text NOT NULL,
    position_lineage text NOT NULL,
    source_epoch integer NOT NULL,
    applied_pos numeric(38, 0) NOT NULL,
    applied_txn_id text NOT NULL,
    declared_next_txn text NOT NULL,
    declared_next_pos numeric(38, 0) NOT NULL,
    binding_provenance text NOT NULL,
    CONSTRAINT watermark_pk PRIMARY KEY (source, position_codec, position_lineage, source_epoch),
    CONSTRAINT watermark_source_check CHECK (source ~ '^[a-z][a-z0-9_]{0,62}$'),
    CONSTRAINT watermark_codec_check CHECK (
        (
            position_codec IN (
                'pg-commit-lsn/v1',
                'mysql-binlog-pos/v1',
                'batch-cursor-us/v1',
                'snapshot-zero/v1'
            )
        )
        OR (
            position_codec = 'mariadb-gtid/v1'
            AND position_lineage ~ '^d(0|[1-9][0-9]{0,9})\.s([1-9][0-9]{0,9})$'
            AND substring(position_lineage from '^d([0-9]+)')::numeric <= 4294967295
            AND substring(position_lineage from '\.s([0-9]+)$')::numeric BETWEEN 1 AND 4294967295
            AND regexp_replace(applied_pos::text, '\.0+$', '') ~ '^(0|[1-9][0-9]{0,19})$'
        )
        OR (
            position_codec = 'mongo-cluster-time/v1'
            AND position_lineage ~ '^[a-z][a-z0-9_.:-]{0,127}$'
            AND position(',' in position_lineage) = 0
            AND regexp_replace(applied_pos::text, '\.0+$', '') ~ '^[1-9][0-9]{0,37}$'
        )
    ),
    CONSTRAINT watermark_lineage_check CHECK (position_lineage ~ '^[a-z][a-z0-9_.:-]{0,127}$'),
    CONSTRAINT watermark_epoch_check CHECK (source_epoch >= 0),
    CONSTRAINT watermark_pos_check CHECK (applied_pos >= 0 AND declared_next_pos > applied_pos),
    CONSTRAINT watermark_txn_check CHECK (applied_txn_id ~ '^[A-Za-z0-9_.:-]{1,128}$'),
    CONSTRAINT watermark_next_txn_check CHECK (
        declared_next_txn ~ '^[A-Za-z0-9_.:-]{1,128}$'
        AND declared_next_txn <> applied_txn_id
    ),
    CONSTRAINT watermark_provenance_check CHECK (
        binding_provenance = 'caller_declared_pending_real_normalizer'
    )
);

CREATE FUNCTION oo_cdc_offset.reject_receipt_write()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog
AS $$
BEGIN
    IF TG_OP = 'TRUNCATE' OR TG_OP = 'UPDATE' OR TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'mirrored receipt is immutable' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER mirrored_receipt_reject_write
    BEFORE UPDATE OR DELETE ON oo_cdc_offset.mirrored_receipt
    FOR EACH ROW
    EXECUTE FUNCTION oo_cdc_offset.reject_receipt_write();

CREATE TRIGGER mirrored_receipt_reject_truncate
    BEFORE TRUNCATE ON oo_cdc_offset.mirrored_receipt
    FOR EACH STATEMENT
    EXECUTE FUNCTION oo_cdc_offset.reject_receipt_write();

CREATE FUNCTION oo_cdc_offset.reject_watermark_write()
RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = pg_catalog
AS $$
BEGIN
    IF TG_OP = 'TRUNCATE' OR TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'watermark delete is closed' USING ERRCODE = '23514';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.declared_next_pos <= NEW.applied_pos THEN
            RAISE EXCEPTION 'declared next is not after the checkpoint' USING ERRCODE = '23514';
        END IF;
        RETURN NEW;
    END IF;
    IF NEW.source IS DISTINCT FROM OLD.source
        OR NEW.position_codec IS DISTINCT FROM OLD.position_codec
        OR NEW.position_lineage IS DISTINCT FROM OLD.position_lineage
        OR NEW.source_epoch IS DISTINCT FROM OLD.source_epoch
        OR NEW.binding_provenance IS DISTINCT FROM OLD.binding_provenance
    THEN
        RAISE EXCEPTION 'watermark identity is immutable' USING ERRCODE = '23514';
    END IF;
    IF NEW.applied_pos <= OLD.applied_pos OR NEW.applied_txn_id = OLD.applied_txn_id THEN
        RAISE EXCEPTION 'watermark position is not advancing' USING ERRCODE = '23514';
    END IF;
    IF NEW.declared_next_pos <= NEW.applied_pos OR NEW.declared_next_txn = NEW.applied_txn_id THEN
        RAISE EXCEPTION 'declared next is not after the checkpoint' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER watermark_reject_write
    BEFORE INSERT OR UPDATE OR DELETE ON oo_cdc_offset.watermark
    FOR EACH ROW
    EXECUTE FUNCTION oo_cdc_offset.reject_watermark_write();

CREATE TRIGGER watermark_reject_truncate
    BEFORE TRUNCATE ON oo_cdc_offset.watermark
    FOR EACH STATEMENT
    EXECUTE FUNCTION oo_cdc_offset.reject_watermark_write();

REVOKE ALL ON FUNCTION oo_cdc_offset.reject_receipt_write() FROM PUBLIC;
REVOKE ALL ON FUNCTION oo_cdc_offset.reject_watermark_write() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION oo_cdc_offset.reject_receipt_write() TO oo_cdc_offset_runtime;
GRANT EXECUTE ON FUNCTION oo_cdc_offset.reject_watermark_write() TO oo_cdc_offset_runtime;
GRANT EXECUTE ON FUNCTION oo_cdc_offset.reject_receipt_write() TO oo_cdc_offset_admin;
GRANT EXECUTE ON FUNCTION oo_cdc_offset.reject_watermark_write() TO oo_cdc_offset_admin;

REVOKE ALL ON ALL TABLES IN SCHEMA oo_cdc_offset FROM PUBLIC;
GRANT SELECT ON oo_cdc_offset.install_manifest TO oo_cdc_offset_runtime;
GRANT SELECT, INSERT ON oo_cdc_offset.mirrored_receipt TO oo_cdc_offset_runtime;
GRANT SELECT, INSERT ON oo_cdc_offset.watermark TO oo_cdc_offset_runtime;
GRANT UPDATE (
    applied_pos,
    applied_txn_id,
    declared_next_txn,
    declared_next_pos
) ON oo_cdc_offset.watermark TO oo_cdc_offset_runtime;
