-- C4 source signatures are opaque. Partition state is authoritative; the
-- numeric watermark column is NULL for this codec.
ALTER TABLE _oo.source_watermark ALTER COLUMN applied_pos DROP NOT NULL;
ALTER TABLE _oo.source_watermark DROP CONSTRAINT source_watermark_codec_check;
ALTER TABLE _oo.source_watermark ADD CONSTRAINT source_watermark_codec_check CHECK (
  position_codec IN ('pg-commit-lsn/v1', 'mysql-binlog-pos/v1',
    'batch-cursor-us/v1', 'snapshot-zero/v1', 'partition-signature/v1')
);
ALTER TABLE _oo.source_watermark ADD CONSTRAINT source_watermark_position_check CHECK (
  (position_codec = 'partition-signature/v1' AND applied_pos IS NULL)
  OR (position_codec <> 'partition-signature/v1' AND applied_pos IS NOT NULL)
);

CREATE TABLE _oo.native_partition_state (
  source text NOT NULL,
  schema_name text NOT NULL,
  table_name text NOT NULL,
  partition_id text NOT NULL,
  table_epoch text NOT NULL,
  schema_fingerprint text NOT NULL,
  current_signature text,
  previous_ack_exists boolean NOT NULL,
  previous_signature text,
  partition_scope jsonb NOT NULL,
  audit_digest text NOT NULL,
  consistency_grade text NOT NULL,
  observed_partition_ids text[] NOT NULL,
  key_texts text[] NOT NULL,
  row_count bigint NOT NULL,
  source_generation bigint NOT NULL,
  target_generation bigint NOT NULL,
  run_id uuid NOT NULL,
  batch_id text NOT NULL,
  payload_digest text NOT NULL,
  job_task_id uuid NOT NULL,
  job_generation bigint NOT NULL,
  acknowledged boolean NOT NULL DEFAULT false,
  PRIMARY KEY (source, schema_name, table_name, partition_id),
  CONSTRAINT native_partition_rows CHECK (row_count >= 0 AND row_count = cardinality(key_texts)),
  CONSTRAINT native_partition_digest CHECK (payload_digest ~ '^[0-9a-f]{64}$'
    AND audit_digest ~ '^[0-9a-f]{64}$'),
  CONSTRAINT native_partition_grade CHECK (consistency_grade IN ('T', 'S1', 'S0', 'B')),
  CONSTRAINT native_partition_job_generation CHECK (job_generation >= 1)
);
REVOKE ALL ON _oo.native_partition_state FROM PUBLIC, oo_pipeline_runtime;

CREATE FUNCTION _oo.native_partition_guard(
  p_shard text, p_owner uuid, p_target_generation bigint,
  p_source text, p_schema text, p_table text,
  p_source_generation bigint, p_parent_generation bigint,
  p_scope jsonb, p_previous_ack_exists boolean, p_previous_signature text,
  p_new_keys text[]
) RETURNS text[] LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $np$
DECLARE
  v_prior _oo.native_partition_state%ROWTYPE;
  v_watermark bigint;
  v_keys text[] := ARRAY[]::text[];
  v_partition text := p_scope->>'partition_id';
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_target_generation);
  IF p_scope IS NULL OR jsonb_typeof(p_scope) <> 'object'
     OR p_scope->>'source_id' IS DISTINCT FROM p_source
     OR (p_scope->>'source_generation')::bigint IS DISTINCT FROM p_source_generation
     OR p_scope->>'logical_schema' IS DISTINCT FROM p_schema
     OR p_scope->>'logical_table' IS DISTINCT FROM p_table
     OR v_partition IS NULL OR v_partition = ''
     OR COALESCE(p_scope->>'table_epoch', '') = ''
     OR COALESCE(p_scope->>'schema_fingerprint', '') = ''
     OR p_scope->>'complete' IS DISTINCT FROM 'true'
     OR p_scope->>'mutations_pending' IS DISTINCT FROM 'false'
     OR p_new_keys IS NULL OR cardinality(p_new_keys) > 1000000
     OR p_scope->>'row_count' IS NULL
     OR cardinality(p_new_keys) IS DISTINCT FROM (p_scope->>'row_count')::bigint
     OR (cardinality(p_new_keys) > 0 AND COALESCE(p_scope->>'current_signature', '') = '')
     OR EXISTS (SELECT 1 FROM unnest(p_new_keys) AS key_text WHERE key_text IS NULL)
     OR cardinality(p_new_keys) <> (SELECT count(DISTINCT key_text) FROM unnest(p_new_keys) AS key_text)
  THEN RAISE EXCEPTION 'partition scope is closed' USING ERRCODE = '23514'; END IF;
  PERFORM pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended(
    p_source || chr(31) || p_schema || chr(31) || p_table, 0));
  SELECT watermark.source_generation INTO v_watermark
  FROM _oo.source_watermark AS watermark WHERE watermark.source = p_source FOR UPDATE;
  IF (p_parent_generation IS NULL AND FOUND)
     OR (p_parent_generation IS NOT NULL AND (NOT FOUND OR v_watermark <> p_parent_generation))
     OR p_source_generation <> COALESCE(p_parent_generation, 0) + 1
  THEN RAISE EXCEPTION 'partition source generation is stale' USING ERRCODE = 'OO010'; END IF;
  SELECT state.* INTO v_prior FROM _oo.native_partition_state AS state
  WHERE state.source = p_source AND state.schema_name = p_schema
    AND state.table_name = p_table AND state.partition_id = v_partition FOR UPDATE;
  IF FOUND IS DISTINCT FROM p_previous_ack_exists THEN
    RAISE EXCEPTION 'partition prior ACK mismatch' USING ERRCODE = 'OO010';
  END IF;
  IF FOUND THEN
    IF NOT v_prior.acknowledged
       OR v_prior.current_signature IS DISTINCT FROM p_previous_signature
       OR v_prior.table_epoch IS DISTINCT FROM p_scope->>'table_epoch'
       OR v_prior.schema_fingerprint IS DISTINCT FROM p_scope->>'schema_fingerprint'
       OR v_prior.partition_scope->>'schema_digest' IS DISTINCT FROM p_scope->>'schema_digest'
       OR v_prior.partition_scope->>'type_digest' IS DISTINCT FROM p_scope->>'type_digest'
    THEN RAISE EXCEPTION 'partition prior ACK mismatch' USING ERRCODE = 'OO010'; END IF;
    v_keys := v_prior.key_texts;
  ELSIF p_previous_signature IS NOT NULL THEN
    RAISE EXCEPTION 'partition prior ACK mismatch' USING ERRCODE = 'OO010';
  END IF;
  IF EXISTS (SELECT 1 FROM _oo.native_partition_state AS state
    WHERE state.source = p_source AND state.schema_name = p_schema
      AND state.table_name = p_table AND NOT state.acknowledged)
  THEN RAISE EXCEPTION 'partition prior ACK mismatch' USING ERRCODE = 'OO010'; END IF;
  IF EXISTS (SELECT 1 FROM _oo.native_partition_state AS state
    WHERE state.source = p_source AND state.schema_name = p_schema AND state.table_name = p_table
      AND state.partition_id <> v_partition
      AND (state.table_epoch IS DISTINCT FROM p_scope->>'table_epoch'
        OR state.schema_fingerprint IS DISTINCT FROM p_scope->>'schema_fingerprint'
        OR state.partition_scope->>'schema_digest' IS DISTINCT FROM p_scope->>'schema_digest'
        OR state.partition_scope->>'type_digest' IS DISTINCT FROM p_scope->>'type_digest'
        OR state.key_texts && p_new_keys))
  THEN RAISE EXCEPTION 'partition epoch or key ownership conflict' USING ERRCODE = 'OO010'; END IF;
  RETURN v_keys;
END;
$np$;

CREATE FUNCTION _oo.native_partition_commit(
  p_shard text, p_owner uuid, p_target_generation bigint,
  p_source text, p_schema text, p_table text,
  p_scope jsonb, p_keys text[], p_run uuid, p_batch_id text,
  p_payload_digest text, p_previous_ack_exists boolean,
  p_previous_signature text, p_audit_digest text, p_consistency_grade text,
  p_job_task_id uuid, p_job_generation bigint
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $np$
DECLARE
  v_ids text[];
BEGIN
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_target_generation);
  PERFORM _oo._require_digest(p_payload_digest, 'payload_digest');
  PERFORM _oo._require_digest(p_audit_digest, 'audit_digest');
  SELECT array_agg(value ORDER BY value) INTO v_ids
  FROM jsonb_array_elements_text(p_scope->'observed_partition_ids') AS value;
  IF v_ids IS NULL OR cardinality(v_ids) > 1024 OR p_keys IS NULL
     OR cardinality(p_keys) IS DISTINCT FROM (p_scope->>'row_count')::bigint
     OR p_job_task_id IS NULL OR p_job_generation IS NULL OR p_job_generation < 1
     OR NOT EXISTS (SELECT 1 FROM _oo.update_batch AS batch
       WHERE batch.source = p_source AND batch.schema_name = p_schema
         AND batch.table_name = p_table AND batch.batch_id = p_batch_id
         AND batch.run_id = p_run AND batch.payload_digest = p_payload_digest)
  THEN RAISE EXCEPTION 'partition receipt is absent' USING ERRCODE = 'OO030'; END IF;
  INSERT INTO _oo.native_partition_state (
    source, schema_name, table_name, partition_id, table_epoch, schema_fingerprint,
    current_signature, previous_ack_exists, previous_signature,
    partition_scope, audit_digest, consistency_grade,
    observed_partition_ids, key_texts, row_count,
    source_generation, target_generation, run_id, batch_id, payload_digest,
    job_task_id, job_generation, acknowledged
  ) VALUES (
    p_source, p_schema, p_table, p_scope->>'partition_id', p_scope->>'table_epoch',
    p_scope->>'schema_fingerprint', p_scope->>'current_signature',
    p_previous_ack_exists, p_previous_signature, p_scope, p_audit_digest,
    p_consistency_grade, v_ids, p_keys,
    cardinality(p_keys), (p_scope->>'source_generation')::bigint, p_target_generation,
    p_run, p_batch_id, p_payload_digest, p_job_task_id, p_job_generation, false
  ) ON CONFLICT (source, schema_name, table_name, partition_id) DO UPDATE SET
    current_signature = EXCLUDED.current_signature,
    previous_ack_exists = EXCLUDED.previous_ack_exists,
    previous_signature = EXCLUDED.previous_signature,
    partition_scope = EXCLUDED.partition_scope,
    audit_digest = EXCLUDED.audit_digest,
    consistency_grade = EXCLUDED.consistency_grade,
    observed_partition_ids = EXCLUDED.observed_partition_ids,
    key_texts = EXCLUDED.key_texts,
    row_count = EXCLUDED.row_count,
    source_generation = EXCLUDED.source_generation,
    target_generation = EXCLUDED.target_generation,
    run_id = EXCLUDED.run_id,
    batch_id = EXCLUDED.batch_id,
    payload_digest = EXCLUDED.payload_digest,
    job_task_id = EXCLUDED.job_task_id,
    job_generation = EXCLUDED.job_generation,
    acknowledged = false;
  UPDATE _oo.source_watermark SET position_codec = 'partition-signature/v1',
    position_lineage = 'partition-map', applied_pos = NULL
  WHERE source = p_source AND source_generation = (p_scope->>'source_generation')::bigint;
  IF NOT FOUND THEN RAISE EXCEPTION 'partition watermark is absent' USING ERRCODE = 'OO030'; END IF;
  UPDATE _oo.table_freshness SET last_pos = NULL
  WHERE source = p_source AND schema_name = p_schema AND table_name = p_table
    AND source_generation = (p_scope->>'source_generation')::bigint;
END;
$np$;

-- A target seal is pending until the exact jobs.claim generation has been
-- completed. This function is idempotent so a worker can retry after losing
-- its connection between jobs.complete_success and this ACK.
CREATE FUNCTION _oo.native_partition_ack(
  p_source text, p_schema text, p_table text, p_partition text,
  p_run uuid, p_batch_id text, p_payload_digest text,
  p_job_task_id uuid, p_job_generation bigint
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $np$
DECLARE
  v_state _oo.native_partition_state%ROWTYPE;
BEGIN
  SELECT state.* INTO v_state FROM _oo.native_partition_state AS state
  WHERE state.source = p_source AND state.schema_name = p_schema
    AND state.table_name = p_table AND state.partition_id = p_partition FOR UPDATE;
  IF NOT FOUND OR v_state.run_id IS DISTINCT FROM p_run
     OR v_state.batch_id IS DISTINCT FROM p_batch_id
     OR v_state.payload_digest IS DISTINCT FROM p_payload_digest
     OR v_state.job_task_id IS DISTINCT FROM p_job_task_id
     OR v_state.job_generation IS DISTINCT FROM p_job_generation
  THEN RAISE EXCEPTION 'partition ACK identity mismatch' USING ERRCODE = 'OO010'; END IF;
  IF NOT v_state.acknowledged THEN
    UPDATE _oo.native_partition_state SET acknowledged = true
    WHERE source = p_source AND schema_name = p_schema AND table_name = p_table
      AND partition_id = p_partition;
  END IF;
  RETURN true;
END;
$np$;

-- A materialized table is complete only when every source-observed partition
-- has a target sidecar and their disjoint key counts equal the sealed table.
CREATE FUNCTION _oo.native_partition_vector(
  p_run uuid, p_source text, p_schema text, p_table text,
  p_expected text[], p_current_partition text, p_current_signature text,
  p_source_generation bigint, p_target_generation bigint
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, _oo AS $np$
DECLARE
  v_rows bigint;
  v_count bigint;
  v_ids text[];
  v_vector jsonb;
  v_current _oo.native_partition_state%ROWTYPE;
BEGIN
  IF p_expected IS NULL OR cardinality(p_expected) = 0 OR cardinality(p_expected) > 1024
     OR EXISTS (SELECT 1 FROM unnest(p_expected) AS id WHERE id IS NULL OR id = '')
     OR p_expected <> (SELECT array_agg(id ORDER BY id) FROM unnest(p_expected) AS id)
     OR cardinality(p_expected) <> (SELECT count(DISTINCT id) FROM unnest(p_expected) AS id)
  THEN RAISE EXCEPTION 'partition vector is incomplete' USING ERRCODE = 'OO030'; END IF;
  SELECT table_row.applied_rows INTO v_rows
  FROM _oo.full_import_table AS table_row
  JOIN _oo.full_import_run AS run_row ON run_row.run_id = table_row.run_id
  WHERE table_row.run_id = p_run AND table_row.schema_ident = p_schema
    AND table_row.table_ident = p_table AND run_row.source_id = p_source
    AND run_row.source_generation = p_source_generation
    AND run_row.target_generation = p_target_generation
    AND run_row.state = 'awaiting_publish';
  IF NOT FOUND THEN RAISE EXCEPTION 'sealed partition run is absent' USING ERRCODE = 'OO030'; END IF;
  SELECT state.* INTO v_current FROM _oo.native_partition_state AS state
  WHERE state.source = p_source AND state.schema_name = p_schema
    AND state.table_name = p_table AND state.partition_id = p_current_partition;
  IF NOT FOUND OR v_current.run_id <> p_run
     OR NOT v_current.acknowledged
     OR v_current.current_signature IS DISTINCT FROM p_current_signature
     OR v_current.source_generation <> p_source_generation
     OR v_current.target_generation <> p_target_generation
     OR v_current.observed_partition_ids <> p_expected
  THEN RAISE EXCEPTION 'current partition receipt is absent' USING ERRCODE = 'OO030'; END IF;
  SELECT count(*), array_agg(state.partition_id ORDER BY state.partition_id),
    COALESCE(sum(state.row_count), 0),
    jsonb_agg(jsonb_build_object(
      'partition_id', state.partition_id,
      'table_epoch', state.table_epoch,
      'schema_fingerprint', state.schema_fingerprint,
      'current_signature', state.current_signature,
      'previous_ack_exists', state.previous_ack_exists,
      'previous_signature', state.previous_signature,
      'partition_scope', state.partition_scope,
      'audit_digest', state.audit_digest,
      'consistency_grade', state.consistency_grade,
      'source_generation', state.source_generation,
      'target_generation', state.target_generation,
      'run_id', state.run_id,
      'batch_id', state.batch_id,
      'payload_digest', state.payload_digest,
      'row_count', state.row_count
    ) ORDER BY state.partition_id)
  INTO v_count, v_ids, v_rows, v_vector
  FROM _oo.native_partition_state AS state
  WHERE state.source = p_source AND state.schema_name = p_schema
    AND state.table_name = p_table;
  IF v_count <> cardinality(p_expected) OR v_ids <> p_expected
     OR v_rows <> (SELECT applied_rows FROM _oo.full_import_table
                   WHERE run_id = p_run AND schema_ident = p_schema AND table_ident = p_table)
     OR EXISTS (SELECT 1 FROM _oo.native_partition_state AS state
       WHERE state.source = p_source AND state.schema_name = p_schema AND state.table_name = p_table
         AND (state.table_epoch <> v_current.table_epoch
           OR state.schema_fingerprint <> v_current.schema_fingerprint
           OR state.partition_scope->>'schema_digest' IS DISTINCT FROM v_current.partition_scope->>'schema_digest'
           OR state.partition_scope->>'type_digest' IS DISTINCT FROM v_current.partition_scope->>'type_digest'
         OR state.observed_partition_ids <> p_expected))
     OR EXISTS (SELECT 1 FROM _oo.native_partition_state AS state
       WHERE state.source = p_source AND state.schema_name = p_schema
         AND state.table_name = p_table AND NOT state.acknowledged)
  THEN RAISE EXCEPTION 'partition vector is incomplete' USING ERRCODE = 'OO030'; END IF;
  RETURN v_vector;
END;
$np$;

REVOKE ALL ON FUNCTION _oo.native_partition_guard(text,uuid,bigint,text,text,text,bigint,bigint,jsonb,boolean,text,text[]) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.native_partition_commit(text,uuid,bigint,text,text,text,jsonb,text[],uuid,text,text,boolean,text,text,text,uuid,bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.native_partition_ack(text,text,text,text,uuid,text,text,uuid,bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION _oo.native_partition_vector(uuid,text,text,text,text[],text,text,bigint,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION _oo.native_partition_guard(text,uuid,bigint,text,text,text,bigint,bigint,jsonb,boolean,text,text[]) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_partition_commit(text,uuid,bigint,text,text,text,jsonb,text[],uuid,text,text,boolean,text,text,text,uuid,bigint) TO oo_pipeline_runtime;
GRANT EXECUTE ON FUNCTION _oo.native_partition_ack(text,text,text,text,uuid,text,text,uuid,bigint) TO oo_target_owner;
GRANT EXECUTE ON FUNCTION _oo.native_partition_vector(uuid,text,text,text,text[],text,text,bigint,bigint) TO oo_pipeline_runtime;
