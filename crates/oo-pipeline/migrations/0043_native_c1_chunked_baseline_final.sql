-- chunked-typed/v2 C1 baseline: final-load helpers and guards (DESIGN s3.8, s4.1, s6 T8;
-- coordinator decision D4). Run as oo_target_owner after 0042. The 0042 names and
-- grants are kept. Nothing here writes a row.
--
-- Two 0042 functions are replaced here, and no other 0042 object changes:
--   * `native_c1_baseline_v2_assert`: refuses a retired run (N1, below).
--   * `native_c1_baseline_v2_seal`: each receipt carries L_end (see its comment).
-- Retired runs (N1). Once `full_import_retire_run` moves a v2 table's run out of
-- `open`, every v2 write refuses the task. The check sits in
-- `native_c1_baseline_v2_assert`, which chunk, batch, seal and open call after the
-- R2 lock.
--
-- Exact tombstone count (N6). `tomb_count` counts the tombstone table itself, under
-- the same lock as the batch that asks. It is not the statistics estimate.
--
-- Final-load reads (D4). The shadow T_new has no grant to the runtime role. The
-- composed digest is therefore computed by SECURITY DEFINER readers that return
-- one keyset page of text cells at a time. `digest_page` returns at most
-- `p_limit` rows, so a 5 GB shadow is never materialised in the loader.

CREATE OR REPLACE FUNCTION _oo.native_c1_baseline_v2_assert(p_source text, p_task uuid, p_generation bigint, p_epoch integer)
RETURNS _oo.native_c1_baseline_run
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
DECLARE v _oo.native_c1_baseline_run%ROWTYPE;
BEGIN
  SELECT * INTO v FROM _oo.native_c1_baseline_run WHERE source = p_source AND task_id = p_task;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'C1 v2 baseline is not open' USING ERRCODE = 'OO010';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo.native_c1_baseline_v2_format f WHERE f.source = p_source AND f.task_id = p_task) THEN
    RAISE EXCEPTION 'format_mismatch' USING ERRCODE = 'OO030';
  END IF;
  IF EXISTS (SELECT 1 FROM _oo.native_c1_baseline_v2_format f
             WHERE f.source = p_source AND f.task_id = p_task AND f.sealed) THEN
    RAISE EXCEPTION 'C1 v2 baseline is sealed' USING ERRCODE = 'OO010';
  END IF;
  IF EXISTS (SELECT 1 FROM _oo.native_c1_baseline_v2_table t
             JOIN _oo.full_import_run r ON r.run_id = t.run_id
             WHERE t.source = p_source AND t.task_id = p_task AND r.state <> 'open') THEN
    RAISE EXCEPTION 'run_retired' USING ERRCODE = 'OO031';
  END IF;
  IF v.lease_generation <> p_generation OR v.source_epoch <> p_epoch OR v.state <> 'open'
     OR NOT EXISTS (SELECT 1 FROM jobs.tasks t WHERE t.id = p_task AND t.state = 'running'
       AND t.generation = p_generation AND t.lease_until > clock_timestamp())
     OR NOT EXISTS (SELECT 1 FROM _oo.source_instance i WHERE i.source = p_source
       AND i.source_epoch = p_epoch AND i.engine = 'mysql' AND i.measured_id = v.measured_id
       AND i.config_digest = v.config_digest AND i.schema_digest = v.schema_digest
       AND i.lease_id = p_task AND i.lease_generation = p_generation) THEN
    RAISE EXCEPTION 'C1 baseline lease or source epoch is not live' USING ERRCODE = 'OO010';
  END IF;
  RETURN v;
END
$v2$;

-- Seal receipt position (T8 correction of 0042). 0042 wrote each table's seal receipt
-- at applied position 0. The typed commit (0031, 0037, 0038) requires the receipt's
-- applied position to equal L_end, so a published v2 run could never commit. The
-- receipt now carries L_end, as the v1 full-import receipt does (0005, 0013). The
-- codec, lineage and epoch arguments keep their 0042 values. Only that argument changed.
CREATE OR REPLACE FUNCTION _oo.native_c1_baseline_v2_seal(
  p_source text, p_task uuid, p_generation bigint, p_epoch integer,
  p_shard text, p_owner uuid, p_target_generation bigint,
  p_l_end text, p_l_end_pos numeric, p_hook_digest text, p_tables jsonb
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
DECLARE
  v _oo.native_c1_baseline_run%ROWTYPE;
  t _oo.native_c1_baseline_v2_table%ROWTYPE;
  f _oo.native_c1_baseline_v2_format%ROWTYPE;
  e jsonb; r _oo.full_import_run%ROWTYPE;
  v_rows bigint; v_expect bigint; v_digest text; v_receipt text; v_out jsonb := '[]'::jsonb;
  v_batch constant text := 'c1v2.seal';
BEGIN
  PERFORM _oo.native_c1_baseline_lock(p_source, p_task);
  SELECT * INTO f FROM _oo.native_c1_baseline_v2_format WHERE source = p_source AND task_id = p_task;
  IF FOUND AND f.sealed THEN
    SELECT * INTO v FROM _oo.native_c1_baseline_run WHERE source = p_source AND task_id = p_task;
    IF v.l_end IS DISTINCT FROM p_l_end_pos THEN
      RAISE EXCEPTION 'seal_conflict' USING ERRCODE = 'OO030';
    END IF;
    RETURN jsonb_build_object('protocol', 'oo.pipeline.native-c1-baseline-stage/v2', 'outcome', 'replay',
      'l_end', _oo.native_pos_text(p_l_end_pos));
  END IF;
  v := _oo.native_c1_baseline_v2_assert(p_source, p_task, p_generation, p_epoch);
  PERFORM _oo.full_import_assert_bound(p_shard, p_owner, p_target_generation);
  IF p_l_end IS NULL OR p_l_end !~ ('^' || substring(v.measured_id from 7) || ':[1-9][0-9]*-[1-9][0-9]*$')
     OR split_part(p_l_end, ':', 2) !~ ('^[1-9][0-9]*-' || _oo.native_pos_text(p_l_end_pos) || '$')
     OR p_l_end_pos <> v.applied_pos OR p_l_end_pos < v.l0_pos
  THEN
    RAISE EXCEPTION 'final barrier is not applied through L_end' USING ERRCODE = 'OO010';
  END IF;
  IF jsonb_typeof(p_tables) IS DISTINCT FROM 'array'
     OR jsonb_array_length(p_tables) <> (SELECT count(*) FROM _oo.native_c1_baseline_v2_table
                                         WHERE source = p_source AND task_id = p_task)
  THEN
    RAISE EXCEPTION 'seal_tables' USING ERRCODE = 'OO030';
  END IF;
  IF p_hook_digest IS NULL OR p_hook_digest !~ '^[0-9a-f]{64}$' THEN
    RAISE EXCEPTION 'seal_tables' USING ERRCODE = 'OO030';
  END IF;

  FOR t IN SELECT * FROM _oo.native_c1_baseline_v2_table
           WHERE source = p_source AND task_id = p_task ORDER BY schema_ident, table_ident LOOP
    IF NOT EXISTS (SELECT 1 FROM _oo.native_c1_baseline_chunk c
                   WHERE c.source = p_source AND c.task_id = p_task
                     AND c.schema_ident = t.schema_ident AND c.table_ident = t.table_ident AND c.terminal) THEN
      RAISE EXCEPTION 'snapshot_incomplete' USING ERRCODE = 'OO010';
    END IF;
    SELECT x.value INTO e FROM jsonb_array_elements(p_tables) x
     WHERE x.value ->> 'schema' = t.schema_ident AND x.value ->> 'table' = t.table_ident;
    IF NOT FOUND OR jsonb_typeof(e -> 'row_count') IS DISTINCT FROM 'number'
       OR (e ->> 'payload_digest') IS NULL OR (e ->> 'payload_digest') !~ '^[0-9a-f]{64}$'
    THEN
      RAISE EXCEPTION 'seal_tables' USING ERRCODE = 'OO030';
    END IF;
    v_expect := (e ->> 'row_count')::bigint;
    v_digest := e ->> 'payload_digest';
    EXECUTE format('SELECT count(*) FROM _oo.%I', t.physical_name) INTO v_rows;
    IF v_rows <> v_expect THEN
      RAISE EXCEPTION 'composed_row_count' USING ERRCODE = 'OO030';
    END IF;

    SELECT * INTO r FROM _oo.full_import_run WHERE run_id = t.run_id FOR UPDATE;
    EXECUTE format('DROP TABLE _oo.%I', t.tomb_name);
    EXECUTE format('DROP TABLE _oo.%I', t.stage_name);
    UPDATE _oo.full_import_table SET applied_rows = v_rows, applied_pos = p_l_end_pos
      WHERE run_id = t.run_id AND table_index = 0;
    v_receipt := _oo.commit_apply_receipt(p_shard, p_owner, p_target_generation, p_source, t.physical_name,
      v_batch, r.config_digest, r.type_digest, p_hook_digest, r.schema_digest, v_digest,
      'snapshot-zero/v1', 'full-import.snapshot', 0, p_l_end_pos, false, false, true, NULL, NULL, ''::bytea);
    -- The sealed-import contract (oo-core) requires exactly one stored `batch` quality row per
    -- sealed table. A chunked baseline loads by COPY and never writes per-row batches, so the
    -- seal records the single accepted batch here, with the composed table digest.
    INSERT INTO _oo.full_import_quality (
      run_id, physical_name, batch_id, kind, input_digest, accepted_rows, rejected_rows, payload_digest
    ) VALUES (t.run_id, t.physical_name, v_batch, 'batch', r.input_digest, v_rows, 0, v_digest);
    PERFORM _oo.full_import_note_table(p_shard, p_owner, p_target_generation, t.run_id, 0, v_rows, v_digest, p_source);
    v_out := v_out || jsonb_build_array(jsonb_build_object('schema', t.schema_ident, 'table', t.table_ident,
      'run_id', t.run_id, 'physical', t.physical_name, 'row_count', v_rows, 'payload_digest', v_digest,
      'receipt_batch_id', v_batch, 'receipt', v_receipt));
  END LOOP;

  UPDATE _oo.native_c1_baseline_v2_format SET sealed = true WHERE source = p_source AND task_id = p_task;
  UPDATE _oo.native_c1_baseline_run SET l_end = p_l_end_pos, state = 'awaiting_publish', sealed_at = clock_timestamp()
    WHERE source = p_source AND task_id = p_task;

  RETURN jsonb_build_object('protocol', 'oo.pipeline.native-c1-baseline-stage/v2', 'outcome', 'sealed',
    'source', p_source, 'task_id', p_task, 'source_epoch', p_epoch, 'l0', v.l0,
    'l_end', p_l_end, 'applied_pos', _oo.native_pos_text(p_l_end_pos), 'tables', v_out);
END
$v2$;

-- Exact tombstone count of one registered table of a live v2 run.
CREATE FUNCTION _oo.native_c1_baseline_v2_tomb_count(
  p_source text, p_task uuid, p_generation bigint, p_epoch integer, p_schema text, p_table text
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
DECLARE t _oo.native_c1_baseline_v2_table%ROWTYPE; n bigint;
BEGIN
  PERFORM _oo.native_c1_baseline_v2_assert(p_source, p_task, p_generation, p_epoch);
  t := _oo.native_c1_baseline_v2_reg(p_source, p_task, p_schema, p_table);
  EXECUTE format('SELECT count(*) FROM _oo.%I', t.tomb_name) INTO n;
  RETURN n;
END
$v2$;

-- Registered tables of a live v2 run, with their key and columns. The order is the
-- order of `schema.table` text, which is the order the typed commit checks.
CREATE FUNCTION _oo.native_c1_baseline_v2_plan(p_source text, p_task uuid, p_generation bigint, p_epoch integer)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
DECLARE t _oo.native_c1_baseline_v2_table%ROWTYPE; out jsonb := '[]'::jsonb;
BEGIN
  PERFORM _oo.native_c1_baseline_v2_assert(p_source, p_task, p_generation, p_epoch);
  FOR t IN SELECT * FROM _oo.native_c1_baseline_v2_table
           WHERE source = p_source AND task_id = p_task
           ORDER BY (schema_ident || '.' || table_ident) LOOP
    out := out || jsonb_build_array(jsonb_build_object(
      'schema', t.schema_ident, 'table', t.table_ident, 'physical', t.physical_name,
      'run_id', t.run_id, 'key_column', t.key_column, 'key_type', t.key_type,
      'columns', to_jsonb(t.physical_columns)));
  END LOOP;
  RETURN out;
END
$v2$;

-- One keyset page of the shadow: each row is the physical columns as text, in
-- column order. NULL stays NULL. The first page passes p_after NULL. Later pages
-- pass the last key text. The key bound is cast to the key type, so the primary
-- key index serves every page.
CREATE FUNCTION _oo.native_c1_baseline_v2_digest_page(
  p_source text, p_task uuid, p_generation bigint, p_epoch integer,
  p_schema text, p_table text, p_after text, p_limit integer
) RETURNS SETOF text[] LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
DECLARE t _oo.native_c1_baseline_v2_table%ROWTYPE; v_cols text; v_sql text;
BEGIN
  PERFORM _oo.native_c1_baseline_v2_assert(p_source, p_task, p_generation, p_epoch);
  t := _oo.native_c1_baseline_v2_reg(p_source, p_task, p_schema, p_table);
  IF p_limit IS NULL OR p_limit < 1 OR p_limit > 100000 THEN
    RAISE EXCEPTION 'invalid_page' USING ERRCODE = 'OO030';
  END IF;
  SELECT string_agg(format('%I::text', c), ', ' ORDER BY ord) INTO v_cols
    FROM unnest(t.physical_columns) WITH ORDINALITY AS u(c, ord);
  IF p_after IS NULL THEN
    v_sql := format('SELECT ARRAY[%s]::text[] FROM _oo.%I ORDER BY %I LIMIT $1',
      v_cols, t.physical_name, t.key_column);
    RETURN QUERY EXECUTE v_sql USING p_limit;
  ELSE
    v_sql := format('SELECT ARRAY[%s]::text[] FROM _oo.%I WHERE %I > ($1::text)::%s ORDER BY %I LIMIT $2',
      v_cols, t.physical_name, t.key_column, t.key_type, t.key_column);
    RETURN QUERY EXECUTE v_sql USING p_after, p_limit;
  END IF;
END
$v2$;

-- Index and statistics step of the final load. T_new carries its primary key from
-- open_target, and no secondary index is defined for it. This runs ANALYZE on each
-- shadow. Only the owner may analyse, so the step is a SECURITY DEFINER function.
CREATE FUNCTION _oo.native_c1_baseline_v2_analyze(p_source text, p_task uuid, p_generation bigint, p_epoch integer)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, _oo
AS $v2$
DECLARE t _oo.native_c1_baseline_v2_table%ROWTYPE;
BEGIN
  PERFORM _oo.native_c1_baseline_v2_assert(p_source, p_task, p_generation, p_epoch);
  FOR t IN SELECT * FROM _oo.native_c1_baseline_v2_table
           WHERE source = p_source AND task_id = p_task ORDER BY schema_ident, table_ident LOOP
    EXECUTE format('ANALYZE _oo.%I', t.physical_name);
  END LOOP;
END
$v2$;

REVOKE ALL ON FUNCTION _oo.native_c1_baseline_v2_tomb_count(text, uuid, bigint, integer, text, text),
  _oo.native_c1_baseline_v2_plan(text, uuid, bigint, integer),
  _oo.native_c1_baseline_v2_digest_page(text, uuid, bigint, integer, text, text, text, integer),
  _oo.native_c1_baseline_v2_analyze(text, uuid, bigint, integer)
  FROM PUBLIC;

GRANT EXECUTE ON FUNCTION _oo.native_c1_baseline_v2_tomb_count(text, uuid, bigint, integer, text, text),
  _oo.native_c1_baseline_v2_plan(text, uuid, bigint, integer),
  _oo.native_c1_baseline_v2_digest_page(text, uuid, bigint, integer, text, text, text, integer),
  _oo.native_c1_baseline_v2_analyze(text, uuid, bigint, integer)
  TO oo_pipeline_runtime;
