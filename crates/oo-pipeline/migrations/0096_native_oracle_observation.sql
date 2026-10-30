-- Additive, operator-admitted observation only. No runtime/Agent grants.
CREATE TABLE _oo.c5_oracle_observer_scope (
  observer oid NOT NULL,
  run_id uuid NOT NULL,
  binding jsonb NOT NULL CHECK (jsonb_typeof(binding) = 'object'),
  PRIMARY KEY (observer, run_id)
);
REVOKE ALL ON _oo.c5_oracle_observer_scope FROM PUBLIC, oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_oracle_observation_check(p_binding jsonb) RETURNS void
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, _oo, pg_temp AS $$
DECLARE
  r _oo.full_import_run;
  t _oo.full_import_table;
  proof jsonb := p_binding->'oracle_c5_binding';
  c jsonb;
  w _oo.source_watermark;
BEGIN
  IF current_setting('transaction_read_only') <> 'on'
    OR current_setting('transaction_isolation') NOT IN ('repeatable read', 'serializable')
    OR NOT EXISTS (
      SELECT FROM _oo.c5_oracle_observer_scope s JOIN pg_catalog.pg_roles u ON u.oid=s.observer
      WHERE u.rolname=session_user AND NOT u.rolsuper AND NOT u.rolcreaterole
        AND NOT u.rolcreatedb AND NOT u.rolreplication AND NOT u.rolbypassrls
        AND NOT EXISTS (SELECT FROM pg_catalog.pg_auth_members m WHERE m.member=u.oid)
        AND s.run_id=(p_binding->>'run_id')::uuid AND s.binding=p_binding)
  THEN RAISE EXCEPTION 'Oracle observation not admitted' USING ERRCODE='42501'; END IF;
  SELECT * INTO STRICT r FROM _oo.full_import_run WHERE run_id=(p_binding->>'run_id')::uuid;
  SELECT * INTO STRICT t FROM _oo.full_import_table WHERE run_id=r.run_id;
  -- Reuse installed 0094 validation; the owner token never leaves this function.
  c := _oo.native_oracle_c5_catalog(r.run_id, p_binding->>'source_id', r.owner_token,
    p_binding->>'config_digest', (p_binding->>'source_generation')::bigint,
    (p_binding->>'target_generation')::bigint, proof->'scope'->>'schema_fingerprint',
    p_binding->>'type_digest', p_binding->>'input_digest', proof->'scope');
  IF c->'binding' IS DISTINCT FROM proof
    OR r.state <> 'published'
    OR t.physical_name IS DISTINCT FROM p_binding->>'stored_physical'
    OR t.applied_rows IS DISTINCT FROM (p_binding->>'accepted_rows')::bigint
    OR c->'catalog'->'tables'->0->>'stored_namespace' IS DISTINCT FROM p_binding->>'stored_namespace'
    OR jsonb_array_length(p_binding->'columns') IS DISTINCT FROM
       jsonb_array_length(c->'catalog'->'tables'->0->'columns')
    OR EXISTS (
      SELECT FROM jsonb_array_elements(p_binding->'columns') b
      WHERE NOT EXISTS (
        SELECT FROM jsonb_array_elements(c->'catalog'->'tables'->0->'columns') a
        WHERE a->>'physical_name'=b->>'stored_physical'
          AND a->>'logical_column'=_oo.c5_oracle_ident(b->>'logical_name')
          AND (a->>'atttypid')::int=(b->>'oid')::int
          AND (a->>'atttypmod')::int=(b->>'typmod')::int))
  THEN RAISE EXCEPTION 'Oracle observation binding differs' USING ERRCODE='OO030'; END IF;
  SELECT * INTO STRICT w FROM _oo.source_watermark WHERE source=r.source_id;
  IF w.state <> 'live' OR w.source_generation <> r.source_generation
    OR w.target_generation <> r.target_generation OR w.position_codec <> 'snapshot-zero/v1'
    OR w.position_lineage <> 'oracle.snapshot' OR w.applied_pos <> 0
    OR w.consistency <> 'per_table' OR w.consistency_grade <> 'S0' OR w.cross_table_consistent
    OR NOT EXISTS (SELECT FROM _oo.table_quality q
      WHERE q.schema_name=t.schema_ident AND q.table_name=t.table_ident
        AND q.rejects_open=0 AND NOT q.partial_txn_in_progress
        AND q.position_state='ok' AND q.budget_state='ok'
        AND q.capture_method='c5_diff' AND q.delete_detection='reconcile')
  THEN RAISE EXCEPTION 'Oracle observation cut differs' USING ERRCODE='OO030'; END IF;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_oracle_observation_check(jsonb) FROM PUBLIC, oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_oracle_profile(p_binding jsonb, p_columns jsonb) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, _oo, pg_temp AS $$
DECLARE
  col jsonb;
  value_sql text;
  objects text[] := ARRAY[]::text[];
  result jsonb;
BEGIN
  PERFORM _oo.c5_oracle_observation_check(p_binding);
  IF jsonb_typeof(p_columns) IS DISTINCT FROM 'array'
    OR jsonb_array_length(p_columns) NOT BETWEEN 1 AND 1024
    OR jsonb_array_length(p_columns) <> jsonb_array_length(p_binding->'columns')
    OR (SELECT count(DISTINCT c->>'physical_name') FROM jsonb_array_elements(p_columns) c)
       <> jsonb_array_length(p_columns)
    OR EXISTS (
      SELECT FROM jsonb_array_elements(p_columns) c
      WHERE NOT EXISTS (
        SELECT FROM jsonb_array_elements(p_binding->'columns') b
        WHERE c->>'physical_name'=b->>'stored_physical'
          AND c->>'name' IN (b->>'logical_name', _oo.c5_oracle_ident(b->>'logical_name'))))
  THEN RAISE EXCEPTION 'Oracle observation columns differ' USING ERRCODE='22023'; END IF;
  FOR col IN SELECT * FROM jsonb_array_elements(p_columns) LOOP
    value_sql := format('%I::text', col->>'physical_name');
    objects := array_append(objects, format(
      'jsonb_build_object(''name'',%L,''non_null'',count(%s),''distinct'',count(distinct %s),'
      '''min_length'',min(length(%s)),''max_length'',max(length(%s)),'
      '''blank_rows'',count(*) filter(where btrim(%s)=''''),'
      '''digits_rows'',count(*) filter(where %s ~ ''^[0-9]+$''),'
      '''leading_zero_rows'',count(*) filter(where %s ~ ''^0[0-9]+$''))',
      col->>'name', value_sql, value_sql, value_sql, value_sql, value_sql, value_sql, value_sql));
  END LOOP;
  -- A SQL array avoids jsonb_build_array's argument limit on wide tables.
  EXECUTE format(
    'select jsonb_build_object(''rows'',count(*),''snapshot'',pg_current_snapshot()::text,'
    '''observed_at'',statement_timestamp()::text,''columns'',to_jsonb(ARRAY[%s])) from _oo.%I',
    array_to_string(objects, ','), p_binding->>'stored_physical') INTO result;
  IF (result->>'rows')::bigint IS DISTINCT FROM (p_binding->>'accepted_rows')::bigint THEN
    RAISE EXCEPTION 'Oracle observation effective rows differ' USING ERRCODE='OO030';
  END IF;
  RETURN result;
END $$;
REVOKE ALL ON FUNCTION _oo.c5_oracle_profile(jsonb,jsonb) FROM PUBLIC, oo_pipeline_runtime;

CREATE FUNCTION _oo.c5_oracle_observation_state(p_binding jsonb) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, _oo, pg_temp AS $$
DECLARE
  r _oo.full_import_run;
  t _oo.full_import_table;
BEGIN
  PERFORM _oo.c5_oracle_observation_check(p_binding);
  SELECT * INTO STRICT r FROM _oo.full_import_run WHERE run_id=(p_binding->>'run_id')::uuid;
  SELECT * INTO STRICT t FROM _oo.full_import_table WHERE run_id=r.run_id;
  RETURN jsonb_build_object('database',current_database(),'principal',session_user,
    'read_only',current_setting('transaction_read_only'),'metadata',jsonb_build_object(
    'source_watermark',(SELECT jsonb_agg(to_jsonb(x)) FROM (
      SELECT source,consistency,consistency_grade,applied_pos,source_committed_at,applied_at,state,
        cross_table_consistent,source_generation,target_generation,position_codec,position_lineage,source_epoch
      FROM _oo.source_watermark WHERE source=r.source_id) x),
    'full_import_run',jsonb_build_array(jsonb_build_object(
      'run_id',r.run_id,'source_id',r.source_id,'source_generation',r.source_generation,
      'target_generation',r.target_generation,'config_digest',r.config_digest,'type_digest',r.type_digest,
      'input_digest',r.input_digest,'schema_digest',r.schema_digest,'state',r.state,'sealed_at',r.sealed_at)),
    'native_publication_receipt',(SELECT jsonb_agg(to_jsonb(x)) FROM (
      SELECT run_id,receipt_sha256,source_id,source_generation,target_generation,position_codec,
        applied_pos,consistency,consistency_grade,cross_table_consistent,table_vector,published_at,publication_xid
      FROM _oo.native_publication_receipt WHERE run_id=r.run_id) x),
    'table_quality',(SELECT jsonb_agg(to_jsonb(x)) FROM (
      SELECT schema_name,table_name,rejects_open,reconcile_status,partial_txn_in_progress,
        capture_method,delete_detection,position_state,budget_state
      FROM _oo.table_quality WHERE schema_name=t.schema_ident AND table_name=t.table_ident) x),
    'c5_oracle_binding',(SELECT jsonb_agg(to_jsonb(x)) FROM (
      SELECT source,schema_name,table_name,source_schema,source_table,identity,baseline_run,
        snapshot_scn,manifest_sha256,key_ident
      FROM _oo.c5_oracle_binding WHERE source=r.source_id
        AND schema_name=t.schema_ident AND table_name=t.table_ident) x),
    'c5_block_receipt',(SELECT jsonb_agg(to_jsonb(x)) FROM (
      SELECT source,schema_name,table_name,chunk_id,source_generation,scope,run_id,batch_id,
        payload_digest,target_generation
      FROM _oo.c5_block_receipt WHERE source=r.source_id AND run_id=r.run_id) x)));
END $$;
REVOKE ALL ON FUNCTION _oo.c5_oracle_observation_state(jsonb) FROM PUBLIC, oo_pipeline_runtime;
