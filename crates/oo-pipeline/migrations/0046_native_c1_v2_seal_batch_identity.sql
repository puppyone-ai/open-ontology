-- update_batch keys include source/table/batch, not the physical generation.
-- A distinct task must therefore have its own seal batch identity.
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
  v_batch constant text := 'c1v2.seal:' || p_task::text;
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
     OR p_hook_digest IS NULL OR p_hook_digest !~ '^[0-9a-f]{64}$'
  THEN
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
