-- target-fence-receipts/3 phase expand
-- Run as oo_target_owner after an approved target-fence-receipts/1 schema,
-- or after target-fence-receipts/2 when that marker is already stored.
-- The applicator holds pg_advisory_xact_lock(6404202605) and, when the row
-- exists, _oo.migration_lock FOR UPDATE. This file does not insert a
-- readiness row. The applicator inserts target-fence-receipts/3 only after
-- this file returns, in the same transaction.
--
-- 0000_bootstrap_roles.sql, 0001_target_fence_expand.sql, and
-- 0002_target_segment_null_expand.sql stay byte-identical. This file
-- supersedes 0002 when the applicator is installing the closed segment
-- rule from v1. It does not rewrite a stored v1 or v2 checksum.
--
-- 0002 folds a regclass cast of the receipt relation while testing
-- to_regclass. On a database with no such relation that cast raises 42P01
-- before the OO002 handler. A later migration in the same aborted
-- transaction cannot repair that failure. The reference applicator
-- therefore executes this file instead of 0002. Running 0002 first on a
-- missing relation is not this install path.
--
-- Preflight resolves the relation with to_regclass into an oid, then looks
-- up apply_receipts_segment_check by that oid. There is no regclass literal.
-- A following probe catches undefined_table and raises the same static OO002.
-- LOCK TABLE is a top-level statement so the share lock is held until the
-- applicator transaction commits. This file does not delete, update, or
-- rewrite _oo.apply_receipts.

SELECT pg_catalog.set_config('search_path', 'pg_catalog, _oo', true);

DO $segment_preflight$
DECLARE
  relation_id oid;
BEGIN
  relation_id := pg_catalog.to_regclass('_oo.apply_receipts');
  IF relation_id IS NULL THEN
    RAISE EXCEPTION 'target-fence-receipts/1 segment relation is absent'
      USING ERRCODE = 'OO002';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM pg_catalog.pg_constraint AS constraint_row
    WHERE constraint_row.conname = 'apply_receipts_segment_check'
      AND constraint_row.conrelid = relation_id
  ) THEN
    RAISE EXCEPTION 'target-fence-receipts/1 segment relation is absent'
      USING ERRCODE = 'OO002';
  END IF;
END
$segment_preflight$;

DO $segment_owner$
DECLARE
  function_owner text;
  function_count integer;
  view_owner text;
  ready_owner text;
  contract_owner text;
  preflight_owner text;
BEGIN
  SELECT count(*), min(pg_catalog.pg_get_userbyid(proc.proowner))
    INTO function_count, function_owner
  FROM pg_catalog.pg_proc AS proc
  JOIN pg_catalog.pg_namespace AS nsp ON nsp.oid = proc.pronamespace
  WHERE nsp.nspname = '_oo'
    AND proc.proname = 'commit_apply_receipt';
  IF function_count > 1
     OR (function_count = 1 AND function_owner IS DISTINCT FROM 'oo_target_owner')
  THEN
    RAISE EXCEPTION 'target segment preflight refuses to adopt commit_apply_receipt'
      USING ERRCODE = '42501';
  END IF;

  SELECT pg_catalog.pg_get_userbyid(proc.proowner)
    INTO contract_owner
  FROM pg_catalog.pg_proc AS proc
  JOIN pg_catalog.pg_namespace AS nsp ON nsp.oid = proc.pronamespace
  WHERE nsp.nspname = '_oo'
    AND proc.proname = 'segment_contract_ready';
  IF contract_owner IS NOT NULL AND contract_owner IS DISTINCT FROM 'oo_target_owner' THEN
    RAISE EXCEPTION 'target segment preflight refuses to adopt segment_contract_ready'
      USING ERRCODE = '42501';
  END IF;

  SELECT pg_catalog.pg_get_userbyid(proc.proowner)
    INTO preflight_owner
  FROM pg_catalog.pg_proc AS proc
  JOIN pg_catalog.pg_namespace AS nsp ON nsp.oid = proc.pronamespace
  WHERE nsp.nspname = '_oo'
    AND proc.proname = 'segment_preflight_ready';
  IF preflight_owner IS NOT NULL AND preflight_owner IS DISTINCT FROM 'oo_target_owner' THEN
    RAISE EXCEPTION 'target segment preflight refuses to adopt segment_preflight_ready'
      USING ERRCODE = '42501';
  END IF;

  SELECT pg_catalog.pg_get_userbyid(cls.relowner)
    INTO view_owner
  FROM pg_catalog.pg_class AS cls
  JOIN pg_catalog.pg_namespace AS nsp ON nsp.oid = cls.relnamespace
  WHERE nsp.nspname = '_oo'
    AND cls.relname = 'segment_service_readiness'
    AND cls.relkind = 'v';
  IF view_owner IS NOT NULL AND view_owner IS DISTINCT FROM 'oo_target_owner' THEN
    RAISE EXCEPTION 'target segment preflight refuses to adopt segment_service_readiness'
      USING ERRCODE = '42501';
  END IF;

  SELECT pg_catalog.pg_get_userbyid(cls.relowner)
    INTO ready_owner
  FROM pg_catalog.pg_class AS cls
  JOIN pg_catalog.pg_namespace AS nsp ON nsp.oid = cls.relnamespace
  WHERE nsp.nspname = '_oo'
    AND cls.relname = 'segment_preflight_readiness'
    AND cls.relkind = 'v';
  IF ready_owner IS NOT NULL AND ready_owner IS DISTINCT FROM 'oo_target_owner' THEN
    RAISE EXCEPTION 'target segment preflight refuses to adopt segment_preflight_readiness'
      USING ERRCODE = '42501';
  END IF;
END
$segment_owner$;

DO $segment_touch$
BEGIN
  PERFORM 1
  FROM _oo.apply_receipts AS receipt
  WHERE false;
EXCEPTION
  WHEN undefined_table THEN
    RAISE EXCEPTION 'target-fence-receipts/1 segment relation is absent'
      USING ERRCODE = 'OO002';
END
$segment_touch$;

LOCK TABLE _oo.apply_receipts IN SHARE ROW EXCLUSIVE MODE;

DO $segment_history$
BEGIN
  IF EXISTS (
    SELECT 1
    FROM _oo.apply_receipts AS receipt
    WHERE (
      (
        receipt.segment_count IS NULL
        AND receipt.segment_index IS NULL
        AND octet_length(receipt.segment_bitmap) = 0
      )
      OR (
        receipt.segment_count IS NOT NULL
        AND receipt.segment_index IS NOT NULL
        AND receipt.segment_count >= 1
        AND receipt.segment_index >= 0
        AND receipt.segment_index < receipt.segment_count
        AND octet_length(receipt.segment_bitmap) BETWEEN 1 AND 8192
      )
    ) IS NOT TRUE
  ) THEN
    RAISE EXCEPTION 'refusing target segment correction: preexisting apply receipts violate the closed segment rule; this migration does not delete, rewrite, or mutate receipts'
      USING ERRCODE = 'OO021';
  END IF;
END
$segment_history$;

ALTER TABLE _oo.apply_receipts
  DROP CONSTRAINT apply_receipts_segment_check;

ALTER TABLE _oo.apply_receipts
  ADD CONSTRAINT apply_receipts_segment_check CHECK (
    (
      (
        segment_count IS NULL
        AND segment_index IS NULL
        AND octet_length(segment_bitmap) = 0
      )
      OR (
        segment_count IS NOT NULL
        AND segment_index IS NOT NULL
        AND segment_count >= 1
        AND segment_index >= 0
        AND segment_index < segment_count
        AND octet_length(segment_bitmap) BETWEEN 1 AND 8192
      )
    ) IS TRUE
  );

COMMENT ON CONSTRAINT apply_receipts_segment_check ON _oo.apply_receipts IS
  'Closed segment metadata. Both index and count are null only with an empty bitmap. A present segment has both fields non-null, count at least 1, index in range, and a bitmap of 1 to 8192 bytes. The IS TRUE wrapper rejects unknown. This constraint does not delete or rewrite an existing row.';

CREATE OR REPLACE FUNCTION _oo.commit_apply_receipt(
  p_shard text,
  p_owner uuid,
  p_generation bigint,
  p_source text,
  p_table_generation text,
  p_txn_or_batch_id text,
  p_config_digest text,
  p_type_digest text,
  p_hook_digest text,
  p_schema_digest text,
  p_payload_digest text,
  p_position_codec text,
  p_position_lineage text,
  p_source_epoch integer,
  p_applied_pos numeric,
  p_partial_txn boolean,
  p_cross_table_consistent boolean,
  p_txn_complete boolean,
  p_segment_index integer,
  p_segment_count integer,
  p_segment_bitmap bytea
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
DECLARE
  v_existing _oo.apply_receipts%ROWTYPE;
BEGIN
  IF p_source IS NULL OR p_source !~ '^[a-z][a-z0-9_]{0,62}$' THEN
    RAISE EXCEPTION 'closed source identity' USING ERRCODE = '23514';
  END IF;
  IF p_table_generation IS NULL OR p_table_generation !~ '^[a-z][a-z0-9_]{0,62}$' THEN
    RAISE EXCEPTION 'closed table generation' USING ERRCODE = '23514';
  END IF;
  IF p_txn_or_batch_id IS NULL OR p_txn_or_batch_id !~ '^[A-Za-z0-9_.:-]{1,128}$' THEN
    RAISE EXCEPTION 'closed batch identity' USING ERRCODE = '23514';
  END IF;
  PERFORM _oo._require_digest(p_config_digest, 'config_digest');
  PERFORM _oo._require_digest(p_type_digest, 'type_digest');
  PERFORM _oo._require_digest(p_hook_digest, 'hook_digest');
  PERFORM _oo._require_digest(p_schema_digest, 'schema_digest');
  PERFORM _oo._require_digest(p_payload_digest, 'payload_digest');
  IF p_position_codec IS NULL OR p_position_codec NOT IN (
    'pg-commit-lsn/v1',
    'mysql-binlog-pos/v1',
    'batch-cursor-us/v1',
    'snapshot-zero/v1'
  ) THEN
    RAISE EXCEPTION 'closed position codec' USING ERRCODE = 'OO015';
  END IF;
  IF p_position_lineage IS NULL OR p_position_lineage !~ '^[a-z][a-z0-9_.:-]{0,127}$' THEN
    RAISE EXCEPTION 'closed position lineage' USING ERRCODE = 'OO015';
  END IF;
  IF p_source_epoch IS NULL OR p_source_epoch < 0 THEN
    RAISE EXCEPTION 'closed source epoch' USING ERRCODE = '22003';
  END IF;
  PERFORM _oo._require_position(p_applied_pos);
  IF p_partial_txn IS NULL
     OR p_cross_table_consistent IS NULL
     OR p_txn_complete IS NULL
     OR p_segment_bitmap IS NULL
  THEN
    RAISE EXCEPTION 'closed completion metadata' USING ERRCODE = '23514';
  END IF;
  IF NOT (
    (
      p_partial_txn
      AND NOT p_txn_complete
      AND NOT p_cross_table_consistent
    )
    OR (
      NOT p_partial_txn
      AND p_txn_complete
      AND p_cross_table_consistent
    )
    OR (
      NOT p_partial_txn
      AND p_txn_complete
      AND NOT p_cross_table_consistent
    )
  ) THEN
    RAISE EXCEPTION 'closed completion metadata' USING ERRCODE = '23514';
  END IF;
  IF (
    (
      p_segment_count IS NULL
      AND p_segment_index IS NULL
      AND octet_length(p_segment_bitmap) = 0
    )
    OR (
      p_segment_count IS NOT NULL
      AND p_segment_index IS NOT NULL
      AND p_segment_count >= 1
      AND p_segment_index >= 0
      AND p_segment_index < p_segment_count
      AND octet_length(p_segment_bitmap) BETWEEN 1 AND 8192
    )
  ) IS NOT TRUE THEN
    RAISE EXCEPTION 'closed segment metadata' USING ERRCODE = '23514';
  END IF;
  PERFORM _oo._load_held_generation(p_shard, p_owner, p_generation);
  SELECT existing.*
    INTO v_existing
  FROM _oo.apply_receipts AS existing
  WHERE existing.source = p_source
    AND existing.table_generation = p_table_generation
    AND existing.txn_or_batch_id = p_txn_or_batch_id
  FOR UPDATE;
  IF FOUND THEN
    IF v_existing.config_digest = p_config_digest
       AND v_existing.type_digest = p_type_digest
       AND v_existing.hook_digest = p_hook_digest
       AND v_existing.schema_digest = p_schema_digest
       AND v_existing.payload_digest = p_payload_digest
       AND v_existing.position_codec = p_position_codec
       AND v_existing.position_lineage = p_position_lineage
       AND v_existing.source_epoch = p_source_epoch
       AND v_existing.applied_pos = p_applied_pos
       AND v_existing.partial_txn = p_partial_txn
       AND v_existing.cross_table_consistent = p_cross_table_consistent
       AND v_existing.txn_complete = p_txn_complete
       AND v_existing.segment_index IS NOT DISTINCT FROM p_segment_index
       AND v_existing.segment_count IS NOT DISTINCT FROM p_segment_count
       AND v_existing.segment_bitmap = p_segment_bitmap
       AND v_existing.config_version = 'config/v1'
       AND v_existing.type_policy_version = 'type-policy/v1'
       AND v_existing.protocol_version = 'batch/v1'
    THEN
      RETURN 'replay';
    END IF;
    RAISE EXCEPTION 'apply receipt identity conflict' USING ERRCODE = 'OO010';
  END IF;
  INSERT INTO _oo.apply_receipts (
    source,
    table_generation,
    txn_or_batch_id,
    config_version,
    config_digest,
    type_policy_version,
    type_digest,
    hook_digest,
    schema_digest,
    payload_digest,
    protocol_version,
    position_codec,
    position_lineage,
    source_epoch,
    applied_pos,
    partial_txn,
    cross_table_consistent,
    txn_complete,
    segment_index,
    segment_count,
    segment_bitmap,
    fence_shard,
    fence_generation,
    target_commit_at
  ) VALUES (
    p_source,
    p_table_generation,
    p_txn_or_batch_id,
    'config/v1',
    p_config_digest,
    'type-policy/v1',
    p_type_digest,
    p_hook_digest,
    p_schema_digest,
    p_payload_digest,
    'batch/v1',
    p_position_codec,
    p_position_lineage,
    p_source_epoch,
    p_applied_pos,
    p_partial_txn,
    p_cross_table_consistent,
    p_txn_complete,
    p_segment_index,
    p_segment_count,
    p_segment_bitmap,
    p_shard,
    p_generation,
    clock_timestamp()
  );
  RETURN 'inserted';
END;
$$;

COMMENT ON FUNCTION _oo.commit_apply_receipt(
  text, uuid, bigint, text, text, text, text, text, text, text, text, text, text, integer, numeric,
  boolean, boolean, boolean, integer, integer, bytea
) IS
  'Inserts one receipt or returns replay when every immutable field matches. Same identity with a different payload, codec, or lineage raises OO010 and does not update the stored row. Segment index and count are closed before the fence check: both null with an empty bitmap, or both non-null with a legal index and a bitmap of 1 to 8192 bytes. Unknown combinations raise 23514 and are not reported as OO013. The fence is checked again in this call. This function does not write user data, quality summaries, reject events, or a control-database mirror. The Loader must call it in the same target transaction as those writes.';

CREATE OR REPLACE FUNCTION _oo.segment_preflight_ready(p_checksum text)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY INVOKER
SET search_path = pg_catalog, _oo
AS $preflight_ready$
  SELECT EXISTS (
    SELECT 1
    FROM _oo.schema_migrations AS recorded
    WHERE recorded.version = 'target-fence-receipts/3'
      AND recorded.phase = 'expand'
      AND recorded.checksum = p_checksum
      AND p_checksum ~ '^[0-9a-f]{64}$'
  );
$preflight_ready$;

COMMENT ON FUNCTION _oo.segment_preflight_ready(text) IS
  'True only when the target-fence-receipts/3 expand row stores the caller checksum. This function does not read 0003_target_segment_preflight_expand.sql and does not treat the stored checksum as a file hash. A driver that requires this contract is authorized only when the caller computed that file''s SHA-256 and this function returns true for that digest. _oo.schema_ready for target-fence-receipts/1 and _oo.segment_contract_ready for target-fence-receipts/2 do not authorize that driver. A stored lie is true only for the lie.';

CREATE OR REPLACE VIEW _oo.segment_preflight_readiness
WITH (security_invoker = true) AS
SELECT
  version,
  checksum,
  phase,
  filename,
  applied_at,
  (version = 'target-fence-receipts/3' AND phase = 'expand') AS schema_version_present
FROM _oo.schema_migrations
WHERE version = 'target-fence-receipts/3';

COMMENT ON VIEW _oo.segment_preflight_readiness IS
  'Version marker for target-fence-receipts/3. schema_version_present does not prove these file bytes. Compare checksum to the SHA-256 of 0003_target_segment_preflight_expand.sql outside the database. target-fence-receipts/1 and target-fence-receipts/2 readiness do not satisfy this view.';

REVOKE ALL ON FUNCTION _oo.segment_preflight_ready(text) FROM PUBLIC;
REVOKE ALL ON TABLE _oo.segment_preflight_readiness FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.segment_preflight_ready(text) TO oo_pipeline_runtime;
GRANT SELECT ON _oo.segment_preflight_readiness TO oo_pipeline_runtime;
