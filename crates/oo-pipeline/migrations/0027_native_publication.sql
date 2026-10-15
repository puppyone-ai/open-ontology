-- native-publication/v1
-- Publish one sealed full-import run only while the coordinator holds its target fence.
-- The source owner receipt is checked by the authenticated release API; this function
-- rechecks the immutable target run/table/quality/freshness vector in the same txn
-- that opens grants. It never invents a cursor or upgrades per-table consistency.

DO $np$
BEGIN
  IF pg_catalog.to_regprocedure('_oo.full_import_assert_bound(text, uuid, bigint)') IS NULL
     OR pg_catalog.to_regclass('_oo.table_freshness') IS NULL
     OR pg_catalog.to_regclass('_oo.source_watermark') IS NULL THEN
    RAISE EXCEPTION 'native publication requires full-import and native-update schemas'
      USING ERRCODE = 'OO002';
  END IF;
END
$np$;

ALTER TABLE _oo.full_import_run DROP CONSTRAINT full_import_run_state_check;
ALTER TABLE _oo.full_import_run ADD CONSTRAINT full_import_run_state_check
  CHECK (state IN ('open', 'awaiting_publish', 'published', 'retired'));
ALTER TABLE _oo.full_import_run DROP CONSTRAINT full_import_run_seal_check;
ALTER TABLE _oo.full_import_run ADD CONSTRAINT full_import_run_seal_check CHECK (
  (state = 'open' AND sealed_at IS NULL AND retired_at IS NULL)
  OR (state IN ('awaiting_publish', 'published') AND sealed_at IS NOT NULL AND retired_at IS NULL)
  OR (state = 'retired' AND sealed_at IS NULL AND retired_at IS NOT NULL)
);
ALTER TABLE _oo.table_freshness DROP CONSTRAINT table_freshness_state_check;
ALTER TABLE _oo.table_freshness ADD CONSTRAINT table_freshness_state_check
  CHECK (state IN ('awaiting_publish', 'live'));
ALTER TABLE _oo.source_watermark DROP CONSTRAINT source_watermark_state_check;
ALTER TABLE _oo.source_watermark ADD CONSTRAINT source_watermark_state_check
  CHECK (state IN ('lagging', 'live'));

CREATE TABLE _oo.native_publication_receipt (
  run_id uuid PRIMARY KEY REFERENCES _oo.full_import_run(run_id),
  receipt_sha256 text NOT NULL CHECK (receipt_sha256 ~ '^[0-9a-f]{64}$'),
  source_id text NOT NULL,
  source_generation bigint NOT NULL,
  target_generation bigint NOT NULL,
  position_codec text NOT NULL,
  applied_pos numeric(38,0) NOT NULL,
  consistency text NOT NULL,
  consistency_grade text NOT NULL,
  cross_table_consistent boolean NOT NULL,
  table_vector jsonb NOT NULL,
  published_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
REVOKE ALL ON TABLE _oo.native_publication_receipt FROM PUBLIC, oo_pipeline_runtime;

CREATE FUNCTION _oo.native_publication_guard_receipt()
RETURNS trigger LANGUAGE plpgsql
SET search_path = pg_catalog, _oo
AS $np$
BEGIN
  RAISE EXCEPTION 'native publication receipt is append-only' USING ERRCODE = 'OO014';
END
$np$;
CREATE TRIGGER native_publication_receipt_guard
  BEFORE UPDATE OR DELETE OR TRUNCATE ON _oo.native_publication_receipt
  FOR EACH STATEMENT EXECUTE FUNCTION _oo.native_publication_guard_receipt();
REVOKE ALL ON FUNCTION _oo.native_publication_guard_receipt() FROM PUBLIC, oo_pipeline_runtime;

CREATE FUNCTION _oo.publish_native_full_import(
  p_run uuid,
  p_receipt_sha256 text,
  p_receipt jsonb
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, _oo
AS $np$
DECLARE
  v_run _oo.full_import_run%ROWTYPE;
  v_watermark _oo.source_watermark%ROWTYPE;
  v_existing _oo.native_publication_receipt%ROWTYPE;
  v_item jsonb;
  v_table _oo.full_import_table%ROWTYPE;
  v_quality record;
  v_count integer;
  v_seen integer := 0;
  v_updated integer;
  v_rows bigint;
  v_position numeric(38,0);
  v_schema text;
  v_table_name text;
  v_physical text;
  v_source text;
  v_source_generation bigint;
  v_target_generation bigint;
  v_codec text;
  v_consistency text;
  v_grade text;
  v_cross_table boolean;
BEGIN
  IF p_run IS NULL OR p_receipt_sha256 IS NULL
     OR p_receipt_sha256 !~ '^[0-9a-f]{64}$'
     OR jsonb_typeof(p_receipt) <> 'object'
     OR p_receipt->>'protocol' <> 'oo.pipeline.native-publication-receipt/v1'
     OR jsonb_typeof(p_receipt->'tables') <> 'array' THEN
    RAISE EXCEPTION 'invalid native publication receipt' USING ERRCODE = 'OO030';
  END IF;

  SELECT run_row.* INTO v_run
  FROM _oo.full_import_run AS run_row
  WHERE run_row.run_id = p_run FOR UPDATE;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'native full import run missing' USING ERRCODE = 'OO030';
  END IF;
  SELECT receipt_row.* INTO v_existing
  FROM _oo.native_publication_receipt AS receipt_row
  WHERE receipt_row.run_id = p_run;
  IF FOUND THEN
    IF v_existing.receipt_sha256 = p_receipt_sha256
       AND v_run.state = 'published' THEN
      RETURN jsonb_build_object('outcome','replay','run_id',p_run,
        'receipt_sha256',p_receipt_sha256,'published_at',v_existing.published_at);
    END IF;
    RAISE EXCEPTION 'native publication receipt conflicts with prior publication' USING ERRCODE = 'OO030';
  END IF;
  IF v_run.state <> 'awaiting_publish'
     OR p_receipt->>'source_id' IS DISTINCT FROM v_run.source_id
     OR (p_receipt->>'source_generation')::bigint IS DISTINCT FROM v_run.source_generation
     OR (p_receipt->>'target_generation')::bigint IS DISTINCT FROM v_run.target_generation THEN
    RAISE EXCEPTION 'native publication does not match sealed run' USING ERRCODE = 'OO030';
  END IF;

  v_source := v_run.source_id;
  v_source_generation := v_run.source_generation;
  v_target_generation := v_run.target_generation;
  v_codec := p_receipt->>'position_codec';
  v_position := (p_receipt->>'applied_pos')::numeric;
  v_consistency := p_receipt->>'consistency';
  v_grade := p_receipt->>'consistency_grade';
  v_cross_table := (p_receipt->>'cross_table_consistent')::boolean;
  IF v_codec IS NULL OR v_position IS NULL OR v_position < 0
     OR v_consistency NOT IN ('transactional','per_table')
     OR v_grade NOT IN ('T','S1','S0','B') THEN
    RAISE EXCEPTION 'native publication position or consistency is invalid' USING ERRCODE = 'OO030';
  END IF;

  SELECT count(*) INTO v_count FROM _oo.full_import_table WHERE run_id = p_run;
  IF v_count = 0 OR v_count <> jsonb_array_length(p_receipt->'tables') THEN
    RAISE EXCEPTION 'native publication table vector is incomplete' USING ERRCODE = 'OO030';
  END IF;
  FOR v_item IN SELECT value FROM jsonb_array_elements(p_receipt->'tables') LOOP
    v_seen := v_seen + 1;
    v_schema := v_item->>'schema';
    v_table_name := v_item->>'table';
    v_physical := v_item->>'physical_name';
    IF v_schema IS NULL OR v_table_name IS NULL OR v_physical IS NULL
       OR (v_item->>'source_generation')::bigint IS DISTINCT FROM v_source_generation
       OR (v_item->>'target_generation')::bigint IS DISTINCT FROM v_target_generation THEN
      RAISE EXCEPTION 'native publication table binding is invalid' USING ERRCODE = 'OO030';
    END IF;
    SELECT table_row.* INTO v_table
    FROM _oo.full_import_table AS table_row
    WHERE table_row.run_id = p_run
      AND table_row.schema_ident = v_schema
      AND table_row.table_ident = v_table_name
    FOR UPDATE;
    IF NOT FOUND OR NOT v_table.exhausted
       OR v_table.physical_name IS DISTINCT FROM v_physical
       OR v_table.applied_rows IS DISTINCT FROM (v_item->>'accepted_rows')::bigint
       OR v_table.canonical_digest IS DISTINCT FROM v_item->>'canonical_digest'
       OR v_table.applied_pos IS DISTINCT FROM (v_item->>'applied_pos')::numeric THEN
      RAISE EXCEPTION 'native publication table proof differs from sealed import' USING ERRCODE = 'OO030';
    END IF;
    SELECT count(*) AS batches,
           coalesce(sum(accepted_rows),0)::bigint AS accepted,
           coalesce(sum(rejected_rows),0)::bigint AS rejected
      INTO v_quality
    FROM _oo.full_import_quality
    WHERE run_id = p_run AND physical_name = v_physical AND kind = 'batch';
    IF v_quality.batches = 0 OR v_quality.accepted <> v_table.applied_rows OR v_quality.rejected <> 0
       OR EXISTS (SELECT 1 FROM _oo.full_import_quality
                  WHERE run_id = p_run AND physical_name = v_physical AND kind = 'reject') THEN
      RAISE EXCEPTION 'native publication quality proof is incomplete' USING ERRCODE = 'OO030';
    END IF;
    PERFORM 1 FROM _oo.table_freshness AS freshness
    WHERE freshness.schema_name = v_schema AND freshness.table_name = v_table_name
      AND freshness.source = v_source AND freshness.source_generation = v_source_generation
      AND freshness.target_generation = v_target_generation
      AND freshness.physical_name = v_physical AND freshness.last_pos = (v_item->>'applied_pos')::numeric
      AND freshness.mode = 'full' AND freshness.state = 'awaiting_publish'
    FOR UPDATE;
    IF NOT FOUND THEN
      RAISE EXCEPTION 'native table freshness is not awaiting publication' USING ERRCODE = 'OO030';
    END IF;
  END LOOP;
  IF v_seen <> v_count OR (SELECT count(DISTINCT (value->>'schema',value->>'table'))
      FROM jsonb_array_elements(p_receipt->'tables')) <> v_count THEN
    RAISE EXCEPTION 'native publication table vector has duplicates' USING ERRCODE = 'OO030';
  END IF;

  SELECT watermark.* INTO v_watermark
  FROM _oo.source_watermark AS watermark
  WHERE watermark.source = v_source FOR UPDATE;
  IF NOT FOUND OR v_watermark.state <> 'lagging'
     OR v_watermark.source_generation <> v_source_generation
     OR v_watermark.target_generation <> v_target_generation
     OR v_watermark.applied_pos <> v_position
     OR v_watermark.position_codec <> v_codec
     OR v_watermark.consistency <> v_consistency
     OR v_watermark.consistency_grade <> v_grade
     OR v_watermark.cross_table_consistent <> v_cross_table THEN
    RAISE EXCEPTION 'native source watermark does not match receipt' USING ERRCODE = 'OO030';
  END IF;

  INSERT INTO _oo.native_publication_receipt (
    run_id, receipt_sha256, source_id, source_generation, target_generation,
    position_codec, applied_pos, consistency, consistency_grade,
    cross_table_consistent, table_vector
  ) VALUES (
    p_run, p_receipt_sha256, v_source, v_source_generation, v_target_generation,
    v_codec, v_position, v_consistency, v_grade, v_cross_table, p_receipt->'tables'
  );
  UPDATE _oo.full_import_run SET state = 'published' WHERE run_id = p_run AND state = 'awaiting_publish';
  IF NOT FOUND THEN
    RAISE EXCEPTION 'native run publication raced' USING ERRCODE = 'OO030';
  END IF;
  UPDATE _oo.table_freshness SET state = 'live', applied_at = clock_timestamp()
  WHERE source = v_source AND source_generation = v_source_generation
    AND target_generation = v_target_generation AND state = 'awaiting_publish'
    AND (schema_name, table_name) IN (
      SELECT value->>'schema', value->>'table' FROM jsonb_array_elements(p_receipt->'tables')
    );
  GET DIAGNOSTICS v_updated = ROW_COUNT;
  IF v_updated <> v_count THEN
    RAISE EXCEPTION 'native publication freshness vector changed' USING ERRCODE = 'OO030';
  END IF;
  UPDATE _oo.source_watermark SET state = 'live', applied_at = clock_timestamp()
  WHERE source = v_source AND state = 'lagging';
  RETURN jsonb_build_object('outcome','published','run_id',p_run,
    'source_id',v_source,'source_generation',v_source_generation,
    'target_generation',v_target_generation,'receipt_sha256',p_receipt_sha256,
    'position_codec',v_codec,'applied_pos',v_position,
    'consistency',v_consistency,'consistency_grade',v_grade,
    'cross_table_consistent',v_cross_table,'tables',v_count);
END
$np$;
REVOKE ALL ON FUNCTION _oo.publish_native_full_import(uuid, text, jsonb) FROM PUBLIC, oo_pipeline_runtime;
DO $np$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_target_admin') THEN
    GRANT EXECUTE ON FUNCTION _oo.publish_native_full_import(uuid, text, jsonb) TO oo_target_admin;
  END IF;
END
$np$;
