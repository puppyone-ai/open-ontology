-- GTID transaction identity is the 5-tuple. Positions are never combined
-- across codec, lineage, or source epoch. source_epoch is the ordinal of a
-- registered source instance, not a binlog field and not a caller argument.

CREATE FUNCTION _oo.native_pos_text(p_pos numeric)
RETURNS text
LANGUAGE sql
IMMUTABLE
STRICT
SET search_path = pg_catalog
AS $$
  SELECT regexp_replace(p_pos::text, '\.0+$', '');
$$;

CREATE FUNCTION _oo.native_position_closed(
  p_codec text,
  p_lineage text,
  p_pos numeric,
  p_epoch integer
)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
SET search_path = pg_catalog
AS $$
  SELECT CASE
    WHEN p_codec IN (
      'pg-commit-lsn/v1',
      'mysql-binlog-pos/v1',
      'batch-cursor-us/v1',
      'snapshot-zero/v1',
      'partition-signature/v1',
      'mssql-ct-version/v1'
    ) THEN true
    WHEN p_codec = 'mariadb-gtid/v1'
      AND p_lineage ~ '^d(0|[1-9][0-9]{0,9})\.s([1-9][0-9]{0,9})$'
      AND substring(p_lineage from '^d([0-9]+)')::numeric <= 4294967295
      AND substring(p_lineage from '\.s([0-9]+)$')::numeric BETWEEN 1 AND 4294967295
      AND p_pos IS NOT NULL
      AND _oo.native_pos_text(p_pos) ~ '^(0|[1-9][0-9]{0,19})$'
      AND (p_epoch IS NULL OR p_epoch >= 0)
    THEN true
    WHEN p_codec = 'mysql-gtid/v1'
      AND p_lineage ~ '^mysql\.[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
      AND p_pos IS NOT NULL
      AND _oo.native_pos_text(p_pos) ~ '^[1-9][0-9]{0,19}$'
      AND (p_epoch IS NULL OR p_epoch >= 0)
    THEN true
    WHEN p_codec = 'mongo-cluster-time/v1'
      AND p_lineage ~ '^[a-z][a-z0-9_.:-]{0,127}$'
      AND position(',' in p_lineage) = 0
      AND p_pos IS NOT NULL
      AND _oo.native_pos_text(p_pos) ~ '^[1-9][0-9]{0,37}$'
      AND (p_epoch IS NULL OR p_epoch >= 0)
    THEN true
    ELSE false
  END;
$$;

ALTER TABLE _oo.apply_receipts DROP CONSTRAINT apply_receipts_codec_check;
ALTER TABLE _oo.apply_receipts ADD CONSTRAINT apply_receipts_codec_check CHECK (
  _oo.native_position_closed(position_codec, position_lineage, applied_pos, source_epoch)
  AND (position_codec <> 'mongo-cluster-time/v1' OR NOT cross_table_consistent)
);

ALTER TABLE _oo.source_watermark ADD COLUMN source_epoch integer;
ALTER TABLE _oo.source_watermark DROP CONSTRAINT source_watermark_codec_check;
ALTER TABLE _oo.source_watermark ADD CONSTRAINT source_watermark_epoch_check CHECK (
  source_epoch IS NULL OR source_epoch >= 0
);
ALTER TABLE _oo.source_watermark ADD CONSTRAINT source_watermark_codec_check CHECK (
  position_codec = 'partition-signature/v1'
  OR (
    source_epoch IS NULL
    AND position_codec IN (
      'pg-commit-lsn/v1',
      'mysql-binlog-pos/v1',
      'batch-cursor-us/v1',
      'snapshot-zero/v1',
      'mssql-ct-version/v1'
    )
  )
  OR (
    position_codec IN ('mariadb-gtid/v1', 'mysql-gtid/v1', 'mongo-cluster-time/v1')
    AND _oo.native_position_closed(position_codec, position_lineage, applied_pos, COALESCE(source_epoch, 0))
  )
);

CREATE TABLE _oo.source_instance (
  source text NOT NULL,
  source_epoch integer NOT NULL,
  engine text NOT NULL,
  measured_id text NOT NULL,
  config_digest text NOT NULL,
  schema_digest text NOT NULL,
  shard_id text NOT NULL,
  owner_token uuid NOT NULL,
  lease_id uuid NOT NULL,
  lease_generation bigint NOT NULL,
  registered_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (source, source_epoch),
  UNIQUE (source, measured_id),
  CONSTRAINT source_instance_source_check CHECK (source ~ '^[a-z][a-z0-9_]{0,62}$'),
  CONSTRAINT source_instance_epoch_check CHECK (source_epoch >= 1),
  CONSTRAINT source_instance_engine_check CHECK (engine IN ('mariadb', 'mysql', 'mongodb')),
  CONSTRAINT source_instance_digest_check CHECK (
    config_digest ~ '^[0-9a-f]{64}$' AND schema_digest ~ '^[0-9a-f]{64}$'
  ),
  CONSTRAINT source_instance_shard_check CHECK (shard_id ~ '^[a-z][a-z0-9_.:-]{0,127}$'),
  CONSTRAINT source_instance_lease_check CHECK (lease_generation >= 1)
);

CREATE TABLE _oo.native_gtid_transaction (
  source text NOT NULL,
  position_codec text NOT NULL,
  position_lineage text NOT NULL,
  source_epoch integer NOT NULL,
  applied_pos numeric(38, 0) NOT NULL,
  txn_id text NOT NULL,
  scope_digest text NOT NULL,
  job_task_id uuid NOT NULL,
  target_generation bigint NOT NULL,
  group_generation bigint NOT NULL,
  shard_id text NOT NULL,
  owner_token uuid NOT NULL,
  entries jsonb NOT NULL,
  acknowledged_at timestamptz,
  PRIMARY KEY (source, position_codec, position_lineage, source_epoch, applied_pos),
  CONSTRAINT native_gtid_transaction_source_check CHECK (source ~ '^[a-z][a-z0-9_]{0,62}$'),
  CONSTRAINT native_gtid_transaction_epoch_check CHECK (source_epoch >= 0),
  CONSTRAINT native_gtid_transaction_pos_check CHECK (applied_pos >= 0 AND applied_pos = trunc(applied_pos)),
  CONSTRAINT native_gtid_transaction_digest_check CHECK (scope_digest ~ '^[0-9a-f]{64}$'),
  CONSTRAINT native_gtid_transaction_txn_check CHECK (txn_id ~ '^[A-Za-z0-9_.:-]{1,128}$' AND position(',' in txn_id) = 0),
  CONSTRAINT native_gtid_transaction_entries_check CHECK (jsonb_typeof(entries) = 'array')
);

CREATE FUNCTION _oo.native_source_instance_register(
  p_shard text,
  p_owner uuid,
  p_lease_generation bigint,
  p_source text,
  p_engine text,
  p_measured_id text,
  p_config_digest text,
  p_schema_digest text,
  p_lease uuid
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $gtid_register$
DECLARE
  v_existing _oo.source_instance%ROWTYPE;
  v_epoch integer;
BEGIN
  IF p_source IS NULL OR p_source !~ '^[a-z][a-z0-9_]{0,62}$' THEN
    RAISE EXCEPTION 'closed source identity' USING ERRCODE = '23514';
  END IF;
  IF p_shard IS NULL OR p_shard !~ '^[a-z][a-z0-9_.:-]{0,127}$' OR p_owner IS NULL THEN
    RAISE EXCEPTION 'closed source identity' USING ERRCODE = '23514';
  END IF;
  PERFORM _oo._require_digest(p_config_digest, 'config_digest');
  PERFORM _oo._require_digest(p_schema_digest, 'schema_digest');
  IF p_measured_id IS NULL OR position(',' in p_measured_id) > 0 THEN
    RAISE EXCEPTION 'gtid identity' USING ERRCODE = '23514';
  END IF;
  IF p_engine = 'mariadb' THEN
    IF NOT _oo.native_position_closed('mariadb-gtid/v1', p_measured_id, 0, 0) THEN
      RAISE EXCEPTION 'gtid identity' USING ERRCODE = '23514';
    END IF;
  ELSIF p_engine = 'mysql' THEN
    IF NOT _oo.native_position_closed('mysql-gtid/v1', p_measured_id, 1, 0) THEN
      RAISE EXCEPTION 'gtid identity' USING ERRCODE = '23514';
    END IF;
  ELSIF p_engine = 'mongodb' THEN
    IF NOT _oo.native_position_closed('mongo-cluster-time/v1', p_measured_id, 1, 0) THEN
      RAISE EXCEPTION 'gtid identity' USING ERRCODE = '23514';
    END IF;
  ELSE
    RAISE EXCEPTION 'gtid identity' USING ERRCODE = '23514';
  END IF;
  IF p_lease IS NULL OR p_lease_generation IS NULL OR p_lease_generation < 1 THEN
    RAISE EXCEPTION 'lease required' USING ERRCODE = 'OO010';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM jobs.tasks AS task
    WHERE task.id = p_lease
      AND task.state = 'running'
      AND task.generation = p_lease_generation
      AND task.lease_until > clock_timestamp()
  ) THEN
    RAISE EXCEPTION 'lease required' USING ERRCODE = 'OO010';
  END IF;
  PERFORM pg_advisory_xact_lock(hashtext('oo-source-instance'), hashtext(p_source));
  SELECT existing.*
    INTO v_existing
  FROM _oo.source_instance AS existing
  WHERE existing.source = p_source
    AND existing.measured_id = p_measured_id
  FOR UPDATE;
  IF FOUND THEN
    IF v_existing.config_digest IS DISTINCT FROM p_config_digest
       OR v_existing.schema_digest IS DISTINCT FROM p_schema_digest
       OR v_existing.shard_id IS DISTINCT FROM p_shard
       OR v_existing.owner_token IS DISTINCT FROM p_owner
       OR v_existing.engine IS DISTINCT FROM p_engine
    THEN
      RAISE EXCEPTION 'source instance context conflict' USING ERRCODE = 'OO010';
    END IF;
    RETURN jsonb_build_object(
      'protocol', 'oo.pipeline.native-source-instance/v1',
      'source', v_existing.source,
      'engine', v_existing.engine,
      'measured_id', v_existing.measured_id,
      'source_epoch', v_existing.source_epoch,
      'config_digest', v_existing.config_digest,
      'schema_digest', v_existing.schema_digest,
      'shard_id', v_existing.shard_id,
      'owner_token', v_existing.owner_token,
      'lease_id', v_existing.lease_id,
      'replay', true
    );
  END IF;
  SELECT existing.source_epoch + 1
    INTO v_epoch
  FROM _oo.source_instance AS existing
  WHERE existing.source = p_source
  ORDER BY existing.source_epoch DESC
  LIMIT 1;
  IF v_epoch IS NULL THEN
    v_epoch := 1;
  END IF;
  INSERT INTO _oo.source_instance (
    source, source_epoch, engine, measured_id, config_digest, schema_digest,
    shard_id, owner_token, lease_id, lease_generation
  ) VALUES (
    p_source, v_epoch, p_engine, p_measured_id, p_config_digest, p_schema_digest,
    p_shard, p_owner, p_lease, p_lease_generation
  );
  RETURN jsonb_build_object(
    'protocol', 'oo.pipeline.native-source-instance/v1',
    'source', p_source,
    'engine', p_engine,
    'measured_id', p_measured_id,
    'source_epoch', v_epoch,
    'config_digest', p_config_digest,
    'schema_digest', p_schema_digest,
    'shard_id', p_shard,
    'owner_token', p_owner,
    'lease_id', p_lease,
    'replay', false
  );
END;
$gtid_register$;

CREATE FUNCTION _oo.native_gtid_transaction_read(
  p_source text,
  p_codec text,
  p_lineage text,
  p_epoch integer,
  p_pos numeric
)
RETURNS jsonb
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $$
  SELECT jsonb_build_object(
    'source', row_in.source,
    'position_codec', row_in.position_codec,
    'position_lineage', row_in.position_lineage,
    'source_epoch', row_in.source_epoch,
    'applied_pos', to_jsonb(_oo.native_pos_text(row_in.applied_pos)),
    'txn_id', row_in.txn_id,
    'scope_digest', row_in.scope_digest,
    'job_task_id', to_jsonb(row_in.job_task_id::text),
    'target_generation', row_in.target_generation,
    'entries', row_in.entries
  )
  FROM _oo.native_gtid_transaction AS row_in
  WHERE row_in.source = p_source
    AND row_in.position_codec = p_codec
    AND row_in.position_lineage = p_lineage
    AND row_in.source_epoch = p_epoch
    AND row_in.applied_pos = p_pos;
$$;

CREATE FUNCTION _oo.native_gtid_transaction_commit(
  p_shard text,
  p_owner uuid,
  p_generation bigint,
  p_source text,
  p_codec text,
  p_lineage text,
  p_epoch integer,
  p_pos numeric,
  p_txn_id text,
  p_scope_digest text,
  p_entries jsonb,
  p_job uuid,
  p_group_generation bigint
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $gtid_commit$
DECLARE
  v_inst _oo.source_instance%ROWTYPE;
  v_wm _oo.source_watermark%ROWTYPE;
  v_saved _oo.native_gtid_transaction%ROWTYPE;
  v_pos text;
  v_entry jsonb;
  v_index integer;
  v_prev_key text := '';
  v_schema text;
  v_table text;
  v_generation bigint;
  v_batch text;
  v_digest text;
  v_physical text;
  v_run uuid;
  v_target bigint;
  v_config text;
  v_schema_digest text;
BEGIN
  v_pos := _oo.native_pos_text(p_pos);
  IF p_scope_digest IS NULL OR p_scope_digest !~ '^[0-9a-f]{64}$' OR p_job IS NULL OR p_group_generation IS NULL THEN
    RAISE EXCEPTION 'gtid identity' USING ERRCODE = 'OO010';
  END IF;
  IF p_txn_id IS NULL OR position(',' in p_txn_id) > 0 OR position('/' in p_txn_id) > 0 OR position(',' in COALESCE(p_lineage, '')) > 0 THEN
    RAISE EXCEPTION 'gtid identity' USING ERRCODE = 'OO010';
  END IF;
  IF NOT _oo.native_position_closed(p_codec, p_lineage, p_pos, p_epoch) OR p_epoch IS NULL THEN
    RAISE EXCEPTION 'gtid identity' USING ERRCODE = 'OO010';
  END IF;
  IF p_codec = 'mariadb-gtid/v1' AND p_txn_id IS DISTINCT FROM substring(p_lineage from '^d([0-9]+)') || '-' || substring(p_lineage from '\.s([0-9]+)$') || '-' || v_pos THEN
    RAISE EXCEPTION 'gtid identity' USING ERRCODE = 'OO010';
  END IF;
  IF p_codec = 'mysql-gtid/v1' AND p_txn_id IS DISTINCT FROM substring(p_lineage from '^mysql\.(.+)$') || ':' || v_pos THEN
    RAISE EXCEPTION 'gtid identity' USING ERRCODE = 'OO010';
  END IF;
  IF p_codec NOT IN ('mariadb-gtid/v1', 'mysql-gtid/v1') THEN
    RAISE EXCEPTION 'gtid identity' USING ERRCODE = 'OO010';
  END IF;
  IF p_entries IS NULL OR jsonb_typeof(p_entries) <> 'array' OR jsonb_array_length(p_entries) < 1 OR jsonb_array_length(p_entries) > 32 THEN
    RAISE EXCEPTION 'gtid identity' USING ERRCODE = 'OO010';
  END IF;
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_generation);
  SELECT existing.*
    INTO v_inst
  FROM _oo.source_instance AS existing
  WHERE existing.source = p_source
    AND existing.source_epoch = p_epoch;
  IF NOT FOUND OR v_inst.measured_id IS DISTINCT FROM p_lineage OR v_inst.shard_id IS DISTINCT FROM p_shard OR v_inst.owner_token IS DISTINCT FROM p_owner THEN
    RAISE EXCEPTION 'wrong source instance' USING ERRCODE = 'OO010';
  END IF;
  IF (v_inst.engine = 'mariadb' AND p_codec <> 'mariadb-gtid/v1') OR (v_inst.engine = 'mysql' AND p_codec <> 'mysql-gtid/v1') OR v_inst.engine NOT IN ('mariadb', 'mysql') THEN
    RAISE EXCEPTION 'wrong source instance' USING ERRCODE = 'OO010';
  END IF;
  SELECT wm.*
    INTO v_wm
  FROM _oo.source_watermark AS wm
  WHERE wm.source = p_source
  FOR UPDATE;
  IF NOT FOUND
     OR v_wm.position_codec IS DISTINCT FROM p_codec
     OR v_wm.position_lineage IS DISTINCT FROM p_lineage
     OR v_wm.applied_pos IS DISTINCT FROM p_pos
     OR v_wm.target_generation IS DISTINCT FROM p_generation
     OR v_wm.source_generation IS DISTINCT FROM p_group_generation
     OR v_wm.consistency IS DISTINCT FROM 'transactional'
     OR v_wm.consistency_grade IS DISTINCT FROM 'T'
     OR NOT v_wm.cross_table_consistent
  THEN
    RAISE EXCEPTION 'incomparable gtid position' USING ERRCODE = 'OO010';
  END IF;
  IF v_wm.source_epoch IS NOT NULL AND p_epoch < v_wm.source_epoch THEN
    RAISE EXCEPTION 'source epoch rollback' USING ERRCODE = 'OO010';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM jobs.tasks AS task
    WHERE task.id = p_job
      AND task.state = 'running'
      AND task.lease_until > clock_timestamp()
  ) THEN
    RAISE EXCEPTION 'lease required' USING ERRCODE = 'OO010';
  END IF;
  FOR v_index IN 0..jsonb_array_length(p_entries) - 1 LOOP
    v_entry := p_entries -> v_index;
    v_schema := v_entry->>'logical_schema';
    v_table := v_entry->>'logical_table';
    v_generation := (v_entry->>'source_generation')::bigint;
    v_batch := v_entry->>'batch_id';
    v_digest := v_entry->>'payload_digest';
    v_physical := v_entry->>'physical_name';
    v_run := (v_entry->>'run_id')::uuid;
    v_target := (v_entry->>'target_generation')::bigint;
    IF v_schema IS NULL OR v_table IS NULL OR v_generation IS NULL OR v_batch IS NULL OR v_digest IS NULL OR v_physical IS NULL OR v_run IS NULL OR v_target IS DISTINCT FROM p_generation THEN
      RAISE EXCEPTION 'gtid identity' USING ERRCODE = 'OO010';
    END IF;
    IF v_schema || '.' || v_table <= v_prev_key THEN
      RAISE EXCEPTION 'gtid identity' USING ERRCODE = 'OO010';
    END IF;
    v_prev_key := v_schema || '.' || v_table;
    SELECT receipt.config_digest, receipt.schema_digest
      INTO v_config, v_schema_digest
    FROM _oo.apply_receipts AS receipt
    WHERE receipt.source = p_source
      AND receipt.table_generation = v_physical
      AND receipt.txn_or_batch_id = v_batch
      AND receipt.payload_digest = v_digest
      AND receipt.txn_complete
      AND receipt.fence_shard = p_shard
      AND receipt.fence_generation = p_generation;
    IF NOT FOUND OR v_config IS DISTINCT FROM v_inst.config_digest OR v_schema_digest IS DISTINCT FROM v_inst.schema_digest THEN
      RAISE EXCEPTION 'applied coverage is absent' USING ERRCODE = 'OO030';
    END IF;
    IF NOT EXISTS (
      SELECT 1
      FROM _oo.update_batch AS batch
      WHERE batch.source = p_source
        AND batch.schema_name = v_schema
        AND batch.table_name = v_table
        AND batch.batch_id = v_batch
        AND batch.payload_digest = v_digest
        AND batch.run_id = v_run
        AND batch.physical_name = v_physical
        AND batch.target_generation = p_generation
        AND batch.source_generation = v_generation
    ) OR NOT EXISTS (
      SELECT 1
      FROM _oo.table_freshness AS fresh
      WHERE fresh.source = p_source
        AND fresh.schema_name = v_schema
        AND fresh.table_name = v_table
        AND fresh.source_generation = v_generation
        AND fresh.target_generation = p_generation
        AND fresh.physical_name = v_physical
        AND fresh.last_pos = p_pos
        AND fresh.state = 'awaiting_publish'
    ) OR NOT EXISTS (
      SELECT 1
      FROM _oo.table_quality AS quality
      WHERE quality.schema_name = v_schema
        AND quality.table_name = v_table
        AND quality.capture_method = 'c1_cdc'
        AND quality.delete_detection = 'native'
    ) OR NOT EXISTS (
      SELECT 1
      FROM _oo.full_import_run AS run_row
      JOIN _oo.full_import_table AS table_row ON table_row.run_id = run_row.run_id
      WHERE run_row.run_id = v_run
        AND run_row.state = 'awaiting_publish'
        AND run_row.source_id = p_source
        AND run_row.source_generation = v_generation
        AND run_row.target_generation = p_generation
        AND run_row.shard_id = p_shard
        AND run_row.owner_token = p_owner
        AND run_row.config_digest = v_inst.config_digest
        AND run_row.schema_digest = v_inst.schema_digest
        AND table_row.schema_ident = v_schema
        AND table_row.table_ident = v_table
        AND table_row.physical_name = v_physical
        AND table_row.exhausted
    ) THEN
      RAISE EXCEPTION 'applied coverage is absent' USING ERRCODE = 'OO030';
    END IF;
  END LOOP;
  UPDATE _oo.source_watermark
  SET source_epoch = p_epoch
  WHERE source = p_source
    AND position_codec = p_codec
    AND position_lineage = p_lineage
    AND applied_pos = p_pos
    AND (source_epoch IS NULL OR source_epoch = p_epoch OR source_epoch < p_epoch);
  IF NOT FOUND THEN
    RAISE EXCEPTION 'source epoch rollback' USING ERRCODE = 'OO010';
  END IF;
  SELECT saved.*
    INTO v_saved
  FROM _oo.native_gtid_transaction AS saved
  WHERE saved.source = p_source
    AND saved.position_codec = p_codec
    AND saved.position_lineage = p_lineage
    AND saved.source_epoch = p_epoch
    AND saved.applied_pos = p_pos
  FOR UPDATE;
  IF FOUND THEN
    IF v_saved.txn_id = p_txn_id
       AND v_saved.scope_digest = p_scope_digest
       AND v_saved.job_task_id = p_job
       AND v_saved.target_generation = p_generation
       AND v_saved.group_generation = p_group_generation
       AND v_saved.shard_id = p_shard
       AND v_saved.owner_token = p_owner
       AND v_saved.entries = p_entries
    THEN
      RETURN;
    END IF;
    RAISE EXCEPTION 'apply receipt identity conflict' USING ERRCODE = 'OO010';
  END IF;
  INSERT INTO _oo.native_gtid_transaction (
    source, position_codec, position_lineage, source_epoch, applied_pos, txn_id,
    scope_digest, job_task_id, target_generation, group_generation, shard_id, owner_token, entries
  ) VALUES (
    p_source, p_codec, p_lineage, p_epoch, p_pos, p_txn_id,
    p_scope_digest, p_job, p_generation, p_group_generation, p_shard, p_owner, p_entries
  );
END;
$gtid_commit$;

CREATE FUNCTION _oo.native_gtid_transaction_ack(
  p_source text,
  p_codec text,
  p_lineage text,
  p_epoch integer,
  p_pos numeric,
  p_job uuid
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $gtid_ack$
DECLARE
  v_saved _oo.native_gtid_transaction%ROWTYPE;
BEGIN
  SELECT saved.*
    INTO v_saved
  FROM _oo.native_gtid_transaction AS saved
  WHERE saved.source = p_source
    AND saved.position_codec = p_codec
    AND saved.position_lineage = p_lineage
    AND saved.source_epoch = p_epoch
    AND saved.applied_pos = p_pos
  FOR UPDATE;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'gtid attestation absent' USING ERRCODE = 'OO010';
  END IF;
  IF v_saved.job_task_id IS DISTINCT FROM p_job THEN
    RAISE EXCEPTION 'apply receipt identity conflict' USING ERRCODE = 'OO010';
  END IF;
  IF v_saved.acknowledged_at IS NULL THEN
    UPDATE _oo.native_gtid_transaction
    SET acknowledged_at = clock_timestamp()
    WHERE source = p_source
      AND position_codec = p_codec
      AND position_lineage = p_lineage
      AND source_epoch = p_epoch
      AND applied_pos = p_pos
      AND job_task_id = p_job
      AND acknowledged_at IS NULL;
  END IF;
  RETURN _oo.native_gtid_transaction_read(p_source, p_codec, p_lineage, p_epoch, p_pos) || jsonb_build_object('ack_allowed', true);
END;
$gtid_ack$;

REVOKE ALL ON FUNCTION _oo.native_pos_text(numeric) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.native_position_closed(text, text, numeric, integer) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.native_source_instance_register(text, uuid, bigint, text, text, text, text, text, uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.native_gtid_transaction_read(text, text, text, integer, numeric) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.native_gtid_transaction_commit(text, uuid, bigint, text, text, text, integer, numeric, text, text, jsonb, uuid, bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.native_gtid_transaction_ack(text, text, text, integer, numeric, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_pos_text(numeric) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_position_closed(text, text, numeric, integer) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_source_instance_register(text, uuid, bigint, text, text, text, text, text, uuid) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_gtid_transaction_read(text, text, text, integer, numeric) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_gtid_transaction_commit(text, uuid, bigint, text, text, text, integer, numeric, text, text, jsonb, uuid, bigint) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_gtid_transaction_ack(text, text, text, integer, numeric, uuid) TO oo_pipeline_runtime;


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
  IF NOT _oo.native_position_closed(p_position_codec, p_position_lineage, p_applied_pos, p_source_epoch) THEN
    RAISE EXCEPTION 'closed position codec' USING ERRCODE = 'OO015';
  END IF;
  IF p_position_codec = 'mongo-cluster-time/v1' AND p_cross_table_consistent THEN
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


CREATE OR REPLACE FUNCTION _oo.native_update_seal_quality(
  p_shard text,
  p_owner uuid,
  p_target_generation bigint,
  p_run uuid,
  p_source text,
  p_source_generation bigint,
  p_schema text,
  p_table text,
  p_physical text,
  p_batch_id text,
  p_payload_digest text,
  p_applied_pos numeric,
  p_position_codec text,
  p_position_lineage text,
  p_capture_method text,
  p_delete_detection text,
  p_consistency_grade text,
  p_cross_table boolean,
  p_tombstones text[],
  p_row_count bigint,
  p_reconcile text,
  p_reconcile_diff bigint,
  p_mode text
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $nu$
DECLARE
  v_consistency text;
  v_index integer;
  v_existing text;
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_target_generation);
  PERFORM _oo._require_digest(p_payload_digest, 'payload_digest');
  IF NOT _oo.native_position_closed(p_position_codec, p_position_lineage, p_applied_pos, 0) THEN
    RAISE EXCEPTION 'closed position codec' USING ERRCODE = '23514';
  END IF;
  IF p_position_lineage IS NULL OR p_position_lineage !~ '^[A-Za-z0-9_.:-]{1,128}$' THEN
    RAISE EXCEPTION 'closed position lineage' USING ERRCODE = '23514';
  END IF;
  IF p_capture_method IS NULL OR p_capture_method NOT IN (
    'c1_cdc', 'c2_change_feed', 'c3_cursor', 'c4_partition', 'c5_diff'
  ) THEN
    RAISE EXCEPTION 'closed capture method' USING ERRCODE = '23514';
  END IF;
  IF p_delete_detection IS NULL OR p_delete_detection NOT IN (
    'native', 'partition_replace', 'reconcile', 'none'
  ) THEN
    RAISE EXCEPTION 'closed delete detection' USING ERRCODE = '23514';
  END IF;
  IF p_consistency_grade IS NULL OR p_consistency_grade NOT IN ('T', 'S1', 'S0', 'B') THEN
    RAISE EXCEPTION 'closed consistency grade' USING ERRCODE = '23514';
  END IF;
  IF p_mode IS NULL OR p_mode NOT IN ('full', 'incremental') THEN
    RAISE EXCEPTION 'closed freshness mode' USING ERRCODE = '23514';
  END IF;
  IF p_reconcile IS NULL OR p_reconcile NOT IN ('ok', 'never') THEN
    RAISE EXCEPTION 'closed reconcile status' USING ERRCODE = '23514';
  END IF;
  IF p_consistency_grade = 'T' THEN
    v_consistency := 'transactional';
  ELSE
    v_consistency := 'per_table';
    IF p_cross_table THEN
      RAISE EXCEPTION 'closed consistency grade' USING ERRCODE = '23514';
    END IF;
  END IF;
  IF p_position_codec = 'mongo-cluster-time/v1' AND (
    p_consistency_grade NOT IN ('S0', 'B') OR p_cross_table OR p_capture_method IS DISTINCT FROM 'c1_cdc' OR p_delete_detection IS DISTINCT FROM 'native'
  ) THEN
    RAISE EXCEPTION 'closed consistency grade' USING ERRCODE = '23514';
  END IF;
  IF p_position_codec IN ('mariadb-gtid/v1', 'mysql-gtid/v1') AND (
    p_consistency_grade IS DISTINCT FROM 'T' OR NOT p_cross_table OR p_capture_method IS DISTINCT FROM 'c1_cdc' OR p_delete_detection IS DISTINCT FROM 'native'
  ) THEN
    RAISE EXCEPTION 'closed consistency grade' USING ERRCODE = '23514';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM _oo.apply_receipts AS receipt
    WHERE receipt.source = p_source
      AND receipt.table_generation = p_physical
      AND receipt.txn_or_batch_id = p_batch_id
      AND receipt.payload_digest = p_payload_digest
  ) THEN
    RAISE EXCEPTION 'applied coverage is absent' USING ERRCODE = 'OO030';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM _oo.full_import_run AS run_row
    WHERE run_row.run_id = p_run
      AND run_row.state = 'open'
      AND run_row.shard_id = p_shard
      AND run_row.owner_token = p_owner
      AND run_row.target_generation = p_target_generation
      AND run_row.source_id = p_source
      AND run_row.source_generation = p_source_generation
  ) THEN
    RAISE EXCEPTION 'full import run is not open' USING ERRCODE = 'OO031';
  END IF;
  INSERT INTO _oo.update_batch (
    source, schema_name, table_name, batch_id, payload_digest, run_id,
    target_generation, source_generation, physical_name, tombstone_count, row_count
  ) VALUES (
    p_source, p_schema, p_table, p_batch_id, p_payload_digest, p_run,
    p_target_generation, p_source_generation, p_physical,
    COALESCE(cardinality(p_tombstones), 0), p_row_count
  )
  ON CONFLICT (source, schema_name, table_name, batch_id) DO NOTHING;
  IF NOT FOUND THEN
    SELECT existing.payload_digest
      INTO v_existing
    FROM _oo.update_batch AS existing
    WHERE existing.source = p_source
      AND existing.schema_name = p_schema
      AND existing.table_name = p_table
      AND existing.batch_id = p_batch_id;
    IF v_existing IS DISTINCT FROM p_payload_digest THEN
      RAISE EXCEPTION 'apply receipt identity conflict' USING ERRCODE = 'OO010';
    END IF;
  END IF;
  IF p_tombstones IS NOT NULL THEN
    FOR v_index IN 1..cardinality(p_tombstones) LOOP
      INSERT INTO _oo.update_tombstone (
        source, schema_name, table_name, source_generation, key_text, batch_id
      ) VALUES (
        p_source, p_schema, p_table, p_source_generation, p_tombstones[v_index], p_batch_id
      )
      ON CONFLICT (source, schema_name, table_name, source_generation, key_text) DO NOTHING;
    END LOOP;
  END IF;
  INSERT INTO _oo.table_freshness (
    schema_name, table_name, source, source_generation, target_generation, physical_name,
    last_pos, source_committed_at, applied_at, mode, state
  ) VALUES (
    p_schema, p_table, p_source, p_source_generation, p_target_generation, p_physical,
    p_applied_pos, NULL, clock_timestamp(), p_mode, 'awaiting_publish'
  )
  ON CONFLICT (schema_name, table_name) DO UPDATE
    SET source = EXCLUDED.source,
        source_generation = EXCLUDED.source_generation,
        target_generation = EXCLUDED.target_generation,
        physical_name = EXCLUDED.physical_name,
        last_pos = EXCLUDED.last_pos,
        source_committed_at = EXCLUDED.source_committed_at,
        applied_at = EXCLUDED.applied_at,
        mode = EXCLUDED.mode,
        state = EXCLUDED.state;
  INSERT INTO _oo.source_watermark (
    source, consistency, consistency_grade, applied_pos, source_committed_at, applied_at,
    state, cross_table_consistent, source_generation, target_generation,
    position_codec, position_lineage
  ) VALUES (
    p_source, v_consistency, p_consistency_grade, p_applied_pos, NULL, clock_timestamp(),
    'lagging', p_cross_table, p_source_generation, p_target_generation,
    p_position_codec, p_position_lineage
  )
  ON CONFLICT (source) DO UPDATE
    SET consistency = EXCLUDED.consistency,
        consistency_grade = EXCLUDED.consistency_grade,
        applied_pos = EXCLUDED.applied_pos,
        source_committed_at = EXCLUDED.source_committed_at,
        applied_at = EXCLUDED.applied_at,
        state = EXCLUDED.state,
        cross_table_consistent = EXCLUDED.cross_table_consistent,
        source_generation = EXCLUDED.source_generation,
        target_generation = EXCLUDED.target_generation,
        position_codec = EXCLUDED.position_codec,
        position_lineage = EXCLUDED.position_lineage;
  INSERT INTO _oo.table_quality (
    schema_name, table_name, rejects_open, rejects_by_column, reconcile_status, reconcile_at,
    reconcile_diff_rows, unexplained_codes, hidden_columns, partial_txn_in_progress,
    entity_check, capture_method, delete_detection, position_state, budget_state, updated_at
  ) VALUES (
    p_schema, p_table, 0, '{}'::jsonb, p_reconcile,
    CASE WHEN p_reconcile = 'ok' THEN clock_timestamp() ELSE NULL END,
    p_reconcile_diff, '{}'::jsonb, 0, false,
    'n/a', p_capture_method, p_delete_detection, 'ok', 'ok', clock_timestamp()
  )
  ON CONFLICT (schema_name, table_name) DO UPDATE
    SET rejects_open = EXCLUDED.rejects_open,
        rejects_by_column = EXCLUDED.rejects_by_column,
        reconcile_status = EXCLUDED.reconcile_status,
        reconcile_at = EXCLUDED.reconcile_at,
        reconcile_diff_rows = EXCLUDED.reconcile_diff_rows,
        unexplained_codes = EXCLUDED.unexplained_codes,
        hidden_columns = EXCLUDED.hidden_columns,
        partial_txn_in_progress = EXCLUDED.partial_txn_in_progress,
        entity_check = EXCLUDED.entity_check,
        capture_method = EXCLUDED.capture_method,
        delete_detection = EXCLUDED.delete_detection,
        position_state = EXCLUDED.position_state,
        budget_state = EXCLUDED.budget_state,
        updated_at = EXCLUDED.updated_at;
END;
$nu$;

